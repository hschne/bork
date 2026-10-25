//! Monotone cubic interpolation through four user-controlled grace points.
use super::Glitchiness;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

pub const MIN_POINT_GAP: f32 = 0.01;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CurvePoint {
    pub progress: f32,
    pub intensity: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "[CurvePoint; 4]", into = "[CurvePoint; 4]")]
pub struct GraceCurve([CurvePoint; 4]);

impl Default for GraceCurve {
    fn default() -> Self {
        Self(
            [
                (0.0, 35.0),
                (1.0 / 3.0, 50.0),
                (2.0 / 3.0, 72.0),
                (1.0, 100.0),
            ]
            .map(|(progress, intensity)| CurvePoint {
                progress,
                intensity,
            }),
        )
    }
}

impl TryFrom<[CurvePoint; 4]> for GraceCurve {
    type Error = anyhow::Error;

    fn try_from(points: [CurvePoint; 4]) -> Result<Self> {
        for point in points {
            ensure!(
                point.progress.is_finite() && (0.0..=1.0).contains(&point.progress),
                "Curve progress must be 0–1"
            );
            ensure!(
                point.intensity.is_finite() && (0.0..=100.0).contains(&point.intensity),
                "Curve intensity must be 0–100"
            );
        }
        ensure!(
            points[0].progress == 0.0 && points[3].progress == 1.0,
            "Curve must span the whole grace interval"
        );
        ensure!(
            points[3].intensity == 100.0,
            "Curve must end at 100 intensity"
        );
        for pair in points.windows(2) {
            ensure!(
                pair[1].progress - pair[0].progress >= MIN_POINT_GAP - f32::EPSILON,
                "Curve points must be ordered and at least 1% apart"
            );
            ensure!(
                pair[1].intensity >= pair[0].intensity,
                "Curve intensity cannot decrease"
            );
        }
        Ok(Self(points))
    }
}

impl From<GraceCurve> for [CurvePoint; 4] {
    fn from(curve: GraceCurve) -> Self {
        curve.0
    }
}

impl GraceCurve {
    pub fn points(self) -> [CurvePoint; 4] {
        self.0
    }

    /// Shape-preserving Hermite tangents: flat intervals stay flat, never overshoot.
    pub(crate) fn level(self, progress: f32) -> Glitchiness {
        let points = self.0;
        let progress = if progress.is_nan() {
            0.0
        } else {
            progress.clamp(0.0, 1.0)
        };
        let x = f64::from(progress);
        let h: [f64; 3] = std::array::from_fn(|i| {
            f64::from(points[i + 1].progress) - f64::from(points[i].progress)
        });
        let d: [f64; 3] = std::array::from_fn(|i| {
            (f64::from(points[i + 1].intensity) - f64::from(points[i].intensity)) / h[i]
        });
        let mut slopes = [d[0], 0.0, 0.0, d[2]];
        for i in 1..3 {
            if d[i - 1] > 0.0 && d[i] > 0.0 {
                let a = 2.0 * h[i] + h[i - 1];
                let b = h[i] + 2.0 * h[i - 1];
                slopes[i] = (a + b) / (a / d[i - 1] + b / d[i]);
            }
        }
        for i in 0..3 {
            if d[i] == 0.0 {
                slopes[i] = 0.0;
                slopes[i + 1] = 0.0;
            } else {
                let magnitude = (slopes[i] / d[i]).hypot(slopes[i + 1] / d[i]);
                if magnitude > 3.0 {
                    slopes[i] *= 3.0 / magnitude;
                    slopes[i + 1] *= 3.0 / magnitude;
                }
            }
        }
        let i = if x <= f64::from(points[1].progress) {
            0
        } else if x <= f64::from(points[2].progress) {
            1
        } else {
            2
        };
        let t = (x - f64::from(points[i].progress)) / h[i];
        let start = f64::from(points[i].intensity);
        let end = f64::from(points[i + 1].intensity);
        let intensity = (2.0 * t.powi(3) - 3.0 * t.powi(2) + 1.0) * start
            + (t.powi(3) - 2.0 * t.powi(2) + t) * h[i] * slopes[i]
            + (-2.0 * t.powi(3) + 3.0 * t.powi(2)) * end
            + (t.powi(3) - t.powi(2)) * h[i] * slopes[i + 1];
        Glitchiness(intensity.clamp(start, end) as f32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_warns_immediately_and_interpolates_every_control_point() {
        let curve = GraceCurve::default();
        for point in curve.points() {
            assert_eq!(curve.level(point.progress).value(), point.intensity);
        }
        assert_eq!(curve.level(0.0).value(), 35.0);
        assert_eq!(curve.level(1.0).value(), 100.0);
        assert_eq!(curve.level(f32::NAN).value(), 35.0);
        assert_eq!(curve.level(-1.0).value(), 35.0);
        assert_eq!(curve.level(2.0).value(), 100.0);
        let encoded = serde_json::to_string(&curve).unwrap();
        assert_eq!(serde_json::from_str::<GraceCurve>(&encoded).unwrap(), curve);
    }

    #[test]
    fn interpolation_is_bounded_and_monotone_for_plateaus_and_uneven_spacing() {
        for positions in [
            [0.0, 0.01, 0.02, 1.0],
            [0.0, 0.98, 0.99, 1.0],
            [0.0, 0.33, 0.67, 1.0],
        ] {
            for levels in [
                [0.0, 0.0, 0.0, 100.0],
                [0.0, 100.0, 100.0, 100.0],
                [0.0, 0.0, 100.0, 100.0],
                [35.0, 50.0, 72.0, 100.0],
                [100.0; 4],
            ] {
                let points = std::array::from_fn(|i| CurvePoint {
                    progress: positions[i],
                    intensity: levels[i],
                });
                let curve = GraceCurve::try_from(points).unwrap();
                let mut previous = levels[0];
                for sample in 0..=10000 {
                    let value = curve.level(sample as f32 / 10000.0).value();
                    assert!(
                        value >= previous && value <= 100.0,
                        "{points:?}: {previous} -> {value}"
                    );
                    previous = value;
                }
            }
        }
    }

    #[test]
    fn constructors_and_deserialization_reject_invalid_curves() {
        let valid = GraceCurve::default().points();
        for (index, point) in [
            (
                0,
                CurvePoint {
                    progress: 0.1,
                    intensity: 35.0,
                },
            ),
            (
                1,
                CurvePoint {
                    progress: 0.0,
                    intensity: 50.0,
                },
            ),
            (
                1,
                CurvePoint {
                    progress: f32::NAN,
                    intensity: 50.0,
                },
            ),
            (
                1,
                CurvePoint {
                    progress: 0.33,
                    intensity: f32::INFINITY,
                },
            ),
            (
                1,
                CurvePoint {
                    progress: 0.33,
                    intensity: 101.0,
                },
            ),
            (
                1,
                CurvePoint {
                    progress: 0.33,
                    intensity: 10.0,
                },
            ),
            (
                3,
                CurvePoint {
                    progress: 1.0,
                    intensity: 99.0,
                },
            ),
        ] {
            let mut invalid = valid;
            invalid[index] = point;
            assert!(GraceCurve::try_from(invalid).is_err());
            assert!(
                serde_json::from_value::<GraceCurve>(serde_json::to_value(invalid).unwrap())
                    .is_err()
            );
        }
        for invalid in [
            serde_json::json!(1.3),
            serde_json::json!([]),
            serde_json::Value::Null,
        ] {
            assert!(serde_json::from_value::<GraceCurve>(invalid).is_err());
        }
    }
}
