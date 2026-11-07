//! Stateless paint/input helpers. Shared by the normal UI and headless egui tests.
use crate::{
    config::{controls as c, layout as l},
    model::TimeWindow,
    theme,
};
use eframe::egui::{self, Response, RichText, Sense, Stroke, pos2, vec2};
use std::ops::RangeInclusive;
use viewfrx_audio::Status;
use viewfrx_core::{annotations::Annotation, waveform::Waveform};

/// Width after a horizontal drag: egui drag deltas grow to the right on
/// screen, so dragging the split bar right grows the left column.
#[cfg(feature = "spectrogram")]
pub fn drag_width(value: f32, delta_x: f32, min: f32, max: f32) -> f32 {
    (value + delta_x).clamp(min, max)
}

/// Vertical resize bar between two side-by-side columns: dragging it
/// horizontally redistributes width between the left and right column,
/// keeping their sum constant. Dragging right grows the left column.
#[cfg(feature = "spectrogram")]
pub fn vsplit_handle(
    ui: &mut egui::Ui,
    left: &mut f32,
    right: &mut f32,
    min_left: f32,
    min_right: f32,
    height: f32,
) -> Response {
    let total = *left + *right;
    let (rect, response) = ui.allocate_exact_size(
        vec2(l::WIDTH_HANDLE, height.max(l::HEIGHT_HANDLE)),
        Sense::drag(),
    );
    let response = response
        .on_hover_cursor(egui::CursorIcon::ResizeHorizontal)
        .on_hover_text("Drag to redistribute width between the annotations and spectrum columns");
    if response.dragged() {
        *left = drag_width(*left, response.drag_delta().x, min_left, total - min_right);
        *right = (total - *left).max(min_right);
    }
    let painter = ui.painter_at(rect);
    if response.hovered() || response.dragged() {
        painter.rect_filled(rect, 0, theme::RAISED);
    }
    let grip = egui::Rect::from_center_size(rect.center(), vec2(3.0, 46.0));
    painter.rect_filled(
        grip,
        2,
        if response.dragged() {
            theme::AMBER
        } else {
            theme::MUTED
        },
    );
    response
}

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
    response.on_hover_text(format!("Stereo RMS envelope ({:.0} ms exponential time constant), with decaying sample-peak markers. Post-gain, post-clamp dBFS. VU-style display, not IEC-calibrated VU.", viewfrx_core::config::RMS_TIME_CONSTANT_SECONDS * 1000.0));
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
        PlotOverlays {
            annotations: &[],
            play_start: None,
        },
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
        // Small tag-like symbol on top of the line.
        let tip = pos2(x, rect.top() + l::ANNOTATION_TAG_H);
        let left = pos2(x - l::ANNOTATION_TAG_W / 2.0, rect.top());
        let right = pos2(x + l::ANNOTATION_TAG_W / 2.0, rect.top());
        painter.add(egui::Shape::convex_polygon(
            vec![left, right, tip],
            theme::ANNOTATION,
            Stroke::new(l::THIN_STROKE, theme::PANEL),
        ));
    }
}

/// Hover preview: when the pointer is near an annotation marker, show its
/// content in a tooltip. Call from plot views that have a hovered response.
pub fn annotation_hover_preview(
    response: &egui::Response,
    rect: egui::Rect,
    annotations: &[Annotation],
    window: TimeWindow,
) {
    if !response.hovered() || window.len() <= 0.0 {
        return;
    }
    let Some(pointer) = response.ctx.input(|i| i.pointer.latest_pos()) else {
        return;
    };
    if !rect.contains(pointer) {
        return;
    }
    let nearest = annotations.iter().min_by_key(|a| {
        let x = rect.left() + window.time_to_fraction(a.seconds) * rect.width();
        ((x - pointer.x).abs() * 1000.0) as i64
    });
    if let Some(a) = nearest {
        let x = rect.left() + window.time_to_fraction(a.seconds) * rect.width();
        if (x - pointer.x).abs() <= l::ANNOTATION_HOVER_PX {
            let timestamp = viewfrx_core::annotations::format_timestamp(a.seconds);
            let body = if a.text.is_empty() {
                format!("{timestamp} (no text yet)")
            } else {
                format!("{timestamp} {}", a.text)
            };
            response.clone().on_hover_ui_at_pointer(|ui| {
                ui.label(RichText::new(body).monospace());
            });
        }
    }
}

/// Per-plot overlay lines: annotation markers and the last play-start marker.
pub struct PlotOverlays<'a> {
    pub annotations: &'a [Annotation],
    pub play_start: Option<f64>,
}

pub fn waveform_channel_view(
    ui: &mut egui::Ui,
    waveform: Option<&Waveform>,
    position: f64,
    window: TimeWindow,
    height: f32,
    channel: Option<usize>,
    overlays: PlotOverlays<'_>,
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
        let normalized = waveform.normalization().map_or(1.0, |n| n.gain);
        // Two passes guarantee the original is in front, never painted over by
        // the quieter normalization preview. Both are source-relative, not live gain.
        // X positions map each bin's center time through the shared zoom
        // window, so waveform content, playhead and annotation lines align.
        for (gain, color) in [(normalized, theme::NORMALIZED_WAVE), (1.0, theme::AMBER)] {
            for (i, p) in visible.iter().enumerate() {
                let center_time = (first + i) as f64 * bin_seconds + bin_seconds / 2.0;
                if center_time < window.start || center_time > window.end {
                    continue;
                }
                let x = rect.left() + window.time_to_fraction(center_time) * rect.width();
                let (min, max) = (p.min - dc, p.max - dc);
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
    annotation_lines(&painter, rect, overlays.annotations, window);
    if let Some(start) = overlays.play_start {
        start_marker(&painter, rect, start, window);
    }
    playhead(&painter, rect, position, window);
    annotation_hover_preview(&response, rect, overlays.annotations, window);
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Grey vertical line at the position where playback was last started.
pub fn start_marker(painter: &egui::Painter, rect: egui::Rect, seconds: f64, window: TimeWindow) {
    if window.len() > 0.0 {
        let x = rect.left() + window.time_to_fraction(seconds) * rect.width();
        painter.line_segment(
            [pos2(x, rect.top()), pos2(x, rect.bottom())],
            Stroke::new(l::THIN_STROKE, theme::PLAY_START),
        );
    }
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
    spectrum: Option<&viewfrx_core::spectrum::Spectrum>,
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
    for hz in viewfrx_core::spectrum::third_octave_centers(nyquist) {
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

    #[cfg(feature = "spectrogram")]
    #[test]
    fn vsplit_drag_right_grows_left_column() {
        // Dragging the vertical bar right (positive x delta) widens the left
        // (annotations) column and shrinks the right (spectrum) column.
        assert_eq!(drag_width(300.0, 50.0, 220.0, 600.0), 350.0);
        assert_eq!(drag_width(300.0, -500.0, 220.0, 600.0), 220.0);
        // Redistribution keeps the sum constant and respects both minimums.
        let total = 300.0f32 + 400.0f32;
        let mut left = 300.0f32;
        left = drag_width(left, 1000.0, 220.0, total - 260.0);
        let right = (total - left).max(260.0);
        assert_eq!(right, 260.0);
        assert_eq!(left + right, total);
    }

    #[test]
    fn annotation_lines_align_with_the_zoom_window() {
        use viewfrx_core::waveform::{Peak, Waveform};
        let ctx = egui::Context::default();
        let wave = Waveform {
            sample_rate: 8000,
            frames: 8000,
            frames_per_bin: 1000,
            peaks: vec![
                Peak {
                    min: -0.5,
                    max: 0.5
                };
                8
            ],
            channel_peaks: vec![vec![
                Peak {
                    min: -0.5,
                    max: 0.5
                };
                8
            ]],
            dc_offset: vec![0.0],
        };
        let window = TimeWindow {
            start: 2.0,
            end: 6.0,
        };
        let annotation = || Annotation {
            seconds: 4.0,
            text: "x".into(),
        };
        let mut rect = egui::Rect::NOTHING;
        let output = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                rect = waveform_channel_view(
                    ui,
                    Some(&wave),
                    4.0,
                    window,
                    100.0,
                    Some(0),
                    PlotOverlays {
                        annotations: &[annotation()],
                        play_start: None,
                    },
                )
                .rect;
            });
        });
        // t=4 s is the center of window [2,6]: the cyan marker line must sit at
        // the plot's horizontal center (same mapping as the playhead).
        let expected_x = rect.center().x;
        let found = output.shapes.iter().any(|shape| {
            if let egui::Shape::LineSegment { points, stroke } = &shape.shape {
                stroke.color == theme::ANNOTATION
                    && (points[0].x - expected_x).abs() < 0.5
                    && (points[1].x - expected_x).abs() < 0.5
            } else {
                false
            }
        });
        assert!(
            found,
            "annotation line must be at x={expected_x} in {rect:?}"
        );
        // And the playhead (purple) at the same position t=4 s.
        let playhead = output.shapes.iter().any(|shape| {
            if let egui::Shape::LineSegment { points, stroke } = &shape.shape {
                stroke.color == theme::PURPLE && (points[0].x - expected_x).abs() < 0.5
            } else {
                false
            }
        });
        assert!(playhead, "playhead must align with the annotation line");
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
