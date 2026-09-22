//! The fullscreen menu surface.
//!
//! Three bands: a header with the screen title, a list of thumb-sized rows, and
//! a hint bar that says what the twelve physical buttons do *here*. The hint bar
//! exists because six of those buttons are letters — without a legend on screen
//! there is nothing to tell you that X searches and Y goes home.

use anyhow::Result;
use pt35_common::menu::Layout;
use pt35_common::theme::{Rgb, Theme};
use pt35_ui::canvas::Canvas;
use pt35_ui::font::Font;
use pt35_ui::keys::{Key, Mode};
use pt35_ui::layer::{self, App, SurfaceSpec};

use crate::model::{Model, Step};
use crate::{exec, providers};

/// One entry in the bottom legend. The pill is coloured like the physical
/// button so the legend can be read at a glance instead of word by word.
struct Hint {
    button: &'static str,
    action: &'static str,
}

const NAV_HINTS: &[Hint] = &[
    Hint {
        button: "A",
        action: "Open",
    },
    Hint {
        button: "B",
        action: "Back",
    },
    Hint {
        button: "X",
        action: "Find",
    },
    Hint {
        button: "Y",
        action: "Home",
    },
    Hint {
        button: "L/R",
        action: "Page",
    },
    Hint {
        button: "Sel",
        action: "Close",
    },
];

const FILTER_HINTS: &[Hint] = &[
    Hint {
        button: "Start",
        action: "Open",
    },
    Hint {
        button: "Sel",
        action: "Cancel",
    },
    Hint {
        button: "^v",
        action: "Move",
    },
];

pub struct Menu {
    theme: Theme,
    font: Font,
    model: Model,
    status: Option<pt35_common::ipc::Status>,
    windows: usize,
}

impl Menu {
    pub fn new(theme: Theme, font: Font, model: Model) -> Self {
        Self {
            theme,
            font,
            model,
            status: crate::live::status(),
            windows: providers::items(pt35_common::menu::Builtin::Windows).len(),
        }
    }

    fn draw_header(&mut self, canvas: &mut Canvas) -> i32 {
        let theme = &self.theme;
        let height = theme.menu.header_height;
        let pad = theme.menu.padding_x as i32;
        canvas.rect(0, 0, canvas.width, height, theme.color.background_alt);
        // A hairline of accent under the header ties the screen together and
        // separates it from the list without spending a whole row on a border.
        canvas.rect(0, height as i32 - 2, canvas.width, 2, theme.color.accent);

        let baseline = (height as f32 * 0.66) as i32;
        let deeper = self.model.depth() > 1;
        let title = self.model.screen().title.clone();
        let mut x = pad;
        if deeper {
            x = self.font.draw(
                canvas,
                "<",
                x,
                baseline,
                theme.font.size_title,
                theme.color.muted,
            ) + 8;
        }
        self.font.draw(
            canvas,
            &title,
            x,
            baseline,
            theme.font.size_title,
            theme.color.foreground,
        );

        // Right-hand side: the live filter, or a nudge that X starts one.
        let filtering = self.model.screen().list.mode() == Mode::Filter;
        let filter = self.model.screen().list.filter().to_string();
        let (text, color) = if filtering {
            (format!("{filter}_"), theme.color.accent)
        } else {
            (theme.menu.filter_hint.clone(), theme.color.muted)
        };
        let width = self.font.measure(&text, theme.font.size_hint) as i32;
        self.font.draw(
            canvas,
            &text,
            canvas.width as i32 - pad - width,
            baseline,
            theme.font.size_hint,
            color,
        );
        height as i32
    }

    fn draw_rows(&mut self, canvas: &mut Canvas, top: i32, bottom: i32) {
        let theme = &self.theme;
        let pad = theme.menu.padding_x as i32;
        let row_h = theme.menu.row_height as i32;
        let size = theme.font.size_menu;

        let rows = self.model.visible_rows();
        if rows.is_empty() {
            let hint = "no matches";
            let width = self.font.measure(hint, size) as i32;
            self.font.draw(
                canvas,
                hint,
                (canvas.width as i32 - width) / 2,
                top + row_h,
                size,
                self.theme.color.muted,
            );
            return;
        }

        let cursor = self.model.screen().list.cursor_row();
        let numbers = theme.menu.show_numbers;
        let radius = theme.menu.radius;

        for (index, row) in rows.iter().enumerate() {
            let y = top + index as i32 * row_h;
            if y + row_h > bottom {
                break;
            }
            let selected = index == cursor;
            if selected {
                canvas.rounded_rect(
                    pad / 2,
                    y + 3,
                    canvas.width - pad as u32,
                    (row_h - 6) as u32,
                    radius,
                    theme.color.accent,
                );
            }
            let fg = if selected {
                theme.color.accent_fg
            } else {
                theme.color.foreground
            };
            let dim = if selected {
                theme.color.accent_fg
            } else {
                theme.color.muted
            };
            let baseline = y + (row_h as f32 * 0.66) as i32;

            let mut x = pad + 6;
            if numbers && index < 9 {
                let number = format!("{}", index + 1);
                self.font
                    .draw(canvas, &number, x, baseline, theme.font.size_hint, dim);
                x += 22;
            }
            // Leave room for the chevron so a long label never collides with it.
            let room = canvas.width.saturating_sub(x as u32 + pad as u32 + 24);
            let label = self.font.elide(&row.label, size, room);
            self.font.draw(canvas, &label, x, baseline, size, fg);

            if row.submenu {
                let chevron_x = canvas.width as i32 - pad - 14;
                self.font.draw(canvas, ">", chevron_x, baseline, size, dim);
            }
        }

        // Scroll indicator: a slim bar on the right, only when it means something.
        let total = self.model.screen().list.len();
        let visible = self.model.screen().list.rows();
        if total > visible {
            let track_h = (bottom - top) as u32;
            let thumb_h = ((visible as f32 / total as f32) * track_h as f32).max(24.0) as u32;
            let first = self.model.screen().list.cursor_row();
            let progress = (self.model.screen().list.selected().unwrap_or(0) as f32 - first as f32)
                / (total as f32 - visible as f32).max(1.0);
            let thumb_y = top + (progress.clamp(0.0, 1.0) * (track_h - thumb_h) as f32) as i32;
            canvas.rect(canvas.width as i32 - 4, top, 2, track_h, theme.color.border);
            canvas.rounded_rect(
                canvas.width as i32 - 5,
                thumb_y,
                4,
                thumb_h,
                2,
                theme.color.muted,
            );
        }
    }

    /// Pill colour and text colour for a legend entry.
    fn button_colors(&self, button: &str) -> (Rgb, Rgb) {
        let c = &self.theme.color;
        match button {
            "A" => (c.button_a, c.accent_fg),
            "B" => (c.button_b, c.accent_fg),
            "X" => (c.button_x, c.background),
            "Y" => (c.button_y, c.background),
            "Start" => (c.ok, c.background),
            _ => (c.button_neutral, c.foreground),
        }
    }

    fn draw_tiles(&mut self, canvas: &mut Canvas, top: i32, bottom: i32) {
        let theme = self.theme.clone();
        let pad = theme.menu.padding_x as i32;
        let gap = theme.menu.gap as i32;
        let columns = theme.menu.columns.max(1) as i32;
        let tile_w = (canvas.width as i32 - pad * 2 - gap * (columns - 1)) / columns;

        let rows = self.model.visible_rows();
        // Grow the tiles to fill the body rather than leaving a dead band under
        // a short grid. Never shrink below the configured height.
        let lines = ((rows.len() as i32 + columns - 1) / columns).max(1);
        let available = bottom - top;
        let tile_h = ((available - gap * (lines - 1)) / lines).max(theme.menu.tile_height as i32);
        let radius = theme.menu.radius;

        let cursor = self.model.screen().list.cursor_index();

        for (index, row) in rows.iter().enumerate() {
            let column = index as i32 % columns;
            let line = index as i32 / columns;
            let x = pad + column * (tile_w + gap);
            let y = top + line * (tile_h + gap);
            if y + tile_h > bottom {
                break;
            }
            let focused = index == cursor;
            let tint = row.tint.unwrap_or(theme.color.accent);
            let note = row
                .builtin
                .and_then(|b| crate::live::note(b, self.status.as_ref(), self.windows))
                .unwrap_or_else(|| row.note.clone());

            if focused {
                // Halo, then border, then face: the focused tile is the only
                // thing on screen allowed to glow.
                canvas.rounded_rect(
                    x - 2,
                    y - 2,
                    (tile_w + 4) as u32,
                    (tile_h + 4) as u32,
                    radius + 2,
                    tint,
                );
                canvas.rounded_rect(
                    x,
                    y,
                    tile_w as u32,
                    tile_h as u32,
                    radius,
                    theme.color.background_alt,
                );
            } else {
                canvas.rounded_rect(
                    x,
                    y,
                    tile_w as u32,
                    tile_h as u32,
                    radius,
                    theme.color.border,
                );
                canvas.rounded_rect(
                    x + 1,
                    y + 1,
                    (tile_w - 2) as u32,
                    (tile_h - 2) as u32,
                    radius,
                    theme.color.background_alt,
                );
            }

            let badge = 40;
            let bx = x + 12;
            let by = y + (tile_h - badge) / 2;
            canvas.rounded_rect(bx, by, badge as u32, badge as u32, 10, tint);
            let glyph_size = theme.font.size_title;
            let glyph_w = self.font.measure(&row.glyph, glyph_size) as i32;
            self.font.draw(
                canvas,
                &row.glyph,
                bx + (badge - glyph_w) / 2,
                by + badge / 2 + (glyph_size / 3.0) as i32,
                glyph_size,
                theme.color.background,
            );

            let text_x = bx + badge + 10;
            let room = (x + tile_w - text_x - 8).max(8) as u32;
            let label = self.font.elide(&row.label, theme.font.size_menu, room);
            let has_note = !note.is_empty();
            let label_y = if has_note {
                y + tile_h / 2 - 2
            } else {
                y + tile_h / 2 + 7
            };
            self.font.draw(
                canvas,
                &label,
                text_x,
                label_y,
                theme.font.size_menu,
                theme.color.foreground,
            );
            if has_note {
                let note = self.font.elide(&note, theme.font.size_hint, room);
                self.font.draw(
                    canvas,
                    &note,
                    text_x,
                    label_y + 18,
                    theme.font.size_hint,
                    theme.color.muted,
                );
            }
        }
    }

    fn draw_hints(&mut self, canvas: &mut Canvas, top: i32) {
        let theme = &self.theme;
        let height = theme.menu.hint_height;
        canvas.rect(0, top, canvas.width, height, theme.color.background_alt);
        canvas.rect(0, top, canvas.width, 1, theme.color.border);

        let filtering = self.model.screen().list.mode() == Mode::Filter;
        let hints: &[Hint] = if filtering { FILTER_HINTS } else { NAV_HINTS };
        let size = theme.font.size_hint;
        let baseline = top + (height as f32 * 0.62) as i32;
        let centre = top + height as i32 / 2;

        // Lay the chips out evenly across the full width: they are touch targets
        // as much as a legend.
        let slot = canvas.width as f32 / hints.len() as f32;
        for (index, hint) in hints.iter().enumerate() {
            let slot_x = (index as f32 * slot) as i32;
            let mut x = slot_x + 7;
            let label_w = self.font.measure(hint.button, size) as i32;
            let (pill, ink) = self.button_colors(hint.button);
            let pill_w = (label_w + 14).max(22);
            canvas.rounded_rect(x, centre - 11, pill_w as u32, 22, 11, pill);
            self.font.draw(
                canvas,
                hint.button,
                x + (pill_w - label_w) / 2,
                baseline,
                size,
                ink,
            );
            x += pill_w + 6;

            let room = (slot as i32 - (x - slot_x) - 4).max(0) as u32;
            let action = self.font.elide(hint.action, size, room);
            self.font
                .draw(canvas, &action, x, baseline, size, self.theme.color.muted);
        }
    }
}

impl App for Menu {
    fn background(&self) -> Rgb {
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
        let hint_top = canvas.height as i32 - self.theme.menu.hint_height as i32;
        let body_top = self.draw_header(canvas) + 8;
        match self.model.layout() {
            Layout::Grid => self.draw_tiles(canvas, body_top, hint_top - 4),
            Layout::List => self.draw_rows(canvas, body_top, hint_top),
        }
        self.draw_hints(canvas, hint_top);
    }
}

pub fn run(page: Option<String>) -> Result<()> {
    let theme: Theme = pt35_common::load_config("pt35/theme.toml").unwrap_or_default();
    let tree = pt35_common::load_config("pt35/menu.toml").unwrap_or_default();
    let font = Font::load(&theme.font.family)?;
    let rows = theme.menu.rows_visible as usize;
    let body = 480 - theme.bar.height - theme.menu.header_height - theme.menu.hint_height;
    let grid_rows = (body / (theme.menu.tile_height + theme.menu.gap)).max(1) as usize;
    let model = Model::sized(
        tree,
        page.as_deref(),
        rows,
        grid_rows,
        theme.menu.columns as usize,
    );
    layer::run(
        Menu::new(theme, font, model),
        SurfaceSpec::overlay("pt35-menu"),
    )
}
