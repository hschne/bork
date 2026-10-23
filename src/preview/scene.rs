//! Shader-rendered sample owns its GL resources, artwork, and animation identity.
use super::renderer::Renderer;
use crate::{
    artwork,
    effect::{self, Glitchiness, Settings},
};
use anyhow::Result;
use eframe::egui::{self, Color32, RichText};
use std::{
    sync::{Arc, Mutex},
    time::Instant,
};

pub(super) struct Scene {
    renderer: Arc<Mutex<Renderer>>,
    error: Arc<Mutex<Option<String>>>,
    sample: Arc<artwork::Canvas>,
    card: Arc<artwork::Canvas>,
    start: Instant,
    seed: u32,
}

impl Scene {
    pub(super) fn new(gl: &eframe::glow::Context, settings: &Settings) -> Result<Self> {
        let seed = effect::fresh_seed()?;
        Ok(Self {
            renderer: Arc::new(Mutex::new(Renderer::new(
                gl,
                settings,
                Glitchiness::try_from(35.0)?,
            )?)),
            error: Arc::new(Mutex::new(None)),
            sample: Arc::new(artwork::desktop()),
            card: Arc::new(artwork::test_card(1280, 720, 272)),
            start: Instant::now(),
            seed,
        })
    }
    pub(super) fn desktop_shader(&self, settings: &Settings, level: Glitchiness) -> Result<String> {
        settings.hyprland_shader_at(level, self.start.elapsed().as_secs_f32(), self.seed)
    }
    pub(super) fn destroy(&self, gl: &eframe::glow::Context) {
        self.renderer.lock().unwrap().destroy(gl);
    }
    pub(super) fn show(
        &self,
        ctx: &egui::Context,
        settings: &Settings,
        level: Glitchiness,
        test_card: bool,
    ) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(14.0);
            ui.horizontal(|ui| {
                ui.heading("Your signal is slipping.");
                ui.label(
                    RichText::new("SHARED GLSL / SAMPLE DESKTOP")
                        .monospace()
                        .small()
                        .color(Color32::from_rgb(164, 170, 161)),
                );
            });
            ui.add_space(16.0);
            let width = ui.available_width();
            let height = (width * 9.0 / 16.0)
                .min(ui.available_height() - 110.0)
                .max(100.0);
            let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
            let renderer = self.renderer.clone();
            let settings = settings.clone();
            let canvas = if test_card {
                self.card.clone()
            } else {
                self.sample.clone()
            };
            let level = if test_card { Glitchiness::ZERO } else { level };
            let time = self.start.elapsed().as_secs_f32();
            let seed = self.seed;
            let error = self.error.clone();
            ui.painter().add(egui::PaintCallback {
                rect,
                callback: Arc::new(eframe::egui_glow::CallbackFn::new(move |_info, painter| {
                    let mut renderer = renderer.lock().unwrap();
                    match renderer.update(painter.gl(), &settings, level) {
                        Ok(()) => renderer.paint(painter.gl(), &canvas, time, seed),
                        Err(e) => *error.lock().unwrap() = Some(format!("{e:#}")),
                    }
                })),
            });
            ui.add_space(18.0);
            ui.label(
                RichText::new(if test_card {
                    "04 / TRANSMISSION PAUSED"
                } else if level.value() < 20.0 {
                    "01 / A WARNING IN THE SIGNAL"
                } else if level.value() < 60.0 {
                    "02 / THE CONNECTION IS UNSTABLE"
                } else {
                    "03 / LOSING THE PICTURE"
                })
                .monospace()
                .color(Color32::from_rgb(212, 230, 162)),
            );
            ui.label("20 min work / 3 min accelerating grace / test card");
            ui.label("Five continuous minutes without keyboard or mouse input completes a break.");
            if let Some(error) = self.error.lock().unwrap().as_ref() {
                ui.colored_label(Color32::LIGHT_RED, error);
            }
        });
    }
}
