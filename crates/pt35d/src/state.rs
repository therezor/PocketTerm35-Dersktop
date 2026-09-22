//! Session state and the actions that change it.

use anyhow::{bail, Result};
use pt35_common::apps::{AppTable, PointerPolicy};
use pt35_common::ipc::{
    CpuProfile, Delta, PointerMode, PowerAction, Request, Response, Status, Toggle,
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
}

impl Session {
    pub fn new() -> Self {
        let session = Self {
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
        session
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

    /// The output scale a workspace wants, from the app profile that owns it.
    /// Workspaces nothing claims go back to the panel's native 640x480.
    pub fn scale_for_workspace(apps: &AppTable, workspace: u8) -> f32 {
        apps.apps
            .values()
            .find(|app| app.workspace == workspace)
            .map(|app| app.scale)
            .unwrap_or(1.0)
    }

    /// True once sway has gone. The daemon must not outlive it: a stale pt35d
    /// holds the socket and the next session cannot start.
    pub fn compositor_gone(&self) -> bool {
        match Sway::socket_path() {
            Ok(path) => !path.exists(),
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
        if let Ok((workspace, app)) = self.sway().and_then(|s| s.focus()) {
            let moved = workspace != self.status.workspace;
            self.status.workspace = workspace;
            self.status.app = app;
            // sway scales the whole output, not one window, so the profile has
            // to follow the focused workspace: leave the browser's workspace and
            // the panel goes back to native 640x480 instead of staying at 0.75.
            if moved {
                let wanted = Self::scale_for_workspace(&self.apps, workspace);
                if (wanted - self.status.scale).abs() > f32::EPSILON {
                    if let Err(e) = self.sway_command(&format!("output * scale {wanted}")) {
                        log::warn!("switching scale to {wanted}: {e}");
                    } else {
                        self.status.scale = wanted;
                    }
                }
            }
        } else {
            self.sway = None;
        }
        // pt35-pointer exits on its own (Escape), so the flag has to be observed
        // rather than remembered.
        if self.status.pointer_armed
            && !matches!(
                self.pointer_proc.as_mut().map(|c| c.try_wait()),
                Some(Ok(None))
            )
        {
            self.pointer_proc = None;
            self.status.pointer_armed = false;
        }
        self.low_battery_hook();
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
        }
        if want_open {
            let mut cmd = Command::new("pt35-menu");
            if let Some(page) = page {
                cmd.arg("--page").arg(page);
            }
            self.menu_proc = Some(cmd.stdin(Stdio::null()).spawn()?);
        }
        Ok(())
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

        if app.workspace > 0 {
            self.sway_command(&format!("workspace number {}", app.workspace))?;
        }
        if (app.scale - self.status.scale).abs() > f32::EPSILON {
            self.sway_command(&format!("output * scale {}", app.scale))?;
            self.status.scale = app.scale;
        }
        if app.fullscreen {
            let criteria = app.sway_criteria();
            let _ = self.sway_command(&format!("for_window {criteria} fullscreen enable"));
        }

        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg(&app.exec).stdin(Stdio::null());
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
    fn a_workspace_takes_the_scale_of_the_app_that_owns_it() {
        let apps: AppTable = toml::from_str(
            r#"
[app.term]
exec = "foot"
workspace = 1
scale = 1.0

[app.browser]
exec = "chromium"
workspace = 8
scale = 0.75
"#,
        )
        .unwrap();
        assert_eq!(Session::scale_for_workspace(&apps, 8), 0.75);
        assert_eq!(Session::scale_for_workspace(&apps, 1), 1.0);
        assert_eq!(
            Session::scale_for_workspace(&apps, 5),
            1.0,
            "an unclaimed workspace returns to native"
        );
    }

    #[test]
    fn screenshots_land_somewhere_writable() {
        let path = screenshot_path();
        assert!(path.ends_with(".png"));
        assert!(path.contains("pt35-screenshot-"));
    }
}
