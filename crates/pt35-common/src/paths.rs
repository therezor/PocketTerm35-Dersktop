//! Where pt35-desktop keeps its files.
//!
//! System defaults: `$PT35_SHARE` (set by `pt35-session`), else
//! `/usr/share/pt35-desktop`. User overrides: `$XDG_CONFIG_HOME/pt35`, else
//! `~/.config/pt35`. The runtime socket lives in `$XDG_RUNTIME_DIR`.

use std::path::{Path, PathBuf};

/// Root of the installed system configuration.
pub fn share_dir() -> PathBuf {
    std::env::var_os("PT35_SHARE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/usr/share/pt35-desktop"))
}

/// Root of the per-user configuration.
pub fn config_dir() -> PathBuf {
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(xdg).join("pt35");
    }
    match std::env::var_os("HOME") {
        Some(home) => PathBuf::from(home).join(".config/pt35"),
        None => PathBuf::from("/etc/pt35"),
    }
}

/// System copy of `relative` (e.g. `"pt35/theme.toml"`), if the tree exists.
pub fn system_config(relative: &str) -> Option<PathBuf> {
    Some(share_dir().join(relative))
}

/// User copy of `relative`; only the basename is used, the user tree is flat.
pub fn user_config(relative: &str) -> Option<PathBuf> {
    let name = Path::new(relative).file_name()?;
    Some(config_dir().join(name))
}

/// Unix socket `pt35d` listens on and `pt35ctl` talks to.
pub fn socket_path() -> PathBuf {
    let run = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    run.join("pt35.sock")
}

/// Directory for hook scripts run by `pt35d` (`hook_startup`, `hook_lowbattery`, …).
pub fn hooks_dir() -> PathBuf {
    config_dir().join("hooks")
}

/// Where the shell keeps what it learned rather than what it was told.
///
/// State, not configuration: nobody edits this and losing it costs nothing.
pub fn state_dir() -> PathBuf {
    if let Some(xdg) = std::env::var_os("XDG_STATE_HOME") {
        return PathBuf::from(xdg).join("pt35");
    }
    match std::env::var_os("HOME") {
        Some(home) => PathBuf::from(home).join(".local/state/pt35"),
        None => std::env::temp_dir().join("pt35"),
    }
}

/// The apps you have opened, most recent first.
pub fn recents_path() -> PathBuf {
    state_dir().join("recent")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_config_flattens_the_relative_path() {
        std::env::set_var("XDG_CONFIG_HOME", "/tmp/cfg");
        assert_eq!(
            user_config("pt35/theme.toml").unwrap(),
            PathBuf::from("/tmp/cfg/pt35/theme.toml")
        );
        std::env::remove_var("XDG_CONFIG_HOME");
    }

    #[test]
    fn share_dir_honours_the_env_override() {
        std::env::set_var("PT35_SHARE", "/opt/pt35");
        assert_eq!(share_dir(), PathBuf::from("/opt/pt35"));
        std::env::remove_var("PT35_SHARE");
    }
}
