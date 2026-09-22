//! Grid jump: the fast way to reach a mouse-only control with no mouse.
//!
//! The screen is divided into labelled cells; pressing a label narrows to that
//! cell and subdivides it again. Two key presses put the cursor within ~20 px
//! of anywhere on a 640x480 panel, which beats nudging a D-pad across it.

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn centre(&self) -> (f32, f32) {
        (self.x + self.w / 2.0, self.y + self.h / 2.0)
    }
}

pub struct Grid {
    labels: Vec<char>,
    columns: usize,
    rows: usize,
    area: Rect,
    /// How many times the area has been subdivided so far.
    pub level: u8,
    levels: u8,
}

impl Grid {
    /// `labels` comes from the theme (default `asdfghjkl` — the home row).
    pub fn new(labels: &str, levels: u8, width: f32, height: f32) -> Self {
        let labels: Vec<char> = labels.chars().collect();
        // Square-ish grid: 9 labels -> 3x3, 8 -> 3x3 with one gap.
        let columns = (labels.len() as f32).sqrt().ceil() as usize;
        let rows = labels.len().div_ceil(columns);
        Self {
            labels,
            columns,
            rows,
            area: Rect {
                x: 0.0,
                y: 0.0,
                w: width,
                h: height,
            },
            level: 0,
            levels: levels.max(1),
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn area(&self) -> Rect {
        self.area
    }

    /// Cells of the current area, as (label, rect).
    pub fn cells(&self) -> Vec<(char, Rect)> {
        let cell_w = self.area.w / self.columns as f32;
        let cell_h = self.area.h / self.rows as f32;
        self.labels
            .iter()
            .enumerate()
            .map(|(index, &label)| {
                let column = index % self.columns;
                let row = index / self.columns;
                (
                    label,
                    Rect {
                        x: self.area.x + column as f32 * cell_w,
                        y: self.area.y + row as f32 * cell_h,
                        w: cell_w,
                        h: cell_h,
                    },
                )
            })
            .collect()
    }

    /// Feed a label. Returns the target point once the last level is reached.
    pub fn select(&mut self, label: char) -> Option<(f32, f32)> {
        let label = label.to_ascii_lowercase();
        let cell = self.cells().into_iter().find(|(l, _)| *l == label)?;
        self.area = cell.1;
        self.level += 1;
        if self.level >= self.levels {
            Some(self.area.centre())
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid() -> Grid {
        Grid::new("asdfghjkl", 2, 640.0, 480.0)
    }

    #[test]
    fn splits_the_screen_into_labelled_cells() {
        let grid = grid();
        let cells = grid.cells();
        assert_eq!(cells.len(), 9);
        let (label, rect) = cells[0];
        assert_eq!(label, 'a');
        assert_eq!((rect.w, rect.h), (640.0 / 3.0, 480.0 / 3.0));
        // The last cell sits in the bottom-right corner.
        let (_, last) = cells[8];
        assert!((last.x + last.w - 640.0).abs() < 0.01);
        assert!((last.y + last.h - 480.0).abs() < 0.01);
    }

    #[test]
    fn two_presses_land_on_a_point() {
        let mut grid = grid();
        assert_eq!(grid.select('a'), None, "the first level only narrows");
        let point = grid.select('l').expect("the second level lands");
        // 'a' is the top-left ninth, 'l' its bottom-right ninth.
        assert!(point.0 > 640.0 / 3.0 * 0.66 && point.0 < 640.0 / 3.0);
        assert!(point.1 > 480.0 / 3.0 * 0.66 && point.1 < 480.0 / 3.0);
    }

    #[test]
    fn the_centre_cell_lands_near_the_middle() {
        let mut grid = grid();
        // "asdfghjkl" laid out 3x3 puts 'g' in the middle.
        grid.select('g');
        let (x, y) = grid.select('g').unwrap();
        assert!((x - 320.0).abs() < 1.0, "{x}");
        assert!((y - 240.0).abs() < 1.0, "{y}");
    }

    #[test]
    fn an_unknown_label_changes_nothing() {
        let mut grid = grid();
        let before = grid.area();
        assert_eq!(grid.select('z'), None);
        assert_eq!(grid.area(), before);
        assert_eq!(grid.level, 0);
    }

    #[test]
    fn labels_are_case_insensitive() {
        let mut grid = grid();
        grid.select('A');
        assert_eq!(grid.level, 1);
    }
}
