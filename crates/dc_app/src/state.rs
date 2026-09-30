// =============================================================================
// dc_app/state - Application State Management
// =============================================================================
// Centralized state management for the application.
//
// ## State Architecture
//
// AppState (root)
// ├── SessionState (current comparison session)
// │   ├── layers: Vec<Layer>  (from dc_core)
// │   ├── viewport: Viewport  (synchronized pan/zoom)
// │   └── diff_config: DiffConfig
// ├── LicenseState (licensing)
// └── UIState (panels, dialogs, etc.)
// =============================================================================

use dc_core::{
    AlignmentConfig, AlignmentEngine, Annotation, DiffConfig, DiffEngine, Layer, LayerId,
    LoaderRegistry, RasterBuffer, Tool, Viewport, MAX_LAYERS,
};
use dc_gpu::{GpuDiffEngine, GpuDiffParams};
use dc_license::{LicenseStatus, LicenseVerifier};
use image::RgbaImage;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{info, warn};

use crate::undo::UndoStack;

/// Computation mode preference
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ComputeMode {
    #[default]
    Auto, // Prefer GPU, fallback to CPU
    Gpu, // Force GPU (no-op if GPU unavailable)
    Cpu, // Force CPU
}

/// Root application state.
pub struct AppState {
    /// Current comparison session
    pub session: SessionState,

    /// License information
    pub license: LicenseState,

    /// UI-related state
    pub ui: UIState,

    /// Drawing tool state
    pub tools: ToolState,

    /// Document loader registry
    pub loader_registry: LoaderRegistry,

    /// Alignment engine
    pub alignment_engine: AlignmentEngine,

    /// Diff computation engine (CPU fallback)
    pub diff_engine: DiffEngine,

    /// GPU diff engine (optional — None if no GPU adapter found)
    pub gpu_engine: Option<Arc<GpuDiffEngine>>,

    /// GPU diff parameters (threshold, blend mode, etc.)
    pub gpu_params: GpuDiffParams,

    /// Preferred computation mode (GPU vs CPU)
    pub compute_mode: ComputeMode,
}

impl Default for AppState {
    fn default() -> Self {
        Self::new(None)
    }
}

impl AppState {
    /// Create a new application state.
    pub fn new(gpu_engine_opt: Option<GpuDiffEngine>) -> Self {
        // GPU engine is passed in (initialized async or sync depending on platform)
        let gpu_engine = match gpu_engine_opt {
            Some(engine) => {
                info!("GPU compute engine initialized");
                Some(Arc::new(engine))
            }
            None => {
                warn!("GPU compute engine unavailable, using CPU fallback");
                None
            }
        };

        // Default Compute Mode:
        // - Web: Force CPU by default (WebGPU readback is currently too slow/unstable)
        // - Native: Use Auto (Prefer GPU)
        let default_mode = if cfg!(target_arch = "wasm32") {
            ComputeMode::Cpu
        } else {
            ComputeMode::Auto
        };

        Self {
            session: SessionState::new(),
            license: LicenseState::new(),
            ui: UIState::default(),
            tools: ToolState::default(),
            loader_registry: LoaderRegistry::with_defaults(),
            alignment_engine: AlignmentEngine::with_defaults(),
            diff_engine: DiffEngine::with_defaults(),
            gpu_engine,
            gpu_params: GpuDiffParams::default(),
            compute_mode: default_mode,
        }
    }

    /// Create a new application state reusing an existing shared GPU engine.
    pub fn new_with_gpu_arc(gpu_engine: Option<Arc<GpuDiffEngine>>) -> Self {
        let default_mode = if cfg!(target_arch = "wasm32") {
            ComputeMode::Cpu
        } else {
            ComputeMode::Auto
        };

        Self {
            session: SessionState::new(),
            license: LicenseState::new(),
            ui: UIState::default(),
            tools: ToolState::default(),
            loader_registry: LoaderRegistry::with_defaults(),
            alignment_engine: AlignmentEngine::with_defaults(),
            diff_engine: DiffEngine::with_defaults(),
            gpu_engine,
            gpu_params: GpuDiffParams::default(),
            compute_mode: default_mode,
        }
    }

    /// Check if the application is licensed for a feature.
    pub fn is_feature_licensed(&self, feature: dc_license::FeatureFlag) -> bool {
        self.license
            .status
            .as_ref()
            .map(|s| s.has_feature(feature))
            .unwrap_or(false)
    }
}

/// State for the active drawing tool.
pub struct ToolState {
    /// Currently active tool (if any)
    pub active_tool: Option<Tool>,

    /// Annotation currently being drawn
    pub pending_annotation: Option<Annotation>,

    /// Active sequence group for the Count/Punch-List tool.
    /// When set, new Count annotations use `next_sequence_number` scoped to this group.
    pub active_sequence_group: Option<String>,
}

impl Default for ToolState {
    fn default() -> Self {
        Self {
            active_tool: None,
            pending_annotation: None,
            active_sequence_group: None,
        }
    }
}

/// State for the current comparison session.
pub struct SessionState {
    /// Layers in the comparison stack
    pub layers: Vec<Layer>,

    /// Next layer ID to assign
    pub(crate) next_layer_id: u32,

    /// Currently selected layer (for operations)
    pub selected_layer: Option<LayerId>,

    /// The synchronized viewport (pan/zoom state)
    pub viewport: Viewport,

    /// Diff configuration
    pub diff_config: DiffConfig,

    /// Alignment configuration
    pub alignment_config: AlignmentConfig,

    /// The computed diff result image (if any)
    pub diff_result: Option<RasterBuffer>,

    /// Whether the session has unsaved changes
    pub is_dirty: bool,

    /// Path to the session file (if saved)
    pub session_path: Option<PathBuf>,

    /// Currently selected annotation (LayerId, Annotation ID)
    pub selected_annotation: Option<(LayerId, String)>,

    /// Calibration mapping (pixels → real-world units)
    pub calibration: dc_core::Calibration,

    /// Next count number for Count tool
    pub count_counter: u32,

    /// Known sequence group names for the punch-list / Count tool.
    /// This list persists across undo/redo and is saved with the session.
    pub sequence_groups: Vec<String>,

    /// Custom columns for the Markups List
    pub custom_columns: Vec<CustomColumn>,

    /// Customizable workflow states for annotations
    pub workflow_states: Vec<WorkflowState>,

    /// Map from Annotation ID -> WorkflowState ID
    pub annotation_statuses: std::collections::HashMap<String, String>,

    /// Undo/redo stack (50 steps deep)
    pub undo_stack: UndoStack,
}

/// A customizable workflow state for annotations
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct WorkflowState {
    pub id: String,
    pub name: String,
    pub color: [u8; 3],
}

impl WorkflowState {
    pub fn default_states() -> Vec<Self> {
        vec![
            Self {
                id: "accepted".to_string(),
                name: "Accepted".to_string(),
                color: [80, 200, 120],
            },
            Self {
                id: "rejected".to_string(),
                name: "Rejected".to_string(),
                color: [220, 60, 60],
            },
            Self {
                id: "cancelled".to_string(),
                name: "Cancelled".to_string(),
                color: [180, 120, 40],
            },
        ]
    }
}

/// Definition of a user-defined column.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CustomColumn {
    pub id: String,
    pub name: String,
    pub col_type: ColumnType,
    pub default_value: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum ColumnType {
    Text,
    Number,
    Checkbox,
    Formula(String),
}

impl Default for SessionState {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionState {
    /// Create a new empty session.
    pub fn new() -> Self {
        Self {
            layers: Vec::new(),
            next_layer_id: 0,
            selected_layer: None,
            viewport: Viewport::default(),
            diff_config: DiffConfig::default(),
            alignment_config: AlignmentConfig::default(),
            diff_result: None,
            is_dirty: false,
            session_path: None,
            selected_annotation: None,
            calibration: dc_core::Calibration::default(),
            count_counter: 1,
            sequence_groups: Vec::new(),
            custom_columns: Vec::new(),
            workflow_states: WorkflowState::default_states(),
            annotation_statuses: std::collections::HashMap::new(),
            undo_stack: UndoStack::default(),
        }
    }

    /// Add a new layer from a loaded image.
    pub fn add_layer(
        &mut self,
        name: String,
        source_path: PathBuf,
        image: RasterBuffer,
    ) -> Result<LayerId, String> {
        if self.layers.len() >= MAX_LAYERS {
            return Err(format!("Maximum of {} layers allowed", MAX_LAYERS));
        }

        let id = LayerId::new(self.next_layer_id);
        self.next_layer_id += 1;

        let is_reference = self.layers.is_empty();
        let layer = Layer::new(id, name, source_path, image, is_reference);

        info!(
            layer_id = %id,
            is_reference,
            "Adding new layer"
        );

        self.layers.push(layer);
        self.selected_layer = Some(id);
        self.is_dirty = true;
        // Invalidate existing diff so auto-diff re-triggers with all visible layers
        self.diff_result = None;

        // Fit viewport to the new image if it's the first layer
        if is_reference {
            if let Some(layer) = self.layers.first() {
                let (w, h) = layer.original.dimensions();
                self.viewport.fit_to_image(w, h, 800.0, 600.0);
            }
        }

        Ok(id)
    }

    /// Toggle layer visibility by ID.
    pub fn toggle_layer_visibility(&mut self, id: LayerId) {
        if let Some(layer) = self.layers.iter_mut().find(|l| l.id == id) {
            layer.visible = !layer.visible;
            self.is_dirty = true;
            // Clear diff result when visibility changes
            self.diff_result = None;
        }
    }

    /// Remove a layer by ID.
    pub fn remove_layer(&mut self, id: LayerId) -> bool {
        if let Some(pos) = self.layers.iter().position(|l| l.id == id) {
            let was_reference = self.layers[pos].is_reference;
            self.layers.remove(pos);

            // If we removed the reference, make the first remaining layer the new reference
            if was_reference && !self.layers.is_empty() {
                self.layers[0].is_reference = true;
                // Clear alignment data since reference changed
                for layer in &mut self.layers {
                    layer.aligned = None;
                    layer.homography_matrix = None;
                    layer.alignment_confidence = None;
                }
            }

            // Update selection
            if self.selected_layer == Some(id) {
                self.selected_layer = self.layers.first().map(|l| l.id);
            }

            self.is_dirty = true;
            self.diff_result = None;
            true
        } else {
            false
        }
    }

    /// Get a layer by ID.
    pub fn get_layer(&self, id: LayerId) -> Option<&Layer> {
        self.layers.iter().find(|l| l.id == id)
    }

    /// Get a mutable layer by ID.
    pub fn get_layer_mut(&mut self, id: LayerId) -> Option<&mut Layer> {
        self.layers.iter_mut().find(|l| l.id == id)
    }

    /// Get the reference layer (first layer, if any).
    pub fn reference_layer(&self) -> Option<&Layer> {
        self.layers.iter().find(|l| l.is_reference)
    }

    /// Get all target layers (non-reference layers).
    pub fn target_layers(&self) -> Vec<&Layer> {
        self.layers.iter().filter(|l| !l.is_reference).collect()
    }

    /// Get visible layers for rendering.
    pub fn visible_layers(&self) -> Vec<&Layer> {
        self.layers.iter().filter(|l| l.visible).collect()
    }

    /// Get the visible reference layer (if any and visible).
    pub fn visible_reference(&self) -> Option<&Layer> {
        self.layers.iter().find(|l| l.is_reference && l.visible)
    }

    /// Get all visible non-reference (target) layers.
    pub fn visible_target_layers(&self) -> Vec<&Layer> {
        self.layers
            .iter()
            .filter(|l| l.visible && !l.is_reference)
            .collect()
    }

    /// Check if there are enough visible layers for comparison.
    /// Requires at least 2 visible layers (1 reference + 1 target).
    pub fn can_compare(&self) -> bool {
        let visible = self.visible_layers();
        visible.len() >= 2 && visible.iter().any(|l| l.is_reference)
    }

    /// Check if alignment has been performed.
    pub fn is_aligned(&self) -> bool {
        self.layers.iter().skip(1).all(|l| l.aligned.is_some())
    }

    /// Move a layer up in the stack.
    pub fn move_layer_up(&mut self, id: LayerId) -> bool {
        if let Some(pos) = self.layers.iter().position(|l| l.id == id) {
            if pos > 0 {
                self.layers.swap(pos, pos - 1);
                self.is_dirty = true;
                self.diff_result = None;
                return true;
            }
        }
        false
    }

    /// Move a layer down in the stack.
    pub fn move_layer_down(&mut self, id: LayerId) -> bool {
        if let Some(pos) = self.layers.iter().position(|l| l.id == id) {
            if pos < self.layers.len() - 1 {
                self.layers.swap(pos, pos + 1);
                self.is_dirty = true;
                self.diff_result = None;
                return true;
            }
        }
        false
    }

    /// Move a layer to a specific index.
    pub fn move_layer_to(&mut self, id: LayerId, index: usize) -> bool {
        if let Some(pos) = self.layers.iter().position(|l| l.id == id) {
            if index < self.layers.len() && pos != index {
                let layer = self.layers.remove(pos);
                self.layers.insert(index, layer);
                self.is_dirty = true;
                self.diff_result = None;
                return true;
            }
        }
        false
    }

    /// Composite all visible layers into a single image.
    fn composite_visible_layers(&self) -> Option<RasterBuffer> {
        let visible_layers = self.visible_layers();
        if visible_layers.is_empty() {
            return None;
        }

        // Use reference layer dimensions for the composite,
        // or just the first visible layer if no reference is visible
        let (width, height) = if let Some(ref_layer) = self.visible_reference() {
            ref_layer.active_image().dimensions()
        } else {
            visible_layers[0].active_image().dimensions()
        };

        if width == 0 || height == 0 {
            return None;
        }

        let origin = self.visible_reference().unwrap_or(visible_layers[0]);
        let mut composite = RgbaImage::new(width, height);
        for layer in visible_layers.iter().rev() {
            let img = &layer.active_image().image;
            let ox = (layer.offset_x - origin.offset_x).round() as i64;
            let oy = (layer.offset_y - origin.offset_y).round() as i64;
            let opacity = layer.opacity.clamp(0.0, 1.0);
            for (y, row) in composite
                .as_mut()
                .chunks_exact_mut(width as usize * 4)
                .enumerate()
            {
                let sy = (y as i64).saturating_sub(oy);
                if sy < 0 || sy >= img.height() as i64 {
                    continue;
                }
                for (x, dst) in row.chunks_exact_mut(4).enumerate() {
                    let sx = (x as i64).saturating_sub(ox);
                    if sx < 0 || sx >= img.width() as i64 {
                        continue;
                    }
                    let src = img.get_pixel(sx as u32, sy as u32);
                    let sa = src[3] as f32 / 255.0 * opacity;
                    let da = dst[3] as f32 / 255.0;
                    let alpha = sa + da * (1.0 - sa);
                    if alpha > 0.0 {
                        for c in 0..3 {
                            dst[c] = ((src[c] as f32 * sa + dst[c] as f32 * da * (1.0 - sa))
                                / alpha)
                                .round() as u8;
                        }
                    }
                    dst[3] = (alpha * 255.0).round() as u8;
                }
            }
        }
        Some(RasterBuffer::new(composite, origin.active_image().dpi))
    }

    /// Flatten visible layers into a single layer.
    pub fn flatten_layers(&mut self, keep_annotations: bool) {
        // First, collect annotations if requested
        let mut collected_annotations = Vec::new();
        if keep_annotations {
            for layer in self.visible_layers() {
                collected_annotations.extend(layer.annotations.iter().cloned());
            }
        }

        if let Some(composite) = self.composite_visible_layers() {
            // Remove all visible layers
            self.layers.retain(|l| !l.visible);

            // Add composite layer
            match self.add_layer(
                "Flattened".to_string(),
                PathBuf::from("flattened.png"),
                composite,
            ) {
                Ok(new_id) => {
                    // Start with annotations visible on the new layer
                    if !collected_annotations.is_empty() {
                        if let Some(new_layer) = self.get_layer_mut(new_id) {
                            new_layer.annotations = collected_annotations;
                        }
                    }
                }
                Err(e) => {
                    // Should probably log this or show status
                    warn!("Failed to add flattened layer: {}", e);
                }
            }
        }
    }

    /// Create a snapshot of visible layers as a new layer.
    pub fn create_snapshot(&mut self) {
        if let Some(composite) = self.composite_visible_layers() {
            let _ = self.add_layer(
                "Snapshot".to_string(),
                PathBuf::from("snapshot.png"),
                composite,
            );
        }
    }

    /// Public accessor for compositing visible layers (used by export).
    pub fn composite_visible_layers_pub(&self) -> Option<RasterBuffer> {
        self.composite_visible_layers()
    }

    /// Set a layer as the new reference.
    pub fn set_reference(&mut self, id: LayerId) {
        // Clear old reference
        for layer in &mut self.layers {
            if layer.is_reference {
                layer.is_reference = false;
            }
        }

        // Set new reference
        if let Some(layer) = self.get_layer_mut(id) {
            layer.is_reference = true;
            layer.aligned = None;
            layer.homography_matrix = None;
        }

        // Clear all alignment data since reference changed
        for layer in &mut self.layers {
            if layer.id != id {
                layer.aligned = None;
                layer.homography_matrix = None;
                layer.alignment_confidence = None;
            }
        }

        self.diff_result = None;
        self.is_dirty = true;
    }

    /// Clear the entire session.
    pub fn clear(&mut self) {
        self.layers.clear();
        self.next_layer_id = 0;
        self.selected_layer = None;
        self.viewport = Viewport::default();
        self.diff_result = None;
        self.is_dirty = false;
        self.session_path = None;
        self.selected_annotation = None;
        self.undo_stack.clear();
    }

    /// Get the layer count.
    pub fn layer_count(&self) -> usize {
        self.layers.len()
    }
}

/// License-related state.
pub struct LicenseState {
    /// License verifier
    pub verifier: LicenseVerifier,

    /// Current license status (if verified)
    pub status: Option<LicenseStatus>,

    /// The license key string (for display/re-verification)
    pub key_string: Option<String>,

    /// Error message if verification failed
    pub error: Option<String>,

    /// Local hardware ID (cached)
    pub local_hwid: Option<String>,
}

impl Default for LicenseState {
    fn default() -> Self {
        Self::new()
    }
}

impl LicenseState {
    /// Create a new license state.
    pub fn new() -> Self {
        let mut verifier = LicenseVerifier::new();
        let local_hwid = verifier.get_local_hwid_string().ok();

        Self {
            verifier,
            status: None,
            key_string: None,
            error: None,
            local_hwid,
        }
    }

    /// Verify a license key.
    pub fn verify(&mut self, key: &str) {
        self.key_string = Some(key.to_string());
        self.error = None;

        match self.verifier.verify(key) {
            Ok(status) => {
                if status.is_valid {
                    info!(licensee = %status.licensee, "License verified successfully");
                } else {
                    warn!(warnings = ?status.warnings, "License verification issues");
                }
                self.status = Some(status);
            }
            Err(e) => {
                warn!(error = %e, "License verification failed");
                self.error = Some(e.to_string());
                self.status = None;
            }
        }
    }

    /// Check if a valid license is present.
    pub fn is_licensed(&self) -> bool {
        self.status.as_ref().map(|s| s.is_valid).unwrap_or(false)
    }

    /// Get the licensee name if licensed.
    pub fn licensee(&self) -> Option<&str> {
        self.status
            .as_ref()
            .filter(|s| s.is_valid)
            .map(|s| s.licensee.as_str())
    }
}

/// UI-related state (panels, dialogs, etc.).
pub struct UIState {
    /// Is the license dialog open?
    pub show_license_dialog: bool,

    /// Is the about dialog open?
    pub show_about_dialog: bool,

    /// Is the settings dialog open?
    pub show_settings_dialog: bool,

    /// Is the flatten dialog open?
    pub show_flatten_dialog: bool,

    /// Auto-compute diff when two images are loaded
    pub auto_diff_enabled: bool,

    /// Status bar message
    pub status_message: String,

    /// Is a file operation in progress?
    pub is_loading: bool,

    /// Generation currently being computed; invalidation never starts a second worker.
    pub diff_running: Option<u64>,
    /// Number of document decoding and alignment jobs still outstanding.
    pub pending_operations: usize,
    /// Debounce rapid parameter changes before allocating image snapshots.
    pub diff_changed_at: Option<std::time::Instant>,
    /// Prevent automatic retries of a failed generation on every frame.
    pub diff_failed: bool,

    /// Progress of current operation (0.0 - 1.0)
    pub progress: f32,

    /// Current tool mode
    pub tool_mode: ToolMode,

    /// License key input field
    pub license_input: String,

    /// Current drag interaction state
    pub drag_state: DragState,

    /// Markups list panel state (sorting, filtering, statuses)
    pub markups_list: crate::panels::MarkupsListState,

    /// Legend panel state
    pub legend: crate::panels::LegendPanelState,

    /// Request to open file dialog (triggered by panels)
    pub request_file_dialog: bool,

    /// Flag: diff parameters changed, needs recomputation.
    /// Set by panels (properties, layer) when offsets/colors/blend change.
    /// Processed in update() to properly cancel in-flight computations.
    pub diff_invalidated: bool,
    /// Fit using the actual canvas dimensions on the next viewport pass.
    pub fit_view_requested: bool,

    /// Flag: alignment parameters changed or reference layer changed.
    /// Needs re-alignment of all target layers.
    pub alignment_invalidated: bool,

    /// Request to open a file dialog for slip-sheeting
    pub request_slipsheet_dialog: Option<SlipSheetRequest>,

    /// Request to export: flattened image
    pub export_flattened_requested: bool,
    /// Request to export: diff result image
    pub export_diff_requested: bool,
    /// Request to export: current/selected layer image
    pub export_layer_requested: bool,
    /// Request to export markups CSV (from File menu)
    pub export_markups_csv_requested: bool,
    /// Request to export markups XLSX (from File menu)
    pub export_markups_xlsx_requested: bool,
    /// Request Save As (new path)
    pub request_save_as: bool,

    /// Current UI language
    pub language: crate::i18n::Language,
}

/// A request to slip-sheet one or more layers
#[derive(Clone, Debug, PartialEq)]
pub enum SlipSheetRequest {
    /// Slip-sheet a single layer
    Single(LayerId),
    /// Slip-sheet all layers originating from the same source document
    Batch(PathBuf),
}

#[derive(Clone, Debug, PartialEq)]
pub enum DragState {
    None,
    /// Moving the whole annotation
    Moving {
        start_mouse_pos: dc_core::Point,
        // We store the original points to apply offsets relative to them
        // This avoids drift/accumulation errors
        original_data: Box<dc_core::AnnotationData>,
    },
    /// Moving a specific control handle
    DraggingHandle {
        handle_index: usize,
        start_mouse_pos: dc_core::Point,
        original_data: Box<dc_core::AnnotationData>,
    },
}

impl Default for DragState {
    fn default() -> Self {
        Self::None
    }
}

impl Default for UIState {
    fn default() -> Self {
        Self::new()
    }
}

impl UIState {
    /// Create default UI state with panels visible.
    pub fn new() -> Self {
        Self {
            show_license_dialog: false,
            show_about_dialog: false,
            show_settings_dialog: false,
            show_flatten_dialog: false,
            auto_diff_enabled: true,
            status_message: crate::i18n::tr("status.ready").to_string(),
            is_loading: false,
            diff_running: None,
            pending_operations: 0,
            diff_changed_at: None,
            diff_failed: false,
            progress: 0.0,
            tool_mode: ToolMode::Pan,
            license_input: String::new(),
            drag_state: DragState::None,
            markups_list: crate::panels::MarkupsListState::default(),
            legend: crate::panels::LegendPanelState::new(),
            request_file_dialog: false,
            diff_invalidated: false,
            fit_view_requested: false,
            alignment_invalidated: false,
            request_slipsheet_dialog: None,
            export_flattened_requested: false,
            export_diff_requested: false,
            export_layer_requested: false,
            export_markups_csv_requested: false,
            export_markups_xlsx_requested: false,
            request_save_as: false,
            language: crate::i18n::Language::default(),
        }
    }

    /// Set a status message.
    pub fn set_status(&mut self, message: impl Into<String>) {
        self.status_message = message.into();
    }
}

/// Current tool mode for viewport interaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ToolMode {
    /// Pan/scroll the viewport
    #[default]
    Pan,
    /// Zoom tool
    Zoom,
    /// Measure distance
    Measure,
    /// Select region
    Select,
    /// Drawing with an active tool
    Drawing,
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::RgbaImage;

    fn create_test_buffer() -> RasterBuffer {
        RasterBuffer::new(RgbaImage::new(100, 100), 300)
    }

    #[test]
    fn test_session_add_layer() {
        let mut session = SessionState::new();

        let id = session
            .add_layer(
                "test.png".to_string(),
                PathBuf::from("/test/test.png"),
                create_test_buffer(),
            )
            .unwrap();

        assert_eq!(session.layer_count(), 1);
        assert!(session.get_layer(id).is_some());
        assert!(session.get_layer(id).unwrap().is_reference);
    }

    #[test]
    fn test_session_remove_layer() {
        let mut session = SessionState::new();

        let id = session
            .add_layer(
                "test.png".to_string(),
                PathBuf::from("/test"),
                create_test_buffer(),
            )
            .unwrap();

        assert!(session.remove_layer(id));
        assert_eq!(session.layer_count(), 0);
        assert!(!session.remove_layer(id)); // Already removed
    }

    #[test]
    fn test_session_max_layers() {
        let mut session = SessionState::new();

        for i in 0..MAX_LAYERS {
            session
                .add_layer(
                    format!("layer{}.png", i),
                    PathBuf::from("/test"),
                    create_test_buffer(),
                )
                .unwrap();
        }

        let result = session.add_layer(
            "overflow.png".to_string(),
            PathBuf::from("/test"),
            create_test_buffer(),
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_session_reference_change() {
        let mut session = SessionState::new();

        let id1 = session
            .add_layer(
                "layer1.png".to_string(),
                PathBuf::from("/test"),
                create_test_buffer(),
            )
            .unwrap();
        let id2 = session
            .add_layer(
                "layer2.png".to_string(),
                PathBuf::from("/test"),
                create_test_buffer(),
            )
            .unwrap();

        assert!(session.get_layer(id1).unwrap().is_reference);
        assert!(!session.get_layer(id2).unwrap().is_reference);

        session.set_reference(id2);

        assert!(!session.get_layer(id1).unwrap().is_reference);
        assert!(session.get_layer(id2).unwrap().is_reference);
    }

    #[test]
    fn test_session_can_compare() {
        let mut session = SessionState::new();

        assert!(!session.can_compare());

        session
            .add_layer(
                "layer1.png".to_string(),
                PathBuf::from("/test"),
                create_test_buffer(),
            )
            .unwrap();
        assert!(!session.can_compare());

        session
            .add_layer(
                "layer2.png".to_string(),
                PathBuf::from("/test"),
                create_test_buffer(),
            )
            .unwrap();
        assert!(session.can_compare());
    }

    #[test]
    fn test_ui_state_defaults() {
        let ui = UIState::new();
        assert_eq!(ui.tool_mode, ToolMode::Pan);
    }
}
