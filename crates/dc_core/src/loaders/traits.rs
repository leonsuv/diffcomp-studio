// =============================================================================
// dc_core/loaders/traits - Document Loader Trait Definitions
// =============================================================================
// The core abstraction that makes the loader system extensible.
// Any document type that can be rendered to pixels implements this trait.
// =============================================================================

use crate::types::{CoreResult, RasterBuffer};
use std::path::Path;

/// Metadata about a loaded document.
/// Provides information for UI display and processing decisions.
#[derive(Debug, Clone)]
pub struct LoaderMetadata {
    /// Original filename
    pub filename: String,

    /// File extension (lowercase, without dot)
    pub extension: String,

    /// File size in bytes
    pub file_size: u64,

    /// Total page count (1 for single images)
    pub page_count: usize,

    /// Native width in pixels (or points for vector formats)
    pub native_width: Option<u32>,

    /// Native height in pixels (or points for vector formats)
    pub native_height: Option<u32>,

    /// Is this a vector format (PDF, SVG)?
    pub is_vector: bool,

    /// Format-specific information (e.g., "PDF 1.7", "PNG 8-bit")
    pub format_info: String,
}

/// Capabilities that a loader supports.
/// Used for feature detection and UI enablement.
#[derive(Debug, Clone, Copy, Default)]
pub struct LoaderCapabilities {
    /// Can render at arbitrary DPI (vector formats)
    pub supports_dpi_scaling: bool,

    /// Has multiple pages
    pub supports_pages: bool,

    /// Can extract text (for future OCR overlay)
    pub supports_text_extraction: bool,

    /// Can preserve layers (e.g., PSD, AI)
    pub supports_layers: bool,

    /// Supports transparency
    pub supports_alpha: bool,
}

/// The core trait for all document loaders.
///
/// # Design Rationale
///
/// This trait converts ANY document format into a `RasterBuffer`, which is
/// our universal format for comparison. This abstraction allows:
///
/// 1. **Uniform Processing**: All alignment/diff logic works on RasterBuffer
/// 2. **Extensibility**: New formats (DWG, 3D) just need to render to buffer
/// 3. **Testing**: Easy to mock for unit tests
///
/// # Example Implementation
///
/// ```ignore
/// struct MyLoader;
///
/// impl DocumentLoader for MyLoader {
///     fn can_load(&self, path: &Path) -> bool {
///         path.extension()
///             .map(|e| e.to_string_lossy().to_lowercase() == "myformat")
///             .unwrap_or(false)
///     }
///
///     fn load(&self, path: &Path, config: &LoadConfig) -> CoreResult<RasterBuffer> {
///         // Convert to pixels...
///     }
///     // ...
/// }
/// ```
pub trait DocumentLoader: Send + Sync {
    /// Check if this loader can handle the given file.
    /// Should be fast (just check extension, magic bytes if needed).
    fn can_load(&self, path: &Path) -> bool;

    /// Get the file extensions this loader supports (lowercase, no dot).
    fn supported_extensions(&self) -> &[&str];

    /// Get loader capabilities.
    fn capabilities(&self) -> LoaderCapabilities;

    /// Load and rasterize the document.
    ///
    /// # Arguments
    /// * `path` - Path to the document file
    /// * `config` - Loading configuration (DPI, page number, etc.)
    ///
    /// # Returns
    /// * `Ok(RasterBuffer)` - The rasterized image
    /// * `Err(CoreError)` - If loading fails
    fn load(&self, path: &Path, config: &LoadConfig) -> CoreResult<RasterBuffer>;

    /// Load a document from memory buffer.
    ///
    /// # Arguments
    /// * `data` - The file content
    /// * `name_hint` - Original filename or hint for format detection
    /// * `config` - Loading configuration
    fn load_from_memory(
        &self,
        _data: &[u8],
        _name_hint: &str,
        _config: &LoadConfig,
    ) -> CoreResult<RasterBuffer> {
        Err(crate::types::CoreError::UnsupportedFeature(
            "load_from_memory".into(),
        ))
    }

    /// Load every page of the document, in page order.
    ///
    /// Single-page formats return one buffer. `config.page_index` is ignored.
    fn load_all_pages(&self, path: &Path, config: &LoadConfig) -> CoreResult<Vec<RasterBuffer>> {
        self.load(path, config).map(|buffer| vec![buffer])
    }

    /// Load every page of an in-memory document, in page order.
    fn load_all_pages_from_memory(
        &self,
        data: &[u8],
        name_hint: &str,
        config: &LoadConfig,
    ) -> CoreResult<Vec<RasterBuffer>> {
        self.load_from_memory(data, name_hint, config)
            .map(|buffer| vec![buffer])
    }

    /// Get document metadata without fully loading.
    /// Useful for displaying file info before expensive rasterization.
    fn get_metadata(&self, path: &Path) -> CoreResult<LoaderMetadata>;

    /// Get a human-readable name for this loader.
    fn name(&self) -> &str;
}

/// Configuration for document loading.
/// Allows customizing the rasterization process.
#[derive(Debug, Clone)]
pub struct LoadConfig {
    /// Target DPI for rasterization (default: 300)
    pub dpi: u32,

    /// Page number to load (0-indexed, for multi-page documents)
    pub page_index: usize,

    /// Maximum dimension (width or height) to prevent memory issues
    /// If the image would exceed this, it's scaled down
    pub max_dimension: Option<u32>,

    /// Background color for transparent areas (RGBA)
    pub background_color: [u8; 4],

    /// Enable anti-aliasing for vector rasterization
    pub anti_alias: bool,
}

impl Default for LoadConfig {
    fn default() -> Self {
        Self {
            dpi: crate::DEFAULT_RENDER_DPI,
            page_index: 0,
            max_dimension: None, // No limit — load at native resolution
            background_color: [255, 255, 255, 255], // White background
            anti_alias: true,
        }
    }
}

impl LoadConfig {
    /// Create a config for high-quality print output
    pub fn print_quality() -> Self {
        Self {
            dpi: 600,
            ..Default::default()
        }
    }

    /// Create a config for screen preview (faster, lower memory)
    pub fn preview_quality() -> Self {
        Self {
            dpi: 150,
            max_dimension: Some(4096),
            ..Default::default()
        }
    }

    /// Create a config for thumbnail generation
    pub fn thumbnail() -> Self {
        Self {
            dpi: 72,
            max_dimension: Some(512),
            anti_alias: false,
            ..Default::default()
        }
    }

    /// Builder: Set DPI
    pub fn with_dpi(mut self, dpi: u32) -> Self {
        self.dpi = dpi;
        self
    }

    /// Builder: Set page index
    pub fn with_page(mut self, page_index: usize) -> Self {
        self.page_index = page_index;
        self
    }

    /// Builder: Set max dimension
    pub fn with_max_dimension(mut self, max: u32) -> Self {
        self.max_dimension = Some(max);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_config_defaults() {
        let config = LoadConfig::default();
        assert_eq!(config.dpi, 300);
        assert_eq!(config.page_index, 0);
        assert!(config.anti_alias);
    }

    #[test]
    fn test_load_config_builder() {
        let config = LoadConfig::default()
            .with_dpi(600)
            .with_page(2)
            .with_max_dimension(8192);

        assert_eq!(config.dpi, 600);
        assert_eq!(config.page_index, 2);
        assert_eq!(config.max_dimension, Some(8192));
    }

    #[test]
    fn test_preview_quality() {
        let config = LoadConfig::preview_quality();
        assert_eq!(config.dpi, 150);
        assert_eq!(config.max_dimension, Some(4096));
    }
}
