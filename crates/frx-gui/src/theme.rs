use crate::config::layout as l;
use eframe::egui::{self, Color32};
pub const BG: Color32 = Color32::from_rgb(15, 16, 20);
pub const PANEL: Color32 = Color32::from_rgb(24, 26, 33);
pub const RAISED: Color32 = Color32::from_rgb(34, 38, 49);
pub const AMBER: Color32 = Color32::from_rgb(255, 179, 0);
pub const MUTED: Color32 = Color32::from_rgb(184, 190, 200);
pub const PURPLE: Color32 = Color32::from_rgb(161, 112, 217);
pub const ANNOTATION: Color32 = Color32::from_rgb(64, 200, 200);
/// Grey line marking where playback was last started.
pub const PLAY_START: Color32 = Color32::from_rgb(120, 124, 132);
pub const NORMALIZED_WAVE: Color32 = Color32::from_rgb(70, 78, 105);
pub const METER_GREEN: Color32 = Color32::from_rgb(70, 185, 136);
pub const ALERT: Color32 = Color32::from_rgb(255, 132, 113);
pub const TEXT: Color32 = Color32::from_rgb(237, 241, 245);
pub const SELECTED: Color32 = Color32::from_rgb(96, 68, 12);
#[cfg(feature = "spectrogram")]
pub const SPECTRAL_PALETTE: [Color32; 5] = [
    Color32::from_rgb(12, 12, 20),
    Color32::from_rgb(65, 22, 91),
    Color32::from_rgb(153, 40, 94),
    Color32::from_rgb(241, 115, 35),
    Color32::from_rgb(255, 244, 169),
];

pub fn apply(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = PANEL;
    visuals.window_fill = PANEL;
    visuals.extreme_bg_color = BG;
    visuals.faint_bg_color = RAISED;
    visuals.override_text_color = Some(TEXT);
    visuals.selection.bg_fill = SELECTED;
    visuals.selection.stroke = egui::Stroke::new(l::THIN_STROKE, AMBER);
    visuals.widgets.active.bg_fill = SELECTED;
    visuals.widgets.hovered.bg_fill = RAISED;
    visuals.widgets.hovered.bg_stroke = egui::Stroke::new(l::THIN_STROKE, AMBER);
    ctx.set_visuals(visuals);
    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = l::ITEM_SPACING.into();
    style.spacing.button_padding = l::BUTTON_PADDING.into();
    ctx.set_style(style);
}

pub fn logo() -> image::RgbaImage {
    image::load_from_memory(include_bytes!("../assets/jesdev.webp"))
        .expect("bundled JESDEV logo")
        .into_rgba8()
}
