//! `pt35-menu` — the hub of the pt35-desktop shell.
//!
//! A fullscreen overlay: launcher, window switcher, Wi-Fi and Bluetooth
//! pickers, volume and brightness, power menu. One key opens it, arrows or
//! 1-9 pick an entry, typing filters, Escape leaves.
//!
//! It is started and stopped by `pt35d` (`pt35ctl menu toggle`) and exits as
//! soon as an entry is chosen.

// On non-Linux hosts only the portable half compiles; its helpers are then unused.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

mod exec;
mod live;
mod model;
mod providers;

#[cfg(target_os = "linux")]
mod ui;

#[cfg(target_os = "linux")]
fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();

    // `--page NAME` opens straight into a submenu (used by `pt35ctl power menu`).
    let mut args = std::env::args().skip(1);
    let mut page = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--page" => page = args.next(),
            "--help" | "-h" => {
                println!("usage: pt35-menu [--page NAME]");
                return Ok(());
            }
            other => anyhow::bail!("unknown argument {other:?}"),
        }
    }
    ui::run(page)
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("pt35-menu runs on Linux/Wayland only");
    std::process::exit(1);
}
