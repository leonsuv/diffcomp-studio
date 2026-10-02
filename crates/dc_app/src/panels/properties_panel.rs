//! Contextual inspector for the current selection and comparison.
use crate::{
    state::{AppState, ComputeMode},
    theme,
    undo::UndoCommand,
};
use dc_core::{diff::BlendMode, LayerColor, ToolType};
use egui::{Color32, RichText, Slider, Ui};

/// Show one context at a time instead of stacking unrelated property panels.
pub fn properties_panel(ui: &mut Ui, state: &mut AppState) {
    let context = format!(
        "{:?}:{:?}:{:?}",
        state.session.selected_layer,
        state.session.selected_annotation,
        state.tools.active_tool.as_ref().map(|t| t.tool_type)
    );
    let tab_id = ui.id().with("inspector_comparison");
    let context_id = ui.id().with("inspector_context");
    let mut comparison = ui.data_mut(|d| {
        if d.get_temp::<String>(context_id).as_ref() != Some(&context) {
            d.insert_temp(context_id, context);
            d.insert_temp(tab_id, false);
        }
        d.get_temp::<bool>(tab_id).unwrap_or(false)
    });
    let tab_width = (ui.available_width() - ui.spacing().item_spacing.x) / 2.0;
    ui.horizontal(|ui| {
        for (value, key) in [
            (false, "inspector.selection"),
            (true, "inspector.comparison"),
        ] {
            let (rect, response) =
                ui.allocate_exact_size(egui::vec2(tab_width, 30.0), egui::Sense::click());
            let label = crate::i18n::tr(key);
            response
                .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
            if response.hovered() {
                ui.painter()
                    .rect_filled(rect, 3.0, Color32::from_rgb(51, 51, 55));
            }
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                label,
                egui::FontId::proportional(12.0),
                if comparison == value {
                    theme::TEXT
                } else {
                    theme::MUTED
                },
            );
            if comparison == value {
                ui.painter().line_segment(
                    [rect.left_bottom(), rect.right_bottom()],
                    egui::Stroke::new(2.0_f32, theme::ACCENT),
                );
            }
            if response.clicked() {
                comparison = value;
            }
        }
    });
    ui.data_mut(|d| d.insert_temp(tab_id, comparison));
    ui.add_space(14.0);
    ui.spacing_mut().slider_width = (ui.available_width() - 175.0).clamp(45.0, 130.0);
    ui.spacing_mut().text_edit_width = (ui.available_width() - 95.0).max(80.0);
    if comparison {
        comparison_inspector(ui, state);
    } else if state.session.selected_annotation.is_some() {
        annotation_inspector(ui, state);
    } else if state.tools.active_tool.is_some() {
        tool_inspector(ui, state);
    } else {
        document_inspector(ui, state);
    }
}

pub(super) fn section(ui: &mut Ui, title: &str, body: impl FnOnce(&mut Ui)) {
    egui::Frame::none()
        .fill(Color32::from_rgb(40, 40, 43))
        .rounding(5.0)
        .inner_margin(12.0)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(title).strong().size(12.0));
            ui.add_space(9.0);
            body(ui);
        });
    ui.add_space(12.0);
}

fn field(ui: &mut Ui, label: &str, body: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.add_sized(
            [82.0, 26.0],
            egui::Label::new(
                RichText::new(label.trim_end_matches(':'))
                    .small()
                    .color(theme::MUTED),
            )
            .truncate(),
        );
        body(ui);
    });
}

fn color_field(ui: &mut Ui, label: &str, color: &mut LayerColor) -> bool {
    let mut rgb = [
        color.r as f32 / 255.0,
        color.g as f32 / 255.0,
        color.b as f32 / 255.0,
    ];
    let mut changed = false;
    field(ui, label, |ui| {
        changed = ui.color_edit_button_rgb(&mut rgb).changed();
    });
    if changed {
        *color = LayerColor::new(
            (rgb[0] * 255.0).round() as u8,
            (rgb[1] * 255.0).round() as u8,
            (rgb[2] * 255.0).round() as u8,
        );
    }
    changed
}

fn document_inspector(ui: &mut Ui, state: &mut AppState) {
    let Some(id) = state.session.selected_layer else {
        ui.add_space(20.0);
        ui.label(RichText::new(crate::i18n::tr("inspector.empty_title")).size(14.0));
        ui.add_space(5.0);
        ui.label(RichText::new(crate::i18n::tr("inspector.empty_hint")).color(theme::MUTED));
        return;
    };
    let before_config = state.session.diff_config.clone();
    let Some(layer) = state.session.get_layer_mut(id) else {
        return;
    };
    let before = (
        layer.name.clone(),
        layer.opacity,
        layer.blend_color,
        layer.offset_x,
        layer.offset_y,
    );
    let reference = layer.is_reference;
    section(ui, crate::i18n::tr("inspector.document"), |ui| {
        ui.label(
            RichText::new(crate::i18n::tr(if reference {
                "workspace.reference"
            } else {
                "workspace.revision"
            }))
            .small()
            .color(theme::MUTED),
        );
        ui.add(egui::TextEdit::singleline(&mut layer.name).desired_width(ui.available_width()));
        ui.add_space(7.0);
        field(ui, crate::i18n::tr("props.opacity"), |ui| {
            ui.add(
                Slider::new(&mut layer.opacity, 0.0..=1.0)
                    .custom_formatter(|v, _| format!("{:.0}%", v * 100.0)),
            );
        });
        if !reference {
            color_field(
                ui,
                crate::i18n::tr("inspector.highlight"),
                &mut layer.blend_color,
            );
        }
    });
    ui.add_space(8.0);
    section(ui, crate::i18n::tr("inspector.position"), |ui| {
        field(ui, crate::i18n::tr("inspector.horizontal"), |ui| {
            ui.add(
                egui::DragValue::new(&mut layer.offset_x)
                    .speed(1.0)
                    .suffix(" px"),
            );
        });
        field(ui, crate::i18n::tr("inspector.vertical"), |ui| {
            ui.add(
                egui::DragValue::new(&mut layer.offset_y)
                    .speed(1.0)
                    .suffix(" px"),
            );
        });
    });
    let after = (
        layer.name.clone(),
        layer.opacity,
        layer.blend_color,
        layer.offset_x,
        layer.offset_y,
    );
    let mut changed = false;
    if before.0 != after.0 {
        state.session.undo_stack.push(UndoCommand::RenameLayer {
            layer_id: id,
            old_name: before.0,
            new_name: after.0,
        });
        changed = true;
    }
    if before.1 != after.1 {
        state.session.undo_stack.push(UndoCommand::SetLayerOpacity {
            layer_id: id,
            old_opacity: before.1,
            new_opacity: after.1,
        });
        changed = true;
    }
    if before.2 != after.2 {
        state
            .session
            .undo_stack
            .push(UndoCommand::SetLayerBlendColor {
                layer_id: id,
                old_color: before.2,
                new_color: after.2,
            });
        state.ui.diff_invalidated = true;
        changed = true;
    }
    if before.3 != after.3 || before.4 != after.4 {
        state.session.undo_stack.push(UndoCommand::SetLayerOffset {
            layer_id: id,
            old_x: before.3,
            old_y: before.4,
            new_x: after.3,
            new_y: after.4,
        });
        state.ui.diff_invalidated = true;
        changed = true;
    }
    if reference {
        section(ui, crate::i18n::tr("inspector.appearance"), |ui| {
            color_field(
                ui,
                crate::i18n::tr("inspector.highlight"),
                &mut state.session.diff_config.reference_color,
            );
        });
        record_config_change(state, before_config);
    }
    state.session.is_dirty |= changed;
}

fn comparison_inspector(ui: &mut Ui, state: &mut AppState) {
    let before = state.session.diff_config.clone();
    let mode = before.blend_mode;
    section(ui, crate::i18n::tr("inspector.comparison"), |ui| {
        ui.label(RichText::new(blend_mode_name(mode)).size(15.0));
        ui.label(
            RichText::new(crate::i18n::tr("inspector.mode_hint"))
                .small()
                .color(theme::MUTED),
        );
        ui.add_space(8.0);
        if mode == BlendMode::ColorDifference && state.compute_mode != ComputeMode::Gpu {
            ui.checkbox(
                &mut state.session.diff_config.morphological_tolerance,
                crate::i18n::tr("workspace.tolerance"),
            );
        }
        if mode == BlendMode::Overlay {
            field(ui, crate::i18n::tr("props.opacity"), |ui| {
                ui.add(
                    Slider::new(&mut state.session.diff_config.overlay_opacity, 0.0..=1.0)
                        .custom_formatter(|v, _| format!("{:.0}%", v * 100.0)),
                );
            });
        }
        let structural = mode == BlendMode::ColorDifference
            && state.session.diff_config.morphological_tolerance
            && state.compute_mode != ComputeMode::Gpu;
        if !structural && mode != BlendMode::Overlay {
            field(ui, crate::i18n::tr("inspector.sensitivity"), |ui| {
                if mode == BlendMode::BinaryMask {
                    ui.add(Slider::new(
                        &mut state.session.diff_config.binary_threshold,
                        0..=255,
                    ));
                } else {
                    ui.add(Slider::new(
                        &mut state.session.diff_config.noise_threshold,
                        0..=50,
                    ));
                }
            });
        }
    });
    if mode == BlendMode::ColorDifference {
        section(ui, crate::i18n::tr("inspector.colors"), |ui| {
            color_field(
                ui,
                crate::i18n::tr("workspace.reference"),
                &mut state.session.diff_config.reference_color,
            );
            for layer in &mut state.session.layers {
                if layer.is_reference {
                    continue;
                }
                let before_color = layer.blend_color;
                if color_field(ui, &layer.name, &mut layer.blend_color) {
                    state
                        .session
                        .undo_stack
                        .push(UndoCommand::SetLayerBlendColor {
                            layer_id: layer.id,
                            old_color: before_color,
                            new_color: layer.blend_color,
                        });
                    state.ui.diff_invalidated = true;
                    state.session.is_dirty = true;
                }
            }
        });
    }
    record_config_change(state, before);
}

fn record_config_change(state: &mut AppState, before: dc_core::DiffConfig) {
    let after = &state.session.diff_config;
    if before.morphological_tolerance != after.morphological_tolerance
        || before.blend_mode != after.blend_mode
        || before.overlay_opacity != after.overlay_opacity
        || before.binary_threshold != after.binary_threshold
        || before.noise_threshold != after.noise_threshold
        || before.reference_color != after.reference_color
    {
        state.session.undo_stack.push(UndoCommand::SetDiffConfig {
            old_config: before,
            new_config: after.clone(),
        });
        state.session.is_dirty = true;
        state.ui.diff_invalidated = true;
    }
}

fn tool_inspector(ui: &mut Ui, state: &mut AppState) {
    let Some(tool) = &mut state.tools.active_tool else {
        return;
    };
    section(ui, &tool.name, |ui| {
        color_field(
            ui,
            crate::i18n::tr("props.color"),
            &mut tool.style.stroke_color,
        );
        field(ui, crate::i18n::tr("props.line_width"), |ui| {
            ui.add(Slider::new(&mut tool.style.line_width, 0.5..=20.0).suffix(" pt"));
        });
        field(ui, crate::i18n::tr("props.opacity"), |ui| {
            ui.add(
                Slider::new(&mut tool.style.opacity, 0.0..=1.0)
                    .custom_formatter(|v, _| format!("{:.0}%", v * 100.0)),
            );
        });
        if matches!(tool.tool_type, ToolType::Text | ToolType::Callout) {
            field(ui, crate::i18n::tr("props.font_size"), |ui| {
                ui.add(Slider::new(&mut tool.style.font_size, 6.0..=72.0).suffix(" pt"));
            });
        }
        if matches!(
            tool.tool_type,
            ToolType::Line | ToolType::Arrow | ToolType::MeasureLength
        ) {
            for (label, ending) in [
                ("props.start", &mut tool.style.line_ending_start),
                ("props.end", &mut tool.style.line_ending_end),
            ] {
                field(ui, crate::i18n::tr(label), |ui| {
                    egui::ComboBox::from_id_salt(label)
                        .selected_text(ending.to_string())
                        .show_ui(ui, |ui| {
                            for &value in dc_core::tools::LineEndingType::ALL {
                                ui.selectable_value(ending, value, value.to_string());
                            }
                        });
                });
            }
        }
    });
    if tool.tool_type == ToolType::Count {
        section(ui, crate::i18n::tr("props.punch_list"), |ui| {
            let label = state
                .tools
                .active_sequence_group
                .as_deref()
                .unwrap_or(crate::i18n::tr("props.none"))
                .to_owned();
            egui::ComboBox::from_id_salt("count_group")
                .width(ui.available_width() - 16.0)
                .selected_text(label)
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut state.tools.active_sequence_group,
                        None,
                        crate::i18n::tr("props.none"),
                    );
                    for group in &state.session.sequence_groups {
                        ui.selectable_value(
                            &mut state.tools.active_sequence_group,
                            Some(group.clone()),
                            group,
                        );
                    }
                });
            let id = ui.id().with("count_new_group");
            let mut name = ui.data_mut(|d| d.get_temp::<String>(id).unwrap_or_default());
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut name)
                        .desired_width((ui.available_width() - 35.0).max(40.0))
                        .hint_text(crate::i18n::tr("props.new_group_hint")),
                );
                if ui.small_button("+").clicked() && !name.trim().is_empty() {
                    let group = name.trim().to_owned();
                    if !state.session.sequence_groups.contains(&group) {
                        state.session.sequence_groups.push(group.clone());
                        state.session.is_dirty = true;
                    }
                    state.tools.active_sequence_group = Some(group);
                    name.clear();
                }
            });
            ui.data_mut(|d| d.insert_temp(id, name));
        });
    }
}

fn annotation_inspector(ui: &mut Ui, state: &mut AppState) {
    // Show selected annotation properties
    let mut annotation_changed = false;
    let mut annot_snapshot_before: Option<(dc_core::LayerId, dc_core::Annotation)> = None;
    // New group to register after the mutable borrow ends
    let mut new_seq_group_to_register: Option<String> = None;

    // Capture values before mutable borrow on session
    let known_seq_groups = state.session.sequence_groups.clone();

    if let Some((layer_id, annot_id)) = state.session.selected_annotation.clone() {
        if let Some(layer) = state.session.get_layer_mut(layer_id) {
            if let Some(annot) = layer.annotations.iter_mut().find(|a| a.id == annot_id) {
                // Snapshot before any changes
                annot_snapshot_before = Some((layer_id, annot.clone()));
                section(ui, crate::i18n::tr("inspector.appearance"), |ui| {
                    // Friendly type name
                    let type_name = match annot.data {
                        dc_core::AnnotationData::Path(_) => crate::i18n::tr("annot.path"),
                        dc_core::AnnotationData::Rectangle { .. } => {
                            crate::i18n::tr("annot.rectangle")
                        }
                        dc_core::AnnotationData::Ellipse { .. } => crate::i18n::tr("annot.ellipse"),
                        dc_core::AnnotationData::Cloud(_) => crate::i18n::tr("annot.cloud"),
                        dc_core::AnnotationData::Line { .. } => crate::i18n::tr("annot.line"),
                        dc_core::AnnotationData::Text { .. } => crate::i18n::tr("annot.text"),
                        dc_core::AnnotationData::Measurement { is_area: true, .. } => {
                            crate::i18n::tr("annot.area")
                        }
                        dc_core::AnnotationData::Measurement { .. } => {
                            crate::i18n::tr("annot.measurement")
                        }
                        dc_core::AnnotationData::Count { .. } => crate::i18n::tr("annot.count"),
                        dc_core::AnnotationData::Viewport { .. } => {
                            crate::i18n::tr("annot.viewport")
                        }
                        dc_core::AnnotationData::DimensionChain { .. } => {
                            crate::i18n::tr("annot.dim_chain")
                        }
                    };
                    ui.label(format!("{} {}", crate::i18n::tr("table.type"), type_name));

                    // Style
                    field(ui, crate::i18n::tr("props.color"), |ui| {
                        let mut color = [
                            annot.style.stroke_color.r as f32 / 255.0,
                            annot.style.stroke_color.g as f32 / 255.0,
                            annot.style.stroke_color.b as f32 / 255.0,
                        ];
                        if ui.color_edit_button_rgb(&mut color).changed() {
                            annot.style.stroke_color.r = (color[0] * 255.0) as u8;
                            annot.style.stroke_color.g = (color[1] * 255.0) as u8;
                            annot.style.stroke_color.b = (color[2] * 255.0) as u8;
                            annotation_changed = true;
                        }
                    });

                    field(ui, crate::i18n::tr("props.line_width"), |ui| {
                        if ui
                            .add(Slider::new(&mut annot.style.line_width, 0.5..=20.0).suffix(" pt"))
                            .changed()
                        {
                            annotation_changed = true;
                        }
                    });

                    field(ui, crate::i18n::tr("props.opacity"), |ui| {
                        if ui
                            .add(Slider::new(&mut annot.style.opacity, 0.0..=1.0))
                            .changed()
                        {
                            annotation_changed = true;
                        }
                    });

                    // Font size (for Text/Callout)
                    if matches!(annot.data, dc_core::AnnotationData::Text { .. }) {
                        field(ui, crate::i18n::tr("props.font_size"), |ui| {
                            if ui
                                .add(
                                    Slider::new(&mut annot.style.font_size, 6.0..=72.0)
                                        .suffix(" pt"),
                                )
                                .changed()
                            {
                                annotation_changed = true;
                            }
                        });

                        // Text content editor
                        if let dc_core::AnnotationData::Text {
                            ref mut content, ..
                        } = annot.data
                        {
                            ui.label(crate::i18n::tr("props.content"));
                            if ui.text_edit_multiline(content).changed() {
                                annotation_changed = true;
                            }
                        }
                    }

                    // Line endings (for Line annotations)
                    if matches!(annot.data, dc_core::AnnotationData::Line { .. }) {
                        ui.separator();
                        ui.label(crate::i18n::tr("props.line_endings"));
                        for (key, ending) in [
                            ("props.start", &mut annot.style.line_ending_start),
                            ("props.end", &mut annot.style.line_ending_end),
                        ] {
                            field(ui, crate::i18n::tr(key), |ui| {
                                egui::ComboBox::from_id_salt(("annotation_ending", key))
                                    .selected_text(ending.to_string())
                                    .show_ui(ui, |ui| {
                                        for &value in dc_core::tools::LineEndingType::ALL {
                                            if ui
                                                .selectable_value(ending, value, value.to_string())
                                                .changed()
                                            {
                                                annotation_changed = true;
                                            }
                                        }
                                    });
                            });
                        }
                    }

                    // Count / Punch List sequence group editor
                    if let dc_core::AnnotationData::Count {
                        ref mut sequence_group_id,
                        ref mut number,
                        ..
                    } = annot.data
                    {
                        ui.separator();
                        ui.label(crate::i18n::tr("props.punch_list"));
                        field(ui, crate::i18n::tr("props.number"), |ui| {
                            let mut num_val = *number as i32;
                            if ui
                                .add(egui::DragValue::new(&mut num_val).range(1..=99999))
                                .changed()
                            {
                                *number = num_val.max(1) as u32;
                                annotation_changed = true;
                            }
                        });

                        // Group selector: ComboBox of existing groups + "(None)"
                        let current_label_owned = sequence_group_id
                            .as_deref()
                            .unwrap_or(crate::i18n::tr("props.none"))
                            .to_string();
                        field(ui, crate::i18n::tr("props.seq_group"), |ui| {
                            let combo = egui::ComboBox::from_id_salt("seq_group_combo")
                                .selected_text(&current_label_owned)
                                .width(120.0);
                            combo.show_ui(ui, |ui| {
                                if ui
                                    .selectable_label(
                                        sequence_group_id.is_none(),
                                        crate::i18n::tr("props.none"),
                                    )
                                    .clicked()
                                {
                                    *sequence_group_id = None;
                                    annotation_changed = true;
                                }
                                for group in &known_seq_groups {
                                    let selected =
                                        sequence_group_id.as_deref() == Some(group.as_str());
                                    if ui.selectable_label(selected, group).clicked() {
                                        *sequence_group_id = Some(group.clone());
                                        annotation_changed = true;
                                    }
                                }
                            });
                        });

                        // Quick-create a new group
                        field(ui, crate::i18n::tr("props.new_group"), |ui| {
                            // Use a transient buffer per-frame
                            let id = ui.id().with("new_seq_group_buf");
                            let mut buf: String =
                                ui.data_mut(|d| d.get_temp(id).unwrap_or_default());
                            let _te = ui.add(
                                egui::TextEdit::singleline(&mut buf)
                                    .desired_width(100.0)
                                    .hint_text(crate::i18n::tr("props.new_group_hint")),
                            );
                            ui.data_mut(|d| d.insert_temp(id, buf.clone()));
                            if ui.small_button("+").clicked() && !buf.trim().is_empty() {
                                let new_name = buf.trim().to_string();
                                *sequence_group_id = Some(new_name.clone());
                                annotation_changed = true;
                                // Defer group registration until after the mutable borrow
                                new_seq_group_to_register = Some(new_name);
                                // Clear the buffer
                                ui.data_mut(|d| d.insert_temp::<String>(id, String::new()));
                            }
                        });
                    }

                    // Viewport properties editor
                    if let dc_core::AnnotationData::Viewport {
                        ref mut scale,
                        ref mut label,
                        ..
                    } = annot.data
                    {
                        ui.separator();
                        ui.label(crate::i18n::tr("props.viewport_props"));
                        field(ui, crate::i18n::tr("props.scale"), |ui| {
                            let mut scale_f32 = *scale as f32;
                            if ui
                                .add(
                                    egui::DragValue::new(&mut scale_f32)
                                        .speed(0.01)
                                        .range(0.001..=10000.0)
                                        .suffix("×"),
                                )
                                .changed()
                            {
                                *scale = scale_f32 as f64;
                                annotation_changed = true;
                            }
                        });

                        field(ui, crate::i18n::tr("props.label"), |ui| {
                            if ui.text_edit_singleline(label).changed() {
                                annotation_changed = true;
                            }
                        });
                    }

                    // DimensionChain properties (read-only display of segments + total)
                    if let dc_core::AnnotationData::DimensionChain {
                        ref segment_labels,
                        ref total_label,
                        ..
                    } = annot.data
                    {
                        ui.separator();
                        ui.label(crate::i18n::tr("props.dimension_chain"));
                        for (i, label) in segment_labels.iter().enumerate() {
                            field(
                                ui,
                                &format!("{} {}", crate::i18n::tr("inspector.segment"), i + 1),
                                |ui| {
                                    ui.label(label);
                                },
                            );
                        }
                        field(ui, crate::i18n::tr("inspector.total"), |ui| {
                            ui.label(total_label);
                        });
                    }

                    // Fill color (optional)
                    ui.horizontal(|ui| {
                        let mut has_fill = annot.style.fill_color.is_some();
                        if ui
                            .checkbox(&mut has_fill, crate::i18n::tr("props.fill"))
                            .changed()
                        {
                            if has_fill {
                                annot.style.fill_color =
                                    Some(dc_core::LayerColor::new(255, 255, 255));
                            } else {
                                annot.style.fill_color = None;
                            }
                            annotation_changed = true;
                        }
                        if let Some(ref mut fill) = annot.style.fill_color {
                            let mut fc = [
                                fill.r as f32 / 255.0,
                                fill.g as f32 / 255.0,
                                fill.b as f32 / 255.0,
                            ];
                            if ui.color_edit_button_rgb(&mut fc).changed() {
                                fill.r = (fc[0] * 255.0) as u8;
                                fill.g = (fc[1] * 255.0) as u8;
                                fill.b = (fc[2] * 255.0) as u8;
                                annotation_changed = true;
                            }
                        }
                    });
                });
                ui.separator();
            }
        }
    }

    // Register any new sequence group that was created while the annotation was mutably borrowed
    if let Some(new_group) = new_seq_group_to_register {
        if !state.session.sequence_groups.contains(&new_group) {
            state.session.sequence_groups.push(new_group);
        }
    }

    if annotation_changed {
        state.session.is_dirty = true;
        // Push undo for annotation modification
        if let Some((layer_id, old_annot)) = annot_snapshot_before {
            if let Some(layer) = state.session.get_layer(layer_id) {
                if let Some(new_annot) = layer.annotations.iter().find(|a| a.id == old_annot.id) {
                    state
                        .session
                        .undo_stack
                        .push(UndoCommand::ModifyAnnotation {
                            layer_id,
                            old_annotation: Box::new(old_annot),
                            new_annotation: Box::new(new_annot.clone()),
                        });
                }
            }
        }
    }
}

fn blend_mode_name(mode: BlendMode) -> &'static str {
    crate::i18n::tr(match mode {
        BlendMode::Overlay => "blend.overlay",
        BlendMode::ColorDifference => "blend.color_diff",
        BlendMode::Heatmap => "blend.heatmap",
        BlendMode::BinaryMask => "blend.binary_mask",
        BlendMode::Subtract => "blend.subtract",
        BlendMode::Xor => "blend.xor",
    })
}
