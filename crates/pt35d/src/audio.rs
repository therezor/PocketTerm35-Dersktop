//! Volume through whatever the system actually has.
//!
//! Raspberry Pi OS Lite ships no audio stack at all; the installer adds
//! PipeWire, so `wpctl` is the primary path. `amixer` is kept as a fallback for
//! a hand-built system, and "no audio" is a valid state we report rather than
//! hide.

use pt35_common::ipc::Delta;
use std::process::Command;

const SINK: &str = "@DEFAULT_AUDIO_SINK@";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    PipeWire,
    Alsa,
    None,
}

pub fn detect() -> Backend {
    if which("wpctl") {
        Backend::PipeWire
    } else if which("amixer") {
        Backend::Alsa
    } else {
        Backend::None
    }
}

fn which(bin: &str) -> bool {
    Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {bin} >/dev/null 2>&1"))
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Current volume percentage and mute state.
pub fn state(backend: Backend) -> (Option<u8>, Option<bool>) {
    match backend {
        Backend::PipeWire => match run(&["wpctl", "get-volume", SINK]) {
            Some(out) => parse_wpctl(&out),
            None => (None, None),
        },
        Backend::Alsa => match run(&["amixer", "-M", "get", "Master"]) {
            Some(out) => parse_amixer(&out),
            None => (None, None),
        },
        Backend::None => (None, None),
    }
}

/// Apply a change. Returns false when there is no audio backend at all.
pub fn apply(backend: Backend, change: Delta) -> bool {
    match (backend, change) {
        (Backend::None, _) => false,
        (Backend::PipeWire, Delta::Mute) => run(&["wpctl", "set-mute", SINK, "toggle"]).is_some(),
        (Backend::PipeWire, Delta::Absolute(v)) => {
            run(&["wpctl", "set-volume", SINK, &format!("{}%", v.min(100))]).is_some()
        }
        (Backend::PipeWire, Delta::Relative(v)) => {
            let arg = format!("{}%{}", v.abs(), if v < 0 { "-" } else { "+" });
            run(&["wpctl", "set-volume", "-l", "1.0", SINK, &arg]).is_some()
        }
        (Backend::Alsa, Delta::Mute) => run(&["amixer", "-M", "set", "Master", "toggle"]).is_some(),
        (Backend::Alsa, Delta::Absolute(v)) => {
            run(&["amixer", "-M", "set", "Master", &format!("{}%", v.min(100))]).is_some()
        }
        (Backend::Alsa, Delta::Relative(v)) => {
            let arg = format!("{}%{}", v.abs(), if v < 0 { "-" } else { "+" });
            run(&["amixer", "-M", "set", "Master", &arg]).is_some()
        }
    }
}

fn run(argv: &[&str]) -> Option<String> {
    let out = Command::new(argv[0]).args(&argv[1..]).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// `wpctl get-volume` prints e.g. `Volume: 0.45` or `Volume: 0.45 [MUTED]`.
pub fn parse_wpctl(out: &str) -> (Option<u8>, Option<bool>) {
    let muted = out.contains("[MUTED]");
    let percent = out
        .split_whitespace()
        .nth(1)
        .and_then(|v| v.parse::<f32>().ok())
        .map(|v| (v * 100.0).round().clamp(0.0, 100.0) as u8);
    (percent, Some(muted))
}

/// `amixer get Master` prints `... [45%] [on]` somewhere in its output.
pub fn parse_amixer(out: &str) -> (Option<u8>, Option<bool>) {
    let mut percent = None;
    let mut muted = None;
    for field in out.split(['[', ']']) {
        if let Some(num) = field.strip_suffix('%') {
            percent = num.trim().parse::<u8>().ok().or(percent);
        }
        match field {
            "on" => muted = Some(false),
            "off" => muted = Some(true),
            _ => {}
        }
    }
    (percent, muted)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_wpctl_output() {
        assert_eq!(parse_wpctl("Volume: 0.45\n"), (Some(45), Some(false)));
        assert_eq!(
            parse_wpctl("Volume: 1.00 [MUTED]\n"),
            (Some(100), Some(true))
        );
        assert_eq!(parse_wpctl("nonsense"), (None, Some(false)));
    }

    #[test]
    fn parses_amixer_output() {
        let out = "Simple mixer control 'Master',0\n  Front Left: Playback 200 [45%] [on]\n";
        assert_eq!(parse_amixer(out), (Some(45), Some(false)));
        let muted = "  Front Left: Playback 0 [0%] [off]\n";
        assert_eq!(parse_amixer(muted), (Some(0), Some(true)));
    }

    #[test]
    fn no_backend_means_no_lying() {
        assert!(!apply(Backend::None, Delta::Mute));
        assert_eq!(state(Backend::None), (None, None));
    }
}
