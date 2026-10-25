//! Validated interference tuning. JSON and the editor cannot bypass range checks.
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::io::Read;

mod curve;
mod shader;
pub(crate) use curve::MIN_POINT_GAP;
pub use curve::{CurvePoint, GraceCurve};

/// One session seed, retained across tuning changes and shader recompilation.
pub fn fresh_seed() -> Result<u32> {
    let mut bytes = [0; 4];
    std::fs::File::open("/dev/urandom")
        .context("Opening animation entropy source")?
        .read_exact(&mut bytes)?;
    Ok(u32::from_ne_bytes(bytes))
}

/// Spatial/burst intensity, independent of the break deadline and test-card state.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct Glitchiness(f32);

impl Glitchiness {
    pub const ZERO: Self = Self(0.0);
    pub const MAXIMUM: Self = Self(100.0);
    pub fn value(self) -> f32 {
        self.0
    }
    pub(crate) fn quantized(self) -> Self {
        Self((self.0 * 2.0).floor() / 2.0)
    }
}

impl TryFrom<f32> for Glitchiness {
    type Error = anyhow::Error;
    fn try_from(value: f32) -> Result<Self> {
        ensure!(
            value.is_finite() && (0.0..=100.0).contains(&value),
            "Glitchiness must be 0–100"
        );
        Ok(Self(value))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EffectKind {
    Tears,
    Colour,
    Bands,
    Noise,
    Ghost,
}

impl EffectKind {
    pub const ALL: [Self; 5] = [
        Self::Tears,
        Self::Colour,
        Self::Bands,
        Self::Noise,
        Self::Ghost,
    ];
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "EffectInput")]
pub struct Effect {
    enabled: bool,
    entry: f32,
    maximum: f32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EffectInput {
    enabled: bool,
    entry: f32,
    maximum: f32,
}

impl TryFrom<EffectInput> for Effect {
    type Error = anyhow::Error;
    fn try_from(input: EffectInput) -> Result<Self> {
        Self::new(input.enabled, input.entry, input.maximum)
    }
}

impl Effect {
    pub fn new(enabled: bool, entry: f32, maximum: f32) -> Result<Self> {
        ensure!(
            entry.is_finite() && (0.0..=99.0).contains(&entry),
            "Entry must be 0–99"
        );
        ensure!(
            maximum.is_finite() && (0.0..=1.0).contains(&maximum),
            "Maximum must be 0–1"
        );
        Ok(Self {
            enabled,
            entry,
            maximum,
        })
    }

    pub fn enabled(self) -> bool {
        self.enabled
    }
    pub fn entry(self) -> f32 {
        self.entry
    }
    pub fn maximum(self) -> f32 {
        self.maximum
    }

    pub fn strength(self, level: Glitchiness) -> f32 {
        if !self.enabled {
            return 0.0;
        }
        let x = ((level.value() - self.entry) / (100.0 - self.entry)).clamp(0.0, 1.0);
        self.maximum * x * x * (3.0 - 2.0 * x)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    #[serde(default)]
    pub timing: crate::cycle::Timing,
    #[serde(default)]
    curve: GraceCurve,
    #[serde(default)]
    pub preview_image: Option<std::path::PathBuf>,
    pub overtime: crate::overtime::Overtime,
    tears: Effect,
    colour: Effect,
    bands: Effect,
    noise: Effect,
    ghost: Effect,
}

impl Default for Settings {
    fn default() -> Self {
        let effect = |entry, maximum| Effect {
            enabled: true,
            entry,
            maximum,
        };
        Self {
            timing: crate::cycle::Timing::default(),
            curve: GraceCurve::default(),
            preview_image: None,
            overtime: crate::overtime::Overtime::default(),
            tears: effect(5.0, 0.7),
            colour: effect(15.0, 0.65),
            bands: effect(30.0, 0.8),
            noise: effect(30.0, 0.45),
            ghost: effect(50.0, 0.55),
        }
    }
}

impl Settings {
    pub fn curve(&self) -> GraceCurve {
        self.curve
    }
    pub fn set_curve(&mut self, curve: [CurvePoint; 4]) -> Result<()> {
        self.curve = curve.try_into()?;
        Ok(())
    }

    pub fn effect(&self, kind: EffectKind) -> Effect {
        match kind {
            EffectKind::Tears => self.tears,
            EffectKind::Colour => self.colour,
            EffectKind::Bands => self.bands,
            EffectKind::Noise => self.noise,
            EffectKind::Ghost => self.ghost,
        }
    }

    pub fn set_effect(&mut self, kind: EffectKind, effect: Effect) {
        *match kind {
            EffectKind::Tears => &mut self.tears,
            EffectKind::Colour => &mut self.colour,
            EffectKind::Bands => &mut self.bands,
            EffectKind::Noise => &mut self.noise,
            EffectKind::Ghost => &mut self.ghost,
        } = effect;
    }

    /// Whether the current tuning produces visible interference at this level.
    pub fn has_visible_effect_at(&self, level: Glitchiness) -> bool {
        EffectKind::ALL
            .into_iter()
            .any(|kind| self.effect(kind).strength(level) > 0.0)
    }
}

#[cfg(test)]
mod tests;
