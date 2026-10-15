//! Wygląd: ciemny motyw z akcentem w kolorze nieba.

use eframe::egui::{self, Color32, CornerRadius, FontFamily, FontId, Stroke, TextStyle, Visuals};

pub const BG: Color32 = Color32::from_rgb(9, 12, 18);
pub const PANEL: Color32 = Color32::from_rgb(17, 21, 29);
pub const PANEL_2: Color32 = Color32::from_rgb(24, 29, 39);
pub const ACCENT: Color32 = Color32::from_rgb(56, 189, 248);
pub const ACCENT_DIM: Color32 = Color32::from_rgb(14, 86, 122);
pub const TEXT: Color32 = Color32::from_rgb(220, 228, 238);
pub const TEXT_DIM: Color32 = Color32::from_rgb(140, 152, 170);
pub const BODY_FILL: Color32 = Color32::from_rgb(38, 46, 60);
pub const BODY_EDGE: Color32 = Color32::from_rgb(150, 170, 196);
pub const FIELD_OFF: Color32 = Color32::from_rgb(12, 16, 24);
pub const GOOD: Color32 = Color32::from_rgb(74, 222, 128);
pub const WARN: Color32 = Color32::from_rgb(250, 204, 21);
pub const BAD: Color32 = Color32::from_rgb(248, 113, 113);
pub const DRAG: Color32 = Color32::from_rgb(251, 146, 60);
pub const LIFT: Color32 = Color32::from_rgb(56, 189, 248);

pub fn apply(ctx: &egui::Context) {
    let mut v = Visuals::dark();
    v.panel_fill = PANEL;
    v.window_fill = PANEL;
    v.extreme_bg_color = Color32::from_rgb(11, 14, 20);
    v.faint_bg_color = PANEL_2;
    v.override_text_color = Some(TEXT);
    v.hyperlink_color = ACCENT;
    v.selection.bg_fill = ACCENT_DIM;
    v.selection.stroke = Stroke::new(1.0_f32, ACCENT);
    v.window_corner_radius = CornerRadius::same(10);
    v.menu_corner_radius = CornerRadius::same(8);
    v.window_stroke = Stroke::new(1.0_f32, Color32::from_rgb(40, 48, 62));
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, Color32::from_rgb(36, 43, 56));
    v.widgets.noninteractive.bg_fill = PANEL;
    v.widgets.inactive.bg_fill = PANEL_2;
    v.widgets.inactive.weak_bg_fill = PANEL_2;
    v.widgets.hovered.bg_fill = Color32::from_rgb(34, 42, 56);
    v.widgets.hovered.weak_bg_fill = Color32::from_rgb(34, 42, 56);
    v.widgets.active.bg_fill = ACCENT_DIM;
    v.widgets.active.weak_bg_fill = ACCENT_DIM;
    for w in [&mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
        w.corner_radius = CornerRadius::same(6);
    }
    ctx.set_visuals(v);
    let mut fonts = egui::FontDefinitions::default();
    if let Some(list) = fonts.families.get_mut(&FontFamily::Proportional) {
        // Hack zawiera m.in. ⁻, →, ◆ — brakujące w Ubuntu
        list.push("Hack".into());
    }
    ctx.set_fonts(fonts);
    ctx.style_mut(|s| {
        s.spacing.item_spacing = egui::vec2(8.0, 6.0);
        s.spacing.button_padding = egui::vec2(8.0, 4.0);
        s.spacing.slider_width = 150.0;
        s.spacing.combo_width = 150.0;
        s.text_styles.insert(TextStyle::Heading, FontId::new(17.0, FontFamily::Proportional));
        s.text_styles.insert(TextStyle::Body, FontId::new(13.5, FontFamily::Proportional));
        s.text_styles.insert(TextStyle::Button, FontId::new(13.5, FontFamily::Proportional));
        s.text_styles.insert(TextStyle::Small, FontId::new(11.0, FontFamily::Proportional));
        s.text_styles.insert(TextStyle::Monospace, FontId::new(12.5, FontFamily::Monospace));
    });
}

/// Nagłówek sekcji panelu.
pub fn section(ui: &mut egui::Ui, title: &str) {
    ui.add_space(6.0);
    ui.label(egui::RichText::new(title.to_uppercase()).size(11.0).color(TEXT_DIM).strong());
    ui.add_space(2.0);
}
