//! CPU text composition over a shipped or user-selected background image.
use super::{Color, Fit, Horizontal, Offset, Overtime, Vertical};
use crate::artwork::Canvas;
use ab_glyph::{Font, FontArc, FontVec, Glyph, ScaleFont, point};
use anyhow::{Context, Result, ensure};
use image::{Rgba, RgbaImage, imageops};

pub struct Renderer {
    settings: Overtime,
    background: RgbaImage,
    fonts: Vec<FontArc>,
}

impl Renderer {
    pub fn load(settings: &Overtime) -> Result<Self> {
        let background = crate::artwork::load_image(&settings.background.image)?;
        let mut database = fontdb::Database::new();
        database.load_system_fonts();
        let fonts = settings.labels.iter().map(|label| {
            let family = label.font_family.as_deref().unwrap_or(&settings.font_family);
            let family = match family {
                "sans-serif" => fontdb::Family::SansSerif,
                "monospace" => fontdb::Family::Monospace,
                "serif" => fontdb::Family::Serif,
                name => fontdb::Family::Name(name),
            };
            let id = database.query(&fontdb::Query {
                families: &[family, fontdb::Family::SansSerif],
                ..fontdb::Query::default()
            }).context("No installed font matches the overtime labels; install a system sans-serif font or configure font_family")?;
            database.with_face_data(id, |bytes, index| {
                FontVec::try_from_vec_and_index(bytes.to_vec(), index).map(FontArc::new)
            }).context("Reading installed overtime font")?
                .context("Decoding installed overtime font")
        }).collect::<Result<Vec<_>>>()?;
        Ok(Self {
            settings: settings.clone(),
            background,
            fonts,
        })
    }

    pub fn settings(&self) -> &Overtime {
        &self.settings
    }

    pub fn background(&self, width: u32, height: u32) -> Result<RgbaImage> {
        ensure!(
            width > 0 && height > 0 && u64::from(width) * u64::from(height) <= 33_554_432,
            "Unsupported overtime dimensions {width}x{height}"
        );
        let color = self.settings.background.color.0;
        let mut canvas =
            RgbaImage::from_pixel(width, height, Rgba([color[0], color[1], color[2], 255]));
        let image = match self.settings.background.fit {
            Fit::Cover => image::DynamicImage::ImageRgba8(self.background.clone())
                .resize_to_fill(width, height, imageops::FilterType::Triangle)
                .to_rgba8(),
            Fit::Contain => image::DynamicImage::ImageRgba8(self.background.clone())
                .resize(width, height, imageops::FilterType::Triangle)
                .to_rgba8(),
            Fit::Stretch => imageops::resize(
                &self.background,
                width,
                height,
                imageops::FilterType::Triangle,
            ),
        };
        let x = (width - image.width()) / 2;
        let y = (height - image.height()) / 2;
        imageops::overlay(&mut canvas, &image, i64::from(x), i64::from(y));
        Ok(canvas)
    }

    pub fn render(&self, width: u32, height: u32, remaining: u64, total: u64) -> Result<Canvas> {
        let background = self.background(width, height)?;
        Ok(self.paint(&background, remaining, total))
    }

    /// Backgrounds are prepared once per output; countdown ticks only copy and paint labels.
    pub fn paint(&self, background: &RgbaImage, remaining: u64, total: u64) -> Canvas {
        let mut image = background.clone();
        let width = image.width() as f32;
        let height = image.height() as f32;
        let scale = height / 720.0;
        let format_time = |seconds: u64| format!("{:02}:{:02}", seconds / 60, seconds % 60);
        let remaining = format_time(remaining.min(total));
        let total = format_time(total);
        for (label, font) in self.settings.labels.iter().zip(&self.fonts) {
            let text = label
                .text
                .replace("{remaining}", &remaining)
                .replace("{total}", &total);
            let size = label.font_size.unwrap_or(self.settings.font_size).0 * scale;
            let scaled = font.as_scaled(
                size * font.height_unscaled()
                    / font.units_per_em().unwrap_or(font.height_unscaled()),
            );
            let mut glyphs = Vec::new();
            let mut cursor = 0.0;
            let mut previous = None;
            for character in text.chars() {
                let id = scaled.glyph_id(character);
                if let Some(previous) = previous {
                    cursor += scaled.kern(previous, id);
                }
                glyphs.push(Glyph {
                    id,
                    scale: scaled.scale(),
                    position: point(cursor, 0.0),
                });
                cursor += scaled.h_advance(id);
                previous = Some(id);
            }
            let halign = label.halign.unwrap_or(self.settings.halign);
            let valign = label.valign.unwrap_or(self.settings.valign);
            let x = match halign {
                Horizontal::Left => 0.0,
                Horizontal::Center => width / 2.0,
                Horizontal::Right => width,
            };
            let y = match valign {
                Vertical::Top => 0.0,
                Vertical::Center => height / 2.0,
                Vertical::Bottom => height,
            };
            let offset = |position: Offset, extent: f32| match position {
                Offset::Pixels(pixels) => pixels * scale,
                Offset::Percent(percent) => extent * percent / 100.0,
            };
            let x = x + offset(label.position[0], width)
                - match halign {
                    Horizontal::Left => 0.0,
                    Horizontal::Center => cursor / 2.0,
                    Horizontal::Right => cursor,
                };
            let text_height = scaled.ascent() - scaled.descent();
            let y = y
                - offset(label.position[1], height)
                - match valign {
                    Vertical::Top => 0.0,
                    Vertical::Center => text_height / 2.0,
                    Vertical::Bottom => text_height,
                }
                + scaled.ascent();
            let color = label.color.unwrap_or(self.settings.color);
            for mut glyph in glyphs {
                glyph.position.x += x;
                glyph.position.y += y;
                if let Some(outline) = font.outline_glyph(glyph) {
                    let bounds = outline.px_bounds();
                    if bounds.max.x < 0.0
                        || bounds.max.y < 0.0
                        || bounds.min.x >= width
                        || bounds.min.y >= height
                    {
                        continue;
                    }
                    outline.draw(|gx, gy, coverage| {
                        let x = bounds.min.x as i32 + gx as i32;
                        let y = bounds.min.y as i32 + gy as i32;
                        if x >= 0 && y >= 0 && x < image.width() as i32 && y < image.height() as i32
                        {
                            blend(image.get_pixel_mut(x as u32, y as u32), color, coverage);
                        }
                    });
                }
            }
        }
        Canvas::from_image(image)
    }
}

fn blend(pixel: &mut Rgba<u8>, color: Color, coverage: f32) {
    let alpha = coverage * f32::from(color.0[3]) / 255.0;
    for (destination, source) in pixel.0.iter_mut().zip(color.0).take(3) {
        *destination =
            (f32::from(*destination) * (1.0 - alpha) + f32::from(source) * alpha).round() as u8;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::overtime::{FontSize, Label};
    use std::fs;

    #[test]
    fn shipped_background_has_no_text_and_countdown_uses_the_actual_total() {
        let renderer = Renderer::load(&Overtime::default()).unwrap();
        let background = renderer.background(1280, 720).unwrap();
        for y in 200..470 {
            for x in 230..1050 {
                assert_eq!(*background.get_pixel(x, y), Rgba([36, 36, 36, 255]));
            }
        }
        let first = renderer.paint(&background, 272, 300);
        let next = renderer.paint(&background, 271, 300);
        let longer = renderer.paint(&background, 272, 600);
        assert_ne!(first.rgba(), next.rgba());
        assert_ne!(first.rgba(), longer.rgba());
        assert_eq!(first.rgba().len(), 1280 * 720 * 4);
        assert!(renderer.background(0, 720).is_err());
    }

    #[test]
    fn text_positions_alignment_and_inherited_color_are_rendered() {
        let mut renderer = Renderer::load(&Overtime::default()).unwrap();
        renderer.settings.font_size = FontSize(24.0);
        renderer.settings.color = Color([255, 0, 0, 255]);
        renderer.settings.halign = Horizontal::Left;
        renderer.settings.valign = Vertical::Top;
        renderer.settings.labels = vec![Label {
            text: "X".into(),
            position: [Offset::Pixels(20.0), Offset::Pixels(-20.0)],
            ..Label::default()
        }];
        renderer.fonts.truncate(1);
        let background = RgbaImage::from_pixel(1280, 720, Rgba([0, 0, 0, 255]));
        let image = renderer.paint(&background, 300, 300);
        let colored: Vec<_> = image
            .rgba()
            .chunks_exact(4)
            .enumerate()
            .filter(|(_, pixel)| pixel[0] > 0)
            .map(|(i, _)| (i % 1280, i / 1280))
            .collect();
        assert!(!colored.is_empty());
        assert!(
            colored
                .iter()
                .all(|&(x, y)| (20..55).contains(&x) && (20..65).contains(&y))
        );
        assert!(
            image
                .rgba()
                .chunks_exact(4)
                .all(|pixel| pixel[1] == 0 && pixel[2] == 0)
        );
    }

    #[test]
    fn image_fit_can_cover_contain_or_stretch_without_cropping_labels() {
        let mut renderer = Renderer::load(&Overtime::default()).unwrap();
        renderer.background = RgbaImage::from_pixel(40, 20, Rgba([255, 0, 0, 255]));
        renderer.settings.background.color = Color([0, 0, 0, 255]);
        renderer.settings.background.fit = Fit::Contain;
        let image = renderer.background(100, 100).unwrap();
        assert_eq!(*image.get_pixel(0, 0), Rgba([0, 0, 0, 255]));
        assert_eq!(*image.get_pixel(50, 50), Rgba([255, 0, 0, 255]));
        for fit in [Fit::Cover, Fit::Stretch] {
            renderer.settings.background.fit = fit;
            assert!(
                renderer
                    .background(100, 100)
                    .unwrap()
                    .pixels()
                    .all(|pixel| *pixel == Rgba([255, 0, 0, 255]))
            );
        }
    }

    #[test]
    fn missing_images_are_not_generated_or_installed() {
        let directory = tempfile::tempdir().unwrap();
        let mut settings = Overtime::default();
        settings.background.image = directory.path().join("overtime.png");
        assert!(Renderer::load(&settings).is_err());
        assert!(!settings.background.image.exists());
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 0);
    }

    #[test]
    fn unavailable_requested_family_falls_back_to_an_installed_sans_serif() {
        let settings = Overtime {
            font_family: "Bork nonexistent font".into(),
            ..Overtime::default()
        };
        let renderer = Renderer::load(&settings).unwrap();
        assert_eq!(renderer.fonts.len(), settings.labels.len());
    }
}
