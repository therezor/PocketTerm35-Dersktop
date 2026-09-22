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

/// One dock slot, sized for the room the dock actually has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Slot {
    /// Total width, padding included.
    pub width: i32,
    /// Room the label gets. `0` means the slot is icon or initials only.
    pub label_width: i32,
}

/// A label narrower than this is not a word, it is noise. Below it the slot
/// falls back to its icon.
const MIN_LABEL: i32 = 28;

/// Lay the dock out across the width it has.
///
/// Every open window gets a slot: a taskbar that drops one has lied about what
/// is running. So labels shrink, then go, but a slot never does. Slots that
/// need less than their share hand the difference back to the ones that need
/// more, which is what keeps "Foot" from getting the same width as "Chromium".
pub fn dock(natural: &[i32], fixed: &[i32], available: i32, gap: i32) -> Vec<Slot> {
    let count = natural.len();
    if count == 0 {
        return Vec::new();
    }
    let budget = (available - gap * (count as i32 - 1)).max(0);
    let mut width: Vec<i32> = natural.to_vec();
    if natural.iter().sum::<i32>() > budget {
        // Fair share, then hand back what the short ones do not use. Repeat
        // while anything is still being handed back.
        let mut share = vec![0; count];
        let mut settled = vec![false; count];
        let mut left = budget;
        let mut open = count as i32;
        loop {
            if open == 0 {
                break;
            }
            let each = left / open;
            let mut changed = false;
            for i in 0..count {
                if !settled[i] && natural[i] <= each {
                    share[i] = natural[i];
                    settled[i] = true;
                    left -= natural[i];
                    open -= 1;
                    changed = true;
                }
            }
            if !changed {
                for (i, settled) in settled.iter().enumerate() {
                    if !settled {
                        share[i] = each;
                    }
                }
                break;
            }
        }
        width = share;
    }
    width
        .into_iter()
        .zip(fixed)
        .map(|(width, fixed)| {
            let label_width = width - fixed;
            if label_width < MIN_LABEL {
                Slot {
                    width: *fixed,
                    label_width: 0,
                }
            } else {
                Slot { width, label_width }
            }
        })
        .collect()
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
        //
        // No signal reading with a link up means a cable: /proc/net/wireless
        // only knows about radios. Four empty bars would say the opposite of
        // the truth.
        if theme.bar.show_network && status.network.is_some() {
            out.push(Segment::icon(
                match status.network_signal {
                    Some(signal) => Icon::Wifi { signal },
                    None => Icon::Wired,
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
    fn a_cable_is_not_a_dead_radio() {
        let theme = Theme::default();
        let wired = Status {
            network: Some("eth0".into()),
            network_signal: None,
            ..status()
        };
        assert!(right(Some(&wired), &theme, "12:34")
            .iter()
            .any(|s| s.icon == Some(Icon::Wired)));
        let wifi = Status {
            network_signal: Some(70),
            ..wired
        };
        assert!(right(Some(&wifi), &theme, "12:34")
            .iter()
            .any(|s| s.icon == Some(Icon::Wifi { signal: 70 })));
    }

    #[test]
    fn a_dock_that_fits_gives_everyone_what_they_asked_for() {
        let slots = dock(&[100, 80], &[40, 40], 400, 6);
        assert_eq!(slots[0].width, 100);
        assert_eq!(slots[1].width, 80);
        assert_eq!(slots[0].label_width, 60);
    }

    #[test]
    fn a_crowded_dock_shares_out_what_the_short_slots_do_not_use() {
        // 300px for three, one of which only wants 60. The other two get 120
        // each, not the 100 an even split would have given them.
        let slots = dock(&[60, 200, 200], &[20, 40, 40], 300, 0);
        assert_eq!(slots[0].width, 60, "a short slot keeps its natural width");
        assert_eq!(slots[1].width, 120);
        assert_eq!(slots[2].width, 120);
        assert_eq!(slots[0].label_width, 40);
        assert_eq!(slots[1].label_width, 80);
    }

    #[test]
    fn a_label_too_small_to_read_is_dropped_not_squeezed() {
        let slots = dock(&[200, 200, 200, 200], &[40, 40, 40, 40], 200, 0);
        assert!(slots.iter().all(|s| s.label_width == 0));
        assert!(
            slots.iter().all(|s| s.width == 40),
            "an icon-only slot shrinks to its icon"
        );
    }

    #[test]
    fn every_window_keeps_a_slot() {
        // Eight windows in a strip that cannot hold eight names. The dock must
        // still say there are eight.
        let natural = [120; 8];
        let fixed = [40; 8];
        assert_eq!(dock(&natural, &fixed, 300, 4).len(), 8);
        assert_eq!(dock(&[], &[], 300, 4).len(), 0);
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
