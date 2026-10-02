// =============================================================================
// dc_app/panels/layer_panel - Layer Control Panel
// =============================================================================
// Left sidebar panel for managing layers in the comparison stack.
// =============================================================================

use crate::app::TextureCache;
use crate::state::{AppState, SlipSheetRequest};
use crate::undo::UndoCommand;
use dc_core::LayerId;
use egui::{Color32, RichText, Ui};

/// Render the layer control panel.
pub fn layer_panel(ui: &mut Ui, state: &mut AppState, textures: &mut TextureCache) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(format!(
                "{} {}",
                state.session.layers.len(),
                crate::i18n::tr("workspace.documents")
            ))
            .small()
            .color(crate::theme::MUTED),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.menu_button("…", |ui| {
                if ui.button(crate::i18n::tr("layers.snapshot")).clicked() {
                    state.session.create_snapshot();
                    ui.close_menu();
                }
                if ui.button(crate::i18n::tr("layers.flatten")).clicked() {
                    state.ui.show_flatten_dialog = true;
                    ui.close_menu();
                }
            });
            if ui.small_button(crate::i18n::tr("workspace.add")).clicked() {
                state.ui.request_file_dialog = true;
            }
        });
    });
    ui.add_space(5.0);

    // Layer list
    if state.session.layers.is_empty() {
        ui.vertical_centered(|ui| {
            ui.add_space(20.0);
            ui.label(
                RichText::new(crate::i18n::tr("layers.no_layers"))
                    .italics()
                    .color(Color32::GRAY),
            );
            ui.add_space(10.0);
            ui.label(crate::i18n::tr("layers.drop_hint"));
        });
    } else {
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let mut layer_to_remove: Option<LayerId> = None;
                let mut layer_to_select: Option<LayerId> = None;
                let mut layer_to_set_reference: Option<LayerId> = None;

                let mut layer_to_move: Option<(LayerId, bool)> = None;
                let mut layer_to_toggle_visibility: Option<LayerId> = None;

                for layer in &state.session.layers {
                    let is_selected = state.session.selected_layer == Some(layer.id);
                    let layer_id = layer.id;
                    let mut button_clicked = false;

                    let frame_response = egui::Frame::none()
                        .fill(if is_selected {
                            Color32::from_rgb(52, 62, 76)
                        } else {
                            Color32::from_rgb(40, 40, 43)
                        })
                        .stroke(egui::Stroke::new(
                            1.0_f32,
                            if is_selected {
                                Color32::from_rgb(74, 103, 144)
                            } else {
                                Color32::TRANSPARENT
                            },
                        ))
                        .inner_margin(egui::Margin::symmetric(7.0, 10.0))
                        .outer_margin(egui::Margin::symmetric(0.0, 3.0))
                        .rounding(5.0)
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                let icon = if layer.visible {
                                    super::tool_rail::Icon::Eye
                                } else {
                                    super::tool_rail::Icon::EyeOff
                                };
                                if super::tool_rail::icon_button(
                                    ui,
                                    icon,
                                    crate::i18n::tr("workspace.visible"),
                                    false,
                                )
                                .clicked()
                                {
                                    layer_to_toggle_visibility = Some(layer_id);
                                    button_clicked = true;
                                }
                                let (thumb, _) = ui.allocate_exact_size(
                                    egui::vec2(30.0, 40.0),
                                    egui::Sense::hover(),
                                );
                                ui.painter()
                                    .rect_filled(thumb, 2.0, Color32::from_rgb(29, 29, 31));
                                if let Some(texture) = textures.thumbnail(
                                    ui.ctx(),
                                    layer.id.0 as u64,
                                    layer.active_image(),
                                ) {
                                    let size = texture.size_vec2();
                                    let scale = (28.0 / size.x).min(38.0 / size.y);
                                    ui.painter().image(
                                        texture.id(),
                                        egui::Rect::from_center_size(thumb.center(), size * scale),
                                        egui::Rect::from_min_max(
                                            egui::Pos2::ZERO,
                                            egui::pos2(1.0, 1.0),
                                        ),
                                        Color32::WHITE,
                                    );
                                }
                                ui.vertical(|ui| {
                                    ui.set_width((ui.available_width() - 44.0).max(30.0));
                                    ui.add(
                                        egui::Label::new(RichText::new(&layer.name).strong())
                                            .truncate(),
                                    )
                                    .on_hover_text(&layer.name);
                                    ui.horizontal(|ui| {
                                        let (r, _) = ui.allocate_exact_size(
                                            egui::vec2(5.0, 5.0),
                                            egui::Sense::hover(),
                                        );
                                        let color = state.session.highlight_color(layer);
                                        ui.painter().circle_filled(
                                            r.center(),
                                            2.5,
                                            Color32::from_rgb(color.r, color.g, color.b),
                                        );
                                        let role = crate::i18n::tr(if layer.is_reference {
                                            "workspace.reference"
                                        } else {
                                            "workspace.revision"
                                        });
                                        let detail = if layer.is_page_missing() {
                                            format!(
                                                "{role} · {}",
                                                crate::i18n::tr("workspace.page_missing")
                                            )
                                        } else if layer.page_count() > 1 {
                                            format!(
                                                "{role} · {} {}",
                                                layer.page_count(),
                                                crate::i18n::tr("workspace.pages")
                                            )
                                        } else {
                                            role.to_string()
                                        };
                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(detail)
                                                    .small()
                                                    .color(crate::theme::MUTED),
                                            )
                                            .truncate(),
                                        );
                                    });
                                });
                                let menu = ui.menu_button("…", |ui| {
                                    if !layer.is_reference
                                        && ui.button(crate::i18n::tr("layers.set_ref")).clicked()
                                    {
                                        layer_to_set_reference = Some(layer_id);
                                        ui.close_menu();
                                    }
                                    if ui.button(crate::i18n::tr("layers.up")).clicked() {
                                        layer_to_move = Some((layer_id, true));
                                        ui.close_menu();
                                    }
                                    if ui.button(crate::i18n::tr("layers.down")).clicked() {
                                        layer_to_move = Some((layer_id, false));
                                        ui.close_menu();
                                    }
                                    ui.separator();
                                    if ui
                                        .button(crate::i18n::tr("layers.slipsheet_page"))
                                        .clicked()
                                    {
                                        state.ui.request_slipsheet_dialog =
                                            Some(SlipSheetRequest::Single(layer_id));
                                        ui.close_menu();
                                    }
                                    if ui.button(crate::i18n::tr("layers.slipsheet_doc")).clicked()
                                    {
                                        state.ui.request_slipsheet_dialog = Some(
                                            SlipSheetRequest::Batch(layer.source_path.clone()),
                                        );
                                        ui.close_menu();
                                    }
                                    ui.separator();
                                    if ui.button(crate::i18n::tr("layers.remove")).clicked() {
                                        layer_to_remove = Some(layer_id);
                                        ui.close_menu();
                                    }
                                });
                                if menu.response.clicked() {
                                    button_clicked = true;
                                }
                            });
                        });

                    // Select layer on click — but NOT when a button/menu was clicked
                    let card_rect = frame_response.response.rect;
                    let pointer_clicked_in_card = ui.input(|i| {
                        i.pointer.button_clicked(egui::PointerButton::Primary)
                            && card_rect.contains(i.pointer.interact_pos().unwrap_or_default())
                    });
                    if pointer_clicked_in_card && !button_clicked {
                        layer_to_select = Some(layer_id);
                    }

                    // Hover highlight (only if not already selected)
                    if !is_selected && ui.rect_contains_pointer(card_rect) {
                        ui.painter()
                            .rect_filled(card_rect, 4.0, Color32::from_white_alpha(10));
                    }

                    ui.add_space(2.0);
                }

                if let Some((id, up)) = layer_to_move {
                    if let Some(pos) = state.session.layers.iter().position(|l| l.id == id) {
                        let target = if up {
                            pos.saturating_sub(1)
                        } else {
                            (pos + 1).min(state.session.layers.len() - 1)
                        };
                        if target != pos {
                            if up {
                                state.session.move_layer_up(id);
                            } else {
                                state.session.move_layer_down(id);
                            }
                            state.session.undo_stack.push(UndoCommand::MoveLayer {
                                layer_id: id,
                                from_index: pos,
                                to_index: target,
                            });
                        }
                    }
                }
                // Apply deferred actions
                if let Some(id) = layer_to_toggle_visibility {
                    // Record undo before toggling
                    if let Some(layer) = state.session.get_layer(id) {
                        let old_visible = layer.visible;
                        state.session.toggle_layer_visibility(id);
                        state
                            .session
                            .undo_stack
                            .push(UndoCommand::SetLayerVisibility {
                                layer_id: id,
                                old_visible,
                                new_visible: !old_visible,
                            });
                    }
                }
                if let Some(id) = layer_to_remove {
                    if let Some(idx) = state.session.layers.iter().position(|l| l.id == id) {
                        let layer_snapshot = state.session.layers[idx].clone();
                        let was_reference = layer_snapshot.is_reference;
                        let old_selected = state.session.selected_layer;
                        state.session.remove_layer(id);
                        state.session.undo_stack.push(UndoCommand::RemoveLayer {
                            layer_id: id,
                            index: idx,
                            layer: Box::new(layer_snapshot),
                            was_reference,
                            old_selected,
                        });
                    }
                }
                if let Some(id) = layer_to_select {
                    state.session.selected_layer = Some(id);
                    state.session.selected_annotation = None;
                    state.tools.active_tool = None;
                    state.ui.tool_mode = crate::state::ToolMode::Select;
                }
                if let Some(id) = layer_to_set_reference {
                    let old_reference_id = state
                        .session
                        .layers
                        .iter()
                        .find(|l| l.is_reference)
                        .map(|l| l.id);
                    state.session.set_reference(id);
                    state.ui.alignment_invalidated = true; // Trigger re-alignment
                    state.session.undo_stack.push(UndoCommand::SetReference {
                        old_reference_id,
                        new_reference_id: id,
                    });
                }
            });
    }
}
