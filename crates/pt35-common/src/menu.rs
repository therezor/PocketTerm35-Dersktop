//! The menu tree: a map of named menus, each a list of entries.
//!
//! Shape is deliberately flat so that a user file can replace a single menu
//! (`[menu.apps]`) without restating the rest.

use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MenuTree {
    /// Id of the menu shown when the menu key is pressed.
    pub root: String,
    #[serde(rename = "menu")]
    pub menus: BTreeMap<String, MenuPage>,
}

impl Default for MenuTree {
    fn default() -> Self {
        Self {
            root: "main".into(),
            menus: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct MenuPage {
    pub title: String,
    /// Tiles or rows. A grid reads better for a small set of destinations you
    /// pick by shape; a list is better for many similar items you scan.
    pub layout: Layout,
    pub entries: Vec<Entry>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Layout {
    #[default]
    List,
    Grid,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct Entry {
    pub label: String,
    /// Second line on a grid tile: what this is, or its current state.
    pub note: String,
    /// One or two characters drawn in the tile's coloured badge, when the icon
    /// theme has nothing for `icon`.
    pub glyph: String,
    /// freedesktop icon name, drawn in the badge when the theme has it.
    pub icon: String,
    /// Badge colour, `#rrggbb`. Defaults to the theme accent.
    pub tint: Option<crate::theme::Rgb>,
    /// Makes this row a quick setting: the D-pad changes the value in place
    /// instead of opening anything. One of volume, brightness, scale.
    pub adjust: Option<Adjust>,
    /// Draws the current value on the right of the row. A toggle without one
    /// is a button that gives no clue whether it is on.
    pub state: Option<StateField>,
    /// Ask before running (used for reboot / shut down).
    pub confirm: bool,
    pub goto: Option<String>,
    pub app: Option<String>,
    pub exec: Option<String>,
    pub action: Option<String>,
    pub builtin: Option<Builtin>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Adjust {
    Volume,
    Brightness,
    Scale,
}

impl Adjust {
    /// The pt35ctl arguments for one step in either direction.
    pub fn step(self, up: bool) -> Vec<String> {
        let arg = |v: &str| v.to_string();
        match self {
            Adjust::Volume => vec![arg("volume"), arg(if up { "+5" } else { "-5" })],
            Adjust::Brightness => vec![arg("brightness"), arg(if up { "+10" } else { "-10" })],
            Adjust::Scale => vec![arg("scale"), arg("cycle")],
        }
    }
}

/// A value from the daemon's status, drawn at the end of a row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StateField {
    Mode,
    Touch,
    Volume,
    Brightness,
    Network,
    Scale,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Builtin {
    /// Everything you can start, plus the screens that are not apps.
    Launcher,
    /// Switches and sliders: the things you change without leaving what you
    /// were doing.
    Quick,
    /// A read-only dashboard: load, memory, storage, network, uptime.
    System,
    Windows,
    Wifi,
    Bluetooth,
    Audio,
    Display,
    DesktopEntries,
    About,
}

impl Builtin {
    /// `pt35ctl menu open windows` names a screen, and a builtin is a screen
    /// even though it is not a page in this file.
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "launcher" => Builtin::Launcher,
            "quick" => Builtin::Quick,
            "system" => Builtin::System,
            "windows" => Builtin::Windows,
            "wifi" => Builtin::Wifi,
            "bluetooth" => Builtin::Bluetooth,
            "audio" => Builtin::Audio,
            "display" => Builtin::Display,
            "desktop_entries" => Builtin::DesktopEntries,
            "about" => Builtin::About,
            _ => return None,
        })
    }
}

/// What an entry does, resolved once at load time so the UI never has to guess.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    /// A quick setting with no submenu behind it.
    Adjust,
    Goto(String),
    App(String),
    Exec(String),
    Action(String),
    Builtin(Builtin),
}

impl Entry {
    /// Exactly one of the action fields must be set. An `adjust` row may have
    /// none: the D-pad is the whole interaction.
    pub fn kind(&self) -> Result<Kind, EntryError> {
        if self.adjust.is_some()
            && self.goto.is_none()
            && self.app.is_none()
            && self.exec.is_none()
            && self.action.is_none()
            && self.builtin.is_none()
        {
            return Ok(Kind::Adjust);
        }
        let mut found = Vec::new();
        if let Some(v) = &self.goto {
            found.push(Kind::Goto(v.clone()));
        }
        if let Some(v) = &self.app {
            found.push(Kind::App(v.clone()));
        }
        if let Some(v) = &self.exec {
            found.push(Kind::Exec(v.clone()));
        }
        if let Some(v) = &self.action {
            found.push(Kind::Action(v.clone()));
        }
        if let Some(v) = self.builtin {
            found.push(Kind::Builtin(v));
        }
        match found.len() {
            1 => Ok(found.remove(0)),
            0 => Err(EntryError::Empty(self.label.clone())),
            n => Err(EntryError::Ambiguous(self.label.clone(), n)),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum EntryError {
    #[error("menu entry {0:?} does nothing: set one of goto/app/exec/action/builtin")]
    Empty(String),
    #[error("menu entry {0:?} sets {1} actions; exactly one is allowed")]
    Ambiguous(String, usize),
}

#[derive(Debug, thiserror::Error)]
pub enum TreeError {
    #[error("root menu {0:?} is not defined")]
    MissingRoot(String),
    #[error("menu {0:?} points at {1:?}, which is not defined")]
    DanglingGoto(String, String),
    #[error(transparent)]
    Entry(#[from] EntryError),
}

impl MenuTree {
    /// Reject a tree the shell could get stuck in: missing root, dangling
    /// submenu, or an entry with no (or more than one) action.
    pub fn validate(&self) -> Result<(), TreeError> {
        // The root may name a builtin screen instead of a page here: the
        // launcher is assembled from apps.toml and the installed .desktop
        // files, so there is nothing to write down.
        if !self.menus.contains_key(&self.root) && Builtin::from_name(&self.root).is_none() {
            return Err(TreeError::MissingRoot(self.root.clone()));
        }
        for (id, page) in &self.menus {
            for entry in &page.entries {
                if let Kind::Goto(target) = entry.kind()? {
                    if !self.menus.contains_key(&target) {
                        return Err(TreeError::DanglingGoto(id.clone(), target));
                    }
                }
            }
        }
        Ok(())
    }

    pub fn page(&self, id: &str) -> Option<&MenuPage> {
        self.menus.get(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
root = "main"

[menu.main]
title = "PT35"
entries = [
  { label = "Applications", goto = "apps" },
  { label = "Power off", action = "power poweroff", confirm = true },
]

[menu.apps]
title = "Applications"
entries = [ { label = "Terminal", app = "terminal" } ]
"#;

    #[test]
    fn parses_and_validates() {
        let tree: MenuTree = toml::from_str(SAMPLE).unwrap();
        tree.validate().unwrap();
        assert_eq!(tree.page("main").unwrap().entries.len(), 2);
        assert_eq!(
            tree.page("main").unwrap().entries[0].kind().unwrap(),
            Kind::Goto("apps".into())
        );
        assert!(tree.page("main").unwrap().entries[1].confirm);
    }

    #[test]
    fn rejects_dangling_submenu() {
        let tree: MenuTree = toml::from_str(
            "root = \"main\"\n[menu.main]\nentries = [{ label=\"x\", goto=\"nope\" }]\n",
        )
        .unwrap();
        assert!(matches!(tree.validate(), Err(TreeError::DanglingGoto(..))));
    }

    #[test]
    fn rejects_entry_with_two_actions() {
        let tree: MenuTree = toml::from_str(
            "root = \"main\"\n[menu.main]\nentries = [{ label=\"x\", exec=\"ls\", action=\"reload\" }]\n",
        )
        .unwrap();
        assert!(matches!(
            tree.validate(),
            Err(TreeError::Entry(EntryError::Ambiguous(..)))
        ));
    }

    #[test]
    fn rejects_missing_root() {
        let tree: MenuTree = toml::from_str("root = \"nope\"\n[menu.main]\n").unwrap();
        assert!(matches!(tree.validate(), Err(TreeError::MissingRoot(_))));
    }
}
