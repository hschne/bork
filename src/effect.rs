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

    pub fn strength(self, level: Glitchiness) -> f32 {
        if !self.enabled {
            return 0.0;
        }
        let x = ((level.value() - self.entry) / (100.0 - self.entry)).clamp(0.0, 1.0);
        self.maximum * x * x * (3.0 - 2.0 * x)
    }
}

/// The five independent interference channels, shared by desktop and preview shaders.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Interference {
    pub tears: Effect,
    pub colour: Effect,
    pub bands: Effect,
    pub noise: Effect,
    pub ghost: Effect,
}

impl Default for Interference {
    fn default() -> Self {
        let effect = |entry, maximum| Effect {
            enabled: true,
            entry,
            maximum,
        };
        Self {
            tears: effect(5.0, 0.7),
            colour: effect(15.0, 0.65),
            bands: effect(30.0, 0.8),
            noise: effect(30.0, 0.45),
            ghost: effect(50.0, 0.55),
        }
    }
}

impl Interference {
    pub fn has_visible_effect_at(&self, level: Glitchiness) -> bool {
        [self.tears, self.colour, self.bands, self.noise, self.ghost]
            .into_iter()
            .any(|effect| effect.strength(level) > 0.0)
    }
}

#[cfg(test)]
mod tests;
