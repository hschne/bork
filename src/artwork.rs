//! Bounded image decoding and owned canvases shared by preview and overtime.
use anyhow::{Context, Result, ensure};
use std::{
    fs::File,
    io::{Cursor, Read},
    path::Path,
};

pub(crate) fn load_image(path: &Path) -> Result<image::RgbaImage> {
    let file = File::open(path).with_context(|| format!("Opening image {}", path.display()))?;
    const MAX_BYTES: u64 = 64 * 1024 * 1024;
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1).read_to_end(&mut bytes)?;
    ensure!(bytes.len() as u64 <= MAX_BYTES, "Image exceeds 64 MiB");
    let mut reader = image::ImageReader::new(Cursor::new(bytes)).with_guessed_format()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    reader
        .decode()
        .with_context(|| format!("Decoding image {}", path.display()))
        .map(|image| image.to_rgba8())
}

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

    /// Convert into a tightly packed Wayland ARGB8888 buffer at the canvas size.
    pub(crate) fn write_argb(&self, output: &mut [u8]) {
        for (source, pixel) in self
            .rgba
            .chunks_exact(4)
            .zip(output[..self.rgba.len()].chunks_exact_mut(4))
        {
            let colour = u32::from_be_bytes([source[3], source[0], source[1], source[2]]);
            pixel.copy_from_slice(&colour.to_ne_bytes());
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
    fn image_loading_is_shared_and_bounded() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("image.png");
        std::fs::write(&path, include_bytes!("../assets/preview-desktop.png")).unwrap();
        assert_eq!(load_image(&path).unwrap().dimensions(), (1600, 900));
        std::fs::write(&path, "not an image").unwrap();
        assert!(load_image(&path).is_err());
        File::create(&path)
            .unwrap()
            .set_len(64 * 1024 * 1024 + 1)
            .unwrap();
        assert!(
            load_image(&path)
                .unwrap_err()
                .to_string()
                .contains("64 MiB")
        );
    }

    #[test]
    fn argb_conversion_preserves_channels_and_buffer_padding() {
        let image = image::RgbaImage::from_fn(7, 5, |x, y| {
            image::Rgba([x as u8 * 31, y as u8 * 51, 42, 255])
        });
        let canvas = Canvas::from_image(image);
        let length = canvas.rgba().len();
        let mut output = vec![0xa5; length + 60];
        canvas.write_argb(&mut output);
        assert!(output[length..].iter().all(|byte| *byte == 0xa5));
        for y in 0..5 {
            for x in 0..7 {
                let offset = (y * 7 + x) * 4;
                let pixel = u32::from_ne_bytes(output[offset..offset + 4].try_into().unwrap());
                assert_eq!(
                    pixel,
                    u32::from_be_bytes([255, x as u8 * 31, y as u8 * 51, 42])
                );
            }
        }
    }
}
