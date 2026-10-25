//! Config-only overtime appearance. Missing fields inherit defaults, never null sentinels.
use anyhow::{Result, bail, ensure};
use serde::{Deserialize, Deserializer, Serialize};
use std::path::PathBuf;

mod render;
pub use render::Renderer;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Overtime {
    pub background: Background,
    pub font_family: String,
    pub font_size: FontSize,
    pub color: Color,
    pub halign: Horizontal,
    pub valign: Vertical,
    #[serde(deserialize_with = "labels")]
    pub labels: Vec<Label>,
}

impl Default for Overtime {
    fn default() -> Self {
        Self {
            background: Background::default(),
            font_family: "sans-serif".into(),
            font_size: FontSize::default(),
            color: Color([238, 238, 238, 255]),
            halign: Horizontal::Center,
            valign: Vertical::Center,
            labels: [
                ("TRANSMISSION PAUSED", 18.0, 13.5),
                ("TAKE A BREAK", 62.0, 4.5),
                ("{remaining} / {total}", 34.0, -5.0),
            ]
            .map(|(text, size, y)| Label {
                text: text.into(),
                font_size: Some(FontSize(size)),
                position: [Offset::Pixels(0.0), Offset::Percent(y)],
                ..Label::default()
            })
            .into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Background {
    pub image: PathBuf,
    pub fit: Fit,
    pub color: Color,
}

impl Default for Background {
    fn default() -> Self {
        Self {
            image: default_image(),
            fit: Fit::Cover,
            color: Color([24, 24, 24, 255]),
        }
    }
}

pub fn default_image() -> PathBuf {
    let directory = if cfg!(debug_assertions) {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    } else {
        std::env::current_exe()
            .expect("Executable path is available")
            .parent()
            .expect("Executable has a parent directory")
            .to_owned()
    };
    directory.join("assets/overtime.png")
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Fit {
    Cover,
    Contain,
    Stretch,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Horizontal {
    Left,
    #[default]
    Center,
    Right,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Vertical {
    Top,
    #[default]
    Center,
    Bottom,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Label {
    #[serde(deserialize_with = "text")]
    pub text: String,
    pub position: [Offset; 2],
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "override_value"
    )]
    pub font_family: Option<String>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "override_value"
    )]
    pub font_size: Option<FontSize>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "override_value"
    )]
    pub color: Option<Color>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "override_value"
    )]
    pub halign: Option<Horizontal>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "override_value"
    )]
    pub valign: Option<Vertical>,
}

fn override_value<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
    deserializer: D,
) -> std::result::Result<Option<T>, D::Error> {
    T::deserialize(deserializer).map(Some)
}

fn text<'de, D: Deserializer<'de>>(deserializer: D) -> std::result::Result<String, D::Error> {
    let text = String::deserialize(deserializer)?;
    if text.len() > 1024 || text.contains(['\n', '\r']) {
        return Err(serde::de::Error::custom(
            "Label text must be a single line of at most 1024 bytes",
        ));
    }
    Ok(text)
}

fn labels<'de, D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Vec<Label>, D::Error> {
    let labels = Vec::<Label>::deserialize(deserializer)?;
    if labels.len() > 32 {
        return Err(serde::de::Error::custom(
            "At most 32 overtime labels are supported",
        ));
    }
    Ok(labels)
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "f32", into = "f32")]
pub struct FontSize(f32);
impl Default for FontSize {
    fn default() -> Self {
        Self(24.0)
    }
}
impl TryFrom<f32> for FontSize {
    type Error = anyhow::Error;
    fn try_from(value: f32) -> Result<Self> {
        ensure!(
            value.is_finite() && (1.0..=256.0).contains(&value),
            "Font size must be 1–256"
        );
        Ok(Self(value))
    }
}
impl From<FontSize> for f32 {
    fn from(value: FontSize) -> Self {
        value.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Color([u8; 4]);
impl TryFrom<String> for Color {
    type Error = anyhow::Error;
    fn try_from(value: String) -> Result<Self> {
        let value = value.strip_prefix('#').unwrap_or(&value);
        ensure!(
            matches!(value.len(), 6 | 8) && value.bytes().all(|byte| byte.is_ascii_hexdigit()),
            "Color must be #RRGGBB or #RRGGBBAA"
        );
        let mut channels = [255; 4];
        for (i, channel) in channels.iter_mut().take(value.len() / 2).enumerate() {
            *channel = u8::from_str_radix(&value[i * 2..i * 2 + 2], 16)?;
        }
        Ok(Self(channels))
    }
}
impl From<Color> for String {
    fn from(value: Color) -> Self {
        let [r, g, b, a] = value.0;
        if a == 255 {
            format!("#{r:02x}{g:02x}{b:02x}")
        } else {
            format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "OffsetValue", into = "OffsetValue")]
pub enum Offset {
    Pixels(f32),
    Percent(f32),
}
impl Default for Offset {
    fn default() -> Self {
        Self::Pixels(0.0)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum OffsetValue {
    Pixels(f32),
    Percent(String),
}
impl TryFrom<OffsetValue> for Offset {
    type Error = anyhow::Error;
    fn try_from(value: OffsetValue) -> Result<Self> {
        let offset = match value {
            OffsetValue::Pixels(value) if value.is_finite() && value.abs() <= 32768.0 => {
                Self::Pixels(value)
            }
            OffsetValue::Percent(value) => {
                let number: f32 = value
                    .strip_suffix('%')
                    .ok_or_else(|| anyhow::anyhow!("Position strings must end in %"))?
                    .parse()?;
                ensure!(
                    number.is_finite() && number.abs() <= 100.0,
                    "Percentage positions must be -100%–100%"
                );
                Self::Percent(number)
            }
            _ => bail!("Pixel positions must be finite and within ±32768"),
        };
        Ok(offset)
    }
}
impl From<Offset> for OffsetValue {
    fn from(value: Offset) -> Self {
        match value {
            Offset::Pixels(value) => Self::Pixels(value),
            Offset::Percent(value) => Self::Percent(format!("{value}%")),
        }
    }
}

#[cfg(test)]
mod tests;
