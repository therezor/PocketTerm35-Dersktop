//! The overlay: takes the keyboard, draws the grid, and tells the driver where
//! to move.

use anyhow::Result;
use pt35_common::theme::Theme;
use pt35_ui::canvas::Canvas;
use pt35_ui::font::Font;
use pt35_ui::keys::Key;
use pt35_ui::layer::{self, App, SurfaceSpec};
use std::sync::mpsc::Sender;
use std::time::Duration;

use crate::driver::Cmd;
use crate::grid::Grid;
use crate::motion::{action, Action, Direction, Motion};

const FRAME: Duration = Duration::from_millis(16);

pub struct Pointer {
    theme: Theme,
    font: Font,
    motion: Motion,
    grid: Option<Grid>,
    driver: Sender<Cmd>,
    size: (u32, u32),
}

impl Pointer {
    pub fn new(theme: Theme, font: Font, driver: Sender<Cmd>, grid_at_start: bool) -> Self {
        let mut pointer = Self {
            theme,
            font,
            motion: Motion::default(),
            grid: None,
            driver,
            size: (640, 480),
        };
        if grid_at_start {
            pointer.open_grid();
        }
        pointer
    }

    fn open_grid(&mut self) {
        self.grid = Some(Grid::new(
            &self.theme.pointer.grid_labels,
            self.theme.pointer.grid_levels,
            self.size.0 as f32,
            self.size.1 as f32,
        ));
    }

    fn send(&self, cmd: Cmd) {
        if let Err(e) = self.driver.send(cmd) {
            log::error!("pointer driver gone: {e}");
        }
    }
}

impl App for Pointer {
    fn transparent(&self) -> bool {
        true
    }

    fn tick_interval(&self) -> Option<Duration> {
        Some(FRAME)
    }

    fn tick(&mut self) -> bool {
        if !self.motion.moving() {
            return false;
        }
        let (dx, dy) = self.motion.step(&self.theme.pointer, FRAME);
        self.send(Cmd::Motion(dx, dy));
        false // motion changes the compositor's cursor, not our surface
    }

    fn key(&mut self, key: Key) -> bool {
        // Grid mode swallows letters: they are cell labels, not shortcuts.
        if self.grid.is_some() {
            if let Some(Action::Quit) = action(&key) {
                self.grid = None;
                return true;
            }
            if let Some(ch) = key.text {
                let extent = self.size;
                let landed = self.grid.as_mut().and_then(|grid| grid.select(ch));
                if let Some((x, y)) = landed {
                    self.send(Cmd::Absolute { x, y, extent });
                    self.grid = None;
                }
                return true;
            }
            return true;
        }

        if let Some(direction) = Direction::from_sym(key.sym) {
            self.motion.press(direction);
            return true;
        }
        match action(&key) {
            Some(Action::Click(button)) => {
                self.send(Cmd::Click(button));
                true
            }
            Some(Action::ScrollUp) => {
                self.send(Cmd::Scroll(-1.0));
                true
            }
            Some(Action::ScrollDown) => {
                self.send(Cmd::Scroll(1.0));
                true
            }
            Some(Action::Grid) => {
                self.open_grid();
                true
            }
            // Escape disarms the pointer: the keyboard goes back to the app.
            Some(Action::Quit) => false,
            None => true,
        }
    }

    fn key_release(&mut self, key: Key) {
        if let Some(direction) = Direction::from_sym(key.sym) {
            self.motion.release(direction);
        }
    }

    fn draw(&mut self, canvas: &mut Canvas) {
        self.size = (canvas.width, canvas.height);
        let Some(grid) = &self.grid else { return };

        let theme = &self.theme;
        let size = theme.font.size_title;
        // Dim the screen a little so the labels are readable over anything.
        canvas.fill_alpha(theme.color.background, 120);

        for (label, rect) in grid.cells() {
            let (x, y, w, h) = (rect.x as i32, rect.y as i32, rect.w as u32, rect.h as u32);
            // Cell outline.
            canvas.rect(x, y, w, 1, theme.color.accent);
            canvas.rect(x, y, 1, h, theme.color.accent);

            let text = label.to_string();
            let text_w = self.font.measure(&text, size) as i32;
            let (cx, cy) = rect.centre();
            self.font.draw(
                canvas,
                &text,
                cx as i32 - text_w / 2,
                cy as i32 + (size / 3.0) as i32,
                size,
                theme.color.accent,
            );
        }
    }
}

pub fn run(start_in_grid: bool) -> Result<()> {
    let theme: Theme = pt35_common::load_config("pt35/theme.toml").unwrap_or_default();
    let font = Font::load(&theme.font.family)?;
    let driver = crate::driver::spawn()?;
    let pointer = Pointer::new(theme, font, driver.clone(), start_in_grid);
    let result = layer::run(pointer, SurfaceSpec::passthrough_overlay("pt35-pointer"));
    let _ = driver.send(Cmd::Stop);
    result
}
