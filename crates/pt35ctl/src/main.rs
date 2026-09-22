//! `pt35ctl` — the one command sway keybindings, menu entries and hook scripts
//! use to talk to `pt35d`.
//!
//! Hand-rolled argument parsing: this binary is on the hot path of every key
//! press, so it stays free of clap and starts in ~1 ms.

use anyhow::{bail, Context, Result};
use pt35_common::ipc::{
    CpuProfile, Delta, PointerMode, PowerAction, Request, Response, Status, Toggle,
};

mod client;

const USAGE: &str = "\
pt35ctl — control the PocketTerm35 desktop session

usage:
  pt35ctl menu [open|close|toggle] [PAGE]
  pt35ctl launch APP
  pt35ctl volume +5 | -5 | 50 | mute
  pt35ctl brightness +10 | -10 | 50
  pt35ctl pointer [toggle|on|off|grid]
  pt35ctl scale [1.0|0.75|cycle]
  pt35ctl window fit
  pt35ctl screenshot
  pt35ctl cpu powersave|balanced|performance
  pt35ctl power screenoff|lock|logout|reboot|poweroff|menu
  pt35ctl touch on|off|toggle
  pt35ctl reload
  pt35ctl status [--json]
";

fn main() {
    if let Err(err) = run() {
        eprintln!("pt35ctl: {err:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let argv: Vec<&str> = args.iter().map(String::as_str).collect();

    match argv.as_slice() {
        [] | ["-h"] | ["--help"] | ["help"] => {
            print!("{USAGE}");
            Ok(())
        }
        ["--version"] => {
            println!("pt35ctl {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        ["status", rest @ ..] => status(rest.contains(&"--json")),
        rest => {
            let request = parse(rest)?;
            match client::send(&request)? {
                Response::Ok => Ok(()),
                Response::Error { message } => bail!(message),
                Response::Status(s) => {
                    print_status(&s);
                    Ok(())
                }
            }
        }
    }
}

/// Turn an argv slice into a request. Kept pure so it can be unit-tested.
fn parse(argv: &[&str]) -> Result<Request> {
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

        ["volume", value] => Request::Volume {
            change: delta(value)?,
        },
        ["brightness", value] => Request::Brightness {
            change: delta(value)?,
        },

        ["pointer"] | ["pointer", "toggle"] => Request::Pointer {
            mode: PointerMode::Toggle,
        },
        ["pointer", "on"] | ["pointer", "move"] => Request::Pointer {
            mode: PointerMode::Move,
        },
        ["pointer", "off"] => Request::Pointer {
            mode: PointerMode::Off,
        },
        ["pointer", "grid"] => Request::Pointer {
            mode: PointerMode::Grid,
        },

        ["scale"] | ["scale", "cycle"] => Request::Scale { value: None },
        ["scale", value] => Request::Scale {
            value: Some(
                value
                    .parse()
                    .with_context(|| format!("bad scale {value:?}"))?,
            ),
        },

        ["window", "fit"] => Request::WindowFit,
        ["screenshot"] => Request::Screenshot,

        ["cpu", profile] => Request::Cpu {
            profile: match *profile {
                "powersave" => CpuProfile::Powersave,
                "balanced" => CpuProfile::Balanced,
                "performance" => CpuProfile::Performance,
                other => bail!("unknown cpu profile {other:?}"),
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
                other => bail!("unknown power action {other:?}"),
            },
        },

        ["touch", action] => Request::Touch {
            action: toggle(action)?,
        },
        ["reload"] => Request::Reload,

        other => bail!("unknown command: {}\n\n{USAGE}", other.join(" ")),
    })
}

fn delta(value: &str) -> Result<Delta> {
    value.parse::<Delta>().map_err(|e| anyhow::anyhow!(e))
}

fn toggle(value: &str) -> Result<Toggle> {
    Ok(match value {
        "on" => Toggle::On,
        "off" => Toggle::Off,
        "toggle" => Toggle::Toggle,
        other => bail!("expected on|off|toggle, got {other:?}"),
    })
}

fn status(json: bool) -> Result<()> {
    match client::send(&Request::Status)? {
        Response::Status(s) if json => {
            println!("{}", serde_json::to_string(&s)?);
            Ok(())
        }
        Response::Status(s) => {
            print_status(&s);
            Ok(())
        }
        Response::Error { message } => bail!(message),
        Response::Ok => bail!("daemon returned no status"),
    }
}

fn print_status(s: &Status) {
    println!("workspace   {}", s.workspace);
    println!("app         {}", s.app.as_deref().unwrap_or("-"));
    println!("scale       {}", s.scale);
    println!(
        "battery     {}",
        match (s.battery_percent, s.charging) {
            (Some(p), Some(true)) => format!("{p}% charging"),
            (Some(p), _) => format!("{p}%"),
            (None, _) => "not exposed by this hardware".to_string(),
        }
    );
    println!(
        "brightness  {}",
        s.brightness_percent
            .map(|p| format!("{p}%"))
            .unwrap_or_else(|| "not exposed by this hardware".into())
    );
    println!(
        "volume      {}{}",
        s.volume_percent
            .map(|p| format!("{p}%"))
            .unwrap_or_else(|| "-".into()),
        if s.muted.unwrap_or(false) {
            " (muted)"
        } else {
            ""
        }
    );
    println!("network     {}", s.network.as_deref().unwrap_or("-"));
    println!(
        "pointer     {}",
        if s.pointer_armed { "armed" } else { "off" }
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_bindings_used_in_the_sway_config() {
        assert_eq!(
            parse(&["menu", "toggle"]).unwrap(),
            Request::Menu {
                action: Toggle::Toggle,
                page: None
            }
        );
        assert_eq!(
            parse(&["volume", "+5"]).unwrap(),
            Request::Volume {
                change: Delta::Relative(5)
            }
        );
        assert_eq!(
            parse(&["brightness", "-10"]).unwrap(),
            Request::Brightness {
                change: Delta::Relative(-10)
            }
        );
        assert_eq!(parse(&["window", "fit"]).unwrap(), Request::WindowFit);
        assert_eq!(
            parse(&["pointer", "toggle"]).unwrap(),
            Request::Pointer {
                mode: PointerMode::Toggle
            }
        );
        assert_eq!(
            parse(&["power", "menu"]).unwrap(),
            Request::Power {
                action: PowerAction::Menu
            }
        );
        assert_eq!(
            parse(&["cpu", "balanced"]).unwrap(),
            Request::Cpu {
                profile: CpuProfile::Balanced
            }
        );
        assert_eq!(parse(&["scale"]).unwrap(), Request::Scale { value: None });
        assert_eq!(
            parse(&["scale", "0.75"]).unwrap(),
            Request::Scale { value: Some(0.75) }
        );
    }

    #[test]
    fn rejects_nonsense() {
        assert!(parse(&["fly", "me", "to", "the", "moon"]).is_err());
        assert!(parse(&["volume", "loud"]).is_err());
        assert!(parse(&["cpu", "turbo"]).is_err());
        assert!(parse(&["touch", "maybe"]).is_err());
    }

    #[test]
    fn every_menu_toml_action_parses() {
        // Keep in step with config/pt35/menu.toml: every `action = "..."` there
        // must be a command this binary understands.
        for action in [
            "pointer toggle",
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
            parse(&argv).unwrap_or_else(|e| panic!("menu action {action:?} does not parse: {e}"));
        }
    }
}
