//! Turning a chosen menu item into something that happens.
//!
//! The decision (what to run) is a pure function so it can be tested; only
//! [`perform`] touches the system.

use crate::model::Command;
use pt35_common::menu::Builtin;

/// How a command is carried out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Plan {
    /// Hand these arguments to `pt35ctl`, reusing its argument parsing rather
    /// than duplicating it here.
    Ctl(Vec<String>),
    /// Run this through `sh -c`.
    Shell(String),
    /// Nothing to do (an informational row, e.g. on the About screen).
    Nothing,
}

pub fn plan(command: &Command) -> Plan {
    match command {
        Command::App(id) => Plan::Ctl(vec!["launch".into(), id.clone()]),
        Command::Exec(cmd) => Plan::Shell(cmd.clone()),
        Command::Action(args) => Plan::Ctl(args.split_whitespace().map(str::to_string).collect()),
        Command::Dynamic { builtin, payload } => dynamic(*builtin, payload),
    }
}

fn dynamic(builtin: Builtin, payload: &str) -> Plan {
    if payload.is_empty() {
        return Plan::Nothing;
    }
    match builtin {
        Builtin::Windows => Plan::Shell(format!("swaymsg '{payload} focus'")),
        // Joining a network may need a passphrase, so it happens in a terminal
        // the user can actually type into.
        Builtin::Wifi => Plan::Shell(format!(
            "foot -a pt35-wifi sh -c \"nmcli --ask device wifi connect {}; read -r _\"",
            shell_quote(payload)
        )),
        Builtin::Bluetooth => Plan::Shell(format!(
            "foot -a pt35-bt sh -c \"bluetoothctl connect {}; read -r _\"",
            shell_quote(payload)
        )),
        // "volume 40" / "brightness 70" are already pt35ctl commands.
        Builtin::Audio | Builtin::Display => {
            Plan::Ctl(payload.split_whitespace().map(str::to_string).collect())
        }
        Builtin::DesktopEntries => Plan::Shell(payload.to_string()),
        Builtin::About => Plan::Nothing,
    }
}

/// Single-quote for `sh`, the way a network name with a space or a quote needs.
pub fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}

pub fn perform(command: &Command) {
    match plan(command) {
        Plan::Nothing => {}
        Plan::Ctl(args) => {
            if let Err(e) = std::process::Command::new("pt35ctl").args(&args).spawn() {
                log::error!("pt35ctl {}: {e}", args.join(" "));
            }
        }
        Plan::Shell(cmd) => {
            if let Err(e) = std::process::Command::new("sh").arg("-c").arg(&cmd).spawn() {
                log::error!("sh -c {cmd:?}: {e}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apps_and_actions_go_through_pt35ctl() {
        assert_eq!(
            plan(&Command::App("browser".into())),
            Plan::Ctl(vec!["launch".into(), "browser".into()])
        );
        assert_eq!(
            plan(&Command::Action("power poweroff".into())),
            Plan::Ctl(vec!["power".into(), "poweroff".into()])
        );
    }

    #[test]
    fn focusing_a_window_uses_its_container_id() {
        assert_eq!(
            plan(&Command::Dynamic {
                builtin: Builtin::Windows,
                payload: "[con_id=34]".into()
            }),
            Plan::Shell("swaymsg '[con_id=34] focus'".into())
        );
    }

    #[test]
    fn wifi_names_are_quoted_for_the_shell() {
        let Plan::Shell(cmd) = plan(&Command::Dynamic {
            builtin: Builtin::Wifi,
            payload: "My Net's AP".into(),
        }) else {
            panic!("expected a shell plan");
        };
        assert!(cmd.contains(r"'My Net'\''s AP'"), "{cmd}");
    }

    #[test]
    fn informational_rows_do_nothing() {
        assert_eq!(
            plan(&Command::Dynamic {
                builtin: Builtin::About,
                payload: String::new()
            }),
            Plan::Nothing
        );
    }

    #[test]
    fn level_rows_map_onto_pt35ctl() {
        assert_eq!(
            plan(&Command::Dynamic {
                builtin: Builtin::Audio,
                payload: "volume 40".into()
            }),
            Plan::Ctl(vec!["volume".into(), "40".into()])
        );
    }
}
