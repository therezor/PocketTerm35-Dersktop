//! The scrolling, filterable list every pt35 screen is made of.
//!
//! Pure state: no Wayland, no drawing. The menu, the window switcher and the
//! Wi-Fi picker all drive this, so its behaviour is tested once here.

use crate::keys::{navigate, Key, Mode, Navigation};

#[derive(Debug, Clone)]
pub struct ListState {
    /// Every item, in definition order.
    items: Vec<String>,
    /// Extra words a search also matches, per item: an app's id or command.
    keywords: Vec<String>,
    /// Indices into `items` that survive the current filter.
    visible: Vec<usize>,
    filter: String,
    selected: usize,
    offset: usize,
    rows: usize,
    columns: usize,
    mode: Mode,
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
    /// Go up one level (B / Backspace in nav mode).
    Back,
    /// D-pad left on a single-column list. A settings row turns it into a
    /// decrement; every other screen ignores it. A grid consumes it as movement
    /// and never emits this.
    Left,
    /// D-pad right on a single-column list. The mirror of [`Outcome::Left`].
    Right,
    /// The screen's own secondary action (Y).
    Secondary,
    /// Nothing happened; do not repaint.
    Nothing,
}

impl ListState {
    pub fn new(items: Vec<String>, rows: usize) -> Self {
        let visible = (0..items.len()).collect();
        Self {
            keywords: vec![String::new(); items.len()],
            items,
            visible,
            filter: String::new(),
            selected: 0,
            offset: 0,
            rows: rows.max(1),
            columns: 1,
            mode: Mode::Nav,
        }
    }

    /// Words a search matches besides each label, one string per item.
    pub fn with_keywords(mut self, keywords: Vec<String>) -> Self {
        if keywords.len() == self.items.len() {
            self.keywords = keywords;
        }
        self
    }

    /// Lay the items out as a grid this many tiles wide.
    pub fn with_columns(mut self, columns: usize) -> Self {
        self.columns = columns.max(1);
        self
    }

    pub fn columns(&self) -> usize {
        self.columns
    }

    fn page(&self) -> usize {
        self.rows * self.columns
    }

    /// Move the cursor to a row of the drawn window. Used by touch.
    pub fn focus_window(&mut self, index: usize) -> bool {
        let target = self.offset + index;
        if target >= self.visible.len() {
            return false;
        }
        self.selected = target;
        true
    }

    /// Put the cursor and the scroll back where another list had them, clamped
    /// to this one. For rows rebuilt in place, where a jump to the top would
    /// lose your place on every toggle.
    pub fn keep_position(&mut self, from: &ListState) {
        let last = self.visible.len().saturating_sub(1);
        self.selected = from.selected.min(last);
        self.offset = from.offset.min(self.selected);
    }

    /// Put the cursor on this item, by its index in the full list, scrolling
    /// it into view. Nothing happens if a filter hides it.
    pub fn select_item(&mut self, item: usize) {
        if let Some(at) = self.visible.iter().position(|&i| i == item) {
            self.selected = at;
            self.scroll_into_view();
        }
    }

    /// Index of the cursor inside the drawn window.
    pub fn cursor_index(&self) -> usize {
        self.selected.saturating_sub(self.offset)
    }

    /// Nav or filter — the hint bar shows a different legend for each.
    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// How many rows fit on screen; the caller sets this from the theme.
    pub fn rows(&self) -> usize {
        self.rows
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
            .take(self.page())
            .map(|&i| (i, self.items[i].as_str()))
            .collect()
    }

    /// Row within the drawn window that is highlighted.
    pub fn cursor_row(&self) -> usize {
        self.selected.saturating_sub(self.offset)
    }

    /// First visible row, counted in filtered positions.
    pub fn offset(&self) -> usize {
        self.offset
    }

    /// Scroll so the drawn window sits at `fraction` of the way down, the
    /// scrollbar's measure, and keep the cursor inside it. True if it moved.
    pub fn scroll_to(&mut self, fraction: f32) -> bool {
        let page = self.page();
        let total = self.len();
        if total <= page {
            return false;
        }
        let span = total - page;
        let offset = (fraction.clamp(0.0, 1.0) * span as f32).round() as usize;
        // A grid scrolls by whole rows of tiles.
        let offset = offset - offset % self.columns;
        let before = (self.offset, self.selected);
        self.offset = offset;
        self.selected = self.selected.clamp(offset, offset + page - 1);
        before != (self.offset, self.selected)
    }

    /// Where the drawn window sits, 0.0 at the top and 1.0 at the bottom.
    /// `None` when everything fits and there is nothing to scroll.
    ///
    /// Measured from `offset`, not from `selected`: `selected` indexes the
    /// unfiltered item list and means nothing once a search narrows it.
    pub fn scroll_progress(&self) -> Option<f32> {
        let total = self.len();
        let rows = self.page();
        if total <= rows {
            return None;
        }
        Some(self.offset as f32 / (total - rows) as f32)
    }

    pub fn handle(&mut self, key: &Key) -> Outcome {
        match navigate(key, self.mode) {
            Navigation::Up => self.move_by(-(self.columns as isize)),
            Navigation::Down => self.move_by(self.columns as isize),
            Navigation::Left => {
                if self.columns > 1 {
                    self.move_by(-1)
                } else {
                    Outcome::Left
                }
            }
            Navigation::Right => {
                if self.columns > 1 {
                    self.move_by(1)
                } else {
                    Outcome::Right
                }
            }
            Navigation::PageUp => self.move_by(-(self.page() as isize)),
            Navigation::PageDown => self.move_by(self.page() as isize),
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
            Navigation::Cancel => {
                // Select/Escape leaves filtering before it leaves the screen.
                if self.mode == Mode::Filter {
                    self.mode = Mode::Nav;
                    self.filter.clear();
                    self.refilter();
                    return Outcome::Redraw;
                }
                Outcome::Cancel
            }
            Navigation::Back => Outcome::Back,
            Navigation::StartFilter => {
                self.mode = Mode::Filter;
                Outcome::Redraw
            }
            Navigation::Secondary => Outcome::Secondary,
            Navigation::Filter(ch) => {
                self.filter.push(ch);
                self.refilter();
                Outcome::Redraw
            }
            Navigation::FilterBackspace => {
                // Backspacing past the start of the filter returns to nav mode,
                // where B and the other letters are buttons again.
                if self.filter.pop().is_none() {
                    self.mode = Mode::Nav;
                }
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
        // A list wraps at the ends. A grid clamps: wrapping a 2D cursor sends it
        // somewhere the eye did not follow.
        let wraps = self.columns == 1 && delta.abs() == 1;
        let next = match self.selected as isize + delta {
            n if n < 0 && wraps && delta == -1 => last,
            n if n > last && wraps && delta == 1 => 0,
            n => n.clamp(0, last),
        };
        self.selected = next as usize;
        self.scroll_into_view();
        Outcome::Redraw
    }

    fn scroll_into_view(&mut self) {
        let page = self.page();
        if self.selected < self.offset {
            // Keep a grid's scroll on a row boundary, or tiles jump by a third.
            self.offset = self.selected - self.selected % self.columns;
        } else if self.selected >= self.offset + page {
            let last_row_start = self.selected - self.selected % self.columns;
            self.offset = last_row_start + self.columns - page;
        }
    }

    fn refilter(&mut self) {
        let needle = self.filter.to_lowercase();
        let mut scored: Vec<(u8, usize)> = self
            .items
            .iter()
            .zip(&self.keywords)
            .enumerate()
            .filter_map(|(i, (label, keywords))| {
                let label = score(&label.to_lowercase(), &needle);
                let keywords = score(&keywords.to_lowercase(), &needle).map(|s| s + 1);
                label.into_iter().chain(keywords).min().map(|s| (s, i))
            })
            .collect();
        // Best match first, ties in the list's own order: Enter opens what
        // the search most likely meant.
        scored.sort();
        self.visible = scored.into_iter().map(|(_, i)| i).collect();
        self.selected = 0;
        self.offset = 0;
        self.scroll_into_view();
    }
}

/// How well `needle` matches `hay`, lower is better, `None` for no match: the
/// start of it, the start of a word, anywhere, then its letters in order.
pub fn score(hay: &str, needle: &str) -> Option<u8> {
    if needle.is_empty() {
        return Some(0);
    }
    if hay.starts_with(needle) {
        return Some(0);
    }
    let word_start = hay
        .match_indices(needle)
        .any(|(at, _)| hay[..at].ends_with([' ', '-', '_', '.', ':', '/']));
    if word_start {
        return Some(2);
    }
    if hay.contains(needle) {
        return Some(4);
    }
    let mut rest = hay.chars();
    needle.chars().all(|c| rest.any(|h| h == c)).then_some(6)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_scrollbar_moves_the_window_and_the_cursor_stays_in_it() {
        let items = (0..20).map(|i| i.to_string()).collect();
        let mut list = ListState::new(items, 5);
        assert!(list.scroll_to(1.0));
        assert_eq!(list.offset(), 15);
        assert_eq!(list.cursor_index(), 0, "dragged along to the top row shown");
        assert!(list.scroll_to(0.5));
        assert_eq!(list.offset(), 8);
        assert!(!list.scroll_to(0.5), "nothing moved");
        let short = ListState::new(vec!["a".into(), "b".into()], 5);
        assert!(!short.clone().scroll_to(1.0), "everything fits");
    }
    use crate::keys::sym;

    fn list() -> ListState {
        ListState::new(
            ["Terminal", "Files", "Editor", "Monitor", "Music", "Browser"]
                .map(String::from)
                .to_vec(),
            3,
        )
    }

    fn letter(list: &mut ListState, ch: char) -> Outcome {
        list.handle(&Key::with_text(ch as u32, ch))
    }

    #[test]
    fn search_ranks_a_name_that_starts_with_it_first() {
        let mut list = ListState::new(
            ["Text Editor", "Terminal", "Thonny", "Files"]
                .map(String::from)
                .to_vec(),
            5,
        )
        .with_keywords(
            ["app:editor", "app:terminal", "exec:thonny", "exec:yazi"]
                .map(String::from)
                .to_vec(),
        );
        list.handle(&Key::new(sym::BUTTON_X));
        for ch in "te".chars() {
            list.handle(&Key::with_text(ch as u32, ch));
        }
        let order: Vec<&str> = list.window().iter().map(|(_, l)| *l).collect();
        assert_eq!(
            order,
            ["Text Editor", "Terminal"],
            "prefix matches, in list order"
        );

        let mut list = list.clone();
        list.handle(&Key::new(sym::BACKSPACE));
        list.handle(&Key::new(sym::BACKSPACE));
        for ch in "tny".chars() {
            list.handle(&Key::with_text(ch as u32, ch));
        }
        assert_eq!(
            list.window()[0].1,
            "Thonny",
            "letters in order still find it"
        );
    }

    #[test]
    fn search_reaches_an_app_by_its_command() {
        assert_eq!(
            score("exec:yazi", "yazi"),
            Some(2),
            "a word inside the keywords"
        );
        assert_eq!(score("files", "yazi"), None);
        assert_eq!(score("thonny", "tny"), Some(6));
    }

    #[test]
    fn a_rebuilt_list_keeps_your_place() {
        let mut old = list();
        for _ in 0..4 {
            old.handle(&Key::new(sym::DOWN));
        }
        let mut new = list();
        new.keep_position(&old);
        assert_eq!(new.selected(), Some(4));
        assert_eq!(new.offset(), old.offset());

        let mut short = ListState::new(vec!["one".into(), "two".into()], 3);
        short.keep_position(&old);
        assert_eq!(short.selected(), Some(1), "clamped to what is left");
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
    fn face_buttons_activate_and_the_shoulders_leave_the_list_alone() {
        let mut list = list();
        // L and R belong to the shell (close, mode), not to paging.
        assert_eq!(letter(&mut list, 'r'), Outcome::Nothing);
        assert_eq!(letter(&mut list, 'l'), Outcome::Nothing);
        assert_eq!(list.selected(), Some(0));
        // A opens, B goes back.
        assert_eq!(letter(&mut list, 'a'), Outcome::Activate(0));
        assert_eq!(letter(&mut list, 'b'), Outcome::Back);
        // Y is the screen's secondary action.
        assert_eq!(letter(&mut list, 'y'), Outcome::Secondary);
    }

    #[test]
    fn start_and_select_both_leave() {
        let mut list = list();
        assert_eq!(list.handle(&Key::new(sym::PAUSE)), Outcome::Cancel);
        assert_eq!(list.handle(&Key::new(sym::PRINT)), Outcome::Cancel);
    }

    #[test]
    fn x_starts_filtering_and_then_letters_type() {
        let mut list = list();
        assert_eq!(list.mode(), Mode::Nav);
        assert_eq!(letter(&mut list, 'x'), Outcome::Redraw);
        assert_eq!(list.mode(), Mode::Filter);

        // 'a' now types instead of activating.
        letter(&mut list, 'i');
        assert_eq!(list.filter(), "i");
        // 'i' matches Terminal, Files, Editor, Monitor and Music — not Browser.
        assert_eq!(list.len(), 5);
    }

    #[test]
    fn filtering_puts_the_highlight_on_the_best_match() {
        let mut list = list();
        list.handle(&Key::new(sym::DOWN)); // Files
        letter(&mut list, 'x');
        letter(&mut list, 'i');
        // Several labels hold an "i" and none starts with it, so list order
        // decides, and Enter opens the top one.
        assert_eq!(list.cursor_row(), 0, "the top match, not where you were");
    }

    #[test]
    fn select_leaves_filtering_before_it_leaves_the_screen() {
        let mut list = list();
        letter(&mut list, 'x');
        letter(&mut list, 'e');
        assert_eq!(list.handle(&Key::new(sym::PRINT)), Outcome::Redraw);
        assert_eq!(list.mode(), Mode::Nav);
        assert_eq!(list.filter(), "");
        assert_eq!(list.handle(&Key::new(sym::PRINT)), Outcome::Cancel);
    }

    #[test]
    fn backspacing_past_the_start_returns_to_nav_mode() {
        let mut list = list();
        letter(&mut list, 'x');
        letter(&mut list, 'e');
        list.handle(&Key::new(sym::BACKSPACE));
        assert_eq!(
            list.mode(),
            Mode::Filter,
            "one backspace only clears the character"
        );
        list.handle(&Key::new(sym::BACKSPACE));
        assert_eq!(list.mode(), Mode::Nav);
        assert_eq!(list.handle(&Key::new(sym::BACKSPACE)), Outcome::Back);
    }

    #[test]
    fn digit_shortcut_activates_the_visible_row() {
        let mut list = list();
        assert_eq!(letter(&mut list, '2'), Outcome::Activate(1));
        // After scrolling, "1" means the first row on screen, not item 1.
        for _ in 0..4 {
            list.handle(&Key::new(sym::DOWN));
        }
        assert_eq!(letter(&mut list, '1'), Outcome::Activate(2));
    }

    #[test]
    fn an_empty_filter_result_cannot_be_activated() {
        let mut list = list();
        letter(&mut list, 'x');
        for ch in "zzz".chars() {
            letter(&mut list, ch);
        }
        assert!(list.is_empty());
        assert_eq!(list.handle(&Key::new(sym::RETURN)), Outcome::Nothing);
    }

    #[test]
    fn the_scrollbar_measures_the_filtered_list() {
        // The thumb must stay on its track with a filter on, which it cannot
        // if it is measured from `selected`.
        let items: Vec<String> = (0..20).map(|n| format!("item {n}")).collect();
        let mut list = ListState::new(items, 5);
        assert_eq!(list.scroll_progress(), Some(0.0));
        list.handle(&Key::new(sym::END));
        assert_eq!(list.scroll_progress(), Some(1.0), "End is the bottom");
        list.handle(&Key::with_text('x' as u32, 'x'));
        for ch in "item 1".chars() {
            list.handle(&Key::with_text(ch as u32, ch));
        }
        let progress = list.scroll_progress();
        assert!(
            progress.is_none_or(|p| (0.0..=1.0).contains(&p)),
            "a filtered list still measures itself: {progress:?}"
        );
    }

    #[test]
    fn a_grid_moves_in_two_dimensions_and_clamps() {
        let items: Vec<String> = (1..=9).map(|n| format!("app {n}")).collect();
        let mut grid = ListState::new(items, 3).with_columns(3);

        grid.handle(&Key::new(sym::RIGHT));
        assert_eq!(grid.selected(), Some(1));
        grid.handle(&Key::new(sym::DOWN));
        assert_eq!(grid.selected(), Some(4), "down moves a whole row");
        grid.handle(&Key::new(sym::LEFT));
        assert_eq!(grid.selected(), Some(3));
        grid.handle(&Key::new(sym::LEFT));
        assert_eq!(
            grid.selected(),
            Some(2),
            "left off the edge walks back a row"
        );

        for _ in 0..10 {
            grid.handle(&Key::new(sym::UP));
        }
        assert_eq!(
            grid.selected(),
            Some(0),
            "a grid clamps instead of wrapping"
        );
    }

    #[test]
    fn a_grid_right_moves_instead_of_activating() {
        let mut grid = ListState::new(vec!["a".into(), "b".into()], 2).with_columns(2);
        assert_eq!(grid.handle(&Key::new(sym::RIGHT)), Outcome::Redraw);
        // In a grid, Back is B or Select, never Left.
        assert_eq!(grid.handle(&Key::new(sym::LEFT)), Outcome::Redraw);
        assert_eq!(grid.handle(&Key::with_text('b' as u32, 'b')), Outcome::Back);
    }

    #[test]
    fn a_list_reports_left_and_right_for_the_screen_to_interpret() {
        let mut list = list();
        assert_eq!(list.handle(&Key::new(sym::LEFT)), Outcome::Left);
        assert_eq!(list.handle(&Key::new(sym::RIGHT)), Outcome::Right);
    }

    #[test]
    fn a_grid_scrolls_by_whole_rows() {
        let items: Vec<String> = (1..=12).map(|n| format!("app {n}")).collect();
        let mut grid = ListState::new(items, 2).with_columns(3);
        assert_eq!(grid.window().len(), 6);
        for _ in 0..3 {
            grid.handle(&Key::new(sym::DOWN));
        }
        assert_eq!(grid.selected(), Some(9));
        let first = grid.window()[0].0;
        assert_eq!(first % 3, 0, "the window starts on a row boundary");
    }

    #[test]
    fn focus_window_moves_the_cursor_and_rejects_empty_slots() {
        let mut list = list();
        assert!(list.focus_window(2));
        assert_eq!(list.selected(), Some(2));
        assert!(!list.focus_window(50));
    }

    #[test]
    fn escape_cancels() {
        assert_eq!(list().handle(&Key::new(sym::ESCAPE)), Outcome::Cancel);
    }
}
