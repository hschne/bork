//! The player previews progression; settings expose timing and the intensity curve.
use super::{Preview, playback::DEMO_SECONDS};
use crate::config;
use eframe::egui::{self, Align, Layout};
use std::time::Instant;

impl Preview {
    pub(super) fn image_menu(&mut self, ui: &mut egui::Ui, rect: egui::Rect) {
        let label = self
            .settings
            .preview_image
            .as_ref()
            .and_then(|path| path.file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Sample desktop".into());
        let menu_rect = egui::Rect::from_min_size(
            rect.left_bottom() + egui::vec2(10.0, -40.0),
            egui::vec2(240.0, 32.0),
        );
        ui.scope_builder(egui::UiBuilder::new().max_rect(menu_rect), |ui| {
            ui.set_max_width(240.0);
            ui.add_enabled_ui(self.image_pending.is_none(), |ui| {
                ui.menu_button(label, |ui| {
                    if ui.button("Choose image…").clicked() {
                        self.load_image(ui.ctx(), None);
                        ui.close();
                    }
                    if self.settings.preview_image.is_some()
                        && ui.button("Reset to sample").clicked()
                    {
                        self.scene.reset_image();
                        self.remember_image(None);
                        ui.close();
                    }
                });
            });
        });
    }

    pub(super) fn transport(&mut self, ui: &mut egui::Ui) {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let label = if self.playback.playing {
                "Pause"
            } else {
                "Play"
            };
            let response = ui
                .add_sized(
                    [32.0, 32.0],
                    egui::Button::new(if self.playback.playing { "⏸" } else { "▶" }).frame(false),
                )
                .on_hover_text(label);
            response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
            if response.clicked() {
                self.playback.toggle(Instant::now());
            }
            let mut position = self.playback.position;
            let width = (ui.available_width() - 210.0).max(60.0);
            ui.scope(|ui| {
                ui.spacing_mut().slider_width = width;
                let response = ui
                    .add(egui::Slider::new(&mut position, 0.0..=DEMO_SECONDS).show_value(false))
                    .on_hover_text("Preview timeline");
                if response.clicked() || response.drag_started() {
                    response.request_focus();
                }
                super::capture::target("timeline", &response);
                if response.changed() {
                    self.playback.seek(position);
                }
            });
            ui.small(format!("0:{:02} / 0:15", self.playback.position as u32));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if self.trial.is_some() {
                    if ui.button("End Live Preview").clicked()
                        && let Some(trial) = self.trial.take()
                    {
                        self.status = match trial.finish() {
                            Ok(()) => "Desktop restored.".into(),
                            Err(error) => format!("Recovery error: {error:#}"),
                        };
                    }
                } else {
                    let response = ui.button("Live Preview");
                    super::capture::target("live-preview", &response);
                    if response.clicked() {
                        self.playback.pause();
                        self.confirm_trial = true;
                    }
                }
            });
        });
        let phase = if self.playback.test_card(&self.settings) {
            "Overtime · break required".into()
        } else if self.playback.transition(&self.settings).is_some() {
            "Overtime · connection lost".into()
        } else if self.playback.position < 1.0 {
            "Clean desktop".into()
        } else {
            format!(
                "Grace · {:.0}% interference",
                self.playback.level(&self.settings).value()
            )
        };
        ui.weak(phase);
    }

    pub(super) fn controls(&mut self, ui: &mut egui::Ui) {
        ui.add_space(8.0);
        if ui.available_width() >= 760.0 {
            ui.columns(2, |columns| {
                self.timing(&mut columns[0]);
                self.curve(&mut columns[1]);
            });
        } else {
            self.timing(ui);
            ui.add_space(16.0);
            self.curve(ui);
        }
        ui.add_space(14.0);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let response = ui.button("Save settings");
            super::capture::target("save", &response);
            if response.clicked() {
                self.status = match config::save(&self.path, &self.settings) {
                    Ok(()) => "Settings saved.".into(),
                    Err(error) => format!("Save failed: {error:#}"),
                };
            }
            ui.label(&self.status);
        });
    }

    fn timing(&mut self, ui: &mut egui::Ui) {
        ui.strong("Timing");
        ui.add_space(6.0);
        let timing = &mut self.settings.timing;
        let mut work = timing.work_seconds.get();
        let mut grace = timing.grace_seconds;
        let mut rest = timing.break_seconds.get();
        egui::Grid::new("timing")
            .spacing([28.0, 10.0])
            .show(ui, |ui| {
                for (label, value, minimum, unit) in [
                    ("Work", &mut work, 1, "min"),
                    ("Grace", &mut grace, 0, "min"),
                    ("Break", &mut rest, 1, "min without input"),
                ] {
                    ui.label(label);
                    ui.horizontal(|ui| {
                        ui.add_sized(
                            [64.0, 28.0],
                            egui::DragValue::new(value)
                                .range(minimum..=u32::MAX)
                                .speed(15.0)
                                .update_while_editing(false)
                                .custom_formatter(|seconds, _| {
                                    format!("{:.2}", seconds / 60.0)
                                        .trim_end_matches('0')
                                        .trim_end_matches('.')
                                        .to_owned()
                                })
                                .custom_parser(|text| {
                                    text.parse::<f64>().ok().map(|minutes| minutes * 60.0)
                                }),
                        );
                        ui.weak(unit);
                    });
                    ui.end_row();
                }
            });
        timing.work_seconds = work.try_into().unwrap();
        timing.grace_seconds = grace;
        timing.break_seconds = rest.try_into().unwrap();
    }

    fn curve(&mut self, ui: &mut egui::Ui) {
        match super::curve::show(
            ui,
            self.settings.curve,
            self.playback.grace_progress(&self.settings),
            self.settings.timing.grace_seconds,
        ) {
            Ok(Some(curve)) => self.settings.curve = curve,
            Ok(None) => (),
            Err(error) => self.status = error.to_string(),
        }
    }
}
