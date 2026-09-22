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
    feed: StatusFeed,
    clock: String,
    /// Tap targets recorded by the last draw.
    hits: Vec<(i32, i32, Action)>,
}

/// What a tap on the bar does.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Action {
    Menu,
    Switch,
    Close,
}

impl Bar {
    fn new(theme: Theme, font: Font, feed: StatusFeed) -> Self {
        let clock = now(&theme);
        Self {
            theme,
            font,
            feed,
            clock,
            hits: Vec::new(),
        }
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
        let height = (canvas.height as i32 - 6).max(16);
        let width = (self.font.measure(glyph, size) as i32 + 16).max(height);
        canvas.rounded_rect(
            x,
            centre - height / 2,
            width as u32,
            height as u32,
            (height / 2) as u32,
            fill,
        );
        let baseline = centre + (size * 0.36) as i32;
        let text_x = x + (width - self.font.measure(glyph, size) as i32) / 2;
        self.font.draw(canvas, glyph, text_x, baseline, size, ink);
        self.hits.push((x, x + width, action));
        x + width
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
        Some(Duration::from_secs(1))
    }

    fn tick(&mut self) -> bool {
        let clock = now(&self.theme);
        let changed = clock != self.clock;
        self.clock = clock;
        changed || self.feed.get().is_some()
    }

    fn touch(&mut self, x: f64, _y: f64) -> bool {
        let x = x as i32;
        let hit = self
            .hits
            .iter()
            .find(|(left, right, _)| x >= *left && x < *right)
            .map(|(_, _, action)| *action);
        let args: &[&str] = match hit {
            Some(Action::Menu) => &["menu", "toggle"],
            Some(Action::Switch) => &["menu", "open", "windows"],
            Some(Action::Close) => &["window", "close"],
            None => return true,
        };
        if let Err(e) = std::process::Command::new("pt35ctl").args(args).spawn() {
            log::error!("pt35ctl {}: {e}", args.join(" "));
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

        // A hairline under the bar, the one bit of chrome that separates it from
        // a fullscreen app.
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
        ) + 6;

        for (index, segment) in segments::left(status.as_ref(), &self.theme)
            .into_iter()
            .enumerate()
        {
            if index == 0 {
                let width = self.font.measure(&segment.text, size) as i32;
                let height = (canvas.height as i32 - 8).max(14);
                canvas.rounded_rect(
                    x,
                    centre - height / 2,
                    (width + 14) as u32,
                    height as u32,
                    4,
                    self.theme.color.background_alt,
                );
                self.font.draw(
                    canvas,
                    &segment.text,
                    x + 7,
                    baseline,
                    size,
                    self.theme.color.accent,
                );
                self.hits.push((x, x + width + 14, Action::Switch));
                x += width + 14 + 6;
                continue;
            }
            let text = self.font.elide(&segment.text, size, canvas.width / 3);
            x = self
                .font
                .draw(canvas, &text, x, baseline, size, segment.color)
                + pad;
        }

        // Close sits hard against the right edge: the same corner every time,
        // whatever else the bar is showing.
        let close_w = (self.font.measure("x", size) as i32 + 16).max(canvas.height as i32 - 6);
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
            let width = self.font.measure(&segment.text, size) as i32;
            right -= width;
            if right <= x {
                break;
            }
            self.font
                .draw(canvas, &segment.text, right, baseline, size, segment.color);
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
    }
}

pub fn run() -> Result<()> {
    let theme: Theme = pt35_common::load_config("pt35/theme.toml").unwrap_or_default();
    let font = Font::load(&theme.font.family)?;
    let feed = StatusFeed::default();
    feed.spawn();

    let height = theme.bar.height;
    layer::run(Bar::new(theme, font, feed), SurfaceSpec::bar(height))
}
