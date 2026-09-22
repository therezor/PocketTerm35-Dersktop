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
    let mut doc = toml::Value::Table(toml::map::Map::new());
    for path in [system, user].into_iter().flatten() {
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
