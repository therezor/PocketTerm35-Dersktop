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
/// A right-click menu: its rows, as a label and the button each one stands
/// for, and where they were drawn.
struct Popup {
    x: i32,
    y: i32,
    items: Vec<(&'static str, &'static str)>,
    hot: usize,
    rects: Vec<(i32, i32, i32, i32)>,
}

/// Height of one row of the right-click menu.
const POPUP_ROW: i32 = 40;

/// How far left of the drawn scrollbar still counts as on it.
const SCROLLBAR_SLOP: i32 = 12;
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
];

const WINDOW_HINTS: &[Hint] = &[
    Hint {
        button: "<>",
        action: "Move",
    },
    Hint {
        button: "A",
        action: "Focus",
    },
    Hint {
        button: "B",
        action: "Back",
    },
    // Searching three tiles you can see is not worth a key. Closing all of
    // them is, and it asks first.
    Hint {
        button: "X",
        action: "Close all",
    },
    Hint {
        button: "Y",
        action: "Close",
    },
];

const MOUSE_HINTS: &[Hint] = &[
    Hint {
        button: "A",
        action: "Click",
    },
    Hint {
        button: "B",
        action: "Options",
    },
    Hint {
        button: "X/Y",
        action: "Scroll",
    },
    Hint {
        button: "R",
        action: "Buttons mode",
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
    /// Nothing is open on the workspace behind the menu.
    desktop: bool,
    /// Hit boxes recorded by the last draw, so touch never has to re-derive
    /// the layout and drift from it.
    /// `(left, top, right, bottom, row index)`. The index is carried rather
    /// than inferred from the hit box's position in this list.
    row_hits: Vec<(i32, i32, i32, i32, usize)>,
    /// Where the switcher's strip is held while the pointer drives it. `None`
    /// off the switcher.
    strip_scroll: Option<i32>,
    /// The last thing that moved the selection was the pointer, not a key.
    pointer_driven: bool,
    /// The right-click menu, while it is up.
    popup: Option<Popup>,
    /// The header's breadcrumbs: `(left, right, depth)`.
    crumb_hits: Vec<(i32, i32, usize)>,
    /// The scrollbar as last drawn: `(left, right, top, bottom, thumb height)`,
    /// hit box included. `None` when everything fits.
    scrollbar: Option<(i32, i32, i32, i32, i32)>,
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
    motion: Motion,
    /// The window last reported to pt35d as under the switcher's cursor.
    hover_sent: Option<i64>,
    /// Whether pt35d was last told the launcher is in front.
    launcher_sent: Option<(bool, Option<String>)>,
    /// A Bluetooth scan is running until then; the list is re-read meanwhile.
    scan_until: Option<std::time::Instant>,
    /// Switcher previews read so far, by window id, with the file's mtime so
    /// a fresher capture replaces them.
    previews: std::collections::HashMap<i64, Preview>,
}

/// One window's picture, as pt35d captured it.
struct Preview {
    modified: std::time::SystemTime,
    width: u32,
    height: u32,
    rgb: Vec<u8>,
}

fn preview_path(id: i64) -> std::path::PathBuf {
    pt35_common::paths::previews_dir().join(format!("{id}.ppm"))
}

fn read_preview(id: i64) -> Option<Preview> {
    let path = preview_path(id);
    let modified = std::fs::metadata(&path).ok()?.modified().ok()?;
    let (width, height, rgb) = pt35_ui::pixel::parse_ppm(&std::fs::read(&path).ok()?)?;
    Some(Preview {
        modified,
        width,
        height,
        rgb,
    })
}

/// The two movements the menu has: the selection gliding to its new row, and
/// a new screen settling in from a few pixels down. Both are over in about a
/// tenth of a second, and nothing is drawn for them once they are.
struct Motion {
    enabled: bool,
    screen: (usize, String),
    entered: std::time::Instant,
    from: Option<i32>,
    to: Option<i32>,
    moved: std::time::Instant,
}

const GLIDE: Duration = Duration::from_millis(90);
const ENTER: Duration = Duration::from_millis(120);
/// How far a new screen rises as it settles.
const ENTER_RISE: f32 = 12.0;

/// Ease-out: quick to start, soft to land.
fn ease(elapsed: Duration, span: Duration) -> f32 {
    let t = (elapsed.as_secs_f32() / span.as_secs_f32()).clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

impl Motion {
    fn new(enabled: bool) -> Self {
        let long_ago = std::time::Instant::now() - Duration::from_secs(1);
        Self {
            enabled,
            screen: (0, String::new()),
            entered: long_ago,
            from: None,
            to: None,
            moved: long_ago,
        }
    }

    /// Note which screen is up. A new one starts its rise and forgets the old
    /// selection, so nothing glides across from a different list.
    fn screen(&mut self, id: (usize, String)) {
        if id != self.screen {
            self.screen = id;
            self.entered = std::time::Instant::now();
            self.from = None;
            self.to = None;
        }
    }

    /// Pixels to push the body down while the screen settles.
    fn rise(&self) -> i32 {
        if !self.enabled {
            return 0;
        }
        ((1.0 - ease(self.entered.elapsed(), ENTER)) * ENTER_RISE) as i32
    }

    /// Where to draw the selection on its way to `target`.
    fn glide(&mut self, target: i32) -> i32 {
        if !self.enabled {
            return target;
        }
        if self.to != Some(target) {
            self.from = Some(self.current().unwrap_or(target));
            self.to = Some(target);
            self.moved = std::time::Instant::now();
        }
        self.current().unwrap_or(target)
    }

    fn current(&self) -> Option<i32> {
        let (from, to) = (self.from?, self.to?);
        let k = ease(self.moved.elapsed(), GLIDE);
        Some(from + ((to - from) as f32 * k) as i32)
    }

    fn busy(&self) -> bool {
        self.enabled && (self.moved.elapsed() < GLIDE || self.entered.elapsed() < ENTER)
    }
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
        // With the pt35 firmware the face buttons are F-keys, so a letter is
        // a letter: typing never presses a button.
        let pt35_firmware = status.as_ref().is_some_and(|s| s.pt35_firmware);
        pt35_ui::keys::set_letters_are_buttons(!pt35_firmware);
        let icons = pt35_ui::icon::Icons::new(&theme.icons.theme);
        let theme_animations = theme.menu.animations;
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
            desktop: status.as_ref().is_none_or(|s| s.workspace_empty()),
            status,
            row_hits: Vec::new(),
            strip_scroll: None,
            pointer_driven: false,
            popup: None,
            crumb_hits: Vec::new(),
            scrollbar: None,
            side_hits: Vec::new(),
            hint_hits: Vec::new(),
            error: None,
            loading: None,
            since_poll: 0,
            apps: pt35_common::load_config("pt35/apps.toml").unwrap_or_default(),
            motion: Motion::new(theme_animations),
            hover_sent: None,
            launcher_sent: None,
            scan_until: None,
            previews: std::collections::HashMap::new(),
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

    /// True when this menu is standing in for a desktop, because nothing is
    /// open behind it. It cannot be closed then: there would be nothing there.
    fn is_desktop(&self) -> bool {
        self.desktop && self.model.depth() == 1
    }

    /// A screen you read and do not press: System, About.
    fn readonly(&self) -> bool {
        self.model
            .dynamic_builtin()
            .is_some_and(|builtin| providers::screen(builtin).readonly)
    }

    /// The launcher row Y would pin: an app, on the top screen, with no search
    /// open.
    fn pin_target(&self) -> Option<String> {
        let filtering = self.model.screen().list.mode() == Mode::Filter;
        if !self.on_launcher() || self.model.depth() != 1 || filtering || self.side.is_some() {
            return None;
        }
        self.model
            .selected_payload()
            .filter(|p| p.starts_with("app:") || p.starts_with("exec:"))
            .map(str::to_string)
    }

    /// What a right click on the selected row offers, as a label and the
    /// button that does it. Empty when a plain click is all the row has.
    fn row_options(&self) -> Vec<(&'static str, &'static str)> {
        if let Some(payload) = self.pin_target() {
            let pinned = crate::providers::pins().contains(&payload);
            return vec![("Open", "A"), (if pinned { "Unpin" } else { "Pin" }, "Y")];
        }
        if self.model.dynamic_builtin() == Some(pt35_common::menu::Builtin::Windows) {
            return match self.model.selected_payload() {
                Some(p) if p != providers::HOME => {
                    vec![("Switch to", "A"), ("Close", "Y"), ("Close all", "X")]
                }
                _ => Vec::new(),
            };
        }
        Vec::new()
    }

    /// The right-click menu, drawn last so it sits over everything. It opens
    /// at the pointer and moves in to stay on screen.
    fn draw_popup(&mut self, canvas: &mut Canvas) {
        let Some(mut popup) = self.popup.take() else {
            return;
        };
        let theme = self.theme.clone();
        let size = theme.font.size_hint;
        let pad = 14;
        let mut widest = 0;
        for (label, _) in &popup.items {
            widest = widest.max(self.font.measure(label, size) as i32);
        }
        let width = widest + pad * 2;
        let width = width.max(140);
        let height = POPUP_ROW * popup.items.len() as i32;
        let x = popup.x.min(canvas.width as i32 - width - 2).max(2);
        let y = popup.y.min(canvas.height as i32 - height - 2).max(2);
        let radius = theme.menu.radius;
        canvas.rounded_rect(
            x - 1,
            y - 1,
            (width + 2) as u32,
            (height + 2) as u32,
            radius,
            theme.color.accent,
        );
        canvas.rounded_rect(
            x,
            y,
            width as u32,
            height as u32,
            radius,
            theme.color.background_alt,
        );
        popup.rects.clear();
        for (i, (label, _)) in popup.items.iter().enumerate() {
            let top = y + i as i32 * POPUP_ROW;
            let ink = if i == popup.hot {
                canvas.rect(x, top, width as u32, POPUP_ROW as u32, theme.color.accent);
                theme.color.accent_fg
            } else {
                theme.color.foreground
            };
            let baseline = top + (POPUP_ROW as f32 * 0.64) as i32;
            self.font.draw(canvas, label, x + pad, baseline, size, ink);
            popup.rects.push((x, top, x + width, top + POPUP_ROW));
        }
        self.popup = Some(popup);
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

    /// Open a screen from inside the menu. The switcher then starts on the
    /// Launcher card: the launcher is the "window" you were in.
    fn open_here(&mut self, builtin: pt35_common::menu::Builtin) {
        self.open(builtin);
        if builtin == pt35_common::menu::Builtin::Windows {
            self.model.screen_mut().list.select_item(0);
        }
    }

    fn open_side(&mut self, index: usize) -> bool {
        self.side = None;
        match SIDE.get(index).map(|button| button.target) {
            Some(Side::Screen(builtin)) => {
                self.open_here(builtin);
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
            "<>" => Key::new(pt35_ui::keys::sym::RIGHT),
            "Bksp" => Key::new(pt35_ui::keys::sym::BACKSPACE),
            "^v" => Key::new(pt35_ui::keys::sym::DOWN),
            // The button's own keysym, not its letter: with the pt35
            // firmware a letter is only a letter.
            "A" => Key::new(pt35_ui::keys::sym::BUTTON_A),
            "B" => Key::new(pt35_ui::keys::sym::BUTTON_B),
            "X" => Key::new(pt35_ui::keys::sym::BUTTON_X),
            "Y" => Key::new(pt35_ui::keys::sym::BUTTON_Y),
            "R" => Key::new(pt35_ui::keys::sym::BUTTON_R),
            "X/Y" => Key::new(pt35_ui::keys::sym::DOWN),
            "Start" => Key::new(pt35_ui::keys::sym::PAUSE),
            "Enter" => Key::new(pt35_ui::keys::sym::RETURN),
            _ => return true,
        };
        // Through `key`, so a pill does what its button does: R, L and Select
        // are handled there, before the list sees them.
        App::key(self, key)
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
        self.desktop = self.status.as_ref().is_none_or(|s| s.workspace_empty());
        let Some(builtin) = self.model.dynamic_builtin() else {
            return;
        };
        let screen = providers::screen(builtin);
        if screen.scanning.is_none() {
            self.model.replace_dynamic((screen.rows)());
            return;
        }
        // A slow screen reads off the Wayland thread, as when it opened. The
        // old rows stay up meanwhile: a radio that has just been switched on
        // has news worth the wait.
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            if matches!(builtin, pt35_common::menu::Builtin::Bluetooth) {
                // `power on` returns before the adapter says so.
                std::thread::sleep(Duration::from_millis(800));
            }
            let _ = tx.send((screen.rows)());
        });
        self.loading = Some((builtin, rx));
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
                        // Counted now, not at the next poll: closing the last
                        // window here makes this menu the desktop, and B must
                        // not leave it.
                        if let Some(status) = self.status.as_mut() {
                            status.windows.retain(|w| w.id != id);
                            self.desktop = status.workspace_empty();
                        }
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
                // Counted now, not at the next poll: closing the last window
                // makes this menu the desktop, and B must not leave it.
                self.windows = self.windows.saturating_sub(1);
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
                if matches!(&command, crate::model::Command::Helper(c) if c.contains("bluetooth scan"))
                {
                    self.scan_until = Some(std::time::Instant::now() + Duration::from_secs(13));
                }
                if matches!(command, crate::model::Command::Theme(_)) {
                    self.theme = pt35_common::load_theme().unwrap_or_default();
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
            Step::Quit if self.desktop => {
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
                // Tell pt35d now, as Quit does: otherwise it holds the menu's
                // bindings and the bar's state until its next poll.
                let _ = crate::live::request(&pt35_common::ipc::Request::Menu {
                    action: pt35_common::ipc::Toggle::Off,
                    page: None,
                });
                false
            }
            Step::Open(builtin) => {
                self.open_here(builtin);
                true
            }
            Step::TogglePin(payload) => {
                match providers::toggle_pin(&payload) {
                    Ok(pinned) => {
                        self.model.replace_dynamic(providers::items(
                            pt35_common::menu::Builtin::Launcher,
                        ));
                        // It just moved: keep the cursor on it.
                        self.model.focus_payload(&payload);
                        let _ = pinned;
                    }
                    Err(e) => self.error = Some(format!("pinning: {e}")),
                }
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
        let size = theme.font.size_title;
        self.crumb_hits.clear();
        // The path is the way back: PT35 is the top screen, and each screen
        // on the way is a link to itself. Only the one in front is plain.
        let trail: Vec<String> = self
            .model
            .trail()
            .iter()
            .map(|t| t.to_uppercase())
            .collect();
        let mut x = pad;
        let start = x;
        x = self
            .mono
            .draw_tracked(canvas, "PT35", x, baseline, size, theme.color.accent, track);
        if trail.len() > 1 {
            self.crumb_hits.push((start - 4, x + 4, 1));
        }
        let current = trail.last().cloned().unwrap_or_default();
        if trail.len() <= 1 {
            self.mono.draw_tracked(
                canvas,
                &format!(" // {current}"),
                x,
                baseline,
                size,
                theme.color.foreground,
                track,
            );
        } else {
            // The screens between the top and this one, fewest first to go
            // when the header runs out of room.
            let room = canvas.width as i32 * 2 / 3;
            let mut middle: Vec<(usize, String)> = trail[1..trail.len() - 1]
                .iter()
                .cloned()
                .enumerate()
                .map(|(i, t)| (i + 2, t))
                .collect();
            let mut widths: Vec<i32> = Vec::new();
            for (_, title) in &middle {
                widths.push(
                    self.mono
                        .measure_tracked(&format!(" < {title}"), size, track)
                        as i32,
                );
            }
            let tail =
                self.mono
                    .measure_tracked(&format!(" < {current}"), size, track) as i32;
            let mut elided = false;
            while !middle.is_empty() && x + widths.iter().sum::<i32>() + tail > room {
                middle.remove(0);
                widths.remove(0);
                elided = true;
            }
            let sep = theme.color.muted;
            if elided {
                x = self
                    .mono
                    .draw_tracked(canvas, " < ..", x, baseline, size, sep, track);
            }
            for (depth, title) in middle {
                x = self
                    .mono
                    .draw_tracked(canvas, " < ", x, baseline, size, sep, track);
                let left = x;
                x = self.mono.draw_tracked(
                    canvas,
                    &title,
                    x,
                    baseline,
                    size,
                    theme.color.accent,
                    track,
                );
                self.crumb_hits.push((left - 4, x + 4, depth));
            }
            x = self
                .mono
                .draw_tracked(canvas, " < ", x, baseline, size, sep, track);
            self.mono.draw_tracked(
                canvas,
                &current,
                x,
                baseline,
                size,
                theme.color.foreground,
                track,
            );
        }

        let filtering = self.model.screen().list.mode() == Mode::Filter;
        let filter = self.model.screen().list.filter().to_string();
        let list = &self.model.screen().list;
        // The legend already says X searches. What the header can add is where
        // you are in a list that runs off the screen.
        let (text, color) = if filtering && filter.is_empty() {
            (format!("[{}_]", theme.menu.filter_hint), theme.color.muted)
        } else if filtering {
            (format!("[{filter}_]"), theme.color.accent)
        } else if list.len() > list.rows() * list.columns().max(1) && !self.readonly() {
            (
                format!("{}/{}", list.selected().map_or(0, |i| i + 1), list.len()),
                theme.color.muted,
            )
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
        // App icons are drawn in their own colours, status icons in the text's.
        let full_colour =
            self.model.dynamic_builtin() == Some(pt35_common::menu::Builtin::DesktopEntries);
        let icon_size = 24;
        let has_icons = rows
            .iter()
            .any(|row| !row.icon.is_empty() || row.tint.is_some());
        let radius = theme.menu.radius;
        // The pill is drawn first and on its own, so it can be between rows.
        let pill = self.motion.glide(top + cursor as i32 * row_h);
        canvas.rounded_rect(
            pad / 2,
            pill + 3,
            canvas.width - pad as u32,
            (row_h - 6) as u32,
            radius,
            theme.color.accent,
        );

        for (index, row) in rows.iter().enumerate() {
            let y = top + index as i32 * row_h;
            if y + row_h > bottom {
                break;
            }
            self.row_hits
                .push((0, y, canvas.width as i32, y + row_h, index));
            // The row under the pill takes the pill's ink, even mid-glide.
            let selected = (pill - y).abs() < row_h / 2;
            if !selected && index + 1 < rows.len() {
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
            let centre = y + row_h / 2;
            if let Some(swatch) = row.tint {
                canvas.rounded_rect(x, centre - 10, 20, 20, radius, fg);
                canvas.rounded_rect(x + 2, centre - 8, 16, 16, radius, swatch);
            } else if full_colour {
                let name = self.icons.resolve(&[&row.icon], icon_size);
                if let Some(icon) = name.and_then(|name| self.icons.get(&name, icon_size)) {
                    icon.draw(canvas, x, centre - icon_size as i32 / 2);
                }
            } else if let Some(icon) = self.icons.get_symbolic(&row.icon, icon_size) {
                icon.draw_tinted(
                    canvas,
                    x,
                    centre - icon_size as i32 / 2,
                    if selected { fg } else { theme.color.muted },
                );
            } else if has_icons {
                // No such icon in the theme, or no theme: the letter stands in.
                let letter = row.label.chars().next().unwrap_or('?').to_string();
                let w = self.bold.measure(&letter, theme.font.size_hint) as i32;
                self.bold.draw(
                    canvas,
                    &letter,
                    x + (icon_size as i32 - w) / 2,
                    centre + (theme.font.size_hint / 3.0) as i32,
                    theme.font.size_hint,
                    dim,
                );
            }
            // Indented on every row or none, so labels stay in one column.
            if has_icons {
                x += icon_size as i32 + 12;
            }

            // Quick settings read out their value on the right, where the
            // chevron would be on a row that opens something.
            let right_text = match (row.adjust, row.state) {
                (Some(adjust), _) => Some(crate::live::value(adjust, self.status.as_ref())),
                (None, Some(field)) => Some(crate::live::state_value(field, self.status.as_ref())),
                (None, None) if row.submenu => Some(">".to_string()),
                (None, None) if !row.note.is_empty() => Some(row.note.clone()),
                (None, None) => None,
            };
            let from_note = row.adjust.is_none() && row.state.is_none() && !row.submenu;
            let mut right_width = 0;
            if let Some(text) = &right_text {
                let mono = row.adjust.is_some() || row.state.is_some() || !row.submenu;
                let size_right = if mono { theme.font.size_hint } else { size };
                let width = if mono {
                    self.mono.measure(text, size_right) as i32
                } else {
                    self.font.measure(text, size_right) as i32
                };
                let tx = canvas.width as i32 - pad - width;
                let colour = match (selected, from_note) {
                    (true, _) => fg,
                    (false, true) => theme.color.muted,
                    (false, false) => theme.color.accent,
                };
                if mono {
                    self.mono
                        .draw(canvas, text, tx, baseline, size_right, colour);
                } else {
                    self.font.draw(canvas, text, tx, baseline, size_right, dim);
                }
                right_width = width + 10;
            }
            // A tick for the one row that is current, left of its readout.
            let current_cpu = self
                .status
                .as_ref()
                .and_then(|s| s.cpu_profile)
                .is_some_and(|p| row.payload == format!("action:cpu {}", p.label()));
            if row.active || current_cpu {
                let tick = "\u{2713}";
                let w = self.bold.measure(tick, size) as i32;
                let colour = if selected { fg } else { theme.color.accent };
                right_width += w + 8;
                self.bold.draw(
                    canvas,
                    tick,
                    canvas.width as i32 - pad - right_width + 8,
                    baseline,
                    size,
                    colour,
                );
            }

            let room = canvas
                .width
                .saturating_sub(x as u32 + pad as u32 + right_width as u32);
            let label = self.font.elide(&row.label, size, room);
            // Red for a row that ends the session, so it reads as one before
            // the confirmation does.
            let ink = if row.confirm && !selected {
                theme.color.critical
            } else {
                fg
            };
            self.font.draw(canvas, &label, x, baseline, size, ink);
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
        let bar = theme.menu.scrollbar_width as i32;
        let left = edge - bar - 2;
        let radius = theme.menu.radius;
        canvas.rounded_rect(left, top, bar as u32, track_h, radius, theme.color.border);
        canvas.rounded_rect(
            left,
            thumb_y,
            bar as u32,
            thumb_h,
            radius,
            theme.color.muted,
        );
        // Wider than it is drawn: a bar a thumb can hit would crowd the rows.
        self.scrollbar = Some((left - SCROLLBAR_SLOP, edge, top, bottom, thumb_h as i32));
    }

    /// Put the list where the pointer is on the scrollbar, the thumb centred
    /// under it. The cursor comes along into view.
    fn scroll_to(&mut self, y: i32) -> bool {
        let Some((_, _, top, bottom, thumb)) = self.scrollbar else {
            return false;
        };
        let room = (bottom - top - thumb).max(1);
        let fraction = (y - top - thumb / 2) as f32 / room as f32;
        self.model.screen_mut().list.scroll_to(fraction)
    }

    fn on_scrollbar(&self, x: i32, y: i32) -> bool {
        self.scrollbar
            .is_some_and(|(l, r, t, b, _)| x >= l && x < r && y >= t && y < b)
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
            let resolved = self.icons.resolve(&[&row.icon], icon_size);
            let has_icon = resolved.is_some();
            if row.icon == providers::SKULL_ICON {
                canvas.rounded_rect(bx, by, badge as u32, badge as u32, theme.menu.radius, tint);
                pt35_ui::pixel::draw_centred(
                    canvas,
                    pt35_ui::pixel::SKULL,
                    bx + badge / 2,
                    by + badge / 2,
                    2,
                    theme.color.accent_fg,
                );
            } else if has_icon {
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
                let name = resolved.unwrap_or_default();
                if let Some(icon) = self.icons.get(&name, icon_size) {
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
        if self.side.is_none() && !rows.is_empty() {
            let y = self.motion.glide(top + cursor as i32 * row_h);
            canvas.rect(0, y, split as u32, row_h as u32, theme.color.background_alt);
            canvas.rect(0, y, 3, row_h as u32, theme.color.accent);
        }
        for (index, row) in rows.iter().enumerate() {
            let y = top + index as i32 * row_h;
            if y + row_h > bottom {
                break;
            }
            self.row_hits.push((0, y, split, y + row_h, index));
            let focused = index == cursor && self.side.is_none();
            let centre = y + row_h / 2;
            let name = self.icons.resolve(&[&row.icon], icon_size);
            let drawn = name
                .and_then(|name| self.icons.get(&name, icon_size))
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
            // Clear of the scrollbar, which sits against the split.
            let bar = theme.menu.scrollbar_width as i32;
            let mut label_right = if running {
                split - bar - 14
            } else {
                split - bar - 4
            };
            if running {
                canvas.rounded_rect(split - bar - 10, centre - 3, 6, 6, 3, theme.color.accent);
            }
            if row.active {
                let (w, _) = pt35_ui::pixel::size(pt35_ui::pixel::PIN, 2);
                let x = label_right - w as i32;
                pt35_ui::pixel::draw_centred(
                    canvas,
                    pt35_ui::pixel::PIN,
                    x + w as i32 / 2,
                    centre,
                    2,
                    theme.color.accent,
                );
                label_right = x - 6;
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
    /// The window switcher: one card per window, left to right in the same
    /// order as the taskbar, Launcher first where the skull button is. The
    /// strip scrolls to keep the selected card near the middle.
    fn draw_windows(&mut self, canvas: &mut Canvas, top: i32, bottom: i32) {
        let theme = self.theme.clone();
        let pad = theme.menu.padding_x as i32;
        let gap = theme.menu.gap as i32;
        let rows = self.model.visible_rows();
        let cursor = self.model.screen().list.cursor_index() as i32;
        self.row_hits.clear();
        if rows.is_empty() {
            self.draw_nothing_here(canvas, top, canvas.width as i32);
            return;
        }
        let width = canvas.width as i32;
        // Three across, so the cards either side say there is more.
        let card_w = (width - 2 * pad - 2 * gap) / 3;
        let dots = 18;
        let card_h = (bottom - top - dots - gap).min(card_w * 5 / 4);
        let card_y = top + (bottom - top - dots - card_h) / 2;
        let count = rows.len() as i32;
        let centre_of = |index: i32| pad + index * (card_w + gap) + card_w / 2;
        // The selected card is always in the middle, even at the ends of the
        // strip: where you are never depends on how many windows are open.
        //
        // Not while the pointer drives it: centring the card you point at
        // slides another one under the pointer, and that one is selected
        // next. Then the strip holds still until the selected card would
        // leave it.
        let target = match (self.pointer_driven, self.strip_scroll) {
            (true, Some(held)) => {
                let left = pad + cursor * (card_w + gap);
                held.clamp(left + card_w + pad - width, left - pad)
            }
            _ => centre_of(cursor) - width / 2,
        };
        self.strip_scroll = Some(target);
        let scroll = self.motion.glide(target);

        // Tell the bar which window the cursor is on, once per change.
        let hover = self.model.selected_payload().and_then(|p| {
            if p == providers::HOME {
                return Some(0);
            }
            p.strip_prefix("con:")?.parse().ok()
        });
        if hover != self.hover_sent {
            self.hover_sent = hover;
            let _ = crate::live::request(&pt35_common::ipc::Request::SwitcherHover { id: hover });
        }

        let icon_size = 48;
        let name_size = theme.font.size_menu * 0.85;
        let note_size = theme.font.size_hint * 0.85;
        let radius = theme.menu.radius;
        for (index, row) in rows.iter().enumerate() {
            let x = pad + index as i32 * (card_w + gap) - scroll;
            if x + card_w < 0 || x > width {
                continue;
            }
            self.row_hits
                .push((x, card_y, x + card_w, card_y + card_h, index));
            let focused = index as i32 == cursor;
            let edge = if focused {
                theme.color.accent
            } else {
                theme.color.border
            };
            let thick = if focused { 2 } else { 1 };
            canvas.rounded_rect(x, card_y, card_w as u32, card_h as u32, radius, edge);
            canvas.rounded_rect(
                x + thick,
                card_y + thick,
                (card_w - 2 * thick) as u32,
                (card_h - 2 * thick) as u32,
                radius,
                theme.color.background_alt,
            );

            let middle = x + card_w / 2;
            let icon_y = card_y + card_h / 3;
            let id = row
                .payload
                .strip_prefix("con:")
                .and_then(|id| id.parse::<i64>().ok());
            if let Some(id) = id.filter(|_| theme.menu.window_previews) {
                if let std::collections::hash_map::Entry::Vacant(slot) = self.previews.entry(id) {
                    if let Some(preview) = read_preview(id) {
                        slot.insert(preview);
                    }
                }
            }
            let preview = id
                .filter(|_| theme.menu.window_previews)
                .and_then(|id| self.previews.get(&id));
            if let Some(preview) = preview {
                // The picture fills the top of the card at the panel's shape,
                // with the app's icon in its corner so it still reads at a
                // glance.
                let pw = card_w - 16;
                let ph =
                    (pw * preview.height as i32 / preview.width.max(1) as i32).min(card_h * 3 / 5);
                let (px, py) = (x + 8, card_y + 8);
                canvas.rect(
                    px - 1,
                    py - 1,
                    (pw + 2) as u32,
                    (ph + 2) as u32,
                    theme.color.border,
                );
                canvas.blit_rgb(
                    px,
                    py,
                    pw as u32,
                    ph as u32,
                    &preview.rgb,
                    preview.width,
                    preview.height,
                );
                let small = theme.icons.size_bar.max(20);
                let name = self.icons.resolve(&[&row.icon], small);
                if let Some(icon) = name.and_then(|name| self.icons.get(&name, small)) {
                    let (ix, iy) = (px + 4, py + ph - small as i32 - 4);
                    canvas.rect(
                        ix - 2,
                        iy - 2,
                        small + 4,
                        small + 4,
                        theme.color.background_alt,
                    );
                    icon.draw(canvas, ix, iy);
                }
            } else if row.icon == providers::SKULL_ICON {
                pt35_ui::pixel::draw_centred(
                    canvas,
                    pt35_ui::pixel::SKULL,
                    middle,
                    icon_y,
                    3,
                    theme.color.accent,
                );
            } else {
                let name = self.icons.resolve(&[&row.icon], icon_size);
                if let Some(icon) = name.and_then(|name| self.icons.get(&name, icon_size)) {
                    icon.draw(
                        canvas,
                        middle - icon_size as i32 / 2,
                        icon_y - icon_size as i32 / 2,
                    );
                }
            }

            let room = (card_w - 16).max(0) as u32;
            let label = self.font.elide(&row.label, name_size, room);
            let label_w = self.font.measure(&label, name_size) as i32;
            let name_y = card_y + card_h * 2 / 3;
            let ink = if focused {
                theme.color.accent
            } else {
                theme.color.foreground
            };
            self.font
                .draw(canvas, &label, middle - label_w / 2, name_y, name_size, ink);
            if !row.note.is_empty() {
                let note = self.mono.elide(&row.note, note_size, room);
                let note_w = self.mono.measure(&note, note_size) as i32;
                self.mono.draw(
                    canvas,
                    &note,
                    middle - note_w / 2,
                    name_y + (note_size * 1.5) as i32,
                    note_size,
                    theme.color.muted,
                );
            }
        }

        // One dot per card: where you are when the strip runs off the edges.
        let dot = 6;
        let spacing = 12;
        let dots_w = count * spacing - (spacing - dot);
        let dots_x = (width - dots_w) / 2;
        let dots_y = card_y + card_h + gap;
        for index in 0..count {
            let colour = if index == cursor {
                theme.color.accent
            } else {
                theme.color.border
            };
            canvas.rect(
                dots_x + index * spacing,
                dots_y,
                dot as u32,
                dot as u32,
                colour,
            );
        }
    }

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
        let readonly = self.readonly();
        if !readonly {
            let y = self.motion.glide(top + cursor as i32 * row_h);
            canvas.rect(0, y, canvas.width, row_h as u32, theme.color.background_alt);
            canvas.rect(0, y, 3, row_h as u32, theme.color.accent);
        }

        for (index, row) in rows.iter().enumerate() {
            let y = top + index as i32 * row_h;
            let focused = index == cursor && !readonly;
            self.row_hits
                .push((0, y, canvas.width as i32, y + row_h, index));
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
            if readonly && index + 1 < rows.len() {
                canvas.rect(
                    pad,
                    y + row_h - 1,
                    canvas.width - 2 * pad as u32,
                    1,
                    theme.color.border,
                );
            }
        }
        self.draw_scrollbar(canvas, canvas.width as i32, top, bottom);
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
        let readonly = self.readonly();
        let empty = self.model.screen().list.is_empty();
        let mouse = self
            .status
            .as_ref()
            .is_some_and(|s| s.input_mode == pt35_common::ipc::InputMode::Mouse);
        let hints: &[Hint] = match (filtering, on_switcher, on_quick_setting) {
            (true, _, _) => FILTER_HINTS,
            // In Mouse mode the face buttons are the mouse, here as anywhere.
            _ if mouse => MOUSE_HINTS,
            (false, true, _) => WINDOW_HINTS,
            (false, false, true) => QUICK_HINTS,
            (false, false, false) => NAV_HINTS,
        };
        let rooted = self.model.depth() == 1;
        let back_closes = self.model.back_closes();
        // The launcher is never closed from inside it, desktop or not.
        let pinned = self.is_desktop() || (rooted && self.model.launcher_is_root());
        let on_home_tile = self.model.selected_payload() == Some(providers::HOME);
        let hints: Vec<Hint> = hints
            .iter()
            // At the top screen there is nowhere to go home to, and on the
            // desktop there is nothing to close the menu onto. A legend that
            // names a key which does nothing is worse than a shorter legend.
            .filter(|hint| !(rooted && hint.button == "Y"))
            .filter(|hint| !(pinned && hint.button == "B"))
            .filter(|hint| !(on_switcher && on_home_tile && hint.action == "Close"))
            .map(|hint| {
                if on_switcher && on_home_tile && hint.button == "A" {
                    Hint {
                        button: "A",
                        action: "Open",
                    }
                } else {
                    *hint
                }
            })
            .filter(|hint| !self.model.is_confirm() || hint.button != "X")
            // Nothing to open on a dashboard or an empty list, nothing to find
            // on a dashboard.
            .filter(|hint| !((readonly || empty) && !filtering && hint.button == "A"))
            .filter(|hint| !(readonly && hint.button == "X"))
            .map(|hint| {
                // At the top screen back means out. On the screen the menu was
                // opened on it is still "Back": back to your app.
                if rooted && !back_closes && hint.button == "B" {
                    Hint {
                        button: "B",
                        action: "Close",
                    }
                } else {
                    hint
                }
            })
            .collect();
        let mut hints = hints;
        // Y on a launcher row pins it: the one thing Y is free for at the top.
        // In Mouse mode Y is the wheel: a right click pins, from its menu.
        if !mouse && self.pin_target().is_some() {
            let pinned = self
                .model
                .visible_rows()
                .get(self.model.screen().list.cursor_index())
                .is_some_and(|row| row.active);
            hints.push(Hint {
                button: "Y",
                action: if pinned { "Unpin" } else { "Pin" },
            });
        }
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

    fn animating(&self) -> bool {
        self.motion.busy()
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
        // A capture pt35d took as the menu opened lands a moment after the
        // first frame. Drop pictures whose file has changed, so the next draw
        // reads the new one.
        if self.model.dynamic_builtin() == Some(pt35_common::menu::Builtin::Windows) {
            let stale: Vec<i64> = self
                .previews
                .iter()
                .filter(|(id, p)| {
                    std::fs::metadata(preview_path(**id))
                        .and_then(|m| m.modified())
                        .map_or(true, |m| m != p.modified)
                })
                .map(|(id, _)| *id)
                .collect();
            if !stale.is_empty() {
                for id in stale {
                    self.previews.remove(&id);
                }
                drawn = true;
            }
            // And a first capture, for a card that had none.
            let missing = self
                .model
                .visible_rows()
                .iter()
                .filter_map(|row| row.payload.strip_prefix("con:")?.parse::<i64>().ok())
                .any(|id| !self.previews.contains_key(&id) && preview_path(id).exists());
            if missing && self.theme.menu.window_previews {
                drawn = true;
            }
        }
        self.since_poll += 1;
        if self.since_poll < POLL_TICKS {
            return drawn;
        }
        self.since_poll = 0;
        // Devices turn up one by one during a scan: re-read the list each poll
        // until it ends, then once more for the last of them.
        if let Some(until) = self.scan_until {
            let on_bluetooth =
                self.model.dynamic_builtin() == Some(pt35_common::menu::Builtin::Bluetooth);
            if !on_bluetooth || std::time::Instant::now() > until {
                self.scan_until = None;
            }
            if on_bluetooth && self.loading.is_none() {
                self.resync();
            }
        }
        let status = crate::live::status();
        if status == self.status {
            return drawn;
        }
        self.status = status;
        self.windows = self.status.as_ref().map_or(0, |s| s.windows.len());
        self.desktop = self.status.as_ref().is_none_or(|s| s.workspace_empty());
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
        self.pointer_driven = false;
        if let Some(popup) = self.popup.as_mut() {
            let count = popup.items.len();
            match navigate(&key, Mode::Nav) {
                Navigation::Up => popup.hot = (popup.hot + count - 1) % count,
                Navigation::Down => popup.hot = (popup.hot + 1) % count,
                Navigation::Activate => {
                    let button = popup.items[popup.hot].1;
                    self.popup = None;
                    return self.press(button);
                }
                Navigation::Back | Navigation::Cancel => self.popup = None,
                // Anything else closes it and then does what it does.
                _ => {
                    self.popup = None;
                    return self.key(key);
                }
            }
            return true;
        }
        // Typing on the launcher is a search: the first letter opens it.
        if self.on_launcher()
            && self.side.is_none()
            && self.theme.menu.type_to_search
            && !pt35_ui::keys::letters_are_buttons()
            && self.model.screen().list.mode() == Mode::Nav
            && key.text.is_some_and(|c| c.is_alphanumeric())
            && pt35_ui::keys::button(&key, Mode::Filter).is_none()
        {
            self.model.handle(&Key::new(pt35_ui::keys::sym::BUTTON_X));
            let step = self.model.handle(&key);
            return self.apply(step);
        }
        // The shoulders keep their meaning with the menu up: sway hands them
        // over while it is open, so the menu does what the binding would.
        let mode = self.model.screen().list.mode();
        match pt35_ui::keys::button(&key, mode) {
            // Select is the switcher's key: it opens it, and on it, closes it.
            // A search keeps Select for clearing itself.
            Some(pt35_ui::keys::Button::Select) if mode == Mode::Nav => {
                let on_switcher =
                    self.model.dynamic_builtin() == Some(pt35_common::menu::Builtin::Windows);
                if !on_switcher {
                    self.open_here(pt35_common::menu::Builtin::Windows);
                    return true;
                }
                let step = self.model.handle(&Key::new(pt35_ui::keys::sym::BACKSPACE));
                return self.apply(step);
            }
            Some(pt35_ui::keys::Button::R) => {
                if let Err(message) = self.ctl(&["mode", "toggle"]) {
                    self.error = Some(message);
                }
                // Rebuilding the rows would drop a search in progress, and
                // only the quick panel shows the mode anyway.
                if mode == Mode::Nav {
                    self.resync();
                } else {
                    self.status = crate::live::status();
                }
                return true;
            }
            // On the switcher, the card under the cursor. Anywhere else the
            // window is hidden behind the menu: closing it unseen is a trap.
            Some(pt35_ui::keys::Button::L) => {
                return match self.model.selected_payload() {
                    Some(payload)
                        if self.model.dynamic_builtin()
                            == Some(pt35_common::menu::Builtin::Windows)
                            && payload.starts_with("con:") =>
                    {
                        let payload = payload.to_string();
                        self.apply(Step::Close(payload))
                    }
                    _ => true,
                };
            }
            _ => {}
        }
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

    fn hover(&mut self, x: f64, y: f64) -> bool {
        // Pointing at a row selects it, the way a mouse does anywhere else, in
        // either mode: an external mouse is a mouse in Buttons mode too. Only
        // motion comes here, so a menu opening under a resting cursor keeps
        // its selection.
        let (x, y) = (x as i32, y as i32);
        if let Some(popup) = self.popup.as_mut() {
            let hot = popup
                .rects
                .iter()
                .position(|&(l, t, r, b)| x >= l && x < r && y >= t && y < b);
            return match hot {
                Some(hot) if hot != popup.hot => {
                    popup.hot = hot;
                    true
                }
                _ => false,
            };
        }
        self.pointer_driven = true;
        let hit = self
            .row_hits
            .iter()
            .find(|(l, t, r, b, _)| x >= *l && x < *r && y >= *t && y < *b)
            .map(|hit| hit.4);
        match hit {
            Some(index) if index != self.model.screen().list.cursor_index() => {
                self.side = None;
                self.model.screen_mut().list.focus_window(index)
            }
            _ => false,
        }
    }

    /// A drag on the scrollbar moves the list. Anywhere else it is a pointer
    /// moving with a button held, which selects like any other.
    fn drag(&mut self, x: f64, y: f64) -> bool {
        if self.on_scrollbar(x as i32, y as i32) {
            return self.scroll_to(y as i32);
        }
        self.hover(x, y)
    }

    /// A right click opens the row's menu, the way it does on any desktop.
    /// Back is the breadcrumbs, B, or Backspace.
    fn back_click(&mut self, x: f64, y: f64) -> bool {
        let (x, y) = (x as i32, y as i32);
        self.popup = None;
        let Some(index) = self
            .row_hits
            .iter()
            .find(|(l, t, r, b, _)| x >= *l && x < *r && y >= *t && y < *b)
            .map(|hit| hit.4)
        else {
            return true;
        };
        self.side = None;
        self.model.screen_mut().list.focus_window(index);
        let items = self.row_options();
        if !items.is_empty() {
            self.popup = Some(Popup {
                x,
                y,
                items,
                hot: 0,
                rects: Vec::new(),
            });
        }
        true
    }

    fn scroll(&mut self, down: bool) -> bool {
        self.popup = None;
        let sym = if down {
            pt35_ui::keys::sym::DOWN
        } else {
            pt35_ui::keys::sym::UP
        };
        self.key(Key::new(sym))
    }

    fn touch(&mut self, x: f64, y: f64) -> bool {
        let (x, y) = (x as i32, y as i32);
        // A click with the right-click menu up is for the menu: on a row it
        // does that row, anywhere else it only closes the menu.
        if let Some(popup) = self.popup.take() {
            let hit = popup
                .rects
                .iter()
                .position(|&(l, t, r, b)| x >= l && x < r && y >= t && y < b);
            return match hit {
                Some(i) => self.press(popup.items[i].1),
                None => true,
            };
        }
        for (left, right, depth) in self.crumb_hits.clone() {
            if y < self.theme.menu.header_height as i32 && x >= left && x < right {
                let step = self.model.back_to(depth);
                return self.apply(step);
            }
        }
        if self.on_scrollbar(x, y) {
            self.scroll_to(y);
            return true;
        }
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
        self.scrollbar = None;
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
        let screen = (self.model.depth(), self.model.screen().title.clone());
        self.motion.screen(screen);
        let body_top = self.draw_header(canvas) + 8 + self.motion.rise();
        let quick = matches!(
            self.model.screen().source,
            crate::model::Source::Dynamic {
                builtin: pt35_common::menu::Builtin::Quick
                    | pt35_common::menu::Builtin::System
                    | pt35_common::menu::Builtin::About,
                ..
            }
        );
        if !self.on_launcher() {
            self.side_hits.clear();
        }
        let picker = self.model.dynamic_builtin() == Some(pt35_common::menu::Builtin::Windows);
        let launcher_active = !self.model.over_app();
        let page = self.model.page_name().map(str::to_string);
        let screen = (launcher_active, page.clone());
        if self.launcher_sent.as_ref() != Some(&screen) {
            self.launcher_sent = Some(screen);
            let _ = crate::live::request(&pt35_common::ipc::Request::MenuScreen {
                launcher_active,
                page,
            });
        }
        if !picker {
            self.strip_scroll = None;
        }
        if !picker && self.hover_sent.is_some() {
            self.hover_sent = None;
            let _ = crate::live::request(&pt35_common::ipc::Request::SwitcherHover { id: None });
        }
        if quick {
            self.draw_quick(canvas, body_top, hint_top - 4);
        } else if picker {
            self.draw_windows(canvas, body_top, hint_top - 4);
        } else if self.on_launcher() {
            self.draw_launcher(canvas, body_top, hint_top - 4);
        } else {
            match self.model.layout() {
                Layout::Grid => self.draw_tiles(canvas, body_top, hint_top - 4),
                Layout::List => self.draw_rows(canvas, body_top, hint_top),
            }
        }
        self.draw_hints(canvas, hint_top);
        self.draw_popup(canvas);
    }
}

pub fn run(page: Option<String>) -> Result<()> {
    let theme: Theme = pt35_common::load_theme().unwrap_or_default();
    let tree = pt35_common::load_config("pt35/menu.toml").unwrap_or_default();
    // Each face takes ~50ms to parse on a Pi, and the menu is started on every
    // open. In parallel, the four cost one.
    let load = |faces: Vec<String>| {
        std::thread::spawn(move || {
            let mut last = Err(anyhow::anyhow!("no font"));
            for face in faces {
                last = Font::load(&face);
                if last.is_ok() {
                    break;
                }
            }
            last
        })
    };
    let f = &theme.font;
    let font = load(vec![f.family.clone()]);
    let mono = load(vec![f.family_mono.clone(), f.family.clone()]);
    // A missing bold face is not worth failing over: the regular one reads.
    let bold = load(vec![f.family_bold.clone(), f.family.clone()]);
    let mono_bold = load(vec![
        f.family_mono_bold.clone(),
        f.family_mono.clone(),
        f.family.clone(),
    ]);
    let join = |handle: std::thread::JoinHandle<Result<Font>>| {
        handle
            .join()
            .map_err(|_| anyhow::anyhow!("font loader panicked"))?
    };
    let (font, mono, bold, mono_bold) = (join(font)?, join(mono)?, join(bold)?, join(mono_bold)?);
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
    let start = page
        .as_deref()
        .map(|name| (name, pt35_common::menu::Builtin::from_name(name)));
    if let Some((name, None)) = start {
        model.open_page(name);
        model.mark_entry();
    }
    let mut menu = Menu::new(theme, font, mono, bold, mono_bold, model);
    // Through `open`, so a slow screen shows its placeholder at once instead of
    // holding the whole menu back until nmcli answers.
    if let Some((_, Some(builtin))) = start.filter(|(_, b)| b.is_some() && *b != root) {
        menu.open(builtin);
        menu.model.mark_entry();
    }
    layer::run(menu, SurfaceSpec::overlay("pt35-menu"))
}
