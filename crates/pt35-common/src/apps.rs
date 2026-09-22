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
    /// sway OUTPUT scale to switch to while this app has focus.
    /// 1.0 = native 640x480; 0.75 = 853x640 logical for GUI apps that will not
    /// shrink. This is global to the panel, hence "while focused".
    pub scale: f32,
    /// Open true-fullscreen (hides our bar) — for video and games.
    pub fullscreen: bool,
    /// Whether the keyboard-driven pointer arms itself for this app.
    pub pointer: PointerPolicy,
    /// True when the face buttons should act as buttons in this app. A terminal
    /// or an editor needs the letters, a viewer does not.
    pub buttons: bool,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PointerPolicy {
    /// Never arm the pointer (keyboard-only app). The default.
    #[default]
    Off,
    /// Do not arm it, but remember this app likes it (bar hint).
    OnDemand,
    /// Arm the pointer as soon as the app takes focus.
    Auto,
}

impl Default for AppProfile {
    fn default() -> Self {
        Self {
            label: String::new(),
            exec: String::new(),
            match_: Match::default(),
            workspace: 0,
            scale: 1.0,
            fullscreen: false,
            pointer: PointerPolicy::Off,
            buttons: false,
            env: BTreeMap::new(),
        }
    }
}

impl AppProfile {
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
pointer = "auto"
env = { FOO = "bar" }
"#,
        )
        .unwrap();
        table.validate().unwrap();
        let app = table.get("browser").unwrap();
        assert_eq!(app.scale, 0.75);
        assert_eq!(app.pointer, PointerPolicy::Auto);
        assert_eq!(app.sway_criteria(), "[app_id=\"chromium\"]");
        assert_eq!(app.env["FOO"], "bar");
    }

    #[test]
    fn defaults_are_conservative() {
        let table: AppTable = toml::from_str("[app.x]\nexec = \"true\"\n").unwrap();
        let app = table.get("x").unwrap();
        assert_eq!(app.scale, 1.0);
        assert_eq!(app.pointer, PointerPolicy::Off);
        assert!(!app.fullscreen);
        table.validate().unwrap();
    }

    #[test]
    fn buttons_are_off_unless_a_profile_asks() {
        let table: AppTable =
            toml::from_str("[app.x]\nexec = \"true\"\n[app.y]\nexec = \"true\"\nbuttons = true\n")
                .unwrap();
        assert!(!table.get("x").unwrap().buttons);
        assert!(table.get("y").unwrap().buttons);
    }

    #[test]
    fn rejects_absurd_scale() {
        let table: AppTable = toml::from_str("[app.x]\nexec = \"true\"\nscale = 4.0\n").unwrap();
        assert!(table.validate().is_err());
    }
}
