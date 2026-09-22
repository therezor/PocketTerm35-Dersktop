//! Live state for the grid tiles.
//!
//! A tile that says "networks" is a label. A tile that says "wlan0" is the
//! answer to why you opened the menu.

use pt35_common::ipc::{Request, Response, Status};
use pt35_common::menu::Builtin;
use pt35_common::paths;
use std::io::{BufRead, BufReader, Write};

/// Status from pt35d, or `None` when the daemon is not answering.
pub fn status() -> Option<Status> {
    #[cfg(unix)]
    {
        use std::os::unix::net::UnixStream;
        let stream = UnixStream::connect(paths::socket_path()).ok()?;
        stream
            .set_read_timeout(Some(std::time::Duration::from_millis(400)))
            .ok()?;
        let mut writer = &stream;
        writeln!(writer, "{}", serde_json::to_string(&Request::Status).ok()?).ok()?;
        writer.flush().ok()?;
        let mut line = String::new();
        BufReader::new(&stream).read_line(&mut line).ok()?;
        match serde_json::from_str(line.trim_end()).ok()? {
            Response::Status(status) => Some(status),
            _ => None,
        }
    }
    #[cfg(not(unix))]
    None
}

/// Replace a tile's static note with what is true right now.
pub fn note(builtin: Builtin, status: Option<&Status>, windows: usize) -> Option<String> {
    let status = status?;
    Some(match builtin {
        Builtin::Windows => match windows {
            0 => "none open".into(),
            1 => "1 open".into(),
            n => format!("{n} open"),
        },
        Builtin::Wifi => status.network.clone().unwrap_or_else(|| "offline".into()),
        Builtin::Audio => match (status.volume_percent, status.muted) {
            (_, Some(true)) => "muted".into(),
            (Some(percent), _) => format!("{percent}%"),
            _ => "no audio".into(),
        },
        Builtin::Display => status
            .brightness_percent
            .map(|percent| format!("{percent}%"))
            .unwrap_or_else(|| "fixed".into()),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status() -> Status {
        Status {
            network: Some("wlan0".into()),
            volume_percent: Some(40),
            scale: 1.0,
            ..Status::default()
        }
    }

    #[test]
    fn tiles_report_live_state() {
        let s = status();
        assert_eq!(note(Builtin::Wifi, Some(&s), 0).unwrap(), "wlan0");
        assert_eq!(note(Builtin::Audio, Some(&s), 0).unwrap(), "40%");
        assert_eq!(note(Builtin::Windows, Some(&s), 3).unwrap(), "3 open");
        assert_eq!(note(Builtin::Windows, Some(&s), 1).unwrap(), "1 open");
    }

    #[test]
    fn absent_hardware_says_so_instead_of_showing_a_dash() {
        let s = Status {
            scale: 1.0,
            ..Status::default()
        };
        assert_eq!(note(Builtin::Display, Some(&s), 0).unwrap(), "fixed");
        assert_eq!(note(Builtin::Wifi, Some(&s), 0).unwrap(), "offline");
        assert_eq!(note(Builtin::Audio, Some(&s), 0).unwrap(), "no audio");
    }

    #[test]
    fn muted_beats_the_level() {
        let s = Status {
            muted: Some(true),
            volume_percent: Some(40),
            ..status()
        };
        assert_eq!(note(Builtin::Audio, Some(&s), 0).unwrap(), "muted");
    }

    #[test]
    fn no_daemon_means_no_override() {
        assert!(note(Builtin::Wifi, None, 0).is_none());
    }
}
