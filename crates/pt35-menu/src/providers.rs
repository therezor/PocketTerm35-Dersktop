//! The live data behind the builtin screens.
//!
//! Each provider is split in two: a pure parser (tested here) and a thin
//! wrapper that actually runs the command or walks the filesystem.

use pt35_common::menu::Builtin;
use std::process::Command;

/// `(label, payload)` pairs for a dynamic screen.
pub type Items = Vec<(String, String)>;

pub fn title(builtin: Builtin) -> &'static str {
    match builtin {
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
        Builtin::Windows => windows(),
        Builtin::Wifi => wifi(),
        Builtin::Bluetooth => bluetooth(),
        Builtin::Audio => levels("volume"),
        Builtin::Display => levels("brightness"),
        Builtin::DesktopEntries => desktop_entries(),
        Builtin::About => about(),
    }
}

// ---------------------------------------------------------------- windows

fn windows() -> Items {
    match pt35_common::sway::Sway::connect()
        .and_then(|mut s| s.request(pt35_common::sway::MessageType::GetTree, ""))
    {
        Ok(json) => match serde_json::from_str(&json) {
            Ok(tree) => parse_windows(&tree),
            Err(e) => {
                log::warn!("sway tree: {e}");
                Vec::new()
            }
        },
        Err(e) => {
            log::warn!("sway: {e}");
            Vec::new()
        }
    }
}

/// Flatten a sway tree into "workspace: window title" rows whose payload is the
/// container id, so activating one focuses exactly that window.
pub fn parse_windows(node: &serde_json::Value) -> Items {
    fn walk(node: &serde_json::Value, workspace: Option<&str>, out: &mut Items) {
        let kind = node["type"].as_str().unwrap_or("");
        let name = node["name"].as_str().unwrap_or("");
        let workspace = if kind == "workspace" {
            Some(name)
        } else {
            workspace
        };

        let is_window = node.get("pid").is_some()
            || node.get("app_id").and_then(|v| v.as_str()).is_some()
            || node.get("window").and_then(|v| v.as_i64()).is_some();
        if is_window && !name.is_empty() {
            if let Some(id) = node["id"].as_i64() {
                // "2  pcmanfm  Home": workspace, app, then the title, so the
                // list scans down the left edge.
                let app = node["app_id"]
                    .as_str()
                    .or_else(|| node["window_properties"]["class"].as_str())
                    .unwrap_or("");
                let label = match (workspace, app.is_empty()) {
                    (Some(ws), false) => format!("{ws}  {app}  {name}"),
                    (Some(ws), true) => format!("{ws}  {name}"),
                    (None, _) => name.to_string(),
                };
                out.push((label, format!("[con_id={id}]")));
            }
        }
        for key in ["nodes", "floating_nodes"] {
            if let Some(children) = node[key].as_array() {
                for child in children {
                    walk(child, workspace, out);
                }
            }
        }
    }

    let mut out = Vec::new();
    walk(node, None, &mut out);
    out
}

// ------------------------------------------------------------------- wifi

fn wifi() -> Items {
    match run(
        "nmcli",
        &["-t", "-f", "ACTIVE,SIGNAL,SSID", "device", "wifi", "list"],
    ) {
        Some(out) => parse_nmcli(&out),
        None => vec![("nmcli not installed".into(), String::new())],
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
        .map(|(ssid, signal, active)| {
            let mark = if active { "* " } else { "  " };
            (format!("{mark}{ssid}  {signal}%"), ssid)
        })
        .collect()
}

// -------------------------------------------------------------- bluetooth

fn bluetooth() -> Items {
    match run("bluetoothctl", &["devices"]) {
        Some(out) => parse_bluetoothctl(&out),
        None => vec![("bluetoothctl not installed".into(), String::new())],
    }
}

/// `Device AA:BB:CC:DD:EE:FF Name Of Thing`
pub fn parse_bluetoothctl(out: &str) -> Items {
    out.lines()
        .filter_map(|line| {
            let rest = line.strip_prefix("Device ")?;
            let (mac, name) = rest.split_once(' ')?;
            Some((name.trim().to_string(), mac.to_string()))
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
            (format!("{percent:>3}%"), format!("{what} {percent}"))
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
            if let Some(item) = parse_desktop_entry(&text) {
                out.push(item);
            }
        }
    }
    out.sort_by_key(|entry| entry.0.to_lowercase());
    out.dedup_by(|a, b| a.0 == b.0);
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
        (
            format!("pt35-desktop {}", env!("CARGO_PKG_VERSION")),
            String::new(),
        ),
        (
            format!(
                "panel {}",
                std::env::var("WAYLAND_DISPLAY").unwrap_or_else(|_| "?".into())
            ),
            String::new(),
        ),
    ];
    if let Ok(model) = std::fs::read_to_string("/proc/device-tree/model") {
        out.push((
            model.trim_end_matches('\0').trim().to_string(),
            String::new(),
        ));
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
    fn flattens_a_sway_tree_into_windows() {
        let tree: serde_json::Value = serde_json::from_str(
            r#"{"type":"root","name":"root","nodes":[
                 {"type":"output","name":"HDMI-A-1","nodes":[
                   {"type":"workspace","name":"1","nodes":[
                     {"type":"con","id":12,"name":"foot","app_id":"foot","pid":900}
                   ],"floating_nodes":[]},
                   {"type":"workspace","name":"2","nodes":[
                     {"type":"con","id":34,"name":"helix","app_id":"pt35-editor","pid":901}
                   ],"floating_nodes":[]}
                 ],"floating_nodes":[]}
               ],"floating_nodes":[]}"#,
        )
        .unwrap();
        let items = parse_windows(&tree);
        assert_eq!(
            items,
            vec![
                ("1  foot  foot".to_string(), "[con_id=12]".to_string()),
                (
                    "2  pt35-editor  helix".to_string(),
                    "[con_id=34]".to_string()
                ),
            ]
        );
    }

    #[test]
    fn skips_workspaces_and_outputs_themselves() {
        let tree: serde_json::Value = serde_json::from_str(
            r#"{"type":"workspace","id":5,"name":"1","nodes":[],"floating_nodes":[]}"#,
        )
        .unwrap();
        assert!(parse_windows(&tree).is_empty());
    }

    #[test]
    fn deduplicates_and_sorts_wifi_networks() {
        let out = "no:42:Cafe\nyes:78:Home\nno:55:Home\nno:0:\n";
        let items = parse_nmcli(out);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].1, "Home");
        assert!(
            items[0].0.starts_with('*'),
            "the active network is marked: {:?}",
            items[0].0
        );
        assert!(
            items[0].0.contains("78%"),
            "keeps the strongest signal: {:?}",
            items[0].0
        );
    }

    #[test]
    fn parses_bluetooth_devices() {
        let out = "Device AA:BB:CC:DD:EE:FF Keyboard K380\nnoise\n";
        assert_eq!(
            parse_bluetoothctl(out),
            vec![("Keyboard K380".to_string(), "AA:BB:CC:DD:EE:FF".to_string())]
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
        assert_eq!(items[0].1, "volume 100");
        assert_eq!(items[10].1, "volume 0");
    }
}
