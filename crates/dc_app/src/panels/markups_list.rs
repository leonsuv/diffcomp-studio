// =============================================================================
// dc_app/panels/markups_list - Data Grid of All Annotations
// =============================================================================
// A spreadsheet-style table listing every annotation in the session.
// Supports sorting, filtering, click-to-select, and status tracking.
// =============================================================================

use crate::state::{AppState, WorkflowState};
use crate::undo::UndoCommand;
use dc_core::{AnnotationData, LayerId};
use egui::{Color32, Ui};
use egui_extras::{Column, TableBuilder};
use uuid::Uuid;

/// Column to sort by.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum SortColumn {
    #[default]
    Type,
    Layer,
    Status,
    Color,
    Position,
    Custom(String),
}

/// Persistent state for the markups list panel.
#[derive(Debug, Clone, Default)]
pub struct MarkupsListState {
    /// Currently applied sort column and direction.
    pub sort_column: SortColumn,
    pub sort_ascending: bool,

    /// Filter: only show annotations with this status ID (None = show all).
    pub filter_status: Option<String>,

    /// Filter: type name substring.
    pub filter_type: String,

    /// UI State: Show "Add Column" dialog
    pub show_add_column_dialog: bool,
    pub add_col_name: String,
    pub add_col_type_idx: usize,
    pub add_col_formula: String,

    /// UI State: Export triggers
    pub export_csv_requested: bool,
    pub export_xlsx_requested: bool,
}

/// A flattened row of annotation data for the table.
#[derive(Clone)]
struct MarkupRow {
    layer_id: LayerId,
    layer_name: String,
    annot_id: String,
    type_name: &'static str,
    color: Color32,
    opacity: f32,
    line_width: f32,
    pos_x: f32,
    pos_y: f32,
    width: f32,
    height: f32,
    status: Option<WorkflowState>,
    custom_values: std::collections::HashMap<String, String>,
    formula_vars: std::collections::HashMap<String, f64>,
}

/// Render the Markups List panel.
pub fn markups_list_panel(ui: &mut Ui, state: &mut AppState) {
    // ── Snapshot markups_list state for the toolbar (avoids borrow issues) ───
    let mut filter_type = state.ui.markups_list.filter_type.clone();
    let mut filter_status = state.ui.markups_list.filter_status.clone();
    let sort_column = state.ui.markups_list.sort_column.clone();
    let sort_ascending = state.ui.markups_list.sort_ascending;
    let custom_cols = state.session.custom_columns.clone();

    // ── Toolbar ──────────────────────────────────────────────────────────────
    ui.horizontal(|ui| {
        ui.label("Filter:");
        ui.add(
            egui::TextEdit::singleline(&mut filter_type)
                .hint_text(crate::i18n::tr("markups.filter_type"))
                .desired_width(120.0),
        );

        ui.separator();

        // Status filter combo
        let current_filter_label = match &filter_status {
            None => crate::i18n::tr("markups.all_statuses").to_string(),
            Some(id) if id.is_empty() => "—".to_string(),
            Some(id) => state
                .session
                .workflow_states
                .iter()
                .find(|s| &s.id == id)
                .map(|s| s.name.clone())
                .unwrap_or_else(|| crate::i18n::tr("misc.unknown").to_string()),
        };
        egui::ComboBox::from_id_salt("markups_status_filter")
            .selected_text(current_filter_label)
            .show_ui(ui, |ui| {
                ui.selectable_value(
                    &mut filter_status,
                    None,
                    crate::i18n::tr("markups.all_statuses"),
                );
                ui.selectable_value(&mut filter_status, Some(String::new()), "—");
                for ws in &state.session.workflow_states {
                    ui.selectable_value(&mut filter_status, Some(ws.id.clone()), &ws.name);
                }
            });

        ui.separator();

        // Summary counts
        let total: usize = state
            .session
            .layers
            .iter()
            .map(|l| l.annotations.len())
            .sum();
        ui.label(format!("{} {}", total, crate::i18n::tr("markups.markups")));

        ui.separator();

        // Columns management menu
        ui.menu_button(crate::i18n::tr("markups.columns"), |ui| {
            if ui.button(crate::i18n::tr("markups.add_column")).clicked() {
                state.ui.markups_list.show_add_column_dialog = true;
                ui.close_menu();
            }
            ui.separator();

            // List existing columns to remove
            let mut to_remove = None;
            for (idx, col) in state.session.custom_columns.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(&col.name);
                    if ui.button("x").clicked() {
                        to_remove = Some(idx);
                    }
                });
            }

            if let Some(idx) = to_remove {
                let removed = state.session.custom_columns.remove(idx);
                state
                    .session
                    .undo_stack
                    .push(UndoCommand::RemoveCustomColumn {
                        index: idx,
                        column: removed,
                    });
            }
        });

        ui.separator();

        ui.menu_button(crate::i18n::tr("markups.export"), |ui| {
            if ui.button(crate::i18n::tr("markups.export_csv")).clicked() {
                state.ui.markups_list.export_csv_requested = true;
                ui.close_menu();
            }
            if ui.button(crate::i18n::tr("markups.export_xlsx")).clicked() {
                state.ui.markups_list.export_xlsx_requested = true;
                ui.close_menu();
            }
        });
    });

    // Write back changed filter state
    state.ui.markups_list.filter_type = filter_type;
    state.ui.markups_list.filter_status = filter_status;

    ui.separator();

    // ── Collect rows ─────────────────────────────────────────────────────────
    let mut rows: Vec<MarkupRow> = Vec::new();

    for layer in &state.session.layers {
        for annot in &layer.annotations {
            let type_name = match &annot.data {
                AnnotationData::Path(_) => crate::i18n::tr("annot.path"),
                AnnotationData::Rectangle { .. } => crate::i18n::tr("annot.rectangle"),
                AnnotationData::Ellipse { .. } => crate::i18n::tr("annot.ellipse"),
                AnnotationData::Cloud(_) => crate::i18n::tr("annot.cloud"),
                AnnotationData::Line { .. } => crate::i18n::tr("annot.line"),
                AnnotationData::Text { .. } => crate::i18n::tr("annot.text"),
                AnnotationData::Measurement { is_area: true, .. } => crate::i18n::tr("annot.area"),
                AnnotationData::Measurement { .. } => crate::i18n::tr("annot.measurement"),
                AnnotationData::Count { .. } => crate::i18n::tr("annot.count"),
                AnnotationData::Viewport { .. } => crate::i18n::tr("annot.viewport"),
                AnnotationData::DimensionChain { .. } => crate::i18n::tr("annot.dim_chain"),
            };

            let status = state
                .session
                .annotation_statuses
                .get(&annot.id)
                .and_then(|id| {
                    state
                        .session
                        .workflow_states
                        .iter()
                        .find(|ws| &ws.id == id)
                        .cloned()
                });

            let sc = &annot.style.stroke_color;
            let color = Color32::from_rgba_premultiplied(
                sc.r,
                sc.g,
                sc.b,
                (annot.style.opacity * 255.0) as u8,
            );

            let (min_x, min_y, max_x, max_y) = annot.bounds;

            let mut formula_vars = std::collections::HashMap::new();
            formula_vars.insert("Width".to_string(), (max_x - min_x) as f64);
            formula_vars.insert("Height".to_string(), (max_y - min_y) as f64);
            formula_vars.insert("Count".to_string(), 1.0); // Base count of 1

            match &annot.data {
                AnnotationData::Measurement {
                    is_area: true,
                    value,
                    ..
                } => {
                    formula_vars.insert("Area".to_string(), *value);
                }
                AnnotationData::Measurement {
                    is_area: false,
                    value,
                    ..
                } => {
                    formula_vars.insert("Length".to_string(), *value);
                }
                AnnotationData::DimensionChain { total_value, .. } => {
                    formula_vars.insert("Length".to_string(), *total_value);
                }
                _ => {}
            }

            // Expose existing custom numeric fields as variables
            for (k, v) in &annot.custom_properties {
                if let Ok(num) = v.parse::<f64>() {
                    // Try looking up by ID to find Name
                    if let Some(col) = custom_cols.iter().find(|c| &c.id == k) {
                        formula_vars.insert(col.name.clone(), num);
                    }
                }
            }

            rows.push(MarkupRow {
                layer_id: annot.layer_id,
                layer_name: layer.name.clone(),
                annot_id: annot.id.clone(),
                type_name,
                color,
                opacity: annot.style.opacity,
                line_width: annot.style.line_width,
                pos_x: min_x,
                pos_y: min_y,
                width: max_x - min_x,
                height: max_y - min_y,
                status,
                custom_values: annot.custom_properties.clone(),
                formula_vars,
            });
        }
    }

    // ── Apply filters ────────────────────────────────────────────────────────
    let filter_type_lower = state.ui.markups_list.filter_type.to_lowercase();
    let fs = state.ui.markups_list.filter_status.clone();

    rows.retain(|row| {
        if !filter_type_lower.is_empty()
            && !row.type_name.to_lowercase().contains(&filter_type_lower)
        {
            return false;
        }
        if let Some(f) = &fs {
            if f.is_empty() {
                if row.status.is_some() {
                    return false;
                }
            } else if row.status.as_ref().map(|s| &s.id) != Some(f) {
                return false;
            }
        }
        true
    });

    // ── Apply sorting ────────────────────────────────────────────────────────
    match &sort_column {
        SortColumn::Type => {
            rows.sort_by(|a, b| {
                let ord = a.type_name.cmp(b.type_name);
                if sort_ascending {
                    ord
                } else {
                    ord.reverse()
                }
            });
        }
        SortColumn::Layer => {
            rows.sort_by(|a, b| {
                let ord = a.layer_name.cmp(&b.layer_name);
                if sort_ascending {
                    ord
                } else {
                    ord.reverse()
                }
            });
        }
        SortColumn::Status => {
            rows.sort_by(|a, b| {
                let default_str = String::new();
                let status_a = a.status.as_ref().map(|s| &s.name).unwrap_or(&default_str);
                let status_b = b.status.as_ref().map(|s| &s.name).unwrap_or(&default_str);
                let ord = status_a.cmp(status_b);
                if sort_ascending {
                    ord
                } else {
                    ord.reverse()
                }
            });
        }
        SortColumn::Color => {
            rows.sort_by(|a, b| {
                let key_a = (a.color.r(), a.color.g(), a.color.b());
                let key_b = (b.color.r(), b.color.g(), b.color.b());
                let ord = key_a.cmp(&key_b);
                if sort_ascending {
                    ord
                } else {
                    ord.reverse()
                }
            });
        }
        SortColumn::Position => {
            rows.sort_by(|a, b| {
                let ord = a
                    .pos_x
                    .partial_cmp(&b.pos_x)
                    .unwrap_or(std::cmp::Ordering::Equal);
                if sort_ascending {
                    ord
                } else {
                    ord.reverse()
                }
            });
        }
        SortColumn::Custom(col_id) => {
            rows.sort_by(|a, b| {
                let val_a = a
                    .custom_values
                    .get(col_id)
                    .map(|s| s.as_str())
                    .unwrap_or("");
                let val_b = b
                    .custom_values
                    .get(col_id)
                    .map(|s| s.as_str())
                    .unwrap_or("");

                let ord = val_a.cmp(val_b);
                if sort_ascending {
                    ord
                } else {
                    ord.reverse()
                }
            });
        }
    }

    // ── Table header helpers ─────────────────────────────────────────────────
    let sort_hdr = |label: &str, col: SortColumn| -> String {
        if sort_column == col {
            format!("{} {}", label, if sort_ascending { "^" } else { "v" })
        } else {
            label.to_string()
        }
    };

    // ── Table ────────────────────────────────────────────────────────────────
    let selected_annot_id = state
        .session
        .selected_annotation
        .as_ref()
        .map(|(_, id)| id.clone());

    let available = ui.available_size();
    let text_height = ui.text_style_height(&egui::TextStyle::Body);
    let row_height = text_height + 8.0;

    // We need to capture which row was clicked.
    let mut clicked_row: Option<usize> = Option::None;
    let mut status_changes: Vec<(String, Option<String>)> = Vec::new();
    let mut delete_target: Option<(LayerId, String)> = Option::None;
    let mut custom_edits: Vec<(LayerId, String, String, String)> = Vec::new();
    let mut sort_trigger: Option<SortColumn> = None;

    let mut builder = TableBuilder::new(ui)
        .striped(true)
        .resizable(true)
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .min_scrolled_height(0.0)
        .max_scroll_height(available.y);

    // Standard Columns
    builder = builder
        .column(Column::auto().at_least(60.0)) // Type
        .column(Column::auto().at_least(80.0)) // Layer
        .column(Column::auto().at_least(30.0)) // Color swatch
        .column(Column::auto().at_least(50.0)) // Opacity
        .column(Column::auto().at_least(55.0)) // Line W
        .column(Column::auto().at_least(80.0)) // Position
        .column(Column::auto().at_least(80.0)) // Size
        .column(Column::auto().at_least(90.0)); // Status

    // Custom Columns
    for _ in &custom_cols {
        builder = builder.column(Column::auto().at_least(80.0));
    }

    builder = builder.sense(egui::Sense::click());

    // Header
    let type_hdr = sort_hdr(crate::i18n::tr("table.type"), SortColumn::Type);
    let layer_hdr = sort_hdr(crate::i18n::tr("table.layer"), SortColumn::Layer);
    let pos_hdr = sort_hdr(crate::i18n::tr("table.position"), SortColumn::Position);
    let status_hdr = sort_hdr(crate::i18n::tr("table.status"), SortColumn::Status);

    builder
        .header(row_height + 2.0, |mut header| {
            header.col(|ui| {
                if ui.button(&type_hdr).clicked() {
                    sort_trigger = Some(SortColumn::Type);
                }
            });
            header.col(|ui| {
                if ui.button(&layer_hdr).clicked() {
                    sort_trigger = Some(SortColumn::Layer);
                }
            });
            header.col(|ui| {
                ui.label(crate::i18n::tr("table.color"));
            });
            header.col(|ui| {
                ui.label(crate::i18n::tr("table.opacity"));
            });
            header.col(|ui| {
                ui.label(crate::i18n::tr("table.width"));
            });
            header.col(|ui| {
                if ui.button(&pos_hdr).clicked() {
                    sort_trigger = Some(SortColumn::Position);
                }
            });
            header.col(|ui| {
                ui.label(crate::i18n::tr("table.size"));
            });
            header.col(|ui| {
                if ui.button(&status_hdr).clicked() {
                    sort_trigger = Some(SortColumn::Status);
                }
            });

            // Custom Columns Header
            for col in &custom_cols {
                let hdr_text = sort_hdr(&col.name, SortColumn::Custom(col.id.clone()));
                header.col(|ui| {
                    if ui.button(&hdr_text).clicked() {
                        sort_trigger = Some(SortColumn::Custom(col.id.clone()));
                    }
                });
            }
        })
        .body(|body| {
            body.rows(row_height, rows.len(), |mut row| {
                let idx = row.index();
                let markup = &rows[idx];
                let is_selected = selected_annot_id.as_deref() == Some(&markup.annot_id);
                row.set_selected(is_selected);

                // Type
                row.col(|ui| {
                    ui.label(markup.type_name);
                });

                // Layer
                row.col(|ui| {
                    ui.label(&markup.layer_name);
                });

                // Color swatch
                row.col(|ui| {
                    let (rect, _) =
                        ui.allocate_exact_size(egui::Vec2::new(16.0, 16.0), egui::Sense::hover());
                    ui.painter().rect_filled(rect, 2.0, markup.color);
                    ui.painter()
                        .rect_stroke(rect, 2.0, egui::Stroke::new(1.0_f32, Color32::GRAY));
                });

                // Opacity
                row.col(|ui| {
                    ui.label(format!("{:.0}%", markup.opacity * 100.0));
                });

                // Line Width
                row.col(|ui| {
                    ui.label(format!("{:.1}pt", markup.line_width));
                });

                // Position
                row.col(|ui| {
                    ui.label(format!("{:.0}, {:.0}", markup.pos_x, markup.pos_y));
                });

                // Size
                row.col(|ui| {
                    ui.label(format!("{:.0}×{:.0}", markup.width, markup.height));
                });

                // Status
                row.col(|ui| {
                    if let Some(ref s) = markup.status {
                        let c = Color32::from_rgb(s.color[0], s.color[1], s.color[2]);
                        ui.colored_label(c, &s.name);
                    } else {
                        ui.label("—");
                    }
                });

                // Custom Values
                for col in &custom_cols {
                    row.col(|ui| {
                        if let crate::state::ColumnType::Formula(ref expr) = col.col_type {
                            match crate::formula::evaluate_formula(expr, &markup.formula_vars) {
                                Ok(val) => {
                                    ui.label(format!("{:.2}", val));
                                }
                                Err(e) => {
                                    ui.label(format!("!")).on_hover_text(e);
                                }
                            }
                        } else {
                            let current_val = markup
                                .custom_values
                                .get(&col.id)
                                .map(|s| s.as_str())
                                .unwrap_or("");
                            let mut text = current_val.to_string();
                            // Minimalist text edit
                            if ui
                                .add(egui::TextEdit::singleline(&mut text).frame(false))
                                .changed()
                            {
                                custom_edits.push((
                                    markup.layer_id,
                                    markup.annot_id.clone(),
                                    col.id.clone(),
                                    text,
                                ));
                            }
                        }
                    });
                }

                // Handle click -> select annotation
                let response = row.response();
                if response.clicked() {
                    clicked_row = Some(idx);
                }

                // Right-click context menu
                response.context_menu(|ui| {
                    ui.label(format!("{} — {}", markup.type_name, &markup.annot_id[..8]));
                    ui.separator();

                    for ws in &state.session.workflow_states {
                        if ui.button(&ws.name).clicked() {
                            status_changes.push((markup.annot_id.clone(), Some(ws.id.clone())));
                            ui.close_menu();
                        }
                    }

                    if ui.button(crate::i18n::tr("markups.clear_status")).clicked() {
                        status_changes.push((markup.annot_id.clone(), None));
                        ui.close_menu();
                    }

                    ui.separator();
                    if ui.button(crate::i18n::tr("markups.delete")).clicked() {
                        delete_target = Some((markup.layer_id, markup.annot_id.clone()));
                        ui.close_menu();
                    }
                });
            });
        });

    // ── Handle click-to-select ───────────────────────────────────────────────
    if let Some(idx) = clicked_row {
        let row = &rows[idx];
        state.session.selected_annotation = Some((row.layer_id, row.annot_id.clone()));
        // Switch to select mode so the viewport highlights it
        state.ui.tool_mode = crate::state::ToolMode::Select;
    }

    // ── Apply status changes ─────────────────────────────────────────────────
    for (id, status_opt) in status_changes {
        if let Some(status_id) = status_opt {
            state.session.annotation_statuses.insert(id, status_id);
        } else {
            state.session.annotation_statuses.remove(&id);
        }
        state.session.is_dirty = true;
    }

    // ── Handle deletion ──────────────────────────────────────────────────────
    if let Some((layer_id, annot_id)) = delete_target {
        if let Some(layer) = state.session.get_layer_mut(layer_id) {
            // Snapshot annotation before removing for undo
            if let Some(pos) = layer.annotations.iter().position(|a| a.id == annot_id) {
                let removed = layer.annotations.remove(pos);
                state
                    .session
                    .undo_stack
                    .push(UndoCommand::RemoveAnnotation {
                        layer_id,
                        annotation: Box::new(removed),
                        index: pos,
                    });
            }
            state.session.is_dirty = true;
        }
        // Clear selection if we deleted the selected annotation
        if state
            .session
            .selected_annotation
            .as_ref()
            .map_or(false, |(_, id)| id == &annot_id)
        {
            state.session.selected_annotation = None;
        }
    }

    // ── Apply custom edits ───────────────────────────────────────────────────
    let mut custom_undo_cmds: Vec<UndoCommand> = Vec::new();
    for (layer_id, annot_id, col_id, new_val) in custom_edits {
        if let Some(layer) = state.session.get_layer_mut(layer_id) {
            if let Some(annot) = layer.annotations.iter_mut().find(|a| a.id == annot_id) {
                let old_annot = annot.clone();
                annot.custom_properties.insert(col_id, new_val);
                let new_annot = annot.clone();
                custom_undo_cmds.push(UndoCommand::ModifyAnnotation {
                    layer_id,
                    old_annotation: Box::new(old_annot),
                    new_annotation: Box::new(new_annot),
                });
                state.session.is_dirty = true;
            }
        }
    }
    for cmd in custom_undo_cmds {
        state.session.undo_stack.push(cmd);
    }

    // ── Dialogs ──────────────────────────────────────────────────────────────
    if state.ui.markups_list.show_add_column_dialog {
        let ctx = ui.ctx().clone();
        let mut open = true;
        egui::Window::new(crate::i18n::tr("markups.add_col_title"))
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .show(&ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(crate::i18n::tr("markups.col_name"));
                    ui.text_edit_singleline(&mut state.ui.markups_list.add_col_name);
                });

                ui.horizontal(|ui| {
                    ui.label(crate::i18n::tr("markups.col_type"));
                    egui::ComboBox::from_id_salt("new_col_type")
                        .selected_text(match state.ui.markups_list.add_col_type_idx {
                            0 => crate::i18n::tr("coltype.text"),
                            1 => crate::i18n::tr("coltype.number"),
                            2 => crate::i18n::tr("coltype.formula"),
                            _ => crate::i18n::tr("coltype.text"),
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut state.ui.markups_list.add_col_type_idx,
                                0,
                                crate::i18n::tr("coltype.text"),
                            );
                            ui.selectable_value(
                                &mut state.ui.markups_list.add_col_type_idx,
                                1,
                                crate::i18n::tr("coltype.number"),
                            );
                            ui.selectable_value(
                                &mut state.ui.markups_list.add_col_type_idx,
                                2,
                                crate::i18n::tr("coltype.formula"),
                            );
                        });
                });

                if state.ui.markups_list.add_col_type_idx == 2 {
                    ui.horizontal(|ui| {
                        ui.label(crate::i18n::tr("markups.formula_label"));
                        ui.text_edit_singleline(&mut state.ui.markups_list.add_col_formula)
                            .on_hover_text(crate::i18n::tr("markups.formula_hint"));
                    });
                }

                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button(crate::i18n::tr("markups.cancel")).clicked() {
                        state.ui.markups_list.show_add_column_dialog = false;
                    }
                    if ui.button(crate::i18n::tr("markups.add")).clicked() {
                        let name = state.ui.markups_list.add_col_name.trim().to_string();
                        if !name.is_empty() {
                            use crate::state::{ColumnType, CustomColumn};
                            let col_type = match state.ui.markups_list.add_col_type_idx {
                                1 => ColumnType::Number,
                                2 => ColumnType::Formula(
                                    state.ui.markups_list.add_col_formula.clone(),
                                ),
                                _ => ColumnType::Text,
                            };
                            let col = CustomColumn {
                                id: Uuid::new_v4().to_string(),
                                name,
                                col_type,
                                default_value: String::new(),
                            };
                            let idx = state.session.custom_columns.len();
                            state.session.custom_columns.push(col.clone());
                            state.session.undo_stack.push(UndoCommand::AddCustomColumn {
                                index: idx,
                                column: col,
                            });
                            state.ui.markups_list.show_add_column_dialog = false;
                            state.ui.markups_list.add_col_name.clear();
                            state.ui.markups_list.add_col_formula.clear();
                        }
                    }
                });
            });

        if !open {
            state.ui.markups_list.show_add_column_dialog = false;
        }
    }

    // ── Export Handling ──────────────────────────────────────────────────────
    if state.ui.markups_list.export_csv_requested {
        state.ui.markups_list.export_csv_requested = false;
        #[cfg(not(target_arch = "wasm32"))]
        {
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("CSV File", &["csv"])
                .set_file_name("markups_export.csv")
                .save_file()
            {
                if let Err(e) = export_to_csv(&path, &rows, &custom_cols) {
                    state.ui.set_status(format!("CSV Export failed: {}", e));
                } else {
                    state.ui.set_status(crate::i18n::tr("status.csv_exported"));
                }
            }
        }
    }

    if state.ui.markups_list.export_xlsx_requested {
        state.ui.markups_list.export_xlsx_requested = false;
        #[cfg(not(target_arch = "wasm32"))]
        {
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("Excel Document", &["xlsx"])
                .set_file_name("markups_export.xlsx")
                .save_file()
            {
                if let Err(e) = export_to_xlsx(&path, &rows, &custom_cols) {
                    state.ui.set_status(format!("Excel Export failed: {}", e));
                } else {
                    state.ui.set_status(crate::i18n::tr("status.xlsx_exported"));
                }
            }
        }
    }
}

/// Helper to gather row data consistently as strings for either exporter
fn gather_row_data(row: &MarkupRow, custom_cols: &[crate::state::CustomColumn]) -> Vec<String> {
    let mut parts = vec![
        row.type_name.to_string(),
        row.layer_name.clone(),
        row.status
            .as_ref()
            .map(|s| s.name.clone())
            .unwrap_or_default(),
        format!("{:.1}", row.line_width),
        format!("{:.0}, {:.0}", row.pos_x, row.pos_y),
        format!("{:.0}x{:.0}", row.width, row.height),
    ];

    for col in custom_cols {
        let val_str = if let crate::state::ColumnType::Formula(ref expr) = col.col_type {
            match crate::formula::evaluate_formula(expr, &row.formula_vars) {
                Ok(val) => format!("{:.2}", val),
                Err(_) => "ERR".to_string(),
            }
        } else {
            row.custom_values
                .get(&col.id)
                .map(|s| s.to_string())
                .unwrap_or_default()
        };
        parts.push(val_str);
    }
    parts
}

/// Exports the rendered, filtered markups rows to CSV.
fn export_to_csv(
    path: &std::path::Path,
    rows: &[MarkupRow],
    custom_cols: &[crate::state::CustomColumn],
) -> anyhow::Result<()> {
    let mut writer = csv::Writer::from_path(path)?;

    // Header
    let mut header = vec!["Type", "Layer", "Status", "Width", "Position", "Size"];
    for col in custom_cols {
        header.push(&col.name);
    }
    writer.write_record(&header)?;

    for row in rows {
        let data = gather_row_data(row, custom_cols);
        writer.write_record(&data)?;
    }
    writer.flush()?;
    Ok(())
}

/// Exports the rendered, filtered markups rows to Excel using rust_xlsxwriter.
fn export_to_xlsx(
    path: &std::path::Path,
    rows: &[MarkupRow],
    custom_cols: &[crate::state::CustomColumn],
) -> anyhow::Result<()> {
    use rust_xlsxwriter::*;
    use std::collections::HashMap;

    let mut workbook = Workbook::new();

    // Format for headers
    let header_format = Format::new().set_bold().set_border(FormatBorder::Thin);

    // ---------------------------------------------------------------------------------
    // Sheet 1: BOM Summary
    // ---------------------------------------------------------------------------------
    let bom_sheet = workbook.add_worksheet().set_name("BOM Summary")?;

    let mut bom_headers = vec!["Type", "Count"];
    // Find numeric/formula cols to summarize
    let mut summary_cols = Vec::new();
    for col in custom_cols {
        if matches!(
            col.col_type,
            crate::state::ColumnType::Number | crate::state::ColumnType::Formula(_)
        ) {
            bom_headers.push(&col.name);
            summary_cols.push(col);
        }
    }
    for (col_num, h) in bom_headers.iter().enumerate() {
        bom_sheet.write_string_with_format(0, col_num as u16, *h, &header_format)?;
    }

    // Accumulate by Type
    // Map: Type -> (Count, Vec<f64> sums)
    let mut bom_map: HashMap<String, (u32, Vec<f64>)> = HashMap::new();

    for row in rows {
        let type_name = row.type_name.to_string();
        let entry = bom_map
            .entry(type_name)
            .or_insert_with(|| (0, vec![0.0; summary_cols.len()]));
        entry.0 += 1; // Count

        for (sum_idx, col) in summary_cols.iter().enumerate() {
            let val = if let crate::state::ColumnType::Formula(ref expr) = col.col_type {
                crate::formula::evaluate_formula(expr, &row.formula_vars).unwrap_or(0.0)
            } else {
                row.custom_values
                    .get(&col.id)
                    .and_then(|s| s.parse::<f64>().ok())
                    .unwrap_or(0.0)
            };
            entry.1[sum_idx] += val;
        }
    }

    let mut bom_row_idx = 1;
    for (type_name, (count, sums)) in bom_map {
        bom_sheet.write_string(bom_row_idx, 0, &type_name)?;
        bom_sheet.write_number(bom_row_idx, 1, count)?;
        for (i, sum) in sums.iter().enumerate() {
            bom_sheet.write_number(bom_row_idx, (i + 2) as u16, *sum)?;
        }
        bom_row_idx += 1;
    }
    bom_sheet.autofit();

    // ---------------------------------------------------------------------------------
    // Sheet 2: Markups List (Raw Data)
    // ---------------------------------------------------------------------------------
    let worksheet = workbook.add_worksheet().set_name("Markups List")?;

    // Format for headers
    let header_format = Format::new().set_bold().set_border(FormatBorder::Thin);

    // Write headers
    let mut header = vec!["Type", "Layer", "Status", "Width", "Position", "Size"];
    for col in custom_cols {
        header.push(&col.name);
    }
    for (col_num, h) in header.iter().enumerate() {
        worksheet.write_string_with_format(0, col_num as u16, *h, &header_format)?;
    }

    // Write row data
    for (row_idx, row) in rows.iter().enumerate() {
        let rs_row = (row_idx + 1) as u32;
        let data = gather_row_data(row, custom_cols);
        for (col_idx, val) in data.iter().enumerate() {
            // Try to parse as number to let Excel sum the values automatically
            if let Ok(num) = val.parse::<f64>() {
                worksheet.write_number(rs_row, col_idx as u16, num)?;
            } else {
                worksheet.write_string(rs_row, col_idx as u16, val)?;
            }
        }
    }

    // Auto-fit columns as a neat UX boost
    worksheet.autofit();

    workbook.save(path)?;
    Ok(())
}
