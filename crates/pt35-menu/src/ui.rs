//! The fullscreen menu surface: a title, a filter line, and a list of rows
//! large enough to hit with a thumb but dense enough to show nine at once.

use anyhow::Result;
use pt35_common::theme::Theme;
use pt35_ui::canvas::Canvas;
use pt35_ui::font::Font;
use pt35_ui::keys::Key;
use pt35_ui::layer::{self, App, SurfaceSpec};

use crate::model::{Model, Step};
use crate::{exec, providers};

pub struct Menu {
    theme: Theme,
    font: Font,
    model: Model,
}

impl Menu {
    pub fn new(theme: Theme, font: Font, model: Model) -> Self {
        Self { theme, font, model }
    }
}

impl App for Menu {
    fn background(&self) -> pt35_common::theme::Rgb {
        self.theme.color.background
    }

    fn key(&mut self, key: Key) -> bool {
        match self.model.handle(&key) {
            Step::Quit => false,
            Step::Run(command) => {
                exec::perform(&command);
                false
            }
            Step::Open(builtin) => {
                let items = providers::items(builtin);
                self.model
                    .push_dynamic(builtin, providers::title(builtin), items);
                true
            }
            Step::Redraw | Step::Nothing => true,
        }
    }

    fn draw(&mut self, canvas: &mut Canvas) {
        let theme = &self.theme;
        let pad = theme.menu.padding_x as i32;
        let row_h = theme.menu.row_height;
        let size = theme.font.size_menu;

        // Header: menu title on the left, filter (or its hint) on the right.
        let header_h = (theme.font.size_title * 1.8) as u32;
        canvas.rect(0, 0, canvas.width, header_h, theme.color.background_alt);
        let title_baseline = (header_h as f32 * 0.68) as i32;
        let screen = self.model.screen();
        // A back chevron is the only hint that Backspace/Left goes up a level.
        let title = if self.model.depth() > 1 {
            format!("< {}", screen.title)
        } else {
            screen.title.clone()
        };
        let filter = screen.list.filter().to_string();
        let count = screen.list.len();
        self.font.draw(
            canvas,
            &title,
            pad,
            title_baseline,
            theme.font.size_title,
            theme.color.foreground,
        );

        let (hint, hint_color) = if filter.is_empty() {
            (theme.menu.filter_hint.clone(), theme.color.muted)
        } else {
            (format!("/{filter}"), theme.color.accent)
        };
        let hint_width = self.font.measure(&hint, theme.font.size_hint) as i32;
        self.font.draw(
            canvas,
            &hint,
            canvas.width as i32 - pad - hint_width,
            title_baseline,
            theme.font.size_hint,
            hint_color,
        );

        if count == 0 {
            self.font.draw(
                canvas,
                "no matches",
                pad,
                header_h as i32 + row_h as i32,
                size,
                theme.color.muted,
            );
            return;
        }

        // Rows. The selected one is a filled bar: at 640x480 an underline or a
        // thin border is easy to lose.
        let rows: Vec<(usize, String)> = self
            .model
            .screen()
            .list
            .window()
            .into_iter()
            .map(|(i, label)| (i, label.to_string()))
            .collect();
        let cursor = self.model.screen().list.cursor_row();
        let numbers = theme.menu.show_numbers;

        for (row, (_, label)) in rows.iter().enumerate() {
            let y = header_h as i32 + (row as u32 * row_h) as i32;
            if y + row_h as i32 > canvas.height as i32 {
                break;
            }
            let selected = row == cursor;
            if selected {
                canvas.rect(0, y, canvas.width, row_h, theme.color.accent);
            }
            let fg = if selected {
                theme.color.accent_fg
            } else {
                theme.color.foreground
            };
            let baseline = y + (row_h as f32 * 0.68) as i32;

            let mut x = pad;
            if numbers && row < 9 {
                let number = format!("{}", row + 1);
                let color = if selected {
                    theme.color.accent_fg
                } else {
                    theme.color.muted
                };
                self.font
                    .draw(canvas, &number, x, baseline, theme.font.size_hint, color);
                x += (size * 0.9) as i32;
            }
            let room = canvas.width.saturating_sub(x as u32 + pad as u32);
            let text = self.font.elide(label, size, room);
            self.font.draw(canvas, &text, x, baseline, size, fg);
        }
    }
}

pub fn run(page: Option<String>) -> Result<()> {
    let theme: Theme = pt35_common::load_config("pt35/theme.toml").unwrap_or_default();
    let tree = pt35_common::load_config("pt35/menu.toml").unwrap_or_default();
    let font = Font::load(&theme.font.family)?;
    let rows = theme.menu.rows_visible as usize;
    let model = Model::new(tree, page.as_deref(), rows);
    layer::run(
        Menu::new(theme, font, model),
        SurfaceSpec::overlay("pt35-menu"),
    )
}
