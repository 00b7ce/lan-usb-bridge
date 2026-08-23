use eframe::egui::{self, Color32, CornerRadius, Stroke, Theme};

pub const GREEN: Color32 = Color32::from_rgb(65, 181, 116);
pub const BLUE: Color32 = Color32::from_rgb(71, 139, 235);
pub const YELLOW: Color32 = Color32::from_rgb(232, 184, 72);
pub const RED: Color32 = Color32::from_rgb(226, 86, 86);
pub const GRAY: Color32 = Color32::from_rgb(137, 143, 153);
pub const CARD: Color32 = Color32::from_rgb(35, 38, 45);

pub fn apply(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = Color32::from_rgb(24, 26, 31);
    visuals.window_fill = CARD;
    visuals.widgets.inactive.corner_radius = CornerRadius::same(6);
    visuals.widgets.hovered.corner_radius = CornerRadius::same(6);
    visuals.widgets.active.corner_radius = CornerRadius::same(6);
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, Color32::from_rgb(61, 65, 75));
    ctx.set_visuals_of(Theme::Dark, visuals);
    let mut style = (*ctx.style_of(Theme::Dark)).clone();
    style.spacing.item_spacing = egui::vec2(10.0, 8.0);
    style.spacing.button_padding = egui::vec2(14.0, 7.0);
    ctx.set_style_of(Theme::Dark, style);
}
