//! GLSL adapters share the same embedded interference implementation.
use super::{Glitchiness, Settings};
use anyhow::{Result, ensure};

impl Settings {
    /// Shared effect implementation; adapters supply sampleDesktop(), UVs, time and seed.
    pub fn shader_body(&self, level: Glitchiness) -> String {
        let normalized = level.value() / 100.0;
        format!(
            "const float TEARS = {:.8};\nconst float COLOUR = {:.8};\nconst float BANDS = {:.8};\nconst float NOISE = {:.8};\nconst float GHOST = {:.8};\nconst float FREQUENCY = {:.8};\nconst float DURATION = {:.8};\n{}",
            self.tears.strength(level) * self.strength(),
            self.colour.strength(level) * self.strength(),
            self.bands.strength(level) * self.strength(),
            self.noise.strength(level) * self.strength(),
            self.ghost.strength(level) * self.strength(),
            0.12 + 0.73 * normalized.powi(2),
            0.08 + 1.02 * normalized.powf(1.2),
            include_str!("../signal.glsl")
        )
    }

    pub fn hyprland_shader(&self, level: Glitchiness) -> Result<String> {
        self.hyprland_shader_at(level, 0.0, 0)
    }

    /// Preserve animation continuity when Hyprland resets its time uniform on reload.
    pub fn hyprland_shader_at(
        &self,
        level: Glitchiness,
        elapsed: f32,
        seed: u32,
    ) -> Result<String> {
        ensure!(
            elapsed.is_finite() && elapsed >= 0.0,
            "Invalid animation time"
        );
        Ok(format!(
            "#version 300 es\nprecision highp float;\nprecision highp int;\nin vec2 v_texcoord;\nlayout(location = 0) out vec4 fragColor;\nuniform sampler2D tex;\nuniform float time;\nuniform int wl_output;\nvec4 sampleDesktop(vec2 uv) {{ return texture(tex, clamp(uv, 0.0, 1.0)); }}\n{}\nvoid main() {{ fragColor = bork(v_texcoord, time + {elapsed:.4}, {seed}u ^ uint(wl_output)); }}\n",
            self.shader_body(level)
        ))
    }
}
