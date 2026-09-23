//! Shared building blocks for the pt35-desktop shell.
//!
//! Everything in here is plain data: configuration loading (system defaults
//! merged with user overrides), the theme, the menu tree, application profiles
//! and the `pt35d` IPC protocol. No Wayland, no Linux-only syscalls — so it
//! builds and tests on any host.

pub mod apps;
pub mod ipc;
pub mod menu;
pub mod paths;
pub mod sway;
pub mod theme;

mod merge;

pub use merge::merge_toml;

/// Load a TOML document from the system defaults, overlaid with the user's copy.
///
/// Both files are optional: missing files fall back to `T::default()` via serde.
pub fn load_config<T: serde::de::DeserializeOwned>(relative: &str) -> anyhow::Result<T> {
    let system = paths::system_config(relative);
    let user = paths::user_config(relative);
    load_config_from(system.as_deref(), user.as_deref())
}

/// Same as [`load_config`], with explicit paths (used by tests and `--config`).
pub fn load_config_from<T: serde::de::DeserializeOwned>(
    system: Option<&std::path::Path>,
    user: Option<&std::path::Path>,
) -> anyhow::Result<T> {
    load_layers(&[system, user])
}

/// The theme, in three layers: the shipped defaults, what was picked in
/// Settings > Appearance, then the user's own `theme.toml`.
///
/// The picks live in a file of their own so the menu never rewrites one a
/// person edits by hand, and a hand edit still beats a pick.
pub fn load_theme() -> anyhow::Result<theme::Theme> {
    let system = paths::system_config("pt35/theme.toml");
    let picked = paths::appearance_path();
    let user = paths::user_config("pt35/theme.toml");
    load_layers(&[system.as_deref(), Some(&picked), user.as_deref()])
}

/// Record Appearance picks in one `section`, keeping the rest of the file.
///
/// `replace` drops what the section held first: a palette is every colour at
/// once, and an accent picked under the last palette must not outlive it.
pub fn set_appearance(
    section: &str,
    entries: &[(&str, toml::Value)],
    replace: bool,
) -> anyhow::Result<()> {
    let path = paths::appearance_path();
    let mut doc: toml::Table = match std::fs::read_to_string(&path) {
        Ok(text) => toml::from_str(&text).unwrap_or_default(),
        Err(_) => toml::Table::new(),
    };
    if replace {
        doc.remove(section);
    }
    let table = doc
        .entry(section)
        .or_insert_with(|| toml::Value::Table(toml::Table::new()));
    if let toml::Value::Table(table) = table {
        for (key, value) in entries {
            table.insert(key.to_string(), value.clone());
        }
    }
    // Checked before writing: a pick that breaks the theme would take every
    // colour back to the defaults with no word of why.
    toml::Value::Table(doc.clone()).try_into::<theme::Theme>()?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let text = format!(
        "# Written by Settings > Appearance. theme.toml next to it wins over this.\n{}",
        toml::to_string(&doc)?
    );
    std::fs::write(&path, text)?;
    Ok(())
}

fn load_layers<T: serde::de::DeserializeOwned>(
    layers: &[Option<&std::path::Path>],
) -> anyhow::Result<T> {
    let mut doc = toml::Value::Table(toml::map::Map::new());
    for path in layers.iter().copied().flatten() {
        if !path.exists() {
            continue;
        }
        let text = std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("reading {}: {e}", path.display()))?;
        let value: toml::Value = toml::from_str(&text)
            .map_err(|e| anyhow::anyhow!("parsing {}: {e}", path.display()))?;
        merge_toml(&mut doc, value);
    }
    Ok(doc.try_into()?)
}
