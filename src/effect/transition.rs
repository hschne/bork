//! Bounded final signal loss, shared by the desktop and preview adapters.

/// The disruption consumes no extra break debt and never delays cancellation.
pub const SIGNAL_LOSS_SECONDS: f32 = 1.14;

#[derive(Clone, Copy, Debug)]
pub struct SignalLoss {
    seed: u32,
    holds: [[f32; 2]; 3],
}

impl SignalLoss {
    pub fn new(seed: u32) -> Self {
        let random = |salt| hash(seed ^ salt) as f64 / u32::MAX as f64;
        let first = 0.19 + random(1) as f32 * 0.07;
        let second = first + 0.04 + random(2) as f32 * 0.04;
        let third = second + 0.15 + random(3) as f32 * 0.05;
        let fourth = third + 0.025 + random(4) as f32 * 0.035;
        Self {
            seed,
            holds: [[0.0, first], [second, third], [fourth, 0.70]],
        }
    }

    /// Repeatable scrubbing within one run; the next run receives another seed.
    pub fn next(self) -> Self {
        Self::new(self.seed.wrapping_add(0x9e3779b9))
    }

    pub fn shader_body(self) -> String {
        let [[a, b], [c, d], [e, f]] = self.holds;
        format!(
            "const uint LOSS_SEED = {}u;\nconst vec2 HOLD_A = vec2({a:.6}, {b:.6});\nconst vec2 HOLD_B = vec2({c:.6}, {d:.6});\nconst vec2 HOLD_C = vec2({e:.6}, {f:.6});\n{}",
            self.seed,
            include_str!("signal-loss.glsl")
        )
    }

    pub fn hyprland_shader(self) -> String {
        format!(
            "#version 300 es\nprecision highp float;\nprecision highp int;\nin vec2 v_texcoord;\nlayout(location = 0) out vec4 fragColor;\nuniform sampler2D tex;\nuniform float time;\nuniform int wl_output;\nvec4 sampleDesktop(vec2 uv) {{ return texture(tex, clamp(uv, 0.0, 1.0)); }}\n{}\nvoid main() {{ fragColor = bork(v_texcoord, time, uint(wl_output)); }}\n",
            self.shader_body()
        )
    }
}

fn hash(mut value: u32) -> u32 {
    value ^= value >> 16;
    value = value.wrapping_mul(0x7feb352d);
    value ^= value >> 15;
    value = value.wrapping_mul(0x846ca68b);
    value ^ (value >> 16)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn randomized_holds_are_ordered_bounded_and_repeatable() {
        for seed in 0..1000 {
            let loss = SignalLoss::new(seed);
            let [[a, b], [c, d], [e, f]] = loss.holds;
            assert_eq!(a, 0.0);
            assert!(a < b && b < c && c < d && d < e && e < f);
            assert_eq!(f, 0.70);
            assert_eq!(loss.shader_body(), SignalLoss::new(seed).shader_body());
            assert_ne!(loss.shader_body(), loss.next().shader_body());
        }
    }

    #[test]
    fn desktop_wraps_the_exact_preview_implementation() {
        let loss = SignalLoss::new(41);
        assert!(loss.hyprland_shader().contains(&loss.shader_body()));
    }
}
