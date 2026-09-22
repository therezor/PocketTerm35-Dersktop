//! The scrolling, filterable list every pt35 screen is made of.
//!
//! Pure state: no Wayland, no drawing. The menu, the window switcher and the
//! Wi-Fi picker all drive this, so its behaviour is tested once here.

use crate::keys::{navigate, Key, Navigation};

#[derive(Debug, Clone)]
pub struct ListState {
    /// Every item, in definition order.
    items: Vec<String>,
    /// Indices into `items` that survive the current filter.
    visible: Vec<usize>,
    filter: String,
    selected: usize,
    offset: usize,
    rows: usize,
}

/// What the caller should do after feeding a key in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Selection or filter changed; repaint.
    Redraw,
    /// Activate the item at this index into the original item list.
    Activate(usize),
    /// Leave this screen (Escape).
    Cancel,
    /// Go up one level (Left / Backspace on an empty filter).
    Back,
    /// Nothing happened; do not repaint.
    Nothing,
}

impl ListState {
    pub fn new(items: Vec<String>, rows: usize) -> Self {
        let visible = (0..items.len()).collect();
        Self {
            items,
            visible,
            filter: String::new(),
            selected: 0,
            offset: 0,
            rows: rows.max(1),
        }
    }

    pub fn filter(&self) -> &str {
        &self.filter
    }

    pub fn selected(&self) -> Option<usize> {
        self.visible.get(self.selected).copied()
    }

    pub fn len(&self) -> usize {
        self.visible.len()
    }

    pub fn is_empty(&self) -> bool {
        self.visible.is_empty()
    }

    /// The window of items to draw, as (index into `items`, label).
    pub fn window(&self) -> Vec<(usize, &str)> {
        self.visible
            .iter()
            .skip(self.offset)
            .take(self.rows)
            .map(|&i| (i, self.items[i].as_str()))
            .collect()
    }

    /// Row within the drawn window that is highlighted.
    pub fn cursor_row(&self) -> usize {
        self.selected.saturating_sub(self.offset)
    }

    pub fn handle(&mut self, key: &Key) -> Outcome {
        match navigate(key, !self.filter.is_empty()) {
            Navigation::Up => self.move_by(-1),
            Navigation::Down => self.move_by(1),
            Navigation::PageUp => self.move_by(-(self.rows as isize)),
            Navigation::PageDown => self.move_by(self.rows as isize),
            Navigation::First => {
                self.selected = 0;
                self.scroll_into_view();
                Outcome::Redraw
            }
            Navigation::Last => {
                self.selected = self.visible.len().saturating_sub(1);
                self.scroll_into_view();
                Outcome::Redraw
            }
            Navigation::Activate => match self.selected() {
                Some(index) => Outcome::Activate(index),
                None => Outcome::Nothing,
            },
            Navigation::Select(row) => {
                // 1-9 pick the nth *visible* row, which is what the user sees.
                match self.visible.get(self.offset + row) {
                    Some(&index) => Outcome::Activate(index),
                    None => Outcome::Nothing,
                }
            }
            Navigation::Cancel => Outcome::Cancel,
            Navigation::Back => Outcome::Back,
            Navigation::Filter(ch) => {
                self.filter.push(ch);
                self.refilter();
                Outcome::Redraw
            }
            Navigation::FilterBackspace => {
                self.filter.pop();
                self.refilter();
                Outcome::Redraw
            }
            Navigation::Ignored => Outcome::Nothing,
        }
    }

    fn move_by(&mut self, delta: isize) -> Outcome {
        if self.visible.is_empty() {
            return Outcome::Nothing;
        }
        let last = self.visible.len() as isize - 1;
        // Wrapping at the ends: on a D-pad, holding one direction to reach the
        // other end of a nine-item menu is worse than wrapping.
        let next = match self.selected as isize + delta {
            n if n < 0 && delta == -1 => last,
            n if n > last && delta == 1 => 0,
            n => n.clamp(0, last),
        };
        self.selected = next as usize;
        self.scroll_into_view();
        Outcome::Redraw
    }

    fn scroll_into_view(&mut self) {
        if self.selected < self.offset {
            self.offset = self.selected;
        } else if self.selected >= self.offset + self.rows {
            self.offset = self.selected + 1 - self.rows;
        }
    }

    fn refilter(&mut self) {
        let needle = self.filter.to_lowercase();
        let previous = self.selected();
        self.visible = self
            .items
            .iter()
            .enumerate()
            .filter(|(_, label)| label.to_lowercase().contains(&needle))
            .map(|(i, _)| i)
            .collect();
        // Keep the highlight on the same item when it survives the filter.
        self.selected = previous
            .and_then(|index| self.visible.iter().position(|&i| i == index))
            .unwrap_or(0);
        self.offset = 0;
        self.scroll_into_view();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::sym;

    fn list() -> ListState {
        ListState::new(
            ["Terminal", "Files", "Editor", "Monitor", "Music", "Browser"]
                .map(String::from)
                .to_vec(),
            3,
        )
    }

    #[test]
    fn moves_and_wraps() {
        let mut list = list();
        assert_eq!(list.selected(), Some(0));
        list.handle(&Key::new(sym::DOWN));
        assert_eq!(list.selected(), Some(1));
        list.handle(&Key::new(sym::UP));
        list.handle(&Key::new(sym::UP));
        assert_eq!(list.selected(), Some(5), "up from the top wraps to the end");
        list.handle(&Key::new(sym::DOWN));
        assert_eq!(
            list.selected(),
            Some(0),
            "down from the end wraps to the top"
        );
    }

    #[test]
    fn scrolls_to_keep_the_selection_visible() {
        let mut list = list();
        for _ in 0..3 {
            list.handle(&Key::new(sym::DOWN));
        }
        let window: Vec<&str> = list.window().iter().map(|(_, l)| *l).collect();
        assert_eq!(window, ["Files", "Editor", "Monitor"]);
        assert_eq!(list.cursor_row(), 2);
    }

    #[test]
    fn filters_case_insensitively_and_keeps_the_highlight() {
        let mut list = list();
        list.handle(&Key::new(sym::DOWN)); // Files
        list.handle(&Key::with_text(0x0069, 'i'));
        assert_eq!(list.filter(), "i");
        // 'i' matches Terminal, Files, Editor, Monitor and Music — not Browser.
        assert_eq!(list.len(), 5);
        let window: Vec<&str> = list.window().iter().map(|(_, l)| *l).collect();
        assert_eq!(window, ["Terminal", "Files", "Editor"]);
        assert_eq!(
            list.selected(),
            Some(1),
            "Files stays selected through the filter"
        );
    }

    #[test]
    fn digit_shortcut_activates_the_visible_row() {
        let mut list = list();
        assert_eq!(
            list.handle(&Key::with_text(0x0032, '2')),
            Outcome::Activate(1)
        );
        // After scrolling, "2" means the second row on screen, not item 2.
        for _ in 0..4 {
            list.handle(&Key::new(sym::DOWN));
        }
        assert_eq!(
            list.handle(&Key::with_text(0x0031, '1')),
            Outcome::Activate(2)
        );
    }

    #[test]
    fn backspace_clears_the_filter_then_goes_back() {
        let mut list = list();
        list.handle(&Key::with_text(0x0065, 'e'));
        assert_eq!(list.handle(&Key::new(sym::BACKSPACE)), Outcome::Redraw);
        assert_eq!(list.filter(), "");
        assert_eq!(list.handle(&Key::new(sym::BACKSPACE)), Outcome::Back);
    }

    #[test]
    fn an_empty_filter_result_cannot_be_activated() {
        let mut list = list();
        for ch in "zzz".chars() {
            list.handle(&Key::with_text(ch as u32, ch));
        }
        assert!(list.is_empty());
        assert_eq!(list.handle(&Key::new(sym::RETURN)), Outcome::Nothing);
    }

    #[test]
    fn escape_cancels() {
        assert_eq!(list().handle(&Key::new(sym::ESCAPE)), Outcome::Cancel);
    }
}
