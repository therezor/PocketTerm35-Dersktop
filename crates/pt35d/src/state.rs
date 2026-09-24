//! Session state and the actions that change it.

use anyhow::{bail, Result};
use pt35_common::apps::AppTable;
use pt35_common::ipc::{
    CpuProfile, Delta, InputMode, ModeRequest, PowerAction, Request, Response, Status, Toggle,
    WindowAction,
};
use pt35_common::menu::MenuTree;
use pt35_common::theme::Theme;
use std::process::{Child, Command, Stdio};

use pt35_common::sway::Sway;

use crate::{audio, hardware::Hardware, hooks};
use pt35_common::apps::{command_binary, on_path};

pub struct Session {
    pub theme: Theme,
    pub menu: MenuTree,
    pub apps: AppTable,
    pub hw: Hardware,
    pub audio: audio::Backend,
    pub status: Status,
    sway: Option<Sway>,
    menu_proc: Option<Child>,
    /// The menu.toml page the menu shows, as it last said.
    menu_page: Option<String>,
    /// Opened at startup: sway takes a moment to adopt a new keyboard, and the
    /// first press must not be the one that is lost.
    keyboard: Option<crate::vkbd::Keyboard>,
    /// Opened at startup for the same reason.
    wheel: Option<crate::vkbd::Wheel>,
    /// False until the mode binds have been sent once. Nothing changes
    /// workspace at startup, so waiting for a workspace change would leave the
    /// buttons doing nothing.
    mode_applied: bool,
    /// The mode to go back to when the menu closes.
    mode_before_menu: InputMode,
    /// Exactly what is bound right now, so it can be taken back exactly.
    bound: Vec<crate::modes::Bind>,
    /// swaylock, while it is up. It is a layer surface and not in the tree, so
    /// the desktop reads as empty under it.
    lock_proc: Option<Child>,
    /// The backlight through the keyboard, when its firmware answers.
    serial_backlight: Option<crate::backlight::SerialBacklight>,
    /// A background look for the pt35 firmware, while it has not answered:
    /// the keyboard reboots after a flash and is replugged now and then.
    backlight_probe: Option<std::sync::mpsc::Receiver<Option<crate::backlight::SerialBacklight>>>,
    last_probe: std::time::Instant,
    /// When the focused window was last captured for the switcher.
    last_preview: std::time::Instant,
    /// True while the menu on screen is standing in for a desktop. It gets out
    /// of the way as soon as there is a window to get out of the way of.
    menu_is_desktop: bool,
    /// Notifications waiting to go out to the bar. Filled while a request is
    /// being handled, drained by whoever is holding the lock afterwards.
    pending: Vec<pt35_common::ipc::Event>,
    /// Set when something was asked to start and no window has appeared yet.
    /// Without it the menu reopens on top of every app you launch: the menu
    /// exits, the window has not mapped, the desktop reads as empty.
    launch_pending: Option<std::time::Instant>,
}

/// How long a launch suppresses the empty-desktop menu. Long enough for a slow
/// GTK app on a Pi 4, short enough that a failed launch does not strand you on
/// a blank screen.
const LAUNCH_GRACE: std::time::Duration = std::time::Duration::from_secs(5);

/// The Start button, by the keysym `KEY_PAUSE` produces. Bound by the daemon
/// rather than the sway config, because it changes hands when the menu opens.
const START: &str = "Pause";

/// Select, by both keysyms `KEY_SYSRQ` can arrive as. Handed to the menu with
/// Start, so Select on the switcher closes it instead of reopening it.
const SELECT: [&str; 2] = ["Print", "Sys_Req"];

/// Start and Select on the pt35 firmware: F21 and F22. Fn+Select is then the
/// real Print Screen, a screenshot, and Fn+Start the real Pause, left to apps.
const PT35_START: &str = "XF86TouchpadOn";
const PT35_SELECT: &str = "XF86TouchpadToggle";

impl Session {
    pub fn new() -> Self {
        let mut session = Self {
            theme: load_theme_or_default(),
            menu: load_or_default("pt35/menu.toml"),
            apps: load_or_default("pt35/apps.toml"),
            hw: Hardware::probe(),
            audio: audio::detect(),
            status: Status {
                workspace: 1,
                scale: 1.0,
                touch_enabled: true,
                ..Status::default()
            },
            sway: None,
            menu_proc: None,
            menu_page: None,
            keyboard: crate::vkbd::Keyboard::open()
                .map_err(|e| log::warn!("/dev/uinput: {e}; face buttons will use wtype"))
                .ok(),
            wheel: crate::vkbd::Wheel::open()
                .map_err(|e| log::warn!("/dev/uinput: {e}; X and Y will not scroll"))
                .ok(),
            mode_applied: false,
            mode_before_menu: InputMode::Buttons,
            bound: Vec::new(),
            lock_proc: None,
            last_preview: std::time::Instant::now(),
            serial_backlight: crate::backlight::SerialBacklight::find(),
            backlight_probe: None,
            last_probe: std::time::Instant::now(),
            menu_is_desktop: false,
            pending: Vec::new(),
            launch_pending: Some(std::time::Instant::now()),
        };
        if let Err(e) = session.menu.validate() {
            log::error!("menu.toml is inconsistent ({e}); the menu key will show an error page");
        }
        if let Err(e) = session.apps.validate() {
            log::error!("apps.toml is inconsistent: {e}");
        }
        log::info!(
            "hardware: battery={} backlight={} audio={:?}",
            session.hw.battery.is_some(),
            session.hw.backlight.is_some(),
            session.audio
        );
        session.apply_assignments();
        session.status.cpu_profile = crate::hardware::current_cpu_profile();
        session.apply_wallpaper();
        apply_color_scheme(&session.theme);
        apply_cursor(&session.theme.pointer);
        session.kill_stray_menu();
        session
    }

    /// Save a small picture of the window in front, in the background.
    ///
    /// Only when nothing covers it: a shot with the menu in it is worse than
    /// an old one.
    fn capture_preview(&mut self) {
        self.last_preview = std::time::Instant::now();
        if !self.theme.menu.window_previews || self.menu_proc.is_some() || self.lock_proc.is_some()
        {
            return;
        }
        let Some(id) = front_window(&self.status) else {
            return;
        };
        let dir = pt35_common::paths::previews_dir();
        if std::fs::create_dir_all(&dir).is_err() {
            return;
        }
        let path = dir.join(format!("{id}.ppm"));
        let top = self.theme.bar.height;
        let area = format!("0,{top} 640x{}", 480 - top);
        // Written aside and renamed, so the menu never reads half a picture.
        let spawned = Command::new("sh")
            .args([
                "-c",
                r#"grim -s 0.3 -t ppm -g "$1" "$2.tmp" && mv "$2.tmp" "$2""#,
                "pt35-preview",
                &area,
                &path.to_string_lossy(),
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        if let Ok(mut child) = spawned {
            std::thread::spawn(move || child.wait());
        }
    }

    /// Look for the pt35 firmware off the poll thread, every PROBE_EVERY until
    /// it answers; a stock keyboard costs a 0.4s read each time. When it
    /// does answer, Start and Select move to its keys.
    fn probe_firmware(&mut self) {
        if self.serial_backlight.is_some() {
            return;
        }
        if let Some(rx) = &self.backlight_probe {
            match rx.try_recv() {
                Ok(found) => {
                    self.backlight_probe = None;
                    if found.is_some() {
                        self.serial_backlight = found;
                        log::info!("pt35 keyboard firmware found");
                        if self.menu_proc.is_none() {
                            self.grab_start();
                        }
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.backlight_probe = None,
            }
            return;
        }
        if self.last_probe.elapsed() >= PROBE_EVERY {
            self.last_probe = std::time::Instant::now();
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let _ = tx.send(crate::backlight::SerialBacklight::find());
            });
            self.backlight_probe = Some(rx);
        }
    }

    /// Whether the menu is up, for the bar. The switcher's hover goes with it.
    fn sync_menu_open(&mut self) {
        self.status.menu_open = self.menu_proc.is_some();
        if !self.status.menu_open {
            self.status.switcher_hover = None;
            self.status.launcher_active = false;
            self.menu_page = None;
        }
    }

    /// Send a key through the virtual keyboard, opened on first use and kept.
    /// `wtype` is the fallback for a machine without the udev rule.
    fn send_key(&mut self, key: crate::vkbd::Key) {
        if self.keyboard.is_none() {
            match crate::vkbd::Keyboard::open() {
                Ok(keyboard) => self.keyboard = Some(keyboard),
                Err(e) => log::warn!("/dev/uinput: {e}; sending keys with wtype"),
            }
        }
        let sent = self
            .keyboard
            .as_mut()
            .map(|keyboard| keyboard.tap(key).is_ok())
            .unwrap_or(false);
        if !sent {
            self.keyboard = None;
            let _ = Command::new("wtype")
                .args(["-k", key.keysym()])
                .stdin(Stdio::null())
                .spawn()
                .map(|mut child| std::thread::spawn(move || child.wait()));
        }
    }

    /// The desktop behind the windows is the theme's background, so a palette
    /// changes the whole screen and not just the shell's own surfaces.
    fn apply_wallpaper(&mut self) {
        let pointer = &self.theme.pointer;
        let cursor = format!(
            "seat * xcursor_theme {} {}",
            pointer.cursor_theme, pointer.cursor_size
        );
        let cmd = format!("output * bg {} solid_color", self.theme.color.background);
        for cmd in [cmd, cursor] {
            if let Err(e) = self.sway_command(&cmd) {
                log::warn!("{cmd}: {e}");
            }
        }
    }

    /// Take down a menu left behind by a previous daemon.
    ///
    /// The menu is a full screen overlay holding the keyboard. A daemon that
    /// restarts under it has no child to wait on and no way to close it, so the
    /// screen stays covered and nothing is focused.
    fn kill_stray_menu(&mut self) {
        let killed = Command::new("pkill")
            .args(["-x", "pt35-menu"])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if killed {
            log::info!("closed a menu left behind by a previous pt35d");
            self.ensure_focus();
        }
        // That menu held Start. Whether or not one was there, the key belongs
        // to the compositor again now.
        self.grab_start();
    }

    /// Tell sway where each app's window belongs, once, at startup.
    ///
    /// Switching workspace and then spawning is a race: a slow app maps after
    /// focus has moved on and lands on the wrong workspace. An assign rule is
    /// evaluated when the window appears, so timing stops mattering.
    fn apply_assignments(&mut self) {
        let rules: Vec<String> = self
            .apps
            .apps
            .values()
            .filter(|app| app.workspace > 0)
            .flat_map(|app| {
                app.sway_criteria_list()
                    .into_iter()
                    .map(move |criteria| {
                        format!("assign {criteria} workspace number {}", app.workspace)
                    })
                    .collect::<Vec<_>>()
            })
            .collect();
        for rule in rules {
            if let Err(e) = self.sway_command(&rule) {
                log::warn!("{rule}: {e}");
            }
        }
    }

    /// Lazily (re)connect to sway; the socket dies when sway restarts.
    fn sway(&mut self) -> Result<&mut Sway> {
        if self.sway.is_none() {
            self.sway = Some(Sway::connect()?);
        }
        Ok(self.sway.as_mut().expect("just connected"))
    }

    fn sway_command(&mut self, cmd: &str) -> Result<()> {
        match self.sway().and_then(|s| s.command(cmd)) {
            Ok(()) => Ok(()),
            Err(first) => {
                // One retry with a fresh connection before giving up.
                log::debug!("sway command failed ({first}); reconnecting");
                self.sway = None;
                self.sway()?.command(cmd)
            }
        }
    }

    /// A query, with the same reconnect-once retry a command gets. A failed read
    /// can leave the reply unconsumed and every later reply misaligned, so the
    /// connection is thrown away rather than reused.
    fn sway_query(&mut self, kind: pt35_common::sway::MessageType) -> Result<String> {
        match self.sway().and_then(|s| s.request(kind, "")) {
            Ok(json) => Ok(json),
            Err(first) => {
                log::debug!("sway query failed ({first}); reconnecting");
                self.sway = None;
                self.sway()?.request(kind, "")
            }
        }
    }

    /// True once sway has gone. The daemon must not outlive it: a stale pt35d
    /// holds the socket and the next session cannot start.
    ///
    /// sway leaves its IPC socket file behind when it is killed, so the file
    /// existing proves nothing. Connecting to it does.
    pub fn compositor_gone(&self) -> bool {
        match Sway::socket_path() {
            Ok(path) => std::os::unix::net::UnixStream::connect(path).is_err(),
            Err(_) => true,
        }
    }

    /// Re-read the sampled hardware values into `status`.
    pub fn refresh(&mut self) {
        self.status.battery_percent = self.hw.battery_percent();
        self.status.charging = self.hw.charging();
        self.status.brightness_percent = self
            .hw
            .brightness_percent()
            .or_else(|| self.serial_backlight.as_ref().and_then(|b| b.level));
        self.probe_firmware();
        self.status.pt35_firmware = self.serial_backlight.is_some();
        let (volume, muted) = audio::state(self.audio);
        self.status.volume_percent = volume;
        self.status.muted = muted;
        self.status.network = self.hw.network();
        self.status.ethernet = self.hw.ethernet();
        self.status.network_signal = self.hw.network_signal();
        // Reap first. Both helpers exit on their own, and whether the menu is
        // still up decides whether an empty desktop needs one: noticing it
        // after the fact costs a whole extra tick of blank screen.
        if let Some(child) = self.menu_proc.as_mut() {
            if matches!(child.try_wait(), Ok(Some(_))) {
                self.menu_proc = None;
                self.restore_mode();
                self.ensure_focus();
            }
        }
        self.sync_menu_open();
        if let Some(child) = self.lock_proc.as_mut() {
            if matches!(child.try_wait(), Ok(Some(_))) {
                self.lock_proc = None;
            }
        }
        self.refresh_windows();
        // Keep the preview of the window in front fresh, slowly: a shot costs
        // about 80ms of one core, so once every PREVIEW_EVERY at most.
        if self.last_preview.elapsed() >= PREVIEW_EVERY {
            self.capture_preview();
        }
        prune_previews(&self.status.windows);
        if !self.mode_applied && self.menu_proc.is_none() {
            // Nothing changes workspace at startup, so the binds have to go
            // out on the first tick or the buttons do nothing until you move.
            match self.set_mode(self.status.input_mode) {
                Ok(()) => self.mode_applied = true,
                Err(e) => log::warn!("input mode: {e}"),
            }
        }
        self.low_battery_hook();
    }

    /// Everything that depends on the sway tree. Called on every sway window or
    /// workspace event, and once per poll as a safety net.
    ///
    /// Deliberately free of hardware reads: an event can arrive many times a
    /// second and `audio::state` spawns a subprocess.
    pub fn refresh_windows(&mut self) {
        if let Some(windows) = self.window_list() {
            self.status.windows = windows;
        }
        self.spread_windows();
        match self.sway().and_then(|s| s.focus()) {
            Ok((workspace, app)) => {
                self.status.workspace = workspace;
                self.status.app = app;
            }
            Err(_) => self.sway = None,
        }
        self.open_menu_on_empty_desktop();
    }

    /// Open windows, for the dock in the bar.
    ///
    /// `None` means sway did not answer. The caller keeps whatever it had,
    /// rather than blanking the dock over one dropped reply.
    fn window_list(&mut self) -> Option<Vec<pt35_common::ipc::WindowInfo>> {
        let json = match self.sway_query(pt35_common::sway::MessageType::GetTree) {
            Ok(json) => json,
            Err(e) => {
                log::debug!("window list: {e}");
                return None;
            }
        };
        let tree = serde_json::from_str::<serde_json::Value>(&json).ok()?;
        let apps = self.apps.clone();
        let list = pt35_common::sway::windows(&tree)
            .into_iter()
            .map(|w| pt35_common::ipc::WindowInfo {
                id: w.id,
                workspace: w.workspace,
                glyph: apps
                    .apps
                    .values()
                    .find(|app| app.matches_app(&w.app))
                    .map(|app| app.glyph.clone())
                    .unwrap_or_default(),
                icon: apps
                    .apps
                    .values()
                    .find(|app| app.matches_app(&w.app))
                    .map(|app| app.icon.clone())
                    .unwrap_or_default(),
                app: w.app,
                title: w.title,
                focused: w.focused,
                floating: w.floating,
            })
            .collect();
        Some(list)
    }

    /// Give every tiled window its own workspace. A second window on one
    /// workspace would split the screen down the middle.
    fn spread_windows(&mut self) {
        for (id, workspace) in overflow_moves(&self.status.windows) {
            let cmd = format!("[con_id={id}] move container to workspace number {workspace}");
            if let Err(e) = self.sway_command(&cmd) {
                log::warn!("{cmd}: {e}");
            } else if let Some(w) = self.status.windows.iter_mut().find(|w| w.id == id) {
                w.workspace = workspace;
            }
        }
    }

    fn low_battery_hook(&mut self) {
        if let (Some(percent), Some(false)) = (self.status.battery_percent, self.status.charging) {
            if percent <= 10 {
                hooks::fire("lowbattery", &[("PT35_BATTERY", percent.to_string())]);
            }
        }
    }

    /// Queue a line for the bar to show and then forget.
    ///
    /// The only feedback a key binding has. `pt35ctl volume +5` from a binding
    /// writes its error to a stderr nobody reads, so "no audio backend" looked
    /// exactly like a working volume key.
    pub fn notify(&mut self, summary: impl Into<String>, urgency: u8) {
        self.pending.push(pt35_common::ipc::Event::Notification {
            summary: summary.into(),
            body: String::new(),
            urgency,
        });
    }

    /// Take everything queued since the last call.
    pub fn take_notifications(&mut self) -> Vec<pt35_common::ipc::Event> {
        std::mem::take(&mut self.pending)
    }

    pub fn handle(&mut self, request: Request) -> Response {
        match self.dispatch(request) {
            Ok(response) => response,
            Err(e) => {
                let message = format!("{e:#}");
                self.notify(message.clone(), 2);
                Response::Error { message }
            }
        }
    }

    fn dispatch(&mut self, request: Request) -> Result<Response> {
        match request {
            // Read the tree rather than answering from the cache: the menu
            // builds its window picker from this and needs it current.
            Request::Status => {
                self.sync_windows();
                Ok(Response::Status(self.status.clone()))
            }
            Request::Subscribe => Ok(Response::Status(self.status.clone())),

            Request::Menu { action, page } => {
                self.toggle_menu(action, page)?;
                Ok(Response::Ok)
            }

            Request::Launch { app } => {
                self.launch(&app)?;
                Ok(Response::Ok)
            }

            Request::Exec { command } => {
                self.exec(&command)?;
                Ok(Response::Ok)
            }

            Request::Volume { change } => {
                if !audio::apply(self.audio, change) {
                    bail!("no audio backend on this system");
                }
                let (volume, muted) = audio::state(self.audio);
                self.status.volume_percent = volume;
                self.status.muted = muted;
                Ok(Response::Ok)
            }

            Request::Brightness { change } => {
                // Plugged in or reflashed since startup: look again.
                if self.hw.backlight.is_none() && self.serial_backlight.is_none() {
                    self.serial_backlight = crate::backlight::SerialBacklight::find();
                }
                let current = self
                    .hw
                    .brightness_percent()
                    .or_else(|| self.serial_backlight.as_ref().and_then(|b| b.level))
                    .ok_or_else(|| {
                        anyhow::anyhow!("brightness is Fn - and = until the keyboard is reflashed")
                    })?;
                let target = match change {
                    Delta::Absolute(v) => v.min(100) as u8,
                    Delta::Relative(v) => (current as i32 + v).clamp(1, 100) as u8,
                    Delta::Mute => bail!("brightness has no mute"),
                };
                let applied = if self.hw.backlight.is_some() {
                    self.hw.set_brightness_percent(target).then_some(target)
                } else {
                    self.serial_backlight.as_mut().and_then(|b| b.set(target))
                };
                self.status.pt35_firmware = self.serial_backlight.is_some();
                let Some(level) = applied else {
                    bail!("the keyboard did not take the brightness");
                };
                self.status.brightness_percent = Some(level);
                self.notify(format!("Brightness {level}%"), 0);
                Ok(Response::Ok)
            }

            Request::Mode { mode } => {
                let target = match mode {
                    ModeRequest::Buttons => InputMode::Buttons,
                    ModeRequest::Mouse => InputMode::Mouse,
                    ModeRequest::Toggle => match self.status.input_mode {
                        InputMode::Buttons => InputMode::Mouse,
                        InputMode::Mouse => InputMode::Buttons,
                    },
                };
                // While the menu is up the keys belong to it. Remember the
                // choice and let the close put it into effect.
                if self.menu_proc.is_some() {
                    // Switched live: the cursor appears or goes over the menu
                    // now, and the close keeps the choice.
                    self.mode_before_menu = target;
                    self.set_menu_mode(target)?;
                } else {
                    self.set_mode(target)?;
                }
                // R has no other answer: the bar icon is small, the toast is not.
                self.notify(
                    match target {
                        InputMode::Mouse => "Mouse mode: the D-pad moves the cursor",
                        InputMode::Buttons => "Buttons mode: the D-pad moves the selection",
                    },
                    0,
                );
                Ok(Response::Ok)
            }

            Request::Scale { value } => {
                let scale = value.unwrap_or_else(|| next_scale(self.status.scale));
                if !(0.5..=2.0).contains(&scale) {
                    bail!("scale {scale} is outside 0.5..=2.0");
                }
                self.sway_command(&format!("output * scale {scale}"))?;
                self.status.scale = scale;
                self.notify(format!("Scale {scale:.2}x"), 0);
                Ok(Response::Ok)
            }

            Request::SwitcherHover { id } => {
                self.status.switcher_hover = id;
                Ok(Response::Ok)
            }

            Request::MenuScreen {
                launcher_active,
                page,
            } => {
                self.status.launcher_active = launcher_active && self.menu_proc.is_some();
                self.menu_page = page;
                Ok(Response::Ok)
            }

            Request::Key { key } => {
                let key = crate::vkbd::Key::parse(&key)
                    .ok_or_else(|| anyhow::anyhow!("no key {key:?}: enter, escape or tab"))?;
                self.send_key(key);
                Ok(Response::Ok)
            }

            Request::Wheel { down } => {
                if self.wheel.is_none() {
                    self.wheel = crate::vkbd::Wheel::open().ok();
                }
                let turned = self.wheel.as_mut().map(|w| w.turn(down).is_ok());
                if turned != Some(true) {
                    self.wheel = None;
                    bail!("no wheel: /dev/uinput is not writable");
                }
                Ok(Response::Ok)
            }

            Request::WindowFit => {
                // Drag a runaway window back inside the panel, leaving room for the bar.
                let height = 480 - self.theme.bar.height;
                self.sway_command(&format!(
                    "resize set width 640 px height {height} px, move position center"
                ))?;
                Ok(Response::Ok)
            }

            Request::Window { action } => {
                match action {
                    // `focus next` stays inside one workspace, and every app
                    // owns its own, so it does nothing here. Walk the same list
                    // the dock draws instead.
                    WindowAction::Next | WindowAction::Previous => {
                        let _ = self.toggle_menu(Toggle::Off, None);
                        self.sync_windows();
                        let forward = action == WindowAction::Next;
                        let workspace = self.status.workspace;
                        match next_window(&self.status.windows, workspace, forward) {
                            Some(id) => self.focus_window(id)?,
                            None => return Ok(Response::Ok),
                        }
                    }
                    // A dock slot is a window you switch to, launcher or not:
                    // the menu steps aside rather than stay over it.
                    WindowAction::Focus(id) => {
                        if self.menu_proc.is_some() {
                            let _ = self.toggle_menu(Toggle::Off, None);
                        }
                        self.focus_window(id)?
                    }
                    WindowAction::Close => {
                        // Named, not a bare `kill`: after the menu has been up
                        // nothing is focused, and a bare kill hits nothing.
                        self.sync_windows();
                        let Some(target) = self.current_window().map(|w| w.id) else {
                            self.notify("Nothing to close", 0);
                            return Ok(Response::Ok);
                        };
                        self.close_window(target)?;
                    }
                    WindowAction::CloseId(id) => self.close_window(id)?,
                    WindowAction::CloseAll => {
                        self.sync_windows();
                        let ids: Vec<i64> = self.status.windows.iter().map(|w| w.id).collect();
                        let count = ids.len();
                        for id in ids {
                            if let Err(e) = self.sway_command(&format!("[con_id={id}] kill")) {
                                log::warn!("closing {id}: {e}");
                            }
                        }
                        // Drop them all at once. Anything that refuses to go
                        // comes back on its own `window::close` never arriving.
                        self.status.windows.clear();
                        self.notify(
                            match count {
                                1 => "Closed 1 window".to_string(),
                                n => format!("Closed {n} windows"),
                            },
                            0,
                        );
                        self.open_menu_on_empty_desktop();
                    }
                    WindowAction::Fullscreen => self.sway_command("fullscreen toggle")?,
                }
                Ok(Response::Ok)
            }

            Request::Screenshot => {
                let path = screenshot_path();
                let status = Command::new("grim")
                    .arg(&path)
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();
                match status {
                    Ok(s) if s.success() => {
                        hooks::fire("screenshot", &[("PT35_SCREENSHOT", path.clone())]);
                        let name = path.rsplit('/').next().unwrap_or(&path).to_string();
                        self.notify(format!("Saved {name}"), 0);
                        Ok(Response::Ok)
                    }
                    _ => bail!("grim failed (is it installed?)"),
                }
            }

            Request::Cpu { profile } => {
                self.set_cpu(profile)?;
                self.notify(format!("CPU: {}", profile.label()), 0);
                Ok(Response::Ok)
            }

            Request::Touch { action } => {
                // Resolved here rather than with sway's own `toggle`, so the
                // menu can say which way it is set.
                let on = match action {
                    Toggle::On => true,
                    Toggle::Off => false,
                    Toggle::Toggle => !self.status.touch_enabled,
                };
                let arg = if on { "enabled" } else { "disabled" };
                self.sway_command(&format!("input type:touch events {arg}"))?;
                self.status.touch_enabled = on;
                self.notify(if on { "Touch on" } else { "Touch off" }, 0);
                Ok(Response::Ok)
            }

            Request::Power { action } => {
                self.power(action)?;
                Ok(Response::Ok)
            }

            Request::Reload => {
                self.theme = load_theme_or_default();
                self.menu = load_or_default("pt35/menu.toml");
                self.apps = load_or_default("pt35/apps.toml");
                self.menu.validate()?;
                self.apps.validate()?;
                self.apply_assignments();
                self.apply_wallpaper();
                apply_color_scheme(&self.theme);
                apply_cursor(&self.theme.pointer);
                hooks::fire("reload", &[]);
                self.notify("Config reloaded", 0);
                Ok(Response::Ok)
            }
        }
    }

    fn toggle_menu(&mut self, action: Toggle, page: Option<String>) -> Result<()> {
        let running = matches!(
            self.menu_proc.as_mut().map(|c| c.try_wait()),
            Some(Ok(None))
        );
        let mut want_open = match action {
            Toggle::On => true,
            Toggle::Off => false,
            Toggle::Toggle => !running,
        };
        // With nothing on this workspace the menu is the desktop, so it cannot
        // be dismissed: closing it would leave a blank screen, and the next
        // sway event would reopen it anyway.
        if !want_open && self.status.workspace_empty() && self.launch_pending.is_none() {
            want_open = true;
        }
        if running && want_open && action != Toggle::On {
            // Already showing what was asked for.
            return Ok(());
        }
        if running {
            if let Some(mut child) = self.menu_proc.take() {
                let _ = child.kill();
                let _ = child.wait();
            }
            self.restore_mode();
            self.ensure_focus();
        }
        if want_open {
            // The menu reads the same keys itself, and a sway binding beats
            // any surface, so the compositor must let go while it is up.
            self.mode_before_menu = self.status.input_mode;
            // A page opened over an app is not the launcher until you go down
            // to it. The menu says so itself from its first frame; this is the
            // guess until then, so the bar does not flash.
            self.status.launcher_active = running || page.is_none();
            if !self.status.workspace_empty() {
                self.menu_is_desktop = false;
            }
            // The window you are leaving, as it looks now. Started before the
            // menu: grim reads the frame in its first ~20ms, the menu draws its
            // own at ~75ms.
            if !running {
                self.capture_preview();
            }
            let mode = self.status.input_mode;
            if let Err(e) = self.set_menu_mode(mode) {
                log::warn!("menu bindings: {e}");
                let _ = self.unbind_all();
            }
            self.release_start();
            let mut cmd = Command::new("pt35-menu");
            if let Some(page) = &page {
                cmd.arg("--page").arg(page);
            }
            self.menu_proc = Some(cmd.stdin(Stdio::null()).spawn()?);
            // Until the menu says otherwise: a second press can beat its first
            // frame.
            self.menu_page = page;
        }
        self.sync_menu_open();
        Ok(())
    }

    /// Put the device in one mode or the other.
    ///
    /// The unbinds and the settings go in one message each; a `bindsym ... exec`
    /// cannot, because sway gives `exec` the rest of the line and would swallow
    /// every command after it into the binding.
    fn set_mode(&mut self, mode: InputMode) -> Result<()> {
        let wanted = crate::modes::binds(mode, &self.theme.pointer);
        self.bind_mode(mode, wanted)
    }

    /// The bindings for `mode` while the menu is up: in Mouse mode the cursor
    /// still moves and clicks over it.
    fn set_menu_mode(&mut self, mode: InputMode) -> Result<()> {
        let wanted = crate::modes::menu_binds(mode, &self.theme.pointer);
        self.bind_mode(mode, wanted)
    }

    fn bind_mode(&mut self, mode: InputMode, wanted: Vec<crate::modes::Bind>) -> Result<()> {
        let pointer = self.theme.pointer.clone();
        let buttons = self.theme.buttons.clone();
        // Only what changes. Mouse mode's A opens the menu from the bar and is
        // still down when the menu takes over: rebinding it frees the binding
        // sway runs on the release, and sway 1.10 segfaults.
        let gone: Vec<String> = self
            .bound
            .iter()
            .filter(|b| !wanted.contains(b))
            .map(|b| b.unbind())
            .collect();
        if !gone.is_empty() {
            self.sway_command(&gone.join(", "))?;
            self.bound.retain(|b| wanted.contains(b));
        }
        // Record each bind as it lands, not all of them at the end: a failure
        // halfway must still leave `bound` describing what sway actually holds,
        // or `unbind_all` returns early and the bindings are stuck.
        for bind in wanted {
            if self.bound.contains(&bind) {
                continue;
            }
            self.sway_command(&bind.bind())?;
            self.bound.push(bind);
        }
        self.sway_command(&crate::modes::settings(mode, &pointer, &buttons).join(", "))?;
        self.status.input_mode = mode;
        Ok(())
    }

    /// Re-read the window list, keeping the old one if sway does not answer.
    fn sync_windows(&mut self) {
        if let Some(windows) = self.window_list() {
            self.status.windows = windows;
        }
    }

    fn focus_window(&mut self, id: i64) -> Result<()> {
        self.sway_command(&format!("[con_id={id}] focus"))?;
        self.sync_windows();
        Ok(())
    }

    /// Ask one window to close, then take it out of the list at once.
    ///
    /// `kill` is a request, not a deletion: sway sends `xdg_toplevel.close` and
    /// answers success straight away, while the container lives until the client
    /// destroys its surface. Re-reading the tree here would return the window we
    /// just killed, which is the whole bug. So drop it locally and let the
    /// `window::close` event put the truth back. An app that refuses to close,
    /// say to ask about unsaved work, reappears, which is correct.
    fn close_window(&mut self, id: i64) -> Result<()> {
        // Closing leaves you on an empty workspace otherwise, because every app
        // owns one.
        let next = next_window(&self.status.windows, self.status.workspace, true);
        self.sway_command(&format!("[con_id={id}] kill"))?;
        self.status.windows.retain(|w| w.id != id);
        if let Some(next) = next.filter(|next| *next != id) {
            let _ = self.sway_command(&format!("[con_id={next}] focus"));
            for window in &mut self.status.windows {
                window.focused = window.id == next;
                // Focus took us there: the workspace left behind is not the
                // one in front any more.
                if window.focused {
                    self.status.workspace = window.workspace;
                }
            }
        }
        self.open_menu_on_empty_desktop();
        Ok(())
    }

    /// With nothing on this workspace the menu is the desktop. Anything else
    /// is a charcoal rectangle: an app that exits leaves its workspace empty,
    /// and sway stays on it.
    fn open_menu_on_empty_desktop(&mut self) {
        // A window appearing is what ends the wait for one.
        if !self.status.workspace_empty() {
            self.launch_pending = None;
            // The menu was only there because nothing else was. Something else
            // is there now, and it is behind a full screen overlay.
            if self.menu_is_desktop {
                self.menu_is_desktop = false;
                if let Err(e) = self.toggle_menu(Toggle::Off, None) {
                    log::warn!("closing the desktop menu: {e}");
                }
            }
            return;
        }
        let pending = self
            .launch_pending
            .is_some_and(|at| at.elapsed() < LAUNCH_GRACE);
        if !should_open_menu(
            true,
            self.menu_proc.is_some(),
            self.lock_proc.is_some(),
            pending,
        ) {
            return;
        }
        self.launch_pending = None;
        match self.toggle_menu(Toggle::On, None) {
            Ok(()) => self.menu_is_desktop = true,
            Err(e) => log::warn!("opening the menu on an empty desktop: {e}"),
        }
    }

    /// The window a keypress means: the focused one, or the one on this
    /// workspace when the menu has just taken focus away from everything.
    ///
    /// No fall back to the first window in the list: close on an empty
    /// workspace must do nothing, not kill something on another one.
    fn current_window(&self) -> Option<&pt35_common::ipc::WindowInfo> {
        self.status.windows.iter().find(|w| w.focused).or_else(|| {
            self.status
                .windows
                .iter()
                .find(|w| w.workspace == self.status.workspace)
        })
    }

    /// Give focus back to a window. A layer surface that took the keyboard
    /// leaves sway with no focused view when it goes, and then every command
    /// that acts on "the focused window" does nothing.
    fn ensure_focus(&mut self) {
        self.sync_windows();
        if self.status.windows.iter().any(|w| w.focused) {
            return;
        }
        // Anything is better than nothing here, so this one does fall back to
        // the first window in the list. `current_window` must not: it decides
        // what gets closed.
        let id = self
            .current_window()
            .or_else(|| self.status.windows.first())
            .map(|w| w.id);
        if let Some(id) = id {
            let _ = self.sway_command(&format!("[con_id={id}] focus"));
        }
    }

    /// Hand every key back. The menu reads the same ones, and a sway binding
    /// beats any surface.
    fn unbind_all(&mut self) -> Result<()> {
        if self.bound.is_empty() {
            return Ok(());
        }
        let commands: Vec<String> = self.bound.iter().map(|b| b.unbind()).collect();
        self.sway_command(&commands.join(", "))?;
        self.bound.clear();
        Ok(())
    }

    fn restore_mode(&mut self) {
        self.grab_start();
        let mode = self.mode_before_menu;
        if let Err(e) = self.set_mode(mode) {
            log::warn!("input mode: {e}");
        }
    }

    /// Hand Start to the menu while it is up.
    ///
    /// A sway binding beats any surface, so with `Pause` bound the menu never
    /// saw the key and could not decide what closing means on the screen you
    /// are actually looking at.
    /// Start's and Select's keysyms. The pt35 firmware's are always bound:
    /// stock never sends them, and a keyboard still booting when pt35d asked
    /// must not be left with dead buttons. Stock's Pause and Print are bound
    /// too until the pt35 firmware is known, then left to Fn.
    fn start_select(&self) -> (Vec<&'static str>, Vec<&'static str>) {
        if self.serial_backlight.is_some() {
            (vec![PT35_START], vec![PT35_SELECT])
        } else {
            (
                vec![PT35_START, START],
                [&[PT35_SELECT][..], &SELECT[..]].concat(),
            )
        }
    }

    fn release_start(&mut self) {
        let (start, select) = self.start_select();
        let keys: Vec<String> = start
            .into_iter()
            .chain(select)
            .map(|key| format!("unbindsym --no-repeat {key}"))
            .collect();
        if let Err(e) = self.sway_command(&keys.join(", ")) {
            log::warn!("releasing Start and Select: {e}");
        }
    }

    /// Take Start back. `$mod+space` stays bound in the sway config throughout,
    /// so the menu is still reachable if this ever fails.
    fn grab_start(&mut self) {
        // One message each: sway gives `exec` the rest of the line.
        //
        // `--no-repeat`, because Start is still down when `release_start` takes
        // it off: a repeating binding leaves sway's key-repeat timer holding
        // the freed binding, and sway 1.10 segfaults when it fires.
        let (start, select) = self.start_select();
        let mut binds: Vec<String> = start
            .iter()
            .map(|key| format!("bindsym --no-repeat {key} exec pt35ctl menu toggle"))
            .chain(
                select
                    .iter()
                    .map(|key| format!("bindsym --no-repeat {key} exec pt35ctl menu open windows")),
            )
            .collect();
        if self.serial_backlight.is_some() {
            // Fn+Select is Print Screen: a screenshot, as on any desktop. The
            // stock bindings on Print and Pause come off, so Fn+Start reaches
            // the app as a plain Pause.
            let _ = self.sway_command(&format!(
                "unbindsym --no-repeat {START}, unbindsym --no-repeat Sys_Req, \
                 unbindsym --no-repeat Print"
            ));
            binds.push("bindsym --no-repeat Print exec pt35ctl screenshot".into());
        }
        for bind in binds {
            if let Err(e) = self.sway_command(&bind) {
                log::warn!("{bind}: {e}");
            }
        }
    }

    fn launch(&mut self, id: &str) -> Result<()> {
        let app = self
            .apps
            .get(id)
            .ok_or_else(|| anyhow::anyhow!("no app profile {id:?} in apps.toml"))?
            .clone();

        // Focus rather than start a second copy, unless the profile says a
        // second copy is the point. Four image viewers each holding a core is
        // what picking the same row twice costs otherwise.
        self.sync_windows();
        if !app.multiple {
            if let Some(open) = self.status.windows.iter().find(|w| app.matches_app(&w.app)) {
                let id = open.id;
                self.focus_window(id)?;
                return Ok(());
            }
        }

        // Say what is wrong rather than switching to an empty workspace and
        // leaving a blank screen.
        for program in pt35_common::apps::command_programs(&app.exec) {
            if !on_path(&program) {
                bail!("{program} is not installed");
            }
        }

        if app.workspace > 0 {
            self.sway_command(&format!("workspace number {}", app.workspace))?;
        }
        if app.fullscreen {
            for criteria in app.sway_criteria_list() {
                let _ = self.sway_command(&format!("for_window {criteria} fullscreen enable"));
            }
        }

        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg(&app.exec).stdin(Stdio::null());
        for (key, value) in app.toolkit_env() {
            cmd.env(key, value);
        }
        for (key, value) in &app.env {
            cmd.env(key, value);
        }
        cmd.spawn()?;
        // The window has not mapped yet, so the desktop still reads as empty.
        // Hold the menu off until it appears.
        self.launch_pending = Some(std::time::Instant::now());
        crate::recents::record(&format!("app:{id}"));

        hooks::fire("launch", &[("PT35_APP", id.to_string())]);
        Ok(())
    }

    /// Run a bare command line, with the checks a profiled app gets.
    fn exec(&mut self, command: &str) -> Result<()> {
        let binary = command_binary(command)
            .ok_or_else(|| anyhow::anyhow!("{command:?} is not a command"))?;

        // `sh -c` always succeeds, so this is the only thing that can tell a
        // missing program from a working one.
        if !on_path(&binary) {
            bail!("{binary} is not installed");
        }

        // The same no-second-copy rule an `apps.toml` entry gets, and the same
        // exception: a profile matching this binary decides.
        self.sync_windows();
        let leaf = binary.rsplit('/').next().unwrap_or(&binary).to_lowercase();
        let many = self
            .apps
            .apps
            .values()
            .any(|app| app.multiple && app.matches_app(&leaf));
        if !many {
            if let Some(open) = self
                .status
                .windows
                .iter()
                .find(|w| w.app.to_lowercase() == leaf)
            {
                let id = open.id;
                self.focus_window(id)?;
                return Ok(());
            }
        }

        Command::new("sh")
            .arg("-c")
            .arg(command)
            .stdin(Stdio::null())
            .spawn()?;
        self.launch_pending = Some(std::time::Instant::now());
        crate::recents::record(&format!("exec:{command}"));
        hooks::fire("launch", &[("PT35_APP", command.to_string())]);
        Ok(())
    }

    fn set_cpu(&mut self, profile: CpuProfile) -> Result<()> {
        if crate::hardware::apply_cpu_profile(profile) {
            self.status.cpu_profile = Some(profile);
            return Ok(());
        }
        // cpufreq is root-owned; the installer drops a NOPASSWD rule for this
        // one helper so the session user can still switch profiles.
        let name = match profile {
            CpuProfile::Powersave => "powersave",
            CpuProfile::Balanced => "balanced",
            CpuProfile::Performance => "performance",
        };
        let ok = Command::new("sudo")
            .args(["-n", "/usr/lib/pt35/pt35-cpu-profile", name])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if !ok {
            bail!("cannot change the CPU profile (no cpufreq write access)");
        }
        self.status.cpu_profile = Some(profile);
        Ok(())
    }

    fn power(&mut self, action: PowerAction) -> Result<()> {
        match action {
            // A second press closes it. Reopening it would only flash. With
            // nothing open, closing means the top screen, as Start does.
            PowerAction::Menu if self.menu_page.as_deref() == Some("power") => {
                if self.status.windows.is_empty() {
                    self.toggle_menu(Toggle::On, None)
                } else {
                    self.toggle_menu(Toggle::Off, None)
                }
            }
            PowerAction::Menu => self.toggle_menu(Toggle::On, Some("power".into())),
            PowerAction::ScreenOff => self.sway_command("output * dpms off"),
            PowerAction::Lock => {
                // The Pi cannot suspend; "lock" is a blank screen plus swaylock
                // when it is installed.
                match Command::new("swaylock").spawn() {
                    Ok(child) => {
                        self.lock_proc = Some(child);
                        Ok(())
                    }
                    Err(_) => self.sway_command("output * dpms off"),
                }
            }
            PowerAction::Logout => self.sway_command("exit"),
            PowerAction::Reboot => {
                hooks::fire("shutdown", &[("PT35_ACTION", "reboot".into())]);
                run_or_fail(&["systemctl", "reboot"])
            }
            PowerAction::Poweroff => {
                hooks::fire("shutdown", &[("PT35_ACTION", "poweroff".into())]);
                run_or_fail(&["systemctl", "poweroff"])
            }
        }
    }
}

/// Whether an empty desktop should bring the menu up.
///
/// Four ways it should not: something is open, the menu is already there,
/// swaylock is up (a layer surface, so the tree looks empty under it), or
/// something was asked to start and its window has not mapped yet.
pub fn should_open_menu(empty: bool, menu_up: bool, locked: bool, launch_pending: bool) -> bool {
    empty && !menu_up && !locked && !launch_pending
}

fn run_or_fail(argv: &[&str]) -> Result<()> {
    let status = Command::new(argv[0]).args(&argv[1..]).status()?;
    if !status.success() {
        bail!("{} failed", argv.join(" "));
    }
    Ok(())
}

/// The window to focus when the user asks for the next or previous app.
///
/// `windows` is the dock's order. Wraps. Nothing is focused while the keyboard
/// pointer holds the keyboard, so the current workspace is the fallback anchor.
pub fn next_window(
    windows: &[pt35_common::ipc::WindowInfo],
    workspace: u8,
    forward: bool,
) -> Option<i64> {
    if windows.is_empty() {
        return None;
    }
    let current = windows
        .iter()
        .position(|w| w.focused)
        .or_else(|| windows.iter().rposition(|w| w.workspace == workspace));
    let Some(current) = current else {
        return Some(windows[0].id);
    };
    if windows.len() == 1 {
        return None;
    }
    let len = windows.len();
    let next = if forward {
        (current + 1) % len
    } else {
        (current + len - 1) % len
    };
    Some(windows[next].id)
}

/// Windows that have to move so no workspace holds two tiled windows.
///
/// 640x480 splits into two unusable halves, so one window owns the screen and
/// the others wait on their own workspace. The focused window keeps its place;
/// floating windows are dialogs and are left alone. So is a portal's file
/// chooser, in case it maps before the sway rule floats it: moved away, the app
/// that asked for it waits on it where you cannot see.
pub fn overflow_moves(windows: &[pt35_common::ipc::WindowInfo]) -> Vec<(i64, u8)> {
    let tiled: Vec<&pt35_common::ipc::WindowInfo> = windows
        .iter()
        .filter(|w| !w.floating && w.workspace > 0 && !w.app.starts_with("xdg-desktop-portal"))
        .collect();
    let mut occupied: Vec<u8> = Vec::new();
    let mut movers: Vec<i64> = Vec::new();
    for workspace in 1..=9u8 {
        let here: Vec<&pt35_common::ipc::WindowInfo> = tiled
            .iter()
            .copied()
            .filter(|w| w.workspace == workspace)
            .collect();
        if here.is_empty() {
            continue;
        }
        occupied.push(workspace);
        let keeper = here
            .iter()
            .find(|w| w.focused)
            .map(|w| w.id)
            .unwrap_or(here[0].id);
        movers.extend(here.iter().map(|w| w.id).filter(|id| *id != keeper));
    }
    let mut moves = Vec::new();
    for id in movers {
        let Some(free) = (1..=9u8).find(|n| !occupied.contains(n)) else {
            break;
        };
        occupied.push(free);
        moves.push((id, free));
    }
    moves
}

/// The scales worth cycling through on a 640x480 panel: native, then the two
/// that GUI apps need (853x640 and 1067x800 logical).
pub fn next_scale(current: f32) -> f32 {
    const STEPS: [f32; 3] = [1.0, 0.75, 0.6];
    let index = STEPS
        .iter()
        .position(|s| (s - current).abs() < 0.01)
        .map(|i| (i + 1) % STEPS.len())
        .unwrap_or(0);
    STEPS[index]
}

/// `~/Pictures`, where an image viewer and the file manager look first.
fn screenshot_path() -> String {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    let dir = format!("{home}/Pictures");
    if std::fs::create_dir_all(&dir).is_err() {
        return format!("{home}/pt35-screenshot-{}.png", now_secs());
    }
    format!("{dir}/pt35-screenshot-{}.png", now_secs())
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Put GTK, libadwaita and the portal's dialogs in the theme's scheme.
///
/// gsettings rather than GTK_THEME: the portal's file chooser is started by
/// dbus, not by us, and reads only these. Off the main thread, because a first
/// gsettings call can wait on dconf.
fn apply_color_scheme(theme: &pt35_common::theme::Theme) {
    let Some(settings) = color_scheme_settings(&theme.apps) else {
        return;
    };
    std::thread::spawn(move || {
        for (key, value) in settings {
            let status = Command::new("gsettings")
                .args(["set", "org.gnome.desktop.interface", key, &value])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            if !matches!(status, Ok(s) if s.success()) {
                log::warn!("gsettings set {key} {value} failed");
                return;
            }
        }
    });
}

/// The `org.gnome.desktop.interface` keys for a scheme, or `None` for `keep`.
pub fn color_scheme_settings(
    apps: &pt35_common::theme::Apps,
) -> Option<Vec<(&'static str, String)>> {
    use pt35_common::theme::ColorScheme;
    let (scheme, gtk) = match apps.color_scheme {
        ColorScheme::Keep => return None,
        ColorScheme::Dark => ("prefer-dark", &apps.gtk_dark),
        ColorScheme::Light => ("prefer-light", &apps.gtk_light),
    };
    Some(vec![
        ("color-scheme", scheme.to_string()),
        ("gtk-theme", gtk.clone()),
    ])
}

/// GTK draws its own cursor over its windows, so it is told too.
fn apply_cursor(pointer: &pt35_common::theme::Pointer) {
    let settings = [
        ("cursor-theme", pointer.cursor_theme.clone()),
        ("cursor-size", pointer.cursor_size.to_string()),
    ];
    std::thread::spawn(move || {
        for (key, value) in settings {
            let _ = Command::new("gsettings")
                .args(["set", "org.gnome.desktop.interface", key, &value])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
    });
}

/// How often to look for the pt35 keyboard firmware until it answers.
const PROBE_EVERY: std::time::Duration = std::time::Duration::from_secs(10);

/// How often the window in front is captured while you use it.
const PREVIEW_EVERY: std::time::Duration = std::time::Duration::from_secs(20);

/// The window a capture of the screen shows: the focused one, or with the
/// menu holding focus, the tiled one on the current workspace.
pub fn front_window(status: &pt35_common::ipc::Status) -> Option<i64> {
    status
        .windows
        .iter()
        .find(|w| w.focused)
        .or_else(|| {
            status
                .windows
                .iter()
                .find(|w| w.workspace == status.workspace && !w.floating)
        })
        .map(|w| w.id)
}

/// Drop the pictures of windows that have closed.
fn prune_previews(windows: &[pt35_common::ipc::WindowInfo]) {
    let Ok(entries) = std::fs::read_dir(pt35_common::paths::previews_dir()) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let id = name
            .to_str()
            .and_then(|n| n.strip_suffix(".ppm"))
            .and_then(|n| n.parse::<i64>().ok());
        if id.is_some_and(|id| !windows.iter().any(|w| w.id == id)) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

fn load_theme_or_default() -> pt35_common::theme::Theme {
    pt35_common::load_theme().unwrap_or_else(|e| {
        log::error!("theme: {e}; falling back to built-in defaults");
        Default::default()
    })
}

fn load_or_default<T: serde::de::DeserializeOwned + Default>(relative: &str) -> T {
    match pt35_common::load_config::<T>(relative) {
        Ok(value) => value,
        Err(e) => {
            log::error!("{relative}: {e}; falling back to built-in defaults");
            T::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_desktop_menu_knows_when_to_stay_out_of_the_way() {
        assert!(should_open_menu(true, false, false, false));
        assert!(
            !should_open_menu(false, false, false, false),
            "a window is open"
        );
        assert!(!should_open_menu(true, true, false, false), "already up");
        assert!(!should_open_menu(true, false, true, false), "locked");
        assert!(
            !should_open_menu(true, false, false, true),
            "a launch is on its way; do not land on top of it"
        );
    }

    #[test]
    fn scale_cycles_through_the_useful_values() {
        assert_eq!(next_scale(1.0), 0.75);
        assert_eq!(next_scale(0.75), 0.6);
        assert_eq!(next_scale(0.6), 1.0);
        assert_eq!(next_scale(1.37), 1.0, "an unknown scale returns to native");
    }

    fn window(id: i64, workspace: u8, focused: bool) -> pt35_common::ipc::WindowInfo {
        pt35_common::ipc::WindowInfo {
            id,
            workspace,
            glyph: String::new(),
            icon: String::new(),
            app: "app".into(),
            title: "title".into(),
            focused,
            floating: false,
        }
    }

    #[test]
    fn next_window_wraps_around_the_dock() {
        let list = [window(1, 1, false), window(2, 2, true), window(3, 3, false)];
        assert_eq!(next_window(&list, 2, true), Some(3));
        assert_eq!(next_window(&list, 2, false), Some(1));
        let last = [window(1, 1, false), window(2, 2, true)];
        assert_eq!(next_window(&last, 2, true), Some(1));
        assert_eq!(next_window(&[], 1, true), None);
        assert_eq!(next_window(&[window(9, 1, true)], 1, true), None);
    }

    #[test]
    fn the_workspace_anchors_the_cycle_when_nothing_is_focused() {
        // The keyboard pointer takes the keyboard, so no view is focused while
        // it is armed. Select must still walk the dock instead of snapping back.
        let list = [
            window(1, 1, false),
            window(2, 2, false),
            window(3, 3, false),
        ];
        assert_eq!(next_window(&list, 2, true), Some(3));
        assert_eq!(next_window(&list, 2, false), Some(1));
        assert_eq!(next_window(&list, 7, true), Some(1), "nothing to anchor on");
    }

    #[test]
    fn dark_apps_ask_gtk_for_dark_and_keep_asks_for_nothing() {
        let mut apps = pt35_common::theme::Apps::default();
        let dark = color_scheme_settings(&apps).unwrap();
        assert!(dark.contains(&("color-scheme", "prefer-dark".into())));
        assert!(dark.contains(&("gtk-theme", "Adwaita-dark".into())));
        apps.color_scheme = pt35_common::theme::ColorScheme::Keep;
        assert!(color_scheme_settings(&apps).is_none());
    }

    #[test]
    fn a_file_chooser_stays_with_the_app_that_asked_for_it() {
        let mut chooser = window(2, 1, true);
        chooser.app = "xdg-desktop-portal-gtk".into();
        assert!(overflow_moves(&[window(1, 1, false), chooser]).is_empty());
    }

    #[test]
    fn a_second_window_on_a_workspace_moves_to_a_free_one() {
        let list = [window(1, 2, true), window(2, 2, false), window(3, 3, false)];
        assert_eq!(overflow_moves(&list), vec![(2, 1)]);
    }

    #[test]
    fn the_focused_window_is_the_one_that_stays() {
        let list = [window(1, 2, false), window(2, 2, true)];
        assert_eq!(overflow_moves(&list), vec![(1, 1)]);
    }

    #[test]
    fn dialogs_and_single_windows_are_left_alone() {
        let mut floating = window(2, 2, false);
        floating.floating = true;
        let list = [window(1, 2, true), floating, window(3, 3, false)];
        assert!(overflow_moves(&list).is_empty());
    }

    #[test]
    fn nothing_moves_when_every_workspace_is_taken() {
        let mut list: Vec<_> = (1..=9).map(|n| window(n as i64, n, false)).collect();
        list.push(window(99, 1, false));
        assert!(overflow_moves(&list).is_empty());
    }

    #[test]
    fn screenshots_land_somewhere_writable() {
        let path = screenshot_path();
        assert!(path.ends_with(".png"));
        assert!(path.contains("pt35-screenshot-"));
    }
}
