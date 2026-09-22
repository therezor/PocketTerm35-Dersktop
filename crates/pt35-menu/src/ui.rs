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
/// The launcher's side buttons: not apps, so not in the app list.
#[derive(Clone, Copy)]
enum Side {
    Screen(pt35_common::menu::Builtin),
    Page(&'static str),
}

/// Label, what it opens, icon, and the colour that marks it. Three of them, so
/// the colour is what you actually navigate by.
struct SideButton {
    label: &'static str,
    target: Side,
    icon: &'static str,
    tint: fn(&pt35_common::theme::Colors) -> Rgb,
}

const SIDE: &[SideButton] = &[
    SideButton {
        label: "Windows",
        target: Side::Screen(pt35_common::menu::Builtin::Windows),
        icon: "multitasking-view",
        tint: |c| c.button_x,
    },
    SideButton {
        label: "Settings",
        target: Side::Screen(pt35_common::menu::Builtin::Quick),
        icon: "preferences-system",
        tint: |c| c.accent,
    },
    SideButton {
        label: "Power",
        target: Side::Page("power"),
        icon: "system-shutdown",
        tint: |c| c.critical,
    },
];

#[derive(Clone, Copy)]
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
        button: "Start",
        action: "Close",
    },
];

const WINDOW_HINTS: &[Hint] = &[
    Hint {
        button: "A",
        action: "Focus",
    },
    Hint {
        button: "B",
        action: "Back",
    },
    Hint {
        button: "Y",
        action: "Close",
    },
    Hint {
        button: "Start",
        action: "Close",
    },
];

const QUICK_HINTS: &[Hint] = &[
    Hint {
        button: "<>",
        action: "Adjust",
    },
    Hint {
        button: "B",
        action: "Back",
    },
    Hint {
        button: "Y",
        action: "Home",
    },
    Hint {
        button: "Start",
        action: "Close",
    },
];

const FILTER_HINTS: &[Hint] = &[
    Hint {
        button: "Enter",
        action: "Open",
    },
    Hint {
        button: "Start",
        action: "Close",
    },
    Hint {
        button: "^v",
        action: "Move",
    },
];

pub struct Menu {
    theme: Theme,
    font: Font,
    mono: Font,
    /// For the things you press. Regular text at 16px on this panel is thin.
    bold: Font,
    mono_bold: Font,
    model: Model,
    status: Option<pt35_common::ipc::Status>,
    icons: pt35_ui::icon::Icons,
    /// Which of the launcher's side buttons has the cursor, if any. The apps
    /// are a list and these three are not, so they are tracked apart.
    side: Option<usize>,
    windows: usize,
    /// Hit boxes recorded by the last draw, so touch never has to re-derive
    /// the layout and drift from it.
    row_hits: Vec<(i32, i32, i32, i32)>,
    hint_hits: Vec<(i32, i32, i32, &'static str)>,
    error: Option<String>,
}

impl Menu {
    pub fn new(
        theme: Theme,
        font: Font,
        mono: Font,
        bold: Font,
        mono_bold: Font,
        model: Model,
    ) -> Self {
        let status = crate::live::status();
        let icons = pt35_ui::icon::Icons::new(&theme.icons.theme);
        Self {
            icons,
            side: None,
            theme,
            font,
            mono,
            bold,
            mono_bold,
            model,
            windows: status.as_ref().map(|s| s.windows.len()).unwrap_or(0),
            status,
            row_hits: Vec::new(),
            hint_hits: Vec::new(),
            error: None,
        }
    }

    fn on_launcher(&self) -> bool {
        matches!(
            self.model.screen().source,
            crate::model::Source::Dynamic {
                builtin: pt35_common::menu::Builtin::Launcher,
                ..
            }
        )
    }

    fn open_side(&mut self, index: usize) -> bool {
        self.side = None;
        match SIDE.get(index).map(|button| button.target) {
            Some(Side::Screen(builtin)) => {
                let items = providers::items(builtin);
                self.model
                    .push_dynamic(builtin, providers::title(builtin), items);
                true
            }
            Some(Side::Page(page)) => {
                self.model.open_page(page);
                true
            }
            None => true,
        }
    }

    /// Feed a tap to the model as if the matching button had been pressed.
    fn press(&mut self, button: &str) -> bool {
        let key = match button {
            "A" => Key::with_text('a' as u32, 'a'),
            "B" => Key::with_text('b' as u32, 'b'),
            "X" => Key::with_text('x' as u32, 'x'),
            "Y" => Key::with_text('y' as u32, 'y'),
            "Start" => Key::new(pt35_ui::keys::sym::PAUSE),
            "Enter" => Key::new(pt35_ui::keys::sym::RETURN),
            _ => return true,
        };
        let step = self.model.handle(&key);
        self.apply(step)
    }

    fn apply(&mut self, step: Step) -> bool {
        self.error = None;
        match step {
            Step::Close(criteria) => {
                let cmd = format!("swaymsg '{criteria} kill'");
                if let Err(e) = std::process::Command::new("sh").arg("-c").arg(&cmd).spawn() {
                    self.error = Some(format!("{cmd}: {e}"));
                }
                // Rebuild the list: the window it named is going away.
                std::thread::sleep(std::time::Duration::from_millis(150));
                let items = providers::items(pt35_common::menu::Builtin::Windows);
                self.model.replace_dynamic(items);
                true
            }
            Step::Adjust(adjust, up) => {
                let args = adjust.step(up);
                if let Err(e) = std::process::Command::new("pt35ctl").args(&args).spawn() {
                    self.error = Some(format!("pt35ctl: {e}"));
                }
                // The value on screen comes from the daemon, so re-read it.
                std::thread::sleep(std::time::Duration::from_millis(120));
                self.status = crate::live::status();
                true
            }
            Step::RunStay(command) => {
                if let Err(message) = exec::perform(&command) {
                    self.error = Some(message);
                }
                // The value on the row comes from the daemon, so re-read it.
                std::thread::sleep(std::time::Duration::from_millis(120));
                self.status = crate::live::status();
                true
            }
            Step::Quit => false,
            Step::Run(command) => {
                // A launch that fails must not close the menu: the screen would
                // go back to an empty workspace with no word of what happened.
                if let Err(message) = exec::perform(&command) {
                    self.error = Some(message);
                    return true;
                }
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

    fn draw_header(&mut self, canvas: &mut Canvas) -> i32 {
        let theme = self.theme.clone();
        let height = theme.menu.header_height;
        let pad = theme.menu.padding_x as i32;
        let track = theme.font.tracking;
        canvas.rect(0, 0, canvas.width, height, theme.color.background_alt);
        canvas.rect(0, height as i32 - 2, canvas.width, 2, theme.color.accent);

        let baseline = (height as f32 * 0.64) as i32;
        let title = self.model.screen().title.to_uppercase();
        // A deeper screen shows the way back in the path itself.
        let path = if self.model.depth() > 1 {
            format!("PT35 < {title}")
        } else {
            format!("PT35 // {title}")
        };
        let mut x = pad;
        x = self.mono.draw_tracked(
            canvas,
            "PT35",
            x,
            baseline,
            theme.font.size_title,
            theme.color.accent,
            track,
        );
        let rest = path.trim_start_matches("PT35");
        self.mono.draw_tracked(
            canvas,
            rest,
            x,
            baseline,
            theme.font.size_title,
            theme.color.foreground,
            track,
        );

        let filtering = self.model.screen().list.mode() == Mode::Filter;
        let filter = self.model.screen().list.filter().to_string();
        let (text, color) = if filtering {
            (format!("[{filter}_]"), theme.color.accent)
        } else {
            (theme.menu.filter_hint.to_uppercase(), theme.color.muted)
        };
        let width = self
            .mono
            .measure_tracked(&text, theme.font.size_hint, track) as i32;
        self.mono.draw_tracked(
            canvas,
            &text,
            canvas.width as i32 - pad - width,
            baseline,
            theme.font.size_hint,
            color,
            track,
        );
        height as i32
    }

    fn draw_rows(&mut self, canvas: &mut Canvas, top: i32, bottom: i32) {
        let theme = &self.theme;
        let pad = theme.menu.padding_x as i32;
        let row_h = theme.menu.row_height as i32;
        let size = theme.font.size_menu;

        self.row_hits.clear();
        let rows = self.model.visible_rows();
        if rows.is_empty() {
            // An empty screen and an empty search are different problems.
            let hint = if self.model.screen().list.mode() == Mode::Filter {
                "no matches"
            } else {
                "nothing here"
            };
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
            self.row_hits.push((0, y, canvas.width as i32, y + row_h));
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
            } else if index + 1 < rows.len() {
                // Hairline between rows. Without it a list of short labels reads
                // as floating text.
                canvas.rect(
                    pad,
                    y + row_h - 1,
                    canvas.width - 2 * pad as u32,
                    1,
                    theme.color.border,
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
                self.mono
                    .draw(canvas, &number, x, baseline, theme.font.size_hint, dim);
                x += 22;
            }

            // Quick settings read out their value on the right, where the
            // chevron would be on a row that opens something.
            let right_text = match (row.adjust, row.state) {
                (Some(adjust), _) => Some(crate::live::value(adjust, self.status.as_ref())),
                (None, Some(field)) => Some(crate::live::state_value(field, self.status.as_ref())),
                (None, None) if row.submenu => Some(">".to_string()),
                (None, None) => None,
            };
            let mut right_width = 0;
            if let Some(text) = &right_text {
                let mono = row.adjust.is_some() || row.state.is_some();
                let size_right = if mono { theme.font.size_hint } else { size };
                let width = if mono {
                    self.mono.measure(text, size_right) as i32
                } else {
                    self.font.measure(text, size_right) as i32
                };
                let tx = canvas.width as i32 - pad - width;
                let colour = if selected { fg } else { theme.color.accent };
                if mono {
                    self.mono
                        .draw(canvas, text, tx, baseline, size_right, colour);
                } else {
                    self.font.draw(canvas, text, tx, baseline, size_right, dim);
                }
                right_width = width + 10;
            }

            let room = canvas
                .width
                .saturating_sub(x as u32 + pad as u32 + right_width as u32);
            let label = self.font.elide(&row.label, size, room);
            self.font.draw(canvas, &label, x, baseline, size, fg);
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
        // Grown to fill the body, but only so far: two windows in the picker
        // must not become two half-screen slabs.
        let tile_h = ((available - gap * (lines - 1)) / lines).clamp(
            theme.menu.tile_height as i32,
            theme.menu.tile_height as i32 * 3 / 2,
        );
        let radius = theme.menu.radius;

        let cursor = self.model.screen().list.cursor_index();
        self.row_hits.clear();

        for (index, row) in rows.iter().enumerate() {
            let column = index as i32 % columns;
            let line = index as i32 / columns;
            let x = pad + column * (tile_w + gap);
            let y = top + line * (tile_h + gap);
            if y + tile_h > bottom {
                break;
            }
            self.row_hits.push((x, y, x + tile_w, y + tile_h));
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
                    x + 1,
                    y + 1,
                    (tile_w - 2) as u32,
                    (tile_h - 2) as u32,
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
            let icon_size = theme.icons.size_tile;
            let has_icon = !row.icon.is_empty() && self.icons.get(&row.icon, icon_size).is_some();
            if has_icon {
                // The icon carries the colour, so the badge steps back to a
                // tinted outline.
                canvas.rounded_rect(bx, by, badge as u32, badge as u32, theme.menu.radius, tint);
                canvas.rounded_rect(
                    bx + 1,
                    by + 1,
                    (badge - 2) as u32,
                    (badge - 2) as u32,
                    theme.menu.radius,
                    theme.color.background,
                );
                let inset = (badge - icon_size as i32) / 2;
                if let Some(icon) = self.icons.get(&row.icon, icon_size) {
                    icon.draw(canvas, bx + inset, by + inset);
                }
            } else {
                canvas.rounded_rect(bx, by, badge as u32, badge as u32, theme.menu.radius, tint);
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
            }

            if focused {
                // Corner ticks: instrument framing, and they survive on a busy
                // wallpaper better than a full ring.
                let t = 10;
                for (cx, cy, dx, dy) in [
                    (x + 4, y + 4, 1, 1),
                    (x + tile_w - 5, y + 4, -1, 1),
                    (x + 4, y + tile_h - 5, 1, -1),
                    (x + tile_w - 5, y + tile_h - 5, -1, -1),
                ] {
                    canvas.rect(cx.min(cx + dx * t), cy, t as u32, 2, tint);
                    canvas.rect(cx, cy.min(cy + dy * t), 2, t as u32, tint);
                }
            }

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
                let note = self.mono.elide(&note, theme.font.size_hint, room);
                self.mono.draw(
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

    /// The launcher: apps down the left, the three screens that are not apps
    /// down the right.
    fn draw_launcher(&mut self, canvas: &mut Canvas, top: i32, bottom: i32) {
        let theme = self.theme.clone();
        let pad = theme.menu.padding_x as i32;
        // The column runs to the edge: a margin outside it would read as the
        // list continuing, and 640px has none to spare.
        let split = canvas.width as i32 - 148;
        let icon_size = 24;

        let rows = self.model.visible_rows();
        let cursor = self.model.screen().list.cursor_index();
        let row_h = theme.menu.row_height as i32;
        self.row_hits.clear();
        for (index, row) in rows.iter().enumerate() {
            let y = top + index as i32 * row_h;
            if y + row_h > bottom {
                break;
            }
            self.row_hits.push((0, y, split, y + row_h));
            let focused = index == cursor && self.side.is_none();
            if focused {
                canvas.rect(0, y, split as u32, row_h as u32, theme.color.background_alt);
                canvas.rect(0, y, 3, row_h as u32, theme.color.accent);
            }
            let centre = y + row_h / 2;
            let drawn = !row.icon.is_empty()
                && self
                    .icons
                    .get(&row.icon, icon_size)
                    .map(|icon| icon.draw(canvas, pad, centre - icon_size as i32 / 2))
                    .is_some();
            if !drawn {
                let glyph = if row.glyph.is_empty() {
                    row.label.chars().next().unwrap_or('?').to_string()
                } else {
                    row.glyph.clone()
                };
                self.font.draw(
                    canvas,
                    &glyph,
                    pad,
                    centre + (theme.font.size_menu / 3.0) as i32,
                    theme.font.size_menu,
                    theme.color.muted,
                );
            }
            let label_x = pad + icon_size as i32 + 12;
            let label = self.font.elide(
                &row.label,
                theme.font.size_menu,
                (split - label_x - 8) as u32,
            );
            self.font.draw(
                canvas,
                &label,
                label_x,
                centre + (theme.font.size_menu / 3.0) as i32,
                theme.font.size_menu,
                if focused {
                    theme.color.accent
                } else {
                    theme.color.foreground
                },
            );
        }

        // The side column is its own surface, not three boxes floating on the
        // list's background.
        let strip = canvas.width as i32 - split;
        canvas.rect(
            split,
            top,
            strip as u32,
            (bottom - top) as u32,
            theme.color.background_alt,
        );
        canvas.rect(split, top, 1, (bottom - top) as u32, theme.color.border);

        let gap = 8;
        let inset = 8;
        let width = strip - inset * 2;
        let height = ((bottom - top) - gap * (SIDE.len() as i32 + 1)) / SIDE.len() as i32;
        for (index, button) in SIDE.iter().enumerate() {
            let x = split + inset;
            let y = top + gap + index as i32 * (height + gap);
            let focused = self.side == Some(index);
            let tint = (button.tint)(&theme.color);
            let centre = y + height / 2;

            canvas.rounded_rect(
                x,
                y,
                width as u32,
                height as u32,
                2,
                if focused { tint } else { theme.color.border },
            );
            canvas.rounded_rect(
                x + 1,
                y + 1,
                (width - 2) as u32,
                (height - 2) as u32,
                2,
                theme.color.background,
            );
            // A bar down the edge, so which one is picked reads from the corner
            // of your eye.
            canvas.rect(x + 1, y + 1, 3, (height - 2) as u32, tint);

            let size = 28;
            if let Some(icon) = self.icons.get(button.icon, size) {
                icon.draw(
                    canvas,
                    x + (width - size as i32) / 2,
                    centre - size as i32 / 2 - 8,
                );
            }
            let track = theme.font.tracking;
            let label = button.label.to_uppercase();
            let tw = self
                .mono_bold
                .measure_tracked(&label, theme.font.size_hint, track) as i32;
            self.mono_bold.draw_tracked(
                canvas,
                &label,
                x + (width - tw) / 2,
                centre + 24,
                theme.font.size_hint,
                if focused { tint } else { theme.color.muted },
                track,
            );
        }
    }

    /// The quick panel: a switch, a slider or a readout per row, and the ways
    /// out drawn as chips along the bottom.
    fn draw_quick(&mut self, canvas: &mut Canvas, top: i32, bottom: i32) {
        let theme = self.theme.clone();
        let pad = theme.menu.padding_x as i32;
        let rows = self.model.visible_rows();
        let cursor = self.model.screen().list.cursor_index();
        self.row_hits.clear();

        let chips: Vec<usize> = rows
            .iter()
            .enumerate()
            .filter(|(_, r)| r.glyph == "button")
            .map(|(i, _)| i)
            .collect();
        let chip_h = 44;
        let chip_top = bottom - chip_h;
        let lines = rows.len() - chips.len();
        let row_h = ((chip_top - top - 8) / lines.max(1) as i32).min(56);
        let icon_size = 24;

        for (index, row) in rows.iter().enumerate() {
            if chips.contains(&index) {
                continue;
            }
            let y = top + index as i32 * row_h;
            let focused = index == cursor;
            self.row_hits.push((0, y, canvas.width as i32, y + row_h));
            if focused {
                canvas.rect(0, y, canvas.width, row_h as u32, theme.color.background_alt);
                canvas.rect(0, y, 3, row_h as u32, theme.color.accent);
            }
            let centre = y + row_h / 2;
            if let Some(icon) = self.icons.get_symbolic(&row.icon, icon_size) {
                icon.draw_tinted(
                    canvas,
                    pad,
                    centre - icon_size as i32 / 2,
                    if focused {
                        theme.color.accent
                    } else {
                        theme.color.muted
                    },
                );
            }
            let label_x = pad + icon_size as i32 + 12;
            self.font.draw(
                canvas,
                &row.label,
                label_x,
                centre + (theme.font.size_menu / 3.0) as i32,
                theme.font.size_menu,
                theme.color.foreground,
            );

            let right = canvas.width as i32 - pad;
            match row.glyph.split(':').next().unwrap_or("") {
                "switch" => {
                    let on = row.glyph.ends_with("on");
                    let w = 62;
                    let h = 26;
                    let x = right - w;
                    let colour = if on {
                        theme.color.accent
                    } else {
                        theme.color.border
                    };
                    canvas.rounded_rect(x, centre - h / 2, w as u32, h as u32, 2, colour);
                    let text = if on { "ON" } else { "OFF" };
                    let tw = self.mono_bold.measure(text, theme.font.size_hint) as i32;
                    self.mono_bold.draw(
                        canvas,
                        text,
                        x + (w - tw) / 2,
                        centre + 5,
                        theme.font.size_hint,
                        if on {
                            theme.color.accent_fg
                        } else {
                            theme.color.muted
                        },
                    );
                    self.note(canvas, &row.note, x - 10, centre);
                }
                "bar" => {
                    let value: i32 = row
                        .glyph
                        .split(':')
                        .nth(1)
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(0);
                    let note_w = self.mono.measure(&row.note, theme.font.size_hint) as i32;
                    let bar_x = label_x + 130;
                    let bar_w = right - note_w - 12 - bar_x;
                    canvas.rect(bar_x, centre - 5, bar_w as u32, 10, theme.color.border);
                    let filled = bar_w * value.clamp(0, 100) / 100;
                    let colour = match value {
                        85..=100 => theme.color.critical,
                        60..=84 => theme.color.warning,
                        _ => theme.color.accent,
                    };
                    canvas.rect(bar_x, centre - 5, filled as u32, 10, colour);
                    self.mono.draw(
                        canvas,
                        &row.note,
                        right - note_w,
                        centre + 5,
                        theme.font.size_hint,
                        theme.color.foreground,
                    );
                }
                "slide" => {
                    let value: i32 = row
                        .glyph
                        .split(':')
                        .nth(1)
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(0);
                    let note_w = self.mono.measure(&row.note, theme.font.size_hint) as i32;
                    let track_x = label_x + 130;
                    let track_w = right - note_w - 12 - track_x;
                    canvas.rect(track_x, centre - 2, track_w as u32, 4, theme.color.border);
                    let filled = track_w * value.clamp(0, 100) / 100;
                    canvas.rect(track_x, centre - 2, filled as u32, 4, theme.color.accent);
                    canvas.rounded_rect(
                        track_x + filled - 4,
                        centre - 9,
                        8,
                        18,
                        2,
                        theme.color.accent,
                    );
                    self.mono.draw(
                        canvas,
                        &row.note,
                        right - note_w,
                        centre + 5,
                        theme.font.size_hint,
                        theme.color.foreground,
                    );
                }
                _ => self.note(canvas, &row.note, right, centre),
            }
        }

        // The ways out, side by side.
        if !chips.is_empty() {
            let gap = 10;
            let width = (canvas.width as i32 - pad * 2 - gap * (chips.len() as i32 - 1))
                / chips.len() as i32;
            for (slot, index) in chips.iter().enumerate() {
                let row = &rows[*index];
                let x = pad + slot as i32 * (width + gap);
                let focused = *index == cursor;
                self.row_hits
                    .push((x, chip_top, x + width, chip_top + chip_h));
                let colour = if focused {
                    theme.color.accent
                } else {
                    theme.color.border
                };
                canvas.rounded_rect(x, chip_top, width as u32, chip_h as u32, 2, colour);
                canvas.rounded_rect(
                    x + 1,
                    chip_top + 1,
                    (width - 2) as u32,
                    (chip_h - 2) as u32,
                    2,
                    theme.color.background,
                );
                let size = theme.font.size_hint + 2.0;
                let tw = self.bold.measure(&row.label, size) as i32;
                self.bold.draw(
                    canvas,
                    &row.label,
                    x + (width - tw) / 2,
                    chip_top + chip_h / 2 + 6,
                    size,
                    if focused {
                        theme.color.accent
                    } else {
                        theme.color.foreground
                    },
                );
            }
        }
    }

    /// Right-aligned readout next to a switch.
    fn note(&mut self, canvas: &mut Canvas, note: &str, right: i32, centre: i32) {
        if note.is_empty() {
            return;
        }
        let size = self.theme.font.size_hint;
        let width = self.mono.measure(note, size) as i32;
        self.mono.draw(
            canvas,
            note,
            right - width,
            centre + 5,
            size,
            self.theme.color.muted,
        );
    }

    fn draw_hints(&mut self, canvas: &mut Canvas, top: i32) {
        let theme = &self.theme;
        let height = theme.menu.hint_height;
        canvas.rect(0, top, canvas.width, height, theme.color.background_alt);
        canvas.rect(0, top, canvas.width, 1, theme.color.border);

        let filtering = self.model.screen().list.mode() == Mode::Filter;
        let on_switcher = matches!(
            self.model.screen().source,
            crate::model::Source::Dynamic {
                builtin: pt35_common::menu::Builtin::Windows,
                ..
            }
        );
        let on_quick_setting = self.model.focused_adjust().is_some();
        let hints: &[Hint] = match (filtering, on_switcher, on_quick_setting) {
            (true, _, _) => FILTER_HINTS,
            (false, true, _) => WINDOW_HINTS,
            (false, false, true) => QUICK_HINTS,
            (false, false, false) => NAV_HINTS,
        };
        // L/R page through a list. Saying so when everything already fits is a
        // promise the screen does not keep.
        let paged = self.model.screen().list.len() > self.model.screen().list.rows();
        let hints: Vec<Hint> = hints
            .iter()
            .filter(|hint| paged || hint.button != "L/R")
            .cloned()
            .collect();
        let size = theme.font.size_hint;
        let baseline = top + (height as f32 * 0.62) as i32;
        let centre = top + height as i32 / 2;

        // Lay the chips out evenly across the full width: they are touch targets
        // as much as a legend.
        let slot = canvas.width as f32 / hints.len().max(1) as f32;
        self.hint_hits.clear();
        for (index, hint) in hints.iter().enumerate() {
            let slot_x = (index as f32 * slot) as i32;
            let mut x = slot_x + 7;
            self.hint_hits
                .push((slot_x, top, slot_x + slot as i32, hint.button));
            let (pill, ink) = self.button_colors(hint.button);
            let label_w = self.bold.measure(hint.button, size) as i32;
            let pill_w = (label_w + 14).max(24);
            canvas.rounded_rect(x, centre - 12, pill_w as u32, 24, 12, pill);
            self.bold.draw(
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
        use pt35_ui::keys::{navigate, Navigation};
        if self.on_launcher() {
            let mode = self.model.screen().list.mode();
            match (self.side, navigate(&key, mode)) {
                // Right leaves the app list for the side buttons, left comes
                // back. Nothing else about the launcher is two-dimensional.
                (None, Navigation::Right) => {
                    self.side = Some(0);
                    return true;
                }
                (Some(_), Navigation::Left) | (Some(_), Navigation::Back) => {
                    self.side = None;
                    return true;
                }
                (Some(index), Navigation::Down) => {
                    self.side = Some((index + 1) % SIDE.len());
                    return true;
                }
                (Some(index), Navigation::Up) => {
                    self.side = Some((index + SIDE.len() - 1) % SIDE.len());
                    return true;
                }
                (Some(index), Navigation::Activate) => return self.open_side(index),
                _ => {}
            }
        }
        let step = self.model.handle(&key);
        self.apply(step)
    }

    fn touch(&mut self, x: f64, y: f64) -> bool {
        let (x, y) = (x as i32, y as i32);
        for (left, top, right, button) in self.hint_hits.clone() {
            if y >= top && x >= left && x < right {
                return self.press(button);
            }
        }
        for (index, (left, top, right, bottom)) in self.row_hits.clone().into_iter().enumerate() {
            if x >= left && x < right && y >= top && y < bottom {
                let step = self.model.activate_window(index);
                return self.apply(step);
            }
        }
        true
    }

    fn draw(&mut self, canvas: &mut Canvas) {
        let mut hint_top = canvas.height as i32 - self.theme.menu.hint_height as i32;
        if let Some(message) = self.error.clone() {
            let size = self.theme.font.size_hint;
            let height = 26;
            let y = hint_top - height;
            canvas.rect(0, y, canvas.width, height as u32, self.theme.color.critical);
            let text =
                self.font
                    .elide(&message, size, canvas.width - 2 * self.theme.menu.padding_x);
            self.font.draw(
                canvas,
                &text,
                self.theme.menu.padding_x as i32,
                y + 18,
                size,
                self.theme.color.background,
            );
            hint_top = y;
        }
        let body_top = self.draw_header(canvas) + 8;
        let quick = matches!(
            self.model.screen().source,
            crate::model::Source::Dynamic {
                builtin: pt35_common::menu::Builtin::Quick | pt35_common::menu::Builtin::System,
                ..
            }
        );
        if quick {
            self.draw_quick(canvas, body_top, hint_top - 4);
        } else if self.on_launcher() {
            self.draw_launcher(canvas, body_top, hint_top - 4);
        } else {
            match self.model.layout() {
                Layout::Grid => self.draw_tiles(canvas, body_top, hint_top - 4),
                Layout::List => self.draw_rows(canvas, body_top, hint_top),
            }
        }
        self.draw_hints(canvas, hint_top);
    }
}

pub fn run(page: Option<String>) -> Result<()> {
    let theme: Theme = pt35_common::load_config("pt35/theme.toml").unwrap_or_default();
    let tree = pt35_common::load_config("pt35/menu.toml").unwrap_or_default();
    let font = Font::load(&theme.font.family)?;
    let mono = Font::load(&theme.font.family_mono).or_else(|_| Font::load(&theme.font.family))?;
    // A missing bold face is not worth failing over: the regular one reads.
    let bold = Font::load(&theme.font.family_bold).or_else(|_| Font::load(&theme.font.family))?;
    let mono_bold = Font::load(&theme.font.family_mono_bold)
        .or_else(|_| Font::load(&theme.font.family_mono))
        .or_else(|_| Font::load(&theme.font.family))?;
    let rows = theme.menu.rows_visible as usize;
    let body = 480 - theme.bar.height - theme.menu.header_height - theme.menu.hint_height;
    let grid_rows = (body / (theme.menu.tile_height + theme.menu.gap)).max(1) as usize;
    // A screen name may be a builtin rather than a page in menu.toml, and that
    // includes the root: the launcher is assembled, not written down.
    let tree: pt35_common::menu::MenuTree = tree;
    let root = pt35_common::menu::Builtin::from_name(&tree.root);
    let mut model = Model::sized(tree, None, rows, grid_rows, theme.menu.columns as usize);
    if let Some(builtin) = root {
        model.set_root_dynamic(
            builtin,
            providers::title(builtin),
            providers::items(builtin),
        );
    }
    match page.as_deref() {
        None => {}
        Some(name) => match pt35_common::menu::Builtin::from_name(name) {
            Some(builtin) if Some(builtin) != root => {
                model.push_dynamic(
                    builtin,
                    providers::title(builtin),
                    providers::items(builtin),
                );
            }
            Some(_) => {}
            None => model.open_page(name),
        },
    }
    layer::run(
        Menu::new(theme, font, mono, bold, mono_bold, model),
        SurfaceSpec::overlay("pt35-menu"),
    )
}
