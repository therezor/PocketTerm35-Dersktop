//! Application profiles: how each app is launched and how the shell must bend
//! the screen to fit it.

use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct AppTable {
    #[serde(rename = "app")]
    pub apps: BTreeMap<String, AppProfile>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AppProfile {
    /// Shown in the menu; falls back to the id.
    pub label: String,
    /// Command line, run through `sh -c`.
    pub exec: String,
    /// How to recognise the window once it appears (sway criteria).
    /// (`match` is a keyword, so the field is `match_` with a serde rename.)
    #[serde(rename = "match")]
    pub match_: Match,
    /// Workspace the app owns. 0 = next free.
    pub workspace: u8,
    /// One or two characters for the dock and the window picker, used when the
    /// icon theme has nothing for `icon`.
    pub glyph: String,
    /// freedesktop icon name for the dock, the picker and the launcher.
    pub icon: String,
    /// How much the app should shrink its own UI. 1.0 leaves it alone.
    ///
    /// This is a toolkit scale, not the sway output scale: the panel stays at
    /// native 640x480 so the bar and the menu never change size. Qt and
    /// Chromium shrink everything, GTK3 shrinks only its text.
    pub scale: f32,
    /// Open true-fullscreen (hides our bar) — for video and games.
    pub fullscreen: bool,
    /// Whether a second copy is worth having.
    ///
    /// Off by default: four image viewers each holding a core is what picking
    /// the same row twice used to cost. A terminal is the exception, and the
    /// only one so far.
    pub multiple: bool,
    /// Extra environment for the child process.
    pub env: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Deserialize, Default, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct Match {
    pub app_id: Option<String>,
    pub class: Option<String>,
    pub title: Option<String>,
}

impl Default for AppProfile {
    fn default() -> Self {
        Self {
            label: String::new(),
            exec: String::new(),
            match_: Match::default(),
            workspace: 0,
            glyph: String::new(),
            icon: String::new(),
            scale: 1.0,
            fullscreen: false,
            multiple: false,
            env: BTreeMap::new(),
        }
    }
}

impl AppProfile {
    /// One criteria string per identifier. A Wayland build reports `app_id` and
    /// an X11 build reports `class`, so a profile that lists both needs two
    /// rules, not one rule that matches neither.
    pub fn sway_criteria_list(&self) -> Vec<String> {
        let mut out = Vec::new();
        if let Some(v) = &self.match_.app_id {
            out.push(format!("[app_id=\"{v}\"]"));
        }
        if let Some(v) = &self.match_.class {
            out.push(format!("[class=\"{v}\"]"));
        }
        if out.is_empty() {
            if let Some(v) = &self.match_.title {
                out.push(format!("[title=\"{v}\"]"));
            }
        }
        out
    }

    /// True when an open window belongs to this profile. `app` is what sway
    /// reports: `app_id` on Wayland, `class` on XWayland.
    pub fn matches_app(&self, app: &str) -> bool {
        if app.is_empty() {
            return false;
        }
        [&self.match_.app_id, &self.match_.class]
            .into_iter()
            .flatten()
            .any(|want| want.eq_ignore_ascii_case(app))
    }

    pub fn sway_criteria(&self) -> String {
        let mut parts = Vec::new();
        if let Some(v) = &self.match_.app_id {
            parts.push(format!("app_id=\"{v}\""));
        }
        if let Some(v) = &self.match_.class {
            parts.push(format!("class=\"{v}\""));
        }
        if let Some(v) = &self.match_.title {
            parts.push(format!("title=\"{v}\""));
        }
        format!("[{}]", parts.join(" "))
    }

    /// Environment that asks the toolkit to shrink the app's own UI.
    ///
    /// The output stays at 640x480, so the shell never changes size with the
    /// app. Qt takes a fractional factor and shrinks everything; GTK3 has no
    /// fractional scale at all, so `GDK_DPI_SCALE` shrinks its text and its
    /// widgets stay put. A key already in `env` wins.
    pub fn toolkit_env(&self) -> Vec<(String, String)> {
        if (self.scale - 1.0).abs() < f32::EPSILON {
            return Vec::new();
        }
        let scale = format!("{}", self.scale);
        [
            ("GDK_DPI_SCALE", scale.clone()),
            ("QT_SCALE_FACTOR", scale),
            ("QT_AUTO_SCREEN_SCALE_FACTOR", "0".to_string()),
            ("QT_ENABLE_HIGHDPI_SCALING", "0".to_string()),
        ]
        .into_iter()
        .filter(|(key, _)| !self.env.contains_key(*key))
        .map(|(key, value)| (key.to_string(), value))
        .collect()
    }

    /// Sanity limits: a scale outside this range makes the panel unusable.
    pub fn validate(&self, id: &str) -> anyhow::Result<()> {
        if self.exec.trim().is_empty() {
            anyhow::bail!("app {id:?} has no exec");
        }
        if !(0.5..=2.0).contains(&self.scale) {
            anyhow::bail!("app {id:?} scale {} is outside 0.5..=2.0", self.scale);
        }
        if self.workspace > 9 {
            anyhow::bail!("app {id:?} workspace {} is outside 0..=9", self.workspace);
        }
        Ok(())
    }
}

impl AppTable {
    pub fn validate(&self) -> anyhow::Result<()> {
        for (id, app) in &self.apps {
            app.validate(id)?;
        }
        Ok(())
    }

    pub fn get(&self, id: &str) -> Option<&AppProfile> {
        self.apps.get(id)
    }
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

/// Every program a command needs: the command itself, and for a terminal the
/// program it runs. `foot -a pt35-files yazi` needs yazi as much as foot, and a
/// missing one only shows as a window that flashes shut.
pub fn command_programs(exec: &str) -> Vec<String> {
    let mut out: Vec<String> = command_binary(exec).into_iter().collect();
    let mut words = exec.split_whitespace();
    if words.next() == Some("foot") {
        // foot's options that take a value, so the value is not the program.
        const VALUED: &[&str] = &[
            "-a",
            "--app-id",
            "-T",
            "--title",
            "-c",
            "--config",
            "-w",
            "--window-size-pixels",
            "-W",
            "--window-size-chars",
            "-o",
            "--override",
            "-D",
            "--working-directory",
            "-f",
            "--font",
        ];
        while let Some(word) = words.next() {
            if VALUED.contains(&word) {
                words.next();
            } else if !word.starts_with('-') {
                out.push(word.to_string());
                break;
            }
        }
    }
    out
}

/// Whether a command is actually installed.
///
/// `sh -c` succeeds whatever you give it, so this is the only check standing
/// between a menu row and a launch that does nothing.
pub fn on_path(binary: &str) -> bool {
    if binary.starts_with('/') {
        return std::path::Path::new(binary).exists();
    }
    let Some(path) = std::env::var_os("PATH") else {
        return true;
    };
    std::env::split_paths(&path).any(|dir| dir.join(binary).exists())
}

/// A window's app id as a person would say it: `pt35-monitor` is "Monitor",
/// `org.gnome.Nautilus` is "Nautilus".
///
/// Lives here so the dock and the window picker name a window the same way.
pub fn pretty_app(app: &str) -> String {
    let name = app
        .rsplit('.')
        .next()
        .unwrap_or(app)
        .trim_start_matches("pt35-");
    let mut chars = name.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => "Window".to_string(),
    }
}

/// Two letters for a window with neither an icon nor room for a name.
pub fn initials(app: &str) -> String {
    let cleaned: String = pretty_app(app)
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect();
    let text: String = cleaned.chars().take(2).collect();
    if text.is_empty() {
        "??".into()
    } else {
        text.to_uppercase()
    }
}

#[cfg(test)]
mod program_tests {
    use super::*;

    #[test]
    fn a_terminal_app_needs_its_program_too() {
        assert_eq!(
            command_programs("foot -a pt35-files yazi"),
            ["foot", "yazi"]
        );
        assert_eq!(command_programs("foot pt35-hello"), ["foot", "pt35-hello"]);
        assert_eq!(command_programs("foot"), ["foot"]);
        assert_eq!(command_programs("mousepad"), ["mousepad"]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn names_a_window_the_way_a_person_would() {
        assert_eq!(pretty_app("pt35-monitor"), "Monitor");
        assert_eq!(pretty_app("foot"), "Foot");
        assert_eq!(pretty_app("org.gnome.Nautilus"), "Nautilus");
        assert_eq!(pretty_app(""), "Window");
        assert_eq!(initials("pt35-monitor"), "MO");
        assert_eq!(initials("org.gnome.Nautilus"), "NA");
        assert_eq!(initials(""), "WI");
    }

    #[test]
    fn parses_a_profile() {
        let table: AppTable = toml::from_str(
            r#"
[app.browser]
label = "Browser"
exec = "chromium --ozone-platform=wayland"
match = { app_id = "chromium" }
workspace = 8
scale = 0.75
env = { FOO = "bar" }
"#,
        )
        .unwrap();
        table.validate().unwrap();
        let app = table.get("browser").unwrap();
        assert_eq!(app.scale, 0.75);
        assert_eq!(app.sway_criteria(), "[app_id=\"chromium\"]");
        assert_eq!(app.env["FOO"], "bar");
    }

    #[test]
    fn a_profile_with_both_identifiers_makes_two_rules() {
        let table: AppTable = toml::from_str(
            "[app.files]\nexec = \"pcmanfm\"\nmatch = { app_id = \"pcmanfm\", class = \"Pcmanfm\" }\n",
        )
        .unwrap();
        assert_eq!(
            table.get("files").unwrap().sway_criteria_list(),
            vec!["[app_id=\"pcmanfm\"]", "[class=\"Pcmanfm\"]"]
        );
    }

    #[test]
    fn defaults_are_conservative() {
        let table: AppTable = toml::from_str("[app.x]\nexec = \"true\"\n").unwrap();
        let app = table.get("x").unwrap();
        assert_eq!(app.scale, 1.0);
        assert!(!app.fullscreen);
        table.validate().unwrap();
    }

    #[test]
    fn a_scaled_profile_asks_the_toolkit_not_the_compositor() {
        let table: AppTable = toml::from_str(
            "[app.x]\nexec = \"true\"\nscale = 0.75\nenv = { QT_SCALE_FACTOR = \"0.5\" }\n",
        )
        .unwrap();
        let env = table.get("x").unwrap().toolkit_env();
        assert!(env.contains(&("GDK_DPI_SCALE".to_string(), "0.75".to_string())));
        assert!(
            !env.iter().any(|(k, _)| k == "QT_SCALE_FACTOR"),
            "an explicit env entry wins"
        );
    }

    #[test]
    fn an_unscaled_profile_sets_nothing() {
        let table: AppTable = toml::from_str("[app.x]\nexec = \"true\"\n").unwrap();
        assert!(table.get("x").unwrap().toolkit_env().is_empty());
    }

    #[test]
    fn a_profile_recognises_its_own_window() {
        let table: AppTable = toml::from_str(
            "[app.files]\nexec = \"pcmanfm\"\nmatch = { app_id = \"pcmanfm\", class = \"Pcmanfm\" }\n",
        )
        .unwrap();
        let app = table.get("files").unwrap();
        assert!(app.matches_app("pcmanfm"));
        assert!(app.matches_app("Pcmanfm"));
        assert!(!app.matches_app("foot"));
        assert!(!app.matches_app(""));
    }

    #[test]
    fn rejects_absurd_scale() {
        let table: AppTable = toml::from_str("[app.x]\nexec = \"true\"\nscale = 4.0\n").unwrap();
        assert!(table.validate().is_err());
    }
}
