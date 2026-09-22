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
    pub speed: f32,
    pub accel: f32,
    pub grid_levels: u8,
    pub grid_labels: String,
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
            background: Rgb(0x0d, 0x11, 0x17),
            button_a: Rgb(0x52, 0xa4, 0x41),
            button_b: Rgb(0xf8, 0x51, 0x49),
            button_x: Rgb(0x48, 0xe6, 0xfe),
            button_y: Rgb(0xff, 0xc1, 0x07),
            button_neutral: Rgb(0x3a, 0x41, 0x4a),
            background_alt: Rgb(0x17, 0x1c, 0x22),
            foreground: Rgb(0xd4, 0xd8, 0xdd),
            muted: Rgb(0x7d, 0x87, 0x94),
            accent: Rgb(0x61, 0xaf, 0xef),
            accent_fg: Rgb(0x0b, 0x0e, 0x11),
            warning: Rgb(0xe5, 0xc0, 0x7b),
            critical: Rgb(0xe0, 0x6c, 0x75),
            ok: Rgb(0x98, 0xc3, 0x79),
            border: Rgb(0x24, 0x2b, 0x33),
        }
    }
}

impl Default for Fonts {
    fn default() -> Self {
        Self {
            family: "DejaVu Sans".into(),
            size_bar: 14.0,
            size_menu: 21.0,
            size_title: 22.0,
            size_hint: 13.0,
        }
    }
}

impl Default for Bar {
    fn default() -> Self {
        Self {
            height: 26,
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
            row_height: 46,
            padding_x: 14,
            show_numbers: true,
            filter_hint: "X to search".into(),
            header_height: 44,
            hint_height: 44,
            radius: 10,
            columns: 3,
            tile_height: 80,
            gap: 8,
        }
    }
}

impl Default for Pointer {
    fn default() -> Self {
        Self {
            speed: 380.0,
            accel: 2.2,
            grid_levels: 2,
            grid_labels: "asdfghjkl".into(),
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
