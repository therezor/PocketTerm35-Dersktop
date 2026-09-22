//! User hook scripts — sxmo's extension model, kept verbatim because it is the
//! cheapest way for someone to change the shell without rebuilding it.
//!
//! `~/.config/pt35/hooks/hook_<name>` is run (if executable) when `<name>`
//! happens. Hooks are fire-and-forget: a slow or broken hook must never stall
//! the session, so nothing waits for the child.

use std::process::{Command, Stdio};

pub fn fire(name: &str, env: &[(&str, String)]) {
    let path = pt35_common::paths::hooks_dir().join(format!("hook_{name}"));
    if !is_executable(&path) {
        return;
    }
    let mut cmd = Command::new(&path);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    for (key, value) in env {
        cmd.env(key, value);
    }
    match cmd.spawn() {
        Ok(_) => log::debug!("hook {name} fired"),
        Err(e) => log::warn!("hook {name} failed to start: {e}"),
    }
}

#[cfg(unix)]
fn is_executable(path: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(path: &std::path::Path) -> bool {
    path.is_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_hook_is_silently_ignored() {
        std::env::set_var(
            "XDG_CONFIG_HOME",
            std::env::temp_dir().join("pt35-no-hooks"),
        );
        fire("startup", &[]); // must not panic
        std::env::remove_var("XDG_CONFIG_HOME");
    }
}
