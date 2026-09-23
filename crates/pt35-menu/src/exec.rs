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
    /// Record an Appearance pick, `section.key=value`.
    Theme(String),
    /// Nothing to do (an informational row, e.g. on the About screen).
    Nothing,
}

pub fn plan(command: &Command) -> Plan {
    match command {
        Command::App(id) => Plan::Ctl(vec!["launch".into(), id.clone()]),
        // Through the daemon: it checks the binary exists, focuses a copy that
        // is already open, and holds the desktop's menu off until the window
        // appears.
        Command::Exec(cmd) => Plan::Ctl(vec!["exec".into(), cmd.clone()]),
        Command::Action(args) => Plan::Ctl(args.split_whitespace().map(str::to_string).collect()),
        // Straight to the shell. A helper is one of ours, it opens no window,
        // and routing it through the daemon would file it under "apps you
        // recently opened".
        Command::Helper(cmd) => Plan::Shell(cmd.clone()),
        Command::Theme(pick) => Plan::Theme(pick.clone()),
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
        // Pairing may ask for a PIN, so it runs where it can be typed.
        Builtin::Bluetooth => Plan::Shell(format!(
            "foot -a pt35-bt sh -c \"pt35-quick bluetooth connect {}; read -r _\"",
            shell_quote(payload)
        )),
        // "volume 40" / "brightness 70" are already pt35ctl commands.
        Builtin::Audio | Builtin::Display => {
            Plan::Ctl(payload.split_whitespace().map(str::to_string).collect())
        }
        Builtin::DesktopEntries => Plan::Shell(payload.to_string()),
        // The model reads these payloads itself: half of them navigate.
        Builtin::Launcher
        | Builtin::Quick
        | Builtin::System
        | Builtin::Appearance
        | Builtin::About => Plan::Nothing,
    }
}

/// Section, keys, and whether the section is replaced whole.
pub type Pick<'a> = (&'a str, Vec<(String, toml::Value)>, bool);

/// What one Appearance row writes.
pub fn picks(pick: &str) -> Result<Pick<'_>, String> {
    if let Some(name) = pick.strip_prefix("palette=") {
        let (_, colours) = crate::providers::PALETTES
            .iter()
            .find(|(n, _)| *n == name)
            .ok_or_else(|| format!("no palette {name}"))?;
        let entries = colours
            .iter()
            .map(|(key, hex)| (key.to_string(), toml::Value::String(hex.to_string())))
            .collect();
        return Ok(("color", entries, true));
    }
    let (section, key, value) =
        crate::providers::parse_pick(pick).ok_or_else(|| format!("bad pick {pick}"))?;
    let mut entries = vec![(key.to_string(), value.clone())];
    // Text on the accent has to stay readable whatever the accent is.
    if section == "color" && key == "accent" {
        if let Some(ink) = value
            .as_str()
            .and_then(|hex| hex.parse::<pt35_common::theme::Rgb>().ok())
        {
            entries.push((
                "accent_fg".into(),
                toml::Value::String(ink.ink().to_string()),
            ));
        }
    }
    Ok((section, entries, false))
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
        // waited for and a failure has somewhere to go.
        Plan::Ctl(args) => {
            let argv: Vec<&str> = args.iter().map(String::as_str).collect();
            let request = pt35_common::ipc::Request::from_ctl(&argv)?;
            match crate::live::request(&request) {
                Ok(pt35_common::ipc::Response::Error { message }) => Err(message),
                Ok(_) => Ok(()),
                Err(message) => Err(message),
            }
        }
        Plan::Theme(pick) => {
            let (section, entries, replace) = picks(&pick)?;
            let entries: Vec<(&str, toml::Value)> = entries
                .iter()
                .map(|(k, v)| (k.as_str(), v.clone()))
                .collect();
            pt35_common::set_appearance(section, &entries, replace).map_err(|e| e.to_string())?;
            // The bar watches the file. The daemon needs telling about [apps],
            // which it hands to gsettings, a palette, whose background is also
            // the wallpaper, and previews, which it captures.
            let daemon_reads = entries.iter().any(|(key, _)| *key == "window_previews");
            if section == "apps" || replace || daemon_reads {
                let _ = crate::live::request(&pt35_common::ipc::Request::Reload);
            }
            Ok(())
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
    fn a_quick_panel_helper_goes_straight_to_the_shell() {
        // Not through the daemon: toggling Wi-Fi is not an app launch and must
        // not turn up in the launcher's recents.
        assert_eq!(
            plan(&Command::Helper("pt35-quick wifi toggle".into())),
            Plan::Shell("pt35-quick wifi toggle".into())
        );
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
