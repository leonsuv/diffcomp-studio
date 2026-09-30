// =============================================================================
// dc_app/panels/legend_panel - Dynamic Legend Generator
// =============================================================================
// Groups annotations by subject + color and renders a summary table.
// Clicking a row selects/flashes the related annotations in the viewport.
// Uses egui_extras::TableBuilder for a professional tabular layout.
// =============================================================================

use crate::state::AppState;
use dc_core::{AnnotationData, LayerColor, LayerId};
use egui::{Color32, RichText, Sense, Stroke, Ui, Vec2};
use egui_extras::{Column, TableBuilder};
use std::collections::BTreeMap;

/// A legend entry grouping annotations with the same key and color.
#[derive(Debug, Clone)]
struct LegendEntry {
    label: String,
    color: LayerColor,
    count: usize,
    /// (layer_id, annotation_id) pairs for selection
    annotations: Vec<(LayerId, String)>,
    /// Friendly type breakdown e.g. "3 Rectangle, 2 Line"
    type_breakdown: String,
    /// Total measurement length (for length measurements)
    total_length: Option<String>,
    /// Total measurement area (for area measurements)
    total_area: Option<String>,
    /// Count of Count annotations in this group
    count_marker_total: Option<u32>,
    /// Centroid of all annotations (image coordinates, for pan-to)
    centroid: Option<(f64, f64)>,
}

/// Persistent state for the legend panel.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct LegendPanelState {
    /// Whether to group by subject (true) or by type (false)
    pub group_by_subject: bool,
    /// Whether to show annotation counts
    pub show_counts: bool,
    /// Whether to show color swatches
    pub show_swatches: bool,
    /// Filter text
    pub filter: String,
    /// Expanded group indices
    #[serde(skip)]
    pub expanded: std::collections::HashSet<usize>,
    /// Flash state: annotation IDs currently being flashed, with remaining flash time
    #[serde(skip)]
    pub flash_annotations: Vec<(LayerId, String, f64)>,
    /// Timestamp of last frame for flash decay
    #[serde(skip)]
    pub last_flash_time: Option<f64>,
}

impl LegendPanelState {
    pub fn new() -> Self {
        Self {
            group_by_subject: true,
            show_counts: true,
            show_swatches: true,
            filter: String::new(),
            expanded: std::collections::HashSet::new(),
            flash_annotations: Vec::new(),
            last_flash_time: None,
        }
    }
}

fn type_name(data: &AnnotationData) -> &'static str {
    match data {
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
    }
}

/// Build legend entries from all visible layers.
fn build_legend_entries(state: &AppState, group_by_type: bool) -> Vec<LegendEntry> {
    // Key: (group_label, color_r, color_g, color_b)
    let mut groups: BTreeMap<(String, u8, u8, u8), LegendEntry> = BTreeMap::new();

    for layer in &state.session.layers {
        if !layer.visible {
            continue;
        }
        for annot in &layer.annotations {
            // Group key: by type name, or by custom_properties "subject" if grouping by subject
            let key_str = if !group_by_type {
                annot
                    .custom_properties
                    .get("subject")
                    .filter(|s| !s.is_empty())
                    .cloned()
                    .unwrap_or_else(|| type_name(&annot.data).to_string())
            } else {
                type_name(&annot.data).to_string()
            };

            let c = &annot.style.stroke_color;
            let key = (key_str.clone(), c.r, c.g, c.b);

            let entry = groups.entry(key).or_insert_with(|| LegendEntry {
                label: key_str,
                color: annot.style.stroke_color,
                count: 0,
                annotations: Vec::new(),
                type_breakdown: String::new(),
                total_length: None,
                total_area: None,
                count_marker_total: None,
                centroid: None,
            });
            entry.count += 1;
            entry.annotations.push((layer.id, annot.id.clone()));
        }
    }

    // Build type breakdowns, measurement totals, and centroids
    for entry in groups.values_mut() {
        let mut type_counts: BTreeMap<&'static str, usize> = BTreeMap::new();
        let mut total_length = 0.0_f64;
        let mut has_length = false;
        let mut total_area = 0.0_f64;
        let mut has_area = false;
        let mut count_total = 0u32;
        let mut has_counts = false;
        let mut cx_sum = 0.0_f64;
        let mut cy_sum = 0.0_f64;
        let mut centroid_n = 0usize;

        for (layer_id, annot_id) in &entry.annotations {
            if let Some(layer) = state.session.layers.iter().find(|l| l.id == *layer_id) {
                if let Some(annot) = layer.annotations.iter().find(|a| a.id == *annot_id) {
                    let tn = type_name(&annot.data);
                    *type_counts.entry(tn).or_insert(0) += 1;

                    // Accumulate centroid from annotation bounds
                    let (min_x, min_y, max_x, max_y) = annot.bounds;
                    cx_sum += ((min_x + max_x) / 2.0 + layer.offset_x) as f64;
                    cy_sum += ((min_y + max_y) / 2.0 + layer.offset_y) as f64;
                    centroid_n += 1;

                    match &annot.data {
                        AnnotationData::Measurement { value, is_area, .. } => {
                            if *is_area {
                                total_area += *value;
                                has_area = true;
                            } else {
                                total_length += *value;
                                has_length = true;
                            }
                        }
                        AnnotationData::DimensionChain { total_value, .. } => {
                            total_length += *total_value;
                            has_length = true;
                        }
                        AnnotationData::Count { .. } => {
                            count_total += 1;
                            has_counts = true;
                        }
                        _ => {}
                    }
                }
            }
        }

        entry.type_breakdown = type_counts
            .iter()
            .map(|(tn, c)| {
                if *c > 1 {
                    format!("{} {}", c, tn)
                } else {
                    tn.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join(", ");

        if has_length {
            let cal = &state.session.calibration;
            entry.total_length = Some(cal.format_length(total_length));
        }
        if has_area {
            let cal = &state.session.calibration;
            entry.total_area = Some(cal.format_area(total_area));
        }
        if has_counts {
            entry.count_marker_total = Some(count_total);
        }
        if centroid_n > 0 {
            entry.centroid = Some((cx_sum / centroid_n as f64, cy_sum / centroid_n as f64));
        }
    }

    groups.into_values().collect()
}

/// Check if an annotation is currently flashing.
fn flash_alpha(state: &LegendPanelState, layer_id: LayerId, annot_id: &str) -> Option<f32> {
    for (lid, aid, remaining) in &state.flash_annotations {
        if *lid == layer_id && aid == annot_id {
            // Pulsing alpha: oscillate between 0.3 and 1.0
            let t = *remaining;
            let pulse = (t * 8.0 * std::f64::consts::PI).sin().abs() as f32;
            return Some(0.3 + 0.7 * pulse);
        }
    }
    None
}

/// Render the Legend panel.
pub fn legend_panel(ui: &mut Ui, state: &mut AppState) {
    // Update flash timers
    let now = ui.input(|i| i.time);
    if let Some(last) = state.ui.legend.last_flash_time {
        let dt = now - last;
        state
            .ui
            .legend
            .flash_annotations
            .iter_mut()
            .for_each(|(_, _, remaining)| *remaining -= dt);
        state
            .ui
            .legend
            .flash_annotations
            .retain(|(_, _, remaining)| *remaining > 0.0);
    }
    state.ui.legend.last_flash_time = Some(now);

    // Request repaint while flashing
    if !state.ui.legend.flash_annotations.is_empty() {
        ui.ctx().request_repaint();
    }

    ui.vertical(|ui| {
        // Header
        ui.horizontal(|ui| {
            ui.heading(crate::i18n::tr("legend.heading"));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .selectable_label(
                        state.ui.legend.group_by_subject,
                        crate::i18n::tr("legend.subject"),
                    )
                    .clicked()
                {
                    state.ui.legend.group_by_subject = true;
                }
                if ui
                    .selectable_label(
                        !state.ui.legend.group_by_subject,
                        crate::i18n::tr("legend.type"),
                    )
                    .clicked()
                {
                    state.ui.legend.group_by_subject = false;
                }
                ui.label(crate::i18n::tr("legend.group_by"));
            });
        });
        ui.separator();

        // Options row
        ui.horizontal(|ui| {
            ui.checkbox(
                &mut state.ui.legend.show_counts,
                crate::i18n::tr("legend.counts"),
            );
            ui.checkbox(
                &mut state.ui.legend.show_swatches,
                crate::i18n::tr("legend.color"),
            );
            ui.separator();
            ui.label("Filter:");
            ui.add(
                egui::TextEdit::singleline(&mut state.ui.legend.filter)
                    .desired_width(120.0)
                    .hint_text(crate::i18n::tr("legend.filter")),
            );
        });
        ui.separator();

        // Build legend entries
        let entries = build_legend_entries(state, !state.ui.legend.group_by_subject);

        // Apply filter
        let filter_lower = state.ui.legend.filter.to_lowercase();
        let filtered: Vec<&LegendEntry> = entries
            .iter()
            .filter(|e| {
                filter_lower.is_empty()
                    || e.label.to_lowercase().contains(&filter_lower)
                    || e.type_breakdown.to_lowercase().contains(&filter_lower)
            })
            .collect();

        if filtered.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.label(
                    RichText::new(crate::i18n::tr("legend.no_annots"))
                        .color(Color32::from_gray(120))
                        .italics(),
                );
            });
            return;
        }

        // Summary stats bar
        let total_annotations: usize = filtered.iter().map(|e| e.count).sum();
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!(
                    "{} groups  •  {} annotations",
                    filtered.len(),
                    total_annotations
                ))
                .small()
                .color(Color32::from_gray(140)),
            );
        });
        ui.add_space(4.0);

        // =====================================================================
        // Table with egui_extras::TableBuilder
        // =====================================================================
        let show_swatches = state.ui.legend.show_swatches;
        let show_counts = state.ui.legend.show_counts;

        // Collect actions to apply after the table (avoids borrowing conflicts)
        let mut click_action: Option<(Vec<(LayerId, String)>, Option<(f64, f64)>)> = None;
        let mut toggle_expand: Option<usize> = None;

        egui::ScrollArea::vertical()
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                let available_width = ui.available_width();

                // Column widths
                let swatch_w = if show_swatches { 20.0 } else { 0.0 };
                let expand_w = 20.0;
                let count_w = if show_counts { 50.0 } else { 0.0 };
                let value_w = 90.0;
                let type_w = 90.0;
                let name_w =
                    (available_width - swatch_w - expand_w - count_w - value_w - type_w - 20.0)
                        .max(60.0);

                let mut table_builder = TableBuilder::new(ui)
                    .striped(true)
                    .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                    .sense(Sense::click());

                // Build columns
                if show_swatches {
                    table_builder = table_builder.column(Column::exact(swatch_w));
                }
                table_builder = table_builder.column(Column::exact(expand_w)); // expand arrow
                table_builder =
                    table_builder.column(Column::initial(name_w).at_least(60.0).resizable(true)); // label
                if show_counts {
                    table_builder = table_builder.column(Column::exact(count_w));
                    // count
                }
                table_builder = table_builder.column(Column::initial(value_w).resizable(true)); // value
                table_builder = table_builder.column(Column::remainder().at_least(type_w)); // type breakdown

                // Header
                let table = table_builder.header(20.0, |mut header| {
                    if show_swatches {
                        header.col(|ui| {
                            ui.label(RichText::new("").small());
                        });
                    }
                    header.col(|_ui| {}); // expand
                    header.col(|ui| {
                        ui.label(
                            RichText::new(crate::i18n::tr("legend.symbol"))
                                .small()
                                .strong(),
                        );
                    });
                    if show_counts {
                        header.col(|ui| {
                            ui.label(
                                RichText::new(crate::i18n::tr("legend.count"))
                                    .small()
                                    .strong(),
                            );
                        });
                    }
                    header.col(|ui| {
                        ui.label(
                            RichText::new(crate::i18n::tr("legend.value"))
                                .small()
                                .strong(),
                        );
                    });
                    header.col(|ui| {
                        ui.label(
                            RichText::new(crate::i18n::tr("legend.types"))
                                .small()
                                .strong(),
                        );
                    });
                });

                // Body
                table.body(|mut body| {
                    for (idx, entry) in filtered.iter().enumerate() {
                        let is_expanded = state.ui.legend.expanded.contains(&idx);
                        let row_height = 22.0;

                        // Main row
                        body.row(row_height, |mut row| {
                            // Color swatch
                            if show_swatches {
                                row.col(|ui| {
                                    let (swatch_rect, _) = ui
                                        .allocate_exact_size(Vec2::new(14.0, 14.0), Sense::hover());
                                    let swatch_color = Color32::from_rgb(
                                        entry.color.r,
                                        entry.color.g,
                                        entry.color.b,
                                    );
                                    ui.painter().rect_filled(swatch_rect, 3.0, swatch_color);
                                    ui.painter().rect_stroke(
                                        swatch_rect,
                                        3.0,
                                        Stroke::new(1.0_f32, Color32::from_gray(80)),
                                    );
                                });
                            }

                            // Expand/collapse arrow
                            row.col(|ui| {
                                let arrow = if is_expanded { "v" } else { ">" };
                                if ui
                                    .add(
                                        egui::Label::new(
                                            RichText::new(arrow)
                                                .small()
                                                .color(Color32::from_gray(160)),
                                        )
                                        .sense(Sense::click()),
                                    )
                                    .clicked()
                                {
                                    toggle_expand = Some(idx);
                                }
                            });

                            // Label (clickable to flash + select)
                            row.col(|ui| {
                                let resp = ui.add(
                                    egui::Label::new(
                                        RichText::new(&entry.label).color(Color32::from_gray(220)),
                                    )
                                    .sense(Sense::click()),
                                );
                                if resp.clicked() {
                                    click_action =
                                        Some((entry.annotations.clone(), entry.centroid));
                                }
                                resp.on_hover_text(format!(
                                    "Click to flash {} annotations",
                                    entry.count
                                ));
                            });

                            // Count
                            if show_counts {
                                row.col(|ui| {
                                    ui.label(
                                        RichText::new(format!("{}", entry.count))
                                            .color(Color32::from_gray(200))
                                            .strong(),
                                    );
                                });
                            }

                            // Value column (length/area/count summary)
                            row.col(|ui| {
                                if let Some(ref total) = entry.total_length {
                                    ui.label(
                                        RichText::new(format!("Σ {}", total))
                                            .small()
                                            .color(Color32::from_rgb(100, 210, 130)),
                                    );
                                } else if let Some(ref total) = entry.total_area {
                                    ui.label(
                                        RichText::new(format!("Σ {}", total))
                                            .small()
                                            .color(Color32::from_rgb(100, 160, 220)),
                                    );
                                } else if let Some(count) = entry.count_marker_total {
                                    ui.label(
                                        RichText::new(format!("#{}", count))
                                            .small()
                                            .color(Color32::from_rgb(255, 180, 80)),
                                    );
                                } else {
                                    ui.label(
                                        RichText::new("—").small().color(Color32::from_gray(80)),
                                    );
                                }
                            });

                            // Type breakdown
                            row.col(|ui| {
                                ui.label(
                                    RichText::new(&entry.type_breakdown)
                                        .small()
                                        .color(Color32::from_gray(130)),
                                );
                            });
                        });

                        // Expanded: show individual annotations
                        if is_expanded {
                            for (layer_id, annot_id) in &entry.annotations {
                                body.row(18.0, |mut row| {
                                    // skip swatch column
                                    if show_swatches {
                                        row.col(|_ui| {});
                                    }
                                    // skip expand column
                                    row.col(|_ui| {});

                                    // Detail label spanning label + count columns
                                    row.col(|ui| {
                                        let annot_label = if let Some(layer) =
                                            state.session.layers.iter().find(|l| l.id == *layer_id)
                                        {
                                            if let Some(annot) =
                                                layer.annotations.iter().find(|a| a.id == *annot_id)
                                            {
                                                let tn = type_name(&annot.data);
                                                format!("{} · {}", tn, layer.name)
                                            } else {
                                                crate::i18n::tr("misc.unknown").to_owned()
                                            }
                                        } else {
                                            crate::i18n::tr("legend.unknown_layer").to_string()
                                        };

                                        let is_selected = state
                                            .session
                                            .selected_annotation
                                            .as_ref()
                                            .map_or(false, |(_, sid)| sid == annot_id);

                                        let text_color = if is_selected {
                                            Color32::from_rgb(100, 180, 255)
                                        } else {
                                            Color32::from_gray(160)
                                        };
                                        let resp = ui.add(
                                            egui::Label::new(
                                                RichText::new(&annot_label)
                                                    .small()
                                                    .color(text_color),
                                            )
                                            .sense(Sense::click()),
                                        );
                                        if resp.clicked() {
                                            click_action = Some((
                                                vec![(*layer_id, annot_id.clone())],
                                                None, // will compute centroid below
                                            ));
                                        }
                                    });

                                    // remaining columns empty
                                    if show_counts {
                                        row.col(|_ui| {});
                                    }
                                    row.col(|ui| {
                                        // Show individual value if available
                                        if let Some(layer) =
                                            state.session.layers.iter().find(|l| l.id == *layer_id)
                                        {
                                            if let Some(annot) =
                                                layer.annotations.iter().find(|a| a.id == *annot_id)
                                            {
                                                match &annot.data {
                                                    AnnotationData::Measurement {
                                                        label, ..
                                                    } => {
                                                        ui.label(
                                                            RichText::new(label)
                                                                .small()
                                                                .color(Color32::from_gray(150)),
                                                        );
                                                    }
                                                    AnnotationData::DimensionChain {
                                                        total_label,
                                                        ..
                                                    } => {
                                                        ui.label(
                                                            RichText::new(total_label)
                                                                .small()
                                                                .color(Color32::from_gray(150)),
                                                        );
                                                    }
                                                    AnnotationData::Count { number, .. } => {
                                                        ui.label(
                                                            RichText::new(format!("#{}", number))
                                                                .small()
                                                                .color(Color32::from_gray(150)),
                                                        );
                                                    }
                                                    _ => {}
                                                }
                                            }
                                        }
                                    });
                                    row.col(|_ui| {});
                                });
                            }
                        }
                    }
                });
            });

        // Apply deferred actions
        if let Some(idx) = toggle_expand {
            if state.ui.legend.expanded.contains(&idx) {
                state.ui.legend.expanded.remove(&idx);
            } else {
                state.ui.legend.expanded.insert(idx);
            }
        }

        if let Some((annotations, centroid)) = click_action {
            // Select the first annotation
            if let Some((layer_id, ref annot_id)) = annotations.first() {
                state.session.selected_layer = Some(*layer_id);
                state.session.selected_annotation = Some((*layer_id, annot_id.clone()));
            }

            // Flash all annotations in the group
            let flash_duration = 1.5; // seconds
            state.ui.legend.flash_annotations.clear();
            for (lid, aid) in &annotations {
                state
                    .ui
                    .legend
                    .flash_annotations
                    .push((*lid, aid.clone(), flash_duration));
            }

            // Pan viewport to centroid of the group
            let target_centroid = if let Some(c) = centroid {
                Some(c)
            } else if annotations.len() == 1 {
                // Compute centroid for single annotation
                let (lid, aid) = &annotations[0];
                state
                    .session
                    .layers
                    .iter()
                    .find(|l| l.id == *lid)
                    .and_then(|layer| {
                        layer.annotations.iter().find(|a| a.id == *aid).map(|a| {
                            let (min_x, min_y, max_x, max_y) = a.bounds;
                            (
                                ((min_x + max_x) / 2.0 + layer.offset_x) as f64,
                                ((min_y + max_y) / 2.0 + layer.offset_y) as f64,
                            )
                        })
                    })
            } else {
                None
            };

            if let Some((cx, cy)) = target_centroid {
                state.session.viewport.center_x = cx;
                state.session.viewport.center_y = cy;
            }

            ui.ctx().request_repaint();
        }
    });
}

/// Public helper: check if a given annotation should be drawn with a flash highlight.
/// Returns Some(alpha) if flashing, None otherwise.
/// Called by render_view to apply the flash effect.
pub fn is_annotation_flashing(state: &AppState, layer_id: LayerId, annot_id: &str) -> Option<f32> {
    flash_alpha(&state.ui.legend, layer_id, annot_id)
}
