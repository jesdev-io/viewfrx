//! Spectrogram presentation. FFT/normalization live in frx-core; color conversion
//! runs on the analysis worker, texture upload/painting on the GUI thread.
use crate::{
    config::{controls as c, layout as l},
    model::TimeWindow,
    theme, widgets,
};
use eframe::egui::{self, Color32, Sense, pos2, vec2};
use frx_core::{
    annotations::Annotation,
    config::spectral as dsp,
    spectrogram::{Settings, Spectrogram},
};

#[derive(Clone, Copy, Debug)]
pub struct RenderSettings {
    pub analysis: Settings,
    pub floor_db: f32,
}
pub struct RenderedSpectrum {
    pub image: egui::ColorImage,
    pub right: Option<egui::ColorImage>,
    pub nyquist_hz: f32,
    /// Seconds between spectrogram columns (hop / sample rate).
    pub column_seconds: f64,
}

pub fn color(db: f32, floor: f32) -> Color32 {
    let fraction = ((db - floor) / (c::SPECTRAL_CEILING_DB - floor)).clamp(0.0, 1.0);
    let last = theme::SPECTRAL_PALETTE.len() - 1;
    let at = fraction * last as f32;
    let index = (at.floor() as usize).min(last - 1);
    let mix = at - index as f32;
    let a = theme::SPECTRAL_PALETTE[index].to_array();
    let b = theme::SPECTRAL_PALETTE[index + 1].to_array();
    let rgb: [u8; 3] =
        std::array::from_fn(|i| (a[i] as f32 + (b[i] as f32 - a[i] as f32) * mix).round() as u8);
    Color32::from_rgb(rgb[0], rgb[1], rgb[2])
}

pub fn render(data: Spectrogram, settings: RenderSettings) -> Option<RenderedSpectrum> {
    if data.columns == 0 {
        return None;
    }
    let pixels = (0..data.frequency_bins)
        .flat_map(|y| {
            let data = &data;
            (0..data.columns).map(move |x| {
                color(
                    data.dbfs[x * data.frequency_bins + data.frequency_bins - 1 - y],
                    settings.floor_db,
                )
            })
        })
        .collect();
    Some(RenderedSpectrum {
        image: egui::ColorImage::new([data.columns, data.frequency_bins], pixels),
        right: None,
        nyquist_hz: data.sample_rate as f32 / 2.0,
        column_seconds: data.hop_size as f64 / data.sample_rate.max(1) as f64,
    })
}

pub struct View {
    pub enabled: bool,
    pub pending: bool,
    fft_size: usize,
    floor_db: f64,
    texture: Option<egui::TextureHandle>,
    right: Option<egui::TextureHandle>,
    pub channels: widgets::Channels,
    nyquist_hz: f32,
    column_seconds: f64,
}
impl Default for View {
    fn default() -> Self {
        Self {
            enabled: true,
            pending: false,
            fft_size: dsp::DEFAULT_FFT_SIZE,
            floor_db: c::SPECTRAL_FLOOR_DEFAULT,
            texture: None,
            right: None,
            channels: Default::default(),
            nyquist_hz: 0.0,
            column_seconds: 0.0,
        }
    }
}
impl View {
    pub fn fft_size(&self) -> usize {
        self.fft_size
    }
    pub fn floor_db(&self) -> f64 {
        self.floor_db
    }
    pub fn settings(&self) -> Option<RenderSettings> {
        self.enabled.then_some(RenderSettings {
            analysis: Settings {
                fft_size: self.fft_size,
            },
            floor_db: self.floor_db as f32,
        })
    }
    pub fn clear(&mut self) {
        self.texture = None;
        self.right = None;
        self.pending = self.enabled;
    }
    pub fn controls(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed =
            widgets::reset_toggle(ui, &mut self.enabled, true, "Spectrogram").changed();
        if self.enabled {
            let previous = self.fft_size;
            let combo = egui::ComboBox::from_id_salt("fft_size")
                .selected_text(format!("FFT: {} samples", self.fft_size))
                .show_ui(ui, |ui| {
                    for &size in dsp::FFT_SIZES {
                        ui.selectable_value(&mut self.fft_size, size, size.to_string());
                    }
                })
                .response
                .on_hover_text(
                    "Periodic Hann window, 75% overlap. Double-click to reset FFT size.",
                );
            if combo.double_clicked() {
                self.fft_size = dsp::DEFAULT_FFT_SIZE;
            }
            changed |= previous != self.fft_size;
            ui.small("Display floor (dBFS)");
            let floor = widgets::reset_slider(
                ui,
                &mut self.floor_db,
                c::SPECTRAL_FLOOR_MIN..=c::SPECTRAL_FLOOR_MAX,
                c::SPECTRAL_FLOOR_DEFAULT,
                " dBFS",
            );
            changed |= floor.drag_stopped() || (floor.changed() && !floor.dragged());
        }
        changed
    }
    pub fn accept(
        &mut self,
        ctx: &egui::Context,
        result: Result<Option<RenderedSpectrum>, String>,
    ) -> Result<(), String> {
        self.pending = false;
        let rendered = result?;
        self.right = None;
        self.texture = rendered.map(|rendered| {
            self.right = rendered.right.map(|image| {
                ctx.load_texture("spectrogram-right", image, egui::TextureOptions::LINEAR)
            });
            self.nyquist_hz = rendered.nyquist_hz;
            self.column_seconds = rendered.column_seconds;
            ctx.load_texture("spectrogram", rendered.image, egui::TextureOptions::LINEAR)
        });
        Ok(())
    }
    pub fn has_image(&self) -> bool {
        self.texture.is_some()
    }
    pub fn paint(
        &self,
        ui: &mut egui::Ui,
        position: f64,
        window: TimeWindow,
        height: f32,
        channel: usize,
        annotations: &[Annotation],
    ) -> egui::Response {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(if self.right.is_some() {
                    if channel == 0 {
                        "SPEC L · dBFS"
                    } else {
                        "SPEC R · dBFS"
                    }
                } else {
                    "SPECTROGRAM · dBFS"
                })
                .size(l::SMALL_FONT)
                .color(theme::MUTED),
            );
            if self.pending {
                ui.spinner();
            }
        });
        let (rect, response) =
            ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, l::PLOT_RADIUS, theme::PANEL);
        if let Some(texture) = if channel == 0 {
            &self.texture
        } else {
            &self.right
        } {
            // Zoom window: map the source-time window exactly onto the
            // texture's column axis (column k is centered at k*hop), so the
            // spectrogram, playhead and annotation lines all align.
            let columns = texture.size()[0].max(1) as f64;
            let col_seconds = self.column_seconds.max(1e-12);
            let u0 = (window.start / col_seconds / columns).clamp(0.0, 1.0);
            let u1 = (window.end / col_seconds / columns).clamp(u0 + 1e-6, 1.0);
            let uv = egui::Rect::from_min_max(pos2(u0 as f32, 0.0), pos2(u1 as f32, 1.0));
            painter.image(texture.id(), rect, uv, Color32::WHITE);
            painter.text(
                rect.left_top() + vec2(l::SPACE_SMALL, l::SPACE_SMALL),
                egui::Align2::LEFT_TOP,
                format!("{:.1} kHz", self.nyquist_hz / 1000.0),
                egui::FontId::monospace(l::SMALL_FONT),
                theme::TEXT,
            );
            painter.text(
                rect.left_bottom() + vec2(l::SPACE_SMALL, -l::SPACE_SMALL),
                egui::Align2::LEFT_BOTTOM,
                "0 Hz",
                egui::FontId::monospace(l::SMALL_FONT),
                theme::TEXT,
            );
            painter.text(
                rect.right_top() + vec2(-l::SPACE_SMALL, l::SPACE_SMALL),
                egui::Align2::RIGHT_TOP,
                format!("{:.0} … 0 dBFS", self.floor_db),
                egui::FontId::monospace(l::SMALL_FONT),
                theme::TEXT,
            );
        } else {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                if self.pending {
                    "Spectral analysis in background…"
                } else {
                    "No spectral data"
                },
                egui::FontId::proportional(l::BODY_FONT),
                theme::MUTED,
            );
        }
        widgets::annotation_lines(&painter, rect, annotations, window);
        widgets::playhead(&painter, rect, position, window);
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn frequency_orientation_and_palette_limits_are_correct() {
        let settings = RenderSettings {
            analysis: Settings::default(),
            floor_db: -100.0,
        };
        let data = Spectrogram {
            sample_rate: 48000,
            frames: 1,
            fft_size: 2048,
            hop_size: 512,
            windows_per_column: 1,
            columns: 1,
            frequency_bins: 2,
            dbfs: vec![-160.0, 0.0],
        };
        let image = render(data, settings).unwrap().image;
        assert_eq!(image.pixels[0], *theme::SPECTRAL_PALETTE.last().unwrap());
        assert_eq!(image.pixels[1], theme::SPECTRAL_PALETTE[0]);
    }
}
