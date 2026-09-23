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
    /// Run a command line, the way a `.desktop` entry's `Exec=` gives it.
    ///
    /// Through the daemon rather than straight from the menu, so that a missing
    /// binary is reported, an app that is already open is focused instead of
    /// started twice, and the desktop knows something is on its way and does not
    /// reopen the menu over it.
    Exec {
        command: String,
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
    /// Send a key to the focused window: `enter`, `escape` or `tab`. What the
    /// face buttons mean in Buttons mode.
    Key {
        key: String,
    },
    /// One notch of the wheel: X and Y in Mouse mode.
    Wheel {
        down: bool,
    },
    /// The switcher's cursor moved onto this window, or off every window.
    SwitcherHover {
        id: Option<i64>,
    },
    /// Whether the launcher, rather than a screen over an app, is in front.
    MenuScreen {
        launcher_active: bool,
        /// The menu.toml page in front, so a key that opened it can close it.
        #[serde(default)]
        page: Option<String>,
    },
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

impl Request {
    /// Parse a `pt35ctl` argument list.
    ///
    /// Lives here rather than in `pt35ctl` so the menu can build the same
    /// request from the same words without shelling out, and so a `menu.toml`
    /// `action =` string has exactly one meaning.
    pub fn from_ctl(argv: &[&str]) -> Result<Request, String> {
        Ok(match argv {
            ["menu"] | ["menu", "toggle"] => Request::Menu {
                action: Toggle::Toggle,
                page: None,
            },
            ["menu", "open"] => Request::Menu {
                action: Toggle::On,
                page: None,
            },
            ["menu", "close"] => Request::Menu {
                action: Toggle::Off,
                page: None,
            },
            ["menu", "open", page] => Request::Menu {
                action: Toggle::On,
                page: Some((*page).to_string()),
            },
            ["menu", page] => Request::Menu {
                action: Toggle::On,
                page: Some((*page).to_string()),
            },

            ["launch", app] => Request::Launch {
                app: (*app).to_string(),
            },
            ["exec", rest @ ..] if !rest.is_empty() => Request::Exec {
                command: rest.join(" "),
            },

            ["volume", value] => Request::Volume {
                change: value.parse::<Delta>()?,
            },
            ["brightness", value] => Request::Brightness {
                change: value.parse::<Delta>()?,
            },

            ["mode"] | ["mode", "toggle"] => Request::Mode {
                mode: ModeRequest::Toggle,
            },
            ["mode", "buttons"] => Request::Mode {
                mode: ModeRequest::Buttons,
            },
            ["mode", "mouse"] => Request::Mode {
                mode: ModeRequest::Mouse,
            },

            ["scale"] | ["scale", "cycle"] => Request::Scale { value: None },
            ["scale", value] => Request::Scale {
                value: Some(value.parse().map_err(|_| format!("bad scale {value:?}"))?),
            },

            ["window", "fit"] => Request::WindowFit,
            ["key", key] => Request::Key {
                key: key.to_string(),
            },
            ["wheel", "up"] => Request::Wheel { down: false },
            ["wheel", "down"] => Request::Wheel { down: true },
            ["window", "closeall"] => Request::Window {
                action: WindowAction::CloseAll,
            },
            ["window", "close"] => Request::Window {
                action: WindowAction::Close,
            },
            ["window", "next"] => Request::Window {
                action: WindowAction::Next,
            },
            ["window", "prev"] => Request::Window {
                action: WindowAction::Previous,
            },
            ["window", "focus", id] => Request::Window {
                action: WindowAction::Focus(
                    id.parse()
                        .map_err(|_| format!("{id:?} is not a container id"))?,
                ),
            },
            ["window", "close", id] => Request::Window {
                action: WindowAction::CloseId(
                    id.parse()
                        .map_err(|_| format!("{id:?} is not a container id"))?,
                ),
            },
            ["window", "fullscreen"] => Request::Window {
                action: WindowAction::Fullscreen,
            },
            ["screenshot"] => Request::Screenshot,

            ["cpu", profile] => Request::Cpu {
                profile: match *profile {
                    "powersave" => CpuProfile::Powersave,
                    "balanced" => CpuProfile::Balanced,
                    "performance" => CpuProfile::Performance,
                    other => return Err(format!("unknown cpu profile {other:?}")),
                },
            },

            ["power", action] => Request::Power {
                action: match *action {
                    "screenoff" => PowerAction::ScreenOff,
                    "lock" => PowerAction::Lock,
                    "logout" => PowerAction::Logout,
                    "reboot" => PowerAction::Reboot,
                    "poweroff" => PowerAction::Poweroff,
                    "menu" => PowerAction::Menu,
                    other => return Err(format!("unknown power action {other:?}")),
                },
            },

            ["touch", action] => Request::Touch {
                action: Toggle::from_ctl(action)?,
            },
            ["reload"] => Request::Reload,

            other => return Err(format!("unknown command: {}", other.join(" "))),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Toggle {
    On,
    Off,
    Toggle,
}

impl Toggle {
    pub fn from_ctl(value: &str) -> Result<Self, String> {
        match value {
            "on" => Ok(Toggle::On),
            "off" => Ok(Toggle::Off),
            "toggle" => Ok(Toggle::Toggle),
            other => Err(format!("expected on|off|toggle, got {other:?}")),
        }
    }
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
    /// Focus one window by container id. The dock and the window picker use
    /// this rather than calling `swaymsg` themselves, so the daemon learns
    /// about the change and tells the bar at once.
    Focus(i64),
    /// Close one window by container id, whichever one has focus.
    CloseId(i64),
    /// Ask every window to close. Destructive, so nothing reaches this without
    /// a confirmation in front of it.
    CloseAll,
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

impl CpuProfile {
    pub fn label(self) -> &'static str {
        match self {
            CpuProfile::Powersave => "powersave",
            CpuProfile::Balanced => "balanced",
            CpuProfile::Performance => "performance",
        }
    }
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
    /// The interface carrying traffic. A cable wins over Wi-Fi.
    pub network: Option<String>,
    /// Wi-Fi link quality, 0-100, even while a cable carries the traffic.
    /// `None` with no Wi-Fi link.
    #[serde(default)]
    pub network_signal: Option<u8>,
    /// The Ethernet interface with a cable in.
    #[serde(default)]
    pub ethernet: Option<String>,
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
    /// The launcher is on screen. The bar's close button is off meanwhile: it
    /// would close an app you cannot see.
    #[serde(default)]
    pub menu_open: bool,
    /// The launcher is what is in front, not a screen opened over an app (the
    /// switcher from Select, the power menu). The bar fills the skull slot
    /// only then, and keeps the app's slot filled otherwise.
    #[serde(default)]
    pub launcher_active: bool,
    /// The keyboard runs the pt35 firmware: the face buttons send F13-F18,
    /// so letters are only letters. Known because that firmware answers the
    /// backlight command.
    #[serde(default)]
    pub pt35_firmware: bool,
    /// The window the switcher's cursor is on, so the taskbar can mark the
    /// same slot. `Some(0)` is the Launcher card: no sway container has id 0.
    #[serde(default)]
    pub switcher_hover: Option<i64>,
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
    /// freedesktop icon name from the app's profile.
    #[serde(default)]
    pub icon: String,
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

#[cfg(test)]
mod ctl_tests {
    use super::*;

    #[test]
    fn parses_the_bindings_used_in_the_sway_config() {
        assert_eq!(
            Request::from_ctl(&["menu", "toggle"]).unwrap(),
            Request::Menu {
                action: Toggle::Toggle,
                page: None
            }
        );
        assert_eq!(
            Request::from_ctl(&["volume", "+5"]).unwrap(),
            Request::Volume {
                change: Delta::Relative(5)
            }
        );
        assert_eq!(
            Request::from_ctl(&["brightness", "-10"]).unwrap(),
            Request::Brightness {
                change: Delta::Relative(-10)
            }
        );
        assert_eq!(
            Request::from_ctl(&["window", "fit"]).unwrap(),
            Request::WindowFit
        );
        assert_eq!(
            Request::from_ctl(&["window", "focus", "34"]).unwrap(),
            Request::Window {
                action: WindowAction::Focus(34)
            }
        );
        assert_eq!(
            Request::from_ctl(&["window", "close", "34"]).unwrap(),
            Request::Window {
                action: WindowAction::CloseId(34)
            }
        );
        assert!(Request::from_ctl(&["window", "focus", "nope"]).is_err());
        assert_eq!(
            Request::from_ctl(&["window", "close"]).unwrap(),
            Request::Window {
                action: WindowAction::Close
            }
        );
        assert_eq!(
            Request::from_ctl(&["window", "next"]).unwrap(),
            Request::Window {
                action: WindowAction::Next
            }
        );
        assert_eq!(
            Request::from_ctl(&["mode", "toggle"]).unwrap(),
            Request::Mode {
                mode: ModeRequest::Toggle
            }
        );
        assert_eq!(
            Request::from_ctl(&["mode", "mouse"]).unwrap(),
            Request::Mode {
                mode: ModeRequest::Mouse
            }
        );
        assert_eq!(
            Request::from_ctl(&["power", "menu"]).unwrap(),
            Request::Power {
                action: PowerAction::Menu
            }
        );
        assert_eq!(
            Request::from_ctl(&["cpu", "balanced"]).unwrap(),
            Request::Cpu {
                profile: CpuProfile::Balanced
            }
        );
        assert_eq!(
            Request::from_ctl(&["scale"]).unwrap(),
            Request::Scale { value: None }
        );
        assert_eq!(
            Request::from_ctl(&["scale", "0.75"]).unwrap(),
            Request::Scale { value: Some(0.75) }
        );
    }

    #[test]
    fn rejects_nonsense() {
        assert!(Request::from_ctl(&["fly", "me", "to", "the", "moon"]).is_err());
        assert!(Request::from_ctl(&["volume", "loud"]).is_err());
        assert!(Request::from_ctl(&["cpu", "turbo"]).is_err());
        assert!(Request::from_ctl(&["touch", "maybe"]).is_err());
    }

    #[test]
    fn every_menu_toml_action_parses() {
        // Keep in step with config/pt35/menu.toml: every `action = "..."` there
        // must be a command this binary understands.
        for action in [
            "mode toggle",
            "mode mouse",
            "scale cycle",
            "cpu powersave",
            "cpu balanced",
            "cpu performance",
            "touch toggle",
            "screenshot",
            "reload",
            "power screenoff",
            "power lock",
            "power logout",
            "power reboot",
            "power poweroff",
        ] {
            let argv: Vec<&str> = action.split_whitespace().collect();
            Request::from_ctl(&argv)
                .unwrap_or_else(|e| panic!("menu action {action:?} does not parse: {e}"));
        }
    }
}
