//! Embedded, deterministic sample content and software-rendered broadcast test card.
use font8x8::{BASIC_FONTS, UnicodeFonts};

pub struct Canvas {
    width: usize,
    height: usize,
    rgba: Vec<u8>,
}

impl Canvas {
    pub(crate) fn from_image(image: image::RgbaImage) -> Self {
        Self {
            width: image.width() as usize,
            height: image.height() as usize,
            rgba: image.into_raw(),
        }
    }

    pub fn width(&self) -> usize {
        self.width
    }
    pub fn height(&self) -> usize {
        self.height
    }
    pub fn rgba(&self) -> &[u8] {
        &self.rgba
    }

    /// Nearest-scale directly into a tightly packed Wayland ARGB8888 buffer.
    pub(crate) fn write_argb_scaled(&self, width: usize, height: usize, output: &mut [u8]) {
        assert!(width > 0 && height > 0);
        let stride = width.checked_mul(4).expect("Row fits address space");
        let length = stride
            .checked_mul(height)
            .expect("Image fits address space");
        // SCTK slots may have alignment padding after the last row.
        let output = &mut output[..length];
        for y in 0..height {
            let source_y = y * self.height / height;
            let offset = y * stride;
            if y > 0 && source_y == (y - 1) * self.height / height {
                output.copy_within(offset - stride..offset, offset);
                continue;
            }
            let source = &self.rgba[source_y * self.width * 4..][..self.width * 4];
            for (x, pixel) in output[offset..offset + stride]
                .chunks_exact_mut(4)
                .enumerate()
            {
                let i = (x * self.width / width) * 4;
                let colour =
                    u32::from_be_bytes([source[i + 3], source[i], source[i + 1], source[i + 2]]);
                pixel.copy_from_slice(&colour.to_ne_bytes());
            }
        }
    }

    fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            rgba: vec![
                0;
                width
                    .checked_mul(height)
                    .and_then(|pixels| pixels.checked_mul(4))
                    .expect("Canvas dimensions fit address space")
            ],
        }
    }

    fn rect(&mut self, x: usize, y: usize, w: usize, h: usize, colour: [u8; 3]) {
        let right = x.saturating_add(w).min(self.width);
        let bottom = y.saturating_add(h).min(self.height);
        if x >= right || y >= bottom {
            return;
        }
        let start = (y * self.width + x) * 4;
        let length = (right - x) * 4;
        let pixel = [colour[0], colour[1], colour[2], 255];
        for destination in self.rgba[start..start + length].chunks_exact_mut(4) {
            destination.copy_from_slice(&pixel);
        }
        for row in y + 1..bottom {
            self.rgba
                .copy_within(start..start + length, (row * self.width + x) * 4);
        }
    }

    fn text(&mut self, text: &str, x: usize, y: usize, scale: usize, colour: [u8; 3]) {
        for (i, ch) in text.chars().enumerate() {
            if let Some(glyph) = BASIC_FONTS.get(ch) {
                for (row, bits) in glyph.iter().enumerate() {
                    for col in 0..8 {
                        if bits & (1 << col) != 0 {
                            self.rect(
                                x + (i * 8 + col) * scale,
                                y + row * scale,
                                scale,
                                scale,
                                colour,
                            );
                        }
                    }
                }
            }
        }
    }
}

pub fn desktop() -> Canvas {
    let image = image::load_from_memory(include_bytes!("../assets/preview-desktop.png"))
        .expect("Embedded preview image is valid")
        .into_rgba8();
    Canvas::from_image(image)
}

pub fn test_card(width: usize, height: usize, remaining: u64) -> Canvas {
    test_card_for_duration(width, height, remaining, 300)
}

pub(crate) fn test_card_for_duration(
    width: usize,
    height: usize,
    remaining: u64,
    total: u64,
) -> Canvas {
    // Draw at the storyboard's native size, then nearest-scale for software Wayland buffers.
    let mut c = Canvas::new(1280, 720);
    let colours = [
        [197, 197, 188],
        [193, 185, 83],
        [100, 181, 185],
        [113, 167, 121],
        [163, 116, 165],
        [173, 98, 105],
        [82, 108, 159],
    ];
    for (i, colour) in colours.into_iter().enumerate() {
        c.rect(i * 1280 / 7, 0, 184, 494, colour);
    }
    for (i, colour) in [
        [82, 108, 159],
        [25, 29, 29],
        [163, 116, 165],
        [25, 29, 29],
        [100, 181, 185],
        [25, 29, 29],
        [197, 197, 188],
    ]
    .into_iter()
    .enumerate()
    {
        c.rect(i * 1280 / 7, 494, 184, 58, colour);
    }
    c.rect(0, 552, 1280, 168, [17, 23, 24]);
    c.rect(0, 552, 225, 168, [24, 49, 60]);
    c.rect(225, 552, 245, 168, [197, 197, 188]);
    c.rect(470, 552, 200, 168, [53, 38, 67]);
    for x in (870..1280).step_by(26) {
        c.rect(x, 552, 13, 168, [40, 45, 43]);
    }
    c.rect(222, 194, 836, 281, [17, 23, 24]);
    c.rect(242, 213, 796, 1, [120, 128, 117]);
    c.text("TRANSMISSION PAUSED", 424, 240, 3, [177, 190, 166]);
    c.text("TAKE A BREAK", 352, 300, 6, [238, 238, 226]);
    c.text(
        "NO KEYS. NO MOUSE. JUST BE HERE.",
        392,
        370,
        2,
        [183, 192, 178],
    );
    let total = total.max(1);
    let remaining = remaining.min(total);
    let label = format!(
        "{:02}:{:02}  /  {:02}:{:02}",
        remaining / 60,
        remaining % 60,
        total / 60,
        total % 60
    );
    c.text(&label, 460, 414, 3, [217, 232, 178]);
    c.rect(426, 450, 428, 3, [63, 73, 59]);
    c.rect(
        426,
        450,
        (428 * (total - remaining) / total) as usize,
        3,
        [217, 232, 178],
    );
    c.text("BORK  /  REST CHANNEL", 472, 630, 2, [221, 220, 208]);
    if width == 1280 && height == 720 {
        return c;
    }
    let mut scaled = Canvas::new(width, height);
    for y in 0..height {
        for x in 0..width {
            let src = ((y * 720 / height) * 1280 + x * 1280 / width) * 4;
            let dst = (y * width + x) * 4;
            scaled.rgba[dst..dst + 4].copy_from_slice(&c.rgba[src..src + 4]);
        }
    }
    scaled
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rectangles_match_per_pixel_drawing_including_clipping() {
        let mut canvas = Canvas::new(37, 29);
        let mut expected = vec![0; 37 * 29 * 4];
        for (index, (x, y, w, h)) in [
            (0, 0, 37, 29),
            (5, 4, 7, 11),
            (35, 27, 20, 20),
            (100, 3, 5, 4),
            (1, 100, 4, 6),
            (0, 0, 0, 10),
            (0, 0, 10, 0),
            (36, 28, usize::MAX, usize::MAX),
        ]
        .into_iter()
        .enumerate()
        {
            let colour = [index as u8, 23, 42];
            canvas.rect(x, y, w, h, colour);
            for row in y..y.saturating_add(h).min(29) {
                for column in x..x.saturating_add(w).min(37) {
                    let offset = (row * 37 + column) * 4;
                    expected[offset..offset + 4]
                        .copy_from_slice(&[colour[0], colour[1], colour[2], 255]);
                }
            }
            assert_eq!(canvas.rgba(), expected);
        }
    }

    #[test]
    fn direct_argb_scaling_matches_rgba_card_at_varied_sizes() {
        for (width, height) in [(1, 1), (127, 73), (1280, 720), (1921, 1081)] {
            for remaining in [0, 137, 300] {
                let native = test_card(1280, 720, remaining);
                let expected = test_card(width, height, remaining);
                let length = width * height * 4;
                let mut output = vec![0xa5; length + 60];
                native.write_argb_scaled(width, height, &mut output);
                assert!(output[length..].iter().all(|byte| *byte == 0xa5));
                for (argb, rgba) in output[..length]
                    .chunks_exact(4)
                    .zip(expected.rgba().chunks_exact(4))
                {
                    assert_eq!(
                        u32::from_ne_bytes(argb.try_into().unwrap()),
                        u32::from_be_bytes([rgba[3], rgba[0], rgba[1], rgba[2]])
                    );
                }
            }
        }
    }
}
