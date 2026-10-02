// =============================================================================
// dc_app/panels/render_view - Main Render View
// =============================================================================
// The central viewport for displaying and comparing documents.
// Implements synchronized pan/zoom across all layers.
// =============================================================================

use crate::app::TextureCache;
use crate::state::{AppState, ToolMode};
use crate::undo::UndoCommand;
use dc_core::{
    compute_dimension_chain, point_distance, polygon_area, polyline_length, Annotation,
    AnnotationData, Point, ToolType,
};
use egui::{Color32, Pos2, Rect, Sense, Stroke, Ui, Vec2};

/// Render the main document view.
pub fn render_view(ui: &mut Ui, state: &mut AppState, textures: &mut TextureCache) {
    let available_rect = ui.available_rect_before_wrap();
    let response = ui.allocate_rect(available_rect, Sense::click_and_drag());

    // Wait for a usable canvas so startup layout cannot consume Fit at zero size.
    if state.ui.fit_view_requested
        && available_rect.width() > 64.0
        && available_rect.height() > 64.0
    {
        if let Some(layer) = state
            .session
            .visible_reference()
            .or_else(|| state.session.layers.first())
        {
            let (w, h) = layer.active_image().dimensions();
            state.session.viewport.fit_to_image(
                w,
                h,
                (available_rect.width() - 64.0).max(1.0) as f64,
                (available_rect.height() - 64.0).max(1.0) as f64,
            );
            state.ui.fit_view_requested = false;
        }
    }
    // Handle inputs based on tool mode
    match state.ui.tool_mode {
        ToolMode::Drawing => handle_drawing_input(ui, state, &response, available_rect),
        ToolMode::Zoom => {
            handle_viewport_input(ui, state, &response, available_rect);
            if response.clicked() {
                let factor = if ui.input(|i| i.modifiers.alt) {
                    1.0 / 1.5
                } else {
                    1.5
                };
                if let Some(mouse) = response.interact_pointer_pos() {
                    let center = (
                        available_rect.center().x as f64,
                        available_rect.center().y as f64,
                    );
                    let before = state.session.viewport.screen_to_image(
                        mouse.x as f64,
                        mouse.y as f64,
                        center,
                    );
                    state.session.viewport.apply_zoom(factor);
                    let after = state
                        .session
                        .viewport
                        .image_to_screen(before.0, before.1, center);
                    state
                        .session
                        .viewport
                        .pan(after.0 - mouse.x as f64, after.1 - mouse.y as f64);
                }
            }
            response.clone().on_hover_cursor(egui::CursorIcon::ZoomIn);
        }
        ToolMode::Select => handle_select_input(ui, state, &response, available_rect),
        _ => handle_viewport_input(ui, state, &response, available_rect),
    }

    // Draw background
    ui.painter()
        .rect_filled(available_rect, 0.0, crate::theme::CANVAS);

    // Draw content
    if state.session.layers.is_empty() {
        // Empty state
        draw_empty_state(ui, state, available_rect);
    } else {
        // Draw layers with textures
        draw_layers(ui, state, textures, available_rect);

        // Draw annotations on top
        draw_annotations(ui, state, available_rect);
    }

    // Draw pending annotation (active drawing)
    if let Some(pending) = &state.tools.pending_annotation {
        // Determine offset based on target layer (selected or reference)
        let offset = if let Some(layer_id) = state.session.selected_layer {
            state
                .session
                .get_layer(layer_id)
                .map(|l| (l.offset_x, l.offset_y))
                .unwrap_or((0.0, 0.0))
        } else if let Some(layer) = state.session.reference_layer() {
            (layer.offset_x, layer.offset_y)
        } else {
            (0.0, 0.0)
        };

        draw_single_annotation(ui, pending, state, available_rect, offset, pending.layer_id);
        ui.ctx().request_repaint(); // Animation/update loop during drawing
    }

    // Draw viewport info overlay
}

/// Find the innermost Viewport annotation that contains the given point (in layer-local coords).
/// Returns the viewport scale if the point is inside a viewport, or None for the global calibration.
fn find_viewport_scale_at(state: &AppState, point: Point, layer_offset: (f32, f32)) -> Option<f64> {
    find_viewport_scale_in_layers(&state.session.layers, point, layer_offset)
}

/// Same as find_viewport_scale_at but takes layers slice to avoid borrow conflicts.
fn find_viewport_scale_in_layers(
    layers: &[dc_core::Layer],
    point: Point,
    layer_offset: (f32, f32),
) -> Option<f64> {
    // Check all visible layers for Viewport annotations that contain this point
    let local_point = Point::new(point.x - layer_offset.0, point.y - layer_offset.1);
    let mut best_scale: Option<(f64, f64)> = None; // (area, scale) — smallest area wins (innermost)

    for layer in layers.iter().filter(|l| l.visible) {
        for annot in &layer.annotations {
            if let AnnotationData::Viewport {
                start, end, scale, ..
            } = &annot.data
            {
                let min_x = start.x.min(end.x);
                let max_x = start.x.max(end.x);
                let min_y = start.y.min(end.y);
                let max_y = start.y.max(end.y);

                // Check if point is inside the viewport rectangle
                // Use layer-local coords since viewport annotations are in layer-local space
                let check_point = Point::new(
                    local_point.x - layer.offset_x + layer_offset.0,
                    local_point.y - layer.offset_y + layer_offset.1,
                );
                if check_point.x >= min_x
                    && check_point.x <= max_x
                    && check_point.y >= min_y
                    && check_point.y <= max_y
                {
                    let area = ((max_x - min_x) * (max_y - min_y)) as f64;
                    match best_scale {
                        Some((best_area, _)) if area < best_area => {
                            best_scale = Some((area, *scale));
                        }
                        None => {
                            best_scale = Some((area, *scale));
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    best_scale.map(|(_, s)| s)
}

/// Handle viewport input (pan, zoom, etc.)
fn handle_viewport_input(ui: &Ui, state: &mut AppState, response: &egui::Response, rect: Rect) {
    let viewport = &mut state.session.viewport;

    // Pan with drag
    if response.dragged() {
        let delta = response.drag_delta();
        viewport.pan(-delta.x as f64, -delta.y as f64);
    }

    // Zoom with scroll wheel
    if response.hovered() {
        let scroll_delta = ui.input(|i| i.raw_scroll_delta.y);
        if scroll_delta != 0.0 {
            let zoom_factor = if scroll_delta > 0.0 { 1.1 } else { 0.9 };

            // Zoom towards mouse position
            if let Some(mouse_pos) = response.hover_pos() {
                let screen_center = (rect.center().x as f64, rect.center().y as f64);

                // Get image coordinates under mouse before zoom
                let (img_x, img_y) =
                    viewport.screen_to_image(mouse_pos.x as f64, mouse_pos.y as f64, screen_center);

                // Apply zoom
                viewport.apply_zoom(zoom_factor);

                // Adjust center so the point under mouse stays fixed
                let (new_screen_x, new_screen_y) =
                    viewport.image_to_screen(img_x, img_y, screen_center);

                let dx = new_screen_x - mouse_pos.x as f64;
                let dy = new_screen_y - mouse_pos.y as f64;
                viewport.pan(dx, dy);
            } else {
                viewport.apply_zoom(zoom_factor);
            }
        }
    }
}

/// Handle input when in Select mode
fn handle_select_input(ui: &Ui, state: &mut AppState, response: &egui::Response, rect: Rect) {
    let screen_center = (rect.center().x as f64, rect.center().y as f64);
    // Copy viewport to avoid holding borrow on state
    let viewport = state.session.viewport;

    // 1. Handle Active Dragging
    if state.ui.drag_state != crate::state::DragState::None {
        if response.drag_stopped() {
            // Push undo for annotation modification (move/resize)
            if let Some((layer_id, ref annot_id)) = state.session.selected_annotation {
                // Extract old_data from the DragState before clearing it
                let old_data = match &state.ui.drag_state {
                    crate::state::DragState::Moving { original_data, .. } => {
                        Some(original_data.as_ref().clone())
                    }
                    crate::state::DragState::DraggingHandle { original_data, .. } => {
                        Some(original_data.as_ref().clone())
                    }
                    _ => None,
                };
                if let Some(old_data) = old_data {
                    if let Some(layer) = state.session.get_layer(layer_id) {
                        if let Some(annot) = layer.annotations.iter().find(|a| a.id == *annot_id) {
                            if old_data != annot.data {
                                // Build old annotation snapshot (current annotation with original data)
                                let mut old_annot = annot.clone();
                                old_annot.data = old_data;
                                old_annot.bounds = old_annot.data.compute_bounds();
                                state
                                    .session
                                    .undo_stack
                                    .push(UndoCommand::ModifyAnnotation {
                                        layer_id,
                                        old_annotation: Box::new(old_annot),
                                        new_annotation: Box::new(annot.clone()),
                                    });
                            }
                        }
                    }
                }
            }
            state.ui.drag_state = crate::state::DragState::None;
            state.session.is_dirty = true;
            return;
        }

        if let Some(mouse_pos) = response.hover_pos() {
            let (curr_x, curr_y) =
                viewport.screen_to_image(mouse_pos.x as f64, mouse_pos.y as f64, screen_center);
            let curr_point = Point::new(curr_x as f32, curr_y as f32);

            update_drag(state, curr_point);
        }
        return; // Don't pan while dragging
    }

    // 2. Check for Drag Start (Pointer Down or Drag Started)
    // We check this before viewport input to intercept interaction with objects
    if response.drag_started() {
        if let Some(mouse_pos) = response.hover_pos() {
            let (img_x, img_y) =
                viewport.screen_to_image(mouse_pos.x as f64, mouse_pos.y as f64, screen_center);
            let click_point = Point::new(img_x as f32, img_y as f32);

            // A. Check Handles of Selected Annotation
            // Clone ID to avoid borrowing state.session
            if let Some((layer_id, annot_id)) = state.session.selected_annotation.clone() {
                if let Some(layer) = state.session.get_layer(layer_id) {
                    if let Some(annot) = layer.annotations.iter().find(|a| a.id == annot_id) {
                        let offset = (layer.offset_x, layer.offset_y);
                        if let Some(handle_idx) =
                            hit_test_handles(annot, &viewport, mouse_pos, screen_center, offset)
                        {
                            let drag_data = annot.data.clone();
                            // If we are dragging handles, we are manipulating local coordinates.
                            // The input 'click_point' was global.
                            // Wait, 'hit_test_handles' returns index.
                            // 'click_point' is stored as start pos.
                            // If we store global point, we compare diffs.
                            // update_drag uses delta, so global vs local delta is same (offset cancels out).

                            state.ui.drag_state = crate::state::DragState::DraggingHandle {
                                handle_index: handle_idx,
                                start_mouse_pos: click_point, // This is global point, but it's okay for delta calc
                                original_data: Box::new(drag_data),
                            };
                            return; // Intercepted
                        }
                    }
                }
            }

            // B. Check Body of ANY Annotation (Top-most)
            // We use the same hit-testing logic as selection
            let tolerance = 5.0 / viewport.zoom as f32;
            let mut hit_annot = None;

            'hit_test: for layer in state.session.visible_layers().iter().rev() {
                // Convert global click point to layer local space
                let local_point = Point::new(
                    click_point.x - layer.offset_x,
                    click_point.y - layer.offset_y,
                );

                for annotation in layer.annotations.iter().rev() {
                    if annotation.data.contains(local_point, tolerance) {
                        hit_annot =
                            Some((layer.id, annotation.id.clone(), annotation.data.clone()));
                        break 'hit_test;
                    }
                }
            }

            if let Some((layer_id, annot_id, annot_data)) = hit_annot {
                // Select it first
                state.session.selected_annotation = Some((layer_id, annot_id));
                state.session.is_dirty = true;

                // Start moving
                state.ui.drag_state = crate::state::DragState::Moving {
                    start_mouse_pos: click_point,
                    original_data: Box::new(annot_data),
                };
                return; // Intercepted
            }
        }
    }

    // 3. Viewport (Pan/Zoom) - Only if we didn't intercept
    handle_viewport_input(ui, state, response, rect);

    // 4. Selection (Click without drag)
    if response.clicked() {
        if let Some(mouse_pos) = response.hover_pos() {
            let (img_x, img_y) =
                viewport.screen_to_image(mouse_pos.x as f64, mouse_pos.y as f64, screen_center);
            let click_point = Point::new(img_x as f32, img_y as f32);

            // Allow deselect if clicking empty space
            let mut found: Option<(dc_core::LayerId, String, AnnotationData, (f32, f32))> = None;
            let tolerance = 5.0 / viewport.zoom as f32; // Tolerance in image pixels

            'outer: for layer in state.session.visible_layers().iter().rev() {
                let local_point = Point::new(
                    click_point.x - layer.offset_x,
                    click_point.y - layer.offset_y,
                );
                for annotation in layer.annotations.iter().rev() {
                    if annotation.data.contains(local_point, tolerance) {
                        found = Some((
                            layer.id,
                            annotation.id.clone(),
                            annotation.data.clone(),
                            (layer.offset_x, layer.offset_y),
                        ));
                        break 'outer;
                    }
                }
            }

            state.session.selected_annotation = found
                .as_ref()
                .map(|(layer_id, annot_id, _, _)| (*layer_id, annot_id.clone()));

            // Double-click on a viewport to frame and zoom into it.
            if response.double_clicked() {
                if let Some((_, _, AnnotationData::Viewport { start, end, .. }, (ox, oy))) = found {
                    let min_x = start.x.min(end.x) + ox;
                    let min_y = start.y.min(end.y) + oy;
                    let max_x = start.x.max(end.x) + ox;
                    let max_y = start.y.max(end.y) + oy;

                    let vp_w = (max_x - min_x).abs().max(1.0) as f64;
                    let vp_h = (max_y - min_y).abs().max(1.0) as f64;
                    let fit_w = (rect.width() * 0.9) as f64;
                    let fit_h = (rect.height() * 0.9) as f64;
                    let target_zoom = (fit_w / vp_w).min(fit_h / vp_h).clamp(0.01, 100.0);

                    state.session.viewport.center_x = ((min_x + max_x) * 0.5) as f64;
                    state.session.viewport.center_y = ((min_y + max_y) * 0.5) as f64;
                    state.session.viewport.zoom = target_zoom;
                }
            }
            state.session.is_dirty = true;
        }
    }
}

/// Helper: Update geometry during drag
fn update_drag(state: &mut AppState, curr_point: Point) {
    let (layer_id, annot_id) = match state.session.selected_annotation.clone() {
        Some(id) => id,
        None => return, // Should not happen if dragging
    };

    // We clone DragState info to avoid borrow checker issues with state
    let (handle_idx, start_pos, original_data) = match &state.ui.drag_state {
        crate::state::DragState::Moving {
            start_mouse_pos,
            original_data,
        } => (None, *start_mouse_pos, original_data.clone()),
        crate::state::DragState::DraggingHandle {
            handle_index,
            start_mouse_pos,
            original_data,
        } => (Some(*handle_index), *start_mouse_pos, original_data.clone()),
        _ => return,
    };

    if let Some(layer) = state.session.get_layer_mut(layer_id) {
        if let Some(annotation) = layer.annotations.iter_mut().find(|a| a.id == annot_id) {
            let delta = Point::new(curr_point.x - start_pos.x, curr_point.y - start_pos.y);

            match handle_idx {
                Some(idx) => apply_handle_move(&mut annotation.data, &original_data, idx, delta),
                None => apply_move(&mut annotation.data, &original_data, delta),
            }

            // Update bounds
            annotation.bounds = annotation.data.compute_bounds();
        }
    }
}

/// Apply move to entire annotation
fn apply_move(data: &mut AnnotationData, original: &AnnotationData, delta: Point) {
    match (data, original) {
        (
            AnnotationData::Rectangle { start, end },
            AnnotationData::Rectangle {
                start: o_s,
                end: o_e,
            },
        )
        | (
            AnnotationData::Ellipse { start, end },
            AnnotationData::Ellipse {
                start: o_s,
                end: o_e,
            },
        )
        | (
            AnnotationData::Line { start, end },
            AnnotationData::Line {
                start: o_s,
                end: o_e,
            },
        ) => {
            *start = Point::new(o_s.x + delta.x, o_s.y + delta.y);
            *end = Point::new(o_e.x + delta.x, o_e.y + delta.y);
        }
        (AnnotationData::Path(points), AnnotationData::Path(orig_points))
        | (AnnotationData::Cloud(points), AnnotationData::Cloud(orig_points)) => {
            if points.len() == orig_points.len() {
                for (i, p) in points.iter_mut().enumerate() {
                    p.x = orig_points[i].x + delta.x;
                    p.y = orig_points[i].y + delta.y;
                }
            }
        }
        (AnnotationData::Text { pos, .. }, AnnotationData::Text { pos: o_p, .. }) => {
            pos.x = o_p.x + delta.x;
            pos.y = o_p.y + delta.y;
        }
        (
            AnnotationData::Measurement { points, .. },
            AnnotationData::Measurement {
                points: orig_points,
                ..
            },
        ) => {
            if points.len() == orig_points.len() {
                for (i, p) in points.iter_mut().enumerate() {
                    p.x = orig_points[i].x + delta.x;
                    p.y = orig_points[i].y + delta.y;
                }
            }
        }
        (AnnotationData::Count { pos, .. }, AnnotationData::Count { pos: o_p, .. }) => {
            pos.x = o_p.x + delta.x;
            pos.y = o_p.y + delta.y;
        }
        (
            AnnotationData::Viewport { start, end, .. },
            AnnotationData::Viewport {
                start: o_s,
                end: o_e,
                ..
            },
        ) => {
            *start = Point::new(o_s.x + delta.x, o_s.y + delta.y);
            *end = Point::new(o_e.x + delta.x, o_e.y + delta.y);
        }
        (
            AnnotationData::DimensionChain { points, .. },
            AnnotationData::DimensionChain {
                points: orig_points,
                ..
            },
        ) => {
            if points.len() == orig_points.len() {
                for (i, p) in points.iter_mut().enumerate() {
                    p.x = orig_points[i].x + delta.x;
                    p.y = orig_points[i].y + delta.y;
                }
            }
        }
        _ => {}
    }
}

/// Apply move to a specific handle
fn apply_handle_move(
    data: &mut AnnotationData,
    original: &AnnotationData,
    idx: usize,
    delta: Point,
) {
    // Current Handle Mapping:
    // Rect: 0=Start(TopLeft?), 1=End(BottomRight?)
    // Line: 0=Start, 1=End

    // Simplistic handle logic: just add delta to the specific point
    match (data, original) {
        (
            AnnotationData::Rectangle { start, end },
            AnnotationData::Rectangle {
                start: o_s,
                end: o_e,
            },
        ) => {
            if idx == 0 {
                *start = Point::new(o_s.x + delta.x, o_s.y + delta.y);
            } else if idx == 1 {
                *end = Point::new(o_e.x + delta.x, o_e.y + delta.y);
            }
        }
        (
            AnnotationData::Ellipse { start, end },
            AnnotationData::Ellipse {
                start: o_s,
                end: o_e,
            },
        ) => {
            if idx == 0 {
                *start = Point::new(o_s.x + delta.x, o_s.y + delta.y);
            } else if idx == 1 {
                *end = Point::new(o_e.x + delta.x, o_e.y + delta.y);
            }
        }
        (
            AnnotationData::Line { start, end },
            AnnotationData::Line {
                start: o_s,
                end: o_e,
            },
        ) => {
            if idx == 0 {
                *start = Point::new(o_s.x + delta.x, o_s.y + delta.y);
            } else if idx == 1 {
                *end = Point::new(o_e.x + delta.x, o_e.y + delta.y);
            }
        }
        (AnnotationData::Text { pos, .. }, AnnotationData::Text { pos: o_p, .. }) => {
            if idx == 0 {
                *pos = Point::new(o_p.x + delta.x, o_p.y + delta.y);
            }
        }
        (
            AnnotationData::Measurement { points, .. },
            AnnotationData::Measurement {
                points: o_points, ..
            },
        ) => {
            if idx < points.len() && idx < o_points.len() {
                points[idx] = Point::new(o_points[idx].x + delta.x, o_points[idx].y + delta.y);
            }
        }
        (AnnotationData::Count { pos, .. }, AnnotationData::Count { pos: o_p, .. }) => {
            if idx == 0 {
                *pos = Point::new(o_p.x + delta.x, o_p.y + delta.y);
            }
        }
        (
            AnnotationData::Viewport { start, end, .. },
            AnnotationData::Viewport {
                start: o_s,
                end: o_e,
                ..
            },
        ) => {
            if idx == 0 {
                *start = Point::new(o_s.x + delta.x, o_s.y + delta.y);
            } else if idx == 1 {
                *end = Point::new(o_e.x + delta.x, o_e.y + delta.y);
            }
        }
        (
            AnnotationData::DimensionChain { points, .. },
            AnnotationData::DimensionChain {
                points: o_points, ..
            },
        ) => {
            if idx < points.len() && idx < o_points.len() {
                points[idx] = Point::new(o_points[idx].x + delta.x, o_points[idx].y + delta.y);
            }
        }
        _ => {}
    }
}

/// Check if mouse over a handle
fn hit_test_handles(
    annot: &Annotation,
    viewport: &dc_core::Viewport,
    mouse_pos: Pos2,
    screen_center: (f64, f64),
    offset: (f32, f32),
) -> Option<usize> {
    let handle_radius = 6.0; // Screen pixels

    // Define handles based on type
    let handles: Vec<Point> = match &annot.data {
        AnnotationData::Rectangle { start, end } => vec![*start, *end],
        AnnotationData::Ellipse { start, end } => vec![*start, *end],
        AnnotationData::Line { start, end } => vec![*start, *end],
        AnnotationData::Text { pos, .. } => vec![*pos],
        AnnotationData::Count { pos, .. } => vec![*pos],
        AnnotationData::Measurement { points, .. } => points.clone(),
        AnnotationData::Viewport { start, end, .. } => vec![*start, *end],
        AnnotationData::DimensionChain { points, .. } => points.clone(),
        _ => vec![],
    };

    for (i, handle) in handles.iter().enumerate() {
        let (sx, sy) = viewport.image_to_screen(
            (handle.x + offset.0) as f64,
            (handle.y + offset.1) as f64,
            screen_center,
        );
        let screen_pt = Pos2::new(sx as f32, sy as f32);

        if screen_pt.distance(mouse_pos) <= handle_radius {
            return Some(i);
        }
    }

    None
}

/// Draw the empty state placeholder.
fn draw_empty_state(ui: &mut Ui, state: &mut AppState, rect: Rect) {
    let center = rect.center();
    let paper = Rect::from_center_size(center + Vec2::new(0.0, -94.0), Vec2::new(42.0, 54.0));
    let painter = ui.painter();
    painter.rect_stroke(
        paper.translate(Vec2::new(-8.0, -6.0)),
        3.0,
        Stroke::new(1.0_f32, Color32::from_gray(86)),
    );
    painter.rect_filled(paper, 3.0, Color32::from_rgb(43, 47, 54));
    painter.rect_stroke(paper, 3.0, Stroke::new(1.0_f32, crate::theme::ACCENT));
    for y in [0.3, 0.45, 0.6, 0.75] {
        painter.line_segment(
            [
                egui::pos2(paper.min.x + 9.0, paper.min.y + paper.height() * y),
                egui::pos2(paper.max.x - 9.0, paper.min.y + paper.height() * y),
            ],
            Stroke::new(1.0_f32, Color32::from_rgb(107, 138, 179)),
        );
    }
    painter.text(
        center + Vec2::new(0.0, -35.0),
        egui::Align2::CENTER_CENTER,
        crate::i18n::tr("workspace.empty_title"),
        egui::FontId::proportional(22.0),
        crate::theme::TEXT,
    );
    painter.text(
        center + Vec2::new(0.0, -3.0),
        egui::Align2::CENTER_CENTER,
        crate::i18n::tr("workspace.empty_hint"),
        egui::FontId::proportional(12.0),
        crate::theme::MUTED,
    );
    let button = Rect::from_center_size(center + Vec2::new(0.0, 41.0), Vec2::new(160.0, 34.0));
    if ui
        .put(
            button,
            egui::Button::new(
                egui::RichText::new(crate::i18n::tr("workspace.open")).color(Color32::WHITE),
            )
            .fill(Color32::from_rgb(48, 109, 192)),
        )
        .clicked()
    {
        state.ui.request_file_dialog = true;
    }
    ui.painter().text(
        center + Vec2::new(0.0, 84.0),
        egui::Align2::CENTER_CENTER,
        crate::i18n::tr("workspace.drop_hint"),
        egui::FontId::proportional(11.0),
        crate::theme::MUTED,
    );
}

/// Draw all visible layers.
fn draw_layers(ui: &Ui, state: &AppState, textures: &mut TextureCache, rect: Rect) {
    let painter = ui.painter();
    let viewport = &state.session.viewport;
    let screen_center = (rect.center().x as f64, rect.center().y as f64);

    // Get visible layers
    let visible_layers = state.session.visible_layers();

    if visible_layers.is_empty() {
        // All layers hidden
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            crate::i18n::tr("render.all_hidden"),
            egui::FontId::proportional(16.0),
            Color32::from_gray(100),
        );
        return;
    }

    // Draw each visible layer (in reverse order so first layer is on top)
    for layer in visible_layers.iter().rev() {
        let image = layer.active_image();
        let (img_w, img_h) = image.dimensions();

        if img_w == 0 || img_h == 0 {
            continue;
        }

        // Calculate screen position for image corners
        let (top_left_x, top_left_y) =
            viewport.image_to_screen(layer.offset_x as f64, layer.offset_y as f64, screen_center);
        let (bot_right_x, bot_right_y) = viewport.image_to_screen(
            img_w as f64 + layer.offset_x as f64,
            img_h as f64 + layer.offset_y as f64,
            screen_center,
        );

        let screen_rect = Rect::from_min_max(
            Pos2::new(top_left_x as f32, top_left_y as f32),
            Pos2::new(bot_right_x as f32, bot_right_y as f32),
        );

        // Only draw if visible on screen
        if !rect.intersects(screen_rect) {
            continue;
        }

        // Get or create tiled texture for this layer
        if let Some(tiled_texture) = textures.get_or_create(ui.ctx(), layer.id.0 as u64, image) {
            // Calculate opacity based on layer settings
            let tint =
                Color32::from_rgba_unmultiplied(255, 255, 255, (layer.opacity * 255.0) as u8);

            // Render each tile
            for tile in tiled_texture.tiles() {
                // Calculate screen position for this tile
                let (tile_left, tile_top) = viewport.image_to_screen(
                    tile.x as f64 + layer.offset_x as f64,
                    tile.y as f64 + layer.offset_y as f64,
                    screen_center,
                );
                let (tile_right, tile_bottom) = viewport.image_to_screen(
                    (tile.x + tile.width) as f64 + layer.offset_x as f64,
                    (tile.y + tile.height) as f64 + layer.offset_y as f64,
                    screen_center,
                );

                let tile_rect = Rect::from_min_max(
                    Pos2::new(tile_left as f32, tile_top as f32),
                    Pos2::new(tile_right as f32, tile_bottom as f32),
                );

                // Only draw tile if visible on screen
                if !rect.intersects(tile_rect) {
                    continue;
                }

                // Draw this tile
                painter.image(
                    tile.texture().id(),
                    tile_rect,
                    Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(1.0, 1.0)),
                    tint,
                );
            }

            // If this is not the reference layer, optionally apply a color tint overlay
            if !layer.is_reference && layer.opacity < 1.0 {
                let overlay_color = Color32::from_rgba_unmultiplied(
                    layer.blend_color.r,
                    layer.blend_color.g,
                    layer.blend_color.b,
                    ((1.0 - layer.opacity) * 50.0) as u8,
                );
                painter.rect_filled(screen_rect, 0.0, overlay_color);
            }
        } else {
            // Fallback: draw colored placeholder if texture upload failed
            let color = Color32::from_rgba_premultiplied(
                layer.blend_color.r,
                layer.blend_color.g,
                layer.blend_color.b,
                (layer.opacity * 128.0) as u8,
            );
            painter.rect_filled(screen_rect, 0.0, color);
        }

        painter.rect_stroke(
            screen_rect,
            0.0,
            Stroke::new(1.0_f32, Color32::from_gray(74)),
        );
    }

    // Draw diff result overlay if available
    if let Some(diff_result) = &state.session.diff_result {
        let (img_w, img_h) = diff_result.dimensions();

        if img_w > 0 && img_h > 0 {
            let origin = state
                .session
                .visible_reference()
                .map(|l| (l.offset_x as f64, l.offset_y as f64))
                .unwrap_or((0.0, 0.0));
            let (top_left_x, top_left_y) =
                viewport.image_to_screen(origin.0, origin.1, screen_center);
            let (bot_right_x, bot_right_y) = viewport.image_to_screen(
                img_w as f64 + origin.0,
                img_h as f64 + origin.1,
                screen_center,
            );

            let screen_rect = Rect::from_min_max(
                Pos2::new(top_left_x as f32, top_left_y as f32),
                Pos2::new(bot_right_x as f32, bot_right_y as f32),
            );

            // Only draw if visible on screen
            if rect.intersects(screen_rect) {
                // Use a special constant ID for the diff texture
                const DIFF_TEXTURE_ID: u64 = u64::MAX;

                // Get or create tiled texture for the diff result
                if let Some(tiled_texture) =
                    textures.get_or_create(ui.ctx(), DIFF_TEXTURE_ID, diff_result)
                {
                    // Render each tile with full opacity
                    for tile in tiled_texture.tiles() {
                        // Calculate screen position for this tile
                        let (tile_left, tile_top) =
                            viewport.image_to_screen(tile.x as f64, tile.y as f64, screen_center);
                        let (tile_right, tile_bottom) = viewport.image_to_screen(
                            (tile.x + tile.width) as f64,
                            (tile.y + tile.height) as f64,
                            screen_center,
                        );

                        let tile_rect = Rect::from_min_max(
                            Pos2::new(tile_left as f32, tile_top as f32),
                            Pos2::new(tile_right as f32, tile_bottom as f32),
                        );

                        // Only draw tile if visible on screen
                        if rect.intersects(tile_rect) {
                            // Draw this tile
                            painter.image(
                                tile.texture().id(),
                                tile_rect,
                                Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(1.0, 1.0)),
                                Color32::WHITE,
                            );
                        }
                    }
                }
            }
        }
    }
}

/// Handle drawing gestures for the active tool.
fn handle_drawing_input(ui: &Ui, state: &mut AppState, response: &egui::Response, rect: Rect) {
    // Only proceed if we have an active tool and a selected layer
    let active_tool = match state.tools.active_tool.clone() {
        Some(t) => t,
        None => return,
    };

    // Use selected layer or reference layer if none selected?
    let layer_id = match state.session.selected_layer {
        Some(id) => id,
        None => {
            if let Some(layer) = state.session.reference_layer() {
                layer.id
            } else {
                return;
            }
        }
    };

    // Zoom with scroll wheel (mirrors handle_viewport_input logic)
    if response.hovered() {
        let scroll_delta = ui.input(|i| i.raw_scroll_delta.y);
        if scroll_delta != 0.0 {
            let zoom_factor = if scroll_delta > 0.0 { 1.1 } else { 0.9 };
            let viewport = &mut state.session.viewport;

            // Zoom towards mouse position
            if let Some(mouse_pos) = response.hover_pos() {
                let screen_center = (rect.center().x as f64, rect.center().y as f64);

                // Get image coordinates under mouse before zoom
                let (img_x, img_y) =
                    viewport.screen_to_image(mouse_pos.x as f64, mouse_pos.y as f64, screen_center);

                // Apply zoom
                viewport.apply_zoom(zoom_factor);

                // Adjust center so the point under mouse stays fixed
                let (new_screen_x, new_screen_y) =
                    viewport.image_to_screen(img_x, img_y, screen_center);

                let dx = new_screen_x - mouse_pos.x as f64;
                let dy = new_screen_y - mouse_pos.y as f64;
                viewport.pan(dx, dy);
            } else {
                viewport.apply_zoom(zoom_factor);
            }
        }
    }

    let viewport = &state.session.viewport;
    let screen_center = (rect.center().x as f64, rect.center().y as f64);

    // =========================================================================
    // DimensionChain: multi-point click interaction (finish with Esc)
    // =========================================================================
    if active_tool.tool_type == ToolType::DimensionChain {
        // Get layer offset
        let (ox, oy) = if let Some(layer) = state.session.get_layer(layer_id) {
            (layer.offset_x, layer.offset_y)
        } else {
            (0.0, 0.0)
        };

        // Check for Esc to finalize
        let esc_pressed = ui.input(|i| i.key_pressed(egui::Key::Escape));
        if esc_pressed {
            if let Some(mut annotation) = state.tools.pending_annotation.take() {
                // Remove the ghost trailing point
                if let AnnotationData::DimensionChain { ref mut points, .. } = annotation.data {
                    if points.len() > 2 {
                        points.pop(); // remove ghost
                    }
                }
                // Recompute chain values after removing ghost
                if let AnnotationData::DimensionChain {
                    ref points,
                    ref mut segment_values,
                    ref mut segment_labels,
                    ref mut total_value,
                    ref mut total_label,
                    ..
                } = annotation.data
                {
                    let cal = &state.session.calibration;
                    let vp_scale = if let Some(p) = points.first() {
                        find_viewport_scale_in_layers(
                            &state.session.layers,
                            Point::new(p.x + ox, p.y + oy),
                            (ox, oy),
                        )
                    } else {
                        None
                    };
                    let (sv, sl, tv, tl) = compute_dimension_chain(points, cal, vp_scale);
                    *segment_values = sv;
                    *segment_labels = sl;
                    *total_value = tv;
                    *total_label = tl;
                }
                // Only commit if we have at least 2 real points
                let point_count = match &annotation.data {
                    AnnotationData::DimensionChain { points, .. } => points.len(),
                    _ => 0,
                };
                if point_count >= 2 {
                    annotation.bounds = annotation.data.compute_bounds();
                    let annotation_clone = annotation.clone();
                    if let Some(layer) = state.session.get_layer_mut(layer_id) {
                        layer.annotations.push(annotation);
                        state.session.is_dirty = true;
                        state.session.undo_stack.push(UndoCommand::AddAnnotation {
                            layer_id,
                            annotation: Box::new(annotation_clone),
                        });
                    }
                }
            }
            return;
        }

        // Click to add a new node
        if response.clicked() {
            if let Some(mouse_pos) = response.hover_pos() {
                let (x, y) =
                    viewport.screen_to_image(mouse_pos.x as f64, mouse_pos.y as f64, screen_center);
                let point = Point::new(x as f32 - ox, y as f32 - oy);

                if state.tools.pending_annotation.is_none() {
                    // First click: create chain with 1 real point + 1 ghost
                    let data = AnnotationData::DimensionChain {
                        points: vec![point, point], // second is ghost
                        segment_values: vec![],
                        segment_labels: vec![],
                        total_value: 0.0,
                        total_label: String::new(),
                    };
                    let annotation = Annotation::new(layer_id, active_tool.style.clone(), data);
                    state.tools.pending_annotation = Some(annotation);
                } else if let Some(ref mut annotation) = state.tools.pending_annotation {
                    // Subsequent click: commit the ghost point in place
                    // and add a new ghost trailing point
                    if let AnnotationData::DimensionChain {
                        ref mut points,
                        ref mut segment_values,
                        ref mut segment_labels,
                        ref mut total_value,
                        ref mut total_label,
                    } = annotation.data
                    {
                        // The last point is the ghost — snap it to click position
                        if let Some(last) = points.last_mut() {
                            *last = point;
                        }
                        // Add a new ghost point at same position
                        points.push(point);

                        // Recompute chain values
                        let cal = &state.session.calibration;
                        let vp_scale = find_viewport_scale_in_layers(
                            &state.session.layers,
                            Point::new(point.x + ox, point.y + oy),
                            (ox, oy),
                        );
                        let (sv, sl, tv, tl) = compute_dimension_chain(points, cal, vp_scale);
                        *segment_values = sv;
                        *segment_labels = sl;
                        *total_value = tv;
                        *total_label = tl;
                    }
                }
            }
        }

        // Update ghost point to track mouse for live preview
        if let Some(mouse_pos) = response.hover_pos() {
            if let Some(ref mut annotation) = state.tools.pending_annotation {
                let (x, y) =
                    viewport.screen_to_image(mouse_pos.x as f64, mouse_pos.y as f64, screen_center);
                let point = Point::new(x as f32 - ox, y as f32 - oy);

                if let AnnotationData::DimensionChain {
                    ref mut points,
                    ref mut segment_values,
                    ref mut segment_labels,
                    ref mut total_value,
                    ref mut total_label,
                } = annotation.data
                {
                    // Update the last (ghost) point
                    if let Some(last) = points.last_mut() {
                        *last = point;
                    }
                    // Recompute chain values
                    let cal = &state.session.calibration;
                    let vp_scale = find_viewport_scale_in_layers(
                        &state.session.layers,
                        Point::new(point.x + ox, point.y + oy),
                        (ox, oy),
                    );
                    let (sv, sl, tv, tl) = compute_dimension_chain(points, cal, vp_scale);
                    *segment_values = sv;
                    *segment_labels = sl;
                    *total_value = tv;
                    *total_label = tl;
                }
                ui.ctx().request_repaint();
            }
        }

        return; // DimensionChain handled — skip drag-based logic
    }

    // Drawing Logic (drag-based tools)
    if response.dragged_by(egui::PointerButton::Primary) {
        if let Some(mouse_pos) = response.hover_pos() {
            // Convert screen pos to image pos, then to local layer pos
            let (x, y) =
                viewport.screen_to_image(mouse_pos.x as f64, mouse_pos.y as f64, screen_center);

            // Get layer offset
            let (ox, oy) = if let Some(layer) = state.session.get_layer(layer_id) {
                (layer.offset_x, layer.offset_y)
            } else {
                (0.0, 0.0)
            };

            let point = Point::new(x as f32 - ox, y as f32 - oy);

            // Should we start a new one?
            if state.tools.pending_annotation.is_none() {
                let data = match active_tool.tool_type {
                    ToolType::Pen | ToolType::Highlighter => AnnotationData::Path(vec![point]),
                    ToolType::Rectangle => AnnotationData::Rectangle {
                        start: point,
                        end: point,
                    },
                    ToolType::Ellipse => AnnotationData::Ellipse {
                        start: point,
                        end: point,
                    },
                    ToolType::Line | ToolType::Arrow => AnnotationData::Line {
                        start: point,
                        end: point,
                    },
                    ToolType::Cloud => AnnotationData::Cloud(vec![point]),
                    ToolType::Text | ToolType::Callout => AnnotationData::Text {
                        pos: point,
                        content: String::new(),
                        width: None,
                    },
                    ToolType::MeasureLength => AnnotationData::Measurement {
                        points: vec![point, point],
                        is_area: false,
                        value: 0.0,
                        label: String::new(),
                    },
                    ToolType::MeasurePolylength => AnnotationData::Measurement {
                        points: vec![point],
                        is_area: false,
                        value: 0.0,
                        label: String::new(),
                    },
                    ToolType::MeasureArea => AnnotationData::Measurement {
                        points: vec![point],
                        is_area: true,
                        value: 0.0,
                        label: String::new(),
                    },
                    ToolType::Count => {
                        // If a sequence group is active, derive the number
                        // from the highest existing number in that group.
                        // This prevents duplicate IDs after Undo/Redo.
                        let (num, group) = if let Some(ref gid) = state.tools.active_sequence_group
                        {
                            let n = dc_core::next_sequence_number(&state.session.layers, gid);
                            // Register the group if not already known
                            if !state.session.sequence_groups.contains(gid) {
                                state.session.sequence_groups.push(gid.clone());
                            }
                            (n, Some(gid.clone()))
                        } else {
                            let n = state.session.count_counter;
                            state.session.count_counter += 1;
                            (n, None)
                        };
                        AnnotationData::Count {
                            pos: point,
                            number: num,
                            sequence_group_id: group,
                        }
                    }
                    ToolType::Viewport => AnnotationData::Viewport {
                        start: point,
                        end: point,
                        scale: state.session.calibration.pixels_per_unit,
                        label: String::new(),
                    },
                    ToolType::DimensionChain => {
                        // DimensionChain uses click-based interaction above;
                        // this path should not be reached.
                        return;
                    }
                };

                let annotation = Annotation::new(layer_id, active_tool.style.clone(), data);
                state.tools.pending_annotation = Some(annotation);
            } else if let Some(annotation) = &mut state.tools.pending_annotation {
                // Update existing annotation
                match &mut annotation.data {
                    AnnotationData::Path(points) | AnnotationData::Cloud(points) => {
                        // Add point if far enough from last point (simple smoothing)
                        if let Some(last) = points.last() {
                            let dist =
                                ((last.x - point.x).powi(2) + (last.y - point.y).powi(2)).sqrt();
                            if dist > 2.0 / viewport.zoom as f32 {
                                points.push(point);
                            }
                        } else {
                            points.push(point);
                        }
                    }
                    AnnotationData::Rectangle { end, .. }
                    | AnnotationData::Ellipse { end, .. }
                    | AnnotationData::Line { end, .. } => {
                        *end = point;
                    }
                    AnnotationData::Measurement {
                        points,
                        is_area,
                        value,
                        label,
                    } => {
                        // For length: always update the second point
                        if active_tool.tool_type == ToolType::MeasureLength {
                            if points.len() == 2 {
                                points[1] = point;
                            }
                        } else {
                            // For polylength/area: add point if far enough
                            if let Some(last) = points.last() {
                                let dist = ((last.x - point.x).powi(2)
                                    + (last.y - point.y).powi(2))
                                .sqrt();
                                if dist > 2.0 / viewport.zoom as f32 {
                                    points.push(point);
                                }
                            } else {
                                points.push(point);
                            }
                        }
                        // Recompute measurement (viewport-aware)
                        let cal = &state.session.calibration;
                        let vp_scale = find_viewport_scale_in_layers(
                            &state.session.layers,
                            Point::new(point.x + ox, point.y + oy),
                            (ox, oy),
                        );
                        if *is_area {
                            let area_px = polygon_area(points);
                            *value = area_px;
                            *label = match vp_scale {
                                Some(s) => cal.format_area_with_scale(area_px, s),
                                None => cal.format_area(area_px),
                            };
                        } else {
                            let len_px = if points.len() == 2 {
                                point_distance(points[0], points[1])
                            } else {
                                polyline_length(points)
                            };
                            *value = len_px;
                            *label = match vp_scale {
                                Some(s) => cal.format_length_with_scale(len_px, s),
                                None => cal.format_length(len_px),
                            };
                        }
                    }
                    AnnotationData::Viewport { end, .. } => {
                        *end = point;
                    }
                    AnnotationData::DimensionChain { .. } => {
                        // DimensionChain uses click-based interaction;
                        // drag updates are handled in the click section above.
                    }
                    _ => {}
                }
            }
        }
    } else if response.drag_stopped_by(egui::PointerButton::Primary) {
        // Finalize annotation
        if let Some(annotation) = state.tools.pending_annotation.take() {
            let is_count = matches!(annotation.data, AnnotationData::Count { .. });
            // Check whether this Count uses the global counter BEFORE we move the clone.
            let uses_global_counter = matches!(
                annotation.data,
                AnnotationData::Count {
                    sequence_group_id: None,
                    ..
                }
            );
            let annotation_clone = annotation.clone();
            if let Some(layer) = state.session.get_layer_mut(layer_id) {
                layer.annotations.push(annotation);
                state.session.is_dirty = true;

                // Push undo for annotation creation
                let add_cmd = UndoCommand::AddAnnotation {
                    layer_id,
                    annotation: Box::new(annotation_clone),
                };
                if is_count && uses_global_counter {
                    // Global counter was incremented — compound undo restores it.
                    state.session.undo_stack.push(UndoCommand::Compound(vec![
                        add_cmd,
                        UndoCommand::SetCountCounter {
                            old_value: state.session.count_counter - 1,
                            new_value: state.session.count_counter,
                        },
                    ]));
                } else {
                    state.session.undo_stack.push(add_cmd);
                }
            }
        }
    }
}

/// Draw all visible annotations
fn draw_annotations(ui: &Ui, state: &AppState, rect: Rect) {
    // Collect all annotations from visible layers
    let visible_layers = state.session.visible_layers();

    for layer in visible_layers {
        for annotation in &layer.annotations {
            draw_single_annotation(
                ui,
                annotation,
                state,
                rect,
                (layer.offset_x, layer.offset_y),
                layer.id,
            );
        }
    }
}

fn draw_line_ending(
    painter: &egui::Painter,
    tip: Pos2,
    other: Pos2,
    ending_type: dc_core::tools::LineEndingType,
    stroke_width: f32,
    color: Color32,
    stroke: Stroke,
) {
    if ending_type == dc_core::tools::LineEndingType::None {
        return;
    }

    let dx = tip.x - other.x;
    let dy = tip.y - other.y;
    let len = (dx * dx + dy * dy).sqrt();
    if len < 1.0 {
        return;
    }

    let nx = dx / len;
    let ny = dy / len;
    let px = -ny;
    let py = nx;

    let base_size = (stroke_width * 3.0).max(8.0);

    match ending_type {
        dc_core::tools::LineEndingType::None => {}
        dc_core::tools::LineEndingType::Arrow => {
            let left = Pos2::new(
                tip.x - nx * base_size + px * base_size * 0.4,
                tip.y - ny * base_size + py * base_size * 0.4,
            );
            let right = Pos2::new(
                tip.x - nx * base_size - px * base_size * 0.4,
                tip.y - ny * base_size - py * base_size * 0.4,
            );
            painter.add(egui::Shape::convex_polygon(
                vec![tip, left, right],
                color,
                Stroke::NONE,
            ));
        }
        dc_core::tools::LineEndingType::OpenArrow => {
            let left = Pos2::new(
                tip.x - nx * base_size + px * base_size * 0.4,
                tip.y - ny * base_size + py * base_size * 0.4,
            );
            let right = Pos2::new(
                tip.x - nx * base_size - px * base_size * 0.4,
                tip.y - ny * base_size - py * base_size * 0.4,
            );
            painter.line_segment([left, tip], stroke);
            painter.line_segment([tip, right], stroke);
            painter.line_segment([right, left], stroke); // Base of arrow
            painter.add(egui::Shape::convex_polygon(
                vec![tip, left, right],
                Color32::TRANSPARENT,
                stroke,
            ));
        }
        dc_core::tools::LineEndingType::ClosedArrow => {
            let left = Pos2::new(
                tip.x - nx * base_size + px * base_size * 0.4,
                tip.y - ny * base_size + py * base_size * 0.4,
            );
            let right = Pos2::new(
                tip.x - nx * base_size - px * base_size * 0.4,
                tip.y - ny * base_size - py * base_size * 0.4,
            );
            painter.add(egui::Shape::convex_polygon(
                vec![tip, left, right],
                color,
                stroke,
            ));
        }
        dc_core::tools::LineEndingType::Diamond => {
            let mid = Pos2::new(tip.x - nx * base_size * 0.6, tip.y - ny * base_size * 0.6);
            let back = Pos2::new(tip.x - nx * base_size * 1.2, tip.y - ny * base_size * 1.2);
            let left = Pos2::new(mid.x + px * base_size * 0.4, mid.y + py * base_size * 0.4);
            let right = Pos2::new(mid.x - px * base_size * 0.4, mid.y - py * base_size * 0.4);
            painter.add(egui::Shape::convex_polygon(
                vec![tip, left, back, right],
                Color32::TRANSPARENT,
                stroke,
            ));
        }
        dc_core::tools::LineEndingType::Circle => {
            let center = Pos2::new(tip.x - nx * base_size * 0.4, tip.y - ny * base_size * 0.4);
            painter.circle(center, base_size * 0.4, color, stroke);
        }
        dc_core::tools::LineEndingType::Square => {
            let center = Pos2::new(tip.x - nx * base_size * 0.4, tip.y - ny * base_size * 0.4);
            let left = Pos2::new(
                center.x - nx * base_size * 0.4 + px * base_size * 0.4,
                center.y - ny * base_size * 0.4 + py * base_size * 0.4,
            );
            let right = Pos2::new(
                center.x - nx * base_size * 0.4 - px * base_size * 0.4,
                center.y - ny * base_size * 0.4 - py * base_size * 0.4,
            );
            let left_tip = Pos2::new(
                center.x + nx * base_size * 0.4 + px * base_size * 0.4,
                center.y + ny * base_size * 0.4 + py * base_size * 0.4,
            );
            let right_tip = Pos2::new(
                center.x + nx * base_size * 0.4 - px * base_size * 0.4,
                center.y + ny * base_size * 0.4 - py * base_size * 0.4,
            );
            painter.add(egui::Shape::convex_polygon(
                vec![left_tip, left, right, right_tip],
                color,
                stroke,
            ));
        }
        dc_core::tools::LineEndingType::RiseSymbol => {
            // Circle with upward arrow and cross bar
            let center = Pos2::new(tip.x - nx * base_size * 0.6, tip.y - ny * base_size * 0.6);
            let r = base_size * 0.6;
            painter.circle_stroke(center, r, stroke);

            // "Up" arrow (triangle)
            let top = Pos2::new(center.x + nx * r * 0.6, center.y + ny * r * 0.6);
            let b_left = Pos2::new(
                center.x - nx * r * 0.2 + px * r * 0.4,
                center.y - ny * r * 0.2 + py * r * 0.4,
            );
            let b_right = Pos2::new(
                center.x - nx * r * 0.2 - px * r * 0.4,
                center.y - ny * r * 0.2 - py * r * 0.4,
            );
            painter.add(egui::Shape::convex_polygon(
                vec![top, b_left, b_right],
                color,
                Stroke::NONE,
            ));

            // Cross bar
            let bar_left = Pos2::new(center.x + px * r, center.y + py * r);
            let bar_right = Pos2::new(center.x - px * r, center.y - py * r);
            painter.line_segment([bar_left, bar_right], stroke);
        }
        dc_core::tools::LineEndingType::DropSymbol => {
            // Circle with downward arrow and cross bar
            let center = Pos2::new(tip.x - nx * base_size * 0.6, tip.y - ny * base_size * 0.6);
            let r = base_size * 0.6;
            painter.circle_stroke(center, r, stroke);

            // "Down" arrow (triangle) - opposite direction of Rise
            let top = Pos2::new(center.x - nx * r * 0.6, center.y - ny * r * 0.6);
            let b_left = Pos2::new(
                center.x + nx * r * 0.2 + px * r * 0.4,
                center.y + ny * r * 0.2 + py * r * 0.4,
            );
            let b_right = Pos2::new(
                center.x + nx * r * 0.2 - px * r * 0.4,
                center.y + ny * r * 0.2 - py * r * 0.4,
            );
            painter.add(egui::Shape::convex_polygon(
                vec![top, b_left, b_right],
                color,
                Stroke::NONE,
            ));

            // Cross bar
            let bar_left = Pos2::new(center.x + px * r, center.y + py * r);
            let bar_right = Pos2::new(center.x - px * r, center.y - py * r);
            painter.line_segment([bar_left, bar_right], stroke);
        }
    }
}

/// Helper to render a single annotation
fn draw_single_annotation(
    ui: &Ui,
    annotation: &Annotation,
    state: &AppState,
    rect: Rect,
    offset: (f32, f32),
    layer_id: dc_core::LayerId,
) {
    let painter = ui.painter();
    let viewport = &state.session.viewport;
    let screen_center = (rect.center().x as f64, rect.center().y as f64);

    // Check if this annotation is being flashed from the legend panel
    let flash_alpha = crate::panels::is_annotation_flashing(state, layer_id, &annotation.id);

    // Convert color
    let color = Color32::from_rgba_premultiplied(
        annotation.style.stroke_color.r,
        annotation.style.stroke_color.g,
        annotation.style.stroke_color.b,
        (annotation.style.opacity * 255.0) as u8,
    );

    let stroke_width = (annotation.style.line_width * viewport.zoom as f32).max(1.0);
    let mut stroke = Stroke::new(stroke_width, color);

    // Highlight if selected
    if let Some((_, selected_id)) = &state.session.selected_annotation {
        if &annotation.id == selected_id {
            stroke = Stroke::new(stroke_width + 2.0, color.to_opaque());
        }
    }

    // Flash highlight: thicken stroke and use bright color
    if let Some(alpha) = flash_alpha {
        let flash_color = Color32::from_rgba_unmultiplied(255, 255, 100, (alpha * 200.0) as u8);
        stroke = Stroke::new(stroke_width + 4.0, flash_color);
    }

    // Check if this annotation is selected
    let is_selected = state
        .session
        .selected_annotation
        .as_ref()
        .map_or(false, |(_, sid)| sid == &annotation.id);

    // Check if the tool is an Arrow type (for arrowhead rendering)
    let _is_arrow = state
        .tools
        .active_tool
        .as_ref()
        .map_or(false, |t| t.tool_type == ToolType::Arrow);
    // Also check if annotation was created with arrow style
    // We'll draw arrowheads on Lines if the active tool is Arrow or annotation subject is Arrow
    // For simplicity: always draw arrowheads on Lines for now (can be toggled later)

    match &annotation.data {
        AnnotationData::Path(points) => {
            if points.len() < 2 {
                return;
            }

            let screen_points: Vec<Pos2> = points
                .iter()
                .map(|p| {
                    let (sx, sy) = viewport.image_to_screen(p.x as f64, p.y as f64, screen_center);
                    Pos2::new(sx as f32, sy as f32)
                })
                .collect();

            // Draw bounding box if selected
            if is_selected {
                let bounds = Rect::from_points(&screen_points);
                painter.rect_stroke(bounds.expand(5.0), 0.0, Stroke::new(1.0_f32, Color32::BLUE));
            }

            painter.add(egui::Shape::line(screen_points, stroke));
        }
        AnnotationData::Cloud(points) => {
            if points.len() < 2 {
                return;
            }

            let screen_points: Vec<Pos2> = points
                .iter()
                .map(|p| {
                    let (sx, sy) = viewport.image_to_screen(p.x as f64, p.y as f64, screen_center);
                    Pos2::new(sx as f32, sy as f32)
                })
                .collect();

            // Draw cloud arcs between consecutive points
            // Each segment gets a small arc (bump) to create the cloud effect
            for pair in screen_points.windows(2) {
                let a = pair[0];
                let b = pair[1];
                let mid = Pos2::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
                let dx = b.x - a.x;
                let dy = b.y - a.y;
                let seg_len = (dx * dx + dy * dy).sqrt();
                // Bump perpendicular to the segment
                let bump = seg_len * 0.25;
                // Normal direction (outward)
                let nx = dy / seg_len;
                let ny = -dx / seg_len;
                let ctrl = Pos2::new(mid.x + nx * bump, mid.y + ny * bump);
                // Approximate bezier with line segments
                let steps = 8;
                let mut arc_points = Vec::with_capacity(steps + 1);
                for s in 0..=steps {
                    let t = s as f32 / steps as f32;
                    let inv = 1.0 - t;
                    let px = inv * inv * a.x + 2.0 * inv * t * ctrl.x + t * t * b.x;
                    let py = inv * inv * a.y + 2.0 * inv * t * ctrl.y + t * t * b.y;
                    arc_points.push(Pos2::new(px, py));
                }
                painter.add(egui::Shape::line(arc_points, stroke));
            }
            // Close the cloud if we have 3+ points
            if screen_points.len() >= 3 {
                let a = *screen_points.last().unwrap();
                let b = screen_points[0];
                let mid = Pos2::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
                let dx = b.x - a.x;
                let dy = b.y - a.y;
                let seg_len = (dx * dx + dy * dy).sqrt().max(0.001);
                let bump = seg_len * 0.25;
                let nx = dy / seg_len;
                let ny = -dx / seg_len;
                let ctrl = Pos2::new(mid.x + nx * bump, mid.y + ny * bump);
                let steps = 8;
                let mut arc_points = Vec::with_capacity(steps + 1);
                for s in 0..=steps {
                    let t = s as f32 / steps as f32;
                    let inv = 1.0 - t;
                    let px = inv * inv * a.x + 2.0 * inv * t * ctrl.x + t * t * b.x;
                    let py = inv * inv * a.y + 2.0 * inv * t * ctrl.y + t * t * b.y;
                    arc_points.push(Pos2::new(px, py));
                }
                painter.add(egui::Shape::line(arc_points, stroke));
            }

            // Selection bounding box
            if is_selected {
                let bounds = Rect::from_points(&screen_points);
                painter.rect_stroke(bounds.expand(5.0), 0.0, Stroke::new(1.0_f32, Color32::BLUE));
            }
        }
        AnnotationData::Rectangle { start, end } => {
            let (sx1, sy1) = viewport.image_to_screen(
                (start.x + offset.0) as f64,
                (start.y + offset.1) as f64,
                screen_center,
            );
            let (sx2, sy2) = viewport.image_to_screen(
                (end.x + offset.0) as f64,
                (end.y + offset.1) as f64,
                screen_center,
            );

            let rect_shape = Rect::from_min_max(
                Pos2::new(sx1 as f32, sy1 as f32),
                Pos2::new(sx2 as f32, sy2 as f32),
            );

            painter.rect_stroke(rect_shape, 0.0, stroke);

            // Draw selection handles
            if is_selected {
                painter.rect_stroke(
                    rect_shape.expand(5.0),
                    0.0,
                    Stroke::new(1.0_f32, Color32::BLUE),
                );
                let p1 = Pos2::new(sx1 as f32, sy1 as f32);
                let p2 = Pos2::new(sx2 as f32, sy2 as f32);
                painter.circle_filled(p1, 4.0, Color32::BLUE);
                painter.circle_filled(p2, 4.0, Color32::BLUE);
            }
        }
        AnnotationData::Ellipse { start, end } => {
            let (sx1, sy1) = viewport.image_to_screen(
                (start.x + offset.0) as f64,
                (start.y + offset.1) as f64,
                screen_center,
            );
            let (sx2, sy2) = viewport.image_to_screen(
                (end.x + offset.0) as f64,
                (end.y + offset.1) as f64,
                screen_center,
            );

            let center = Pos2::new(
                (sx1 as f32 + sx2 as f32) / 2.0,
                (sy1 as f32 + sy2 as f32) / 2.0,
            );
            let rx = ((sx2 - sx1) as f32 / 2.0).abs();
            let ry = ((sy2 - sy1) as f32 / 2.0).abs();

            // Approximate ellipse with polyline
            let segments = 48;
            let ellipse_points: Vec<Pos2> = (0..=segments)
                .map(|i| {
                    let theta = 2.0 * std::f32::consts::PI * i as f32 / segments as f32;
                    Pos2::new(center.x + rx * theta.cos(), center.y + ry * theta.sin())
                })
                .collect();

            painter.add(egui::Shape::line(ellipse_points, stroke));

            // Draw selection handles (bounding box corners)
            if is_selected {
                let bounding = Rect::from_min_max(
                    Pos2::new(sx1 as f32, sy1 as f32),
                    Pos2::new(sx2 as f32, sy2 as f32),
                );
                painter.rect_stroke(
                    bounding.expand(5.0),
                    0.0,
                    Stroke::new(1.0_f32, Color32::BLUE),
                );
                painter.circle_filled(Pos2::new(sx1 as f32, sy1 as f32), 4.0, Color32::BLUE);
                painter.circle_filled(Pos2::new(sx2 as f32, sy2 as f32), 4.0, Color32::BLUE);
            }
        }
        AnnotationData::Line { start, end } => {
            let (sx1, sy1) = viewport.image_to_screen(
                (start.x + offset.0) as f64,
                (start.y + offset.1) as f64,
                screen_center,
            );
            let (sx2, sy2) = viewport.image_to_screen(
                (end.x + offset.0) as f64,
                (end.y + offset.1) as f64,
                screen_center,
            );

            let p1 = Pos2::new(sx1 as f32, sy1 as f32);
            let p2 = Pos2::new(sx2 as f32, sy2 as f32);

            painter.line_segment([p1, p2], stroke);

            // Draw line endings
            draw_line_ending(
                painter,
                p1,
                p2,
                annotation.style.line_ending_start,
                stroke_width,
                color,
                stroke,
            );
            draw_line_ending(
                painter,
                p2,
                p1,
                annotation.style.line_ending_end,
                stroke_width,
                color,
                stroke,
            );

            // Draw selection handles
            if is_selected {
                painter.circle_filled(p1, 5.0, Color32::BLUE);
                painter.circle_filled(p2, 5.0, Color32::BLUE);
            }
        }
        AnnotationData::Text {
            pos,
            content,
            width,
        } => {
            let (sx, sy) = viewport.image_to_screen(
                (pos.x + offset.0) as f64,
                (pos.y + offset.1) as f64,
                screen_center,
            );
            let screen_pos = Pos2::new(sx as f32, sy as f32);

            let font_size = annotation.style.font_size * viewport.zoom as f32;
            let font_id = egui::FontId::proportional(font_size.max(8.0));

            let display_text = if content.is_empty() {
                "Text"
            } else {
                content.as_str()
            };

            // Background box
            let text_w = width.unwrap_or(100.0) * viewport.zoom as f32;
            let text_h = font_size + 8.0;
            let bg_rect = Rect::from_min_size(screen_pos, Vec2::new(text_w, text_h));
            painter.rect_filled(bg_rect, 2.0, Color32::from_black_alpha(100));
            painter.rect_stroke(bg_rect, 2.0, Stroke::new(1.0_f32, color));

            painter.text(
                screen_pos + Vec2::new(4.0, 4.0),
                egui::Align2::LEFT_TOP,
                display_text,
                font_id,
                color,
            );

            // Selection indicator
            if is_selected {
                painter.rect_stroke(
                    bg_rect.expand(3.0),
                    2.0,
                    Stroke::new(1.5_f32, Color32::BLUE),
                );
                painter.circle_filled(screen_pos, 4.0, Color32::BLUE);
            }
        }
        AnnotationData::Measurement {
            points,
            is_area,
            label,
            ..
        } => {
            if points.len() < 2 {
                return;
            }

            let screen_points: Vec<Pos2> = points
                .iter()
                .map(|p| {
                    let (sx, sy) = viewport.image_to_screen(
                        (p.x + offset.0) as f64,
                        (p.y + offset.1) as f64,
                        screen_center,
                    );
                    Pos2::new(sx as f32, sy as f32)
                })
                .collect();

            // Use a distinct green measurement stroke
            let meas_color = Color32::from_rgba_premultiplied(
                annotation.style.stroke_color.r,
                annotation.style.stroke_color.g,
                annotation.style.stroke_color.b,
                (annotation.style.opacity * 255.0) as u8,
            );
            let meas_stroke = Stroke::new(stroke_width, meas_color);

            // Draw the polyline
            painter.add(egui::Shape::line(screen_points.clone(), meas_stroke));

            // If area, draw closing segment
            if *is_area && screen_points.len() > 2 {
                painter.line_segment(
                    [*screen_points.last().unwrap(), screen_points[0]],
                    meas_stroke,
                );

                // Translucent fill
                let fill_color = Color32::from_rgba_premultiplied(
                    annotation.style.stroke_color.r,
                    annotation.style.stroke_color.g,
                    annotation.style.stroke_color.b,
                    40,
                );
                let mut polygon = screen_points.clone();
                polygon.push(screen_points[0]);
                painter.add(egui::Shape::convex_polygon(
                    polygon,
                    fill_color,
                    Stroke::NONE,
                ));
            }

            // Draw endpoints
            for sp in &screen_points {
                painter.circle_filled(*sp, 3.0, meas_color);
            }

            // Draw measurement label at midpoint
            if !label.is_empty() {
                let mid = if screen_points.len() == 2 {
                    Pos2::new(
                        (screen_points[0].x + screen_points[1].x) / 2.0,
                        (screen_points[0].y + screen_points[1].y) / 2.0,
                    )
                } else {
                    // Centroid
                    let cx: f32 =
                        screen_points.iter().map(|p| p.x).sum::<f32>() / screen_points.len() as f32;
                    let cy: f32 =
                        screen_points.iter().map(|p| p.y).sum::<f32>() / screen_points.len() as f32;
                    Pos2::new(cx, cy)
                };

                let label_font = egui::FontId::proportional(12.0);
                // Background pill
                let galley =
                    painter.layout_no_wrap(label.clone(), label_font.clone(), Color32::WHITE);
                let label_rect = egui::Rect::from_min_size(
                    mid - Vec2::new(galley.size().x / 2.0 + 4.0, galley.size().y / 2.0 + 2.0),
                    galley.size() + Vec2::new(8.0, 4.0),
                );
                painter.rect_filled(label_rect, 4.0, Color32::from_black_alpha(180));
                painter.text(
                    mid,
                    egui::Align2::CENTER_CENTER,
                    label,
                    label_font,
                    Color32::WHITE,
                );
            }

            // Selection indicator
            if is_selected {
                let bounds = Rect::from_points(&screen_points);
                painter.rect_stroke(bounds.expand(5.0), 0.0, Stroke::new(1.0_f32, Color32::BLUE));
            }
        }
        AnnotationData::Count {
            pos,
            number,
            sequence_group_id,
        } => {
            let (sx, sy) = viewport.image_to_screen(
                (pos.x + offset.0) as f64,
                (pos.y + offset.1) as f64,
                screen_center,
            );
            let screen_pos = Pos2::new(sx as f32, sy as f32);

            // Draw a filled circle with the count number
            let radius = 14.0;
            let count_color = Color32::from_rgba_premultiplied(
                annotation.style.stroke_color.r,
                annotation.style.stroke_color.g,
                annotation.style.stroke_color.b,
                (annotation.style.opacity * 255.0) as u8,
            );
            painter.circle_filled(screen_pos, radius, count_color);
            painter.circle_stroke(screen_pos, radius, Stroke::new(2.0_f32, Color32::WHITE));

            painter.text(
                screen_pos,
                egui::Align2::CENTER_CENTER,
                format!("{}", number),
                egui::FontId::proportional(12.0),
                Color32::WHITE,
            );

            // Show sequence group label below the marker if set
            if let Some(ref group_id) = sequence_group_id {
                let group_pos = Pos2::new(screen_pos.x, screen_pos.y + radius + 10.0);
                let group_font = egui::FontId::proportional(9.0);
                let galley =
                    painter.layout_no_wrap(group_id.clone(), group_font.clone(), Color32::WHITE);
                let bg_rect =
                    Rect::from_center_size(group_pos, galley.size() + Vec2::new(6.0, 2.0));
                painter.rect_filled(bg_rect, 2.0, Color32::from_black_alpha(160));
                painter.text(
                    group_pos,
                    egui::Align2::CENTER_CENTER,
                    group_id,
                    group_font,
                    Color32::WHITE,
                );
            }

            // Selection indicator
            if is_selected {
                painter.circle_stroke(
                    screen_pos,
                    radius + 4.0,
                    Stroke::new(1.5_f32, Color32::BLUE),
                );
            }
        }
        AnnotationData::Viewport {
            start,
            end,
            scale,
            label,
        } => {
            let (sx1, sy1) = viewport.image_to_screen(
                (start.x + offset.0) as f64,
                (start.y + offset.1) as f64,
                screen_center,
            );
            let (sx2, sy2) = viewport.image_to_screen(
                (end.x + offset.0) as f64,
                (end.y + offset.1) as f64,
                screen_center,
            );

            let min_x = (sx1 as f32).min(sx2 as f32);
            let min_y = (sy1 as f32).min(sy2 as f32);
            let max_x = (sx1 as f32).max(sx2 as f32);
            let max_y = (sy1 as f32).max(sy2 as f32);

            let vp_rect = Rect::from_min_max(Pos2::new(min_x, min_y), Pos2::new(max_x, max_y));

            // Avoid drawing degenerate viewport geometry.
            if vp_rect.width() < 1.0 || vp_rect.height() < 1.0 {
                return;
            }

            // Draw dashed border for the viewport
            let dash_len = 8.0_f32;
            let gap_len = 4.0_f32;
            let vp_color = Color32::from_rgba_premultiplied(
                annotation.style.stroke_color.r,
                annotation.style.stroke_color.g,
                annotation.style.stroke_color.b,
                (annotation.style.opacity * 255.0) as u8,
            );
            let vp_stroke = Stroke::new(stroke_width, vp_color);

            // Draw all 4 dashed edges
            let corners = [
                Pos2::new(min_x, min_y),
                Pos2::new(max_x, min_y),
                Pos2::new(max_x, max_y),
                Pos2::new(min_x, max_y),
            ];
            for i in 0..4 {
                let a = corners[i];
                let b = corners[(i + 1) % 4];
                let dx = b.x - a.x;
                let dy = b.y - a.y;
                let edge_len = (dx * dx + dy * dy).sqrt();
                if edge_len < 1.0 {
                    continue;
                }
                let nx = dx / edge_len;
                let ny = dy / edge_len;
                let mut t = 0.0_f32;
                let mut drawing = true;
                while t < edge_len {
                    let seg = if drawing { dash_len } else { gap_len };
                    let end_t = (t + seg).min(edge_len);
                    if drawing {
                        let p0 = Pos2::new(a.x + nx * t, a.y + ny * t);
                        let p1 = Pos2::new(a.x + nx * end_t, a.y + ny * end_t);
                        painter.line_segment([p0, p1], vp_stroke);
                    }
                    t = end_t;
                    drawing = !drawing;
                }
            }

            // Keep a very light tint for orientation, but avoid hiding content.
            let fill_alpha = if is_selected { 12_u8 } else { 0_u8 };
            let fill_color = Color32::from_rgba_premultiplied(
                annotation.style.stroke_color.r,
                annotation.style.stroke_color.g,
                annotation.style.stroke_color.b,
                fill_alpha,
            );
            painter.rect_filled(vp_rect, 0.0, fill_color);

            // Draw corner brackets so the viewport reads as a framing tool.
            let bracket_len = 14.0_f32;
            let bracket_stroke = Stroke::new(stroke_width + 0.8, vp_color);
            let corners = [
                Pos2::new(min_x, min_y),
                Pos2::new(max_x, min_y),
                Pos2::new(max_x, max_y),
                Pos2::new(min_x, max_y),
            ];
            for (idx, c) in corners.iter().enumerate() {
                let (sx, sy) = match idx {
                    0 => (1.0_f32, 1.0_f32),
                    1 => (-1.0_f32, 1.0_f32),
                    2 => (-1.0_f32, -1.0_f32),
                    _ => (1.0_f32, -1.0_f32),
                };
                painter.line_segment([*c, Pos2::new(c.x + sx * bracket_len, c.y)], bracket_stroke);
                painter.line_segment([*c, Pos2::new(c.x, c.y + sy * bracket_len)], bracket_stroke);
            }

            // Center marker helps when composing a local viewport.
            let center = vp_rect.center();
            let center_cross = 10.0_f32;
            painter.line_segment(
                [
                    Pos2::new(center.x - center_cross, center.y),
                    Pos2::new(center.x + center_cross, center.y),
                ],
                Stroke::new(1.5_f32, vp_color),
            );
            painter.line_segment(
                [
                    Pos2::new(center.x, center.y - center_cross),
                    Pos2::new(center.x, center.y + center_cross),
                ],
                Stroke::new(1.5_f32, vp_color),
            );

            // Scale badge in top-left corner
            let img_w = (end.x - start.x).abs();
            let img_h = (end.y - start.y).abs();
            let badge_text = if label.is_empty() {
                format!("{}x | {:.0} x {:.0}", scale, img_w, img_h)
            } else {
                format!("{} | {}x | {:.0} x {:.0}", label, scale, img_w, img_h)
            };
            let badge_font = egui::FontId::proportional(11.0);
            let galley =
                painter.layout_no_wrap(badge_text.clone(), badge_font.clone(), Color32::WHITE);
            let badge_padding = Vec2::new(6.0, 3.0);
            let badge_pos = Pos2::new(min_x + 4.0, min_y + 4.0);
            let badge_rect = Rect::from_min_size(badge_pos, galley.size() + badge_padding * 2.0);
            painter.rect_filled(
                badge_rect,
                3.0,
                Color32::from_rgba_premultiplied(
                    annotation.style.stroke_color.r,
                    annotation.style.stroke_color.g,
                    annotation.style.stroke_color.b,
                    200,
                ),
            );
            painter.text(
                badge_pos + badge_padding,
                egui::Align2::LEFT_TOP,
                badge_text,
                badge_font,
                Color32::WHITE,
            );

            // Selection handles
            if is_selected {
                painter.rect_stroke(
                    vp_rect.expand(4.0),
                    0.0,
                    Stroke::new(1.5_f32, Color32::BLUE),
                );
                painter.circle_filled(Pos2::new(sx1 as f32, sy1 as f32), 4.0, Color32::BLUE);
                painter.circle_filled(Pos2::new(sx2 as f32, sy2 as f32), 4.0, Color32::BLUE);
            }
        }
        AnnotationData::DimensionChain {
            points,
            segment_labels,
            total_value: _,
            total_label,
            ..
        } => {
            if points.len() < 2 {
                return;
            }

            let screen_points: Vec<Pos2> = points
                .iter()
                .map(|p| {
                    let (sx, sy) = viewport.image_to_screen(
                        (p.x + offset.0) as f64,
                        (p.y + offset.1) as f64,
                        screen_center,
                    );
                    Pos2::new(sx as f32, sy as f32)
                })
                .collect();

            let dc_color = Color32::from_rgba_premultiplied(
                annotation.style.stroke_color.r,
                annotation.style.stroke_color.g,
                annotation.style.stroke_color.b,
                (annotation.style.opacity * 255.0) as u8,
            );
            let dc_stroke = Stroke::new(stroke_width, dc_color);

            // Draw connected chain segments
            painter.add(egui::Shape::line(screen_points.clone(), dc_stroke));

            // Draw tick marks at each node (perpendicular to chain direction)
            let tick_half = 8.0_f32;
            for (i, sp) in screen_points.iter().enumerate() {
                // Compute direction at this node
                let (dx, dy) = if i == 0 && screen_points.len() > 1 {
                    let next = screen_points[1];
                    (next.x - sp.x, next.y - sp.y)
                } else if i == screen_points.len() - 1 && screen_points.len() > 1 {
                    let prev = screen_points[i - 1];
                    (sp.x - prev.x, sp.y - prev.y)
                } else if screen_points.len() > 1 {
                    let prev = screen_points[i - 1];
                    let next = screen_points[i + 1];
                    (next.x - prev.x, next.y - prev.y)
                } else {
                    (1.0, 0.0)
                };
                let len = (dx * dx + dy * dy).sqrt().max(0.001);
                // Perpendicular direction
                let px = -dy / len;
                let py = dx / len;
                let tick_a = Pos2::new(sp.x + px * tick_half, sp.y + py * tick_half);
                let tick_b = Pos2::new(sp.x - px * tick_half, sp.y - py * tick_half);
                painter.line_segment([tick_a, tick_b], dc_stroke);

                // Draw node dot
                painter.circle_filled(*sp, 3.0, dc_color);
            }

            // Draw segment labels centered above/beside each segment
            let label_font = egui::FontId::proportional(11.0);
            for (i, pair) in screen_points.windows(2).enumerate() {
                let a = pair[0];
                let b = pair[1];
                let mid = Pos2::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);

                // Offset label perpendicular to segment
                let dx = b.x - a.x;
                let dy = b.y - a.y;
                let seg_len = (dx * dx + dy * dy).sqrt().max(0.001);
                let px = -dy / seg_len;
                let py = dx / seg_len;
                let label_offset = 14.0_f32;
                let label_pos = Pos2::new(mid.x + px * label_offset, mid.y + py * label_offset);

                let seg_label = segment_labels.get(i).cloned().unwrap_or_default();
                if !seg_label.is_empty() {
                    let galley = painter.layout_no_wrap(
                        seg_label.clone(),
                        label_font.clone(),
                        Color32::WHITE,
                    );
                    let bg_rect =
                        Rect::from_center_size(label_pos, galley.size() + Vec2::new(8.0, 4.0));
                    painter.rect_filled(bg_rect, 3.0, Color32::from_black_alpha(180));
                    painter.text(
                        label_pos,
                        egui::Align2::CENTER_CENTER,
                        seg_label,
                        label_font.clone(),
                        Color32::WHITE,
                    );
                }
            }

            // Draw total label at the chain midpoint
            if !total_label.is_empty() && screen_points.len() >= 2 {
                let mid_idx = screen_points.len() / 2;
                let total_pos = if screen_points.len() % 2 == 0 && mid_idx > 0 {
                    let a = screen_points[mid_idx - 1];
                    let b = screen_points[mid_idx];
                    Pos2::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0 - 28.0)
                } else {
                    Pos2::new(screen_points[mid_idx].x, screen_points[mid_idx].y - 28.0)
                };

                let total_font = egui::FontId::proportional(13.0);
                let total_text = format!("Σ {}", total_label);
                let galley =
                    painter.layout_no_wrap(total_text.clone(), total_font.clone(), Color32::WHITE);
                let bg_rect =
                    Rect::from_center_size(total_pos, galley.size() + Vec2::new(12.0, 6.0));
                painter.rect_filled(
                    bg_rect,
                    4.0,
                    Color32::from_rgba_premultiplied(
                        annotation.style.stroke_color.r,
                        annotation.style.stroke_color.g,
                        annotation.style.stroke_color.b,
                        220,
                    ),
                );
                painter.text(
                    total_pos,
                    egui::Align2::CENTER_CENTER,
                    total_text,
                    total_font,
                    Color32::WHITE,
                );
            }

            // Selection indicator
            if is_selected {
                let bounds = Rect::from_points(&screen_points);
                painter.rect_stroke(bounds.expand(5.0), 0.0, Stroke::new(1.0_f32, Color32::BLUE));
                for sp in &screen_points {
                    painter.circle_filled(*sp, 4.0, Color32::BLUE);
                }
            }
        }
    }
}
