//! The bar's two status icons.
//!
//! "98%" and "wlan0" say nothing you can read at a glance on a 3.5" panel. A
//! speaker and a signal meter do. Papirus draws both, and when it is not
//! installed these rectangles stand in.

use pt35_common::theme::Rgb;
use pt35_ui::canvas::Canvas;

/// Height of the drawn box. The bar is 34px and the text sits on a 15px font,
/// so 13 keeps the icons the same visual weight as the digits.
const BOX: i32 = 13;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    /// Speaker plus four level bars, or a crossed-out speaker when muted.
    Volume { level: u8, muted: bool },
    /// Four ascending bars. `None` is offline: every bar drawn empty.
    Wifi { signal: Option<u8> },
}

impl Icon {
    /// The freedesktop name, when the theme has one to draw instead.
    pub fn theme_name(&self) -> &'static str {
        match *self {
            Icon::Volume { level, muted } => pt35_ui::icon::volume_icon(level, muted),
            Icon::Wifi { signal } => pt35_ui::icon::wifi_icon(signal),
        }
    }

    /// Width of the drawn fallback.
    pub fn width(&self) -> i32 {
        match self {
            Icon::Volume { .. } => 7 + 2 + bars_width(4, 2, 1),
            Icon::Wifi { .. } => bars_width(4, 3, 1),
        }
    }

    /// `y` is the vertical centre of the bar's text line.
    pub fn draw(&self, canvas: &mut Canvas, x: i32, y: i32, on: Rgb, off: Rgb) {
        match *self {
            Icon::Volume { level, muted } => {
                let colour = if muted { off } else { on };
                speaker(canvas, x, y, colour);
                if muted {
                    cross(canvas, x + 9, y, off);
                } else {
                    Meter {
                        count: 4,
                        width: 2,
                        gap: 1,
                        lit: filled(level, 4),
                    }
                    .draw(canvas, x + 9, y, on, off);
                }
            }
            Icon::Wifi { signal } => {
                Meter {
                    count: 4,
                    width: 3,
                    gap: 1,
                    lit: signal.map(|s| filled(s, 4)).unwrap_or(0),
                }
                .draw(canvas, x, y, on, off);
            }
        }
    }
}

fn bars_width(count: i32, width: i32, gap: i32) -> i32 {
    count * width + (count - 1) * gap
}

/// How many of `count` bars a 0-100 level lights. Anything above zero lights at
/// least one: a bar chart that reads empty at 10% volume is a lie.
fn filled(level: u8, count: i32) -> i32 {
    if level == 0 {
        return 0;
    }
    ((level as i32 * count).div_euclid(100) + 1).min(count)
}

/// Ascending bars, tallest on the right.
struct Meter {
    count: i32,
    width: i32,
    gap: i32,
    lit: i32,
}

impl Meter {
    fn draw(&self, canvas: &mut Canvas, x: i32, y: i32, on: Rgb, off: Rgb) {
        let bottom = y + BOX / 2;
        for i in 0..self.count {
            let height = BOX * (i + 1) / self.count;
            canvas.rect(
                x + i * (self.width + self.gap),
                bottom - height,
                self.width as u32,
                height as u32,
                if i < self.lit { on } else { off },
            );
        }
    }
}

/// A 7x13 speaker: a square box with a cone widening to the right.
fn speaker(canvas: &mut Canvas, x: i32, y: i32, colour: Rgb) {
    canvas.rect(x, y - 2, 3, 5, colour);
    for i in 0..4 {
        let height = 3 + i * 2;
        canvas.rect(x + 3 + i, y - height / 2 - 1, 1, height as u32 + 1, colour);
    }
}

/// The mute mark: a diagonal cross drawn as squares, because the canvas has no
/// line primitive and a two-pixel diagonal does not need one.
fn cross(canvas: &mut Canvas, x: i32, y: i32, colour: Rgb) {
    for i in 0..4 {
        canvas.rect(x + i * 2, y - 3 + i * 2, 2, 2, colour);
        canvas.rect(x + 6 - i * 2, y - 3 + i * 2, 2, 2, colour);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_level_above_zero_lights_a_bar() {
        assert_eq!(filled(0, 4), 0);
        assert_eq!(filled(1, 4), 1);
        assert_eq!(filled(50, 4), 3);
        assert_eq!(filled(100, 4), 4);
    }

    #[test]
    fn both_icons_fit_the_bar() {
        for icon in [
            Icon::Volume {
                level: 50,
                muted: false,
            },
            Icon::Wifi { signal: Some(80) },
        ] {
            assert!((14..=24).contains(&icon.width()), "{icon:?}");
        }
    }
}
