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
use std::sync::mpsc::Receiver;
use std::time::Duration;

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

/// How often the menu wakes up.
const TICK: Duration = Duration::from_millis(250);
/// How many of those ticks go by between reads of the daemon.
const POLL_TICKS: u8 = 4;

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

// No Start anywhere but the search legend. B goes back and, at the top screen,
// back means out: two keys for one job is one key wasted on a device with
// twelve of them.
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
    // Searching three tiles you can see is not worth a key. Closing all of
    // them is, and it asks first.
    Hint {
        button: "X",
        action: "Close all",
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
];

// Start here clears the search rather than closing the menu: the list decides
// that before the model ever sees the key.
// Start earns its place here: it drops the whole filter at once, which
// Backspace only does one character at a time.
const FILTER_HINTS: &[Hint] = &[
    Hint {
        button: "Enter",
        action: "Open",
    },
    Hint {
        button: "^v",
        action: "Move",
    },
    Hint {
        button: "Start",
        action: "Clear",
    },
    Hint {
        button: "Bksp",
        action: "Exit",
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
    /// `(left, top, right, bottom, row index)`. The index is carried rather
    /// than inferred from the hit box's position in this list.
    row_hits: Vec<(i32, i32, i32, i32, usize)>,
    /// The launcher's side column, same idea. Touch is the way back in when the
    /// RP2040 that owns the keyboard drops off the USB bus, and Windows,
    /// Settings and Power were unreachable that way.
    side_hits: Vec<(i32, i32, i32, i32, usize)>,
    hint_hits: Vec<(i32, i32, i32, &'static str)>,
    error: Option<String>,
    /// A slow provider still running on a worker thread, and the screen it
    /// belongs to. The menu draws a placeholder until it lands.
    loading: Option<(pt35_common::menu::Builtin, Receiver<providers::Items>)>,
    /// Ticks since the daemon was last asked. See [`Menu::tick`].
    since_poll: u8,
    /// App profiles, for matching a launcher row to an open window.
    apps: pt35_common::apps::AppTable,
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
            side_hits: Vec::new(),
            hint_hits: Vec::new(),
            error: None,
            loading: None,
            since_poll: 0,
            apps: pt35_common::load_config("pt35/apps.toml").unwrap_or_default(),
        }
    }

    /// Whether a launcher row already has a window open.
    ///
    /// The daemon focuses rather than duplicating when you pick one of these,
    /// so the row says so before you press it.
    fn is_running(&self, payload: &str) -> bool {
        let Some(status) = self.status.as_ref() else {
            return false;
        };
        let Some(binary) = payload
            .strip_prefix("exec:")
            .and_then(pt35_common::apps::command_binary)
            .map(|b| b.rsplit('/').next().unwrap_or(&b).to_lowercase())
        else {
            return match payload.strip_prefix("app:") {
                Some(id) => self
                    .apps
                    .get(id)
                    .is_some_and(|app| status.windows.iter().any(|w| app.matches_app(&w.app))),
                None => false,
            };
        };
        status
            .windows
            .iter()
            .any(|w| w.app.to_lowercase() == binary)
    }

    /// True when this menu is standing in for a desktop, because nothing else
    /// is open. It cannot be closed then: there would be nothing behind it.
    fn is_desktop(&self) -> bool {
        self.windows == 0 && self.model.depth() == 1
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

    /// Push a builtin screen, reading its rows off the Wayland thread when they
    /// are slow to come by.
    fn open(&mut self, builtin: pt35_common::menu::Builtin) {
        let screen = providers::screen(builtin);
        let Some(scanning) = screen.scanning else {
            self.model
                .push_dynamic(builtin, screen.title, (screen.rows)());
            return;
        };
        self.model
            .push_dynamic(builtin, screen.title, providers::placeholder(scanning));
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send((screen.rows)());
        });
        self.loading = Some((builtin, rx));
    }

    fn open_side(&mut self, index: usize) -> bool {
        self.side = None;
        match SIDE.get(index).map(|button| button.target) {
            Some(Side::Screen(builtin)) => {
                self.open(builtin);
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
    ///
    /// Every pill that is drawn is hit-tested and every one that is
    /// hit-tested does something.
    fn press(&mut self, button: &str) -> bool {
        let key = match button {
            "L/R" => Key::new(pt35_ui::keys::sym::PAGE_DOWN),
            "<>" => Key::new(pt35_ui::keys::sym::RIGHT),
            "Bksp" => Key::new(pt35_ui::keys::sym::BACKSPACE),
            "^v" => Key::new(pt35_ui::keys::sym::DOWN),
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

    /// Ask the daemon to do something and wait for the reply. Waiting is the
    /// point: the daemon has applied the change by the time it answers, so the
    /// value on screen can be re-read with no sleep.
    fn ctl(&mut self, args: &[&str]) -> Result<(), String> {
        let request = pt35_common::ipc::Request::from_ctl(args)?;
        match crate::live::request(&request) {
            Ok(pt35_common::ipc::Response::Error { message }) => Err(message),
            Ok(_) => Ok(()),
            Err(message) => Err(message),
        }
    }

    /// Re-read the daemon and rebuild the current screen from it.
    ///
    /// Both halves matter. The rows of a page screen read `self.status` as they
    /// draw, but a builtin screen's rows were built by its provider and are
    /// frozen: without the rebuild, toggling Wi-Fi or Touch leaves the pill on
    /// its old value, which is the opposite of watching it flip.
    fn resync(&mut self) {
        self.status = crate::live::status();
        self.windows = self.status.as_ref().map_or(0, |s| s.windows.len());
        if let Some(builtin) = self.model.dynamic_builtin() {
            self.model.replace_dynamic(providers::items(builtin));
        }
    }

    fn apply(&mut self, step: Step) -> bool {
        self.error = None;
        match step {
            Step::Close(payload) => {
                match payload.strip_prefix("con:").and_then(|id| id.parse().ok()) {
                    Some(id) => {
                        let request = pt35_common::ipc::Request::Window {
                            action: pt35_common::ipc::WindowAction::CloseId(id),
                        };
                        match crate::live::request(&request) {
                            Ok(pt35_common::ipc::Response::Error { message }) => {
                                self.error = Some(message)
                            }
                            Err(message) => self.error = Some(message),
                            Ok(_) => {}
                        }
                    }
                    None => self.error = Some("that row is not a window".into()),
                }
                // Take the row out now. sway answers `kill` before the client
                // has actually gone, so re-reading would show it still open. The
                // The tick puts it back if the app refuses to close.
                self.model.drop_dynamic(&payload);
                true
            }
            Step::Adjust(adjust, up) => {
                let args = adjust.step(up);
                let args: Vec<&str> = args.iter().map(String::as_str).collect();
                if let Err(message) = self.ctl(&args) {
                    self.error = Some(message);
                }
                self.resync();
                true
            }
            Step::RunStay(command) => {
                if let Err(message) = exec::perform(&command) {
                    self.error = Some(message);
                }
                // A shell payload is a detached script, so its effect lands after
                // it has been spawned. Everything else went through the daemon,
                // which had already applied it when it replied.
                if matches!(exec::plan(&command), exec::Plan::Shell(_)) {
                    std::thread::sleep(std::time::Duration::from_millis(120));
                }
                self.resync();
                true
            }
            // The menu is the desktop, so closing it means going back to the
            // top screen. There is nothing behind it to close onto.
            Step::Quit if self.windows == 0 => {
                let step = self.model.go_home();
                self.apply(step)
            }
            Step::Quit => {
                // Tell the daemon on the way out rather than leaving it to
                // notice on its next poll, which left every button dead for up
                // to two seconds after the menu had gone.
                let _ = crate::live::request(&pt35_common::ipc::Request::Menu {
                    action: pt35_common::ipc::Toggle::Off,
                    page: None,
                });
                false
            }
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
                self.open(builtin);
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
        // Nothing to search on a two-row yes or no.
        let searchable = !self.model.is_confirm();
        let (text, color) = if filtering {
            (format!("[{filter}_]"), theme.color.accent)
        } else if searchable {
            (theme.menu.filter_hint.to_uppercase(), theme.color.muted)
        } else {
            (String::new(), theme.color.muted)
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
            self.draw_nothing_here(canvas, top, canvas.width as i32);
            return;
        }

        let cursor = self.model.screen().list.cursor_row();
        let numbers = theme.menu.show_numbers && !self.model.is_confirm();
        let radius = theme.menu.radius;

        for (index, row) in rows.iter().enumerate() {
            let y = top + index as i32 * row_h;
            if y + row_h > bottom {
                break;
            }
            self.row_hits
                .push((0, y, canvas.width as i32, y + row_h, index));
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
        self.draw_scrollbar(canvas, canvas.width as i32, top, bottom);
    }

    /// What an empty screen says. Drawing nothing at all reads as broken
    /// rather than empty.
    fn draw_nothing_here(&mut self, canvas: &mut Canvas, top: i32, right: i32) {
        let filtering = self.model.screen().list.mode() == Mode::Filter;
        let text = if filtering {
            "no matches"
        } else if self.status.is_none() {
            // Not the same thing as nothing being there. A dead daemon returns
            // an empty list from every screen that asks it.
            "pt35d is not answering"
        } else {
            "nothing here"
        };
        let size = self.theme.font.size_menu;
        let pad = self.theme.menu.padding_x as i32;
        let colour = if self.status.is_none() && !filtering {
            self.theme.color.critical
        } else {
            self.theme.color.muted
        };
        let text = self.font.elide(text, size, (right - 2 * pad).max(0) as u32);
        self.font.draw(canvas, &text, pad, top + 40, size, colour);
    }

    /// The only thing that says a list goes on past the bottom of the screen.
    ///
    /// `edge` is the right-hand boundary of the list, which is the panel edge on
    /// most screens and the split on the launcher.
    fn draw_scrollbar(&mut self, canvas: &mut Canvas, edge: i32, top: i32, bottom: i32) {
        let Some(progress) = self.model.screen().list.scroll_progress() else {
            return;
        };
        let theme = &self.theme;
        let total = self.model.screen().list.len().max(1);
        let visible = self.model.screen().list.rows();
        let track_h = (bottom - top).max(1) as u32;
        let thumb_h = ((visible as f32 / total as f32) * track_h as f32)
            .max(24.0)
            .min(track_h as f32) as u32;
        let thumb_y = top + (progress.clamp(0.0, 1.0) * (track_h - thumb_h) as f32) as i32;
        canvas.rect(edge - 4, top, 2, track_h, theme.color.border);
        canvas.rounded_rect(edge - 5, thumb_y, 4, thumb_h, 2, theme.color.muted);
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
        if rows.is_empty() {
            self.row_hits.clear();
            self.draw_nothing_here(canvas, top, canvas.width as i32);
            return;
        }
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
            self.row_hits.push((x, y, x + tile_w, y + tile_h, index));
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
        if rows.is_empty() {
            self.draw_nothing_here(canvas, top, split);
        }
        for (index, row) in rows.iter().enumerate() {
            let y = top + index as i32 * row_h;
            if y + row_h > bottom {
                break;
            }
            self.row_hits.push((0, y, split, y + row_h, index));
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
            // A dot for an app that already has a window. The daemon focuses
            // rather than starting a second copy, and this is the only warning
            // you get before you press.
            let running = self.is_running(&row.payload);
            let label_right = if running { split - 18 } else { split - 8 };
            if running {
                canvas.rounded_rect(split - 14, centre - 3, 6, 6, 3, theme.color.accent);
            }

            let label_x = pad + icon_size as i32 + 12;
            let label = self.font.elide(
                &row.label,
                theme.font.size_menu,
                (label_right - label_x).max(0) as u32,
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

        self.draw_scrollbar(canvas, split, top, bottom);

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
        self.side_hits.clear();
        for (index, button) in SIDE.iter().enumerate() {
            let x = split + inset;
            let y = top + gap + index as i32 * (height + gap);
            // The whole strip is the target, not just the drawn pill: a thumb on
            // a 3.5" panel is wider than 8px of margin.
            self.side_hits.push((
                split,
                y - gap / 2,
                canvas.width as i32,
                y + height + gap / 2,
                index,
            ));
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
    /// out as ordinary rows: one column, one thing per line.
    fn draw_quick(&mut self, canvas: &mut Canvas, top: i32, bottom: i32) {
        let theme = self.theme.clone();
        let pad = theme.menu.padding_x as i32;
        let rows = self.model.visible_rows();
        let cursor = self.model.screen().list.cursor_index();
        self.row_hits.clear();
        if rows.is_empty() {
            self.draw_nothing_here(canvas, top, canvas.width as i32);
            return;
        }

        let row_h = ((bottom - top - 8) / rows.len().max(1) as i32).min(56);
        let icon_size = 24;

        for (index, row) in rows.iter().enumerate() {
            let y = top + index as i32 * row_h;
            let focused = index == cursor;
            self.row_hits
                .push((0, y, canvas.width as i32, y + row_h, index));
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
                // A row that leads somewhere says so, the same way a submenu
                // row in a plain list does.
                "nav" => {
                    let size = theme.font.size_menu;
                    let colour = if focused {
                        theme.color.accent
                    } else {
                        theme.color.muted
                    };
                    let w = self.font.measure(">", size) as i32;
                    self.font.draw(
                        canvas,
                        ">",
                        right - w,
                        centre + (size / 3.0) as i32,
                        size,
                        colour,
                    );
                }
                _ => self.note(canvas, &row.note, right, centre),
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
        let rooted = self.model.depth() == 1;
        let pinned = self.is_desktop();
        let hints: Vec<Hint> = hints
            .iter()
            .filter(|hint| paged || hint.button != "L/R")
            // At the top screen there is nowhere to go home to, and on the
            // desktop there is nothing to close the menu onto. A legend that
            // names a key which does nothing is worse than a shorter legend.
            .filter(|hint| !(rooted && hint.button == "Y"))
            .filter(|hint| !(pinned && hint.button == "B"))
            .filter(|hint| !self.model.is_confirm() || hint.button != "X")
            .map(|hint| {
                // At the top screen, back means out.
                if rooted && hint.button == "B" {
                    Hint {
                        button: "B",
                        action: "Close",
                    }
                } else {
                    *hint
                }
            })
            .collect();
        let size = theme.font.size_hint;
        let baseline = top + (height as f32 * 0.62) as i32;
        let centre = top + height as i32 / 2;

        // Each chip takes the width it needs and the slack is shared out
        // between them: even slots cut the last word off when the legend is
        // long, and "Clos..." is worse than a tighter gap.
        let widths: Vec<i32> = hints
            .iter()
            .map(|hint| {
                let pill = (self.bold.measure(hint.button, size) as i32 + 14).max(24);
                pill + 6 + self.font.measure(hint.action, size) as i32
            })
            .collect();
        let total: i32 = widths.iter().sum();
        let count = hints.len().max(1) as i32;
        let gap = ((canvas.width as i32 - total) / (count + 1)).max(4);
        let tight = total + gap * (count + 1) > canvas.width as i32;

        self.hint_hits.clear();
        let mut x = gap;
        for (index, hint) in hints.iter().enumerate() {
            let start = x;
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

            // Only when it cannot fit: elide the words, never the pill.
            let room = if tight {
                (canvas.width as i32 - x - 4).max(0) as u32
            } else {
                widths[index] as u32
            };
            let action = self.font.elide(hint.action, size, room);
            self.font
                .draw(canvas, &action, x, baseline, size, self.theme.color.muted);
            x += self.font.measure(&action, size) as i32;
            self.hint_hits.push((start, top, x + gap / 2, hint.button));
            x += gap;
        }
    }
}

impl App for Menu {
    fn background(&self) -> Rgb {
        self.theme.color.background
    }

    fn tick_interval(&self) -> Option<Duration> {
        // Fast enough that a finished Wi-Fi scan appears the moment it lands.
        // The daemon is only asked every fourth tick: a status read costs it a
        // sway tree walk, and four a second of that is not free on a Pi.
        Some(TICK)
    }

    fn tick(&mut self) -> bool {
        let mut drawn = false;
        // A slow provider finishing is worth a frame on its own.
        if let Some((builtin, rx)) = &self.loading {
            match rx.try_recv() {
                Ok(items) => {
                    if self.model.dynamic_builtin() == Some(*builtin) {
                        self.model.replace_dynamic(items);
                    }
                    self.loading = None;
                    drawn = true;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.loading = None;
                    self.error = Some("that screen could not be read".into());
                    drawn = true;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }
        // Re-read the daemon, so a window closing behind the menu or a toggle
        // flipping shows without reopening it.
        self.since_poll += 1;
        if self.since_poll < POLL_TICKS {
            return drawn;
        }
        self.since_poll = 0;
        let status = crate::live::status();
        if status == self.status {
            return drawn;
        }
        self.status = status;
        self.windows = self.status.as_ref().map_or(0, |s| s.windows.len());
        // A dynamic screen's rows were built by its provider and are frozen, so
        // a fresh Status is not enough on its own.
        if let Some(builtin) = self.model.dynamic_builtin() {
            if matches!(builtin, pt35_common::menu::Builtin::Windows) {
                self.model.replace_dynamic(providers::items(builtin));
            }
        }
        true
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
        for (left, top, right, bottom, index) in self.side_hits.clone() {
            if x >= left && x < right && y >= top && y < bottom {
                return self.open_side(index);
            }
        }
        for (left, top, right, bottom, index) in self.row_hits.clone() {
            if x < left || x >= right || y < top || y >= bottom {
                continue;
            }
            // A slider is dragged with the D-pad, so a tap on one has to mean
            // something too: the left half turns it down and the right half up.
            if let Some(adjust) = self.model.adjust_at(index) {
                let up = x > (left + right) / 2;
                return self.apply(Step::Adjust(adjust, up));
            }
            let step = self.model.activate_window(index);
            return self.apply(step);
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
        if !self.on_launcher() {
            self.side_hits.clear();
        }
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
    let body = 480 - theme.bar.height - theme.menu.header_height - theme.menu.hint_height;
    // How many rows actually fit, not how many the theme asks for: a list that
    // hands out one row more than the body can draw hides the cursor on it.
    let rows =
        ((body - 12) / theme.menu.row_height).clamp(1, theme.menu.rows_visible.max(1)) as usize;
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
