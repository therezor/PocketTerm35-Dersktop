//! Session state and the actions that change it.

use anyhow::{bail, Result};
use pt35_common::apps::{AppTable, PointerPolicy};
use pt35_common::ipc::{
    CpuProfile, Delta, PointerMode, PowerAction, Request, Response, Status, Toggle, WindowAction,
};
use pt35_common::menu::MenuTree;
use pt35_common::theme::Theme;
use std::process::{Child, Command, Stdio};

use pt35_common::sway::Sway;

use crate::{audio, hardware::Hardware, hooks};

pub struct Session {
    pub theme: Theme,
    pub menu: MenuTree,
    pub apps: AppTable,
    pub hw: Hardware,
    pub audio: audio::Backend,
    pub status: Status,
    sway: Option<Sway>,
    menu_proc: Option<Child>,
    pointer_proc: Option<Child>,
    /// Whether button mode was on before the menu took the keyboard.
    buttons_before_menu: bool,
}

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
                ..Status::default()
            },
            sway: None,
            menu_proc: None,
            pointer_proc: None,
            buttons_before_menu: false,
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

    /// Whether the face buttons act as buttons on this workspace. They do
    /// everywhere except in an app you type into, so an unclaimed workspace
    /// keeps them.
    pub fn buttons_for_workspace(apps: &AppTable, workspace: u8) -> bool {
        apps.apps
            .values()
            .find(|app| app.workspace == workspace)
            .map(|app| app.buttons)
            .unwrap_or(true)
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
        self.status.windows = self.window_list();
        self.spread_windows();
        if let Ok((workspace, app)) = self.sway().and_then(|s| s.focus()) {
            let moved = workspace != self.status.workspace;
            self.status.workspace = workspace;
            self.status.app = app;
            if moved && self.menu_proc.is_none() {
                let buttons = Self::buttons_for_workspace(&self.apps, workspace);
                if buttons != self.status.button_mode {
                    if let Err(e) = self.set_button_mode(buttons) {
                        log::warn!("button mode: {e}");
                    }
                }
            }
        } else {
            self.sway = None;
        }
        // Both helpers exit on their own, so reap them here or they pile up as
        // zombies.
        if let Some(child) = self.menu_proc.as_mut() {
            if matches!(child.try_wait(), Ok(Some(_))) {
                self.menu_proc = None;
                self.restore_buttons();
            }
        }
        if !matches!(
            self.pointer_proc.as_mut().map(|c| c.try_wait()),
            Some(Ok(None))
        ) {
            self.pointer_proc = None;
            self.status.pointer_armed = false;
        }
        self.low_battery_hook();
    }

    /// Open windows, for the dock in the bar.
    fn window_list(&mut self) -> Vec<pt35_common::ipc::WindowInfo> {
        let reply = self
            .sway()
            .and_then(|s| s.request(pt35_common::sway::MessageType::GetTree, ""));
        let Ok(json) = reply else { return Vec::new() };
        let Ok(tree) = serde_json::from_str::<serde_json::Value>(&json) else {
            return Vec::new();
        };
        pt35_common::sway::windows(&tree)
            .into_iter()
            .map(|w| pt35_common::ipc::WindowInfo {
                id: w.id,
                workspace: w.workspace,
                app: w.app,
                title: w.title,
                focused: w.focused,
                floating: w.floating,
            })
            .collect()
    }

    /// Give every tiled window its own workspace. Without this, a second
    /// window on one workspace splits the screen down the middle.
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

    pub fn handle(&mut self, request: Request) -> Response {
        match self.dispatch(request) {
            Ok(response) => response,
            Err(e) => Response::Error {
                message: format!("{e:#}"),
            },
        }
    }

    fn dispatch(&mut self, request: Request) -> Result<Response> {
        match request {
            Request::Status => Ok(Response::Status(self.status.clone())),
            Request::Subscribe => Ok(Response::Status(self.status.clone())),

            Request::Menu { action, page } => {
                self.toggle_menu(action, page)?;
                Ok(Response::Ok)
            }

            Request::Launch { app } => {
                self.launch(&app)?;
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

            Request::Pointer { mode } => {
                self.set_pointer(mode)?;
                Ok(Response::Ok)
            }

            Request::Scale { value } => {
                let scale = value.unwrap_or_else(|| next_scale(self.status.scale));
                if !(0.5..=2.0).contains(&scale) {
                    bail!("scale {scale} is outside 0.5..=2.0");
                }
                self.sway_command(&format!("output * scale {scale}"))?;
                self.status.scale = scale;
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
                        self.status.windows = self.window_list();
                        let forward = action == WindowAction::Next;
                        let workspace = self.status.workspace;
                        match next_window(&self.status.windows, workspace, forward) {
                            Some(id) => self.sway_command(&format!("[con_id={id}] focus"))?,
                            None => return Ok(Response::Ok),
                        }
                    }
                    WindowAction::Close => self.sway_command("kill")?,
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
                        Ok(Response::Ok)
                    }
                    _ => bail!("grim failed (is it installed?)"),
                }
            }

            Request::Cpu { profile } => {
                self.set_cpu(profile)?;
                Ok(Response::Ok)
            }

            Request::Buttons { action } => {
                let want = match action {
                    Toggle::On => true,
                    Toggle::Off => false,
                    Toggle::Toggle => !self.status.button_mode,
                };
                self.set_button_mode(want)?;
                Ok(Response::Ok)
            }

            Request::Touch { action } => {
                let arg = match action {
                    Toggle::On => "enabled",
                    Toggle::Off => "disabled",
                    Toggle::Toggle => "toggle",
                };
                self.sway_command(&format!("input type:touch events {arg}"))?;
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
                Ok(Response::Ok)
            }
        }
    }

    fn toggle_menu(&mut self, action: Toggle, page: Option<String>) -> Result<()> {
        let running = matches!(
            self.menu_proc.as_mut().map(|c| c.try_wait()),
            Some(Ok(None))
        );
        let want_open = match action {
            Toggle::On => true,
            Toggle::Off => false,
            Toggle::Toggle => !running,
        };
        if running {
            if let Some(mut child) = self.menu_proc.take() {
                let _ = child.kill();
                let _ = child.wait();
            }
            self.restore_buttons();
        }
        if want_open {
            // The menu reads the letters itself, so the compositor must not
            // be holding them while it is up.
            if self.status.button_mode {
                self.buttons_before_menu = true;
                let _ = self.set_button_mode(false);
            }
            let mut cmd = Command::new("pt35-menu");
            if let Some(page) = page {
                cmd.arg("--page").arg(page);
            }
            self.menu_proc = Some(cmd.stdin(Stdio::null()).spawn()?);
        }
        Ok(())
    }

    /// Grab or release the six letter buttons.
    fn set_button_mode(&mut self, on: bool) -> Result<()> {
        let commands = if on {
            crate::buttons::enable()
        } else {
            crate::buttons::disable()
        };
        for command in commands {
            self.sway_command(&command)?;
        }
        self.status.button_mode = on;
        Ok(())
    }

    fn restore_buttons(&mut self) {
        if self.buttons_before_menu {
            self.buttons_before_menu = false;
            let _ = self.set_button_mode(true);
        }
    }

    fn set_pointer(&mut self, mode: PointerMode) -> Result<()> {
        let running = matches!(
            self.pointer_proc.as_mut().map(|c| c.try_wait()),
            Some(Ok(None))
        );
        let target = match mode {
            PointerMode::Toggle if running => PointerMode::Off,
            PointerMode::Toggle => PointerMode::Move,
            other => other,
        };
        if running {
            if let Some(mut child) = self.pointer_proc.take() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
        match target {
            PointerMode::Off => self.status.pointer_armed = false,
            PointerMode::Move | PointerMode::Grid => {
                let arg = if matches!(target, PointerMode::Grid) {
                    "grid"
                } else {
                    "move"
                };
                self.pointer_proc = Some(
                    Command::new("pt35-pointer")
                        .arg("--mode")
                        .arg(arg)
                        .stdin(Stdio::null())
                        .spawn()?,
                );
                self.status.pointer_armed = true;
            }
            PointerMode::Toggle => unreachable!("resolved above"),
        }
        Ok(())
    }

    fn launch(&mut self, id: &str) -> Result<()> {
        let app = self
            .apps
            .get(id)
            .ok_or_else(|| anyhow::anyhow!("no app profile {id:?} in apps.toml"))?
            .clone();

        // A missing binary used to switch to an empty workspace and leave a
        // blank screen. Say what is wrong instead.
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

        if app.pointer == PointerPolicy::Auto {
            let _ = self.set_pointer(PointerMode::Move);
        }
        hooks::fire("launch", &[("PT35_APP", id.to_string())]);
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
                if Command::new("swaylock").arg("-f").spawn().is_err() {
                    self.sway_command("output * dpms off")?;
                }
                Ok(())
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

fn run_or_fail(argv: &[&str]) -> Result<()> {
    let status = Command::new(argv[0]).args(&argv[1..]).status()?;
    if !status.success() {
        bail!("{} failed", argv.join(" "));
    }
    Ok(())
}

/// The binary an `exec` line runs, skipping a `sh -c` wrapper.
pub fn command_binary(exec: &str) -> Option<String> {
    let mut words = exec.split_whitespace();
    let first = words.next()?;
    if first == "sh" || first == "bash" {
        // sh -c '<real command> ...': take the first word inside the quotes.
        let rest = exec.split_once("-c")?.1.trim();
        let inner = rest.trim_start_matches(['\'', '"']);
        return inner.split_whitespace().next().map(str::to_string);
    }
    Some(first.to_string())
}

fn on_path(binary: &str) -> bool {
    if binary.starts_with('/') {
        return std::path::Path::new(binary).exists();
    }
    let Some(path) = std::env::var_os("PATH") else {
        return true;
    };
    std::env::split_paths(&path).any(|dir| dir.join(binary).exists())
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
    fn scale_cycles_through_the_useful_values() {
        assert_eq!(next_scale(1.0), 0.75);
        assert_eq!(next_scale(0.75), 0.6);
        assert_eq!(next_scale(0.6), 1.0);
        assert_eq!(next_scale(1.37), 1.0, "an unknown scale returns to native");
    }

    #[test]
    fn a_workspace_takes_the_button_mode_of_the_app_that_owns_it() {
        let apps: AppTable = toml::from_str(
            "[app.term]\nexec = \"foot\"\nworkspace = 1\nbuttons = false\n[app.pix]\nexec = \"imv\"\nworkspace = 6\n",
        )
        .unwrap();
        assert!(!Session::buttons_for_workspace(&apps, 1));
        assert!(Session::buttons_for_workspace(&apps, 6));
        assert!(
            Session::buttons_for_workspace(&apps, 4),
            "an unclaimed workspace keeps the buttons"
        );
    }

    fn window(id: i64, workspace: u8, focused: bool) -> pt35_common::ipc::WindowInfo {
        pt35_common::ipc::WindowInfo {
            id,
            workspace,
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
    fn finds_the_binary_behind_an_exec_line() {
        assert_eq!(command_binary("foot").as_deref(), Some("foot"));
        assert_eq!(
            command_binary("chromium --ozone-platform=wayland").as_deref(),
            Some("chromium")
        );
        assert_eq!(
            command_binary("sh -c 'imv-wayland \"$HOME/Pictures\"'").as_deref(),
            Some("imv-wayland")
        );
        assert_eq!(command_binary("").as_deref(), None);
    }

    #[test]
    fn screenshots_land_somewhere_writable() {
        let path = screenshot_path();
        assert!(path.ends_with(".png"));
        assert!(path.contains("pt35-screenshot-"));
    }
}
