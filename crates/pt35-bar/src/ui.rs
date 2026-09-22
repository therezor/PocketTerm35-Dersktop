//! The Wayland half of the bar: a layer-shell strip pinned to the top edge.
//! Linux-only, because layer-shell is.

use anyhow::Result;
use pt35_common::theme::Theme;
use pt35_ui::canvas::Canvas;
use pt35_ui::font::Font;
use pt35_ui::layer::{self, App, SurfaceSpec};
use std::time::Duration;

use crate::segments;
use crate::status::StatusFeed;

struct Bar {
    theme: Theme,
    font: Font,
    mono: Font,
    feed: StatusFeed,
    /// What the last frame was drawn from. Ticking is cheap, drawing is not.
    drawn: Option<pt35_common::ipc::Status>,
    clock: String,
    hits: Vec<(i32, i32, Action)>,
}

/// What a tap on the bar does.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Action {
    Menu,
    Focus(i64),
    Close,
}

impl Bar {
    fn new(theme: Theme, font: Font, mono: Font, feed: StatusFeed) -> Self {
        let clock = now(&theme);
        Self {
            theme,
            font,
            mono,
            feed,
            drawn: None,
            clock,
            hits: Vec::new(),
        }
    }

    /// One dock slot: the app's initial in a rounded square, filled when it has
    /// focus and outlined when it does not.
    fn slot(&mut self, canvas: &mut Canvas, x: i32, app: &str, focused: bool, id: i64) -> i32 {
        let size = self.theme.font.size_bar;
        let centre = canvas.height as i32 / 2;
        let box_h = (canvas.height as i32 - 8).max(18);
        let label = initials(app);
        let width = (self.mono.measure(&label, size) as i32 + 14).max(box_h);
        let y = centre - box_h / 2;

        if focused {
            canvas.rounded_rect(
                x,
                y,
                width as u32,
                box_h as u32,
                RADIUS,
                self.theme.color.accent,
            );
        } else {
            canvas.rounded_rect(
                x,
                y,
                width as u32,
                box_h as u32,
                RADIUS,
                self.theme.color.border,
            );
            canvas.rounded_rect(
                x + 1,
                y + 1,
                (width - 2) as u32,
                (box_h - 2) as u32,
                RADIUS,
                self.theme.color.background,
            );
        }
        let ink = if focused {
            self.theme.color.accent_fg
        } else {
            self.theme.color.muted
        };
        let text_x = x + (width - self.mono.measure(&label, size) as i32) / 2;
        self.mono.draw(
            canvas,
            &label,
            text_x,
            centre + (size * 0.36) as i32,
            size,
            ink,
        );
        self.hits.push((x, x + width, Action::Focus(id)));
        x + width + 4
    }

    fn button(
        &mut self,
        canvas: &mut Canvas,
        x: i32,
        glyph: &str,
        fill: pt35_common::theme::Rgb,
        ink: pt35_common::theme::Rgb,
        action: Action,
    ) -> i32 {
        let size = self.theme.font.size_bar;
        let centre = canvas.height as i32 / 2;
        let height = (canvas.height as i32 - 8).max(18);
        let width = (self.mono.measure(glyph, size) as i32 + 16).max(height);
        canvas.rounded_rect(
            x,
            centre - height / 2,
            width as u32,
            height as u32,
            RADIUS,
            fill,
        );
        let baseline = centre + (size * 0.36) as i32;
        let text_x = x + (width - self.mono.measure(glyph, size) as i32) / 2;
        self.mono.draw(canvas, glyph, text_x, baseline, size, ink);
        self.hits.push((x, x + width, action));
        x + width
    }
}

/// Square corners with the sharpness taken off. A deck panel is machined, not
/// moulded.
const RADIUS: u32 = 2;

/// Two characters of an app id: "pcmanfm" -> "PC", "foot" -> "FO".
fn initials(app: &str) -> String {
    let cleaned: String = app
        .trim_start_matches("org.")
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect();
    let text: String = cleaned.chars().take(2).collect();
    if text.is_empty() {
        "??".into()
    } else {
        text.to_uppercase()
    }
}

fn now(theme: &Theme) -> String {
    chrono::Local::now()
        .format(&theme.bar.clock_format)
        .to_string()
}

impl App for Bar {
    fn background(&self) -> pt35_common::theme::Rgb {
        self.theme.color.background
    }

    fn tick_interval(&self) -> Option<Duration> {
        // Fast enough that a mode switch looks instant, and it costs nothing:
        // a tick that finds no change does not draw.
        Some(Duration::from_millis(150))
    }

    fn tick(&mut self) -> bool {
        let clock = now(&self.theme);
        let status = self.feed.get();
        let changed = clock != self.clock || status != self.drawn;
        self.clock = clock;
        self.drawn = status;
        changed
    }

    fn touch(&mut self, x: f64, _y: f64) -> bool {
        let x = x as i32;
        let hit = self
            .hits
            .iter()
            .find(|(left, right, _)| x >= *left && x < *right)
            .map(|(_, _, action)| *action);
        match hit {
            Some(Action::Menu) => spawn("pt35ctl", &["menu".into(), "toggle".into()]),
            Some(Action::Close) => spawn("pt35ctl", &["window".into(), "close".into()]),
            Some(Action::Focus(id)) => spawn("swaymsg", &[format!("[con_id={id}] focus")]),
            None => {}
        }
        true
    }

    fn draw(&mut self, canvas: &mut Canvas) {
        let status = self.feed.get();
        let size = self.theme.font.size_bar;
        let pad = self.theme.bar.padding_x as i32;
        let baseline = (canvas.height as f32 / 2.0 + size * 0.36).round() as i32;
        let centre = canvas.height as i32 / 2;
        self.hits.clear();

        canvas.rect(
            0,
            canvas.height as i32 - 1,
            canvas.width,
            1,
            self.theme.color.border,
        );

        let mut x = pad / 2;
        x = self.button(
            canvas,
            x,
            "=",
            self.theme.color.accent,
            self.theme.color.accent_fg,
            Action::Menu,
        ) + 8;

        // Right side first, so the dock knows how much room it has left.
        let close_w = (self.mono.measure("x", size) as i32 + 16).max(canvas.height as i32 - 8);
        let close_x = canvas.width as i32 - close_w - pad / 2;
        self.button(
            canvas,
            close_x,
            "x",
            self.theme.color.critical,
            self.theme.color.background,
            Action::Close,
        );

        let mut right = close_x - pad;
        let mut first = true;
        for segment in segments::right(status.as_ref(), &self.theme, &self.clock)
            .into_iter()
            .rev()
        {
            let icon_width = segment.icon.map(|i| i.width() + 4).unwrap_or(0);
            let width = self.mono.measure(&segment.text, size) as i32 + icon_width;
            if right - width <= x {
                break;
            }
            right -= width;
            if let Some(icon) = segment.icon {
                icon.draw(
                    canvas,
                    right,
                    centre,
                    segment.color,
                    self.theme.color.border,
                );
            }
            self.mono.draw(
                canvas,
                &segment.text,
                right + icon_width,
                baseline,
                size,
                segment.color,
            );
            if !first {
                canvas.rect(
                    right + width + pad - 1,
                    centre - 1,
                    2,
                    2,
                    self.theme.color.border,
                );
            }
            first = false;
            right -= pad * 2;
        }

        // The dock: one slot per open window, focused one filled.
        match status.as_ref() {
            Some(status) if !status.windows.is_empty() => {
                for window in &status.windows {
                    let app = if window.app.is_empty() {
                        window.title.clone()
                    } else {
                        window.app.clone()
                    };
                    if x + 40 > right {
                        break;
                    }
                    x = self.slot(canvas, x, &app, window.focused, window.id);
                }
            }
            Some(_) => {
                self.font.draw(
                    canvas,
                    "no windows",
                    x,
                    baseline,
                    size,
                    self.theme.color.muted,
                );
            }
            None => {
                self.font.draw(
                    canvas,
                    "pt35d?",
                    x,
                    baseline,
                    size,
                    self.theme.color.critical,
                );
            }
        }
    }
}

fn spawn(binary: &str, args: &[String]) {
    if let Err(e) = std::process::Command::new(binary).args(args).spawn() {
        log::error!("{binary}: {e}");
    }
}

pub fn run() -> Result<()> {
    let theme: Theme = pt35_common::load_config("pt35/theme.toml").unwrap_or_default();
    let font = Font::load(&theme.font.family)?;
    let mono = Font::load(&theme.font.family_mono).or_else(|_| Font::load(&theme.font.family))?;
    let feed = StatusFeed::default();
    feed.spawn();

    let height = theme.bar.height;
    layer::run(Bar::new(theme, font, mono, feed), SurfaceSpec::bar(height))
}
