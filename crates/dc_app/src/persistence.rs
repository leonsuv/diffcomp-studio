// =============================================================================
// dc_app/persistence - Session Saving and Loading
// =============================================================================
// Handles serialization and deserialization of the application session.
// =============================================================================

use crate::state::SessionState;
use crate::undo::{LayerSaveData as UndoLayerSaveData, UndoStackSave};
use base64::Engine;
use dc_core::{
    Annotation, CoreError, CoreResult, Layer, LayerColor, LayerId, RasterBuffer, Viewport,
};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use tracing::{error, info};

/// Data structure for saving a session to disk.
#[derive(Serialize, Deserialize)]
struct SessionSaveData {
    layers: Vec<LayerSaveData>,
    #[serde(default)]
    diff_config: dc_core::DiffConfig,
    #[serde(default)]
    calibration: dc_core::Calibration,
    #[serde(default = "default_count")]
    count_counter: u32,
    viewport: Viewport,
    next_layer_id: u32,
    selected_layer: Option<LayerId>,
    /// Undo/redo stack (optional for backward compatibility with older files).
    #[serde(default)]
    undo_stack: Option<UndoStackSave>,
    /// Known sequence group names for punch-list tool.
    #[serde(default)]
    sequence_groups: Vec<String>,
    /// Custom columns
    #[serde(default)]
    custom_columns: Vec<crate::state::CustomColumn>,
    /// Custom workflow states
    #[serde(default = "default_workflow_states")]
    workflow_states: Vec<crate::state::WorkflowState>,
    /// Maps annotation IDs to workflow state IDs
    #[serde(default)]
    annotation_statuses: std::collections::HashMap<String, String>,
}

fn default_count() -> u32 {
    1
}

fn default_workflow_states() -> Vec<crate::state::WorkflowState> {
    crate::state::WorkflowState::default_states()
}

/// Data structure for saving a layer.
/// Note: We do NOT save the pixel data. We save the path and metadata.
#[derive(Serialize, Deserialize)]
struct LayerSaveData {
    id: LayerId,
    name: String,
    source_path: PathBuf,
    homography_matrix: Option<[[f64; 3]; 3]>,
    visible: bool,
    opacity: f32,
    blend_color: LayerColor,
    is_reference: bool,
    annotations: Vec<Annotation>,
    #[serde(default)]
    offset_x: f32,
    #[serde(default)]
    offset_y: f32,
    #[serde(default = "default_dpi")]
    dpi: u32,
    #[serde(default)]
    page_index: Option<usize>,
    #[serde(default)]
    aligned_size: Option<(u32, u32)>,
    #[serde(default)]
    embedded_png: Option<String>,
    #[serde(default)]
    aligned_png: Option<String>,
}

/// Save the current session to a file.
pub fn save_session(session: &SessionState, path: &Path) -> CoreResult<()> {
    let save_data = SessionSaveData {
        layers: session
            .layers
            .iter()
            .map(|layer| {
                let mut data = LayerSaveData::from(layer);
                // Files imported from memory must survive reopening the session.
                if !layer.source_path.is_file() {
                    let mut png = std::io::Cursor::new(Vec::new());
                    layer
                        .original
                        .image
                        .write_to(&mut png, image::ImageFormat::Png)
                        .map_err(|e| CoreError::InternalError {
                            message: e.to_string(),
                        })?;
                    data.embedded_png =
                        Some(base64::engine::general_purpose::STANDARD.encode(png.into_inner()));
                }
                if let Some(aligned) = &layer.aligned {
                    let mut png = std::io::Cursor::new(Vec::new());
                    aligned
                        .image
                        .write_to(&mut png, image::ImageFormat::Png)
                        .map_err(|e| CoreError::InternalError {
                            message: e.to_string(),
                        })?;
                    data.aligned_png =
                        Some(base64::engine::general_purpose::STANDARD.encode(png.into_inner()));
                }
                Ok(data)
            })
            .collect::<CoreResult<Vec<_>>>()?,
        diff_config: session.diff_config.clone(),
        calibration: session.calibration.clone(),
        count_counter: session.count_counter,
        viewport: session.viewport,
        next_layer_id: session.next_layer_id,
        selected_layer: session.selected_layer,
        undo_stack: Some(session.undo_stack.to_save()),
        sequence_groups: session.sequence_groups.clone(),
        custom_columns: session.custom_columns.clone(),
        workflow_states: session.workflow_states.clone(),
        annotation_statuses: session.annotation_statuses.clone(),
    };

    // Serialize into a sibling file, flush it, then replace the destination atomically.
    let temporary = path.with_extension(format!("dcs-{}.tmp", uuid::Uuid::new_v4()));
    let write_result = (|| -> CoreResult<()> {
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|e| CoreError::FileReadError {
                path: temporary.clone(),
                source: e,
            })?;
        let mut writer = BufWriter::new(file);
        serde_json::to_writer(&mut writer, &save_data).map_err(|e| CoreError::InternalError {
            message: format!("Failed to serialize session: {e}"),
        })?;
        writer
            .flush()
            .and_then(|()| writer.get_ref().sync_all())
            .map_err(|e| CoreError::FileReadError {
                path: temporary.clone(),
                source: e,
            })?;
        drop(writer);
        std::fs::rename(&temporary, path).map_err(|e| CoreError::FileReadError {
            path: path.to_path_buf(),
            source: e,
        })?;
        Ok(())
    })();
    if write_result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    write_result?;

    info!("Session saved to {:?}", path);
    Ok(())
}

/// Load a session from a file.
pub fn load_session(path: &Path) -> CoreResult<SessionState> {
    let file = File::open(path).map_err(|e| CoreError::FileReadError {
        path: path.to_path_buf(),
        source: e,
    })?;

    let reader = BufReader::new(file);
    let save_data: SessionSaveData =
        serde_json::from_reader(reader).map_err(|e| CoreError::InternalError {
            message: format!("Failed to deserialize session: {}", e),
        })?;

    // Reconstruct SessionState
    let mut session = SessionState::new();
    session.viewport = save_data.viewport;
    session.next_layer_id = save_data.next_layer_id;
    session.diff_config = save_data.diff_config;
    session.calibration = save_data.calibration;
    session.count_counter = save_data.count_counter;

    // Load layers
    for layer_data in save_data.layers {
        let source_path = if layer_data.source_path.is_absolute() || layer_data.source_path.exists()
        {
            layer_data.source_path.clone()
        } else {
            path.parent()
                .unwrap_or(Path::new("."))
                .join(&layer_data.source_path)
        };
        let image = if let Some(encoded) = &layer_data.embedded_png {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(encoded)
                .map_err(|e| CoreError::InternalError {
                    message: e.to_string(),
                })?;
            let decoded =
                image::load_from_memory(&bytes).map_err(|e| CoreError::ImageDecodeError {
                    reason: e.to_string(),
                })?;
            let mut buffer = RasterBuffer::from_dynamic(decoded, layer_data.dpi);
            buffer.page_index = layer_data.page_index;
            buffer
        } else {
            load_image(&source_path, layer_data.dpi, layer_data.page_index)?
        };

        // Reconstruct layer
        let mut layer = Layer::new(
            layer_data.id,
            layer_data.name,
            source_path,
            image,
            layer_data.is_reference,
        );

        layer.homography_matrix = layer_data.homography_matrix;
        layer.visible = layer_data.visible;
        layer.opacity = layer_data.opacity;
        layer.blend_color = layer_data.blend_color;
        layer.annotations = layer_data.annotations;

        layer.offset_x = layer_data.offset_x;
        layer.offset_y = layer_data.offset_y;
        if let Some(encoded) = &layer_data.aligned_png {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(encoded)
                .map_err(|e| CoreError::InternalError {
                    message: e.to_string(),
                })?;
            let decoded =
                image::load_from_memory(&bytes).map_err(|e| CoreError::ImageDecodeError {
                    reason: e.to_string(),
                })?;
            layer.aligned = Some(std::sync::Arc::new(RasterBuffer::from_dynamic(
                decoded,
                layer.original.dpi,
            )));
        } else if let (Some(matrix), Some(size)) =
            (layer.homography_matrix, layer_data.aligned_size)
        {
            layer.aligned = Some(std::sync::Arc::new(
                dc_core::HomographySolver::new().warp_image(
                    &layer.original,
                    &dc_core::HomographyMatrix { elements: matrix },
                    size,
                )?,
            ));
        } else if let Some(size) = layer_data.aligned_size {
            let mut padded = image::RgbaImage::from_pixel(size.0, size.1, image::Rgba([255; 4]));
            image::imageops::replace(&mut padded, &layer.original.image, 0, 0);
            layer.aligned = Some(std::sync::Arc::new(RasterBuffer::new(
                padded,
                layer.original.dpi,
            )));
        }

        session.layers.push(layer);
    }

    session.next_layer_id = session.next_layer_id.max(
        session
            .layers
            .iter()
            .map(|l| l.id.0.saturating_add(1))
            .max()
            .unwrap_or(0),
    );
    session.selected_layer = save_data
        .selected_layer
        .filter(|id| session.get_layer(*id).is_some());
    session.sequence_groups = save_data.sequence_groups;
    session.custom_columns = save_data.custom_columns;
    session.workflow_states = save_data.workflow_states;
    session.annotation_statuses = save_data.annotation_statuses;
    session.session_path = Some(path.to_path_buf());

    // Restore undo stack if present
    if let Some(undo_save) = save_data.undo_stack {
        let load_layer_fn = |data: &UndoLayerSaveData| -> Option<Layer> {
            let image = match load_image(&data.source_path, data.dpi, data.page_index) {
                Ok(img) => img,
                Err(e) => {
                    error!("Failed to reload image for undo layer {}: {}", data.name, e);
                    // Skip this command if the image can't be loaded
                    return None;
                }
            };
            let mut layer = Layer::new(
                data.id,
                data.name.clone(),
                data.source_path.clone(),
                image,
                data.is_reference,
            );
            layer.homography_matrix = data.homography_matrix;
            layer.visible = data.visible;
            layer.opacity = data.opacity;
            layer.blend_color = data.blend_color;
            layer.annotations = data.annotations.clone();
            layer.offset_x = data.offset_x;
            layer.offset_y = data.offset_y;
            if let (Some(matrix), Some(size)) = (data.homography_matrix, data.aligned_size) {
                layer.aligned = dc_core::HomographySolver::new()
                    .warp_image(
                        &layer.original,
                        &dc_core::HomographyMatrix { elements: matrix },
                        size,
                    )
                    .ok()
                    .map(std::sync::Arc::new);
            }
            Some(layer)
        };
        session.undo_stack = crate::undo::UndoStack::from_save(undo_save, &load_layer_fn);
    }

    info!("Session loaded from {:?}", path);
    Ok(session)
}

// Helper to convert Layer to LayerSaveData
impl From<&Layer> for LayerSaveData {
    fn from(layer: &Layer) -> Self {
        Self {
            id: layer.id,
            name: layer.name.clone(),
            source_path: layer
                .source_path
                .canonicalize()
                .unwrap_or_else(|_| layer.source_path.clone()),
            homography_matrix: layer.homography_matrix,
            visible: layer.visible,
            opacity: layer.opacity,
            blend_color: layer.blend_color,
            is_reference: layer.is_reference,
            annotations: layer.annotations.clone(),
            offset_x: layer.offset_x,
            offset_y: layer.offset_y,
            dpi: layer.original.dpi,
            page_index: layer.original.page_index,
            aligned_size: layer.aligned.as_ref().map(|b| b.dimensions()),
            embedded_png: None,
            aligned_png: None,
        }
    }
}

fn default_dpi() -> u32 {
    dc_core::DEFAULT_RENDER_DPI
}

fn load_image(path: &Path, dpi: u32, page_index: Option<usize>) -> CoreResult<RasterBuffer> {
    dc_core::LoaderRegistry::with_defaults().load(
        path,
        &dc_core::LoadConfig::default()
            .with_dpi(dpi)
            .with_page(page_index.unwrap_or(0)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};
    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("diffcomp-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        dir
    }
    #[test]
    fn roundtrip_memory_images_alignment_and_comparison_settings() {
        let dir = temp_dir();
        let path = dir.join("session.dcs");
        let mut session = SessionState::new();
        let id = session
            .add_layer(
                "memory".into(),
                dir.join("absent.png"),
                RasterBuffer::new(RgbaImage::from_pixel(3, 4, Rgba([40, 80, 90, 128])), 150),
            )
            .unwrap();
        let layer = session.get_layer_mut(id).unwrap();
        layer.original = std::sync::Arc::new({
            let mut b = (*layer.original).clone();
            b.page_index = Some(2);
            b
        });
        layer.offset_x = -2.5;
        layer.offset_y = 10.0;
        layer.aligned = Some(std::sync::Arc::new(RasterBuffer::new(
            RgbaImage::from_pixel(5, 6, Rgba([0, 50, 100, 255])),
            150,
        )));
        session.calibration.pixels_per_unit = 25.0;
        session.diff_config.blend_mode = dc_core::diff::BlendMode::Heatmap;
        session.diff_config.morphological_tolerance = false;
        session.count_counter = 42;
        save_session(&session, &path).unwrap();
        let loaded = load_session(&path).unwrap();
        let restored = loaded.get_layer(id).unwrap();
        assert_eq!(restored.original.image, session.layers[0].original.image);
        assert_eq!(
            restored.active_image().image,
            session.layers[0].active_image().image
        );
        assert_eq!((restored.offset_x, restored.offset_y), (-2.5, 10.0));
        assert_eq!(restored.original.page_index, Some(2));
        assert_eq!(restored.original.dpi, 150);
        assert_eq!(loaded.calibration, session.calibration);
        assert_eq!(loaded.count_counter, 42);
        assert_eq!(
            loaded.diff_config.blend_mode,
            dc_core::diff::BlendMode::Heatmap
        );
        assert!(!loaded.diff_config.morphological_tolerance);
        // A second save atomically replaces the previous file.
        save_session(&loaded, &path).unwrap();
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn missing_source_is_reported_instead_of_silently_replaced() {
        let dir = temp_dir();
        let image_path = dir.join("image.png");
        let path = dir.join("session.dcs");
        let image = RgbaImage::from_pixel(2, 2, Rgba([255; 4]));
        image.save(&image_path).unwrap();
        let mut session = SessionState::new();
        session
            .add_layer(
                "image".into(),
                image_path.clone(),
                RasterBuffer::new(image, 72),
            )
            .unwrap();
        save_session(&session, &path).unwrap();
        std::fs::remove_file(image_path).unwrap();
        assert!(load_session(&path).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
