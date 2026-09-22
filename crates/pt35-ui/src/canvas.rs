//! A plain ARGB8888 canvas with the handful of primitives the shell needs.

use pt35_common::theme::Rgb;

pub struct Canvas {
    pub width: u32,
    pub height: u32,
    pixels: Vec<u32>,
}

impl Canvas {
    pub fn new(width: u32, height: u32) -> Self {
        Self { width, height, pixels: vec![0xff00_0000; (width * height) as usize] }
    }

    pub fn as_bytes(&self) -> &[u8] {
        // Wayland wants the raw buffer; ARGB8888 is little-endian BGRA in memory,
        // which is exactly how a u32 0xAARRGGBB lays out on the Pi (LE).
        unsafe {
            std::slice::from_raw_parts(self.pixels.as_ptr() as *const u8, self.pixels.len() * 4)
        }
    }

    pub fn pixel(&self, x: u32, y: u32) -> u32 {
        self.pixels[(y * self.width + x) as usize]
    }

    pub fn fill(&mut self, color: Rgb) {
        self.pixels.fill(color.to_argb8888());
    }

    pub fn rect(&mut self, x: i32, y: i32, w: u32, h: u32, color: Rgb) {
        let argb = color.to_argb8888();
        let (x0, y0) = (x.max(0) as u32, y.max(0) as u32);
        let x1 = ((x + w as i32).max(0) as u32).min(self.width);
        let y1 = ((y + h as i32).max(0) as u32).min(self.height);
        for row in y0..y1 {
            let start = (row * self.width + x0) as usize;
            let end = (row * self.width + x1) as usize;
            self.pixels[start..end].fill(argb);
        }
    }

    /// Blend one coverage value (0..=255, from the font rasteriser) over a pixel.
    pub fn blend(&mut self, x: i32, y: i32, coverage: u8, color: Rgb) {
        if x < 0 || y < 0 || x as u32 >= self.width || y as u32 >= self.height || coverage == 0 {
            return;
        }
        let index = (y as u32 * self.width + x as u32) as usize;
        let dst = self.pixels[index];
        let a = coverage as u32;
        let inv = 255 - a;
        let mix = |shift: u32, src: u8| -> u32 {
            let d = (dst >> shift) & 0xff;
            (((src as u32 * a) + (d * inv)) / 255) << shift
        };
        self.pixels[index] = 0xff00_0000 | mix(16, color.0) | mix(8, color.1) | mix(0, color.2);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fills_and_clips_rectangles() {
        let mut canvas = Canvas::new(8, 4);
        canvas.fill(Rgb(0, 0, 0));
        canvas.rect(-2, -2, 4, 4, Rgb(0xff, 0, 0));
        assert_eq!(canvas.pixel(0, 0), 0xffff_0000);
        assert_eq!(canvas.pixel(2, 2), 0xff00_0000, "clipped area stays background");

        canvas.rect(6, 0, 100, 100, Rgb(0, 0xff, 0));
        assert_eq!(canvas.pixel(7, 3), 0xff00_ff00, "overhanging rect is clipped, not wrapped");
    }

    #[test]
    fn blends_coverage() {
        let mut canvas = Canvas::new(2, 1);
        canvas.fill(Rgb(0, 0, 0));
        canvas.blend(0, 0, 255, Rgb(0xff, 0xff, 0xff));
        assert_eq!(canvas.pixel(0, 0), 0xffff_ffff);
        canvas.blend(1, 0, 128, Rgb(0xff, 0xff, 0xff));
        let half = canvas.pixel(1, 0) & 0xff;
        assert!((120..=136).contains(&half), "50% coverage should be mid grey, got {half}");
    }

    #[test]
    fn out_of_bounds_blend_is_ignored() {
        let mut canvas = Canvas::new(2, 2);
        canvas.blend(-1, 0, 255, Rgb(0xff, 0, 0));
        canvas.blend(0, 9, 255, Rgb(0xff, 0, 0));
        assert_eq!(canvas.pixel(0, 0), 0xff00_0000);
    }

    #[test]
    fn buffer_is_the_right_size_for_wayland() {
        let canvas = Canvas::new(640, 480);
        assert_eq!(canvas.as_bytes().len(), 640 * 480 * 4);
    }
}
