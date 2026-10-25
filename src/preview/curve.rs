//! Direct manipulation of the same validated curve used by the break cycle.
use crate::effect::{CurvePoint, GraceCurve, MIN_POINT_GAP};
use anyhow::Result;
use eframe::egui::{self, Align2, FontId, Pos2, Rect, Sense, Stroke, Vec2};

pub(super) fn show(
    ui: &mut egui::Ui,
    curve: GraceCurve,
    progress: Option<f32>,
    grace_seconds: u32,
) -> Result<Option<GraceCurve>> {
    let mut edited = curve;
    ui.horizontal(|ui| {
        ui.strong("Interference");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button("Reset curve").clicked() {
                edited = GraceCurve::default();
            }
        });
    });
    let (outer, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 210.0), Sense::hover());
    let plot = Rect::from_min_max(
        outer.min + egui::vec2(34.0, 22.0),
        outer.max - egui::vec2(14.0, 26.0),
    );
    let screen = |progress: f32, intensity: f32| {
        Pos2::new(
            egui::lerp(plot.x_range(), progress),
            egui::lerp(plot.bottom()..=plot.top(), intensity / 100.0),
        )
    };
    let mut points = edited.points();
    let mut focused = None;
    ui.add_enabled_ui(grace_seconds > 0, |ui| {
        for i in 0..3 {
            let point = screen(points[i].progress, points[i].intensity);
            let response = ui.interact(
                Rect::from_center_size(point, Vec2::splat(28.0)),
                ui.id().with(("curve-point", i)),
                Sense::click_and_drag(),
            );
            if i == 0 {
                super::capture::target("curve-start", &response);
            }
            response.widget_info(|| {
                egui::WidgetInfo::slider(
                    ui.is_enabled(),
                    f64::from(points[i].intensity),
                    if i == 0 {
                        "Starting intensity".into()
                    } else {
                        format!("Curve point {i}")
                    },
                )
            });
            if response.clicked() || response.drag_started() {
                response.request_focus();
            }
            if response.dragged()
                && let Some(pointer) = response.interact_pointer_pos()
            {
                move_point(
                    &mut points,
                    i,
                    (pointer.x - plot.left()) / plot.width(),
                    100.0 * (plot.bottom() - pointer.y) / plot.height(),
                );
            }
            if response.has_focus() && ui.is_enabled() {
                let (dx, dy) = ui.input_mut(|input| {
                    let modifiers = input.modifiers;
                    let step = if modifiers.shift { 5.0 } else { 1.0 };
                    let dx = input.consume_key(modifiers, egui::Key::ArrowRight) as u8 as f32
                        - input.consume_key(modifiers, egui::Key::ArrowLeft) as u8 as f32;
                    let dy = input.consume_key(modifiers, egui::Key::ArrowUp) as u8 as f32
                        - input.consume_key(modifiers, egui::Key::ArrowDown) as u8 as f32;
                    (dx * step / 100.0, dy * step)
                });
                if dx != 0.0 || dy != 0.0 {
                    let point = points[i];
                    move_point(&mut points, i, point.progress + dx, point.intensity + dy);
                }
            }
            if response.hovered() || response.has_focus() || response.dragged() {
                focused = Some(i);
            }
        }
    });
    if points != edited.points() {
        edited = GraceCurve::try_from(points)?;
    }
    let painter = ui.painter();
    let color = ui.visuals().selection.stroke.color;
    let text = ui.visuals().weak_text_color();
    painter.rect_filled(outer, 3.0, ui.visuals().extreme_bg_color);
    for intensity in [0.0, 25.0, 50.0, 75.0, 100.0] {
        let y = screen(0.0, intensity).y;
        painter.line_segment(
            [Pos2::new(plot.left(), y), Pos2::new(plot.right(), y)],
            ui.visuals().widgets.noninteractive.bg_stroke,
        );
        painter.text(
            Pos2::new(plot.left() - 7.0, y),
            Align2::RIGHT_CENTER,
            format!("{intensity:.0}"),
            FontId::proportional(11.0),
            text,
        );
    }
    painter.text(
        Pos2::new(plot.left(), outer.top() + 6.0),
        Align2::LEFT_TOP,
        "Intensity %",
        FontId::proportional(11.0),
        text,
    );
    for fraction in [0.0, 1.0 / 3.0, 2.0 / 3.0, 1.0] {
        let x = screen(fraction, 0.0).x;
        painter.line_segment(
            [Pos2::new(x, plot.top()), Pos2::new(x, plot.bottom())],
            ui.visuals().widgets.noninteractive.bg_stroke,
        );
        let seconds = (grace_seconds as f32 * fraction).round() as u32;
        painter.text(
            Pos2::new(x, plot.bottom() + 9.0),
            Align2::CENTER_TOP,
            format!("{}:{:02}", seconds / 60, seconds % 60),
            FontId::proportional(11.0),
            text,
        );
    }
    let line = (0..=160)
        .map(|i| {
            let progress = i as f32 / 160.0;
            screen(progress, edited.level(progress).value())
        })
        .collect();
    painter.add(egui::Shape::line(line, Stroke::new(2.0, color)));
    if let Some(progress) = progress {
        let marker = screen(progress, edited.level(progress).value());
        painter.line_segment(
            [
                Pos2::new(marker.x, plot.top()),
                Pos2::new(marker.x, plot.bottom()),
            ],
            Stroke::new(1.0, text),
        );
        painter.circle_filled(marker, 4.0, ui.visuals().text_color());
    }
    for (i, point) in edited.points().into_iter().enumerate() {
        let center = screen(point.progress, point.intensity);
        painter.circle_filled(
            center,
            if i == 3 { 4.0 } else { 5.0 },
            if i == 3 {
                color
            } else {
                ui.visuals().extreme_bg_color
            },
        );
        if i < 3 {
            painter.circle_stroke(center, 5.0, Stroke::new(2.0, color));
            if focused == Some(i) {
                painter.circle_stroke(center, 9.0, Stroke::new(1.0, color));
            }
        }
    }
    if grace_seconds == 0 {
        ui.weak("Grace is disabled.");
    } else if let Some(i) = focused {
        let point = edited.points()[i];
        let seconds = (point.progress * grace_seconds as f32).round() as u32;
        ui.weak(format!(
            "Grace {}:{:02} · {:.0}% intensity",
            seconds / 60,
            seconds % 60,
            point.intensity
        ));
    } else {
        ui.weak("Drag points · arrow keys to fine-tune");
    }
    Ok((edited != curve).then_some(edited))
}

fn move_point(points: &mut [CurvePoint; 4], index: usize, progress: f32, intensity: f32) {
    let minimum = if index == 0 {
        0.0
    } else {
        points[index - 1].intensity
    };
    points[index].intensity = intensity.clamp(minimum, points[index + 1].intensity);
    if index > 0 {
        points[index].progress = progress.clamp(
            points[index - 1].progress + MIN_POINT_GAP,
            points[index + 1].progress - MIN_POINT_GAP,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dragging_cannot_cross_neighbors_or_move_the_start_in_time() {
        let mut points = GraceCurve::default().points();
        move_point(&mut points, 0, 0.8, 200.0);
        assert_eq!(points[0].progress, 0.0);
        assert_eq!(points[0].intensity, points[1].intensity);
        move_point(&mut points, 1, 1.0, -10.0);
        assert!(points[1].progress < points[2].progress);
        assert_eq!(points[1].intensity, points[0].intensity);
        GraceCurve::try_from(points).unwrap();
    }
}
