// =============================================================================
// dc_core/loaders - Document Loader Module (v2: Pure Rust)
// =============================================================================
// Provides a trait-based document loading architecture.
//
// v2 CHANGES:
//   ✅ REMOVED: pdf_loader (pdfium-render system dependency)
//   ✅ ADDED:   pdf_loader (pure Rust via printpdf + resvg)
//   ✅ KEPT:    image_loader (pure Rust via `image` crate)
//   ✅ KEPT:    registry (loader discovery & unified interface)
//   ✅ KEPT:    traits (DocumentLoader trait for extensibility)
// =============================================================================

/// Image loading implementation
pub mod image_loader;
/// PDF loading implementation (pure Rust)
pub mod pdf_loader;
/// Loader registry
pub mod registry;
/// Loader traits
pub mod traits;

pub use image_loader::ImageLoader;
pub use pdf_loader::{load_pdf_all_pages, PdfLoader, DEFAULT_PDF_DPI};
pub use registry::{LoaderInfo, LoaderRegistry};
pub use traits::{DocumentLoader, LoadConfig, LoaderCapabilities, LoaderMetadata};
