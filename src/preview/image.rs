//! User-selected screenshots are decoded off the UI thread with bounded resources.
use crate::artwork::Canvas;
use anyhow::{Context, Result, ensure};
use std::{
    fs::File,
    io::{Cursor, Read},
    path::Path,
};

pub(super) fn load(path: &Path) -> Result<Canvas> {
    let file = File::open(path).with_context(|| format!("Opening {}", path.display()))?;
    const MAX_BYTES: u64 = 64 * 1024 * 1024;
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= MAX_BYTES,
        "Preview image exceeds 64 MiB"
    );
    let mut reader = ::image::ImageReader::new(Cursor::new(bytes)).with_guessed_format()?;
    let mut limits = ::image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    let image = reader.decode().context("Decoding preview image")?;
    let image = if image.width() > 4096 || image.height() > 4096 {
        image.thumbnail(4096, 4096)
    } else {
        image
    };
    Ok(Canvas::from_image(image.to_rgba8()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_screenshots_and_rejects_invalid_or_oversized_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("screenshot.png");
        std::fs::write(&path, include_bytes!("../../assets/preview-desktop.png")).unwrap();
        let canvas = load(&path).unwrap();
        assert_eq!((canvas.width(), canvas.height()), (1600, 900));
        std::fs::write(&path, "not an image").unwrap();
        assert!(load(&path).is_err());
        File::create(&path)
            .unwrap()
            .set_len(64 * 1024 * 1024 + 1)
            .unwrap();
        assert!(load(&path).err().unwrap().to_string().contains("64 MiB"));
    }
}
