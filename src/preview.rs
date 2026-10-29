use crate::{artwork, config, config::Settings, hyprland::Hyprland, recovery::DesktopTrial};
use anyhow::{Context, Result};
use eframe::egui;
use std::{
    path::PathBuf,
    sync::mpsc,
    time::{Duration, Instant},
};

mod capture;
mod controls;
mod curve;
mod playback;
// GL calls require a current eframe context and stay private to the sample scene.
#[allow(unsafe_code)]
mod renderer;
mod scene;
use capture::Capture;
use playback::Playback;
use scene::Scene;

type ImageResult = Result<Option<(PathBuf, artwork::Canvas)>>;

pub fn open() -> Result<()> {
    let path = config::path()?;
    let settings = config::load(&path)?;
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Glow,
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1120.0, 950.0])
            .with_min_inner_size([640.0, 650.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Bork — Preview",
        options,
        Box::new(move |cc| {
            let gl = cc.gl.as_ref().context("OpenGL context unavailable")?;
            let scene = Scene::new(gl, &settings)?;
            // egui follows the system theme; leave its neutral controls intact.
            cc.egui_ctx.style_mut(|style| {
                style.spacing.item_spacing = egui::vec2(10.0, 10.0);
                style.spacing.button_padding = egui::vec2(12.0, 7.0);
            });
            let mut preview = Preview {
                settings,
                path,
                playback: Playback::new(crate::effect::fresh_seed()?),
                scene,
                status: String::new(),
                trial: None,
                confirm_trial: false,
                capture: Capture::new(&cc.egui_ctx),
                image_pending: None,
            };
            if let Some(path) = preview.settings.preview_image.clone() {
                preview.load_image(&cc.egui_ctx, Some(path));
            }
            Ok(Box::new(preview))
        }),
    )
    .map_err(|error| anyhow::anyhow!("Preview failed: {error}"))
}

struct Preview {
    settings: Settings,
    path: PathBuf,
    playback: Playback,
    scene: Scene,
    status: String,
    trial: Option<DesktopTrial>,
    confirm_trial: bool,
    capture: Capture,
    image_pending: Option<mpsc::Receiver<ImageResult>>,
}

impl Preview {
    fn trial(&mut self) -> Result<()> {
        let hypr = Hyprland::from_env()?;
        hypr.check()?;
        self.trial = Some(if self.playback.overtime(&self.settings) {
            DesktopTrial::start_card(
                &hypr,
                u64::from(self.settings.timing.break_seconds.get()),
                u64::from(self.settings.timing.break_seconds.get()),
                self.settings.overtime.clone(),
            )?
        } else {
            DesktopTrial::start(
                &hypr,
                self.scene
                    .desktop_shader(&self.settings, self.playback.level(&self.settings))?,
            )?
        });
        Ok(())
    }

    fn load_image(&mut self, ctx: &egui::Context, path: Option<PathBuf>) {
        if self.image_pending.is_some() {
            return;
        }
        let (sender, receiver) = mpsc::channel();
        self.image_pending = Some(receiver);
        let ctx = ctx.clone();
        // Keep the picker and image decoding off the guardian's UI heartbeat path.
        // The receiver belongs to this preview; a late worker result is discarded on exit.
        std::thread::spawn(move || {
            let path = path.or_else(|| {
                rfd::FileDialog::new()
                    .set_title("Choose preview image")
                    .add_filter("Images", &["png", "jpg", "jpeg", "webp"])
                    .pick_file()
            });
            let result = path
                .map(|path| {
                    artwork::load_image(&path).map(|image| {
                        let image = if image.width() > 4096 || image.height() > 4096 {
                            image::DynamicImage::ImageRgba8(image)
                                .thumbnail(4096, 4096)
                                .into_rgba8()
                        } else {
                            image
                        };
                        (path, artwork::Canvas::from_image(image))
                    })
                })
                .transpose();
            let _ = sender.send(result);
            ctx.request_repaint();
        });
    }

    fn remember_image(&mut self, path: Option<PathBuf>) {
        self.settings.preview_image = path;
        // Remember the image immediately, without saving other unfinished edits.
        self.status = match serde_json::to_value(&self.settings.preview_image)
            .map_err(anyhow::Error::from)
            .and_then(|value| config::update(&self.path, "preview_image", Some(value)))
        {
            Ok(()) => String::new(),
            Err(error) => format!("Could not remember preview image: {error:#}"),
        };
    }
}

impl eframe::App for Preview {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.playback.tick(Instant::now());
        if let Some(receiver) = &self.image_pending {
            match receiver.try_recv() {
                Ok(result) => {
                    self.image_pending = None;
                    match result {
                        Ok(Some((path, image))) => {
                            self.scene.set_image(image);
                            if self.settings.preview_image.as_ref() != Some(&path) {
                                self.remember_image(Some(path));
                            }
                        }
                        Ok(None) => (),
                        Err(error) => {
                            self.status = format!("Could not load preview image: {error:#}")
                        }
                    }
                }
                Err(mpsc::TryRecvError::Empty) => (),
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.image_pending = None;
                    self.status = "Image picker stopped. Try again.".into();
                }
            }
        }
        if let Some(trial) = self.trial.as_mut() {
            match trial.poll() {
                Ok(false) => (),
                Ok(true) => {
                    if let Some(trial) = self.trial.take() {
                        self.status = match trial.finish() {
                            Ok(()) => "Desktop restored.".into(),
                            Err(error) => format!("Recovery error: {error:#}"),
                        };
                    }
                }
                Err(error) => {
                    self.status = format!("Live Preview stopped: {error:#}");
                    self.trial = None;
                }
            }
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::central_panel(&ctx.style()).inner_margin(20))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.strong("Bork — Preview");
                    ui.add_space(4.0);
                    let rect = self.scene.show(
                        ui,
                        &self.settings,
                        self.playback.level(&self.settings),
                        self.playback.test_card(&self.settings),
                        self.playback.transition(&self.settings),
                    );
                    self.image_menu(ui, rect);
                    self.transport(ui);
                    ui.separator();
                    self.controls(ui);
                });
            });
        if self.confirm_trial {
            let response =
                egui::Modal::new(egui::Id::new("live-preview-confirmation")).show(ctx, |ui| {
                    ui.set_width(300.0);
                    ui.strong("Live Preview");
                    ui.label("Apply to your desktop for 5 seconds? It restores automatically.");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Start").clicked() {
                            self.confirm_trial = false;
                            if let Err(error) = self.trial() {
                                self.status = format!("Live Preview refused: {error:#}");
                            }
                        }
                        if ui.button("Cancel").clicked() {
                            self.confirm_trial = false;
                        }
                    });
                });
            if response.should_close() {
                self.confirm_trial = false;
            }
        }
        let level = self.playback.level(&self.settings);
        let test_card = self.playback.test_card(&self.settings);
        self.capture.update(
            ctx,
            level.value(),
            self.settings.curve,
            test_card,
            self.playback
                .transition(&self.settings)
                .map(|(_, seconds)| seconds),
        );
        if self.playback.playing
            || (!test_card && self.settings.interference.has_visible_effect_at(level))
        {
            ctx.request_repaint_after(Duration::from_millis(33));
        } else if self.trial.is_some() || self.image_pending.is_some() {
            ctx.request_repaint_after(Duration::from_millis(250));
        }
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
