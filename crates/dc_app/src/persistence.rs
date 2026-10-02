// =============================================================================
// dc_app/persistence - Session Saving and Loading
// =============================================================================
// Handles serialization and deserialization of the application session.
// =============================================================================

use crate::state::SessionState;
use crate::undo::{
    saved_page, LayerSaveData as UndoLayerSaveData, PageSaveData as UndoPageSaveData, UndoStackSave,
};
use base64::Engine;
use dc_core::{
    Annotation, CoreError, CoreResult, Layer, LayerColor, LayerId, LayerPage, RasterBuffer,
    Viewport,
};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
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
    /// Page shown for all documents
    #[serde(default)]
    current_page: usize,
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
    /// Every page of a multi-page document. The top-level fields above repeat
    /// the shown page so older versions still open the session.
    #[serde(default)]
    pages: Vec<PageData>,
    #[serde(default)]
    active_page: usize,
}

/// Saved state of one document page.
#[derive(Serialize, Deserialize)]
struct PageData {
    #[serde(flatten)]
    page: UndoPageSaveData,
    #[serde(default)]
    embedded_png: Option<String>,
    #[serde(default)]
    aligned_png: Option<String>,
}

fn encode_png(buffer: &RasterBuffer) -> CoreResult<String> {
    let mut png = std::io::Cursor::new(Vec::new());
    buffer
        .image
        .write_to(&mut png, image::ImageFormat::Png)
        .map_err(|e| CoreError::InternalError {
            message: e.to_string(),
        })?;
    Ok(base64::engine::general_purpose::STANDARD.encode(png.into_inner()))
}

fn decode_png(encoded: &str, dpi: u32) -> CoreResult<RasterBuffer> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|e| CoreError::InternalError {
            message: e.to_string(),
        })?;
    let decoded = image::load_from_memory(&bytes).map_err(|e| CoreError::ImageDecodeError {
        reason: e.to_string(),
    })?;
    Ok(RasterBuffer::from_dynamic(decoded, dpi))
}

/// Rebuild a page and its alignment from saved metadata.
fn restore_page(
    original: RasterBuffer,
    data: &UndoPageSaveData,
    aligned_png: Option<&str>,
) -> CoreResult<LayerPage> {
    let mut page = LayerPage::new(original);
    page.homography_matrix = data.homography_matrix;
    page.annotations = data.annotations.clone();
    page.offset_x = data.offset_x;
    page.offset_y = data.offset_y;
    if let Some(encoded) = aligned_png {
        page.aligned = Some(Arc::new(decode_png(encoded, page.original.dpi)?));
    } else if let (Some(matrix), Some(size)) = (data.homography_matrix, data.aligned_size) {
        page.aligned = Some(Arc::new(dc_core::HomographySolver::new().warp_image(
            &page.original,
            &dc_core::HomographyMatrix { elements: matrix },
            size,
        )?));
    } else if let Some(size) = data.aligned_size {
        let mut padded = image::RgbaImage::from_pixel(size.0, size.1, image::Rgba([255; 4]));
        image::imageops::replace(&mut padded, &page.original.image, 0, 0);
        page.aligned = Some(Arc::new(RasterBuffer::new(padded, page.original.dpi)));
    }
    Ok(page)
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
                let embed = !layer.source_path.is_file();
                let shown = saved_page(layer);
                if embed {
                    data.embedded_png = Some(encode_png(&shown.original)?);
                }
                if let Some(aligned) = &shown.aligned {
                    data.aligned_png = Some(encode_png(aligned)?);
                }
                if layer.page_count() > 1 {
                    data.pages = (0..layer.page_count())
                        .map(|index| {
                            let page = layer.page(index);
                            Ok(PageData {
                                page: UndoPageSaveData::from(&page),
                                embedded_png: if embed {
                                    Some(encode_png(&page.original)?)
                                } else {
                                    None
                                },
                                aligned_png: page.aligned.as_deref().map(encode_png).transpose()?,
                            })
                        })
                        .collect::<CoreResult<_>>()?;
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
        current_page: session.current_page,
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
        let pages = if layer_data.pages.is_empty() {
            let image = if let Some(encoded) = &layer_data.embedded_png {
                let mut buffer = decode_png(encoded, layer_data.dpi)?;
                buffer.page_index = layer_data.page_index;
                buffer
            } else {
                load_image(&source_path, layer_data.dpi, layer_data.page_index)?
            };
            let data = UndoPageSaveData {
                homography_matrix: layer_data.homography_matrix,
                annotations: layer_data.annotations.clone(),
                offset_x: layer_data.offset_x,
                offset_y: layer_data.offset_y,
                aligned_size: layer_data.aligned_size,
            };
            vec![restore_page(
                image,
                &data,
                layer_data.aligned_png.as_deref(),
            )?]
        } else {
            let mut from_disk: Vec<Option<RasterBuffer>> =
                if layer_data.pages.iter().any(|p| p.embedded_png.is_none()) {
                    load_all_pages(&source_path, layer_data.dpi)?
                        .into_iter()
                        .map(Some)
                        .collect()
                } else {
                    Vec::new()
                };
            layer_data
                .pages
                .iter()
                .enumerate()
                .map(|(index, page)| {
                    let original =
                        match &page.embedded_png {
                            Some(encoded) => {
                                let mut buffer = decode_png(encoded, layer_data.dpi)?;
                                buffer.page_index = Some(index);
                                buffer
                            }
                            None => from_disk.get_mut(index).and_then(Option::take).ok_or_else(
                                || CoreError::ImageDecodeError {
                                    reason: format!(
                                        "{} has no page {}",
                                        source_path.display(),
                                        index + 1
                                    ),
                                },
                            )?,
                        };
                    restore_page(original, &page.page, page.aligned_png.as_deref())
                })
                .collect::<CoreResult<Vec<_>>>()?
        };

        // Reconstruct layer
        let mut layer = Layer::new(
            layer_data.id,
            layer_data.name,
            source_path,
            RasterBuffer::new(image::RgbaImage::new(1, 1), layer_data.dpi),
            layer_data.is_reference,
        );
        let active_page = if pages.len() > 1 {
            layer_data.active_page
        } else {
            0
        };
        layer.set_pages(pages, active_page);
        layer.visible = layer_data.visible;
        layer.opacity = layer_data.opacity;
        layer.blend_color = layer_data.blend_color;

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
    session.current_page = save_data.current_page;
    session.sync_pages();

    // Restore undo stack if present
    if let Some(undo_save) = save_data.undo_stack {
        let load_layer_fn = |data: &UndoLayerSaveData| -> Option<Layer> {
            let restore = || -> CoreResult<Layer> {
                let mut layer = Layer::new(
                    data.id,
                    data.name.clone(),
                    data.source_path.clone(),
                    RasterBuffer::new(image::RgbaImage::new(1, 1), data.dpi),
                    data.is_reference,
                );
                if data.pages.is_empty() {
                    let image = load_image(&data.source_path, data.dpi, data.page_index)?;
                    let page = UndoPageSaveData {
                        homography_matrix: data.homography_matrix,
                        annotations: data.annotations.clone(),
                        offset_x: data.offset_x,
                        offset_y: data.offset_y,
                        aligned_size: data.aligned_size,
                    };
                    layer.set_pages(vec![restore_page(image, &page, None)?], 0);
                } else {
                    let pages = load_all_pages(&data.source_path, data.dpi)?
                        .into_iter()
                        .zip(&data.pages)
                        .map(|(image, page)| restore_page(image, page, None))
                        .collect::<CoreResult<Vec<_>>>()?;
                    layer.set_pages(pages, data.active_page);
                }
                layer.visible = data.visible;
                layer.opacity = data.opacity;
                layer.blend_color = data.blend_color;
                Ok(layer)
            };
            match restore() {
                Ok(layer) => Some(layer),
                Err(e) => {
                    error!("Failed to reload image for undo layer {}: {}", data.name, e);
                    // Skip this command if the image can't be loaded
                    None
                }
            }
        };
        session.undo_stack = crate::undo::UndoStack::from_save(undo_save, &load_layer_fn);
    }

    info!("Session loaded from {:?}", path);
    Ok(session)
}

// Helper to convert Layer to LayerSaveData
impl From<&Layer> for LayerSaveData {
    fn from(layer: &Layer) -> Self {
        let page = saved_page(layer);
        Self {
            id: layer.id,
            name: layer.name.clone(),
            source_path: layer
                .source_path
                .canonicalize()
                .unwrap_or_else(|_| layer.source_path.clone()),
            homography_matrix: page.homography_matrix,
            visible: layer.visible,
            opacity: layer.opacity,
            blend_color: layer.blend_color,
            is_reference: layer.is_reference,
            annotations: page.annotations.clone(),
            offset_x: page.offset_x,
            offset_y: page.offset_y,
            dpi: page.original.dpi,
            page_index: page.original.page_index,
            aligned_size: page.aligned.as_ref().map(|b| b.dimensions()),
            embedded_png: None,
            aligned_png: None,
            pages: Vec::new(),
            active_page: layer.active_page,
        }
    }
}

fn default_dpi() -> u32 {
    dc_core::DEFAULT_RENDER_DPI
}

fn load_all_pages(path: &Path, dpi: u32) -> CoreResult<Vec<RasterBuffer>> {
    dc_core::LoaderRegistry::with_defaults()
        .load_all_pages(path, &dc_core::LoadConfig::default().with_dpi(dpi))
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
    fn roundtrip_multi_page_tiff_keeps_page_state() {
        use tiff::encoder::{colortype, TiffEncoder};
        let dir = temp_dir();
        let tiff_path = dir.join("sheets.tif");
        {
            let file = std::fs::File::create(&tiff_path).unwrap();
            let mut encoder = TiffEncoder::new(file).unwrap();
            encoder
                .write_image::<colortype::Gray8>(2, 2, &[0, 255, 255, 255])
                .unwrap();
            encoder
                .write_image::<colortype::Gray8>(3, 2, &[255, 0, 255, 255, 255, 255])
                .unwrap();
        }
        let pages = dc_core::LoaderRegistry::with_defaults()
            .load_all_pages(&tiff_path, &dc_core::LoadConfig::default())
            .unwrap();
        let mut session = SessionState::new();
        let id = session
            .add_document("sheets".into(), tiff_path.clone(), pages)
            .unwrap();
        session.get_layer_mut(id).unwrap().offset_x = 4.0;
        session.set_page(1);
        session.get_layer_mut(id).unwrap().offset_y = 9.0;
        let path = dir.join("session.dcs");
        save_session(&session, &path).unwrap();

        let mut loaded = load_session(&path).unwrap();
        assert_eq!(loaded.current_page, 1);
        let layer = loaded.get_layer(id).unwrap();
        assert_eq!(layer.page_count(), 2);
        assert_eq!(layer.original.dimensions(), (3, 2));
        assert_eq!(layer.offset_y, 9.0);
        loaded.set_page(0);
        let layer = loaded.get_layer(id).unwrap();
        assert_eq!(layer.original.dimensions(), (2, 2));
        assert_eq!(layer.offset_x, 4.0);
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
