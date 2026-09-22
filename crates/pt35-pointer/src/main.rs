//! `pt35-pointer` — a mouse for a device that has none.
//!
//! The PocketTerm35 has a D-pad, a touchscreen and no pointing device. Most
//! GUI applications still assume a cursor somewhere: a right-click menu, a
//! canvas, a drag handle. This binary drives the compositor's real cursor from
//! the keyboard, either by nudging it with the D-pad or by jumping straight to
//! a labelled cell of a two-level grid.
//!
//! It is started and stopped by `pt35d` (`pt35ctl pointer toggle`) and holds
//! the keyboard while it runs, so Escape disarms it.

// On non-Linux hosts only the portable half compiles; its helpers are then unused.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

mod grid;
mod motion;

#[cfg(target_os = "linux")]
mod app;
#[cfg(target_os = "linux")]
mod driver;
#[cfg(target_os = "linux")]
mod virtual_pointer;

#[cfg(target_os = "linux")]
fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();

    let mut args = std::env::args().skip(1);
    let mut grid = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--mode" => grid = args.next().as_deref() == Some("grid"),
            "--help" | "-h" => {
                println!("usage: pt35-pointer [--mode move|grid]");
                return Ok(());
            }
            other => anyhow::bail!("unknown argument {other:?}"),
        }
    }
    app::run(grid)
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("pt35-pointer runs on Linux/Wayland only");
    std::process::exit(1);
}
