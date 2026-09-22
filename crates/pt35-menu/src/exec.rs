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
        // Through the daemon: it checks the binary exists, focuses a copy that
        // is already open, and holds the desktop's menu off until the window
        // appears. A bare `sh -c` did none of that and could not fail.
        Command::Exec(cmd) => Plan::Ctl(vec!["exec".into(), cmd.clone()]),
        Command::Action(args) => Plan::Ctl(args.split_whitespace().map(str::to_string).collect()),
        Command::Dynamic { builtin, payload } => dynamic(*builtin, payload),
    }
}

fn dynamic(builtin: Builtin, payload: &str) -> Plan {
    if payload.is_empty() {
        return Plan::Nothing;
    }
    match builtin {
        // Through the daemon rather than straight to swaymsg, so it knows the
        // focus moved and the dock does not wait for the next tree read.
        Builtin::Windows => match payload.strip_prefix("con:") {
            Some(id) => Plan::Ctl(vec!["window".into(), "focus".into(), id.into()]),
            None => Plan::Nothing,
        },
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
        // The model reads these payloads itself: half of them navigate.
        Builtin::Launcher | Builtin::Quick | Builtin::System | Builtin::About => Plan::Nothing,
    }
}

/// Single-quote for `sh`, the way a network name with a space or a quote needs.
pub fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}

/// Carry out a command. The error is what the menu shows on screen, so it has
/// to read as a sentence, not as a debug dump.
pub fn perform(command: &Command) -> Result<(), String> {
    match plan(command) {
        Plan::Nothing => Ok(()),
        // Over the socket rather than by forking `pt35ctl`, so the reply is
        // waited for and a failure has somewhere to go. Forking gave a launch
        // with a missing binary, or a volume key with no audio backend, exactly
        // the same silence as a success.
        Plan::Ctl(args) => {
            let argv: Vec<&str> = args.iter().map(String::as_str).collect();
            let request = pt35_common::ipc::Request::from_ctl(&argv)?;
            match crate::live::request(&request) {
                Ok(pt35_common::ipc::Response::Error { message }) => Err(message),
                Ok(_) => Ok(()),
                Err(message) => Err(message),
            }
        }
        Plan::Shell(cmd) => std::process::Command::new("sh")
            .arg("-c")
            .arg(&cmd)
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("{cmd}: {e}")),
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
    fn focusing_a_window_goes_through_the_daemon() {
        assert_eq!(
            plan(&Command::Dynamic {
                builtin: Builtin::Windows,
                payload: "con:34".into()
            }),
            Plan::Ctl(vec!["window".into(), "focus".into(), "34".into()])
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
