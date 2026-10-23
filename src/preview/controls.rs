//! Editor controls edit only validated tuning, never guardian transport state.
use super::Preview;
use crate::{
    config,
    effect::{Effect, EffectKind, Glitchiness},
};
use eframe::egui::{self, Color32, RichText};

impl Preview {
    pub(super) fn controls(&mut self, ui: &mut egui::Ui) {
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
        let mut level = self.level.value();
        if ui
            .add(
                egui::Slider::new(&mut level, 0.0..=100.0)
                    .step_by(0.1)
                    .text("/ 100"),
            )
            .changed()
        {
            match Glitchiness::try_from(level) {
                Ok(level) => self.level = level,
                Err(error) => self.status = error.to_string(),
            }
        }
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
                for (name, kind) in [
                    ("Thin tears", EffectKind::Tears),
                    ("Colour split", EffectKind::Colour),
                    ("Wide bands", EffectKind::Bands),
                    ("Static", EffectKind::Noise),
                    ("Ghosting", EffectKind::Ghost),
                ] {
                    let effect = self.settings.effect(kind);
                    let (mut enabled, mut entry, mut maximum) =
                        (effect.enabled(), effect.entry(), effect.maximum());
                    ui.checkbox(&mut enabled, name);
                    ui.add(
                        egui::DragValue::new(&mut entry)
                            .range(0.0..=99.0)
                            .speed(0.2),
                    );
                    ui.add(
                        egui::DragValue::new(&mut maximum)
                            .range(0.0..=1.0)
                            .speed(0.01)
                            .max_decimals(2),
                    );
                    match Effect::new(enabled, entry, maximum) {
                        Ok(effect) => self.settings.set_effect(kind, effect),
                        Err(error) => self.status = error.to_string(),
                    }
                    ui.end_row();
                }
            });
        ui.add_space(8.0);
        let mut curve = self.settings.curve().value();
        if ui
            .add(egui::Slider::new(&mut curve, 1.01..=5.0).text("Grace curve"))
            .changed()
            && let Err(error) = self.settings.set_curve(curve)
        {
            self.status = error.to_string();
        }
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
            && let Some(guardian) = self.trial.take()
        {
            self.status = match guardian.finish() {
                Ok(()) => "Desktop restored.".into(),
                Err(e) => format!("Recovery error: {e:#}"),
            };
        }
        ui.small("Emergency: signal-loss reset");
        ui.add_space(8.0);
        ui.label(&self.status);
    }
}
