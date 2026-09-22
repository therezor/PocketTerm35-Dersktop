//! `pt35ctl` — the one command sway keybindings, menu entries and hook scripts
//! use to talk to `pt35d`.
//!
//! Hand-rolled argument parsing: this binary is on the hot path of every key
//! press, so it stays free of clap and starts in ~1 ms.

use anyhow::{bail, Result};
use pt35_common::ipc::{Request, Response, Status};

mod client;

const USAGE: &str = "\
pt35ctl — control the PocketTerm35 desktop session

usage:
  pt35ctl menu [open|close|toggle] [PAGE]
  pt35ctl launch APP
  pt35ctl exec COMMAND...
  pt35ctl volume +5 | -5 | 50 | mute
  pt35ctl brightness +10 | -10 | 50
  pt35ctl mode [toggle|buttons|mouse]
  pt35ctl scale [1.0|0.75|cycle]
  pt35ctl window fit|close|next|prev|fullscreen
  pt35ctl window focus|close ID
  pt35ctl window closeall
  pt35ctl screenshot
  pt35ctl cpu powersave|balanced|performance
  pt35ctl power screenoff|lock|logout|reboot|poweroff|menu
  pt35ctl touch on|off|toggle
  pt35ctl reload
  pt35ctl status [--json]
";

fn main() {
    if let Err(err) = run() {
        eprintln!("pt35ctl: {err:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let argv: Vec<&str> = args.iter().map(String::as_str).collect();

    match argv.as_slice() {
        [] | ["-h"] | ["--help"] | ["help"] => {
            print!("{USAGE}");
            Ok(())
        }
        ["--version"] => {
            println!("pt35ctl {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        ["status", rest @ ..] => status(rest.contains(&"--json")),
        rest => {
            let request = Request::from_ctl(rest).map_err(anyhow::Error::msg)?;
            match client::send(&request)? {
                Response::Ok => Ok(()),
                Response::Error { message } => bail!(message),
                Response::Status(s) => {
                    print_status(&s);
                    Ok(())
                }
            }
        }
    }
}

/// Turn an argv slice into a request. Kept pure so it can be unit-tested.
fn status(json: bool) -> Result<()> {
    match client::send(&Request::Status)? {
        Response::Status(s) if json => {
            println!("{}", serde_json::to_string(&s)?);
            Ok(())
        }
        Response::Status(s) => {
            print_status(&s);
            Ok(())
        }
        Response::Error { message } => bail!(message),
        Response::Ok => bail!("daemon returned no status"),
    }
}

fn print_status(s: &Status) {
    println!("workspace   {}", s.workspace);
    println!("app         {}", s.app.as_deref().unwrap_or("-"));
    println!("scale       {}", s.scale);
    println!(
        "battery     {}",
        match (s.battery_percent, s.charging) {
            (Some(p), Some(true)) => format!("{p}% charging"),
            (Some(p), _) => format!("{p}%"),
            (None, _) => "not exposed by this hardware".to_string(),
        }
    );
    println!(
        "brightness  {}",
        s.brightness_percent
            .map(|p| format!("{p}%"))
            .unwrap_or_else(|| "not exposed by this hardware".into())
    );
    println!(
        "volume      {}{}",
        s.volume_percent
            .map(|p| format!("{p}%"))
            .unwrap_or_else(|| "-".into()),
        if s.muted.unwrap_or(false) {
            " (muted)"
        } else {
            ""
        }
    );
    println!("network     {}", s.network.as_deref().unwrap_or("-"));
    println!("mode        {}", s.input_mode.label());
}
