use crate::{
    config,
    effect::{EffectKind, Glitchiness, Settings},
    hyprland::Hyprland,
    recovery::DesktopTrial,
};
use anyhow::{Context, Result};
use eframe::egui::{self, Color32};
use std::{path::PathBuf, time::Duration};

mod capture;
mod controls;
// GL calls require a current eframe context and stay private to the sample scene.
#[allow(unsafe_code)]
mod renderer;
mod scene;
use capture::Capture;
use scene::Scene;

pub fn open() -> Result<()> {
    let path = config::path()?;
    let settings = config::load(&path)?;
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Glow,
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 830.0])
            .with_min_inner_size([850.0, 650.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Signal Loss · signal editor",
        options,
        Box::new(move |cc| {
            let gl = cc.gl.as_ref().context("OpenGL context unavailable")?;
            let scene = Scene::new(gl, &settings)?;
            let mut visuals = egui::Visuals::dark();
            visuals.panel_fill = Color32::from_rgb(17, 19, 18);
            visuals.window_fill = visuals.panel_fill;
            visuals.override_text_color = Some(Color32::from_rgb(238, 234, 224));
            visuals.selection.bg_fill = Color32::from_rgb(80, 95, 62);
            cc.egui_ctx.set_visuals(visuals);
            cc.egui_ctx.style_mut(|style| {
                style.spacing.item_spacing = egui::vec2(12.0, 12.0);
                style.spacing.slider_width = 180.0;
            });
            Ok(Box::new(Preview {
                settings,
                path,
                level: Glitchiness::try_from(35.0)?,
                test_card: false,
                scene,
                status: String::new(),
                trial: None,
                confirm_trial: false,
                capture: Capture::new(),
            }))
        }),
    )
    .map_err(|error| anyhow::anyhow!("Preview failed: {error}"))
}

struct Preview {
    settings: Settings,
    path: PathBuf,
    level: Glitchiness,
    test_card: bool,
    scene: Scene,
    status: String,
    trial: Option<DesktopTrial>,
    confirm_trial: bool,
    capture: Capture,
}

impl Preview {
    fn trial(&mut self) -> Result<()> {
        let hypr = Hyprland::from_env()?;
        hypr.check()?;
        self.trial = Some(DesktopTrial::start(
            &hypr,
            self.scene.desktop_shader(&self.settings, self.level)?,
        )?);
        Ok(())
    }
}

impl eframe::App for Preview {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Some(trial) = self.trial.as_mut() {
            match trial.poll() {
                Ok(false) => (),
                Ok(true) => {
                    if let Some(trial) = self.trial.take() {
                        self.status = match trial.finish() {
                            Ok(()) => "Five seconds elapsed. Desktop restored.".into(),
                            Err(error) => format!("Recovery error: {error:#}"),
                        };
                    }
                }
                Err(error) => {
                    self.status = format!("Trial stopped: {error:#}");
                    self.trial = None;
                }
            }
        }
        egui::SidePanel::left("clean-controls")
            .exact_width(340.0)
            .resizable(false)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| self.controls(ui));
            });
        self.scene
            .show(ctx, &self.settings, self.level, self.test_card);
        if self.confirm_trial {
            egui::Window::new("Real desktop trial")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label("This distorts all real windows at the selected level.");
                    ui.label("It expires after five seconds, independently of this editor.");
                    ui.label(
                        "The background timer keeps running; its current state returns afterward.",
                    );
                    if ui.button("Start five-second trial").clicked() {
                        self.confirm_trial = false;
                        if let Err(error) = self.trial() {
                            self.status = format!("Trial refused: {error:#}");
                        }
                    }
                    if ui.button("Cancel").clicked() {
                        self.confirm_trial = false;
                    }
                });
        }
        self.capture.update(
            ctx,
            self.level.value(),
            self.test_card,
            self.settings.effect(EffectKind::Tears).enabled(),
        );
        ctx.request_repaint_after(Duration::from_millis(33));
    }

    fn on_exit(&mut self, gl: Option<&eframe::glow::Context>) {
        if let Some(trial) = self.trial.take()
            && let Err(error) = trial.finish()
        {
            eprintln!("Desktop restoration failed: {error:#}");
        }
        if let Some(gl) = gl {
            self.scene.destroy(gl);
        }
    }
}
