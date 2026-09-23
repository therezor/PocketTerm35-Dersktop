//! A plain ARGB8888 canvas with the handful of primitives the shell needs.

use pt35_common::theme::Rgb;

pub struct Canvas {
    pub width: u32,
    pub height: u32,
    pixels: Vec<u32>,
}

impl Canvas {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            pixels: vec![0xff00_0000; (width * height) as usize],
        }
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

    /// Wipe the surface to fully transparent — for overlays (the pointer grid)
    /// that must let the application below show through.
    pub fn clear_transparent(&mut self) {
        self.pixels.fill(0);
    }

    /// Fill with a colour at a given alpha, premultiplied as Wayland expects.
    pub fn fill_alpha(&mut self, color: Rgb, alpha: u8) {
        let a = alpha as u32;
        let premul = |c: u8| ((c as u32 * a) / 255) & 0xff;
        self.pixels
            .fill((a << 24) | (premul(color.0) << 16) | (premul(color.1) << 8) | premul(color.2));
    }

    pub fn rect(&mut self, x: i32, y: i32, w: u32, h: u32, color: Rgb) {
        let argb = color.to_argb8888();
        // Clamped both ways: a rect that starts past the right edge must come
        // out empty, not as a start index beyond its end.
        let (x0, y0) = (
            (x.max(0) as u32).min(self.width),
            (y.max(0) as u32).min(self.height),
        );
        let x1 = ((x + w as i32).max(0) as u32).min(self.width);
        let y1 = ((y + h as i32).max(0) as u32).min(self.height);
        if x0 >= x1 {
            return;
        }
        for row in y0..y1 {
            let start = (row * self.width + x0) as usize;
            let end = (row * self.width + x1) as usize;
            self.pixels[start..end].fill(argb);
        }
    }

    /// Draw an RGB image scaled to `w` x `h`, nearest neighbour, clipped to the
    /// canvas. For window previews: a few thousand pixels, cheap every frame.
    #[allow(clippy::too_many_arguments)]
    pub fn blit_rgb(&mut self, x: i32, y: i32, w: u32, h: u32, rgb: &[u8], src_w: u32, src_h: u32) {
        if src_w == 0 || src_h == 0 || rgb.len() < (src_w * src_h * 3) as usize {
            return;
        }
        for row in 0..h as i32 {
            let ty = y + row;
            if ty < 0 || ty >= self.height as i32 {
                continue;
            }
            let sy = (row as u32 * src_h / h.max(1)).min(src_h - 1);
            for col in 0..w as i32 {
                let tx = x + col;
                if tx < 0 || tx >= self.width as i32 {
                    continue;
                }
                let sx = (col as u32 * src_w / w.max(1)).min(src_w - 1);
                let i = ((sy * src_w + sx) * 3) as usize;
                self.pixels[(ty as u32 * self.width + tx as u32) as usize] = 0xff00_0000
                    | (rgb[i] as u32) << 16
                    | (rgb[i + 1] as u32) << 8
                    | rgb[i + 2] as u32;
            }
        }
    }

    /// A filled rectangle with rounded corners, with the corner pixels
    /// anti-aliased — on a 640x480 panel a hard corner reads as a stair-step.
    pub fn rounded_rect(&mut self, x: i32, y: i32, w: u32, h: u32, radius: u32, color: Rgb) {
        let r = radius.min(w / 2).min(h / 2) as i32;
        if r <= 0 {
            self.rect(x, y, w, h, color);
            return;
        }
        // Middle band and the two side bands, then the four corners.
        self.rect(x, y + r, w, h - 2 * r as u32, color);
        self.rect(x + r, y, w - 2 * r as u32, r as u32, color);
        self.rect(x + r, y + h as i32 - r, w - 2 * r as u32, r as u32, color);

        let corners = [
            (x + r, y + r, -1, -1),
            (x + w as i32 - r - 1, y + r, 1, -1),
            (x + r, y + h as i32 - r - 1, -1, 1),
            (x + w as i32 - r - 1, y + h as i32 - r - 1, 1, 1),
        ];
        for (cx, cy, sx, sy) in corners {
            for dy in 0..=r {
                for dx in 0..=r {
                    let distance = ((dx * dx + dy * dy) as f32).sqrt();
                    let coverage = ((r as f32 + 0.5 - distance) * 255.0).clamp(0.0, 255.0) as u8;
                    self.blend(cx + sx * dx, cy + sy * dy, coverage, color);
                }
            }
        }
    }

    /// A filled circle — the hint bar's button chips.
    pub fn circle(&mut self, cx: i32, cy: i32, radius: u32, color: Rgb) {
        let r = radius as i32;
        for dy in -r..=r {
            for dx in -r..=r {
                let distance = ((dx * dx + dy * dy) as f32).sqrt();
                let coverage = ((r as f32 + 0.5 - distance) * 255.0).clamp(0.0, 255.0) as u8;
                self.blend(cx + dx, cy + dy, coverage, color);
            }
        }
    }

    /// A rectangle blended at `alpha` over what is already there.
    pub fn rect_alpha(&mut self, x: i32, y: i32, w: u32, h: u32, color: Rgb, alpha: u8) {
        for row in 0..h as i32 {
            for col in 0..w as i32 {
                self.blend(x + col, y + row, alpha, color);
            }
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
    fn an_image_scales_and_clips() {
        let mut canvas = Canvas::new(4, 4);
        // 2x1: red, blue.
        let rgb = [0xff, 0, 0, 0, 0, 0xff];
        canvas.blit_rgb(-1, 0, 4, 2, &rgb, 2, 1);
        assert_eq!(canvas.pixel(0, 0), 0xffff_0000, "left half is red");
        assert_eq!(canvas.pixel(2, 1), 0xff00_00ff, "right half is blue");
        canvas.blit_rgb(0, 0, 4, 4, &rgb[..3], 2, 1); // too short: ignored
    }

    #[test]
    fn fills_and_clips_rectangles() {
        let mut canvas = Canvas::new(8, 4);
        canvas.fill(Rgb(0, 0, 0));
        canvas.rect(-2, -2, 4, 4, Rgb(0xff, 0, 0));
        // Wholly off the right and bottom edges: nothing drawn, no panic.
        canvas.rect(12, 0, 4, 4, Rgb(0xff, 0, 0));
        canvas.rect(0, 12, 4, 4, Rgb(0xff, 0, 0));
        assert_eq!(canvas.pixel(0, 0), 0xffff_0000);
        assert_eq!(
            canvas.pixel(2, 2),
            0xff00_0000,
            "clipped area stays background"
        );

        canvas.rect(6, 0, 100, 100, Rgb(0, 0xff, 0));
        assert_eq!(
            canvas.pixel(7, 3),
            0xff00_ff00,
            "overhanging rect is clipped, not wrapped"
        );
    }

    #[test]
    fn blends_coverage() {
        let mut canvas = Canvas::new(2, 1);
        canvas.fill(Rgb(0, 0, 0));
        canvas.blend(0, 0, 255, Rgb(0xff, 0xff, 0xff));
        assert_eq!(canvas.pixel(0, 0), 0xffff_ffff);
        canvas.blend(1, 0, 128, Rgb(0xff, 0xff, 0xff));
        let half = canvas.pixel(1, 0) & 0xff;
        assert!(
            (120..=136).contains(&half),
            "50% coverage should be mid grey, got {half}"
        );
    }

    #[test]
    fn out_of_bounds_blend_is_ignored() {
        let mut canvas = Canvas::new(2, 2);
        canvas.blend(-1, 0, 255, Rgb(0xff, 0, 0));
        canvas.blend(0, 9, 255, Rgb(0xff, 0, 0));
        assert_eq!(canvas.pixel(0, 0), 0xff00_0000);
    }

    #[test]
    fn transparent_and_translucent_fills() {
        let mut canvas = Canvas::new(2, 1);
        canvas.clear_transparent();
        assert_eq!(canvas.pixel(0, 0), 0);
        canvas.fill_alpha(Rgb(0xff, 0xff, 0xff), 128);
        let px = canvas.pixel(0, 0);
        assert_eq!(px >> 24, 128, "alpha is kept");
        assert_eq!(px & 0xff, 128, "colour is premultiplied by alpha");
    }

    #[test]
    fn rounded_rect_fills_the_middle_and_softens_the_corners() {
        let mut canvas = Canvas::new(20, 20);
        canvas.fill(Rgb(0, 0, 0));
        canvas.rounded_rect(0, 0, 20, 20, 6, Rgb(0xff, 0xff, 0xff));
        assert_eq!(canvas.pixel(10, 10), 0xffff_ffff, "the middle is solid");
        let corner = canvas.pixel(0, 0) & 0xff;
        assert!(
            corner < 0x40,
            "the very corner is mostly background, got {corner:#x}"
        );
        assert_eq!(
            canvas.pixel(10, 0),
            0xffff_ffff,
            "the top edge is solid between corners"
        );
    }

    #[test]
    fn circles_are_centred() {
        let mut canvas = Canvas::new(21, 21);
        canvas.fill(Rgb(0, 0, 0));
        canvas.circle(10, 10, 8, Rgb(0xff, 0xff, 0xff));
        assert_eq!(canvas.pixel(10, 10), 0xffff_ffff);
        assert_eq!(
            canvas.pixel(0, 0),
            0xff00_0000,
            "outside the radius is untouched"
        );
    }

    #[test]
    fn rect_alpha_blends_instead_of_replacing() {
        let mut canvas = Canvas::new(4, 4);
        canvas.fill(Rgb(0, 0, 0));
        canvas.rect_alpha(0, 0, 4, 4, Rgb(0xff, 0xff, 0xff), 128);
        let value = canvas.pixel(1, 1) & 0xff;
        assert!(
            (120..=136).contains(&value),
            "expected a mid grey, got {value:#x}"
        );
    }

    #[test]
    fn buffer_is_the_right_size_for_wayland() {
        let canvas = Canvas::new(640, 480);
        assert_eq!(canvas.as_bytes().len(), 640 * 480 * 4);
    }
}
