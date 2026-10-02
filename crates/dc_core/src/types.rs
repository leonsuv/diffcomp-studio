// =============================================================================
// dc_core/types - Common Types and Error Definitions
// =============================================================================
// This module defines the foundational types used throughout dc_core.
// All error handling follows the "no unwrap()" policy using thiserror.
// =============================================================================

use image::{DynamicImage, RgbaImage};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use thiserror::Error;

// =============================================================================
// Error Types
// =============================================================================

/// Comprehensive error type for all dc_core operations.
/// Uses thiserror for zero-cost error handling with rich context.
#[derive(Error, Debug)]
pub enum CoreError {
    // -------------------------------------------------------------------------
    // File I/O Errors
    // -------------------------------------------------------------------------
    /// Failed to read a file from disk
    #[error("Failed to read file '{path}': {source}")]
    FileReadError {
        /// The path of the file that failed to read
        path: PathBuf,
        #[source]
        /// The underlying I/O error
        source: std::io::Error,
    },

    /// File format not supported
    #[error("Unsupported file format: {extension}")]
    UnsupportedFormat {
        /// The file extension that is not supported
        extension: String,
    },

    /// File not found
    #[error("File not found: {path}")]
    FileNotFound {
        /// The path of the missing file
        path: PathBuf,
    },

    // -------------------------------------------------------------------------
    // Image Processing Errors
    // -------------------------------------------------------------------------
    /// Image decoding failed
    #[error("Failed to decode image: {reason}")]
    ImageDecodeError {
        /// The reason for the decoding failure
        reason: String,
    },

    /// Image dimensions invalid or mismatched
    #[error("Invalid image dimensions: {width}x{height} - {reason}")]
    InvalidDimensions {
        /// width of the image
        width: u32,
        /// height of the image
        height: u32,
        /// reason for invalidity
        reason: String,
    },

    /// Image too large to process
    #[error("Image exceeds maximum size: {width}x{height} (max: {max_pixels} pixels)")]
    ImageTooLarge {
        /// width of the image
        width: u32,
        /// height of the image
        height: u32,
        /// maximum allowed pixels
        max_pixels: u64,
    },

    // -------------------------------------------------------------------------
    // Alignment Errors
    // -------------------------------------------------------------------------
    /// Not enough features detected for alignment
    #[error("Insufficient features detected: found {found}, minimum required {required}")]
    InsufficientFeatures {
        /// Number of features found
        found: usize,
        /// Minimum number of features required
        required: usize,
    },

    /// Feature matching failed
    #[error("Feature matching failed: {reason}")]
    FeatureMatchingFailed {
        /// The reason for the failure
        reason: String,
    },

    /// Homography computation failed (RANSAC didn't converge)
    #[error("Homography computation failed: {reason}")]
    HomographyFailed {
        /// The reason for the failure
        reason: String,
    },

    /// Images are too dissimilar to align
    #[error("Images too dissimilar for alignment: confidence {confidence:.2}% (minimum: {threshold:.2}%)")]
    AlignmentConfidenceTooLow {
        /// The calculated confidence score
        confidence: f64,
        /// The minimum threshold required
        threshold: f64,
    },

    // -------------------------------------------------------------------------
    // Document Loading Errors (reserved for future PDF/vector support)
    // -------------------------------------------------------------------------

    // -------------------------------------------------------------------------
    // Compute Pipeline Errors
    // -------------------------------------------------------------------------
    /// A compute pipeline operation failed (alignment, warp, GPU dispatch, etc.)
    #[error("Compute error: {operation} - {message}")]
    ComputeError {
        /// The operation that failed
        operation: String,
        /// The error message
        message: String,
    },

    // -------------------------------------------------------------------------
    // Layer Management Errors
    // -------------------------------------------------------------------------
    /// Maximum layer count exceeded
    #[error("Maximum layer count ({max}) exceeded")]
    MaxLayersExceeded {
        /// The maximum allowed layers
        max: usize,
    },

    /// Layer not found
    #[error("Layer not found: {id:?}")]
    LayerNotFound {
        /// The ID of the layer that was not found
        id: LayerId,
    },

    // -------------------------------------------------------------------------
    // Generic Errors
    // -------------------------------------------------------------------------
    /// Internal error (should never happen in production)
    #[error("Internal error: {message}")]
    InternalError {
        /// The internal error message
        message: String,
    },

    /// Feature not supported
    #[error("Feature not supported: {0}")]
    UnsupportedFeature(String),
}

/// Result type alias using CoreError
pub type CoreResult<T> = Result<T, CoreError>;

// =============================================================================
// Layer Types
// =============================================================================

/// Unique identifier for a layer in the comparison session.
/// Uses a newtype pattern for type safety.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LayerId(pub u32);

impl LayerId {
    /// Create a new LayerId
    pub fn new(id: u32) -> Self {
        Self(id)
    }

    /// Get the inner value
    pub fn inner(&self) -> u32 {
        self.0
    }
}

impl std::fmt::Display for LayerId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Layer-{}", self.0)
    }
}

use crate::annotations::Annotation;

/// Represents a single layer in the comparison stack.
/// Each layer contains:
/// - The original loaded image
/// - The aligned (warped) version (computed after alignment)
/// - Display properties (visibility, opacity, blend mode)
/// - Vector annotations
#[derive(Debug, Clone)]
pub struct Layer {
    /// Unique identifier
    pub id: LayerId,

    /// Human-readable name (usually the filename)
    pub name: String,

    /// Original file path
    pub source_path: PathBuf,

    /// Original image as loaded
    pub original: std::sync::Arc<RasterBuffer>,

    /// Aligned image (after homography transform)
    /// None if this is the reference layer or alignment hasn't run
    pub aligned: Option<std::sync::Arc<RasterBuffer>>,

    /// The homography matrix used for alignment (3x3, row-major)
    /// None if this is the reference layer
    pub homography_matrix: Option<[[f64; 3]; 3]>,

    /// Is this layer visible in the viewport?
    pub visible: bool,

    /// Opacity (0.0 = transparent, 1.0 = opaque)
    pub opacity: f32,

    /// Blend color for difference visualization
    pub blend_color: LayerColor,

    /// Is this the reference layer (first added)?
    pub is_reference: bool,

    /// Alignment confidence score (0.0 - 1.0)
    pub alignment_confidence: Option<f64>,

    /// Vector annotations on this layer
    pub annotations: Vec<Annotation>,

    /// Manual offset X (pixels)
    pub offset_x: f32,
    /// Manual offset Y (pixels)
    pub offset_y: f32,

    /// All pages of the document. The page-specific fields above always hold
    /// `active_page`; its entry here is outdated until another page is shown.
    pub pages: Vec<LayerPage>,

    /// Page shown in the page-specific fields. May exceed the page count when
    /// the session shows a page this document does not have (blank).
    pub active_page: usize,
}

/// Page-specific state of a document layer.
#[derive(Debug, Clone)]
pub struct LayerPage {
    /// Page image as loaded
    pub original: std::sync::Arc<RasterBuffer>,
    /// Page aligned to the reference page
    pub aligned: Option<std::sync::Arc<RasterBuffer>>,
    /// Alignment homography for this page
    pub homography_matrix: Option<[[f64; 3]; 3]>,
    /// Alignment confidence for this page
    pub alignment_confidence: Option<f64>,
    /// Markups drawn on this page
    pub annotations: Vec<Annotation>,
    /// Manual offset X (pixels)
    pub offset_x: f32,
    /// Manual offset Y (pixels)
    pub offset_y: f32,
}

impl LayerPage {
    /// Unaligned page without markups.
    pub fn new(original: RasterBuffer) -> Self {
        Self {
            original: std::sync::Arc::new(original),
            aligned: None,
            homography_matrix: None,
            alignment_confidence: None,
            annotations: Vec::new(),
            offset_x: 0.0,
            offset_y: 0.0,
        }
    }

    /// Transparent stand-in for a page the document does not have.
    fn blank(dpi: u32) -> Self {
        Self::new(RasterBuffer::new(RgbaImage::new(1, 1), dpi))
    }
}

impl Layer {
    /// Create a new layer from a loaded image
    pub fn new(
        id: LayerId,
        name: String,
        source_path: PathBuf,
        image: RasterBuffer,
        is_reference: bool,
    ) -> Self {
        Self {
            id,
            name,
            source_path,
            original: std::sync::Arc::new(image),
            aligned: None,
            homography_matrix: None,
            visible: true,
            opacity: 1.0,
            blend_color: LayerColor::default_for_index(id.inner() as usize),
            is_reference,
            alignment_confidence: None,
            annotations: Vec::new(),
            offset_x: 0.0,
            offset_y: 0.0,
            pages: Vec::new(),
            active_page: 0,
        }
    }

    /// Create a document layer from its pages; page 0 is shown first.
    pub fn with_pages(
        id: LayerId,
        name: String,
        source_path: PathBuf,
        pages: Vec<RasterBuffer>,
        is_reference: bool,
    ) -> Self {
        let mut pages = pages.into_iter();
        let first = pages
            .next()
            .unwrap_or_else(|| RasterBuffer::new(RgbaImage::new(1, 1), crate::DEFAULT_RENDER_DPI));
        let mut layer = Self::new(id, name, source_path, first, is_reference);
        let rest: Vec<LayerPage> = pages.map(LayerPage::new).collect();
        if !rest.is_empty() {
            layer.pages = std::iter::once(layer.page(0)).chain(rest).collect();
        }
        layer
    }

    /// Number of pages in the document.
    pub fn page_count(&self) -> usize {
        self.pages.len().max(1)
    }

    /// Whether the document has the given page (0-indexed).
    pub fn has_page(&self, page: usize) -> bool {
        page < self.page_count()
    }

    /// Whether the shown page is a blank stand-in for a missing page.
    pub fn is_page_missing(&self) -> bool {
        !self.has_page(self.active_page)
    }

    /// Current state of a page, including the active one.
    pub fn page(&self, page: usize) -> LayerPage {
        if page == self.active_page || self.pages.is_empty() {
            LayerPage {
                original: self.original.clone(),
                aligned: self.aligned.clone(),
                homography_matrix: self.homography_matrix,
                alignment_confidence: self.alignment_confidence,
                annotations: self.annotations.clone(),
                offset_x: self.offset_x,
                offset_y: self.offset_y,
            }
        } else {
            self.pages[page].clone()
        }
    }

    /// Show another page, keeping the state of the current one.
    /// A page the document does not have is shown as a blank stand-in.
    pub fn show_page(&mut self, page: usize) {
        if page == self.active_page {
            return;
        }
        if self.pages.is_empty() {
            // Single-page documents keep their page in a slot while blank.
            self.pages.push(self.page(0));
        }
        let previous = self.active_page;
        let mut current = if self.has_page(page) {
            std::mem::replace(&mut self.pages[page], LayerPage::blank(self.original.dpi))
        } else {
            LayerPage::blank(self.original.dpi)
        };
        self.swap_page_fields(&mut current);
        if self.has_page(previous) {
            self.pages[previous] = current;
        }
        self.active_page = page;
        if self.pages.len() == 1 && self.active_page == 0 {
            self.pages.clear();
        }
    }

    /// Replace all pages and show `page` (blank if the document is shorter).
    pub fn set_pages(&mut self, pages: Vec<LayerPage>, page: usize) {
        if pages.is_empty() {
            return;
        }
        let mut shown = pages
            .get(page)
            .cloned()
            .unwrap_or_else(|| LayerPage::blank(pages[0].original.dpi));
        self.swap_page_fields(&mut shown);
        self.active_page = page;
        self.pages = if pages.len() == 1 && page == 0 {
            Vec::new()
        } else {
            pages
        };
    }

    fn swap_page_fields(&mut self, page: &mut LayerPage) {
        std::mem::swap(&mut self.original, &mut page.original);
        std::mem::swap(&mut self.aligned, &mut page.aligned);
        std::mem::swap(&mut self.homography_matrix, &mut page.homography_matrix);
        std::mem::swap(
            &mut self.alignment_confidence,
            &mut page.alignment_confidence,
        );
        std::mem::swap(&mut self.annotations, &mut page.annotations);
        std::mem::swap(&mut self.offset_x, &mut page.offset_x);
        std::mem::swap(&mut self.offset_y, &mut page.offset_y);
    }

    /// Cheap immutable snapshot for background processing and undo history.
    pub fn active_buffer(&self) -> std::sync::Arc<RasterBuffer> {
        self.aligned.as_ref().unwrap_or(&self.original).clone()
    }

    /// Get the active image (aligned if available, otherwise original)
    pub fn active_image(&self) -> &RasterBuffer {
        self.aligned.as_ref().unwrap_or(&self.original)
    }
}

/// Color used for layer blending in difference views
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LayerColor {
    /// Red component (0-255)
    pub r: u8,
    /// Green component (0-255)
    pub g: u8,
    /// Blue component (0-255)
    pub b: u8,
}

impl LayerColor {
    /// Create a new color
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// Standard colors for layers (reference=red, first target=green, etc.)
    pub fn default_for_index(index: usize) -> Self {
        const COLORS: [LayerColor; 10] = [
            LayerColor::new(232, 92, 82),
            LayerColor::new(78, 145, 218),
            LayerColor::new(90, 170, 138),
            LayerColor::new(190, 149, 79),
            LayerColor::new(165, 125, 201),
            LayerColor::new(85, 166, 179),
            LayerColor::new(210, 132, 85),
            LayerColor::new(135, 142, 213),
            LayerColor::new(141, 170, 95),
            LayerColor::new(202, 126, 159),
        ];
        COLORS[index % COLORS.len()]
    }

    /// Convert to RGBA array
    pub fn to_rgba(&self) -> [u8; 4] {
        [self.r, self.g, self.b, 255]
    }
}

// =============================================================================
// Raster Buffer Types
// =============================================================================

/// A rasterized image buffer with metadata.
/// This is the standard format for all internal image operations.
#[derive(Debug, Clone)]
pub struct RasterBuffer {
    /// The actual pixel data in RGBA format
    pub image: RgbaImage,

    /// Original width before any transforms
    pub original_width: u32,

    /// Original height before any transforms
    pub original_height: u32,

    /// DPI at which the image was rasterized (relevant for PDFs)
    pub dpi: u32,

    /// Page number if from a multi-page document (0-indexed)
    pub page_index: Option<usize>,
}

impl RasterBuffer {
    /// Create a new RasterBuffer from an RgbaImage
    pub fn new(image: RgbaImage, dpi: u32) -> Self {
        let width = image.width();
        let height = image.height();
        Self {
            image,
            original_width: width,
            original_height: height,
            dpi,
            page_index: None,
        }
    }

    /// Create from a DynamicImage (converts to RGBA)
    pub fn from_dynamic(image: DynamicImage, dpi: u32) -> Self {
        Self::new(image.to_rgba8(), dpi)
    }

    /// Get image dimensions as (width, height)
    pub fn dimensions(&self) -> (u32, u32) {
        (self.image.width(), self.image.height())
    }

    /// Get total pixel count
    pub fn pixel_count(&self) -> u64 {
        self.image.width() as u64 * self.image.height() as u64
    }

    /// Check if buffer is empty (zero dimensions)
    pub fn is_empty(&self) -> bool {
        self.image.width() == 0 || self.image.height() == 0
    }
}

// =============================================================================
// Viewport Types (for synchronized pan/zoom)
// =============================================================================

/// Represents the virtual camera view for synchronized pan/zoom.
/// All coordinate transforms go through this for consistent rendering.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Viewport {
    /// Center point of the view in image coordinates
    /// Center point of the view in image coordinates (X)
    pub center_x: f64,
    /// Center point of the view in image coordinates (Y)
    pub center_y: f64,

    /// Zoom level (1.0 = 100%, 2.0 = 200%, etc.)
    pub zoom: f64,

    /// Rotation in radians (for future use)
    pub rotation: f64,
}

impl Default for Viewport {
    fn default() -> Self {
        Self {
            center_x: 0.0,
            center_y: 0.0,
            zoom: 1.0,
            rotation: 0.0,
        }
    }
}

impl Viewport {
    /// Apply zoom (multiplicative)
    pub fn apply_zoom(&mut self, factor: f64) {
        // Clamp zoom to reasonable bounds
        self.zoom = (self.zoom * factor).clamp(0.01, 100.0);
    }

    /// Pan by delta in screen coordinates
    pub fn pan(&mut self, dx: f64, dy: f64) {
        // Convert screen delta to image coordinates (inverse of zoom)
        self.center_x += dx / self.zoom;
        self.center_y += dy / self.zoom;
    }

    /// Transform image coordinates to screen coordinates
    pub fn image_to_screen(&self, x: f64, y: f64, screen_center: (f64, f64)) -> (f64, f64) {
        let sx = (x - self.center_x) * self.zoom + screen_center.0;
        let sy = (y - self.center_y) * self.zoom + screen_center.1;
        (sx, sy)
    }

    /// Transform screen coordinates to image coordinates
    pub fn screen_to_image(&self, sx: f64, sy: f64, screen_center: (f64, f64)) -> (f64, f64) {
        let x = (sx - screen_center.0) / self.zoom + self.center_x;
        let y = (sy - screen_center.1) / self.zoom + self.center_y;
        (x, y)
    }

    /// Reset to default view
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Fit to show the entire image
    pub fn fit_to_image(
        &mut self,
        image_width: u32,
        image_height: u32,
        screen_width: f64,
        screen_height: f64,
    ) {
        // Center on the image
        self.center_x = image_width as f64 / 2.0;
        self.center_y = image_height as f64 / 2.0;

        // Calculate zoom to fit
        let zoom_x = screen_width / image_width as f64;
        let zoom_y = screen_height / image_height as f64;
        self.zoom = zoom_x.min(zoom_y) * 0.95; // 95% to leave some margin
    }
}

// =============================================================================
// Feature/Keypoint Types
// =============================================================================

/// Represents a detected feature keypoint in an image
#[derive(Debug, Clone, Copy)]
pub struct Keypoint {
    /// X coordinate in image space
    pub x: f32,
    /// Y coordinate in image space
    pub y: f32,
    /// Size/scale of the feature
    pub size: f32,
    /// Orientation angle in radians
    pub angle: f32,
    /// Response strength (higher = more confident)
    pub response: f32,
    /// Octave (scale-space level)
    pub octave: i32,
}

/// A matched pair of keypoints between two images
#[derive(Debug, Clone, Copy)]
pub struct KeypointMatch {
    /// Index in the reference image's keypoint list
    pub reference_idx: usize,
    /// Index in the target image's keypoint list
    pub target_idx: usize,
    /// Match distance (lower = better match)
    pub distance: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_layer_id_display() {
        let id = LayerId::new(42);
        assert_eq!(format!("{}", id), "Layer-42");
    }

    #[test]
    fn test_viewport_zoom_clamp() {
        let mut vp = Viewport::default();
        vp.apply_zoom(0.001); // Try to zoom out too far
        assert!(vp.zoom >= 0.01);

        vp.zoom = 1.0;
        vp.apply_zoom(1000.0); // Try to zoom in too far
        assert!(vp.zoom <= 100.0);
    }

    #[test]
    fn test_viewport_coordinate_transform_roundtrip() {
        let vp = Viewport {
            center_x: 100.0,
            center_y: 100.0,
            zoom: 2.0,
            rotation: 0.0,
        };
        let screen_center = (400.0, 300.0);

        let (ix, iy) = (150.0, 150.0);
        let (sx, sy) = vp.image_to_screen(ix, iy, screen_center);
        let (ix2, iy2) = vp.screen_to_image(sx, sy, screen_center);

        assert!((ix - ix2).abs() < 1e-10);
        assert!((iy - iy2).abs() < 1e-10);
    }

    #[test]
    fn test_layer_pages_keep_their_state() {
        let page = |shade: u8| {
            RasterBuffer::new(
                RgbaImage::from_pixel(2, 2, image::Rgba([shade, 0, 0, 255])),
                300,
            )
        };
        let mut layer = Layer::with_pages(
            LayerId::new(0),
            "sheet".into(),
            PathBuf::from("sheet.tif"),
            vec![page(10), page(20)],
            true,
        );
        assert_eq!(layer.page_count(), 2);
        layer.offset_x = 5.0;
        layer.show_page(1);
        assert_eq!(layer.original.image.get_pixel(0, 0)[0], 20);
        assert_eq!(layer.offset_x, 0.0);
        layer.offset_x = 7.0;
        layer.show_page(2);
        assert!(layer.is_page_missing());
        assert_eq!(layer.original.dimensions(), (1, 1));
        layer.show_page(0);
        assert_eq!(layer.original.image.get_pixel(0, 0)[0], 10);
        assert_eq!(layer.offset_x, 5.0);
        assert_eq!(layer.page(1).offset_x, 7.0);

        let mut single = Layer::new(
            LayerId::new(1),
            "single".into(),
            PathBuf::from("single.png"),
            page(30),
            false,
        );
        single.offset_y = 3.0;
        single.show_page(1);
        assert!(single.is_page_missing());
        single.show_page(0);
        assert_eq!(single.original.image.get_pixel(0, 0)[0], 30);
        assert_eq!(single.offset_y, 3.0);
        assert!(single.pages.is_empty());
    }

    #[test]
    fn test_layer_color_defaults() {
        assert_eq!(
            LayerColor::default_for_index(0),
            LayerColor::new(232, 92, 82)
        );
        assert_eq!(
            LayerColor::default_for_index(1),
            LayerColor::new(78, 145, 218)
        );
        assert_ne!(
            LayerColor::default_for_index(0),
            LayerColor::default_for_index(1)
        );
    }
}
