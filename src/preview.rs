use crate::{
    artwork, config, effect::Settings, hyprland::Hyprland, recovery::DesktopTrial,
    renderer::Renderer,
};
use anyhow::{Context, Result};
use eframe::egui::{self, Color32, RichText};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

pub fn open() -> Result<()> {
    let path = config::path()?;
    let settings = config::load(&path)?;
    let seed = crate::effect::fresh_seed()?;
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
            let renderer = Renderer::new(gl, &settings, 35.0)?;
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
                level: 35.0,
                seed,
                test_card: false,
                renderer: Arc::new(Mutex::new(renderer)),
                error: Arc::new(Mutex::new(None)),
                status: String::new(),
                start: Instant::now(),
                trial: None,
                confirm_trial: false,
                sample: Arc::new(artwork::desktop()),
                card: Arc::new(artwork::test_card(1280, 720, 272)),
                capture_requested: false,
            }))
        }),
    )
    .map_err(|error| anyhow::anyhow!("Preview failed: {error}"))
}

struct Preview {
    settings: Settings,
    path: PathBuf,
    level: f32,
    seed: u32,
    test_card: bool,
    renderer: Arc<Mutex<Renderer>>,
    error: Arc<Mutex<Option<String>>>,
    status: String,
    start: Instant,
    trial: Option<(DesktopTrial, Duration)>,
    confirm_trial: bool,
    sample: Arc<artwork::Canvas>,
    card: Arc<artwork::Canvas>,
    capture_requested: bool,
}

impl Preview {
    fn trial(&mut self) -> Result<()> {
        let hypr = Hyprland::from_env()?;
        hypr.check()?;
        let trial = DesktopTrial::start(
            &hypr,
            self.settings.hyprland_shader_at(
                self.level,
                self.start.elapsed().as_secs_f32(),
                self.seed,
            )?,
        )?;
        self.trial = Some((trial, crate::wayland::boot_time()));
        Ok(())
    }

    fn controls(&mut self, ui: &mut egui::Ui) {
        ui.add_space(14.0);
        ui.label(
            RichText::new("SIGNAL LOSS / EDITOR")
                .monospace()
                .color(Color32::from_rgb(212, 230, 162)),
        );
        ui.add_space(12.0);
        ui.heading("Shape the interference.");
        ui.label("A quiet warning. Then a lost signal.");
        ui.separator();
        ui.label("GLITCHINESS");
        ui.add(
            egui::Slider::new(&mut self.level, 0.0..=100.0)
                .step_by(0.1)
                .text("/ 100"),
        );
        ui.label("Type an exact value in the number field.");
        ui.checkbox(&mut self.test_card, "Preview steady test card");
        ui.small("100 is maximum interference, not the deadline.");
        ui.separator();
        egui::Grid::new("effects")
            .num_columns(3)
            .spacing([10.0, 12.0])
            .show(ui, |ui| {
                ui.strong("Effect");
                ui.strong("Entry / 100");
                ui.strong("Max / 1");
                ui.end_row();
                for (name, effect) in [
                    ("Thin tears", &mut self.settings.tears),
                    ("Colour split", &mut self.settings.colour),
                    ("Wide bands", &mut self.settings.bands),
                    ("Static", &mut self.settings.noise),
                    ("Ghosting", &mut self.settings.ghost),
                ] {
                    ui.checkbox(&mut effect.enabled, name);
                    ui.add(
                        egui::DragValue::new(&mut effect.entry)
                            .range(0.0..=99.0)
                            .speed(0.2),
                    );
                    ui.add(
                        egui::DragValue::new(&mut effect.maximum)
                            .range(0.0..=1.0)
                            .speed(0.01)
                            .max_decimals(2),
                    );
                    ui.end_row();
                }
            });
        ui.add_space(8.0);
        ui.add(egui::Slider::new(&mut self.settings.curve, 1.01..=5.0).text("Grace curve"));
        ui.small("Timing only: larger powers delay escalation.");
        ui.separator();
        if ui.button("Save settings").clicked() {
            self.status = match config::save(&self.path, &self.settings) {
                Ok(()) => format!("Saved {}", self.path.display()),
                Err(error) => format!("Save failed: {error:#}"),
            };
        }
        if ui
            .add_enabled(
                self.trial.is_none(),
                egui::Button::new("Try on desktop · 5 seconds"),
            )
            .clicked()
        {
            self.confirm_trial = true;
        }
        if self.trial.is_some()
            && ui.button("End desktop trial now").clicked()
            && let Some((guardian, _)) = self.trial.take()
        {
            self.status = match guardian.finish() {
                Ok(()) => "Desktop restored.".into(),
                Err(e) => format!("Recovery error: {e:#}"),
            };
        }
        ui.small("Emergency: signal-loss stop");
        ui.add_space(8.0);
        ui.label(&self.status);
    }
}

impl eframe::App for Preview {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Some((guardian, started)) = self.trial.as_mut() {
            if let Err(error) = guardian.heartbeat() {
                self.status = format!("Trial stopped: {error:#}");
                self.trial.take();
            } else if crate::wayland::boot_time().saturating_sub(*started)
                >= Duration::from_millis(5200)
            {
                let (guardian, _) = self.trial.take().unwrap();
                self.status = match guardian.finish() {
                    Ok(()) => "Five seconds elapsed. Desktop restored.".into(),
                    Err(e) => format!("Recovery error: {e:#}"),
                };
            }
        }
        egui::SidePanel::left("clean-controls")
            .exact_width(340.0)
            .resizable(false)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| self.controls(ui));
            });
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
            let settings = self.settings.clone();
            let canvas = if self.test_card {
                self.card.clone()
            } else {
                self.sample.clone()
            };
            let level = if self.test_card { 0.0 } else { self.level };
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
                RichText::new(if self.test_card {
                    "04 / TRANSMISSION PAUSED"
                } else if level < 20.0 {
                    "01 / A WARNING IN THE SIGNAL"
                } else if level < 60.0 {
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
        // Private verification seam; not a product control or animation setting.
        if let Some(path) = std::env::var_os("SIGNAL_LOSS_TEST_CAPTURE") {
            if self.start.elapsed() > Duration::from_millis(1500) && !self.capture_requested {
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
                self.capture_requested = true;
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
                    let path = PathBuf::from(&path);
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
                            "level": self.level, "test_card": self.test_card, "tears_enabled": self.settings.tears.enabled,
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
        ctx.request_repaint_after(Duration::from_millis(33));
    }

    fn on_exit(&mut self, gl: Option<&eframe::glow::Context>) {
        if let Some((guardian, _)) = self.trial.take()
            && let Err(error) = guardian.finish()
        {
            eprintln!("Desktop restoration failed: {error:#}");
        }
        if let Some(gl) = gl {
            self.renderer.lock().unwrap().destroy(gl);
        }
    }
}
