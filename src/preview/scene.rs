//! Shader-rendered sample owns its GL resources, artwork, and animation identity.
use super::renderer::Renderer;
use crate::{
    artwork,
    config::Settings,
    effect::{self, Glitchiness, SignalLoss},
};
use anyhow::Result;
use eframe::egui;
use std::{
    sync::{Arc, Mutex},
    time::Instant,
};

pub(super) struct Scene {
    renderer: Arc<Mutex<Renderer>>,
    error: Arc<Mutex<Option<String>>>,
    sample: Arc<artwork::Canvas>,
    bundled: Arc<artwork::Canvas>,
    card: Arc<artwork::Canvas>,
    card_renderer: crate::overtime::Renderer,
    card_background: image::RgbaImage,
    card_seconds: u32,
    start: Instant,
    seed: u32,
}

impl Scene {
    pub(super) fn new(gl: &eframe::glow::Context, settings: &Settings) -> Result<Self> {
        let sample = Arc::new(artwork::desktop());
        let card_renderer = crate::overtime::Renderer::load(&settings.overtime)?;
        let card_background = card_renderer.background(1280, 720)?;
        let seconds = settings.timing.break_seconds.get();
        let card =
            Arc::new(card_renderer.paint(&card_background, u64::from(seconds), u64::from(seconds)));
        Ok(Self {
            renderer: Arc::new(Mutex::new(Renderer::new(
                gl,
                &settings.interference,
                Glitchiness::ZERO,
            )?)),
            error: Arc::new(Mutex::new(None)),
            sample: sample.clone(),
            bundled: sample,
            card,
            card_renderer,
            card_background,
            card_seconds: seconds,
            start: Instant::now(),
            seed: effect::fresh_seed()?,
        })
    }

    pub(super) fn set_image(&mut self, image: artwork::Canvas) {
        self.sample = Arc::new(image);
    }

    pub(super) fn reset_image(&mut self) {
        self.sample = self.bundled.clone();
    }

    pub(super) fn desktop_shader(&self, settings: &Settings, level: Glitchiness) -> Result<String> {
        settings.interference.hyprland_shader_at(
            level,
            self.start.elapsed().as_secs_f32(),
            self.seed,
        )
    }

    pub(super) fn destroy(&self, gl: &eframe::glow::Context) {
        self.renderer.lock().unwrap().destroy(gl);
    }

    pub(super) fn show(
        &mut self,
        ui: &mut egui::Ui,
        settings: &Settings,
        level: Glitchiness,
        test_card: bool,
        transition: Option<(SignalLoss, f32)>,
    ) -> egui::Rect {
        let seconds = settings.timing.break_seconds.get();
        if seconds != self.card_seconds {
            self.card = Arc::new(self.card_renderer.paint(
                &self.card_background,
                u64::from(seconds),
                u64::from(seconds),
            ));
            self.card_seconds = seconds;
        }
        let width = ui.available_width();
        let height =
            (width * 9.0 / 16.0).min((ui.ctx().content_rect().height() - 530.0).max(180.0));
        let (outer, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
        let rect =
            egui::Rect::from_center_size(outer.center(), egui::vec2(height * 16.0 / 9.0, height));
        let renderer = self.renderer.clone();
        let canvas = if test_card {
            self.card.clone()
        } else {
            self.sample.clone()
        };
        // Letterbox custom screenshots instead of stretching them.
        let aspect = canvas.width() as f32 / canvas.height() as f32;
        let size = if aspect > rect.aspect_ratio() {
            egui::vec2(rect.width(), rect.width() / aspect)
        } else {
            egui::vec2(rect.height() * aspect, rect.height())
        };
        let image_rect = egui::Rect::from_center_size(rect.center(), size);
        ui.painter().rect_filled(rect, 0.0, egui::Color32::BLACK);
        let level = if test_card {
            Glitchiness::ZERO
        } else {
            level.quantized()
        };
        let (body, time) = match transition {
            Some((loss, elapsed)) => (loss.shader_body(), elapsed),
            None => (
                settings.interference.shader_body(level),
                self.start.elapsed().as_secs_f32(),
            ),
        };
        let seed = if transition.is_some() { 0 } else { self.seed };
        let error = self.error.clone();
        ui.painter().add(egui::PaintCallback {
            rect: image_rect,
            callback: Arc::new(eframe::egui_glow::CallbackFn::new(move |_info, painter| {
                let mut renderer = renderer.lock().unwrap();
                match renderer.update(painter.gl(), &body) {
                    Ok(()) => renderer.paint(painter.gl(), &canvas, time, seed),
                    Err(e) => *error.lock().unwrap() = Some(format!("{e:#}")),
                }
            })),
        });
        if let Some(error) = self.error.lock().unwrap().as_ref() {
            ui.colored_label(egui::Color32::RED, error);
        }
        rect
    }
}
