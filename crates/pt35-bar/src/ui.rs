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
    /// For toasts: the one message a key press gets has to read at a glance.
    bold: Font,
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
    /// When the theme files last changed, so a pick in Settings > Appearance
    /// lands without restarting the session.
    theme_stamp: ThemeStamp,
    ticks: u32,
}

type ThemeStamp = [Option<std::time::SystemTime>; 2];

fn theme_stamp() -> ThemeStamp {
    let modified = |path: Option<std::path::PathBuf>| {
        path.and_then(|p| std::fs::metadata(p).ok())
            .and_then(|m| m.modified().ok())
    };
    [
        modified(Some(pt35_common::paths::appearance_path())),
        modified(pt35_common::paths::user_config("pt35/theme.toml")),
    ]
}

/// What a tap on the bar does.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Action {
    Menu,
    Focus(i64),
    Close,
    Status(segments::Tap),
}

impl Bar {
    fn new(theme: Theme, font: Font, mono: Font, bold: Font, feed: StatusFeed) -> Self {
        let clock = now(&theme);
        let icons = pt35_ui::icon::Icons::new(&theme.icons.theme);
        Self {
            icons,
            theme,
            font,
            mono,
            bold,
            feed,
            drawn: None,
            clock,
            hits: Vec::new(),
            children: Vec::new(),
            toast: None,
            theme_stamp: theme_stamp(),
            ticks: 0,
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

/// The switcher's cursor on its Launcher card. No sway container has id 0.
const LAUNCHER_HOVER: i64 = 0;

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

    fn raised(&self) -> bool {
        self.drawn.as_ref().is_some_and(|s| s.menu_open)
    }

    fn tick_interval(&self) -> Option<Duration> {
        // Fast enough that a mode switch looks instant, and it costs nothing:
        // a tick that finds no change does not draw.
        Some(Duration::from_millis(150))
    }

    fn tick(&mut self) -> bool {
        self.children
            .retain_mut(|child| !matches!(child.try_wait(), Ok(Some(_))));
        // Two stats a second. Cheap next to a frame, and a pick shows within one.
        self.ticks = self.ticks.wrapping_add(1);
        let mut restyled = false;
        if self.ticks % 3 == 0 {
            let stamp = theme_stamp();
            if stamp != self.theme_stamp {
                self.theme_stamp = stamp;
                if let Ok(theme) = pt35_common::load_theme() {
                    self.theme = theme;
                    restyled = true;
                }
            }
        }
        let clock = now(&self.theme);
        let status = self.feed.get();
        let toast = self.feed.toast();
        let changed =
            restyled || clock != self.clock || status != self.drawn || toast != self.toast;
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
            // The launcher is a window that is always there: the skull brings it
            // to the front and never closes it. You leave it the way you leave
            // any window, for another one.
            Some(Action::Menu) => {
                let front = self
                    .drawn
                    .as_ref()
                    .is_some_and(|s| s.menu_open && s.launcher_active);
                if !front {
                    self.run(&["menu", "open"]);
                }
            }
            Some(Action::Close) => self.run(&["window", "close"]),
            // Through the daemon, not straight to swaymsg: it has to learn about
            // the change or the dock keeps the old slot filled. It also takes
            // the menu down, so the slot you tap is the window you get.
            Some(Action::Focus(id)) => self.run(&["window", "focus", &id.to_string()]),
            Some(Action::Status(tap)) => self.run(tap.command()),
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

        // The launcher is the window underneath every other one, so its slot
        // behaves like theirs: filled while it is in front, outlined when not.
        let launcher_up = status
            .as_ref()
            .is_some_and(|s| s.menu_open && s.launcher_active);
        let (fill, ink) = if launcher_up {
            (self.theme.color.accent, self.theme.color.accent_fg)
        } else {
            (self.theme.color.border, self.theme.color.muted)
        };
        let start = pad / 2;
        let mut x = self.button(canvas, start, "", fill, ink, Action::Menu);
        if !launcher_up {
            // Outlined like an unfocused slot: border, then the bar behind.
            let height = (canvas.height as i32 - 8).max(18);
            canvas.rounded_rect(
                start + 1,
                centre - height / 2 + 1,
                (x - start - 2) as u32,
                (height - 2) as u32,
                self.theme.menu.radius,
                self.theme.color.background,
            );
        }
        if status.as_ref().and_then(|s| s.switcher_hover) == Some(LAUNCHER_HOVER) && !launcher_up {
            let height = (canvas.height as i32 - 8).max(18);
            let (top, w) = (centre - height / 2, x - start);
            let accent = self.theme.color.accent;
            canvas.rect(start, top, w as u32, 2, accent);
            canvas.rect(start, top + height - 2, w as u32, 2, accent);
            canvas.rect(start, top, 2, height as u32, accent);
            canvas.rect(start + w - 2, top, 2, height as u32, accent);
        }
        // Drawn, not typed: an `=` in a mono face reads as maths, not a menu.
        let middle = x - (canvas.height as i32 - 8) / 2;
        if self.theme.bar.menu_icon == "bars" {
            for row in [-5, 0, 5] {
                canvas.rect(middle - 7, centre + row - 1, 14, 2, ink);
            }
        } else {
            pt35_ui::pixel::draw_centred(canvas, pt35_ui::pixel::SKULL, middle, centre, 1, ink);
        }
        x += 8;

        // Right side first, so the dock knows how much room it has left. No
        // close button with nothing to close: a red x over the desktop reads
        // as an error.
        let focused = status
            .as_ref()
            .is_some_and(|s| !s.menu_open && s.windows.iter().any(|w| w.focused));
        let close_w = (self.mono.measure("x", size) as i32 + 16).max(canvas.height as i32 - 8);
        let close_x = canvas.width as i32 - close_w - pad / 2;
        let mut right = canvas.width as i32 - pad / 2;
        if focused {
            self.button(
                canvas,
                close_x,
                "x",
                self.theme.color.critical,
                self.theme.color.background,
                Action::Close,
            );
            right = close_x - pad;
        }
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
            // The gap on both sides is part of the target: an icon alone is
            // too narrow to hit with a thumb.
            if let Some(tap) = segment.tap {
                self.hits
                    .push((right - width - 4, right + 4, Action::Status(tap)));
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
            // A filled pill over the dock, in bold and a size up: the only
            // answer a button press gets must not be missed.
            let height = (canvas.height as i32 - 8).max(18);
            let width = (right - x - 8).max(0);
            canvas.rounded_rect(
                x,
                centre - height / 2,
                width as u32,
                height as u32,
                self.theme.menu.radius,
                colour,
            );
            let big = size * 1.1;
            let room = (width - 16).max(0) as u32;
            let text = self.bold.elide(&toast.text, big, room);
            let baseline = centre + (big * 0.36) as i32;
            self.bold
                .draw(canvas, &text, x + 8, baseline, big, colour.ink());
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
        // Every window gets a picture: its profile's icon, one named after the
        // app, a dialog's icon, or the generic one.
        let icons: Vec<String> = status
            .windows
            .iter()
            .map(|w| {
                let app = w.app.to_lowercase();
                let dialog = if w.floating { "dialog-information" } else { "" };
                self.icons
                    .resolve(&[&w.icon, &app, dialog], icon_size)
                    .unwrap_or_default()
            })
            .collect();
        let labels = self.theme.bar.dock_labels;
        let mut natural = Vec::with_capacity(names.len());
        let mut fixed = Vec::with_capacity(names.len());
        for (icon, name) in icons.iter().zip(&names) {
            let base = if !labels {
                (icon_size as i32 + 2 * SLOT_PAD).max(box_h)
            } else if icon.is_empty() {
                (2 * SLOT_PAD).max(box_h)
            } else {
                (icon_size as i32 + ICON_GAP + 2 * SLOT_PAD).max(box_h)
            };
            fixed.push(base);
            natural.push(if labels {
                base + self.font.measure(name, size) as i32
            } else {
                base
            });
        }
        let slots = segments::dock(&natural, &fixed, right - x, SLOT_GAP);
        for (((window, name), icon), slot) in
            status.windows.iter().zip(&names).zip(&icons).zip(slots)
        {
            let start = x;
            // With a screen over it (the switcher from Select), the app you
            // came from is still the one in front.
            // With the launcher in front, no app is.
            let launcher_front = status.menu_open && status.launcher_active;
            let front = !launcher_front
                && (window.focused
                    || (status.menu_open
                        && window.workspace == status.workspace
                        && !window.floating));
            x = self.slot(canvas, x, name, icon, front, window.id, slot);
            // The switcher's cursor, shown on the taskbar too: the strip and
            // the dock are the same list in the same order.
            if status.switcher_hover == Some(window.id) && !front {
                let height = (canvas.height as i32 - 8).max(18);
                let (top, w) = (centre - height / 2, slot.width);
                let accent = self.theme.color.accent;
                canvas.rect(start, top, w as u32, 2, accent);
                canvas.rect(start, top + height - 2, w as u32, 2, accent);
                canvas.rect(start, top, 2, height as u32, accent);
                canvas.rect(start + w - 2, top, 2, height as u32, accent);
            }
        }
    }
}

pub fn run() -> Result<()> {
    let theme: Theme = pt35_common::load_theme().unwrap_or_default();
    let font = Font::load(&theme.font.family)?;
    let mono = Font::load(&theme.font.family_mono).or_else(|_| Font::load(&theme.font.family))?;
    let feed = StatusFeed::default();
    feed.spawn();

    let height = theme.bar.height;
    let bold = Font::load(&theme.font.family_bold).or_else(|_| Font::load(&theme.font.family))?;
    layer::run(
        Bar::new(theme, font, mono, bold, feed),
        SurfaceSpec::bar(height),
    )
}
