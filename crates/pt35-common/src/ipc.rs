//! The `pt35ctl` <-> `pt35d` protocol: one JSON object per line over a unix
//! socket. Line-delimited JSON keeps the daemon dependency-free and makes the
//! socket debuggable with `socat`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Request {
    /// Open, close or toggle the menu overlay.
    Menu {
        action: Toggle,
        page: Option<String>,
    },
    /// Launch an app by its `apps.toml` id.
    Launch {
        app: String,
    },
    /// Relative (`+5`, `-5`) or absolute (`50`) volume, or mute toggle.
    Volume {
        change: Delta,
    },
    Brightness {
        change: Delta,
    },
    /// Switch between the two input modes.
    Mode {
        mode: ModeRequest,
    },
    /// Set the sway output scale, or cycle through the configured ones.
    Scale {
        value: Option<f32>,
    },
    /// Force the focused window back inside 640x480.
    WindowFit,
    /// Close the focused window, or move focus between windows.
    Window {
        action: WindowAction,
    },
    Screenshot,
    Cpu {
        profile: CpuProfile,
    },
    Power {
        action: PowerAction,
    },
    Touch {
        action: Toggle,
    },
    /// Re-read theme/menu/apps from disk.
    Reload,
    /// Everything the bar draws, in one message.
    Status,
    /// Subscribe to status updates; the daemon then streams `Event`s.
    Subscribe,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Toggle {
    On,
    Off,
    Toggle,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Delta {
    Relative(i32),
    Absolute(u32),
    Mute,
}

impl std::str::FromStr for Delta {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        match s {
            "mute" | "toggle" => Ok(Delta::Mute),
            _ if s.starts_with('+') || s.starts_with('-') => s
                .parse::<i32>()
                .map(Delta::Relative)
                .map_err(|_| format!("bad relative value {s:?}")),
            _ => s
                .parse::<u32>()
                .map(Delta::Absolute)
                .map_err(|_| format!("bad value {s:?}: want +N, -N, N or 'mute'")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowAction {
    Close,
    Next,
    Previous,
    Fullscreen,
}

/// The device has twelve controls and two things to do with them, so there are
/// two modes and nothing else.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputMode {
    /// D-pad navigates, A is Enter, B is Escape. The default.
    #[default]
    Buttons,
    /// D-pad moves the cursor, A and B are the mouse buttons.
    Mouse,
}

impl InputMode {
    /// What the bar shows.
    pub fn label(self) -> &'static str {
        match self {
            InputMode::Buttons => "BTN",
            InputMode::Mouse => "MOUSE",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModeRequest {
    Buttons,
    Mouse,
    Toggle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CpuProfile {
    Powersave,
    Balanced,
    Performance,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PowerAction {
    ScreenOff,
    Lock,
    Logout,
    Reboot,
    Poweroff,
    Menu,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "reply", rename_all = "snake_case")]
pub enum Response {
    Ok,
    Error { message: String },
    Status(Status),
}

/// One snapshot of everything the bar shows. Absent fields mean "this device
/// does not expose it" — on the PocketTerm35 battery and backlight may well be
/// owned by the RP2040 and invisible to Linux.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Status {
    pub workspace: u8,
    pub app: Option<String>,
    pub battery_percent: Option<u8>,
    pub charging: Option<bool>,
    pub brightness_percent: Option<u8>,
    pub volume_percent: Option<u8>,
    pub muted: Option<bool>,
    pub network: Option<String>,
    /// Wi-Fi link quality, 0-100. `None` on a wired or offline machine.
    #[serde(default)]
    pub network_signal: Option<u8>,
    /// Whether the touchscreen is accepted. Off is for when a palm on the panel
    /// keeps tapping things.
    #[serde(default = "yes")]
    pub touch_enabled: bool,
    #[serde(default)]
    pub input_mode: InputMode,
    pub scale: f32,
    pub cpu_profile: Option<CpuProfile>,
    /// Open windows, for the dock in the bar.
    #[serde(default)]
    pub windows: Vec<WindowInfo>,
}

fn yes() -> bool {
    true
}

/// One entry in the dock.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct WindowInfo {
    pub id: i64,
    pub workspace: u8,
    pub app: String,
    pub title: String,
    /// One or two characters standing in for an icon, from the app's profile.
    #[serde(default)]
    pub glyph: String,
    pub focused: bool,
    #[serde(default)]
    pub floating: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Event {
    Status(Status),
    Notification {
        summary: String,
        body: String,
        urgency: u8,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(req: Request) {
        let line = serde_json::to_string(&req).unwrap();
        assert!(!line.contains('\n'), "requests must fit on one line");
        assert_eq!(serde_json::from_str::<Request>(&line).unwrap(), req);
    }

    #[test]
    fn requests_roundtrip() {
        roundtrip(Request::Menu {
            action: Toggle::Toggle,
            page: None,
        });
        roundtrip(Request::Launch {
            app: "browser".into(),
        });
        roundtrip(Request::Volume {
            change: Delta::Relative(5),
        });
        roundtrip(Request::Scale { value: Some(0.75) });
        roundtrip(Request::Power {
            action: PowerAction::Poweroff,
        });
        roundtrip(Request::Status);
        roundtrip(Request::Window {
            action: WindowAction::Close,
        });
        roundtrip(Request::Mode {
            mode: ModeRequest::Toggle,
        });
    }

    #[test]
    fn parses_delta_arguments() {
        assert_eq!("+5".parse::<Delta>().unwrap(), Delta::Relative(5));
        assert_eq!("-10".parse::<Delta>().unwrap(), Delta::Relative(-10));
        assert_eq!("42".parse::<Delta>().unwrap(), Delta::Absolute(42));
        assert_eq!("mute".parse::<Delta>().unwrap(), Delta::Mute);
        assert!("loud".parse::<Delta>().is_err());
    }

    #[test]
    fn status_survives_missing_hardware() {
        let status = Status {
            workspace: 1,
            scale: 1.0,
            ..Status::default()
        };
        let line = serde_json::to_string(&Response::Status(status.clone())).unwrap();
        match serde_json::from_str::<Response>(&line).unwrap() {
            Response::Status(got) => {
                assert_eq!(got, status);
                assert!(got.battery_percent.is_none());
            }
            other => panic!("unexpected {other:?}"),
        }
    }
}
