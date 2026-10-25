//! Shader-rendered sample owns its GL resources, artwork, and animation identity.
use super::renderer::Renderer;
use crate::{
    artwork,
    effect::{self, Glitchiness, Settings},
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
    card_seconds: u32,
    start: Instant,
    seed: u32,
}

impl Scene {
    pub(super) fn new(gl: &eframe::glow::Context, settings: &Settings) -> Result<Self> {
        let sample = Arc::new(artwork::desktop());
        Ok(Self {
            renderer: Arc::new(Mutex::new(Renderer::new(gl, settings, Glitchiness::ZERO)?)),
            error: Arc::new(Mutex::new(None)),
            sample: sample.clone(),
            bundled: sample,
            card: Arc::new(artwork::test_card(1280, 720, 300)),
            card_seconds: 300,
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
        settings.hyprland_shader_at(level, self.start.elapsed().as_secs_f32(), self.seed)
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
    ) -> egui::Rect {
        let seconds = settings.timing.break_seconds.get();
        if seconds != self.card_seconds {
            self.card = Arc::new(artwork::test_card_for_duration(
                1280,
                720,
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
        let settings = settings.clone();
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
        let time = self.start.elapsed().as_secs_f32();
        let seed = self.seed;
        let error = self.error.clone();
        ui.painter().add(egui::PaintCallback {
            rect: image_rect,
            callback: Arc::new(eframe::egui_glow::CallbackFn::new(move |_info, painter| {
                let mut renderer = renderer.lock().unwrap();
                match renderer.update(painter.gl(), &settings, level) {
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
