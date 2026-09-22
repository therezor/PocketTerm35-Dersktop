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
}

impl Bar {
    fn new(theme: Theme, font: Font, feed: StatusFeed) -> Self {
        let clock = now(&theme);
        Self {
            theme,
            font,
            feed,
            clock,
        }
    }
}

fn now(theme: &Theme) -> String {
    chrono::Local::now()
        .format(&theme.bar.clock_format)
        .to_string()
}

impl App for Bar {
    // Deliberately the *alt* background: the strip should read as a separate
    // surface from the app filling the rest of the panel.
    #[allow(clippy::misnamed_getters)]
    fn background(&self) -> pt35_common::theme::Rgb {
        self.theme.color.background_alt
    }

    fn tick_interval(&self) -> Option<Duration> {
        // Once a second: fine for a clock that shows minutes, and cheap enough
        // that the bar stays invisible in `top`.
        Some(Duration::from_secs(1))
    }

    fn tick(&mut self) -> bool {
        let clock = now(&self.theme);
        let changed = clock != self.clock;
        self.clock = clock;
        // Status may have changed under us at any time; repaint on the same beat.
        changed || self.feed.get().is_some()
    }

    fn draw(&mut self, canvas: &mut Canvas) {
        let status = self.feed.get();
        let size = self.theme.font.size_bar;
        let pad = self.theme.bar.padding_x as i32;
        let baseline = (canvas.height as f32 / 2.0 + size * 0.36).round() as i32;
        let centre = canvas.height as i32 / 2;
        let pill_h = (canvas.height as i32 - 8).max(14);

        let mut x = pad;
        for (index, segment) in segments::left(status.as_ref(), &self.theme)
            .into_iter()
            .enumerate()
        {
            // The workspace number gets a pill. It is the one thing on the bar
            // that changes as you move, so it has to be findable without reading.
            if index == 0 {
                let width = self.font.measure(&segment.text, size) as i32;
                let pill_w = (width + 16).max(pill_h);
                canvas.rounded_rect(
                    x,
                    centre - pill_h / 2,
                    pill_w as u32,
                    pill_h as u32,
                    (pill_h / 2) as u32,
                    segment.color,
                );
                self.font.draw(
                    canvas,
                    &segment.text,
                    x + (pill_w - width) / 2,
                    baseline,
                    size,
                    self.theme.color.accent_fg,
                );
                x += pill_w + pad;
                continue;
            }
            let text = self.font.elide(&segment.text, size, canvas.width / 2);
            x = self
                .font
                .draw(canvas, &text, x, baseline, size, segment.color)
                + pad;
        }

        // Laid out backwards from the edge so the clock never moves when a
        // widget appears or disappears.
        let mut right = canvas.width as i32 - pad;
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

/// Load the configuration, connect to pt35d and put the bar on screen.
pub fn run() -> Result<()> {
    let theme: Theme = pt35_common::load_config("pt35/theme.toml").unwrap_or_default();
    let font = Font::load(&theme.font.family)?;
    let feed = StatusFeed::default();
    feed.spawn();

    let height = theme.bar.height;
    layer::run(Bar::new(theme, font, feed), SurfaceSpec::bar(height))
}
