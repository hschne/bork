//! Private screenshot verification adapter; no product controls or animation state.
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
}
impl Capture {
    pub(super) fn new() -> Self {
        Self {
            path: std::env::var_os("SIGNAL_LOSS_TEST_CAPTURE").map(PathBuf::from),
            started: Instant::now(),
            requested: false,
        }
    }
    pub(super) fn update(
        &mut self,
        ctx: &egui::Context,
        level: f32,
        test_card: bool,
        tears_enabled: bool,
    ) {
        if let Some(path) = self.path.as_ref() {
            if !self.requested {
                let delay = Duration::from_millis(1500).saturating_sub(self.started.elapsed());
                if delay.is_zero() {
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
                    let path = path.clone();
                    let file = std::fs::File::create(&path)?;
                    let mut encoder =
                        png::Encoder::new(file, image.width() as u32, image.height() as u32);
                    encoder.set_color(png::ColorType::Rgba);
                    encoder.set_depth(png::BitDepth::Eight);
                    encoder.write_header()?.write_image_data(
                        &image
                            .pixels
                            .iter()
                            .flat_map(|p| p.to_array())
                            .collect::<Vec<_>>(),
                    )?;
                    std::fs::write(
                        path.with_extension("json"),
                        serde_json::to_vec_pretty(&serde_json::json!({
                            "level": level, "test_card": test_card, "tears_enabled": tears_enabled,
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
    fn static_capture_schedules_its_own_wakeup_and_requests_only_once() {
        let ctx = egui::Context::default();
        let mut capture = Capture {
            path: Some(PathBuf::from("unused.png")),
            started: Instant::now(),
            requested: false,
        };
        // Discard egui's initial layout/font repaint requests.
        for _ in 0..3 {
            let _ = ctx.run(egui::RawInput::default(), |ctx| {
                capture.update(ctx, 0.0, true, true)
            });
        }
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            capture.update(ctx, 0.0, true, true)
        });
        let viewport = &output.viewport_output[&egui::ViewportId::ROOT];
        assert!(viewport.repaint_delay > Duration::from_millis(100));
        assert!(viewport.repaint_delay <= Duration::from_millis(1500));
        assert!(!capture.requested);

        capture.started = Instant::now() - Duration::from_secs(2);
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            capture.update(ctx, 0.0, true, true)
        });
        assert!(
            output.viewport_output[&egui::ViewportId::ROOT]
                .commands
                .iter()
                .any(|command| { matches!(command, egui::ViewportCommand::Screenshot(_)) })
        );
        assert!(capture.requested);
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            capture.update(ctx, 0.0, true, true)
        });
        assert!(
            !output.viewport_output[&egui::ViewportId::ROOT]
                .commands
                .iter()
                .any(|command| { matches!(command, egui::ViewportCommand::Screenshot(_)) })
        );
    }
}
