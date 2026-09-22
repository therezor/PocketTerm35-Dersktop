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
    icons: pt35_ui::icon::Icons,
    /// What the last frame was drawn from. Ticking is cheap, drawing is not.
    drawn: Option<pt35_common::ipc::Status>,
    clock: String,
    hits: Vec<(i32, i32, Action)>,
    /// Helpers a tap started. std does not reap on drop, so they are waited
    /// for here or every tap leaves a zombie for the life of the bar.
    children: Vec<std::process::Child>,
    /// What the last frame drew over the dock, if anything.
    toast: Option<crate::status::Toast>,
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
        let icons = pt35_ui::icon::Icons::new(&theme.icons.theme);
        Self {
            icons,
            theme,
            font,
            mono,
            feed,
            drawn: None,
            clock,
            hits: Vec::new(),
            children: Vec::new(),
            toast: None,
        }
    }

    fn run(&mut self, args: &[&str]) {
        match std::process::Command::new("pt35ctl").args(args).spawn() {
            Ok(child) => self.children.push(child),
            Err(e) => log::error!("pt35ctl {}: {e}", args.join(" ")),
        }
    }

    /// One dock slot: icon, then the app's name while there is room for it.
    ///
    /// Filled when it has focus, outlined when it does not. A name is what makes
    /// this a taskbar rather than a row of coloured squares, so it is drawn
    /// whenever the width allows.
    #[allow(clippy::too_many_arguments)]
    fn slot(
        &mut self,
        canvas: &mut Canvas,
        x: i32,
        app: &str,
        icon: &str,
        focused: bool,
        id: i64,
        slot: segments::Slot,
    ) -> i32 {
        let size = self.theme.font.size_bar;
        let centre = canvas.height as i32 / 2;
        let box_h = (canvas.height as i32 - 8).max(18);
        let icon_size = self.icon_size(canvas);
        let has_icon = !icon.is_empty() && self.icons.get(icon, icon_size).is_some();
        let radius = self.theme.menu.radius;
        let width = slot.width;
        let y = centre - box_h / 2;

        if focused {
            canvas.rounded_rect(
                x,
                y,
                width as u32,
                box_h as u32,
                radius,
                self.theme.color.accent,
            );
        } else {
            canvas.rounded_rect(
                x,
                y,
                width as u32,
                box_h as u32,
                radius,
                self.theme.color.border,
            );
            canvas.rounded_rect(
                x + 1,
                y + 1,
                (width - 2) as u32,
                (box_h - 2) as u32,
                radius,
                self.theme.color.background,
            );
        }
        let ink = if focused {
            self.theme.color.accent_fg
        } else {
            self.theme.color.muted
        };
        let baseline = centre + (size * 0.36) as i32;
        self.hits.push((x, x + width, Action::Focus(id)));

        // No room for a name. An icon stands in; without one, two letters do.
        if slot.label_width == 0 {
            if has_icon {
                let inset = (width - icon_size as i32) / 2;
                if let Some(icon) = self.icons.get(icon, icon_size) {
                    icon.draw(canvas, x + inset, centre - icon_size as i32 / 2);
                }
            } else {
                let label = pt35_common::apps::initials(app);
                let text_x = x + (width - self.mono.measure(&label, size) as i32) / 2;
                self.mono.draw(canvas, &label, text_x, baseline, size, ink);
            }
            return x + width + SLOT_GAP;
        }

        let mut text_x = x + SLOT_PAD;
        if has_icon {
            if let Some(drawn) = self.icons.get(icon, icon_size) {
                drawn.draw(canvas, text_x, centre - icon_size as i32 / 2);
            }
            text_x += icon_size as i32 + ICON_GAP;
        }
        let label = self.font.elide(app, size, slot.label_width.max(0) as u32);
        self.font.draw(canvas, &label, text_x, baseline, size, ink);
        x + width + SLOT_GAP
    }

    fn icon_size(&self, canvas: &Canvas) -> u32 {
        ((canvas.height as i32 - 8).max(18) - 6).max(12) as u32
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
            self.theme.menu.radius,
            fill,
        );
        let baseline = centre + (size * 0.36) as i32;
        let text_x = x + (width - self.mono.measure(glyph, size) as i32) / 2;
        self.mono.draw(canvas, glyph, text_x, baseline, size, ink);
        self.hits.push((x, x + width, action));
        x + width
    }
}

/// Space between two dock slots.
const SLOT_GAP: i32 = 6;
/// Inset from a slot's edge to its contents.
const SLOT_PAD: i32 = 6;
/// Between a slot's icon and its name.
const ICON_GAP: i32 = 5;

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
        self.children
            .retain_mut(|child| !matches!(child.try_wait(), Ok(Some(_))));
        let clock = now(&self.theme);
        let status = self.feed.get();
        let toast = self.feed.toast();
        let changed = clock != self.clock || status != self.drawn || toast != self.toast;
        self.clock = clock;
        self.drawn = status;
        self.toast = toast;
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
            Some(Action::Menu) => self.run(&["menu", "toggle"]),
            Some(Action::Close) => self.run(&["window", "close"]),
            // Through the daemon, not straight to swaymsg: it has to learn about
            // the change or the dock keeps the old slot filled.
            Some(Action::Focus(id)) => self.run(&["window", "focus", &id.to_string()]),
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
        for segment in segments::right(status.as_ref(), &self.theme, &self.clock)
            .into_iter()
            .rev()
        {
            let themed = segment.icon.map(|i| i.theme_name()).filter(|name| {
                self.icons
                    .get_symbolic(name, self.theme.icons.size_bar)
                    .is_some()
            });
            let drawn = segment.icon.and_then(|icon| icon.width());
            let icon_width = match (&themed, drawn) {
                (Some(_), _) => self.theme.icons.size_bar as i32 + 4,
                (None, Some(w)) => w + 4,
                (None, None) => 0,
            };
            // An icon replaces the words; the words are what is left when
            // neither the theme nor this crate can draw the thing.
            let text: &str = if themed.is_some() || drawn.is_some() {
                ""
            } else {
                &segment.text
            };
            let width = self.mono.measure(text, size) as i32 + icon_width;
            if right - width <= x {
                break;
            }
            right -= width;
            match (themed, segment.icon) {
                (Some(name), _) => {
                    let size_icon = self.theme.icons.size_bar;
                    let top = centre - size_icon as i32 / 2;
                    if let Some(icon) = self.icons.get_symbolic(name, size_icon) {
                        icon.draw_tinted(canvas, right, top, segment.color);
                    }
                }
                (None, Some(icon)) => icon.draw(
                    canvas,
                    right,
                    centre,
                    segment.color,
                    self.theme.color.border,
                ),
                (None, None) => {}
            }
            self.mono.draw(
                canvas,
                text,
                right + icon_width,
                baseline,
                size,
                segment.color,
            );
            // No separator: an icon is its own boundary, and a dot plus two
            // pads costs more of a 640px bar than it earns.
            right -= 8;
        }

        // A toast takes the dock's room for a couple of seconds. It is the only
        // answer a key binding ever gets: `pt35ctl volume +5` from a binding
        // writes its error to a stderr nobody reads.
        if let Some(toast) = self.feed.toast() {
            let colour = if toast.urgency >= 2 {
                self.theme.color.critical
            } else {
                self.theme.color.accent
            };
            let room = (right - x - 8).max(0) as u32;
            let text = self.font.elide(&toast.text, size, room);
            self.font.draw(canvas, &text, x, baseline, size, colour);
            return;
        }

        // The dock: one slot per open window, focused one filled.
        //
        // No "nothing open" text. With no windows the menu is the desktop, and
        // it covers this strip anyway.
        let Some(status) = status.as_ref() else {
            self.font.draw(
                canvas,
                "pt35d?",
                x,
                baseline,
                size,
                self.theme.color.critical,
            );
            return;
        };

        let icon_size = self.icon_size(canvas);
        let box_h = (canvas.height as i32 - 8).max(18);
        let names: Vec<String> = status
            .windows
            .iter()
            .map(|w| {
                if w.app.is_empty() {
                    w.title.clone()
                } else {
                    pt35_common::apps::pretty_app(&w.app)
                }
            })
            .collect();
        let mut natural = Vec::with_capacity(names.len());
        let mut fixed = Vec::with_capacity(names.len());
        for (window, name) in status.windows.iter().zip(&names) {
            let has_icon =
                !window.icon.is_empty() && self.icons.get(&window.icon, icon_size).is_some();
            let icon_cost = if has_icon {
                icon_size as i32 + ICON_GAP
            } else {
                0
            };
            let base = (icon_cost + 2 * SLOT_PAD).max(box_h);
            fixed.push(base);
            natural.push(base + self.font.measure(name, size) as i32);
        }
        let slots = segments::dock(&natural, &fixed, right - x, SLOT_GAP);
        for ((window, name), slot) in status.windows.iter().zip(&names).zip(slots) {
            x = self.slot(
                canvas,
                x,
                name,
                &window.icon,
                window.focused,
                window.id,
                slot,
            );
        }
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
