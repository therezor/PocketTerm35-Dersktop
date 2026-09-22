//! Colours, fonts and sizes, tuned for a 640x480 panel.

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
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

impl Default for Theme {
    fn default() -> Self {
        Self {
            color: Colors::default(),
            font: Fonts::default(),
            bar: Bar::default(),
            menu: Menu::default(),
            pointer: Pointer::default(),
        }
    }
}

impl Default for Colors {
    fn default() -> Self {
        Self {
            background: Rgb(0x10, 0x14, 0x18),
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
            size_bar: 12.0,
            size_menu: 16.0,
            size_title: 18.0,
            size_hint: 11.0,
        }
    }
}

impl Default for Bar {
    fn default() -> Self {
        Self {
            height: 18,
            padding_x: 4,
            show_battery: Visibility::Auto,
            show_network: true,
            clock_format: "%H:%M".into(),
        }
    }
}

impl Default for Menu {
    fn default() -> Self {
        Self {
            rows_visible: 9,
            row_height: 34,
            padding_x: 10,
            show_numbers: true,
            filter_hint: "type to filter".into(),
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
        assert_eq!(theme.bar.height, 18);
    }
}
