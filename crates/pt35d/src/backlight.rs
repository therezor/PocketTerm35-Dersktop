//! The backlight through the keyboard's RP2040, which owns the PWM pin.
//!
//! Linux has no `/sys/class/backlight` on this board. The patched firmware
//! listens on its USB console for `B<0-100>` and `B?` and answers
//! `PT35 B=<percent>`. Stock firmware ignores the lines, the reply never
//! comes, and brightness stays what it was: the Fn keys on the keyboard.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub struct SerialBacklight {
    path: PathBuf,
    /// The level the firmware last reported. `None` until it has answered.
    pub level: Option<u8>,
}

impl SerialBacklight {
    /// The keyboard's console, if it is plugged in, asked for its level once.
    pub fn find() -> Option<Self> {
        let path = console()?;
        // Raw, no echo, and no hang-up on close: every request opens it anew.
        // `time 3` makes a read give up after 0.3s, which is what stock
        // firmware costs us, once.
        let ok = Command::new("stty")
            .args([
                "-F",
                &path.to_string_lossy(),
                "raw",
                "-echo",
                "-hupcl",
                "min",
                "0",
                "time",
                "3",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
        if !ok {
            return None;
        }
        let mut backlight = Self { path, level: None };
        backlight.level = backlight.ask("B?");
        backlight.level.map(|_| backlight)
    }

    /// Set a level; returns what the firmware says it is now.
    pub fn set(&mut self, percent: u8) -> Option<u8> {
        let level = self.ask(&format!("B{}", percent.min(100)))?;
        self.level = Some(level);
        Some(level)
    }

    fn ask(&self, command: &str) -> Option<u8> {
        let mut port = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&self.path)
            .ok()?;
        // `pt35-kbd` holds this lock while it writes the firmware: a line
        // from us landing in its REPL breaks the flash. Skip, do not wait.
        if !try_lock(&port) {
            return None;
        }
        port.write_all(format!("{command}\n").as_bytes()).ok()?;
        let deadline = Instant::now() + Duration::from_millis(400);
        let mut seen = Vec::new();
        let mut chunk = [0u8; 256];
        while Instant::now() < deadline {
            match port.read(&mut chunk) {
                Ok(0) => {}
                Ok(n) => {
                    seen.extend_from_slice(&chunk[..n]);
                    if let Some(level) = parse_reply(&String::from_utf8_lossy(&seen)) {
                        return Some(level);
                    }
                }
                Err(_) => return None,
            }
        }
        None
    }
}

extern "C" {
    fn flock(fd: i32, operation: i32) -> i32;
}

/// An exclusive, non-blocking flock, released when the file closes.
fn try_lock(file: &std::fs::File) -> bool {
    use std::os::fd::AsRawFd;
    const LOCK_EX: i32 = 2;
    const LOCK_NB: i32 = 4;
    // SAFETY: a valid open fd and plain integer flags.
    unsafe { flock(file.as_raw_fd(), LOCK_EX | LOCK_NB) == 0 }
}

/// The keyboard's console under /dev/serial/by-id, else the first ACM port.
fn console() -> Option<PathBuf> {
    let by_id = Path::new("/dev/serial/by-id");
    let named = std::fs::read_dir(by_id)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .find(|p| p.to_string_lossy().contains("Pico"));
    named.or_else(|| Some(PathBuf::from("/dev/ttyACM0")).filter(|p| p.exists()))
}

/// The last `PT35 B=<n>` in what came back. The firmware prints debug lines
/// on the same port, and CircuitPython a title-bar escape with no newline the
/// moment the port opens, so the reply is searched for, not read as a line.
pub fn parse_reply(text: &str) -> Option<u8> {
    let (_, after) = text.rsplit_once("PT35 B=")?;
    let digits: String = after.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_reply_is_found_among_debug_lines() {
        assert_eq!(parse_reply("[HID] ready\r\nPT35 B=40\r\n"), Some(40));
        assert_eq!(
            parse_reply("PT35 B=40\nPT35 B=55\n"),
            Some(55),
            "the latest"
        );
        assert_eq!(parse_reply("bl_pwm: 3000\n"), None);
        let opened = "\x1b]0;code.py | 10.0.0\x1b\\PT35 B=92\r\n";
        assert_eq!(parse_reply(opened), Some(92), "after the title escape");
    }
}
