//! What the bar shows, as data. Keeping the layout decisions out of the drawing
//! code means the "does the battery widget disappear when the hardware has no
//! battery" question is answerable in a unit test.

use pt35_common::ipc::Status;
use pt35_common::theme::{Rgb, Theme, Visibility};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    pub text: String,
    pub color: Rgb,
}

/// Left-hand side: where you are.
pub fn left(status: Option<&Status>, theme: &Theme) -> Vec<Segment> {
    let Some(status) = status else {
        return vec![Segment {
            text: "pt35d?".into(),
            color: theme.color.critical,
        }];
    };
    let mut out = vec![Segment {
        text: format!("{}", status.workspace),
        color: theme.color.accent,
    }];
    if let Some(app) = &status.app {
        out.push(Segment {
            text: app.clone(),
            color: theme.color.foreground,
        });
    }
    out
}

/// Right-hand side: state of the machine. `clock` is passed in so the caller
/// owns time formatting (and tests are deterministic).
pub fn right(status: Option<&Status>, theme: &Theme, clock: &str) -> Vec<Segment> {
    let mut out = Vec::new();
    if let Some(status) = status {
        if status.pointer_armed {
            out.push(Segment {
                text: "ptr".into(),
                color: theme.color.accent,
            });
        }
        if (status.scale - 1.0).abs() > 0.01 {
            out.push(Segment {
                text: format!("{:.2}x", status.scale),
                color: theme.color.muted,
            });
        }
        if let Some(volume) = status.volume_percent {
            let muted = status.muted.unwrap_or(false);
            out.push(Segment {
                text: if muted {
                    "mute".into()
                } else {
                    format!("{volume}%")
                },
                color: if muted {
                    theme.color.muted
                } else {
                    theme.color.foreground
                },
            });
        }
        if theme.bar.show_network {
            if let Some(net) = &status.network {
                out.push(Segment {
                    text: net.clone(),
                    color: theme.color.foreground,
                });
            }
        }
        match (theme.bar.show_battery, status.battery_percent) {
            (Visibility::Never, _) => {}
            // `auto` is the interesting case: on a unit where the RP2040 keeps
            // the gauge to itself there is nothing to show, so show nothing.
            (Visibility::Auto, None) => {}
            (Visibility::Always, None) => out.push(Segment {
                text: "--".into(),
                color: theme.color.muted,
            }),
            (_, Some(percent)) => {
                let charging = status.charging.unwrap_or(false);
                let color = match percent {
                    _ if charging => theme.color.ok,
                    0..=10 => theme.color.critical,
                    11..=25 => theme.color.warning,
                    _ => theme.color.foreground,
                };
                let mark = if charging { "+" } else { "" };
                out.push(Segment {
                    text: format!("{percent}{mark}%"),
                    color,
                });
            }
        }
    }
    out.push(Segment {
        text: clock.to_string(),
        color: theme.color.foreground,
    });
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
    fn shows_the_workspace_and_app() {
        let theme = Theme::default();
        let segments = left(Some(&status()), &theme);
        assert_eq!(segments[0].text, "3");
        assert_eq!(segments[1].text, "helix");
    }

    #[test]
    fn says_so_when_the_daemon_is_gone() {
        let theme = Theme::default();
        let segments = left(None, &theme);
        assert_eq!(segments[0].text, "pt35d?");
        assert_eq!(segments[0].color, theme.color.critical);
    }

    #[test]
    fn hides_battery_when_the_hardware_does_not_expose_one() {
        let theme = Theme::default();
        let segments = right(Some(&status()), &theme, "12:34");
        assert_eq!(
            segments.iter().map(|s| s.text.as_str()).collect::<Vec<_>>(),
            ["12:34"]
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
    fn surfaces_a_non_native_scale_and_the_pointer() {
        let theme = Theme::default();
        let status = Status {
            scale: 0.75,
            pointer_armed: true,
            ..status()
        };
        let texts: Vec<String> = right(Some(&status), &theme, "12:34")
            .into_iter()
            .map(|s| s.text)
            .collect();
        assert!(texts.contains(&"ptr".to_string()));
        assert!(texts.contains(&"0.75x".to_string()));
    }
}
