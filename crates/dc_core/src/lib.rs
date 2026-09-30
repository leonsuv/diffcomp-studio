// =============================================================================
// dc_core - Core Library (v2: Pure Rust, Zero System Dependencies)
// =============================================================================
// The core image processing engine for DiffComp Studio.
//
// ## Architecture
//
// ┌──────────────────────────────────────────────────────────────┐
// │                          dc_core                            │
// ├──────────────┬──────────────┬──────────────┬────────────────┤
// │  alignment   │     diff     │   loaders    │    types       │
// │              │              │              │                │
// │  FAST + ORB  │ CPU pixel    │ image crate  │ RasterBuffer   │
// │  RANSAC +    │ diffing      │ (PNG, JPEG,  │ Layer          │
// │  Homography  │ (heatmap,    │  TIFF, BMP)  │ Viewport       │
// │  Warp        │  overlay)    │              │ CoreError      │
// └──────────────┴──────────────┴──────────────┴────────────────┘
//
// v2: ALL OpenCV and pdfium dependencies REMOVED.
//     Everything compiles with `cargo build` — no vcpkg, no brew, no DLLs.
// =============================================================================

#![warn(clippy::all, clippy::pedantic, clippy::nursery, missing_docs)]
#![allow(
    clippy::module_name_repetitions,
    clippy::similar_names,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::cast_lossless,
    clippy::too_many_lines,
    clippy::struct_excessive_bools,
    clippy::must_use_candidate,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::return_self_not_must_use,
    clippy::future_not_send
)]

//! # dc_core — Pure Rust Image Comparison Engine
//!
//! Core library for DiffComp Studio. Handles:
//! - Feature-based image alignment (FAST + binary descriptors + RANSAC)
//! - Pixel-level image differencing (heatmap, overlay, binary mask)
//! - Document loading via extensible trait system
//!
//! ## Zero Dependencies Philosophy
//! This crate depends only on pure-Rust libraries (`image`, `rayon`, `serde`).
//! No OpenCV. No pdfium. No C/C++ interop. Builds on every platform
//! including WebAssembly.

/// Alignment algorithms (FAST, ORB-like descriptors, RANSAC)
pub mod alignment;
/// Annotation primitives and serialization
pub mod annotations;
/// Image differencing engine and heatmap generation
pub mod diff;
/// File format loaders and registry
pub mod loaders;
/// Tool definitions and state management
pub mod tools;
/// Common data types and error definitions
pub mod types;

// Re-export commonly used items at the crate root
pub use alignment::force_fit::{force_fit_align, ForceFitInfo, ForceFitResult};
pub use alignment::{
    AlignmentConfig, AlignmentEngine, AlignmentResult, FeatureDetector, FeatureDetectorConfig,
    FeatureDetectorType, HomographyMatrix, HomographySolver,
};
pub use annotations::{
    compute_dimension_chain, next_sequence_number, point_distance, polygon_area, polyline_length,
    Annotation, AnnotationData, Calibration, Point,
};
pub use diff::morphological::{compute_morphological_diff, MorphDiffResult};
pub use diff::{BlendMode, DiffConfig, DiffEngine, DiffResult};
pub use loaders::{
    load_pdf_all_pages, DocumentLoader, ImageLoader, LoadConfig, LoaderCapabilities,
    LoaderMetadata, LoaderRegistry, PdfLoader, DEFAULT_PDF_DPI,
};
pub use tools::{Tool, ToolStyle, ToolType};
pub use types::{
    CoreError, CoreResult, Keypoint, KeypointMatch, Layer, LayerColor, LayerId, RasterBuffer,
    Viewport,
};

/// Maximum number of layers allowed in a comparison session.
pub const MAX_LAYERS: usize = 8;

/// Default DPI for rendering vector documents.
pub const DEFAULT_RENDER_DPI: u32 = 300;

/// Library version string.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
