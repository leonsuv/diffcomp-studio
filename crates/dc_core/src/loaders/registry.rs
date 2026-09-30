// =============================================================================
// dc_core/loaders/registry - Loader Registry
// =============================================================================
// Central registry for all document loaders.
// Allows dynamic loader discovery and unified file loading interface.
// =============================================================================

use super::traits::{DocumentLoader, LoadConfig, LoaderMetadata};
use super::ImageLoader;
use super::PdfLoader;
use crate::types::{CoreError, CoreResult, RasterBuffer};
use std::path::Path;
use std::sync::Arc;
use tracing::{debug, instrument};

/// Registry for document loaders.
///
/// The registry maintains a list of available loaders and provides
/// a unified interface for loading any supported document type.
///
/// # Usage
///
/// ```ignore
/// let registry = LoaderRegistry::with_defaults();
/// let buffer = registry.load("document.pdf", &LoadConfig::default())?;
/// ```
///
/// # Extensibility
///
/// Custom loaders can be registered for new file formats:
///
/// ```ignore
/// let mut registry = LoaderRegistry::new();
/// registry.register(Arc::new(MyCustomLoader::new()));
/// ```
#[derive(Clone)]
pub struct LoaderRegistry {
    loaders: Vec<Arc<dyn DocumentLoader>>,
}

impl Default for LoaderRegistry {
    fn default() -> Self {
        Self::with_defaults()
    }
}

impl LoaderRegistry {
    /// Create an empty registry (no loaders).
    pub fn new() -> Self {
        Self {
            loaders: Vec::new(),
        }
    }

    /// Create a registry with all default loaders registered.
    pub fn with_defaults() -> Self {
        let mut registry = Self::new();

        // Register built-in loaders
        registry.register(Arc::new(ImageLoader::new()));
        registry.register(Arc::new(PdfLoader::new()));

        debug!(
            loader_count = registry.loaders.len(),
            "Loader registry initialized with defaults"
        );

        registry
    }

    /// Register a new loader.
    ///
    /// Loaders are checked in registration order, so register
    /// more specific loaders before generic ones.
    pub fn register(&mut self, loader: Arc<dyn DocumentLoader>) {
        debug!(
            loader_name = loader.name(),
            extensions = ?loader.supported_extensions(),
            "Registering loader"
        );
        self.loaders.push(loader);
    }

    /// Find a loader that can handle the given file.
    pub fn find_loader(&self, path: &Path) -> Option<&Arc<dyn DocumentLoader>> {
        self.loaders.iter().find(|loader| loader.can_load(path))
    }

    /// Check if any loader can handle the given file.
    pub fn can_load(&self, path: &Path) -> bool {
        self.find_loader(path).is_some()
    }

    /// Get all supported file extensions across all loaders.
    pub fn supported_extensions(&self) -> Vec<&str> {
        self.loaders
            .iter()
            .flat_map(|loader| loader.supported_extensions().iter().copied())
            .collect()
    }

    /// Get a file filter string for open dialogs.
    /// Format: "PDF Files (*.pdf)|*.pdf|Image Files (*.png;*.jpg)|*.png;*.jpg"
    pub fn file_filter_string(&self) -> String {
        let mut parts = Vec::new();

        for loader in &self.loaders {
            let exts = loader.supported_extensions();
            if exts.is_empty() {
                continue;
            }

            let patterns: Vec<String> = exts.iter().map(|e| format!("*.{}", e)).collect();
            let pattern_str = patterns.join(";");
            parts.push(format!(
                "{} ({})|{}",
                loader.name(),
                pattern_str,
                pattern_str
            ));
        }

        // Add "All Supported" option at the beginning
        let all_exts: Vec<String> = self
            .supported_extensions()
            .iter()
            .map(|e| format!("*.{}", e))
            .collect();
        let all_pattern = all_exts.join(";");

        let mut result = format!("All Supported ({})|{}", all_pattern, all_pattern);
        for part in parts {
            result.push('|');
            result.push_str(&part);
        }

        result
    }

    /// Load a document from the given path.
    ///
    /// Automatically selects the appropriate loader based on file extension.
    pub fn load(&self, path: impl AsRef<Path>, config: &LoadConfig) -> CoreResult<RasterBuffer> {
        let path = path.as_ref();
        self.load_inner(path, config)
    }

    #[instrument(skip(self, config), fields(path = %path.display()))]
    fn load_inner(&self, path: &Path, config: &LoadConfig) -> CoreResult<RasterBuffer> {
        let loader = self.find_loader(path).ok_or_else(|| {
            let extension = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("unknown")
                .to_string();
            CoreError::UnsupportedFormat { extension }
        })?;

        debug!(loader = loader.name(), "Using loader");
        loader.load(path, config)
    }

    /// Load a document from memory buffer.
    #[instrument(skip(self, data, config), fields(name = %name_hint))]
    pub fn load_from_memory(
        &self,
        data: &[u8],
        name_hint: &str,
        config: &LoadConfig,
    ) -> CoreResult<RasterBuffer> {
        let path = Path::new(name_hint);
        if let Some(loader) = self.find_loader(path) {
            debug!(loader = loader.name(), "Using loader from hint");
            return loader.load_from_memory(data, name_hint, config);
        }

        // Fallback: try to guess by magic bytes?
        // For now, failure.
        Err(CoreError::UnsupportedFormat {
            extension: "unknown".into(),
        })
    }

    /// Get metadata for a document without fully loading it.
    pub fn get_metadata(&self, path: impl AsRef<Path>) -> CoreResult<LoaderMetadata> {
        let path = path.as_ref();
        self.get_metadata_inner(path)
    }

    #[instrument(skip(self), fields(path = %path.display()))]
    fn get_metadata_inner(&self, path: &Path) -> CoreResult<LoaderMetadata> {
        let loader = self.find_loader(path).ok_or_else(|| {
            let extension = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("unknown")
                .to_string();
            CoreError::UnsupportedFormat { extension }
        })?;

        loader.get_metadata(path)
    }

    /// Get the number of registered loaders.
    pub fn loader_count(&self) -> usize {
        self.loaders.len()
    }

    /// Get information about all registered loaders.
    pub fn loader_info(&self) -> Vec<LoaderInfo> {
        self.loaders
            .iter()
            .map(|loader| LoaderInfo {
                name: loader.name().to_string(),
                extensions: loader
                    .supported_extensions()
                    .iter()
                    .map(|s| s.to_string())
                    .collect(),
                capabilities: loader.capabilities(),
            })
            .collect()
    }
}

/// Information about a registered loader (for UI display).
#[derive(Debug, Clone)]
pub struct LoaderInfo {
    /// Loader name
    pub name: String,
    /// Supported file extensions
    pub extensions: Vec<String>,
    /// Loader capabilities
    pub capabilities: super::traits::LoaderCapabilities,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_registry_has_loaders() {
        let registry = LoaderRegistry::with_defaults();
        assert!(registry.loader_count() >= 1);
    }

    #[test]
    fn test_pdf_loaded() {
        let registry = LoaderRegistry::with_defaults();
        // PDF re-enabled in v2 via pure-Rust printpdf+resvg
        assert!(registry.can_load(Path::new("test.pdf")));
    }

    #[test]
    fn test_can_load_images() {
        let registry = LoaderRegistry::with_defaults();
        assert!(registry.can_load(Path::new("test.png")));
        assert!(registry.can_load(Path::new("test.jpg")));
        assert!(registry.can_load(Path::new("test.tiff")));
    }

    #[test]
    fn test_cannot_load_unknown() {
        let registry = LoaderRegistry::with_defaults();
        assert!(!registry.can_load(Path::new("test.xyz")));
        assert!(!registry.can_load(Path::new("test.dwg")));
    }

    #[test]
    fn test_supported_extensions() {
        let registry = LoaderRegistry::with_defaults();
        let exts = registry.supported_extensions();

        // PDF re-enabled in v2
        assert!(exts.contains(&"pdf"));
        assert!(exts.contains(&"png"));
        assert!(exts.contains(&"jpg"));
    }

    #[test]
    fn test_find_loader_returns_correct_type() {
        let registry = LoaderRegistry::with_defaults();

        // PDF should be found (re-enabled in v2)
        let pdf_loader = registry.find_loader(Path::new("test.pdf"));
        assert!(pdf_loader.is_some());
        assert!(pdf_loader.unwrap().capabilities().supports_dpi_scaling);

        // Image loader should work
        let png_loader = registry.find_loader(Path::new("test.png"));
        assert!(png_loader.is_some());
        assert!(!png_loader.unwrap().capabilities().supports_dpi_scaling);
    }

    #[test]
    fn test_loader_info() {
        let registry = LoaderRegistry::with_defaults();
        let info = registry.loader_info();

        assert!(!info.is_empty());
        for loader_info in &info {
            assert!(!loader_info.name.is_empty());
            assert!(!loader_info.extensions.is_empty());
        }
    }
}
