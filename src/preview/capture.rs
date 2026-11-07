//! Private screenshot verification adapter; no product controls or animation state.
use crate::effect::GraceCurve;
use anyhow::Result;
use eframe::egui;
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

pub(super) struct Capture {
    path: Option<PathBuf>,
    started: Instant,
    requested: bool,
    manual: bool,
    layout: String,
}

// Record actual widget bounds only when the private capture adapter is active.
pub(super) fn target(name: &'static str, response: &egui::Response) {
    if response.ctx.data(|data| {
        data.get_temp::<bool>(egui::Id::new("bork-test-capture"))
            .unwrap_or(false)
    }) {
        let value = (response.rect, response.has_focus());
        response.ctx.data_mut(|data| {
            data.insert_temp(egui::Id::new(("bork-test-target", name)), value);
        });
    }
}

impl Capture {
    pub(super) fn new(ctx: &egui::Context) -> Self {
        let path = std::env::var_os("BORK_TEST_CAPTURE").map(PathBuf::from);
        ctx.data_mut(|data| data.insert_temp(egui::Id::new("bork-test-capture"), path.is_some()));
        Self {
            path,
            started: Instant::now(),
            requested: false,
            manual: std::env::var_os("BORK_TEST_CAPTURE_MANUAL").is_some(),
            layout: String::new(),
        }
    }
    pub(super) fn update(
        &mut self,
        ctx: &egui::Context,
        level: f32,
        curve: GraceCurve,
        test_card: bool,
        signal_loss: Option<f32>,
    ) {
        if let Some(path) = self.path.as_ref() {
            if self.manual && ctx.cumulative_frame_nr() >= 2 {
                let mut layout = String::new();
                for name in ["curve-start", "save", "timeline", "live-preview"] {
                    let Some((rect, focused)) = ctx.data(|data| {
                        data.get_temp::<(egui::Rect, bool)>(egui::Id::new((
                            "bork-test-target",
                            name,
                        )))
                    }) else {
                        return;
                    };
                    let center = rect.center() * ctx.pixels_per_point();
                    layout.push_str(&format!(
                        "{name} {:.0} {:.0} {}\n",
                        center.x,
                        center.y,
                        if focused { "focused" } else { "idle" }
                    ));
                }
                layout.push_str(&format!("intensity {}\n", curve.points()[0].intensity));
                layout.push_str(&format!("test-card {test_card}\n"));
                layout.push_str(&format!(
                    "signal-loss {}\n",
                    signal_loss.map_or_else(|| "none".into(), |seconds| format!("{seconds:.4}"))
                ));
                if layout != self.layout {
                    let result = std::fs::write(path.with_extension("ready.tmp"), &layout)
                        .and_then(|()| {
                            std::fs::rename(
                                path.with_extension("ready.tmp"),
                                path.with_extension("ready"),
                            )
                        });
                    if let Err(error) = result {
                        eprintln!("Capture readiness failed: {error}");
                    }
                    self.layout = layout;
                }
            }
            if !self.requested {
                let delay = if self.manual {
                    Duration::from_millis(50)
                } else {
                    Duration::from_millis(1500).saturating_sub(self.started.elapsed())
                };
                if (self.manual && path.with_extension("capture").exists())
                    || (!self.manual && delay.is_zero())
                {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(
                        egui::UserData::default(),
                    ));
                    self.requested = true;
                } else {
                    // Capture must wake even when a static scene stops repainting.
                    ctx.request_repaint_after(delay);
                }
            }
            let image = ctx.input(|input| {
                input.events.iter().find_map(|event| {
                    if let egui::Event::Screenshot { image, .. } = event {
                        Some(image.clone())
                    } else {
                        None
                    }
                })
            });
            if let Some(image) = image {
                let result = (|| -> Result<()> {
                    image::save_buffer_with_format(
                        path,
                        image.as_raw(),
                        image.width() as u32,
                        image.height() as u32,
                        image::ColorType::Rgba8,
                        image::ImageFormat::Png,
                    )?;
                    std::fs::write(
                        path.with_extension("json"),
                        serde_json::to_vec_pretty(&serde_json::json!({
                            "level": level, "curve": curve, "test_card": test_card,
                            "signal_loss": signal_loss,
                        }))?,
                    )?;
                    Ok(())
                })();
                if let Err(error) = result {
                    eprintln!("Capture failed: {error:#}");
                }
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_capture_reports_targets_and_waits_for_the_request_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("preview.png");
        let ctx = egui::Context::default();
        ctx.data_mut(|data| data.insert_temp(egui::Id::new("bork-test-capture"), true));
        let mut capture = Capture {
            path: Some(path.clone()),
            started: Instant::now() - Duration::from_secs(10),
            requested: false,
            manual: true,
            layout: String::new(),
        };
        for _ in 0..4 {
            let _ = ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    for name in ["curve-start", "save", "timeline", "live-preview"] {
                        target(name, &ui.button(name));
                    }
                });
                capture.update(ctx, 0.0, GraceCurve::default(), false, None);
            });
        }
        let ready = std::fs::read_to_string(path.with_extension("ready")).unwrap();
        assert!(ready.contains("curve-start "));
        assert!(ready.contains("intensity 35\n"));
        assert!(!capture.requested);
        std::fs::write(path.with_extension("capture"), "").unwrap();
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            capture.update(ctx, 0.0, GraceCurve::default(), false, None);
        });
        assert!(capture.requested);
        assert!(
            output.viewport_output[&egui::ViewportId::ROOT]
                .commands
                .iter()
                .any(|command| matches!(command, egui::ViewportCommand::Screenshot(_)))
        );
    }

    #[test]
    fn screenshot_event_writes_rgba_png_and_metadata_then_closes() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("preview.png");
        let ctx = egui::Context::default();
        let mut capture = Capture {
            path: Some(path.clone()),
            started: Instant::now(),
            requested: true,
            manual: false,
            layout: String::new(),
        };
        let pixels = [255, 0, 0, 255, 0, 128, 255, 255];
        let input = egui::RawInput {
            events: vec![egui::Event::Screenshot {
                viewport_id: egui::ViewportId::ROOT,
                user_data: egui::UserData::default(),
                image: std::sync::Arc::new(egui::ColorImage::from_rgba_unmultiplied(
                    [2, 1],
                    &pixels,
                )),
            }],
            ..Default::default()
        };
        let output = ctx.run(input, |ctx| {
            capture.update(ctx, 35.0, GraceCurve::default(), false, Some(0.25));
        });
        let image = image::open(&path).unwrap().into_rgba8();
        assert_eq!(image.dimensions(), (2, 1));
        assert_eq!(image.as_raw(), &pixels);
        let metadata: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path.with_extension("json")).unwrap()).unwrap();
        assert_eq!(metadata["level"], 35.0);
        assert_eq!(metadata["test_card"], false);
        assert_eq!(metadata["signal_loss"], 0.25);
        assert!(
            output.viewport_output[&egui::ViewportId::ROOT]
                .commands
                .iter()
                .any(|command| matches!(command, egui::ViewportCommand::Close))
        );
    }

    #[test]
    fn static_capture_schedules_its_own_wakeup_and_requests_only_once() {
        let ctx = egui::Context::default();
        let mut capture = Capture {
            path: Some(PathBuf::from("unused.png")),
            started: Instant::now(),
            requested: false,
            manual: false,
            layout: String::new(),
        };
        // Discard egui's initial layout/font repaint requests.
        for _ in 0..3 {
            let _ = ctx.run(egui::RawInput::default(), |ctx| {
                capture.update(ctx, 0.0, GraceCurve::default(), true, None)
            });
        }
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            capture.update(ctx, 0.0, GraceCurve::default(), true, None)
        });
        let viewport = &output.viewport_output[&egui::ViewportId::ROOT];
        assert!(viewport.repaint_delay > Duration::from_millis(100));
        assert!(viewport.repaint_delay <= Duration::from_millis(1500));
        assert!(!capture.requested);

        capture.started = Instant::now() - Duration::from_secs(2);
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            capture.update(ctx, 0.0, GraceCurve::default(), true, None)
        });
        assert!(
            output.viewport_output[&egui::ViewportId::ROOT]
                .commands
                .iter()
                .any(|command| { matches!(command, egui::ViewportCommand::Screenshot(_)) })
        );
        assert!(capture.requested);
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            capture.update(ctx, 0.0, GraceCurve::default(), true, None)
        });
        assert!(
            !output.viewport_output[&egui::ViewportId::ROOT]
                .commands
                .iter()
                .any(|command| { matches!(command, egui::ViewportCommand::Screenshot(_)) })
        );
    }
}
