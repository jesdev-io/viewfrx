//! Stateless paint/input helpers. Shared by the normal UI and headless egui tests.
use crate::{
    config::{controls as c, layout as l},
    model::TimeWindow,
    theme,
};
use eframe::egui::{self, Response, Sense, Stroke, pos2, vec2};
use frx_audio::Status;
use frx_core::{annotations::Annotation, waveform::Waveform};
use std::ops::RangeInclusive;

/// Height after a bottom-edge drag: egui drag deltas grow downward on screen,
/// so dragging the lower handle DOWN expands the plot (and up compacts it).
pub fn drag_height(value: f32, delta_y: f32, min: f32, max: f32) -> f32 {
    (value + delta_y).clamp(min, max)
}

/// Drag strip at a plot's bottom edge: dragging resizes the plot height
/// within [min, max]. Returns the handle response.
pub fn height_handle(ui: &mut egui::Ui, value: &mut f32, min: f32, max: f32) -> Response {
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), l::HEIGHT_HANDLE), Sense::drag());
    let response = response
        .on_hover_cursor(egui::CursorIcon::ResizeVertical)
        .on_hover_text("Drag down/up to resize this plot");
    if response.dragged() {
        *value = drag_height(*value, response.drag_delta().y, min, max);
    }
    // Visible grip so the handle is discoverable: a centered grab bar that
    // highlights on hover/drag.
    let painter = ui.painter_at(rect);
    if response.hovered() || response.dragged() {
        painter.rect_filled(rect, 0, theme::RAISED);
    }
    let grip = egui::Rect::from_center_size(rect.center(), vec2(46.0, 3.0));
    painter.rect_filled(
        grip,
        2,
        if response.dragged() {
            theme::AMBER
        } else {
            theme::MUTED
        },
    );
    painter.line_segment(
        [rect.left_bottom(), rect.right_bottom()],
        Stroke::new(l::THIN_STROKE, theme::RAISED),
    );
    response
}

#[derive(Clone, Copy, Default, PartialEq)]
pub enum Channels {
    Left,
    Right,
    #[default]
    Both,
}
impl Channels {
    pub fn indices(self, stereo: bool) -> &'static [usize] {
        if !stereo {
            return &[0];
        }
        match self {
            Self::Left => &[0],
            Self::Right => &[1],
            Self::Both => &[0, 1],
        }
    }
    pub fn control(&mut self, ui: &mut egui::Ui, label: &str, stereo: bool) {
        ui.horizontal(|ui| {
            ui.label(label);
            if stereo {
                for (value, text) in [
                    (Self::Left, "Left"),
                    (Self::Right, "Right"),
                    (Self::Both, "Both"),
                ] {
                    if ui.selectable_value(self, value, text).double_clicked() {
                        *self = Self::Both;
                    }
                }
            }
        });
    }
}

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

#[cfg(test)]
pub fn waveform_view(
    ui: &mut egui::Ui,
    waveform: Option<&Waveform>,
    position: f64,
    duration: f64,
    height: f32,
) -> Response {
    waveform_channel_view(
        ui,
        waveform,
        position,
        TimeWindow::full(duration),
        height,
        None,
        &[],
    )
}

/// Vertical lines for timestamp annotations stored in the file's metadata.
pub fn annotation_lines(
    painter: &egui::Painter,
    rect: egui::Rect,
    annotations: &[Annotation],
    window: TimeWindow,
) {
    for annotation in annotations {
        let x = rect.left() + window.time_to_fraction(annotation.seconds) * rect.width();
        painter.line_segment(
            [pos2(x, rect.top()), pos2(x, rect.bottom())],
            Stroke::new(l::THIN_STROKE, theme::ANNOTATION),
        );
    }
}

pub fn waveform_channel_view(
    ui: &mut egui::Ui,
    waveform: Option<&Waveform>,
    position: f64,
    window: TimeWindow,
    height: f32,
    channel: Option<usize>,
    annotations: &[Annotation],
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
        let peaks = channel
            .and_then(|c| waveform.channel_peaks.get(c))
            .unwrap_or(&waveform.peaks);
        // Playback removes the per-channel DC offset, so the plot shows the
        // DC-removed signal (the file's DC offset is listed separately in dBFS).
        let dc = channel
            .and_then(|c| waveform.dc_offset.get(c))
            .copied()
            .unwrap_or(0.0);
        // Zoom window: only bins intersecting [start, end] are painted,
        // stretched across the full plot width (all plots share the window).
        let bin_seconds = waveform.frames_per_bin as f64 / waveform.sample_rate.max(1) as f64;
        let (first, last) = if peaks.is_empty() {
            (0, 0)
        } else {
            let first = ((window.start / bin_seconds) as usize).min(peaks.len() - 1);
            let last = ((window.end / bin_seconds) as usize + 1).clamp(first + 1, peaks.len());
            (first, last)
        };
        let visible = &peaks[first..last];
        let columns = (rect.width().ceil() as usize).max(1).min(visible.len());
        let normalized = waveform.normalization().map_or(1.0, |n| n.gain);
        // Two passes guarantee the original is in front, never painted over by
        // the quieter normalization preview. Both are source-relative, not live gain.
        for (gain, color) in [(normalized, theme::NORMALIZED_WAVE), (1.0, theme::AMBER)] {
            for column in 0..columns {
                let start = column * visible.len() / columns;
                let end = ((column + 1) * visible.len() / columns).max(start + 1);
                let (min, max) = visible[start..end]
                    .iter()
                    .fold((f32::INFINITY, f32::NEG_INFINITY), |(min, max), p| {
                        (min.min(p.min - dc), max.max(p.max - dc))
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
    annotation_lines(&painter, rect, annotations, window);
    playhead(&painter, rect, position, window);
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub fn playhead(painter: &egui::Painter, rect: egui::Rect, position: f64, window: TimeWindow) {
    if window.len() > 0.0 {
        let x = rect.left() + window.time_to_fraction(position) * rect.width();
        painter.line_segment(
            [pos2(x, rect.top()), pos2(x, rect.bottom())],
            Stroke::new(l::PLAYHEAD_STROKE, theme::PURPLE),
        );
    }
}

/// Live magnitude spectrum panel (bottom right of the center area). Log-frequency
/// axis with a 1/3-octave center-frequency grid and a height-dependent dB step
/// grid; when the fixed highpass is enabled its response curve is plotted too.
#[cfg(feature = "spectrogram")]
pub fn spectrum_view(
    ui: &mut egui::Ui,
    spectrum: Option<&frx_core::spectrum::Spectrum>,
    floor_db: f32,
    height: f32,
    // When the fixed highpass is enabled: (cutoff Hz, sample rate); its
    // one-pole magnitude response is plotted over the spectrum.
    highpass: Option<(f32, u32)>,
    sample_rate: u32,
) -> Response {
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, l::PLOT_RADIUS, theme::PANEL);
    let pad = l::SPACE_SMALL;
    let inner = egui::Rect::from_min_size(
        rect.left_top() + vec2(pad + 14.0, pad),
        vec2(
            (rect.width() - 2.0 * pad - 14.0).max(10.0),
            (rect.height() - 2.0 * pad).max(10.0),
        ),
    );
    let nyquist = (sample_rate.max(1) as f32 / 2.0).max(c::SPECTRUM_MIN_HZ * 2.0);
    let f_min = c::SPECTRUM_MIN_HZ.min(nyquist / 2.0);
    let x_of = |hz: f32| -> f32 {
        let f = hz.clamp(f_min, nyquist);
        inner.left()
            + (f.log10() - f_min.log10()) / (nyquist.log10() - f_min.log10()) * inner.width()
    };
    let y_of = |db: f32| {
        inner.bottom()
            - ((db - floor_db) / (c::SPECTRAL_CEILING_DB - floor_db)).clamp(0.0, 1.0)
                * inner.height()
    };
    // Horizontal dB grid: the step adapts to the current panel height.
    let step = db_grid_step(height, floor_db, c::SPECTRAL_CEILING_DB);
    let mut db = (floor_db / step).ceil() * step;
    while db <= c::SPECTRAL_CEILING_DB + 1e-6 {
        let y = y_of(db);
        painter.line_segment(
            [pos2(inner.left(), y), pos2(inner.right(), y)],
            Stroke::new(l::THIN_STROKE, theme::RAISED),
        );
        painter.text(
            pos2(rect.left() + 2.0, y),
            egui::Align2::LEFT_CENTER,
            format!("{db:.0}"),
            egui::FontId::monospace(l::MICRO_FONT),
            theme::MUTED,
        );
        db += step;
    }
    // Vertical 1/3-octave center-frequency grid.
    for hz in frx_core::spectrum::third_octave_centers(nyquist) {
        let x = x_of(hz);
        painter.line_segment(
            [pos2(x, inner.top()), pos2(x, inner.bottom())],
            Stroke::new(l::THIN_STROKE, theme::RAISED),
        );
        painter.text(
            pos2(x, rect.bottom() - 1.0),
            egui::Align2::CENTER_BOTTOM,
            format_hz(hz),
            egui::FontId::monospace(l::MICRO_FONT),
            theme::MUTED,
        );
    }
    if let Some(spectrum) = spectrum {
        let bins = spectrum.dbfs.len();
        if bins > 1 {
            let mut previous = None;
            for (i, &db) in spectrum.dbfs.iter().enumerate() {
                let hz = (i as f32 + 0.5) * spectrum.hz_per_bin;
                let point = pos2(x_of(hz), y_of(db));
                if let Some(p) = previous {
                    painter.line_segment([p, point], Stroke::new(l::THIN_STROKE, theme::AMBER));
                }
                previous = Some(point);
            }
        }
        // Highpass filter response |H(f)| for y = a*(y + x - x_prev).
        if let Some((cutoff_hz, fs)) = highpass {
            let fs = fs.max(1) as f32;
            let a = (-std::f32::consts::TAU * cutoff_hz / fs).exp();
            let mut previous = None;
            for i in 0..bins.max(1) {
                let hz = (i as f32 + 0.5) * spectrum.hz_per_bin;
                let w = std::f32::consts::TAU * hz / fs;
                let mag = a * (2.0 * (w / 2.0).sin()) / (1.0 - 2.0 * a * w.cos() + a * a).sqrt();
                let db = 20.0 * mag.max(1e-9).log10();
                let point = pos2(x_of(hz), y_of(db));
                if let Some(p) = previous {
                    painter.line_segment([p, point], Stroke::new(l::THIN_STROKE, theme::PURPLE));
                }
                previous = Some(point);
            }
        }
    } else {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "Spectrum",
            egui::FontId::proportional(l::BODY_FONT),
            theme::MUTED,
        );
    }
    painter.text(
        rect.right_top() + vec2(-pad, pad),
        egui::Align2::RIGHT_TOP,
        format!("{:.0} dBFS top", c::SPECTRAL_CEILING_DB),
        egui::FontId::monospace(l::MICRO_FONT),
        theme::MUTED,
    );
    if let Some((cutoff_hz, _)) = highpass {
        painter.text(
            rect.left_top() + vec2(pad + 14.0, pad),
            egui::Align2::LEFT_TOP,
            format!("HPF {cutoff_hz:.0} Hz"),
            egui::FontId::monospace(l::MICRO_FONT),
            theme::PURPLE,
        );
    }
    response.on_hover_text(if highpass.is_some() {
        "Amber: magnitude spectrum at the playhead (log frequency, 1/3-octave grid). Purple: fixed highpass response (enabled)."
    } else {
        "Magnitude spectrum at the playhead, dBFS, log frequency with 1/3-octave grid; dB grid step adapts to the panel height."
    })
}

/// Pick the smallest dB grid step that still leaves enough pixels between
/// lines at the current panel height (height-dependent dB steps).
#[cfg(feature = "spectrogram")]
pub fn db_grid_step(height: f32, floor_db: f32, ceiling_db: f32) -> f32 {
    let px_per_db = (height - 2.0 * l::SPACE_SMALL) / (ceiling_db - floor_db).max(1.0);
    c::SPECTRUM_DB_STEPS
        .iter()
        .copied()
        .find(|step| step * px_per_db >= c::SPECTRUM_MIN_GRID_PX)
        .unwrap_or_else(|| *c::SPECTRUM_DB_STEPS.last().unwrap())
}

/// Compact frequency label for the 1/3-octave grid: 31.5, 1k, 10k.
#[cfg(feature = "spectrogram")]
pub fn format_hz(hz: f32) -> String {
    if hz >= 1000.0 {
        let khz = hz / 1000.0;
        if (khz - khz.round()).abs() < 0.05 {
            format!("{}k", khz.round() as u32)
        } else {
            format!("{khz:.1}k")
        }
    } else if (hz - hz.round()).abs() < 0.05 {
        format!("{}", hz.round() as u32)
    } else {
        format!("{hz:.1}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "spectrogram")]
    #[test]
    fn db_grid_step_depends_on_panel_height() {
        // Short panel: only coarse steps fit -> 20 dB. Tall panel: fine steps.
        assert_eq!(db_grid_step(130.0, -100.0, 0.0), 20.0);
        assert_eq!(db_grid_step(380.0, -100.0, 0.0), 10.0);
        assert!(db_grid_step(60.0, -100.0, 0.0) >= db_grid_step(380.0, -100.0, 0.0));
        assert_eq!(format_hz(1000.0), "1k");
        assert_eq!(format_hz(10000.0), "10k");
        assert_eq!(format_hz(31.5), "31.5");
        assert_eq!(format_hz(250.0), "250");
    }

    #[cfg(feature = "spectrogram")]
    #[test]
    fn spectrum_handle_sits_directly_below_the_spectrum_view() {
        let ctx = egui::Context::default();
        let mut spectrum_rect = egui::Rect::NOTHING;
        let mut handle_rect = egui::Rect::NOTHING;
        let _ = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                spectrum_rect = spectrum_view(ui, None, -100.0, 130.0, None, 48000).rect;
                let mut h = 130.0f32;
                handle_rect =
                    height_handle(ui, &mut h, l::PLOT_HEIGHT_MIN, l::PLOT_MAX_HEIGHT).rect;
            });
        });
        assert!(spectrum_rect.width() > 100.0);
        assert!(handle_rect.top() >= spectrum_rect.bottom() - 1.0);
    }

    #[test]
    fn bottom_handle_drag_down_expands_and_drag_up_compacts() {
        // Screen y grows downward: dragging the lower handle down (positive
        // delta) must grow the plot, dragging up must shrink it.
        assert_eq!(drag_height(100.0, 40.0, 60.0, 380.0), 140.0);
        assert_eq!(drag_height(100.0, -40.0, 60.0, 380.0), 60.0);
        assert_eq!(drag_height(370.0, 40.0, 60.0, 380.0), 380.0);
    }

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
