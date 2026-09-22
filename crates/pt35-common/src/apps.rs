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
    /// One or two characters for the dock and the window picker. There is no
    /// icon theme on this device and no room to draw one at 640x480.
    pub glyph: String,
    /// How much the app should shrink its own UI. 1.0 leaves it alone.
    ///
    /// This is a toolkit scale, not the sway output scale: the panel stays at
    /// native 640x480 so the bar and the menu never change size. Qt and
    /// Chromium shrink everything, GTK3 shrinks only its text.
    pub scale: f32,
    /// Open true-fullscreen (hides our bar) — for video and games.
    pub fullscreen: bool,
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
            scale: 1.0,
            fullscreen: false,
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

#[cfg(test)]
mod tests {
    use super::*;

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
