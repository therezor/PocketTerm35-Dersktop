//! Icons from a freedesktop icon theme, rasterised once and kept.
//!
//! The shell ships no artwork. Papirus is packaged, complete and readable at
//! 24px, which is the size this panel can afford. A missing theme is not an
//! error: every caller has a drawn fallback, so the shell still runs on a
//! machine with no icons installed.

use crate::canvas::Canvas;
use pt35_common::theme::Rgb;
use resvg::{tiny_skia, usvg};
use std::collections::HashMap;
use std::path::PathBuf;

/// Where themes live, in the order a freedesktop lookup would search.
const ROOTS: &[&str] = &["/usr/share/icons", "/usr/local/share/icons"];

/// Sizes to try, nearest first: an SVG scales, but starting from the artwork
/// drawn for roughly the right size keeps the hinting Papirus puts in.
const SIZES: &[&str] = &["24x24", "32x32", "22x22", "48x48", "16x16", "64x64"];

/// One rasterised icon, premultiplied RGBA, square.
pub struct Icon {
    pub size: u32,
    pixels: Vec<u8>,
}

impl Icon {
    /// Draw it as it is: for an application icon, whose colours are the point.
    pub fn draw(&self, canvas: &mut Canvas, x: i32, y: i32) {
        for row in 0..self.size {
            for col in 0..self.size {
                let i = ((row * self.size + col) * 4) as usize;
                let (r, g, b, a) = (
                    self.pixels[i],
                    self.pixels[i + 1],
                    self.pixels[i + 2],
                    self.pixels[i + 3],
                );
                if a == 0 {
                    continue;
                }
                // The pixmap is premultiplied; `blend` wants straight colour
                // plus coverage.
                let un = |c: u8| ((c as u32 * 255) / a as u32).min(255) as u8;
                canvas.blend(x + col as i32, y + row as i32, a, Rgb(un(r), un(g), un(b)));
            }
        }
    }

    /// Draw the shape in one colour. Status icons belong to the theme, not to
    /// whoever drew them.
    pub fn draw_tinted(&self, canvas: &mut Canvas, x: i32, y: i32, color: Rgb) {
        for row in 0..self.size {
            for col in 0..self.size {
                let alpha = self.pixels[((row * self.size + col) * 4 + 3) as usize];
                canvas.blend(x + col as i32, y + row as i32, alpha, color);
            }
        }
    }
}

/// An icon theme, with its inherited fallbacks, rasterising on first use.
pub struct Icons {
    themes: Vec<String>,
    cache: HashMap<(String, u32), Option<Icon>>,
}

impl Icons {
    /// `theme` first, then the usual fallbacks. Pass the name from theme.toml.
    pub fn new(theme: &str) -> Self {
        let mut themes = vec![theme.to_string()];
        for fallback in ["Papirus-Dark", "Papirus", "hicolor"] {
            if !themes.iter().any(|t| t == fallback) {
                themes.push(fallback.to_string());
            }
        }
        Self {
            themes,
            cache: HashMap::new(),
        }
    }

    /// True when at least one of the themes is actually installed.
    pub fn available(&self) -> bool {
        self.themes.iter().any(|theme| {
            ROOTS
                .iter()
                .any(|root| PathBuf::from(root).join(theme).is_dir())
        })
    }

    pub fn get(&mut self, name: &str, size: u32) -> Option<&Icon> {
        let key = (name.to_string(), size);
        if !self.cache.contains_key(&key) {
            let icon = find(&self.themes, name).and_then(|path| render(&path, size));
            self.cache.insert(key.clone(), icon);
        }
        self.cache.get(&key)?.as_ref()
    }
}

/// First `<root>/<theme>/<size>/<category>/<name>.svg` that exists.
fn find(themes: &[String], name: &str) -> Option<PathBuf> {
    let file = format!("{name}.svg");
    for root in ROOTS {
        for theme in themes {
            for size in SIZES {
                let dir = PathBuf::from(root).join(theme).join(size);
                let Ok(categories) = std::fs::read_dir(&dir) else {
                    continue;
                };
                for category in categories.flatten() {
                    let candidate = category.path().join(&file);
                    if candidate.is_file() {
                        return Some(candidate);
                    }
                }
            }
        }
    }
    None
}

fn render(path: &PathBuf, size: u32) -> Option<Icon> {
    let data = std::fs::read(path).ok()?;
    let tree = usvg::Tree::from_data(&data, &usvg::Options::default()).ok()?;
    let mut pixmap = tiny_skia::Pixmap::new(size, size)?;
    let source = tree.size();
    let scale = (size as f32 / source.width()).min(size as f32 / source.height());
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    Some(Icon {
        size,
        pixels: pixmap.take(),
    })
}

/// The Papirus name for a volume level, so the icon follows the level the way
/// the old drawn meter did.
pub fn volume_icon(level: u8, muted: bool) -> &'static str {
    match (muted, level) {
        (true, _) | (_, 0) => "audio-volume-muted",
        (_, 1..=33) => "audio-volume-low",
        (_, 34..=66) => "audio-volume-medium",
        _ => "audio-volume-high",
    }
}

/// The Papirus name for a signal strength. `None` is "no network".
pub fn wifi_icon(signal: Option<u8>) -> &'static str {
    match signal {
        None => "network-wireless-offline",
        Some(0..=20) => "network-wireless-signal-none",
        Some(21..=40) => "network-wireless-signal-weak",
        Some(41..=60) => "network-wireless-signal-ok",
        Some(61..=80) => "network-wireless-signal-good",
        Some(_) => "network-wireless-signal-excellent",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_volume_icon_follows_the_level() {
        assert_eq!(volume_icon(0, false), "audio-volume-muted");
        assert_eq!(volume_icon(80, true), "audio-volume-muted");
        assert_eq!(volume_icon(20, false), "audio-volume-low");
        assert_eq!(volume_icon(50, false), "audio-volume-medium");
        assert_eq!(volume_icon(90, false), "audio-volume-high");
    }

    #[test]
    fn the_wifi_icon_follows_the_signal() {
        assert_eq!(wifi_icon(None), "network-wireless-offline");
        assert_eq!(wifi_icon(Some(10)), "network-wireless-signal-none");
        assert_eq!(wifi_icon(Some(95)), "network-wireless-signal-excellent");
    }

    #[test]
    fn a_missing_theme_is_not_an_error() {
        let mut icons = Icons::new("NoSuchTheme");
        assert!(icons.get("definitely-not-an-icon", 24).is_none());
    }
}
