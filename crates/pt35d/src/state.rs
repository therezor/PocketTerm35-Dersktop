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

impl Session {
    pub fn new() -> Self {
        let mut session = Self {
            theme: load_or_default("pt35/theme.toml"),
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
            mode_applied: false,
            mode_before_menu: InputMode::Buttons,
            bound: Vec::new(),
            lock_proc: None,
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
        session
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
        self.status.brightness_percent = self.hw.brightness_percent();
        let (volume, muted) = audio::state(self.audio);
        self.status.volume_percent = volume;
        self.status.muted = muted;
        self.status.network = self.hw.network();
        self.status.network_signal = self.hw.network_signal();
        self.refresh_windows();
        if !self.mode_applied && self.menu_proc.is_none() {
            // Nothing changes workspace at startup, so the binds have to go
            // out on the first tick or the buttons do nothing until you move.
            match self.set_mode(self.status.input_mode) {
                Ok(()) => self.mode_applied = true,
                Err(e) => log::warn!("input mode: {e}"),
            }
        }
        // Both helpers exit on their own, so reap them here or they pile up as
        // zombies.
        if let Some(child) = self.menu_proc.as_mut() {
            if matches!(child.try_wait(), Ok(Some(_))) {
                self.menu_proc = None;
                self.restore_mode();
                self.ensure_focus();
            }
        }
        if let Some(child) = self.lock_proc.as_mut() {
            if matches!(child.try_wait(), Ok(Some(_))) {
                self.lock_proc = None;
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
                let current = self
                    .hw
                    .brightness_percent()
                    .ok_or_else(|| anyhow::anyhow!("no backlight exposed to Linux on this unit"))?;
                let target = match change {
                    Delta::Absolute(v) => v.min(100) as u8,
                    Delta::Relative(v) => (current as i32 + v).clamp(1, 100) as u8,
                    Delta::Mute => bail!("brightness has no mute"),
                };
                if !self.hw.set_brightness_percent(target) {
                    bail!("could not write the backlight (permission or RP2040-owned)");
                }
                self.status.brightness_percent = Some(target);
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
                    self.mode_before_menu = target;
                    self.status.input_mode = target;
                } else {
                    self.set_mode(target)?;
                }
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
                    WindowAction::Focus(id) => self.focus_window(id)?,
                    WindowAction::Close => {
                        // Named, not a bare `kill`: after the menu has been up
                        // nothing is focused, and a bare kill hits nothing.
                        self.sync_windows();
                        let Some(target) = self.current_window().map(|w| w.id) else {
                            return Ok(Response::Ok);
                        };
                        self.close_window(target)?;
                    }
                    WindowAction::CloseId(id) => self.close_window(id)?,
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
                self.theme = load_or_default("pt35/theme.toml");
                self.menu = load_or_default("pt35/menu.toml");
                self.apps = load_or_default("pt35/apps.toml");
                self.menu.validate()?;
                self.apps.validate()?;
                self.apply_assignments();
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
        // With nothing open the menu is the desktop, so it cannot be dismissed:
        // closing it would leave a blank screen, and the next sway event would
        // reopen it anyway.
        if !want_open && self.status.windows.is_empty() && self.launch_pending.is_none() {
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
            if !self.status.windows.is_empty() {
                self.menu_is_desktop = false;
            }
            let _ = self.unbind_all();
            let mut cmd = Command::new("pt35-menu");
            if let Some(page) = page {
                cmd.arg("--page").arg(page);
            }
            self.menu_proc = Some(cmd.stdin(Stdio::null()).spawn()?);
        }
        Ok(())
    }

    /// Put the device in one mode or the other.
    ///
    /// The unbinds and the settings go in one message each; a `bindsym ... exec`
    /// cannot, because sway gives `exec` the rest of the line and would swallow
    /// every command after it into the binding.
    fn set_mode(&mut self, mode: InputMode) -> Result<()> {
        let pointer = self.theme.pointer.clone();
        let buttons = self.theme.buttons.clone();
        let wanted = crate::modes::binds(mode, &pointer);
        self.unbind_all()?;
        // Record each bind as it lands, not all of them at the end: a failure
        // halfway must still leave `bound` describing what sway actually holds,
        // or `unbind_all` returns early and the bindings are stuck.
        for bind in wanted {
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
            }
        }
        self.open_menu_on_empty_desktop();
        Ok(())
    }

    /// With nothing open the menu is the desktop. Anything else is a charcoal
    /// rectangle with a dock that has no slots in it.
    fn open_menu_on_empty_desktop(&mut self) {
        // A window appearing is what ends the wait for one.
        if !self.status.windows.is_empty() {
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
        let mode = self.mode_before_menu;
        if let Err(e) = self.set_mode(mode) {
            log::warn!("input mode: {e}");
        }
    }

    fn launch(&mut self, id: &str) -> Result<()> {
        let app = self
            .apps
            .get(id)
            .ok_or_else(|| anyhow::anyhow!("no app profile {id:?} in apps.toml"))?
            .clone();

        // Focus rather than start a second copy. Four imv processes, each
        // holding a core, is what the alternative costs.
        self.sync_windows();
        if let Some(open) = self.status.windows.iter().find(|w| app.matches_app(&w.app)) {
            let id = open.id;
            self.focus_window(id)?;
            return Ok(());
        }

        // Say what is wrong rather than switching to an empty workspace and
        // leaving a blank screen.
        if let Some(binary) = command_binary(&app.exec) {
            if !on_path(&binary) {
                bail!("{binary} is not installed");
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

        // The same no-second-copy rule an `apps.toml` entry gets.
        self.sync_windows();
        let leaf = binary.rsplit('/').next().unwrap_or(&binary).to_lowercase();
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
/// floating windows are dialogs and are left alone.
pub fn overflow_moves(windows: &[pt35_common::ipc::WindowInfo]) -> Vec<(i64, u8)> {
    let tiled: Vec<&pt35_common::ipc::WindowInfo> = windows
        .iter()
        .filter(|w| !w.floating && w.workspace > 0)
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

fn screenshot_path() -> String {
    let dir = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{dir}/pt35-screenshot-{stamp}.png")
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
