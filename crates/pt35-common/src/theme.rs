//! Colours, fonts and sizes, tuned for a 640x480 panel.

use serde::Deserialize;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Theme {
    pub color: Colors,
    pub font: Fonts,
    pub bar: Bar,
    pub menu: Menu,
    pub pointer: Pointer,
    pub buttons: Buttons,
    pub icons: IconTheme,
    pub apps: Apps,
}

/// How GTK and Qt apps are drawn. The shell is dark; an app in the Pi's light
/// theme is a white slab on a 3.5" panel.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Apps {
    pub color_scheme: ColorScheme,
    /// GTK theme for each scheme. Qt follows GTK through its platform theme.
    pub gtk_dark: String,
    pub gtk_light: String,
}

impl Default for Apps {
    fn default() -> Self {
        Self {
            color_scheme: ColorScheme::Dark,
            gtk_dark: "Adwaita-dark".into(),
            gtk_light: "Adwaita".into(),
        }
    }
}

/// `keep` leaves the desktop settings alone, for a user who sets them by hand.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ColorScheme {
    #[default]
    Dark,
    Light,
    Keep,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Colors {
    pub background: Rgb,
    /// Face-button colours, used by the hint bar so the legend matches the
    /// physical buttons: A green, B red, X cyan, Y amber.
    pub button_a: Rgb,
    pub button_b: Rgb,
    pub button_x: Rgb,
    pub button_y: Rgb,
    pub button_neutral: Rgb,
    pub background_alt: Rgb,
    pub foreground: Rgb,
    pub muted: Rgb,
    pub accent: Rgb,
    pub accent_fg: Rgb,
    pub warning: Rgb,
    pub critical: Rgb,
    pub ok: Rgb,
    pub border: Rgb,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Fonts {
    pub family: String,
    /// Used for the things you press: buttons, chips, legends.
    pub family_bold: String,
    pub family_mono_bold: String,
    /// Used for readouts: the bar, headers, notes and the button legend. A
    /// fixed pitch is what makes a panel read as instrumentation.
    pub family_mono: String,
    /// Extra pixels between glyphs in headers and the legend.
    pub tracking: f32,
    pub size_bar: f32,
    pub size_menu: f32,
    pub size_title: f32,
    pub size_hint: f32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Bar {
    pub height: u32,
    pub padding_x: u32,
    pub show_battery: Visibility,
    pub show_network: bool,
    pub show_volume: bool,
    /// The BTN / MOUSE readout. Off saves 20px for a user who knows the mode by
    /// feel.
    pub show_mode: bool,
    pub show_clock: bool,
    pub clock_format: String,
    /// The mark on the menu button: `skull` or `bars`.
    pub menu_icon: String,
    /// App names next to the dock icons. Off by default: at 640px, names crowd
    /// each other out by the third window.
    pub dock_labels: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Menu {
    pub rows_visible: u32,
    pub row_height: u32,
    pub padding_x: u32,
    pub show_numbers: bool,
    /// Shown in the header while a search is open and still empty.
    pub filter_hint: String,
    /// How many recent launches go to the top of the launcher. 0 keeps the
    /// launcher in a fixed order.
    pub recents: u32,
    /// Every .desktop file on the machine in the launcher, or only the apps
    /// with a profile in apps.toml.
    pub show_desktop_entries: bool,
    /// The selection glides and a new screen settles in, each for about a
    /// tenth of a second. Off draws every change in one frame.
    pub animations: bool,
    /// Typing on the launcher starts a search, no X first. Off for a unit on
    /// stock firmware, where the letters a b x y l r are also the buttons.
    pub type_to_search: bool,
    /// A picture of each window on its switcher card, from the last time it
    /// was on screen. Off saves the capture: about 80ms of one core per shot.
    pub window_previews: bool,
    /// Height of the header strip that carries the screen title.
    pub header_height: u32,
    /// Height of the button-legend bar along the bottom. It doubles as a touch
    /// target, so it is deliberately taller than the text needs.
    pub hint_height: u32,
    /// Corner radius of the selection pill and the grid tiles.
    pub radius: u32,
    /// Grid layout: how many tiles across, how tall each one is, and the gap.
    pub columns: u32,
    pub tile_height: u32,
    pub gap: u32,
    /// Width of the list's scrollbar. It is a target too: a click on the
    /// track jumps there and a drag moves the list.
    pub scrollbar_width: u32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Pointer {
    /// Pixels the cursor moves per D-pad step.
    pub step: i32,
    /// Key repeat while the cursor is on the D-pad. Faster than navigating a
    /// list, or the cursor crawls.
    pub repeat_delay: u32,
    pub repeat_rate: u32,
    /// Cursor theme and size, for sway, GTK and XWayland alike.
    pub cursor_theme: String,
    pub cursor_size: u32,
}

/// Buttons mode: the D-pad is the arrow keys, so it repeats at reading speed
/// rather than pointer speed.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Buttons {
    /// One press of the D-pad, one row. Faster and a press skips two.
    pub repeat_delay: u32,
    pub repeat_rate: u32,
    /// Milliseconds of stillness before the cursor is hidden. There is no
    /// cursor to speak of in this mode, so it goes quickly.
    pub hide_cursor: u32,
}

impl Default for Buttons {
    fn default() -> Self {
        Self {
            repeat_delay: 500,
            repeat_rate: 8,
            hide_cursor: 1500,
        }
    }
}

/// Icons come from an installed freedesktop theme; the shell ships none. Every
/// place that draws one falls back to a letter or a drawn meter, so a machine
/// without the theme still works.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct IconTheme {
    pub theme: String,
    /// Status icons in the bar.
    pub size_bar: u32,
    /// The badge on a menu tile and a dock slot.
    pub size_tile: u32,
}

impl Default for IconTheme {
    fn default() -> Self {
        Self {
            theme: "Papirus-Dark".into(),
            size_bar: 18,
            size_tile: 32,
        }
    }
}

/// `auto` hides a widget when the hardware it reports on is absent — which is
/// the expected case for battery if the RP2040 keeps it to itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Visibility {
    #[default]
    Auto,
    Always,
    Never,
}

/// A `#rrggbb` colour, kept as premultiplied-ready components.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    pub const fn to_argb8888(self) -> u32 {
        0xff00_0000 | ((self.0 as u32) << 16) | ((self.1 as u32) << 8) | self.2 as u32
    }

    /// WCAG relative luminance, 0 for black and 1 for white.
    pub fn luminance(self) -> f32 {
        let channel = |c: u8| {
            let c = c as f32 / 255.0;
            if c <= 0.039_28 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(self.0) + 0.7152 * channel(self.1) + 0.0722 * channel(self.2)
    }

    /// Near-black or near-white, whichever reads better on this colour. For
    /// text on an accent the user picked, which may be light or dark.
    pub fn ink(self) -> Rgb {
        let dark = Rgb(0x0a, 0x0e, 0x0c);
        let light = Rgb(0xf7, 0xf7, 0xf2);
        let contrast = |a: Rgb, b: Rgb| {
            let (hi, lo) = if a.luminance() > b.luminance() {
                (a, b)
            } else {
                (b, a)
            };
            (hi.luminance() + 0.05) / (lo.luminance() + 0.05)
        };
        if contrast(self, dark) >= contrast(self, light) {
            dark
        } else {
            light
        }
    }
}

impl std::fmt::Display for Rgb {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "#{:02x}{:02x}{:02x}", self.0, self.1, self.2)
    }
}

impl std::str::FromStr for Rgb {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let hex = s.strip_prefix('#').unwrap_or(s);
        if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(format!("expected #rrggbb, got {s:?}"));
        }
        let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).unwrap();
        Ok(Rgb(byte(0), byte(2), byte(4)))
    }
}

impl<'de> Deserialize<'de> for Rgb {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

impl Default for Colors {
    fn default() -> Self {
        Self {
            background: Rgb(0x0a, 0x13, 0x10),
            button_a: Rgb(0x3d, 0xdc, 0x97),
            button_b: Rgb(0xef, 0x6b, 0x73),
            button_x: Rgb(0x5f, 0xd0, 0xe8),
            button_y: Rgb(0xe8, 0xc4, 0x68),
            button_neutral: Rgb(0x25, 0x42, 0x36),
            background_alt: Rgb(0x12, 0x1f, 0x1a),
            foreground: Rgb(0xdc, 0xf3, 0xe5),
            muted: Rgb(0x7a, 0xa4, 0x8e),
            accent: Rgb(0x3d, 0xdc, 0x97),
            accent_fg: Rgb(0x06, 0x13, 0x0d),
            warning: Rgb(0xe8, 0xc4, 0x68),
            critical: Rgb(0xef, 0x6b, 0x73),
            ok: Rgb(0x3d, 0xdc, 0x97),
            border: Rgb(0x25, 0x42, 0x36),
        }
    }
}

impl Default for Fonts {
    fn default() -> Self {
        Self {
            family: "DejaVu Sans".into(),
            family_bold: "DejaVu Sans Bold".into(),
            family_mono: "DejaVu Sans Mono".into(),
            family_mono_bold: "DejaVu Sans Mono Bold".into(),
            tracking: 1.0,
            size_bar: 15.0,
            size_menu: 23.0,
            size_title: 24.0,
            size_hint: 16.0,
        }
    }
}

impl Default for Bar {
    fn default() -> Self {
        Self {
            height: 34,
            padding_x: 10,
            show_battery: Visibility::Auto,
            show_network: true,
            show_volume: true,
            show_mode: true,
            show_clock: true,
            clock_format: "%H:%M".into(),
            menu_icon: "skull".into(),
            dock_labels: false,
        }
    }
}

impl Default for Menu {
    fn default() -> Self {
        Self {
            rows_visible: 7,
            row_height: 50,
            padding_x: 14,
            show_numbers: true,
            filter_hint: "type to search".into(),
            recents: 5,
            show_desktop_entries: true,
            animations: true,
            type_to_search: true,
            window_previews: true,
            header_height: 46,
            hint_height: 46,
            radius: 2,
            columns: 3,
            tile_height: 86,
            gap: 10,
            scrollbar_width: 8,
        }
    }
}

impl Default for Pointer {
    fn default() -> Self {
        Self {
            step: 16,
            repeat_delay: 200,
            repeat_rate: 40,
            cursor_theme: "Bibata-Modern-Classic".into(),
            cursor_size: 20,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hex_colours() {
        assert_eq!("#61afef".parse::<Rgb>().unwrap(), Rgb(0x61, 0xaf, 0xef));
        assert_eq!("61afef".parse::<Rgb>().unwrap(), Rgb(0x61, 0xaf, 0xef));
        assert!("#xyzxyz".parse::<Rgb>().is_err());
        assert!("#abc".parse::<Rgb>().is_err());
    }

    #[test]
    fn ink_is_dark_on_a_light_accent_and_light_on_a_dark_one() {
        assert_eq!(Rgb(0x3d, 0xdc, 0x97).ink(), Rgb(0x0a, 0x0e, 0x0c));
        assert_eq!(Rgb(0x1f, 0x3a, 0x80).ink(), Rgb(0xf7, 0xf7, 0xf2));
        assert_eq!(Rgb(0x0a, 0x13, 0x10).to_string(), "#0a1310");
    }

    #[test]
    fn packs_argb() {
        assert_eq!(Rgb(0x10, 0x14, 0x18).to_argb8888(), 0xff10_1418);
    }

    #[test]
    fn partial_user_theme_keeps_defaults() {
        let theme: Theme = toml::from_str("[color]\naccent = \"#ff0000\"\n").unwrap();
        assert_eq!(theme.color.accent, Rgb(0xff, 0, 0));
        assert_eq!(theme.color.background, Colors::default().background);
        assert_eq!(theme.bar.height, Bar::default().height);
    }
}
