use anyhow::{Context, Result, ensure};
use std::io::Read;

/// One session seed, retained across tuning changes and shader recompilation.
pub fn fresh_seed() -> Result<u32> {
    let mut bytes = [0; 4];
    std::fs::File::open("/dev/urandom")
        .context("Opening animation entropy source")?
        .read_exact(&mut bytes)?;
    Ok(u32::from_ne_bytes(bytes))
}
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Effect {
    pub enabled: bool,
    pub entry: f32,
    pub maximum: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub curve: f32,
    pub tears: Effect,
    pub colour: Effect,
    pub bands: Effect,
    pub noise: Effect,
    pub ghost: Effect,
}

impl Default for Settings {
    fn default() -> Self {
        fn effect(entry: f32, maximum: f32) -> Effect {
            Effect {
                enabled: true,
                entry,
                maximum,
            }
        }
        Self {
            curve: 2.0,
            tears: effect(5.0, 0.7),
            colour: effect(15.0, 0.65),
            bands: effect(30.0, 0.8),
            noise: effect(30.0, 0.45),
            ghost: effect(50.0, 0.55),
        }
    }
}

impl Effect {
    pub fn strength(&self, level: f32) -> f32 {
        if !self.enabled || !level.is_finite() {
            return 0.0;
        }
        let x = ((level - self.entry) / (100.0 - self.entry)).clamp(0.0, 1.0);
        self.maximum * x * x * (3.0 - 2.0 * x)
    }
}

impl Settings {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.curve.is_finite() && (1.01..=5.0).contains(&self.curve),
            "Curve must be 1.01–5"
        );
        for e in [
            &self.tears,
            &self.colour,
            &self.bands,
            &self.noise,
            &self.ghost,
        ] {
            ensure!(
                e.entry.is_finite() && (0.0..=99.0).contains(&e.entry),
                "Entry must be 0–99"
            );
            ensure!(
                e.maximum.is_finite() && (0.0..=1.0).contains(&e.maximum),
                "Maximum must be 0–1"
            );
        }
        Ok(())
    }

    /// Shared effect implementation; adapters supply sampleDesktop(), UVs, time and seed.
    pub fn shader_body(&self, level: f32) -> Result<String> {
        self.validate()?;
        ensure!(
            level.is_finite() && (0.0..=100.0).contains(&level),
            "Glitchiness must be 0–100"
        );
        let normalized = level / 100.0;
        Ok(format!(
            "const float TEARS = {:.8};\nconst float COLOUR = {:.8};\nconst float BANDS = {:.8};\nconst float NOISE = {:.8};\nconst float GHOST = {:.8};\nconst float FREQUENCY = {:.8};\nconst float DURATION = {:.8};\n{}",
            self.tears.strength(level),
            self.colour.strength(level),
            self.bands.strength(level),
            self.noise.strength(level),
            self.ghost.strength(level),
            0.12 + 0.73 * normalized.powi(2),
            0.08 + 1.02 * normalized.powf(1.2),
            include_str!("signal.glsl")
        ))
    }

    pub fn hyprland_shader(&self, level: f32) -> Result<String> {
        self.hyprland_shader_at(level, 0.0, 0)
    }

    /// Preserve animation continuity when Hyprland resets its time uniform on reload.
    pub fn hyprland_shader_at(&self, level: f32, elapsed: f32, seed: u32) -> Result<String> {
        ensure!(
            elapsed.is_finite() && elapsed >= 0.0,
            "Invalid animation time"
        );
        Ok(format!(
            "#version 300 es\nprecision highp float;\nprecision highp int;\nin vec2 v_texcoord;\nlayout(location = 0) out vec4 fragColor;\nuniform sampler2D tex;\nuniform float time;\nuniform int wl_output;\nvec4 sampleDesktop(vec2 uv) {{ return texture(tex, clamp(uv, 0.0, 1.0)); }}\n{}\nvoid main() {{ fragColor = signalLoss(v_texcoord, time + {elapsed:.4}, {seed}u ^ uint(wl_output)); }}\n",
            self.shader_body(level)?
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn actual_shader_constants_raise_burst_frequency_and_duration_separately() {
        let settings = Settings::default();
        let mut previous = [0.0, 0.0];
        for level in 0..=100 {
            let body = settings.shader_body(level as f32).unwrap();
            let mut current = [0.0; 2];
            for (i, name) in ["FREQUENCY", "DURATION"].iter().enumerate() {
                let prefix = format!("const float {name} = ");
                current[i] = body
                    .lines()
                    .find_map(|line| line.strip_prefix(&prefix))
                    .unwrap()
                    .trim_end_matches(';')
                    .parse::<f32>()
                    .unwrap();
                assert!(current[i] >= previous[i]);
            }
            assert!(current[0] * current[1] > 0.0 && current[0] * current[1] < 1.0);
            previous = current;
        }
        assert!((previous[0] - 0.85).abs() < 0.000001);
        assert!((previous[1] - 1.1).abs() < 0.000001);
    }

    #[test]
    fn seeded_adapter_preserves_time_and_monitor_identity() {
        let settings = Settings::default();
        let shader = settings.hyprland_shader_at(75.0, 123.5, u32::MAX).unwrap();
        assert!(shader.contains("uniform int wl_output;"));
        assert!(shader.contains("time + 123.5000, 4294967295u ^ uint(wl_output)"));
        assert_eq!(
            shader,
            settings.hyprland_shader_at(75.0, 123.5, u32::MAX).unwrap()
        );
        assert_ne!(
            shader,
            settings.hyprland_shader_at(75.0, 123.5, 17).unwrap()
        );
        assert!(settings.hyprland_shader_at(75.0, f32::INFINITY, 0).is_err());
        assert!(settings.hyprland_shader_at(75.0, -1.0, 0).is_err());
        fresh_seed().unwrap();
    }

    #[test]
    fn bounded_smooth_monotonic_and_toggle() {
        let mut s = Settings::default();
        s.validate().unwrap();
        for e in [&s.tears, &s.colour, &s.bands, &s.noise, &s.ghost] {
            let mut previous = 0.0;
            for i in 0..=10000 {
                let v = e.strength(i as f32 / 100.0);
                assert!(v >= previous && v <= e.maximum);
                previous = v;
            }
            assert_eq!(e.strength(e.entry), 0.0);
            assert!(e.strength(e.entry + 0.001) < 0.000001);
        }
        s.tears.enabled = false;
        assert_eq!(s.tears.strength(100.0), 0.0);
        assert!(s.hyprland_shader(100.0).unwrap().contains("signalLoss"));
        assert!(s.hyprland_shader(f32::NAN).is_err());
        s.curve = f32::INFINITY;
        assert!(s.validate().is_err());
    }
}
