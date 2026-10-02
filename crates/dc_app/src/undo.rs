// =============================================================================
// dc_app/undo - Undo/Redo System
// =============================================================================
// Command-pattern based undo/redo with a max depth of 50 steps.
//
// Each undoable operation pushes a `UndoCommand` onto the undo stack.
// Executing undo pops the command and applies its inverse, pushing the
// forward version onto the redo stack. Any new command clears the redo stack.
//
// ## Design Decisions
//
// - **Command pattern** over full-state snapshots — layers contain heavy
//   pixel buffers (100s of MB each), so we store only the minimal delta.
// - For layer add/remove, we store the full `Layer` (including pixel data)
//   in memory. On serialization to session files we strip pixel buffers
//   and store only source_path + metadata; reloading from disk on undo.
// - Viewport changes (pan, zoom) are NOT tracked — they are navigation,
//   not document mutations.
// - Annotation selection changes are NOT tracked — they are ephemeral UI.
// =============================================================================

use dc_core::{Annotation, DiffConfig, Layer, LayerColor, LayerId, LayerPage};
use serde::{Deserialize, Serialize};

/// Maximum number of undo steps retained.
pub const MAX_UNDO_DEPTH: usize = 50;

/// A single undoable command.
///
/// Each variant stores all data needed to reverse the operation (undo)
/// and to re-apply it (redo). The naming convention uses the *forward*
/// direction — i.e. `AddLayer` means "the user added a layer".
#[derive(Debug, Clone)]
pub enum UndoCommand {
    // =========================================================================
    // Layer Operations
    // =========================================================================
    /// A layer was added to the session.
    AddLayer {
        layer_id: LayerId,
        /// Index in the layers vec where it was inserted.
        index: usize,
        /// Full snapshot of the layer (including pixel data in memory).
        layer: Box<Layer>,
    },

    /// A layer was removed from the session.
    RemoveLayer {
        layer_id: LayerId,
        /// Index from which it was removed.
        index: usize,
        /// Full snapshot of the removed layer.
        layer: Box<Layer>,
        /// Was this layer the reference at the time of removal?
        was_reference: bool,
        /// The old selected_layer value (so we can restore it).
        old_selected: Option<LayerId>,
    },

    /// A layer was moved from one index to another.
    MoveLayer {
        layer_id: LayerId,
        from_index: usize,
        to_index: usize,
    },

    /// Layer name changed.
    RenameLayer {
        layer_id: LayerId,
        old_name: String,
        new_name: String,
    },

    /// Layer visibility toggled.
    SetLayerVisibility {
        layer_id: LayerId,
        old_visible: bool,
        new_visible: bool,
    },

    /// Layer opacity changed.
    SetLayerOpacity {
        layer_id: LayerId,
        old_opacity: f32,
        new_opacity: f32,
    },

    /// Layer blend color changed.
    SetLayerBlendColor {
        layer_id: LayerId,
        old_color: LayerColor,
        new_color: LayerColor,
    },

    /// Layer manual offset changed.
    SetLayerOffset {
        layer_id: LayerId,
        old_x: f32,
        old_y: f32,
        new_x: f32,
        new_y: f32,
    },

    /// Reference layer changed.
    SetReference {
        old_reference_id: Option<LayerId>,
        new_reference_id: LayerId,
    },

    /// Another page was shown. Recorded so that undoing page-specific
    /// changes (markups, offsets) always happens on their own page.
    SetPage { old_page: usize, new_page: usize },

    // =========================================================================
    // Annotation Operations
    // =========================================================================
    /// An annotation was created.
    AddAnnotation {
        layer_id: LayerId,
        annotation: Box<Annotation>,
    },

    /// An annotation was deleted.
    RemoveAnnotation {
        layer_id: LayerId,
        annotation: Box<Annotation>,
        /// Index in the layer's annotations vec.
        index: usize,
    },

    /// An annotation was modified (style, geometry, text, custom properties).
    ModifyAnnotation {
        layer_id: LayerId,
        old_annotation: Box<Annotation>,
        new_annotation: Box<Annotation>,
    },

    // =========================================================================
    // Diff Configuration
    // =========================================================================
    /// Diff configuration changed.
    SetDiffConfig {
        old_config: DiffConfig,
        new_config: DiffConfig,
    },

    // =========================================================================
    // Custom Columns
    // =========================================================================
    /// A custom column was added.
    AddCustomColumn {
        index: usize,
        column: crate::state::CustomColumn,
    },

    /// A custom column was removed.
    RemoveCustomColumn {
        index: usize,
        column: crate::state::CustomColumn,
    },

    // =========================================================================
    // Count Counter
    // =========================================================================
    /// Count counter changed (usually incremented on Count annotation creation).
    SetCountCounter { old_value: u32, new_value: u32 },

    // =========================================================================
    // Compound
    // =========================================================================
    /// Multiple commands grouped as a single undoable step.
    Compound(Vec<UndoCommand>),
}

impl UndoCommand {
    /// Human-readable description of this command (for UI display).
    pub fn description(&self) -> String {
        match self {
            Self::AddLayer { .. } => "Add Layer".to_string(),
            Self::RemoveLayer { .. } => "Remove Layer".to_string(),
            Self::MoveLayer { .. } => "Move Layer".to_string(),
            Self::RenameLayer { new_name, .. } => format!("Rename Layer to \"{}\"", new_name),
            Self::SetLayerVisibility { new_visible, .. } => {
                if *new_visible {
                    "Show Layer".to_string()
                } else {
                    "Hide Layer".to_string()
                }
            }
            Self::SetLayerOpacity { .. } => "Change Layer Opacity".to_string(),
            Self::SetLayerBlendColor { .. } => "Change Layer Color".to_string(),
            Self::SetLayerOffset { .. } => "Change Layer Offset".to_string(),
            Self::SetReference { .. } => "Set Reference Layer".to_string(),
            Self::SetPage { new_page, .. } => format!("Show Page {}", new_page + 1),
            Self::AddAnnotation { .. } => "Add Annotation".to_string(),
            Self::RemoveAnnotation { .. } => "Delete Annotation".to_string(),
            Self::ModifyAnnotation { .. } => "Modify Annotation".to_string(),
            Self::SetDiffConfig { .. } => "Change Diff Settings".to_string(),
            Self::AddCustomColumn { column, .. } => {
                format!("Add Column \"{}\"", column.name)
            }
            Self::RemoveCustomColumn { column, .. } => {
                format!("Remove Column \"{}\"", column.name)
            }
            Self::SetCountCounter { .. } => "Change Count".to_string(),
            Self::Compound(cmds) => {
                if let Some(first) = cmds.first() {
                    first.description()
                } else {
                    "Compound".to_string()
                }
            }
        }
    }
}

/// The undo/redo stack.
///
/// Maintains two stacks:
/// - `undo_stack`: commands that can be undone (most recent at the back).
/// - `redo_stack`: commands that were undone and can be re-applied.
///
/// Pushing a new command always clears the redo stack (branching history
/// is not supported).
#[derive(Debug, Clone)]
pub struct UndoStack {
    undo_stack: Vec<UndoCommand>,
    redo_stack: Vec<UndoCommand>,
    max_depth: usize,
}

impl Default for UndoStack {
    fn default() -> Self {
        Self::new(MAX_UNDO_DEPTH)
    }
}

impl UndoStack {
    /// Create a new undo stack with the given depth limit.
    pub fn new(max_depth: usize) -> Self {
        Self {
            undo_stack: Vec::with_capacity(max_depth),
            redo_stack: Vec::new(),
            max_depth,
        }
    }

    /// Push a new command onto the undo stack.
    /// Clears the redo stack (new branch of history).
    /// Trims the oldest entry if exceeding max depth.
    pub fn push(&mut self, command: UndoCommand) {
        self.redo_stack.clear();
        self.undo_stack.push(command);
        if self.undo_stack.len() > self.max_depth {
            self.undo_stack.remove(0);
        }
    }

    /// Pop the most recent command for undo.
    /// Returns `None` if the undo stack is empty.
    pub fn pop_undo(&mut self) -> Option<UndoCommand> {
        self.undo_stack.pop()
    }

    /// Push a command onto the redo stack (called after undo).
    pub fn push_redo(&mut self, command: UndoCommand) {
        self.redo_stack.push(command);
    }

    /// Pop the most recent command for redo.
    /// Returns `None` if the redo stack is empty.
    pub fn pop_redo(&mut self) -> Option<UndoCommand> {
        self.redo_stack.pop()
    }

    /// Push onto the undo stack **without** clearing the redo stack.
    /// Used when re-applying a redo command — the command goes back
    /// onto undo but existing redo entries must survive.
    pub fn push_for_redo(&mut self, command: UndoCommand) {
        self.undo_stack.push(command);
        if self.undo_stack.len() > self.max_depth {
            self.undo_stack.remove(0);
        }
    }

    /// Whether there are commands to undo.
    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    /// Whether there are commands to redo.
    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    /// Number of commands on the undo stack.
    pub fn undo_count(&self) -> usize {
        self.undo_stack.len()
    }

    /// Number of commands on the redo stack.
    pub fn redo_count(&self) -> usize {
        self.redo_stack.len()
    }

    /// Get a description of the next undo command.
    pub fn undo_description(&self) -> Option<String> {
        self.undo_stack.last().map(|c| c.description())
    }

    /// Get a description of the next redo command.
    pub fn redo_description(&self) -> Option<String> {
        self.redo_stack.last().map(|c| c.description())
    }

    /// Clear both stacks.
    pub fn clear(&mut self) {
        self.undo_stack.clear();
        self.redo_stack.clear();
    }

    /// Get max depth.
    pub fn max_depth(&self) -> usize {
        self.max_depth
    }

    /// Reference to the undo stack (for inspection/testing).
    pub fn undo_stack(&self) -> &[UndoCommand] {
        &self.undo_stack
    }

    /// Reference to the redo stack (for inspection/testing).
    pub fn redo_stack(&self) -> &[UndoCommand] {
        &self.redo_stack
    }

    /// Reconstruct an UndoStack from raw vectors (used during deserialization).
    pub fn from_parts(undo: Vec<UndoCommand>, redo: Vec<UndoCommand>, max_depth: usize) -> Self {
        Self {
            undo_stack: undo,
            redo_stack: redo,
            max_depth,
        }
    }
}

// =============================================================================
// Serializable Mirror Types (for session persistence)
// =============================================================================
// Layer contains RasterBuffer which is large and not serializable.
// We mirror UndoCommand with a serializable version that stores LayerSaveData
// instead of full Layer objects. On deserialization, images are reloaded from
// disk using the source_path.

/// Serializable replacement for a Layer in undo commands.
/// Stores only metadata + source path; pixel data is reloaded from disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayerSaveData {
    /// Layer ID.
    pub id: LayerId,
    /// Layer name.
    pub name: String,
    /// Path to the source image file.
    pub source_path: std::path::PathBuf,
    /// Homography matrix for alignment.
    pub homography_matrix: Option<[[f64; 3]; 3]>,
    /// Visibility.
    pub visible: bool,
    /// Opacity.
    pub opacity: f32,
    /// Blend color.
    pub blend_color: LayerColor,
    /// Whether this was the reference layer.
    pub is_reference: bool,
    /// Annotations.
    pub annotations: Vec<Annotation>,
    /// Offset X.
    pub offset_x: f32,
    /// Offset Y.
    pub offset_y: f32,
    /// Rasterization DPI for reproducible PDF loading.
    #[serde(default = "default_layer_dpi")]
    pub dpi: u32,
    /// Original PDF page.
    #[serde(default)]
    pub page_index: Option<usize>,
    /// Aligned output canvas.
    #[serde(default)]
    pub aligned_size: Option<(u32, u32)>,
    /// All pages of a multi-page document; empty for single pages.
    #[serde(default)]
    pub pages: Vec<PageSaveData>,
    /// Page shown when the snapshot was taken.
    #[serde(default)]
    pub active_page: usize,
}

/// Page-specific layer state without pixel data.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PageSaveData {
    /// Homography matrix for alignment.
    #[serde(default)]
    pub homography_matrix: Option<[[f64; 3]; 3]>,
    /// Markups on this page.
    #[serde(default)]
    pub annotations: Vec<Annotation>,
    /// Offset X.
    #[serde(default)]
    pub offset_x: f32,
    /// Offset Y.
    #[serde(default)]
    pub offset_y: f32,
    /// Aligned output canvas.
    #[serde(default)]
    pub aligned_size: Option<(u32, u32)>,
}

impl From<&LayerPage> for PageSaveData {
    fn from(page: &LayerPage) -> Self {
        Self {
            homography_matrix: page.homography_matrix,
            annotations: page.annotations.clone(),
            offset_x: page.offset_x,
            offset_y: page.offset_y,
            aligned_size: page.aligned.as_ref().map(|b| b.dimensions()),
        }
    }
}

/// The page whose state is stored in a layer's top-level save fields.
pub fn saved_page(layer: &Layer) -> LayerPage {
    layer.page(if layer.is_page_missing() {
        0
    } else {
        layer.active_page
    })
}

/// Save data for every page of a multi-page document.
pub fn save_pages(layer: &Layer) -> Vec<PageSaveData> {
    if layer.page_count() < 2 {
        return Vec::new();
    }
    (0..layer.page_count())
        .map(|page| PageSaveData::from(&layer.page(page)))
        .collect()
}

fn default_layer_dpi() -> u32 {
    dc_core::DEFAULT_RENDER_DPI
}

impl From<&Layer> for LayerSaveData {
    fn from(layer: &Layer) -> Self {
        let page = saved_page(layer);
        Self {
            id: layer.id,
            name: layer.name.clone(),
            source_path: layer.source_path.clone(),
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
            pages: save_pages(layer),
            active_page: layer.active_page,
        }
    }
}

/// Serializable mirror of [`UndoCommand`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum UndoCommandSave {
    /// Layer added.
    AddLayer {
        /// Layer ID.
        layer_id: LayerId,
        /// Index.
        index: usize,
        /// Serializable layer data.
        layer: LayerSaveData,
    },
    /// Layer removed.
    RemoveLayer {
        /// Layer ID.
        layer_id: LayerId,
        /// Index.
        index: usize,
        /// Serializable layer data.
        layer: LayerSaveData,
        /// Was this the reference layer?
        was_reference: bool,
        /// Previous selection.
        old_selected: Option<LayerId>,
    },
    /// Layer moved.
    MoveLayer {
        /// Layer ID.
        layer_id: LayerId,
        /// Source index.
        from_index: usize,
        /// Destination index.
        to_index: usize,
    },
    /// Layer renamed.
    RenameLayer {
        /// Layer ID.
        layer_id: LayerId,
        /// Old name.
        old_name: String,
        /// New name.
        new_name: String,
    },
    /// Layer visibility changed.
    SetLayerVisibility {
        /// Layer ID.
        layer_id: LayerId,
        /// Old visibility.
        old_visible: bool,
        /// New visibility.
        new_visible: bool,
    },
    /// Layer opacity changed.
    SetLayerOpacity {
        /// Layer ID.
        layer_id: LayerId,
        /// Old opacity.
        old_opacity: f32,
        /// New opacity.
        new_opacity: f32,
    },
    /// Blend color changed.
    SetLayerBlendColor {
        /// Layer ID.
        layer_id: LayerId,
        /// Old color.
        old_color: LayerColor,
        /// New color.
        new_color: LayerColor,
    },
    /// Offset changed.
    SetLayerOffset {
        /// Layer ID.
        layer_id: LayerId,
        /// Old X.
        old_x: f32,
        /// Old Y.
        old_y: f32,
        /// New X.
        new_x: f32,
        /// New Y.
        new_y: f32,
    },
    /// Reference changed.
    SetReference {
        /// Old reference.
        old_reference_id: Option<LayerId>,
        /// New reference.
        new_reference_id: LayerId,
    },
    /// Page changed.
    SetPage {
        /// Old page.
        old_page: usize,
        /// New page.
        new_page: usize,
    },
    /// Annotation added.
    AddAnnotation {
        /// Layer ID.
        layer_id: LayerId,
        /// The annotation.
        annotation: Box<Annotation>,
    },
    /// Annotation removed.
    RemoveAnnotation {
        /// Layer ID.
        layer_id: LayerId,
        /// The annotation.
        annotation: Box<Annotation>,
        /// Index.
        index: usize,
    },
    /// Annotation modified.
    ModifyAnnotation {
        /// Layer ID.
        layer_id: LayerId,
        /// Old annotation.
        old_annotation: Box<Annotation>,
        /// New annotation.
        new_annotation: Box<Annotation>,
    },
    /// Diff config changed.
    SetDiffConfig {
        /// Old config.
        old_config: DiffConfig,
        /// New config.
        new_config: DiffConfig,
    },
    /// Custom column added.
    AddCustomColumn {
        /// Index.
        index: usize,
        /// Column data.
        column: crate::state::CustomColumn,
    },
    /// Custom column removed.
    RemoveCustomColumn {
        /// Index.
        index: usize,
        /// Column data.
        column: crate::state::CustomColumn,
    },
    /// Count counter changed.
    SetCountCounter {
        /// Old value.
        old_value: u32,
        /// New value.
        new_value: u32,
    },
    /// Compound command.
    Compound(Vec<UndoCommandSave>),
}

impl UndoCommand {
    /// Convert to the serializable save representation.
    pub fn to_save(&self) -> UndoCommandSave {
        match self {
            Self::AddLayer {
                layer_id,
                index,
                layer,
            } => UndoCommandSave::AddLayer {
                layer_id: *layer_id,
                index: *index,
                layer: LayerSaveData::from(layer.as_ref()),
            },
            Self::RemoveLayer {
                layer_id,
                index,
                layer,
                was_reference,
                old_selected,
            } => UndoCommandSave::RemoveLayer {
                layer_id: *layer_id,
                index: *index,
                layer: LayerSaveData::from(layer.as_ref()),
                was_reference: *was_reference,
                old_selected: *old_selected,
            },
            Self::MoveLayer {
                layer_id,
                from_index,
                to_index,
            } => UndoCommandSave::MoveLayer {
                layer_id: *layer_id,
                from_index: *from_index,
                to_index: *to_index,
            },
            Self::RenameLayer {
                layer_id,
                old_name,
                new_name,
            } => UndoCommandSave::RenameLayer {
                layer_id: *layer_id,
                old_name: old_name.clone(),
                new_name: new_name.clone(),
            },
            Self::SetLayerVisibility {
                layer_id,
                old_visible,
                new_visible,
            } => UndoCommandSave::SetLayerVisibility {
                layer_id: *layer_id,
                old_visible: *old_visible,
                new_visible: *new_visible,
            },
            Self::SetLayerOpacity {
                layer_id,
                old_opacity,
                new_opacity,
            } => UndoCommandSave::SetLayerOpacity {
                layer_id: *layer_id,
                old_opacity: *old_opacity,
                new_opacity: *new_opacity,
            },
            Self::SetLayerBlendColor {
                layer_id,
                old_color,
                new_color,
            } => UndoCommandSave::SetLayerBlendColor {
                layer_id: *layer_id,
                old_color: *old_color,
                new_color: *new_color,
            },
            Self::SetLayerOffset {
                layer_id,
                old_x,
                old_y,
                new_x,
                new_y,
            } => UndoCommandSave::SetLayerOffset {
                layer_id: *layer_id,
                old_x: *old_x,
                old_y: *old_y,
                new_x: *new_x,
                new_y: *new_y,
            },
            Self::SetReference {
                old_reference_id,
                new_reference_id,
            } => UndoCommandSave::SetReference {
                old_reference_id: *old_reference_id,
                new_reference_id: *new_reference_id,
            },
            Self::SetPage { old_page, new_page } => UndoCommandSave::SetPage {
                old_page: *old_page,
                new_page: *new_page,
            },
            Self::AddAnnotation {
                layer_id,
                annotation,
            } => UndoCommandSave::AddAnnotation {
                layer_id: *layer_id,
                annotation: annotation.clone(),
            },
            Self::RemoveAnnotation {
                layer_id,
                annotation,
                index,
            } => UndoCommandSave::RemoveAnnotation {
                layer_id: *layer_id,
                annotation: annotation.clone(),
                index: *index,
            },
            Self::ModifyAnnotation {
                layer_id,
                old_annotation,
                new_annotation,
            } => UndoCommandSave::ModifyAnnotation {
                layer_id: *layer_id,
                old_annotation: old_annotation.clone(),
                new_annotation: new_annotation.clone(),
            },
            Self::SetDiffConfig {
                old_config,
                new_config,
            } => UndoCommandSave::SetDiffConfig {
                old_config: old_config.clone(),
                new_config: new_config.clone(),
            },
            Self::AddCustomColumn { index, column } => UndoCommandSave::AddCustomColumn {
                index: *index,
                column: column.clone(),
            },
            Self::RemoveCustomColumn { index, column } => UndoCommandSave::RemoveCustomColumn {
                index: *index,
                column: column.clone(),
            },
            Self::SetCountCounter {
                old_value,
                new_value,
            } => UndoCommandSave::SetCountCounter {
                old_value: *old_value,
                new_value: *new_value,
            },
            Self::Compound(cmds) => {
                UndoCommandSave::Compound(cmds.iter().map(|c| c.to_save()).collect())
            }
        }
    }
}

impl UndoCommandSave {
    /// Convert from save representation back to runtime command.
    /// `load_layer_fn` reconstructs a `Layer` from `LayerSaveData` by reloading
    /// pixel data from disk.
    pub fn to_command(
        self,
        load_layer_fn: &dyn Fn(&LayerSaveData) -> Option<Layer>,
    ) -> Option<UndoCommand> {
        match self {
            Self::AddLayer {
                layer_id,
                index,
                layer,
            } => {
                let loaded = load_layer_fn(&layer)?;
                Some(UndoCommand::AddLayer {
                    layer_id,
                    index,
                    layer: Box::new(loaded),
                })
            }
            Self::RemoveLayer {
                layer_id,
                index,
                layer,
                was_reference,
                old_selected,
            } => {
                let loaded = load_layer_fn(&layer)?;
                Some(UndoCommand::RemoveLayer {
                    layer_id,
                    index,
                    layer: Box::new(loaded),
                    was_reference,
                    old_selected,
                })
            }
            Self::MoveLayer {
                layer_id,
                from_index,
                to_index,
            } => Some(UndoCommand::MoveLayer {
                layer_id,
                from_index,
                to_index,
            }),
            Self::RenameLayer {
                layer_id,
                old_name,
                new_name,
            } => Some(UndoCommand::RenameLayer {
                layer_id,
                old_name,
                new_name,
            }),
            Self::SetLayerVisibility {
                layer_id,
                old_visible,
                new_visible,
            } => Some(UndoCommand::SetLayerVisibility {
                layer_id,
                old_visible,
                new_visible,
            }),
            Self::SetLayerOpacity {
                layer_id,
                old_opacity,
                new_opacity,
            } => Some(UndoCommand::SetLayerOpacity {
                layer_id,
                old_opacity,
                new_opacity,
            }),
            Self::SetLayerBlendColor {
                layer_id,
                old_color,
                new_color,
            } => Some(UndoCommand::SetLayerBlendColor {
                layer_id,
                old_color,
                new_color,
            }),
            Self::SetLayerOffset {
                layer_id,
                old_x,
                old_y,
                new_x,
                new_y,
            } => Some(UndoCommand::SetLayerOffset {
                layer_id,
                old_x,
                old_y,
                new_x,
                new_y,
            }),
            Self::SetReference {
                old_reference_id,
                new_reference_id,
            } => Some(UndoCommand::SetReference {
                old_reference_id,
                new_reference_id,
            }),
            Self::SetPage { old_page, new_page } => {
                Some(UndoCommand::SetPage { old_page, new_page })
            }
            Self::AddAnnotation {
                layer_id,
                annotation,
            } => Some(UndoCommand::AddAnnotation {
                layer_id,
                annotation,
            }),
            Self::RemoveAnnotation {
                layer_id,
                annotation,
                index,
            } => Some(UndoCommand::RemoveAnnotation {
                layer_id,
                annotation,
                index,
            }),
            Self::ModifyAnnotation {
                layer_id,
                old_annotation,
                new_annotation,
            } => Some(UndoCommand::ModifyAnnotation {
                layer_id,
                old_annotation,
                new_annotation,
            }),
            Self::SetDiffConfig {
                old_config,
                new_config,
            } => Some(UndoCommand::SetDiffConfig {
                old_config,
                new_config,
            }),
            Self::AddCustomColumn { index, column } => {
                Some(UndoCommand::AddCustomColumn { index, column })
            }
            Self::RemoveCustomColumn { index, column } => {
                Some(UndoCommand::RemoveCustomColumn { index, column })
            }
            Self::SetCountCounter {
                old_value,
                new_value,
            } => Some(UndoCommand::SetCountCounter {
                old_value,
                new_value,
            }),
            Self::Compound(cmds) => {
                let converted: Vec<UndoCommand> = cmds
                    .into_iter()
                    .filter_map(|c| c.to_command(load_layer_fn))
                    .collect();
                if converted.is_empty() {
                    None
                } else {
                    Some(UndoCommand::Compound(converted))
                }
            }
        }
    }
}

/// Serializable form of the entire undo stack.
#[derive(Debug, Serialize, Deserialize)]
pub struct UndoStackSave {
    /// Undo commands (oldest first).
    pub undo: Vec<UndoCommandSave>,
    /// Redo commands (oldest first).
    pub redo: Vec<UndoCommandSave>,
    /// Max depth setting.
    pub max_depth: usize,
}

impl UndoStack {
    /// Convert the undo stack to a serializable form.
    pub fn to_save(&self) -> UndoStackSave {
        UndoStackSave {
            undo: self.undo_stack.iter().map(|c| c.to_save()).collect(),
            redo: self.redo_stack.iter().map(|c| c.to_save()).collect(),
            max_depth: self.max_depth,
        }
    }

    /// Reconstruct from serialized save data.
    /// Layer commands that fail to reload are silently dropped.
    pub fn from_save(
        save: UndoStackSave,
        load_layer_fn: &dyn Fn(&LayerSaveData) -> Option<Layer>,
    ) -> Self {
        let undo: Vec<UndoCommand> = save
            .undo
            .into_iter()
            .filter_map(|c| c.to_command(load_layer_fn))
            .collect();
        let redo: Vec<UndoCommand> = save
            .redo
            .into_iter()
            .filter_map(|c| c.to_command(load_layer_fn))
            .collect();
        Self::from_parts(undo, redo, save.max_depth)
    }
}

// =============================================================================
// Apply / Reverse Logic
// =============================================================================

use crate::state::SessionState;

impl UndoCommand {
    /// Apply the *reverse* of this command to the session (i.e. undo it).
    /// Returns the forward command that can be pushed onto the redo stack.
    pub fn undo(&self, session: &mut SessionState) -> UndoCommand {
        match self {
            // -----------------------------------------------------------------
            // Layer operations
            // -----------------------------------------------------------------
            Self::AddLayer {
                layer_id,
                index,
                layer,
            } => {
                // Undo "add" = remove the layer
                let actual_index = session
                    .layers
                    .iter()
                    .position(|l| l.id == *layer_id)
                    .unwrap_or(*index);
                let removed = session.layers.remove(actual_index);
                let old_selected = session.selected_layer;
                session.diff_result = None;
                session.is_dirty = true;
                // If this was the selected layer, deselect
                if session.selected_layer == Some(*layer_id) {
                    session.selected_layer = session.layers.first().map(|l| l.id);
                }
                // Return RemoveLayer — redo will call .undo() on it = re-insert
                Self::RemoveLayer {
                    layer_id: *layer_id,
                    index: actual_index,
                    layer: Box::new(removed),
                    was_reference: layer.is_reference,
                    old_selected: Some(old_selected.unwrap_or(*layer_id)),
                }
            }

            Self::RemoveLayer {
                layer_id,
                index,
                layer,
                was_reference,
                old_selected,
            } => {
                // Undo "remove" = re-insert the layer
                let insert_idx = (*index).min(session.layers.len());
                session.layers.insert(insert_idx, *layer.clone());
                // Restore reference status if it was the reference
                if *was_reference {
                    // Clear reference from any other layer first
                    for l in &mut session.layers {
                        if l.id != *layer_id {
                            l.is_reference = false;
                        }
                    }
                    if let Some(l) = session.layers.iter_mut().find(|l| l.id == *layer_id) {
                        l.is_reference = true;
                    }
                }
                session.selected_layer = *old_selected;
                session.diff_result = None;
                session.is_dirty = true;
                // Return AddLayer — redo will call .undo() on it = remove again
                Self::AddLayer {
                    layer_id: *layer_id,
                    index: insert_idx,
                    layer: layer.clone(),
                }
            }

            Self::MoveLayer {
                layer_id,
                from_index,
                to_index,
            } => {
                // Undo "move from→to" = move to→from
                if let Some(pos) = session.layers.iter().position(|l| l.id == *layer_id) {
                    let layer = session.layers.remove(pos);
                    let restore_idx = (*from_index).min(session.layers.len());
                    session.layers.insert(restore_idx, layer);
                    session.diff_result = None;
                    session.is_dirty = true;
                }
                Self::MoveLayer {
                    layer_id: *layer_id,
                    from_index: *to_index,
                    to_index: *from_index,
                }
            }

            Self::RenameLayer {
                layer_id,
                old_name,
                new_name,
            } => {
                if let Some(layer) = session.layers.iter_mut().find(|l| l.id == *layer_id) {
                    layer.name = old_name.clone();
                }
                session.is_dirty = true;
                Self::RenameLayer {
                    layer_id: *layer_id,
                    old_name: new_name.clone(),
                    new_name: old_name.clone(),
                }
            }

            Self::SetLayerVisibility {
                layer_id,
                old_visible,
                new_visible,
            } => {
                if let Some(layer) = session.layers.iter_mut().find(|l| l.id == *layer_id) {
                    layer.visible = *old_visible;
                }
                session.diff_result = None;
                session.is_dirty = true;
                Self::SetLayerVisibility {
                    layer_id: *layer_id,
                    old_visible: *new_visible,
                    new_visible: *old_visible,
                }
            }

            Self::SetLayerOpacity {
                layer_id,
                old_opacity,
                new_opacity,
            } => {
                if let Some(layer) = session.layers.iter_mut().find(|l| l.id == *layer_id) {
                    layer.opacity = *old_opacity;
                }
                session.is_dirty = true;
                Self::SetLayerOpacity {
                    layer_id: *layer_id,
                    old_opacity: *new_opacity,
                    new_opacity: *old_opacity,
                }
            }

            Self::SetLayerBlendColor {
                layer_id,
                old_color,
                new_color,
            } => {
                if let Some(layer) = session.layers.iter_mut().find(|l| l.id == *layer_id) {
                    layer.blend_color = *old_color;
                }
                session.diff_result = None;
                session.is_dirty = true;
                Self::SetLayerBlendColor {
                    layer_id: *layer_id,
                    old_color: *new_color,
                    new_color: *old_color,
                }
            }

            Self::SetLayerOffset {
                layer_id,
                old_x,
                old_y,
                new_x,
                new_y,
            } => {
                if let Some(layer) = session.layers.iter_mut().find(|l| l.id == *layer_id) {
                    layer.offset_x = *old_x;
                    layer.offset_y = *old_y;
                }
                session.diff_result = None;
                session.is_dirty = true;
                Self::SetLayerOffset {
                    layer_id: *layer_id,
                    old_x: *new_x,
                    old_y: *new_y,
                    new_x: *old_x,
                    new_y: *old_y,
                }
            }

            Self::SetReference {
                old_reference_id,
                new_reference_id,
            } => {
                // Undo: restore old reference
                for layer in &mut session.layers {
                    layer.is_reference = false;
                }
                if let Some(old_id) = old_reference_id {
                    if let Some(layer) = session.layers.iter_mut().find(|l| l.id == *old_id) {
                        layer.is_reference = true;
                    }
                }
                session.diff_result = None;
                session.is_dirty = true;
                Self::SetReference {
                    old_reference_id: Some(*new_reference_id),
                    new_reference_id: old_reference_id.unwrap_or(*new_reference_id),
                }
            }

            Self::SetPage { old_page, new_page } => {
                session.set_page(*old_page);
                Self::SetPage {
                    old_page: *new_page,
                    new_page: *old_page,
                }
            }

            // -----------------------------------------------------------------
            // Annotation operations
            // -----------------------------------------------------------------
            Self::AddAnnotation {
                layer_id,
                annotation,
            } => {
                // Undo "add" = remove
                let mut removed_index = 0;
                if let Some(layer) = session.layers.iter_mut().find(|l| l.id == *layer_id) {
                    if let Some(pos) = layer.annotations.iter().position(|a| a.id == annotation.id)
                    {
                        removed_index = pos;
                        layer.annotations.remove(pos);
                    }
                }
                session.is_dirty = true;
                // Return RemoveAnnotation so redo re-inserts
                Self::RemoveAnnotation {
                    layer_id: *layer_id,
                    annotation: annotation.clone(),
                    index: removed_index,
                }
            }

            Self::RemoveAnnotation {
                layer_id,
                annotation,
                index,
            } => {
                // Undo "remove" = re-insert
                if let Some(layer) = session.layers.iter_mut().find(|l| l.id == *layer_id) {
                    let insert_idx = (*index).min(layer.annotations.len());
                    layer.annotations.insert(insert_idx, *annotation.clone());
                }
                session.is_dirty = true;
                // Return AddAnnotation so redo removes again
                Self::AddAnnotation {
                    layer_id: *layer_id,
                    annotation: annotation.clone(),
                }
            }

            Self::ModifyAnnotation {
                layer_id,
                old_annotation,
                new_annotation,
            } => {
                // Undo = replace current with old
                if let Some(layer) = session.layers.iter_mut().find(|l| l.id == *layer_id) {
                    if let Some(annot) = layer
                        .annotations
                        .iter_mut()
                        .find(|a| a.id == old_annotation.id)
                    {
                        *annot = *old_annotation.clone();
                    }
                }
                session.is_dirty = true;
                Self::ModifyAnnotation {
                    layer_id: *layer_id,
                    old_annotation: new_annotation.clone(),
                    new_annotation: old_annotation.clone(),
                }
            }

            // -----------------------------------------------------------------
            // Diff config
            // -----------------------------------------------------------------
            Self::SetDiffConfig {
                old_config,
                new_config,
            } => {
                session.diff_config = old_config.clone();
                session.diff_result = None;
                session.is_dirty = true;
                Self::SetDiffConfig {
                    old_config: new_config.clone(),
                    new_config: old_config.clone(),
                }
            }

            // -----------------------------------------------------------------
            // Custom columns
            // -----------------------------------------------------------------
            Self::AddCustomColumn { index, column } => {
                // Undo "add" = remove
                if *index < session.custom_columns.len() {
                    session.custom_columns.remove(*index);
                } else if !session.custom_columns.is_empty() {
                    session.custom_columns.pop();
                }
                session.is_dirty = true;
                // Return RemoveCustomColumn so redo re-inserts
                Self::RemoveCustomColumn {
                    index: *index,
                    column: column.clone(),
                }
            }

            Self::RemoveCustomColumn { index, column } => {
                // Undo "remove" = re-insert
                let insert_idx = (*index).min(session.custom_columns.len());
                session.custom_columns.insert(insert_idx, column.clone());
                session.is_dirty = true;
                // Return AddCustomColumn so redo removes again
                Self::AddCustomColumn {
                    index: *index,
                    column: column.clone(),
                }
            }

            // -----------------------------------------------------------------
            // Count counter
            // -----------------------------------------------------------------
            Self::SetCountCounter {
                old_value,
                new_value,
            } => {
                session.count_counter = *old_value;
                Self::SetCountCounter {
                    old_value: *new_value,
                    new_value: *old_value,
                }
            }

            // -----------------------------------------------------------------
            // Compound
            // -----------------------------------------------------------------
            Self::Compound(cmds) => {
                // Undo in reverse order
                let reversed: Vec<UndoCommand> =
                    cmds.iter().rev().map(|c| c.undo(session)).collect();
                // The redo command re-applies them in the original order
                Self::Compound(reversed.into_iter().rev().collect())
            }
        }
    }

    /// Apply this command *forward* (i.e. redo it).
    /// This is identical to `undo()` conceptually but for redo we apply
    /// the forward direction. Since our undo() returns the symmetric command,
    /// redo is simply calling undo() on the reversed command.
    pub fn redo(&self, session: &mut SessionState) -> UndoCommand {
        // Redo is symmetric: undo the "reverse" command = apply forward
        self.undo(session)
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use dc_core::{LayerId, RasterBuffer};
    use image::RgbaImage;
    use std::path::PathBuf;

    fn test_buffer() -> RasterBuffer {
        RasterBuffer::new(RgbaImage::new(10, 10), 72)
    }

    fn test_layer(id: u32, name: &str) -> Layer {
        Layer::new(
            LayerId::new(id),
            name.to_string(),
            PathBuf::from(format!("/test/{}.png", name)),
            test_buffer(),
            id == 0,
        )
    }

    fn test_session_with_layers() -> SessionState {
        let mut session = SessionState::new();
        let buf1 = test_buffer();
        let buf2 = test_buffer();
        session
            .add_layer("layer0.png".into(), "/test/layer0.png".into(), buf1)
            .unwrap();
        session
            .add_layer("layer1.png".into(), "/test/layer1.png".into(), buf2)
            .unwrap();
        session
    }

    // =========================================================================
    // UndoStack basic operations
    // =========================================================================

    #[test]
    fn test_undo_stack_push_and_pop() {
        let mut stack = UndoStack::new(50);
        assert!(!stack.can_undo());
        assert!(!stack.can_redo());

        stack.push(UndoCommand::SetLayerVisibility {
            layer_id: LayerId::new(0),
            old_visible: true,
            new_visible: false,
        });

        assert!(stack.can_undo());
        assert_eq!(stack.undo_count(), 1);

        let cmd = stack.pop_undo().unwrap();
        assert!(!stack.can_undo());
        assert_eq!(stack.undo_count(), 0);

        stack.push_redo(cmd);
        assert!(stack.can_redo());
        assert_eq!(stack.redo_count(), 1);
    }

    #[test]
    fn test_undo_stack_max_depth() {
        let mut stack = UndoStack::new(3);

        for i in 0..5 {
            stack.push(UndoCommand::SetLayerOpacity {
                layer_id: LayerId::new(0),
                old_opacity: i as f32,
                new_opacity: (i + 1) as f32,
            });
        }

        // Should only keep the last 3
        assert_eq!(stack.undo_count(), 3);
    }

    #[test]
    fn test_push_clears_redo() {
        let mut stack = UndoStack::new(50);

        stack.push(UndoCommand::SetLayerVisibility {
            layer_id: LayerId::new(0),
            old_visible: true,
            new_visible: false,
        });
        let cmd = stack.pop_undo().unwrap();
        stack.push_redo(cmd);
        assert!(stack.can_redo());

        // New push should clear redo
        stack.push(UndoCommand::SetLayerOpacity {
            layer_id: LayerId::new(0),
            old_opacity: 1.0,
            new_opacity: 0.5,
        });
        assert!(!stack.can_redo());
    }

    #[test]
    fn test_undo_stack_clear() {
        let mut stack = UndoStack::new(50);
        stack.push(UndoCommand::SetLayerVisibility {
            layer_id: LayerId::new(0),
            old_visible: true,
            new_visible: false,
        });
        let cmd = stack.pop_undo().unwrap();
        stack.push_redo(cmd);

        stack.clear();
        assert!(!stack.can_undo());
        assert!(!stack.can_redo());
    }

    #[test]
    fn test_undo_stack_descriptions() {
        let mut stack = UndoStack::new(50);
        assert!(stack.undo_description().is_none());
        assert!(stack.redo_description().is_none());

        stack.push(UndoCommand::AddAnnotation {
            layer_id: LayerId::new(1),
            annotation: Box::new(Annotation::new(
                LayerId::new(1),
                dc_core::ToolStyle::default(),
                dc_core::AnnotationData::Rectangle {
                    start: dc_core::Point::new(0.0, 0.0),
                    end: dc_core::Point::new(10.0, 10.0),
                },
            )),
        });

        assert_eq!(stack.undo_description(), Some("Add Annotation".to_string()));
    }

    // =========================================================================
    // Individual command undo/redo round-trips
    // =========================================================================

    #[test]
    fn test_undo_redo_set_layer_visibility() {
        let mut session = test_session_with_layers();
        let layer_id = session.layers[0].id;

        // Layer starts visible
        assert!(session.layers[0].visible);

        // Forward: hide it
        let cmd = UndoCommand::SetLayerVisibility {
            layer_id,
            old_visible: true,
            new_visible: false,
        };

        // Apply forward by calling undo on the reverse
        // Actually, we first "apply forward" by directly setting the state,
        // then push the command. The undo() method reverses it.
        session.layers[0].visible = false;

        // Undo
        let redo_cmd = cmd.undo(&mut session);
        assert!(session.layers[0].visible); // restored

        // Redo
        let _undo_cmd = redo_cmd.undo(&mut session);
        assert!(!session.layers[0].visible); // hidden again
    }

    #[test]
    fn test_undo_redo_set_layer_opacity() {
        let mut session = test_session_with_layers();
        let layer_id = session.layers[0].id;

        session.layers[0].opacity = 0.3;
        let cmd = UndoCommand::SetLayerOpacity {
            layer_id,
            old_opacity: 1.0,
            new_opacity: 0.3,
        };

        let redo_cmd = cmd.undo(&mut session);
        assert!((session.layers[0].opacity - 1.0).abs() < 0.001);

        let _undo_cmd = redo_cmd.undo(&mut session);
        assert!((session.layers[0].opacity - 0.3).abs() < 0.001);
    }

    #[test]
    fn test_undo_redo_set_layer_blend_color() {
        let mut session = test_session_with_layers();
        let layer_id = session.layers[0].id;
        let old_color = session.layers[0].blend_color;
        let new_color = LayerColor::new(0, 128, 255);

        session.layers[0].blend_color = new_color;
        let cmd = UndoCommand::SetLayerBlendColor {
            layer_id,
            old_color,
            new_color,
        };

        let redo_cmd = cmd.undo(&mut session);
        assert_eq!(session.layers[0].blend_color, old_color);

        let _undo_cmd = redo_cmd.undo(&mut session);
        assert_eq!(session.layers[0].blend_color, new_color);
    }

    #[test]
    fn test_undo_redo_set_layer_offset() {
        let mut session = test_session_with_layers();
        let layer_id = session.layers[1].id;

        session.layers[1].offset_x = 10.0;
        session.layers[1].offset_y = -5.0;

        let cmd = UndoCommand::SetLayerOffset {
            layer_id,
            old_x: 0.0,
            old_y: 0.0,
            new_x: 10.0,
            new_y: -5.0,
        };

        let redo_cmd = cmd.undo(&mut session);
        assert!((session.layers[1].offset_x).abs() < 0.001);
        assert!((session.layers[1].offset_y).abs() < 0.001);

        let _undo_cmd = redo_cmd.undo(&mut session);
        assert!((session.layers[1].offset_x - 10.0).abs() < 0.001);
        assert!((session.layers[1].offset_y + 5.0).abs() < 0.001);
    }

    #[test]
    fn test_undo_redo_rename_layer() {
        let mut session = test_session_with_layers();
        let layer_id = session.layers[0].id;

        session.layers[0].name = "Renamed".to_string();
        let cmd = UndoCommand::RenameLayer {
            layer_id,
            old_name: "layer0.png".to_string(),
            new_name: "Renamed".to_string(),
        };

        let redo_cmd = cmd.undo(&mut session);
        assert_eq!(session.layers[0].name, "layer0.png");

        let _undo_cmd = redo_cmd.undo(&mut session);
        assert_eq!(session.layers[0].name, "Renamed");
    }

    #[test]
    fn test_undo_redo_move_layer() {
        let mut session = test_session_with_layers();
        // Add a third layer
        let buf = test_buffer();
        session
            .add_layer("layer2.png".into(), "/test/layer2.png".into(), buf)
            .unwrap();

        let id0 = session.layers[0].id;
        let id1 = session.layers[1].id;
        let id2 = session.layers[2].id;

        // Move layer at index 2 to index 0
        let layer = session.layers.remove(2);
        session.layers.insert(0, layer);

        let cmd = UndoCommand::MoveLayer {
            layer_id: id2,
            from_index: 2,
            to_index: 0,
        };

        // Undo: should restore original order
        let redo_cmd = cmd.undo(&mut session);
        assert_eq!(session.layers[0].id, id0);
        assert_eq!(session.layers[1].id, id1);
        assert_eq!(session.layers[2].id, id2);

        // Redo: move again
        let _undo_cmd = redo_cmd.undo(&mut session);
        assert_eq!(session.layers[0].id, id2);
    }

    #[test]
    fn test_undo_redo_remove_layer() {
        let mut session = test_session_with_layers();
        let layer_id = session.layers[1].id;
        let layer_snapshot = session.layers[1].clone();
        let old_selected = session.selected_layer;

        session.layers.remove(1);

        let cmd = UndoCommand::RemoveLayer {
            layer_id,
            index: 1,
            layer: Box::new(layer_snapshot),
            was_reference: false,
            old_selected,
        };

        assert_eq!(session.layers.len(), 1);

        // Undo: re-insert
        let redo_cmd = cmd.undo(&mut session);
        assert_eq!(session.layers.len(), 2);
        assert_eq!(session.layers[1].id, layer_id);

        // Redo: remove again
        let _undo_cmd = redo_cmd.undo(&mut session);
        assert_eq!(session.layers.len(), 1);
    }

    #[test]
    fn test_undo_redo_add_layer() {
        let mut session = SessionState::new();
        let buf = test_buffer();
        let id = session
            .add_layer("test.png".into(), "/test/test.png".into(), buf)
            .unwrap();

        let layer_snapshot = session.layers[0].clone();

        let cmd = UndoCommand::AddLayer {
            layer_id: id,
            index: 0,
            layer: Box::new(layer_snapshot),
        };

        // Undo: remove the added layer
        let redo_cmd = cmd.undo(&mut session);
        assert_eq!(session.layers.len(), 0);

        // Redo: re-add it
        let _undo_cmd = redo_cmd.undo(&mut session);
        assert_eq!(session.layers.len(), 1);
        assert_eq!(session.layers[0].id, id);
    }

    #[test]
    fn test_undo_redo_set_reference() {
        let mut session = test_session_with_layers();
        let id0 = session.layers[0].id;
        let id1 = session.layers[1].id;

        assert!(session.layers[0].is_reference);
        assert!(!session.layers[1].is_reference);

        // Change reference to layer 1
        for l in &mut session.layers {
            l.is_reference = false;
        }
        session.layers[1].is_reference = true;

        let cmd = UndoCommand::SetReference {
            old_reference_id: Some(id0),
            new_reference_id: id1,
        };

        // Undo: restore id0 as reference
        let redo_cmd = cmd.undo(&mut session);
        assert!(session.layers[0].is_reference);
        assert!(!session.layers[1].is_reference);

        // Redo: set id1 as reference again
        let _undo_cmd = redo_cmd.undo(&mut session);
        assert!(!session.layers[0].is_reference);
        assert!(session.layers[1].is_reference);
    }

    #[test]
    fn test_undo_redo_add_annotation() {
        let mut session = test_session_with_layers();
        let layer_id = session.layers[0].id;

        let annotation = Annotation::new(
            layer_id,
            dc_core::ToolStyle::default(),
            dc_core::AnnotationData::Rectangle {
                start: dc_core::Point::new(0.0, 0.0),
                end: dc_core::Point::new(100.0, 100.0),
            },
        );
        let annot_id = annotation.id.clone();

        session.layers[0].annotations.push(annotation.clone());

        let cmd = UndoCommand::AddAnnotation {
            layer_id,
            annotation: Box::new(annotation),
        };

        // Undo: remove annotation
        let redo_cmd = cmd.undo(&mut session);
        assert!(session.layers[0].annotations.is_empty());

        // Redo: re-add
        let _undo_cmd = redo_cmd.undo(&mut session);
        assert_eq!(session.layers[0].annotations.len(), 1);
        assert_eq!(session.layers[0].annotations[0].id, annot_id);
    }

    #[test]
    fn test_undo_redo_remove_annotation() {
        let mut session = test_session_with_layers();
        let layer_id = session.layers[0].id;

        let annotation = Annotation::new(
            layer_id,
            dc_core::ToolStyle::default(),
            dc_core::AnnotationData::Line {
                start: dc_core::Point::new(0.0, 0.0),
                end: dc_core::Point::new(50.0, 50.0),
            },
        );
        session.layers[0].annotations.push(annotation.clone());

        // Remove it
        session.layers[0].annotations.remove(0);

        let cmd = UndoCommand::RemoveAnnotation {
            layer_id,
            annotation: Box::new(annotation.clone()),
            index: 0,
        };

        // Undo: re-insert
        let redo_cmd = cmd.undo(&mut session);
        assert_eq!(session.layers[0].annotations.len(), 1);

        // Redo: remove again
        let _undo_cmd = redo_cmd.undo(&mut session);
        assert!(session.layers[0].annotations.is_empty());
    }

    #[test]
    fn test_undo_redo_modify_annotation() {
        let mut session = test_session_with_layers();
        let layer_id = session.layers[0].id;

        let mut annotation = Annotation::new(
            layer_id,
            dc_core::ToolStyle::default(),
            dc_core::AnnotationData::Rectangle {
                start: dc_core::Point::new(0.0, 0.0),
                end: dc_core::Point::new(100.0, 100.0),
            },
        );
        let old_annotation = annotation.clone();

        annotation.data = dc_core::AnnotationData::Rectangle {
            start: dc_core::Point::new(10.0, 10.0),
            end: dc_core::Point::new(200.0, 200.0),
        };
        annotation.bounds = annotation.data.compute_bounds();
        let new_annotation = annotation.clone();

        session.layers[0].annotations.push(annotation);

        let cmd = UndoCommand::ModifyAnnotation {
            layer_id,
            old_annotation: Box::new(old_annotation.clone()),
            new_annotation: Box::new(new_annotation.clone()),
        };

        // Undo: restore original geometry
        let redo_cmd = cmd.undo(&mut session);
        match &session.layers[0].annotations[0].data {
            dc_core::AnnotationData::Rectangle { start, end } => {
                assert!((start.x - 0.0).abs() < 0.001);
                assert!((end.x - 100.0).abs() < 0.001);
            }
            _ => panic!("Expected rectangle"),
        }

        // Redo: apply modified geometry
        let _undo_cmd = redo_cmd.undo(&mut session);
        match &session.layers[0].annotations[0].data {
            dc_core::AnnotationData::Rectangle { start, end } => {
                assert!((start.x - 10.0).abs() < 0.001);
                assert!((end.x - 200.0).abs() < 0.001);
            }
            _ => panic!("Expected rectangle"),
        }
    }

    #[test]
    fn test_undo_redo_diff_config() {
        let mut session = test_session_with_layers();
        let old_config = session.diff_config.clone();

        session.diff_config.overlay_opacity = 0.8;
        session.diff_config.noise_threshold = 30;
        let new_config = session.diff_config.clone();

        let cmd = UndoCommand::SetDiffConfig {
            old_config: old_config.clone(),
            new_config: new_config.clone(),
        };

        // Undo
        let redo_cmd = cmd.undo(&mut session);
        assert!((session.diff_config.overlay_opacity - old_config.overlay_opacity).abs() < 0.001);
        assert_eq!(
            session.diff_config.noise_threshold,
            old_config.noise_threshold
        );

        // Redo
        let _undo_cmd = redo_cmd.undo(&mut session);
        assert!((session.diff_config.overlay_opacity - 0.8).abs() < 0.001);
        assert_eq!(session.diff_config.noise_threshold, 30);
    }

    #[test]
    fn test_undo_redo_custom_column() {
        let mut session = test_session_with_layers();
        let column = crate::state::CustomColumn {
            id: "col_1".to_string(),
            name: "Status".to_string(),
            col_type: crate::state::ColumnType::Text,
            default_value: String::new(),
        };

        session.custom_columns.push(column.clone());

        let cmd = UndoCommand::AddCustomColumn {
            index: 0,
            column: column.clone(),
        };

        // Undo: remove
        let redo_cmd = cmd.undo(&mut session);
        assert!(session.custom_columns.is_empty());

        // Redo: add back
        let _undo_cmd = redo_cmd.undo(&mut session);
        assert_eq!(session.custom_columns.len(), 1);
        assert_eq!(session.custom_columns[0].name, "Status");
    }

    #[test]
    fn test_undo_redo_count_counter() {
        let mut session = test_session_with_layers();
        session.count_counter = 5;

        let cmd = UndoCommand::SetCountCounter {
            old_value: 1,
            new_value: 5,
        };

        let redo_cmd = cmd.undo(&mut session);
        assert_eq!(session.count_counter, 1);

        let _undo_cmd = redo_cmd.undo(&mut session);
        assert_eq!(session.count_counter, 5);
    }

    #[test]
    fn test_compound_undo_redo() {
        let mut session = test_session_with_layers();
        let layer_id = session.layers[0].id;

        // Compound: hide layer + change opacity
        session.layers[0].visible = false;
        session.layers[0].opacity = 0.5;

        let compound = UndoCommand::Compound(vec![
            UndoCommand::SetLayerVisibility {
                layer_id,
                old_visible: true,
                new_visible: false,
            },
            UndoCommand::SetLayerOpacity {
                layer_id,
                old_opacity: 1.0,
                new_opacity: 0.5,
            },
        ]);

        // Undo compound
        let redo_compound = compound.undo(&mut session);
        assert!(session.layers[0].visible);
        assert!((session.layers[0].opacity - 1.0).abs() < 0.001);

        // Redo compound
        let _undo_compound = redo_compound.undo(&mut session);
        assert!(!session.layers[0].visible);
        assert!((session.layers[0].opacity - 0.5).abs() < 0.001);
    }

    // =========================================================================
    // Full undo/redo workflow tests
    // =========================================================================

    #[test]
    fn test_full_undo_redo_workflow() {
        let mut session = test_session_with_layers();
        let mut stack = UndoStack::new(50);
        let layer_id = session.layers[0].id;

        // Action 1: Hide layer
        let old_vis = session.layers[0].visible;
        session.layers[0].visible = false;
        stack.push(UndoCommand::SetLayerVisibility {
            layer_id,
            old_visible: old_vis,
            new_visible: false,
        });

        // Action 2: Change opacity
        let old_opacity = session.layers[0].opacity;
        session.layers[0].opacity = 0.3;
        stack.push(UndoCommand::SetLayerOpacity {
            layer_id,
            old_opacity,
            new_opacity: 0.3,
        });

        assert_eq!(stack.undo_count(), 2);

        // Undo action 2 (opacity)
        let cmd = stack.pop_undo().unwrap();
        let redo_cmd = cmd.undo(&mut session);
        stack.push_redo(redo_cmd);
        assert!((session.layers[0].opacity - 1.0).abs() < 0.001);

        // Undo action 1 (visibility)
        let cmd = stack.pop_undo().unwrap();
        let redo_cmd = cmd.undo(&mut session);
        stack.push_redo(redo_cmd);
        assert!(session.layers[0].visible);

        assert_eq!(stack.redo_count(), 2);

        // Redo action 1 (visibility)
        let cmd = stack.pop_redo().unwrap();
        let undo_cmd = cmd.undo(&mut session);
        stack.push(undo_cmd);
        assert!(!session.layers[0].visible);
        // Note: push clears redo, so only 1 in redo now (wait, push clears redo!)
        // Actually we just cleared it. For a proper redo workflow, we should
        // not use push() but a special method. Let me fix the full workflow test.
    }

    #[test]
    fn test_proper_undo_redo_workflow() {
        let mut session = test_session_with_layers();
        let mut stack = UndoStack::new(50);
        let id = session.layers[0].id;

        // Action 1: hide
        session.layers[0].visible = false;
        stack.push(UndoCommand::SetLayerVisibility {
            layer_id: id,
            old_visible: true,
            new_visible: false,
        });

        // Action 2: change opacity
        session.layers[0].opacity = 0.5;
        stack.push(UndoCommand::SetLayerOpacity {
            layer_id: id,
            old_opacity: 1.0,
            new_opacity: 0.5,
        });

        // Undo opacity
        if let Some(cmd) = stack.pop_undo() {
            let redo_cmd = cmd.undo(&mut session);
            stack.push_redo(redo_cmd);
        }
        assert!((session.layers[0].opacity - 1.0).abs() < 0.001);
        assert_eq!(stack.undo_count(), 1);
        assert_eq!(stack.redo_count(), 1);

        // Undo visibility
        if let Some(cmd) = stack.pop_undo() {
            let redo_cmd = cmd.undo(&mut session);
            stack.push_redo(redo_cmd);
        }
        assert!(session.layers[0].visible);
        assert_eq!(stack.undo_count(), 0);
        assert_eq!(stack.redo_count(), 2);

        // Redo visibility
        if let Some(cmd) = stack.pop_redo() {
            let undo_cmd = cmd.redo(&mut session);
            stack.push_for_redo(undo_cmd);
        }
        assert!(!session.layers[0].visible);
        assert_eq!(stack.undo_count(), 1);
        assert_eq!(stack.redo_count(), 1);

        // Redo opacity
        if let Some(cmd) = stack.pop_redo() {
            let undo_cmd = cmd.redo(&mut session);
            stack.push_for_redo(undo_cmd);
        }
        assert!((session.layers[0].opacity - 0.5).abs() < 0.001);
        assert_eq!(stack.undo_count(), 2);
        assert_eq!(stack.redo_count(), 0);
    }

    #[test]
    fn test_undo_branch_clears_redo() {
        let mut session = test_session_with_layers();
        let mut stack = UndoStack::new(50);
        let id = session.layers[0].id;

        // Action 1
        session.layers[0].visible = false;
        stack.push(UndoCommand::SetLayerVisibility {
            layer_id: id,
            old_visible: true,
            new_visible: false,
        });

        // Undo
        if let Some(cmd) = stack.pop_undo() {
            let redo_cmd = cmd.undo(&mut session);
            stack.push_redo(redo_cmd);
        }
        assert_eq!(stack.redo_count(), 1);

        // New action (should clear redo)
        session.layers[0].opacity = 0.7;
        stack.push(UndoCommand::SetLayerOpacity {
            layer_id: id,
            old_opacity: 1.0,
            new_opacity: 0.7,
        });
        assert_eq!(stack.redo_count(), 0);
        assert_eq!(stack.undo_count(), 1);
    }

    #[test]
    fn test_multiple_annotations_undo_redo() {
        let mut session = test_session_with_layers();
        let mut stack = UndoStack::new(50);
        let layer_id = session.layers[0].id;

        // Add 3 annotations
        for i in 0..3 {
            let annotation = Annotation::new(
                layer_id,
                dc_core::ToolStyle::default(),
                dc_core::AnnotationData::Rectangle {
                    start: dc_core::Point::new(i as f32 * 10.0, 0.0),
                    end: dc_core::Point::new(i as f32 * 10.0 + 50.0, 50.0),
                },
            );
            session.layers[0].annotations.push(annotation.clone());
            stack.push(UndoCommand::AddAnnotation {
                layer_id,
                annotation: Box::new(annotation),
            });
        }

        assert_eq!(session.layers[0].annotations.len(), 3);

        // Undo all 3
        for _ in 0..3 {
            if let Some(cmd) = stack.pop_undo() {
                let redo_cmd = cmd.undo(&mut session);
                stack.push_redo(redo_cmd);
            }
        }
        assert_eq!(session.layers[0].annotations.len(), 0);

        // Redo all 3
        for _ in 0..3 {
            if let Some(cmd) = stack.pop_redo() {
                let undo_cmd = cmd.redo(&mut session);
                stack.push_for_redo(undo_cmd);
            }
        }
        assert_eq!(session.layers[0].annotations.len(), 3);
    }

    // =========================================================================
    // Serialization Roundtrip Tests
    // =========================================================================

    #[test]
    fn test_save_roundtrip_simple_commands() {
        // Test that simple commands survive save → load roundtrip
        let cmds = vec![
            UndoCommand::MoveLayer {
                layer_id: LayerId(1),
                from_index: 0,
                to_index: 2,
            },
            UndoCommand::RenameLayer {
                layer_id: LayerId(2),
                old_name: "old".into(),
                new_name: "new".into(),
            },
            UndoCommand::SetLayerVisibility {
                layer_id: LayerId(3),
                old_visible: true,
                new_visible: false,
            },
            UndoCommand::SetLayerOpacity {
                layer_id: LayerId(4),
                old_opacity: 0.5,
                new_opacity: 1.0,
            },
            UndoCommand::SetLayerBlendColor {
                layer_id: LayerId(5),
                old_color: LayerColor { r: 255, g: 0, b: 0 },
                new_color: LayerColor { r: 0, g: 255, b: 0 },
            },
            UndoCommand::SetLayerOffset {
                layer_id: LayerId(6),
                old_x: 0.0,
                old_y: 0.0,
                new_x: 10.0,
                new_y: 20.0,
            },
            UndoCommand::SetReference {
                old_reference_id: Some(LayerId(1)),
                new_reference_id: LayerId(2),
            },
            UndoCommand::SetCountCounter {
                old_value: 5,
                new_value: 6,
            },
        ];

        // No layer loading needed for simple commands
        let load_fn = |_: &LayerSaveData| -> Option<Layer> { None };

        for cmd in cmds {
            let saved = cmd.to_save();
            let json = serde_json::to_string(&saved).unwrap();
            let deserialized: UndoCommandSave = serde_json::from_str(&json).unwrap();
            let restored = deserialized.to_command(&load_fn);
            assert!(
                restored.is_some(),
                "Command should roundtrip: {}",
                cmd.description()
            );
        }
    }

    #[test]
    fn test_save_roundtrip_annotation_commands() {
        let annot = Annotation::new(
            LayerId(1),
            dc_core::ToolStyle::default(),
            dc_core::AnnotationData::Rectangle {
                start: dc_core::Point::new(10.0, 20.0),
                end: dc_core::Point::new(100.0, 200.0),
            },
        );
        let annot2 = {
            let mut a = annot.clone();
            a.data = dc_core::AnnotationData::Rectangle {
                start: dc_core::Point::new(15.0, 25.0),
                end: dc_core::Point::new(105.0, 205.0),
            };
            a
        };

        let cmds = vec![
            UndoCommand::AddAnnotation {
                layer_id: LayerId(1),
                annotation: Box::new(annot.clone()),
            },
            UndoCommand::RemoveAnnotation {
                layer_id: LayerId(1),
                annotation: Box::new(annot.clone()),
                index: 0,
            },
            UndoCommand::ModifyAnnotation {
                layer_id: LayerId(1),
                old_annotation: Box::new(annot),
                new_annotation: Box::new(annot2),
            },
        ];

        let load_fn = |_: &LayerSaveData| -> Option<Layer> { None };

        for cmd in cmds {
            let saved = cmd.to_save();
            let json = serde_json::to_string(&saved).unwrap();
            let deserialized: UndoCommandSave = serde_json::from_str(&json).unwrap();
            let restored = deserialized.to_command(&load_fn);
            assert!(
                restored.is_some(),
                "Annotation command should roundtrip: {}",
                cmd.description()
            );
        }
    }

    #[test]
    fn test_save_roundtrip_diff_config() {
        let cmd = UndoCommand::SetDiffConfig {
            old_config: DiffConfig::default(),
            new_config: DiffConfig {
                binary_threshold: 42,
                ..DiffConfig::default()
            },
        };

        let saved = cmd.to_save();
        let json = serde_json::to_string(&saved).unwrap();
        let deserialized: UndoCommandSave = serde_json::from_str(&json).unwrap();
        let load_fn = |_: &LayerSaveData| -> Option<Layer> { None };
        let restored = deserialized.to_command(&load_fn).unwrap();

        // Verify the data survived
        if let UndoCommand::SetDiffConfig { new_config, .. } = &restored {
            assert_eq!(new_config.binary_threshold, 42);
        } else {
            panic!("Wrong command type after roundtrip");
        }
    }

    #[test]
    fn test_save_roundtrip_custom_column() {
        use crate::state::{ColumnType, CustomColumn};

        let col = CustomColumn {
            id: "test-col".into(),
            name: "My Column".into(),
            col_type: ColumnType::Text,
            default_value: "".into(),
        };

        let cmds = vec![
            UndoCommand::AddCustomColumn {
                index: 0,
                column: col.clone(),
            },
            UndoCommand::RemoveCustomColumn {
                index: 0,
                column: col,
            },
        ];

        let load_fn = |_: &LayerSaveData| -> Option<Layer> { None };

        for cmd in cmds {
            let saved = cmd.to_save();
            let json = serde_json::to_string(&saved).unwrap();
            let deserialized: UndoCommandSave = serde_json::from_str(&json).unwrap();
            let restored = deserialized.to_command(&load_fn);
            assert!(restored.is_some());
        }
    }

    #[test]
    fn test_save_roundtrip_compound() {
        let compound = UndoCommand::Compound(vec![
            UndoCommand::SetCountCounter {
                old_value: 0,
                new_value: 1,
            },
            UndoCommand::RenameLayer {
                layer_id: LayerId(1),
                old_name: "a".into(),
                new_name: "b".into(),
            },
        ]);

        let saved = compound.to_save();
        let json = serde_json::to_string(&saved).unwrap();
        let deserialized: UndoCommandSave = serde_json::from_str(&json).unwrap();
        let load_fn = |_: &LayerSaveData| -> Option<Layer> { None };
        let restored = deserialized.to_command(&load_fn).unwrap();

        if let UndoCommand::Compound(cmds) = restored {
            assert_eq!(cmds.len(), 2);
        } else {
            panic!("Expected Compound command");
        }
    }

    #[test]
    fn test_save_roundtrip_layer_commands_with_loader() {
        // Test that AddLayer/RemoveLayer serialize correctly and can be loaded
        // back when a proper loader is provided
        let layer = test_layer(42, "test_layer.png");
        let cmd = UndoCommand::AddLayer {
            layer_id: LayerId(42),
            index: 0,
            layer: Box::new(layer),
        };

        let saved = cmd.to_save();
        let json = serde_json::to_string(&saved).unwrap();
        let deserialized: UndoCommandSave = serde_json::from_str(&json).unwrap();

        // Provide a mock loader that creates a layer from save data
        let load_fn = |data: &LayerSaveData| -> Option<Layer> {
            let img = dc_core::RasterBuffer::new(image::RgbaImage::new(10, 10), 72);
            let mut layer = Layer::new(
                data.id,
                data.name.clone(),
                data.source_path.clone(),
                img,
                data.is_reference,
            );
            layer.visible = data.visible;
            layer.opacity = data.opacity;
            layer.blend_color = data.blend_color;
            layer.offset_x = data.offset_x;
            layer.offset_y = data.offset_y;
            layer.annotations = data.annotations.clone();
            Some(layer)
        };

        let restored = deserialized.to_command(&load_fn).unwrap();
        if let UndoCommand::AddLayer {
            layer_id,
            index,
            layer,
        } = &restored
        {
            assert_eq!(*layer_id, LayerId(42));
            assert_eq!(*index, 0);
            assert_eq!(layer.name, "test_layer.png");
        } else {
            panic!("Wrong command type");
        }
    }

    #[test]
    fn test_save_roundtrip_layer_commands_fail_gracefully() {
        // When the loader returns None, layer commands are silently dropped
        let layer = test_layer(1, "missing.png");
        let cmd = UndoCommand::AddLayer {
            layer_id: LayerId(1),
            index: 0,
            layer: Box::new(layer),
        };

        let saved = cmd.to_save();
        let load_fn = |_: &LayerSaveData| -> Option<Layer> { None };
        let restored = saved.to_command(&load_fn);
        assert!(restored.is_none(), "Should return None when loader fails");
    }

    #[test]
    fn test_undo_stack_save_roundtrip() {
        // Test full UndoStack save/load
        let mut stack = UndoStack::new(50);

        stack.push(UndoCommand::RenameLayer {
            layer_id: LayerId(1),
            old_name: "a".into(),
            new_name: "b".into(),
        });
        stack.push(UndoCommand::SetLayerOpacity {
            layer_id: LayerId(1),
            old_opacity: 0.5,
            new_opacity: 1.0,
        });

        // Simulate an undo to create a redo entry
        let _undone = stack.pop_undo().unwrap();
        stack.push_redo(UndoCommand::SetLayerOpacity {
            layer_id: LayerId(1),
            old_opacity: 1.0,
            new_opacity: 0.5,
        });

        assert_eq!(stack.undo_count(), 1);
        assert_eq!(stack.redo_count(), 1);

        // Save
        let save = stack.to_save();
        let json = serde_json::to_string(&save).unwrap();

        // Load
        let loaded_save: UndoStackSave = serde_json::from_str(&json).unwrap();
        let load_fn = |_: &LayerSaveData| -> Option<Layer> { None };
        let restored = UndoStack::from_save(loaded_save, &load_fn);

        assert_eq!(restored.undo_count(), 1);
        assert_eq!(restored.redo_count(), 1);
        assert_eq!(restored.max_depth(), 50);
        assert!(restored.can_undo());
        assert!(restored.can_redo());
    }

    // =========================================================================
    // Compound Undo/Redo Tests
    // =========================================================================

    #[test]
    fn test_compound_count_undo_redo() {
        let mut session = SessionState::new();
        let layer = test_layer(1, "test");
        session.layers.push(layer);
        session.count_counter = 0;

        let annot = Annotation::new(
            LayerId(1),
            dc_core::ToolStyle::default(),
            dc_core::AnnotationData::Count {
                pos: dc_core::Point::new(50.0, 50.0),
                number: 0,
                sequence_group_id: None,
            },
        );
        session.layers[0].annotations.push(annot);
        session.count_counter = 1;

        // Compound: AddAnnotation + SetCountCounter
        let compound = UndoCommand::Compound(vec![
            UndoCommand::AddAnnotation {
                layer_id: LayerId(1),
                annotation: Box::new(session.layers[0].annotations[0].clone()),
            },
            UndoCommand::SetCountCounter {
                old_value: 0,
                new_value: 1,
            },
        ]);

        // Undo compound
        let redo_cmd = compound.undo(&mut session);
        assert_eq!(session.layers[0].annotations.len(), 0);
        assert_eq!(session.count_counter, 0);

        // Redo compound
        let undo_cmd2 = redo_cmd.redo(&mut session);
        assert_eq!(session.layers[0].annotations.len(), 1);
        assert_eq!(session.count_counter, 1);

        // Undo again
        let _ = undo_cmd2.undo(&mut session);
        assert_eq!(session.layers[0].annotations.len(), 0);
        assert_eq!(session.count_counter, 0);
    }

    // =========================================================================
    // Edge Case Tests
    // =========================================================================

    #[test]
    fn test_undo_empty_stack() {
        let mut stack = UndoStack::new(50);
        assert!(stack.pop_undo().is_none());
        assert!(stack.pop_redo().is_none());
        assert!(!stack.can_undo());
        assert!(!stack.can_redo());
        assert!(stack.undo_description().is_none());
        assert!(stack.redo_description().is_none());
    }

    #[test]
    fn test_push_for_redo_preserves_redo_stack() {
        let mut stack = UndoStack::new(50);

        stack.push(UndoCommand::SetCountCounter {
            old_value: 0,
            new_value: 1,
        });

        // Pop into redo
        let cmd = stack.pop_undo().unwrap();
        stack.push_redo(cmd);

        assert_eq!(stack.undo_count(), 0);
        assert_eq!(stack.redo_count(), 1);

        // push_for_redo should add to undo without clearing redo
        stack.push_for_redo(UndoCommand::SetCountCounter {
            old_value: 1,
            new_value: 2,
        });

        assert_eq!(stack.undo_count(), 1);
        assert_eq!(stack.redo_count(), 1); // Redo not cleared!
    }

    #[test]
    fn test_new_push_clears_redo_stack() {
        let mut stack = UndoStack::new(50);

        stack.push(UndoCommand::SetCountCounter {
            old_value: 0,
            new_value: 1,
        });

        let cmd = stack.pop_undo().unwrap();
        stack.push_redo(cmd);
        assert_eq!(stack.redo_count(), 1);

        // New push should clear redo (branching)
        stack.push(UndoCommand::SetCountCounter {
            old_value: 0,
            new_value: 2,
        });
        assert_eq!(stack.redo_count(), 0);
    }

    #[test]
    fn test_max_depth_enforcement() {
        let mut stack = UndoStack::new(3);

        for i in 0..5 {
            stack.push(UndoCommand::SetCountCounter {
                old_value: i,
                new_value: i + 1,
            });
        }

        // Only 3 should remain
        assert_eq!(stack.undo_count(), 3);

        // Oldest (0→1, 1→2) should have been dropped, leaving 2→3, 3→4, 4→5
        let cmd = stack.pop_undo().unwrap();
        if let UndoCommand::SetCountCounter {
            old_value,
            new_value,
        } = &cmd
        {
            assert_eq!(*old_value, 4);
            assert_eq!(*new_value, 5);
        }
    }

    #[test]
    fn test_undo_redo_full_cycle_5_operations() {
        let mut session = SessionState::new();
        let layer = test_layer(1, "layer1");
        session.layers.push(layer);
        let mut stack = UndoStack::new(50);

        // Push 5 rename operations
        let names = vec!["A", "B", "C", "D", "E"];
        let mut prev_name = "layer1".to_string();
        for name in &names {
            session.layers[0].name = name.to_string();
            stack.push(UndoCommand::RenameLayer {
                layer_id: LayerId(1),
                old_name: prev_name.clone(),
                new_name: name.to_string(),
            });
            prev_name = name.to_string();
        }

        // Undo all 5
        for i in (0..5).rev() {
            let cmd = stack.pop_undo().unwrap();
            let redo = cmd.undo(&mut session);
            stack.push_redo(redo);
        }
        assert_eq!(session.layers[0].name, "layer1");

        // Redo 3 of 5
        for _ in 0..3 {
            let cmd = stack.pop_redo().unwrap();
            let undo = cmd.redo(&mut session);
            stack.push_for_redo(undo);
        }
        assert_eq!(session.layers[0].name, "C");

        // Undo 1
        let cmd = stack.pop_undo().unwrap();
        let redo = cmd.undo(&mut session);
        stack.push_redo(redo);
        assert_eq!(session.layers[0].name, "B");
    }

    #[test]
    fn test_interleaved_undo_redo_with_new_commands() {
        let mut session = SessionState::new();
        let layer = test_layer(1, "start");
        session.layers.push(layer);
        let mut stack = UndoStack::new(50);

        // Rename to "A"
        session.layers[0].name = "A".to_string();
        stack.push(UndoCommand::RenameLayer {
            layer_id: LayerId(1),
            old_name: "start".into(),
            new_name: "A".into(),
        });

        // Rename to "B"
        session.layers[0].name = "B".to_string();
        stack.push(UndoCommand::RenameLayer {
            layer_id: LayerId(1),
            old_name: "A".into(),
            new_name: "B".into(),
        });

        // Undo B→A
        let cmd = stack.pop_undo().unwrap();
        let redo = cmd.undo(&mut session);
        stack.push_redo(redo);
        assert_eq!(session.layers[0].name, "A");

        // Now push a new command (branch) — this should clear redo
        session.layers[0].name = "C".to_string();
        stack.push(UndoCommand::RenameLayer {
            layer_id: LayerId(1),
            old_name: "A".into(),
            new_name: "C".into(),
        });
        assert_eq!(stack.redo_count(), 0);

        // Undo C→A
        let cmd = stack.pop_undo().unwrap();
        let _ = cmd.undo(&mut session);
        assert_eq!(session.layers[0].name, "A");
    }

    #[test]
    fn test_undo_modify_annotation_with_custom_properties() {
        let mut session = SessionState::new();
        let layer = test_layer(1, "layer");
        session.layers.push(layer);

        let mut annot = Annotation::new(
            LayerId(1),
            dc_core::ToolStyle::default(),
            dc_core::AnnotationData::Rectangle {
                start: dc_core::Point::new(0.0, 0.0),
                end: dc_core::Point::new(100.0, 100.0),
            },
        );
        annot
            .custom_properties
            .insert("status".into(), "open".into());
        session.layers[0].annotations.push(annot.clone());

        // Modify: change custom property
        let old_annot = annot.clone();
        let mut new_annot = annot.clone();
        new_annot
            .custom_properties
            .insert("status".into(), "closed".into());
        session.layers[0].annotations[0] = new_annot.clone();

        let cmd = UndoCommand::ModifyAnnotation {
            layer_id: LayerId(1),
            old_annotation: Box::new(old_annot),
            new_annotation: Box::new(new_annot),
        };

        // Undo
        let redo = cmd.undo(&mut session);
        assert_eq!(
            session.layers[0].annotations[0]
                .custom_properties
                .get("status"),
            Some(&"open".to_string())
        );

        // Redo
        let undo2 = redo.redo(&mut session);
        assert_eq!(
            session.layers[0].annotations[0]
                .custom_properties
                .get("status"),
            Some(&"closed".to_string())
        );
    }

    #[test]
    fn test_undo_remove_layer_restores_position_and_reference() {
        let mut session = SessionState::new();
        let l1 = test_layer(1, "first");
        let mut l2 = test_layer(2, "second");
        l2.is_reference = false;
        let l3 = test_layer(3, "third");
        session.layers.push(l1);
        session.layers.push(l2.clone());
        session.layers.push(l3);
        session.selected_layer = Some(LayerId(2));

        // Remove layer 2 (index 1)
        let cmd = UndoCommand::RemoveLayer {
            layer_id: LayerId(2),
            index: 1,
            layer: Box::new(l2),
            was_reference: false,
            old_selected: Some(LayerId(2)),
        };

        session.layers.remove(1);
        assert_eq!(session.layers.len(), 2);

        // Undo: should re-insert at index 1
        let redo = cmd.undo(&mut session);
        assert_eq!(session.layers.len(), 3);
        assert_eq!(session.layers[1].name, "second");
        assert_eq!(session.selected_layer, Some(LayerId(2)));

        // Redo: should remove again
        let _ = redo.redo(&mut session);
        assert_eq!(session.layers.len(), 2);
    }
}
