//! What the bar shows, as data. Keeping the layout decisions out of the drawing
//! code means the "does the battery widget disappear when the hardware has no
//! battery" question is answerable in a unit test.

use crate::icons::Icon;
use pt35_common::ipc::Status;
use pt35_common::theme::{Rgb, Theme, Visibility};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    pub text: String,
    pub color: Rgb,
    /// Drawn to the left of the text. A segment may be icon only.
    pub icon: Option<Icon>,
}

impl Segment {
    fn text(text: impl Into<String>, color: Rgb) -> Self {
        Self {
            text: text.into(),
            color,
            icon: None,
        }
    }

    fn icon(icon: Icon, color: Rgb) -> Self {
        Self {
            text: String::new(),
            color,
            icon: Some(icon),
        }
    }
}

/// Right-hand side: state of the machine. `clock` is passed in so the caller
/// owns time formatting (and tests are deterministic).
pub fn right(status: Option<&Status>, theme: &Theme, clock: &str) -> Vec<Segment> {
    let mut out = Vec::new();
    if let Some(status) = status {
        // The D-pad either navigates or moves a cursor, and the same buttons
        // either confirm or click. Nothing else on screen says which.
        let mouse = status.input_mode == pt35_common::ipc::InputMode::Mouse;
        out.push(Segment {
            text: status.input_mode.label().into(),
            color: if mouse {
                theme.color.warning
            } else {
                theme.color.accent
            },
            icon: Some(Icon::Mode { mouse }),
        });
        if (status.scale - 1.0).abs() > 0.01 {
            out.push(Segment::text(
                format!("{:.2}x", status.scale),
                theme.color.muted,
            ));
        }
        if let Some(volume) = status.volume_percent {
            out.push(Segment::icon(
                Icon::Volume {
                    level: volume.min(100),
                    muted: status.muted.unwrap_or(false),
                },
                theme.color.foreground,
            ));
        }
        // An interface name is not news. Whether there is signal is.
        if theme.bar.show_network && status.network.is_some() {
            out.push(Segment::icon(
                Icon::Wifi {
                    signal: status.network_signal,
                },
                theme.color.foreground,
            ));
        }
        match (theme.bar.show_battery, status.battery_percent) {
            (Visibility::Never, _) => {}
            // `auto` is the interesting case: on a unit where the RP2040 keeps
            // the gauge to itself there is nothing to show, so show nothing.
            (Visibility::Auto, None) => {}
            (Visibility::Always, None) => out.push(Segment::text("--", theme.color.muted)),
            (_, Some(percent)) => {
                let charging = status.charging.unwrap_or(false);
                let color = match percent {
                    _ if charging => theme.color.ok,
                    0..=10 => theme.color.critical,
                    11..=25 => theme.color.warning,
                    _ => theme.color.foreground,
                };
                let mark = if charging { "+" } else { "" };
                out.push(Segment::text(format!("{percent}{mark}%"), color));
            }
        }
    }
    out.push(Segment::text(clock, theme.color.foreground));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status() -> Status {
        Status {
            workspace: 3,
            app: Some("helix".into()),
            scale: 1.0,
            ..Status::default()
        }
    }

    #[test]
    fn hides_battery_when_the_hardware_does_not_expose_one() {
        let theme = Theme::default();
        let segments = right(Some(&status()), &theme, "12:34");
        assert_eq!(
            segments.iter().map(|s| s.text.as_str()).collect::<Vec<_>>(),
            ["BTN", "12:34"]
        );
    }

    #[test]
    fn colours_a_low_battery() {
        let theme = Theme::default();
        let mut status = status();
        status.battery_percent = Some(8);
        status.charging = Some(false);
        let segments = right(Some(&status), &theme, "12:34");
        let battery = segments.iter().find(|s| s.text.contains('%')).unwrap();
        assert_eq!(battery.text, "8%");
        assert_eq!(battery.color, theme.color.critical);

        status.charging = Some(true);
        let segments = right(Some(&status), &theme, "12:34");
        let battery = segments.iter().find(|s| s.text.contains('%')).unwrap();
        assert_eq!(battery.text, "8+%");
        assert_eq!(battery.color, theme.color.ok);
    }

    #[test]
    fn says_which_mode_the_d_pad_is_in() {
        let theme = Theme::default();
        let mut s = status();
        assert!(right(Some(&s), &theme, "12:34")
            .iter()
            .any(|seg| seg.text == "BTN"));
        s.input_mode = pt35_common::ipc::InputMode::Mouse;
        assert!(right(Some(&s), &theme, "12:34")
            .iter()
            .any(|seg| seg.text == "MOUSE"));
    }

    #[test]
    fn surfaces_a_non_native_scale() {
        let theme = Theme::default();
        let status = Status {
            scale: 0.75,
            ..status()
        };
        let texts: Vec<String> = right(Some(&status), &theme, "12:34")
            .into_iter()
            .map(|s| s.text)
            .collect();
        assert!(texts.contains(&"0.75x".to_string()));
    }
}
