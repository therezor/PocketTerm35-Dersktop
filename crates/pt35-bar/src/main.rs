//! `pt35-bar` — the one persistent piece of chrome on a 640x480 screen.
//!
//! An 18-pixel strip at the top: workspace and focused app on the left, machine
//! state and the clock on the right. It redraws once a second and sleeps in
//! between.

// On non-Linux hosts only the portable half compiles; its helpers are then unused.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

mod icons;
mod segments;
mod status;

#[cfg(target_os = "linux")]
mod ui;

#[cfg(target_os = "linux")]
fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    ui::run()
}

/// The bar needs wlr-layer-shell, so it only builds for Linux. The rest of the
/// crate (status feed, segment layout) is portable and its tests run anywhere.
#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("pt35-bar runs on Linux/Wayland only");
    std::process::exit(1);
}
