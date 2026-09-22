//! Font loading and text drawing.
//!
//! No fontconfig and no shaping engine: the shell's own UI is Latin text at a
//! handful of sizes, so a single TTF rasterised by fontdue covers it at a
//! fraction of the memory. The font is looked up by the family name in
//! `theme.toml` against the paths Raspberry Pi OS actually ships.

use anyhow::{Context, Result};
use pt35_common::theme::Rgb;
use std::collections::HashMap;
use std::path::PathBuf;

use crate::canvas::Canvas;

pub struct Font {
    inner: fontdue::Font,
    /// Rasterised glyphs, keyed by (char, size in 1/10 px) — the shell redraws
    /// the same few strings constantly, so caching is most of the speed.
    cache: HashMap<(char, u32), (fontdue::Metrics, Vec<u8>)>,
}

/// Where a family name might live on Raspberry Pi OS / Debian.
pub fn candidates(family: &str) -> Vec<PathBuf> {
    let slug: String = family.chars().filter(|c| !c.is_whitespace()).collect();
    let mut paths = Vec::new();
    if let Some(explicit) = std::env::var_os("PT35_FONT") {
        paths.push(PathBuf::from(explicit));
    }
    for dir in [
        "/usr/share/fonts/truetype/dejavu",
        "/usr/share/fonts/truetype/liberation",
        "/usr/share/fonts/truetype/noto",
        "/usr/share/fonts/TTF",
        "/System/Library/Fonts/Supplemental",
    ] {
        paths.push(PathBuf::from(dir).join(format!("{slug}.ttf")));
        paths.push(PathBuf::from(dir).join(format!("{slug}-Regular.ttf")));
    }
    paths
}

impl Font {
    /// Load the first candidate that exists, or fail with every path tried so
    /// the log says exactly what to install.
    pub fn load(family: &str) -> Result<Self> {
        let tried = candidates(family);
        for path in &tried {
            let Ok(bytes) = std::fs::read(path) else { continue };
            let inner = fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default())
                .map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?;
            log::debug!("font {family:?} from {}", path.display());
            return Ok(Self { inner, cache: HashMap::new() });
        }
        Err(anyhow::anyhow!(
            "no font file for {family:?}; tried: {}",
            tried.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", ")
        ))
        .context("install fonts-dejavu-core, or set PT35_FONT to a .ttf")
    }

    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self> {
        let inner = fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default())
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        Ok(Self { inner, cache: HashMap::new() })
    }

    fn glyph(&mut self, ch: char, size: f32) -> &(fontdue::Metrics, Vec<u8>) {
        let key = (ch, (size * 10.0) as u32);
        self.cache.entry(key).or_insert_with(|| self.inner.rasterize(ch, size))
    }

    /// Width in pixels of `text` at `size`.
    pub fn measure(&mut self, text: &str, size: f32) -> u32 {
        let mut width = 0.0;
        for ch in text.chars() {
            width += self.glyph(ch, size).0.advance_width;
        }
        width.ceil() as u32
    }

    /// Draw `text` with its left edge at `x` and its baseline at `y`.
    /// Returns the x coordinate just past the last glyph.
    pub fn draw(&mut self, canvas: &mut Canvas, text: &str, x: i32, y: i32, size: f32, color: Rgb) -> i32 {
        let mut pen = x as f32;
        for ch in text.chars() {
            let (metrics, bitmap) = self.glyph(ch, size).clone();
            let gx = pen as i32 + metrics.xmin;
            let gy = y - metrics.height as i32 - metrics.ymin;
            for row in 0..metrics.height {
                for col in 0..metrics.width {
                    let coverage = bitmap[row * metrics.width + col];
                    canvas.blend(gx + col as i32, gy + row as i32, coverage, color);
                }
            }
            pen += metrics.advance_width;
        }
        pen.ceil() as i32
    }

    /// Shorten `text` with an ellipsis so it fits in `max_width` pixels.
    pub fn elide(&mut self, text: &str, size: f32, max_width: u32) -> String {
        if self.measure(text, size) <= max_width {
            return text.to_string();
        }
        // When even the ellipsis does not fit, a bare truncation is better
        // than drawing past the edge of a 640px screen.
        let ellipsis = "…";
        let ellipsis_width = self.measure(ellipsis, size);
        let (budget, suffix) = if ellipsis_width > max_width {
            (max_width, "")
        } else {
            (max_width - ellipsis_width, ellipsis)
        };
        let mut out = String::new();
        let mut width = 0;
        for ch in text.chars() {
            let advance = self.measure(&ch.to_string(), size);
            if width + advance > budget {
                break;
            }
            width += advance;
            out.push(ch);
        }
        out.push_str(suffix);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Any real TTF on the host will do; skip when the box has none.
    fn test_font() -> Option<Font> {
        for family in ["DejaVu Sans", "Helvetica", "Arial"] {
            if let Ok(font) = Font::load(family) {
                return Some(font);
            }
        }
        None
    }

    #[test]
    fn candidate_paths_cover_debian_and_the_env_override() {
        std::env::set_var("PT35_FONT", "/tmp/x.ttf");
        let paths = candidates("DejaVu Sans");
        std::env::remove_var("PT35_FONT");
        assert_eq!(paths[0], PathBuf::from("/tmp/x.ttf"));
        assert!(paths.iter().any(|p| p.ends_with("dejavu/DejaVuSans.ttf")));
    }

    #[test]
    fn missing_font_names_every_path_tried() {
        let err = match Font::load("NoSuchFamilyAtAll") {
            Ok(_) => panic!("a font named NoSuchFamilyAtAll should not exist"),
            Err(err) => err,
        };
        let text = format!("{err:#}");
        assert!(text.contains("fonts-dejavu-core"), "error should say how to fix it: {text}");
    }

    #[test]
    fn measures_and_elides() {
        let Some(mut font) = test_font() else { return };
        let wide = font.measure("mmmmmmmmmmmm", 16.0);
        let narrow = font.measure("ii", 16.0);
        assert!(wide > narrow);

        let elided = font.elide("a very long menu entry indeed", 16.0, wide);
        assert!(elided.ends_with('…'), "{elided:?}");
        assert!(font.measure(&elided, 16.0) <= wide, "elided text must fit the budget");

        // Budget too small even for the ellipsis: truncate rather than overflow.
        let tiny = font.elide("a very long menu entry indeed", 16.0, 4);
        assert!(font.measure(&tiny, 16.0) <= 4, "{tiny:?} overflows a 4px budget");
    }

    #[test]
    fn draws_ink_on_the_canvas() {
        let Some(mut font) = test_font() else { return };
        let mut canvas = Canvas::new(64, 24);
        canvas.fill(Rgb(0, 0, 0));
        let end = font.draw(&mut canvas, "Hi", 2, 18, 14.0, Rgb(0xff, 0xff, 0xff));
        assert!(end > 2);
        let lit = (0..64 * 24).filter(|i| canvas.pixel(i % 64, i / 64) != 0xff00_0000).count();
        assert!(lit > 10, "expected glyph coverage, got {lit} lit pixels");
    }
}
