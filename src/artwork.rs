//! Owned RGBA canvases shared by the shader sample and software overtime renderer.
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
            .expect("Canvas dimensions fit address space");
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
}

pub fn desktop() -> Canvas {
    let image = image::load_from_memory(include_bytes!("../assets/preview-desktop.png"))
        .expect("Embedded preview image is valid")
        .into_rgba8();
    Canvas::from_image(image)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn argb_scaling_preserves_channels_and_buffer_padding() {
        let image = image::RgbaImage::from_fn(7, 5, |x, y| {
            image::Rgba([x as u8 * 31, y as u8 * 51, 42, 255])
        });
        let canvas = Canvas::from_image(image);
        for (width, height) in [(1, 1), (3, 2), (7, 5), (17, 23)] {
            let length = width * height * 4;
            let mut output = vec![0xa5; length + 60];
            canvas.write_argb_scaled(width, height, &mut output);
            assert!(output[length..].iter().all(|byte| *byte == 0xa5));
            for y in 0..height {
                for x in 0..width {
                    let offset = (y * width + x) * 4;
                    let pixel = u32::from_ne_bytes(output[offset..offset + 4].try_into().unwrap());
                    assert_eq!(
                        pixel,
                        u32::from_be_bytes([
                            255,
                            (x * 7 / width) as u8 * 31,
                            (y * 5 / height) as u8 * 51,
                            42
                        ])
                    );
                }
            }
        }
    }
}
