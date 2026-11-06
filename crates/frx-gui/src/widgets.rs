//! Stateless paint/input helpers. Shared by the normal UI and headless egui tests.
use crate::{
    config::{controls as c, layout as l},
    theme,
};
use eframe::egui::{self, Response, Sense, Stroke, pos2, vec2};
use frx_audio::Status;
use frx_core::waveform::Waveform;
use std::ops::RangeInclusive;

pub fn reset_slider(
    ui: &mut egui::Ui,
    value: &mut f64,
    range: RangeInclusive<f64>,
    default: f64,
    suffix: &str,
) -> Response {
    let mut response = ui
        .horizontal(|ui| {
            let slider = ui.add(egui::Slider::new(value, range).show_value(false));
            let label = ui.add_sized(
                [l::VALUE_MIN_WIDTH, ui.spacing().interact_size.y],
                egui::Label::new(format!(
                    "{value:.precision$}{suffix}",
                    precision = c::SLIDER_DECIMALS
                ))
                .sense(Sense::click()),
            );
            slider.union(label)
        })
        .inner;
    if response.double_clicked() {
        *value = default;
        response.mark_changed();
    }
    response.on_hover_text(format!("Double-click to reset to {default}{suffix}"))
}

pub fn reset_toggle(ui: &mut egui::Ui, value: &mut bool, default: bool, label: &str) -> Response {
    let mut response = ui.checkbox(value, label);
    if response.double_clicked() {
        *value = default;
        response.mark_changed();
    }
    response.on_hover_text(format!(
        "Double-click to reset ({})",
        if default { "on" } else { "off" }
    ))
}

pub fn db_label(value: Option<f32>) -> String {
    value.map_or("−∞".into(), |db| format!("{db:.1}"))
}

pub fn stereo_meter(ui: &mut egui::Ui, status: Option<&Status>) {
    let rms = status.map_or([None; 2], |s| s.rms_dbfs);
    let peaks = status.map_or([None; 2], |s| s.peak_dbfs);
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), l::METER_HEIGHT), Sense::hover());
    response.on_hover_text(format!("Stereo RMS envelope ({:.0} ms exponential time constant), with decaying sample-peak markers. Post-gain, post-clamp dBFS. VU-style display, not IEC-calibrated VU.", frx_core::config::RMS_TIME_CONSTANT_SECONDS * 1000.0));
    let painter = ui.painter_at(rect);
    let top = rect.top() + l::METER_TOP;
    let bottom = rect.bottom() - l::METER_BOTTOM;
    let y = |db: f32| bottom - ((db - c::DB_FLOOR) / -c::DB_FLOOR).clamp(0.0, 1.0) * (bottom - top);
    for channel in 0..2 {
        let left =
            rect.left() + l::METER_LEFT + channel as f32 * (l::METER_BAR_WIDTH + l::METER_BAR_GAP);
        let bar =
            egui::Rect::from_min_max(pos2(left, top), pos2(left + l::METER_BAR_WIDTH, bottom));
        painter.rect_filled(bar, l::CARD_RADIUS, theme::RAISED);
        let level = rms[channel].unwrap_or(c::DB_FLOOR);
        for (low, high, color) in [
            (c::DB_FLOOR, c::METER_WARN_DB, theme::METER_GREEN),
            (c::METER_WARN_DB, c::METER_HOT_DB, theme::AMBER),
            (c::METER_HOT_DB, 0.0, theme::ALERT),
        ] {
            if level > low {
                painter.rect_filled(
                    egui::Rect::from_min_max(
                        pos2(left, y(level.min(high))),
                        pos2(bar.right(), y(low)),
                    ),
                    0,
                    color,
                );
            }
        }
        if let Some(peak) = peaks[channel] {
            painter.line_segment(
                [pos2(left, y(peak)), pos2(bar.right(), y(peak))],
                Stroke::new(
                    l::PLAYHEAD_STROKE,
                    if peak >= 0.0 {
                        theme::ALERT
                    } else {
                        theme::TEXT
                    },
                ),
            );
        }
        painter.text(
            pos2(bar.center().x, rect.top()),
            egui::Align2::CENTER_TOP,
            if channel == 0 { "L" } else { "R" },
            egui::FontId::proportional(l::SMALL_FONT),
            theme::TEXT,
        );
        painter.text(
            pos2(bar.center().x, bottom + l::SPACE_SMALL),
            egui::Align2::CENTER_TOP,
            db_label(rms[channel]),
            egui::FontId::monospace(l::SMALL_FONT),
            theme::TEXT,
        );
    }
    let axis = rect.left()
        + l::METER_LEFT
        + 2.0 * l::METER_BAR_WIDTH
        + l::METER_BAR_GAP
        + l::METER_AXIS_GAP;
    for &tick in c::METER_TICKS {
        painter.text(
            pos2(axis, y(tick)),
            egui::Align2::LEFT_CENTER,
            format!("{tick:.0}"),
            egui::FontId::monospace(l::MICRO_FONT),
            theme::MUTED,
        );
    }
}

pub fn waveform_view(
    ui: &mut egui::Ui,
    waveform: Option<&Waveform>,
    position: f64,
    duration: f64,
    height: f32,
) -> Response {
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, l::PLOT_RADIUS, theme::PANEL);
    for i in 1..l::GRID_Y_DIVISIONS {
        let y = rect.top() + rect.height() * i as f32 / l::GRID_Y_DIVISIONS as f32;
        painter.line_segment(
            [pos2(rect.left(), y), pos2(rect.right(), y)],
            Stroke::new(l::THIN_STROKE, theme::RAISED),
        );
    }
    for i in 1..l::GRID_X_DIVISIONS {
        let x = rect.left() + rect.width() * i as f32 / l::GRID_X_DIVISIONS as f32;
        painter.line_segment(
            [pos2(x, rect.top()), pos2(x, rect.bottom())],
            Stroke::new(l::THIN_STROKE, theme::RAISED),
        );
    }
    if let Some(waveform) = waveform {
        let columns = (rect.width().ceil() as usize)
            .max(1)
            .min(waveform.peaks.len());
        let normalized = waveform.normalization().map_or(1.0, |n| n.gain);
        // Two passes guarantee the original is in front, never painted over by
        // the quieter normalization preview. Both are source-relative, not live gain.
        for (gain, color) in [(normalized, theme::NORMALIZED_WAVE), (1.0, theme::AMBER)] {
            for column in 0..columns {
                let start = column * waveform.peaks.len() / columns;
                let end = ((column + 1) * waveform.peaks.len() / columns).max(start + 1);
                let (min, max) = waveform.peaks[start..end]
                    .iter()
                    .fold((f32::INFINITY, f32::NEG_INFINITY), |(min, max), p| {
                        (min.min(p.min), max.max(p.max))
                    });
                let x = rect.left() + (column as f32 + 0.5) / columns as f32 * rect.width();
                let scale = rect.height() * l::WAVE_SCALE;
                painter.line_segment(
                    [
                        pos2(x, rect.center().y - (max * gain).clamp(-1.0, 1.0) * scale),
                        pos2(x, rect.center().y - (min * gain).clamp(-1.0, 1.0) * scale),
                    ],
                    Stroke::new(l::THIN_STROKE, color),
                );
            }
        }
    } else {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "Waveform preview",
            egui::FontId::proportional(l::BODY_FONT),
            theme::MUTED,
        );
    }
    playhead(&painter, rect, position, duration);
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub fn playhead(painter: &egui::Painter, rect: egui::Rect, position: f64, duration: f64) {
    if duration > 0.0 {
        let x = rect.left() + (position / duration).clamp(0.0, 1.0) as f32 * rect.width();
        painter.line_segment(
            [pos2(x, rect.top()), pos2(x, rect.bottom())],
            Stroke::new(l::PLAYHEAD_STROKE, theme::PURPLE),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn double_click_slider_restores_default_and_marks_change() {
        let ctx = egui::Context::default();
        let mut value = -12.0;
        let mut rect = egui::Rect::NOTHING;
        let _ = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                rect = reset_slider(ui, &mut value, -60.0..=12.0, 0.0, " dB").rect;
            });
        });
        let pos = pos2(rect.right() - 2.0, rect.center().y);
        let mut reset = false;
        for (time, pressed) in [(0.1, true), (0.15, false), (0.2, true), (0.25, false)] {
            let input = egui::RawInput {
                time: Some(time),
                events: vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: Default::default(),
                    },
                ],
                ..Default::default()
            };
            let _ = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let response = reset_slider(ui, &mut value, -60.0..=12.0, 0.0, " dB");
                    if response.double_clicked() {
                        reset = response.changed();
                    }
                });
            });
        }
        assert_eq!(value, 0.0);
        assert!(reset);
    }
}
