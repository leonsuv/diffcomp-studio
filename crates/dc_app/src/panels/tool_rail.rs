//! Persistent tools, with vector icons, active states and accessible names.
use crate::state::{AppState, ToolMode};
use crate::theme;
use dc_core::{Tool, ToolType};
use egui::{Color32, Pos2, Rect, Response, Stroke, Ui, Vec2};
#[derive(Clone, Copy)]
pub enum Icon {
    Select,
    Hand,
    Zoom,
    Pen,
    Rectangle,
    Ellipse,
    Line,
    Arrow,
    Text,
    Ruler,
    Count,
    Cloud,
    Eye,
    EyeOff,
    Open,
    Align,
    Compare,
    Fit,
}
pub fn icon_button(ui: &mut Ui, icon: Icon, label: &str, selected: bool) -> Response {
    let response = ui
        .add_sized(
            [32.0, 32.0],
            egui::Button::new("").selected(selected).frame(false),
        )
        .on_hover_text(label);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    if selected || response.hovered() {
        ui.painter().rect_filled(
            response.rect.shrink(1.0),
            3.0,
            if selected {
                Color32::from_rgb(57, 78, 107)
            } else {
                Color32::from_rgb(61, 61, 65)
            },
        );
    }
    paint_icon(
        ui.painter(),
        response.rect.shrink(8.0),
        icon,
        if selected { theme::ACCENT } else { theme::TEXT },
    );
    response
}
pub fn paint_icon(p: &egui::Painter, r: Rect, icon: Icon, color: Color32) {
    let stroke = Stroke::new(1.4_f32, color);
    let point = |x: f32, y: f32| Pos2::new(r.min.x + x * r.width(), r.min.y + y * r.height());
    let line = |a: (f32, f32), b: (f32, f32)| {
        p.line_segment([point(a.0, a.1), point(b.0, b.1)], stroke);
    };
    match icon {
        Icon::Eye | Icon::EyeOff => {
            p.add(egui::Shape::closed_line(
                vec![
                    point(0.0, 0.5),
                    point(0.25, 0.25),
                    point(0.5, 0.15),
                    point(0.75, 0.25),
                    point(1.0, 0.5),
                    point(0.75, 0.75),
                    point(0.5, 0.85),
                    point(0.25, 0.75),
                ],
                stroke,
            ));
            p.circle_stroke(r.center(), r.width() * 0.15, stroke);
            if matches!(icon, Icon::EyeOff) {
                line((0.0, 1.0), (1.0, 0.0));
            }
        }
        Icon::Select => {
            p.add(egui::Shape::closed_line(
                vec![
                    point(0.15, 0.0),
                    point(0.8, 0.65),
                    point(0.48, 0.65),
                    point(0.4, 1.0),
                ],
                stroke,
            ));
        }
        Icon::Hand => {
            p.add(egui::Shape::line(
                vec![
                    point(0.2, 0.6),
                    point(0.05, 0.45),
                    point(0.1, 0.35),
                    point(0.3, 0.45),
                    point(0.3, 0.1),
                    point(0.4, 0.1),
                    point(0.4, 0.45),
                    point(0.5, 0.0),
                    point(0.6, 0.05),
                    point(0.6, 0.45),
                    point(0.75, 0.1),
                    point(0.85, 0.15),
                    point(0.8, 0.5),
                    point(1.0, 0.35),
                    point(0.9, 0.75),
                    point(0.7, 1.0),
                    point(0.35, 1.0),
                    point(0.2, 0.6),
                ],
                stroke,
            ));
        }
        Icon::Zoom => {
            p.circle_stroke(point(0.4, 0.4), r.width() * 0.35, stroke);
            line((0.68, 0.68), (1.0, 1.0));
            line((0.2, 0.4), (0.6, 0.4));
            line((0.4, 0.2), (0.4, 0.6));
        }
        Icon::Rectangle => {
            p.rect_stroke(r.shrink(1.0), 0.0, stroke);
        }
        Icon::Ellipse => {
            p.circle_stroke(r.center(), r.width() * 0.45, stroke);
        }
        Icon::Line => line((0.1, 0.9), (0.9, 0.1)),
        Icon::Arrow => {
            line((0.0, 1.0), (1.0, 0.0));
            line((0.55, 0.0), (1.0, 0.0));
            line((1.0, 0.0), (1.0, 0.45));
        }
        Icon::Text => {
            line((0.1, 0.05), (0.9, 0.05));
            line((0.5, 0.05), (0.5, 1.0));
            line((0.3, 1.0), (0.7, 1.0));
        }
        Icon::Pen => {
            p.add(egui::Shape::closed_line(
                vec![
                    point(0.0, 1.0),
                    point(0.15, 0.55),
                    point(0.8, 0.0),
                    point(1.0, 0.2),
                    point(0.4, 0.85),
                ],
                stroke,
            ));
            line((0.15, 0.55), (0.4, 0.85));
        }
        Icon::Ruler => {
            p.rect_stroke(
                Rect::from_min_max(point(0.0, 0.2), point(1.0, 0.8)),
                0.0,
                stroke,
            );
            for i in 1..5 {
                let x = i as f32 / 5.0;
                line((x, 0.2), (x, if i % 2 == 0 { 0.6 } else { 0.45 }));
            }
        }
        Icon::Count => {
            p.circle_stroke(r.center(), r.width() * 0.47, stroke);
            p.text(
                r.center(),
                egui::Align2::CENTER_CENTER,
                "1",
                egui::FontId::proportional(13.0),
                color,
            );
        }
        Icon::Cloud => {
            for (x, y) in [
                (0.2, 0.5),
                (0.4, 0.25),
                (0.65, 0.25),
                (0.8, 0.5),
                (0.65, 0.75),
                (0.4, 0.75),
            ] {
                p.circle_stroke(point(x, y), r.width() * 0.22, stroke);
            }
        }
        Icon::Open => {
            p.add(egui::Shape::closed_line(
                vec![
                    point(0.05, 0.3),
                    point(0.05, 0.1),
                    point(0.4, 0.1),
                    point(0.5, 0.3),
                    point(0.95, 0.3),
                    point(0.85, 0.9),
                    point(0.05, 0.9),
                ],
                stroke,
            ));
        }
        Icon::Align => {
            line((0.5, 0.0), (0.5, 1.0));
            line((0.0, 0.25), (0.4, 0.25));
            line((0.0, 0.75), (0.4, 0.75));
            line((0.6, 0.5), (1.0, 0.5));
        }
        Icon::Compare => {
            p.rect_stroke(
                Rect::from_min_max(point(0.0, 0.0), point(0.65, 0.75)),
                0.0,
                stroke,
            );
            p.rect_stroke(
                Rect::from_min_max(point(0.35, 0.25), point(1.0, 1.0)),
                0.0,
                stroke,
            );
        }
        Icon::Fit => {
            for (x, y, sx, sy) in [
                (0.0, 0.0, 1.0, 1.0),
                (1.0, 0.0, -1.0, 1.0),
                (0.0, 1.0, 1.0, -1.0),
                (1.0, 1.0, -1.0, -1.0),
            ] {
                line((x, y), (x + sx * 0.3, y));
                line((x, y), (x, y + sy * 0.3));
            }
        }
    }
}
pub fn tool_rail(ctx: &egui::Context, state: &mut AppState) {
    egui::SidePanel::left("tools_rail")
        .exact_width(44.0)
        .resizable(false)
        .frame(
            egui::Frame::none()
                .fill(theme::BAR)
                .inner_margin(egui::Margin::symmetric(5.0, 10.0)),
        )
        .show(ctx, |ui| {
            ui.spacing_mut().item_spacing = Vec2::new(0.0, 4.0);
            for (icon, label, mode) in [
                (Icon::Select, "rail.select", ToolMode::Select),
                (Icon::Hand, "rail.pan", ToolMode::Pan),
                (Icon::Zoom, "rail.zoom", ToolMode::Zoom),
            ] {
                let label = crate::i18n::tr(label);
                if icon_button(ui, icon, label, state.ui.tool_mode == mode).clicked() {
                    state.ui.tool_mode = mode;
                    state.tools.active_tool = None;
                }
            }
            ui.add_space(5.0);
            ui.separator();
            ui.add_space(5.0);
            for (icon, label, tool) in [
                (Icon::Pen, "rail.pen", ToolType::Pen),
                (Icon::Line, "rail.line", ToolType::Line),
                (Icon::Arrow, "rail.arrow", ToolType::Arrow),
                (Icon::Rectangle, "rail.rectangle", ToolType::Rectangle),
                (Icon::Ellipse, "rail.ellipse", ToolType::Ellipse),
                (Icon::Text, "rail.text", ToolType::Text),
                (Icon::Cloud, "rail.cloud", ToolType::Cloud),
            ] {
                let label = crate::i18n::tr(label);
                let selected = state.ui.tool_mode == ToolMode::Drawing
                    && state
                        .tools
                        .active_tool
                        .as_ref()
                        .is_some_and(|t| t.tool_type == tool);
                if icon_button(ui, icon, label, selected).clicked() {
                    state.session.selected_annotation = None;
                    state.tools.active_tool = Some(Tool::new_default(tool));
                    state.ui.tool_mode = ToolMode::Drawing;
                }
            }
            ui.add_space(5.0);
            ui.separator();
            ui.add_space(5.0);
            for (icon, label, tool) in [
                (Icon::Ruler, "rail.measure", ToolType::MeasureLength),
                (Icon::Count, "rail.count", ToolType::Count),
            ] {
                let label = crate::i18n::tr(label);
                let selected = state.ui.tool_mode == ToolMode::Drawing
                    && state
                        .tools
                        .active_tool
                        .as_ref()
                        .is_some_and(|t| t.tool_type == tool);
                if icon_button(ui, icon, label, selected).clicked() {
                    state.session.selected_annotation = None;
                    state.tools.active_tool = Some(Tool::new_default(tool));
                    state.ui.tool_mode = ToolMode::Drawing;
                }
            }
            ui.add_space(5.0);
            let menu = ui.menu_button("…", |ui| {
                for (tool, label) in [
                    (ToolType::Highlighter, "rail.highlighter"),
                    (ToolType::Callout, "rail.callout"),
                    (ToolType::MeasureArea, "rail.area"),
                    (ToolType::MeasurePolylength, "rail.polylength"),
                    (ToolType::Viewport, "rail.viewport"),
                    (ToolType::DimensionChain, "rail.dim_chain"),
                ] {
                    if ui.button(crate::i18n::tr(label)).clicked() {
                        state.session.selected_annotation = None;
                        state.tools.active_tool = Some(Tool::new_default(tool));
                        state.ui.tool_mode = ToolMode::Drawing;
                        ui.close_menu();
                    }
                }
            });
            menu.response
                .on_hover_text(crate::i18n::tr("inspector.more_tools"));
        });
}
