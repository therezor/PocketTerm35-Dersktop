//! Live state for the grid tiles.
//!
//! A tile that says "networks" is a label. A tile that says "wlan0" is the
//! answer to why you opened the menu.

use pt35_common::ipc::{Request, Response, Status};
use pt35_common::menu::{Adjust, Builtin, StateField};
use pt35_common::paths;
use std::io::{BufRead, BufReader, Write};

/// Send one request and wait for the reply.
#[cfg(unix)]
pub fn request(req: &Request) -> Result<Response, String> {
    use std::os::unix::net::UnixStream;

    let stream = UnixStream::connect(paths::socket_path())
        .map_err(|_| "pt35d is not running".to_string())?;
    stream
        .set_read_timeout(Some(std::time::Duration::from_millis(2000)))
        .map_err(|e| e.to_string())?;
    let mut writer = &stream;
    let line = serde_json::to_string(req).map_err(|e| e.to_string())?;
    writeln!(writer, "{line}").map_err(|e| e.to_string())?;
    writer.flush().map_err(|e| e.to_string())?;

    let mut reply = String::new();
    BufReader::new(&stream)
        .read_line(&mut reply)
        .map_err(|e| e.to_string())?;
    serde_json::from_str(reply.trim_end()).map_err(|e| e.to_string())
}

#[cfg(not(unix))]
pub fn request(_req: &Request) -> Result<Response, String> {
    Err("no unix socket".into())
}

/// Status from pt35d, or `None` when the daemon is not answering.
pub fn status() -> Option<Status> {
    match request(&Request::Status) {
        Ok(Response::Status(status)) => Some(status),
        _ => None,
    }
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

/// Current value of a quick setting, shown on the right of its row.
pub fn value(adjust: Adjust, status: Option<&Status>) -> String {
    let Some(status) = status else {
        return "--".into();
    };
    match adjust {
        Adjust::Volume => match (status.volume_percent, status.muted) {
            (_, Some(true)) => "muted".into(),
            (Some(percent), _) => format!("{percent}%"),
            _ => "n/a".into(),
        },
        Adjust::Brightness => status
            .brightness_percent
            .map(|percent| format!("{percent}%"))
            .unwrap_or_else(|| "n/a".into()),
        Adjust::Scale => format!("{:.2}x", status.scale),
    }
}

/// A toggle that does not say whether it is on is a guess. Read it out.
pub fn state_value(field: StateField, status: Option<&Status>) -> String {
    let Some(status) = status else {
        return "--".into();
    };
    match field {
        StateField::Mode => status.input_mode.label().into(),
        StateField::Volume => value(Adjust::Volume, Some(status)),
        StateField::Brightness => value(Adjust::Brightness, Some(status)),
        StateField::Scale => value(Adjust::Scale, Some(status)),
        StateField::Network => status.network.clone().unwrap_or_else(|| "offline".into()),
    }
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
    fn quick_settings_read_out_their_value() {
        let s = status();
        assert_eq!(value(Adjust::Volume, Some(&s)), "40%");
        assert_eq!(value(Adjust::Scale, Some(&s)), "1.00x");
        assert_eq!(value(Adjust::Brightness, Some(&s)), "n/a");
        assert_eq!(value(Adjust::Volume, None), "--");
    }

    #[test]
    fn a_toggle_says_which_way_it_is_set() {
        let s = Status {
            input_mode: pt35_common::ipc::InputMode::Mouse,
            ..status()
        };
        assert_eq!(state_value(StateField::Mode, Some(&s)), "MOUSE");
        assert_eq!(state_value(StateField::Mode, None), "--");
    }

    #[test]
    fn no_daemon_means_no_override() {
        assert!(note(Builtin::Wifi, None, 0).is_none());
    }
}
