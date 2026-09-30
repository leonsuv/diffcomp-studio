// =============================================================================
// dc_app/panels/minimap - Minimap / Overview Panel
// =============================================================================
// Shows a bird's-eye view of the entire document canvas with a viewport
// indicator rectangle. Clicking or dragging on the minimap navigates the
// main viewport to that location.
// =============================================================================

use crate::app::TextureCache;
use crate::state::AppState;
use egui::{Color32, Pos2, Rect, Sense, Stroke, Ui, Vec2};

/// Render the minimap panel.
pub fn minimap_panel(ui: &mut Ui, state: &mut AppState, textures: &mut TextureCache) {
    let available = ui.available_rect_before_wrap();

    // If there are no layers, just show a placeholder
    if state.session.layers.is_empty() {
        ui.centered_and_justified(|ui| {
            ui.label(crate::i18n::tr("minimap.no_doc"));
        });
        return;
    }

    // Determine the full image bounds from the reference layer (or first layer)
    let (img_w, img_h) = state
        .session
        .reference_layer()
        .or_else(|| state.session.layers.first())
        .map(|l| l.active_image().dimensions())
        .unwrap_or((1, 1));

    if img_w == 0 || img_h == 0 {
        return;
    }

    // ── Compute fitting transform ────────────────────────────────────────────
    // Map the full image rect into the available minimap area with padding.
    let padding = 8.0;
    let draw_area = available.shrink(padding);

    let scale_x = draw_area.width() / img_w as f32;
    let scale_y = draw_area.height() / img_h as f32;
    let scale = scale_x.min(scale_y);

    let map_w = img_w as f32 * scale;
    let map_h = img_h as f32 * scale;

    // Center the minimap image in the draw area
    let map_origin = Pos2::new(
        draw_area.min.x + (draw_area.width() - map_w) / 2.0,
        draw_area.min.y + (draw_area.height() - map_h) / 2.0,
    );
    let map_rect = Rect::from_min_size(map_origin, Vec2::new(map_w, map_h));

    // ── Allocate interaction ─────────────────────────────────────────────────
    let response = ui.allocate_rect(available, Sense::click_and_drag());

    let painter = ui.painter();

    // Background
    painter.rect_filled(available, 0.0, Color32::from_gray(30));
    painter.rect_stroke(available, 0.0, Stroke::new(1.0_f32, Color32::from_gray(60)));

    // ── Draw the minimap image ───────────────────────────────────────────────
    // Try to render all visible layers scaled down
    let visible_layers = state.session.visible_layers();

    for layer in visible_layers.iter().rev() {
        let image = layer.active_image();
        let (lw, lh) = image.dimensions();
        if lw == 0 || lh == 0 {
            continue;
        }

        if let Some(tiled_texture) = textures.get_or_create(ui.ctx(), layer.id.0 as u64, image) {
            let tint =
                Color32::from_rgba_unmultiplied(255, 255, 255, (layer.opacity * 255.0) as u8);

            for tile in tiled_texture.tiles() {
                // Map tile image coordinates to minimap screen coordinates
                let tile_left = map_origin.x + tile.x as f32 * scale;
                let tile_top = map_origin.y + tile.y as f32 * scale;
                let tile_right = map_origin.x + (tile.x + tile.width) as f32 * scale;
                let tile_bottom = map_origin.y + (tile.y + tile.height) as f32 * scale;

                let tile_rect = Rect::from_min_max(
                    Pos2::new(tile_left, tile_top),
                    Pos2::new(tile_right, tile_bottom),
                );

                painter.image(
                    tile.texture().id(),
                    tile_rect,
                    Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(1.0, 1.0)),
                    tint,
                );
            }
        } else {
            // Fallback: colored placeholder
            let fill = Color32::from_rgba_premultiplied(
                layer.blend_color.r,
                layer.blend_color.g,
                layer.blend_color.b,
                (layer.opacity * 100.0) as u8,
            );
            painter.rect_filled(map_rect, 0.0, fill);
        }
    }

    // Border around the minimap image
    painter.rect_stroke(map_rect, 0.0, Stroke::new(1.0_f32, Color32::from_gray(80)));

    // ── Draw annotation markers ──────────────────────────────────────────────
    for layer in &state.session.layers {
        for annot in &layer.annotations {
            let (min_x, min_y, max_x, max_y) = annot.bounds;
            let a_rect = Rect::from_min_max(
                Pos2::new(map_origin.x + min_x * scale, map_origin.y + min_y * scale),
                Pos2::new(map_origin.x + max_x * scale, map_origin.y + max_y * scale),
            );

            let sc = &annot.style.stroke_color;
            let annot_color = Color32::from_rgba_premultiplied(sc.r, sc.g, sc.b, 160);

            // Draw a tiny rectangle for each annotation
            let dot_size = a_rect.size().max(Vec2::splat(3.0));
            let dot_rect = Rect::from_center_size(a_rect.center(), dot_size);
            painter.rect_filled(dot_rect, 1.0, annot_color.linear_multiply(0.4));
            painter.rect_stroke(dot_rect, 1.0, Stroke::new(0.5_f32, annot_color));
        }
    }

    // ── Draw viewport indicator ──────────────────────────────────────────────
    // Figure out what rectangle of the image is currently visible in the main viewport.
    // The viewport center is at (center_x, center_y) in image coords.
    // The visible area depends on the main viewport screen size and zoom.
    // We approximate the main viewport size from the last known main content area.
    // Since we don't have direct access, use a reasonable estimate.
    // Copy viewport values (avoids mutable borrow conflict with click handler below)
    let vp_center_x = state.session.viewport.center_x as f32;
    let vp_center_y = state.session.viewport.center_y as f32;
    let vp_zoom = state.session.viewport.zoom as f32;

    // Estimate the main viewport screen dimensions (we'll use available width of window)
    // A more precise approach would store these, but this is a good approximation.
    let main_screen_w = ui.ctx().screen_rect().width() * 0.6; // ~60% of window is viewport
    let main_screen_h = ui.ctx().screen_rect().height() * 0.7; // ~70% of window height

    // Visible image area
    let half_vis_w = (main_screen_w / 2.0) / vp_zoom;
    let half_vis_h = (main_screen_h / 2.0) / vp_zoom;

    let vis_min_x = vp_center_x - half_vis_w;
    let vis_min_y = vp_center_y - half_vis_h;
    let vis_max_x = vp_center_x + half_vis_w;
    let vis_max_y = vp_center_y + half_vis_h;

    // Map to minimap screen coords
    let vp_screen_min = Pos2::new(
        map_origin.x + vis_min_x * scale,
        map_origin.y + vis_min_y * scale,
    );
    let vp_screen_max = Pos2::new(
        map_origin.x + vis_max_x * scale,
        map_origin.y + vis_max_y * scale,
    );

    let vp_rect = Rect::from_min_max(vp_screen_min, vp_screen_max);

    // Clamp to map rect for visual clarity
    let clamped_vp = vp_rect.intersect(map_rect);

    // Dimming outside the viewport
    let dim_color = Color32::from_black_alpha(100);

    // Top strip
    if clamped_vp.min.y > map_rect.min.y {
        painter.rect_filled(
            Rect::from_min_max(map_rect.min, Pos2::new(map_rect.max.x, clamped_vp.min.y)),
            0.0,
            dim_color,
        );
    }
    // Bottom strip
    if clamped_vp.max.y < map_rect.max.y {
        painter.rect_filled(
            Rect::from_min_max(Pos2::new(map_rect.min.x, clamped_vp.max.y), map_rect.max),
            0.0,
            dim_color,
        );
    }
    // Left strip (between top and bottom)
    if clamped_vp.min.x > map_rect.min.x {
        let top = clamped_vp.min.y.max(map_rect.min.y);
        let bot = clamped_vp.max.y.min(map_rect.max.y);
        painter.rect_filled(
            Rect::from_min_max(
                Pos2::new(map_rect.min.x, top),
                Pos2::new(clamped_vp.min.x, bot),
            ),
            0.0,
            dim_color,
        );
    }
    // Right strip (between top and bottom)
    if clamped_vp.max.x < map_rect.max.x {
        let top = clamped_vp.min.y.max(map_rect.min.y);
        let bot = clamped_vp.max.y.min(map_rect.max.y);
        painter.rect_filled(
            Rect::from_min_max(
                Pos2::new(clamped_vp.max.x, top),
                Pos2::new(map_rect.max.x, bot),
            ),
            0.0,
            dim_color,
        );
    }

    // Viewport rectangle outline
    painter.rect_stroke(
        clamped_vp,
        0.0,
        Stroke::new(2.0_f32, Color32::from_rgb(0, 150, 255)),
    );

    // Crosshair at viewport center
    let center_screen = Pos2::new(
        map_origin.x + vp_center_x * scale,
        map_origin.y + vp_center_y * scale,
    );
    if map_rect.contains(center_screen) {
        let cross_size = 4.0;
        painter.line_segment(
            [
                center_screen - Vec2::new(cross_size, 0.0),
                center_screen + Vec2::new(cross_size, 0.0),
            ],
            Stroke::new(1.0_f32, Color32::from_rgb(0, 150, 255)),
        );
        painter.line_segment(
            [
                center_screen - Vec2::new(0.0, cross_size),
                center_screen + Vec2::new(0.0, cross_size),
            ],
            Stroke::new(1.0_f32, Color32::from_rgb(0, 150, 255)),
        );
    }

    // ── Handle click/drag to navigate ────────────────────────────────────────
    if response.clicked() || response.dragged() {
        if let Some(pos) = response.interact_pointer_pos() {
            // Only navigate if click is within the map area
            if map_rect.contains(pos) {
                // Convert minimap screen position to image coordinates
                let img_x = (pos.x - map_origin.x) / scale;
                let img_y = (pos.y - map_origin.y) / scale;

                state.session.viewport.center_x = img_x as f64;
                state.session.viewport.center_y = img_y as f64;
            }
        }
    }

    // ── Info label ────────────────────────────────────────────────────────────
    let info_text = format!("{}×{} · {:.0}%", img_w, img_h, vp_zoom * 100.0);
    painter.text(
        Pos2::new(available.min.x + 4.0, available.max.y - 14.0),
        egui::Align2::LEFT_TOP,
        info_text,
        egui::FontId::proportional(9.0),
        Color32::from_gray(140),
    );
}
