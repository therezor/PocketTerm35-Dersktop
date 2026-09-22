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
    pub clock_format: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Menu {
    pub rows_visible: u32,
    pub row_height: u32,
    pub padding_x: u32,
    pub show_numbers: bool,
    pub filter_hint: String,
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
            family_mono: "DejaVu Sans Mono".into(),
            tracking: 1.0,
            size_bar: 15.0,
            size_menu: 23.0,
            size_title: 24.0,
            size_hint: 14.0,
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
            clock_format: "%H:%M".into(),
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
            filter_hint: "X to search".into(),
            header_height: 46,
            hint_height: 46,
            radius: 2,
            columns: 3,
            tile_height: 86,
            gap: 10,
        }
    }
}

impl Default for Pointer {
    fn default() -> Self {
        Self {
            step: 16,
            repeat_delay: 200,
            repeat_rate: 40,
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
