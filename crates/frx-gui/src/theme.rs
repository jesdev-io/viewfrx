use eframe::egui::{self, Color32};
pub const BG: Color32 = Color32::from_rgb(15, 16, 20);
pub const PANEL: Color32 = Color32::from_rgb(24, 26, 33);
pub const RAISED: Color32 = Color32::from_rgb(34, 38, 49);
pub const AMBER: Color32 = Color32::from_rgb(255, 179, 0);
pub const MUTED: Color32 = Color32::from_rgb(184, 190, 200);
pub const PURPLE: Color32 = Color32::from_rgb(161, 112, 217);

pub fn apply(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = PANEL;
    visuals.window_fill = PANEL;
    visuals.extreme_bg_color = BG;
    visuals.faint_bg_color = RAISED;
    visuals.override_text_color = Some(Color32::from_rgb(237, 241, 245));
    visuals.selection.bg_fill = Color32::from_rgb(96, 68, 12);
    visuals.selection.stroke = egui::Stroke::new(1.0, AMBER);
    visuals.widgets.active.bg_fill = Color32::from_rgb(96, 68, 12);
    visuals.widgets.hovered.bg_fill = RAISED;
    visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, AMBER);
    ctx.set_visuals(visuals);
    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(10.0, 8.0);
    style.spacing.button_padding = egui::vec2(12.0, 7.0);
    ctx.set_style(style);
}

pub fn logo() -> image::RgbaImage {
    image::load_from_memory(include_bytes!("../assets/jesdev.webp"))
        .expect("bundled JESDEV logo")
        .into_rgba8()
}
