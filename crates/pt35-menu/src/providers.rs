//! The live data behind the builtin screens.
//!
//! Each provider is split in two: a pure parser (tested here) and a thin
//! wrapper that actually runs the command or walks the filesystem.

use pt35_common::menu::Builtin;
use std::process::Command;

/// One row of a dynamic screen. `payload` is opaque to the model: only the
/// provider and the code that acts on the row know what it means.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Item {
    pub label: String,
    pub payload: String,
    /// Second line on a tile, right-hand text on a row.
    pub note: String,
    /// One or two characters standing in for an icon.
    pub glyph: String,
    /// freedesktop icon name, preferred over the glyph.
    pub icon: String,
}

impl Item {
    pub fn new(label: impl Into<String>, payload: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            payload: payload.into(),
            ..Self::default()
        }
    }
}

pub type Items = Vec<Item>;

pub fn title(builtin: Builtin) -> &'static str {
    match builtin {
        Builtin::Launcher => "Launcher",
        Builtin::Windows => "Windows",
        Builtin::Wifi => "Wi-Fi",
        Builtin::Bluetooth => "Bluetooth",
        Builtin::Audio => "Volume",
        Builtin::Display => "Brightness",
        Builtin::DesktopEntries => "All apps",
        Builtin::About => "About",
    }
}

pub fn items(builtin: Builtin) -> Items {
    match builtin {
        Builtin::Launcher => launcher(),
        Builtin::Windows => windows(),
        Builtin::Wifi => wifi(),
        Builtin::Bluetooth => bluetooth(),
        Builtin::Audio => levels("volume"),
        Builtin::Display => levels("brightness"),
        Builtin::DesktopEntries => desktop_entries(),
        Builtin::About => about(),
    }
}

// --------------------------------------------------------------- launcher

/// The root screen: the four screens that are not apps, then the apps with a
/// profile, then everything else that is installed. One list, so typing finds
/// any of it.
///
/// The payload says what activating a row means, because these rows are not all
/// the same kind of thing: `screen:`, `app:` or `exec:`.
fn launcher() -> Items {
    let mut out = vec![
        Item {
            label: "Windows".into(),
            payload: "screen:windows".into(),
            note: "running".into(),
            glyph: "W".into(),
            icon: "multitasking-view".into(),
        },
        Item {
            label: "Quick".into(),
            payload: "page:quick".into(),
            note: "toggles".into(),
            glyph: "Q".into(),
            icon: "preferences-desktop".into(),
        },
        Item {
            label: "Settings".into(),
            payload: "page:settings".into(),
            note: "system".into(),
            glyph: "S".into(),
            icon: "preferences-system".into(),
        },
        Item {
            label: "Power".into(),
            payload: "page:power".into(),
            note: "off".into(),
            glyph: "P".into(),
            icon: "system-shutdown".into(),
        },
    ];

    let apps: pt35_common::apps::AppTable =
        pt35_common::load_config("pt35/apps.toml").unwrap_or_default();
    for (id, app) in &apps.apps {
        out.push(Item {
            label: if app.label.is_empty() {
                pretty_app(id)
            } else {
                app.label.clone()
            },
            payload: format!("app:{id}"),
            note: String::new(),
            glyph: app.glyph.clone(),
            icon: app.icon.clone(),
        });
    }

    // Anything else with a .desktop file, so search covers the whole machine.
    let known: Vec<String> = out.iter().map(|i| i.label.to_lowercase()).collect();
    for mut entry in desktop_entries() {
        if !known.contains(&entry.label.to_lowercase()) {
            entry.payload = format!("exec:{}", entry.payload);
            out.push(entry);
        }
    }
    out
}

// ---------------------------------------------------------------- windows

/// The picker. The daemon already knows every window and which profile owns it,
/// so the glyph comes back with the list instead of being guessed here.
fn windows() -> Items {
    let Some(status) = crate::live::status() else {
        return Vec::new();
    };
    status
        .windows
        .iter()
        .map(|w| Item {
            label: pretty_app(&w.app),
            payload: format!("[con_id={}]", w.id),
            note: w.title.clone(),
            glyph: if w.glyph.is_empty() {
                initials(&w.app)
            } else {
                w.glyph.clone()
            },
            icon: w.icon.clone(),
        })
        .collect()
}

/// `pt35-monitor` is what sway calls it; "Monitor" is what it is.
fn pretty_app(app: &str) -> String {
    let name = app.trim_start_matches("pt35-");
    let mut chars = name.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => "Window".to_string(),
    }
}

/// Two letters for an app with no profile, the same rule the dock uses.
fn initials(app: &str) -> String {
    let name = app.trim_start_matches("pt35-");
    name.chars().take(2).collect::<String>().to_uppercase()
}

// ------------------------------------------------------------------- wifi

fn wifi() -> Items {
    match run(
        "nmcli",
        &["-t", "-f", "ACTIVE,SIGNAL,SSID", "device", "wifi", "list"],
    ) {
        Some(out) => parse_nmcli(&out),
        None => vec![Item::new("nmcli not installed", "")],
    }
}

/// nmcli terse output: `ACTIVE:SIGNAL:SSID`, strongest first, no duplicates.
pub fn parse_nmcli(out: &str) -> Items {
    let mut seen: Vec<(String, i32, bool)> = Vec::new();
    for line in out.lines() {
        let mut fields = line.splitn(3, ':');
        let active = fields.next().unwrap_or("no") == "yes";
        let signal: i32 = fields.next().unwrap_or("0").parse().unwrap_or(0);
        let ssid = fields.next().unwrap_or("").trim().to_string();
        if ssid.is_empty() {
            continue;
        }
        match seen.iter_mut().find(|(name, ..)| *name == ssid) {
            Some(entry) => {
                entry.1 = entry.1.max(signal);
                entry.2 |= active;
            }
            None => seen.push((ssid, signal, active)),
        }
    }
    seen.sort_by_key(|entry| std::cmp::Reverse(entry.1));
    seen.into_iter()
        .map(|(ssid, signal, active)| Item {
            label: ssid.clone(),
            payload: ssid,
            note: format!("{signal}%"),
            glyph: if active { "*".into() } else { String::new() },
            icon: String::new(),
        })
        .collect()
}

// -------------------------------------------------------------- bluetooth

fn bluetooth() -> Items {
    match run("bluetoothctl", &["devices"]) {
        Some(out) => parse_bluetoothctl(&out),
        None => vec![Item::new("bluetoothctl not installed", "")],
    }
}

/// `Device AA:BB:CC:DD:EE:FF Name Of Thing`
pub fn parse_bluetoothctl(out: &str) -> Items {
    out.lines()
        .filter_map(|line| {
            let rest = line.strip_prefix("Device ")?;
            let (mac, name) = rest.split_once(' ')?;
            Some(Item::new(name.trim(), mac))
        })
        .collect()
}

// ----------------------------------------------------------------- levels

/// Volume and brightness screens are just a ladder of absolute values: a slider
/// is useless without a pointer, a list of ten steps is two key presses.
fn levels(what: &str) -> Items {
    (0..=10)
        .rev()
        .map(|step| {
            let percent = step * 10;
            Item::new(format!("{percent:>3}%"), format!("{what} {percent}"))
        })
        .collect()
}

// --------------------------------------------------------- desktop entries

fn desktop_entries() -> Items {
    let mut dirs = vec![std::path::PathBuf::from("/usr/share/applications")];
    if let Some(home) = std::env::var_os("HOME") {
        dirs.push(std::path::PathBuf::from(home).join(".local/share/applications"));
    }
    let mut out: Items = Vec::new();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("desktop") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            if let Some((name, exec)) = parse_desktop_entry(&text) {
                out.push(Item::new(name, exec));
            }
        }
    }
    out.sort_by_key(|entry| entry.label.to_lowercase());
    out.dedup_by(|a, b| a.label == b.label);
    out
}

/// Pull `Name` and `Exec` out of a .desktop file, skipping hidden ones and
/// stripping the field codes (`%U`, `%f`, …) that would confuse `sh -c`.
pub fn parse_desktop_entry(text: &str) -> Option<(String, String)> {
    let mut name = None;
    let mut exec = None;
    let mut in_main_section = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_main_section = line == "[Desktop Entry]";
            continue;
        }
        if !in_main_section {
            continue;
        }
        match line.split_once('=') {
            Some(("Name", value)) => name = Some(value.trim().to_string()),
            Some(("Exec", value)) => exec = Some(value.trim().to_string()),
            Some(("NoDisplay", value)) | Some(("Hidden", value))
                if value.trim().eq_ignore_ascii_case("true") =>
            {
                return None
            }
            Some(("Type", value)) if value.trim() != "Application" => return None,
            _ => {}
        }
    }
    let exec = exec?
        .split_whitespace()
        .filter(|word| !(word.len() == 2 && word.starts_with('%')))
        .collect::<Vec<_>>()
        .join(" ");
    Some((name?, exec))
}

// ------------------------------------------------------------------ about

fn about() -> Items {
    let mut out = vec![
        Item::new(format!("pt35-desktop {}", env!("CARGO_PKG_VERSION")), ""),
        Item::new(
            format!(
                "panel {}",
                std::env::var("WAYLAND_DISPLAY").unwrap_or_else(|_| "?".into())
            ),
            "",
        ),
    ];
    if let Ok(model) = std::fs::read_to_string("/proc/device-tree/model") {
        out.push(Item::new(model.trim_end_matches('\0').trim(), ""));
    }
    out
}

fn run(bin: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(bin).args(args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_app_without_a_profile_still_gets_a_name_and_two_letters() {
        assert_eq!(pretty_app("pt35-monitor"), "Monitor");
        assert_eq!(pretty_app("chromium"), "Chromium");
        assert_eq!(pretty_app(""), "Window");
        assert_eq!(initials("pcmanfm"), "PC");
        assert_eq!(initials("pt35-monitor"), "MO");
    }

    #[test]
    fn deduplicates_and_sorts_wifi_networks() {
        let out = "no:42:Cafe\nyes:78:Home\nno:55:Home\nno:0:\n";
        let items = parse_nmcli(out);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].label, "Home");
        assert_eq!(items[0].glyph, "*", "the active network is marked");
        assert_eq!(items[0].note, "78%", "keeps the strongest signal");
    }

    #[test]
    fn parses_bluetooth_devices() {
        let out = "Device AA:BB:CC:DD:EE:FF Keyboard K380\nnoise\n";
        assert_eq!(
            parse_bluetoothctl(out),
            vec![Item::new("Keyboard K380", "AA:BB:CC:DD:EE:FF")]
        );
    }

    #[test]
    fn parses_a_desktop_entry_and_strips_field_codes() {
        let text = "[Desktop Entry]\nType=Application\nName=Image Viewer\nExec=imv %U\n";
        assert_eq!(
            parse_desktop_entry(text),
            Some(("Image Viewer".to_string(), "imv".to_string()))
        );
    }

    #[test]
    fn hides_nodisplay_and_non_application_entries() {
        assert_eq!(
            parse_desktop_entry(
                "[Desktop Entry]\nType=Application\nName=x\nExec=x\nNoDisplay=true\n"
            ),
            None
        );
        assert_eq!(
            parse_desktop_entry("[Desktop Entry]\nType=Link\nName=x\nExec=x\n"),
            None
        );
        assert_eq!(
            parse_desktop_entry("[Desktop Action new]\nName=x\nExec=x\n"),
            None,
            "only the main section counts"
        );
    }

    #[test]
    fn level_ladders_run_from_full_to_zero() {
        let items = levels("volume");
        assert_eq!(items.len(), 11);
        assert_eq!(items[0].payload, "volume 100");
        assert_eq!(items[10].payload, "volume 0");
    }
}
