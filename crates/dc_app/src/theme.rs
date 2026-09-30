//! Compact neutral workspace inspired by professional creative applications.
use egui::{Color32, Context, FontId, Margin, Rounding, Stroke, TextStyle, Visuals};
pub const PANEL: Color32 = Color32::from_rgb(45, 45, 47);
pub const BAR: Color32 = Color32::from_rgb(36, 36, 38);
pub const CANVAS: Color32 = Color32::from_rgb(28, 28, 30);
pub const BORDER: Color32 = Color32::from_rgb(62, 62, 65);
pub const TEXT: Color32 = Color32::from_rgb(226, 226, 229);
pub const MUTED: Color32 = Color32::from_rgb(153, 153, 160);
pub const ACCENT: Color32 = Color32::from_rgb(88, 157, 246);
pub fn configure(ctx: &Context) {
    let mut style = (*ctx.style()).clone();
    let mut visuals = Visuals::dark();
    visuals.panel_fill = PANEL;
    visuals.window_fill = PANEL;
    visuals.extreme_bg_color = Color32::from_rgb(32, 32, 34);
    visuals.faint_bg_color = Color32::from_rgb(51, 51, 54);
    visuals.override_text_color = Some(TEXT);
    visuals.window_stroke = Stroke::new(1.0_f32, BORDER);
    visuals.window_rounding = Rounding::same(6.0);
    visuals.selection.bg_fill = Color32::from_rgb(54, 74, 101);
    visuals.selection.stroke = Stroke::new(1.0_f32, ACCENT);
    visuals.widgets.noninteractive.bg_fill = PANEL;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, BORDER);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, MUTED);
    visuals.widgets.inactive.bg_fill = Color32::from_rgb(57, 57, 61);
    visuals.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, Color32::from_rgb(74, 74, 80));
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, TEXT);
    visuals.widgets.hovered.bg_fill = Color32::from_rgb(69, 69, 75);
    visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(59, 59, 64);
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, Color32::from_rgb(109, 109, 119));
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, Color32::WHITE);
    visuals.widgets.active.bg_fill = Color32::from_rgb(55, 82, 116);
    visuals.widgets.active.weak_bg_fill = Color32::from_rgb(55, 82, 116);
    visuals.widgets.active.bg_stroke = Stroke::new(1.0_f32, ACCENT);
    visuals.widgets.active.fg_stroke = Stroke::new(1.0_f32, Color32::WHITE);
    for widget in [
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.noninteractive,
    ] {
        widget.rounding = Rounding::same(3.0);
        widget.expansion = 0.0;
    }
    style.visuals = visuals;
    style
        .text_styles
        .insert(TextStyle::Body, FontId::proportional(12.0));
    style
        .text_styles
        .insert(TextStyle::Button, FontId::proportional(12.0));
    style
        .text_styles
        .insert(TextStyle::Heading, FontId::proportional(14.0));
    style
        .text_styles
        .insert(TextStyle::Small, FontId::proportional(10.5));
    style.spacing.item_spacing = egui::vec2(7.0, 7.0);
    style.spacing.button_padding = egui::vec2(9.0, 5.0);
    style.spacing.interact_size = egui::vec2(28.0, 26.0);
    style.spacing.slider_width = 108.0;
    style.spacing.window_margin = Margin::same(14.0);
    ctx.set_style(style);
}
pub fn dock_style(ctx: &Context) -> egui_dock::Style {
    let mut style = egui_dock::Style::from_egui(ctx.style().as_ref());
    style.main_surface_border_stroke = Stroke::NONE;
    style.main_surface_border_rounding = Rounding::ZERO;
    style.dock_area_padding = Some(Margin::ZERO);
    style.separator.width = 1.0;
    style.separator.color_idle = Color32::from_rgb(23, 23, 25);
    style.separator.color_hovered = ACCENT;
    style.separator.color_dragged = ACCENT;
    style.tab_bar.height = 30.0;
    style.tab_bar.bg_fill = BAR;
    style.tab_bar.hline_color = BORDER;
    style.tab_bar.rounding = Rounding::ZERO;
    for tab in [
        &mut style.tab.active,
        &mut style.tab.focused,
        &mut style.tab.hovered,
        &mut style.tab.inactive,
    ] {
        tab.outline_color = Color32::TRANSPARENT;
        tab.rounding = Rounding::ZERO;
        tab.text_color = MUTED;
        tab.bg_fill = BAR;
    }
    style.tab.active.bg_fill = PANEL;
    style.tab.active.text_color = TEXT;
    style.tab.focused.bg_fill = PANEL;
    style.tab.focused.text_color = TEXT;
    style.tab.hovered.bg_fill = Color32::from_rgb(51, 51, 54);
    style.tab.hovered.text_color = TEXT;
    style.tab.tab_body.bg_fill = PANEL;
    style.tab.tab_body.stroke = Stroke::NONE;
    style.tab.tab_body.rounding = Rounding::ZERO;
    style.tab.tab_body.inner_margin = Margin::same(10.0);
    style
}
