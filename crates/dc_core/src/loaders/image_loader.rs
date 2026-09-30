// =============================================================================
// dc_core/loaders/image_loader - Raster Image Loader
// =============================================================================
// Handles loading of standard raster image formats: PNG, JPEG, TIFF, BMP.
// Uses the `image` crate for decoding.
// =============================================================================

use super::traits::{DocumentLoader, LoadConfig, LoaderCapabilities, LoaderMetadata};
use crate::types::{CoreError, CoreResult, RasterBuffer};
use image::{GenericImageView, ImageFormat, ImageReader};
use std::fs;
use std::path::Path;
use tracing::{debug, instrument};

/// Loader for raster image formats (PNG, JPEG, TIFF, BMP).
///
/// # Supported Formats
/// - PNG (8-bit, 16-bit, with alpha)
/// - JPEG (baseline, progressive)
/// - TIFF (common variants)
/// - BMP (uncompressed, RLE)
///
/// # Notes
/// - All images are converted to RGBA8 internally
/// - Transparency is preserved for PNG/TIFF
/// - JPEG has no alpha channel (fully opaque)
#[derive(Debug, Default)]
pub struct ImageLoader;

impl ImageLoader {
    /// Create a new ImageLoader instance.
    pub fn new() -> Self {
        Self
    }

    /// Get the ImageFormat from a file extension.
    fn format_from_extension(ext: &str) -> Option<ImageFormat> {
        match ext.to_lowercase().as_str() {
            "png" => Some(ImageFormat::Png),
            "jpg" | "jpeg" => Some(ImageFormat::Jpeg),
            "tiff" | "tif" => Some(ImageFormat::Tiff),
            "bmp" => Some(ImageFormat::Bmp),
            "gif" => Some(ImageFormat::Gif),
            "webp" => Some(ImageFormat::WebP),
            _ => None,
        }
    }

    /// Get format info string for metadata.
    fn format_info(format: ImageFormat, img: &image::DynamicImage) -> String {
        let color_type = match img {
            image::DynamicImage::ImageLuma8(_) => "Grayscale 8-bit",
            image::DynamicImage::ImageLumaA8(_) => "Grayscale+Alpha 8-bit",
            image::DynamicImage::ImageRgb8(_) => "RGB 8-bit",
            image::DynamicImage::ImageRgba8(_) => "RGBA 8-bit",
            image::DynamicImage::ImageLuma16(_) => "Grayscale 16-bit",
            image::DynamicImage::ImageLumaA16(_) => "Grayscale+Alpha 16-bit",
            image::DynamicImage::ImageRgb16(_) => "RGB 16-bit",
            image::DynamicImage::ImageRgba16(_) => "RGBA 16-bit",
            image::DynamicImage::ImageRgb32F(_) => "RGB 32-bit float",
            image::DynamicImage::ImageRgba32F(_) => "RGBA 32-bit float",
            _ => "Unknown",
        };

        let format_name = match format {
            ImageFormat::Png => "PNG",
            ImageFormat::Jpeg => "JPEG",
            ImageFormat::Tiff => "TIFF",
            ImageFormat::Bmp => "BMP",
            ImageFormat::Gif => "GIF",
            ImageFormat::WebP => "WebP",
            _ => "Unknown",
        };

        format!("{} ({})", format_name, color_type)
    }
    fn process_image(
        &self,
        dynamic_image: image::DynamicImage,
        config: &LoadConfig,
    ) -> CoreResult<RasterBuffer> {
        let (width, height) = dynamic_image.dimensions();
        debug!(width, height, "Image decoded successfully");

        // Check dimensions against limits
        if let Some(max_dim) = config.max_dimension {
            if width > max_dim || height > max_dim {
                // Scale down to fit within max dimension while preserving aspect ratio
                let scale = max_dim as f64 / width.max(height) as f64;
                let new_width = (width as f64 * scale) as u32;
                let new_height = (height as f64 * scale) as u32;

                debug!(
                    original_width = width,
                    original_height = height,
                    new_width,
                    new_height,
                    "Scaling down image to fit max dimension"
                );

                let resized = dynamic_image.resize(
                    new_width,
                    new_height,
                    image::imageops::FilterType::Lanczos3,
                );

                let rgba = resized.to_rgba8();
                let mut buffer = RasterBuffer::new(rgba, config.dpi);
                buffer.original_width = width;
                buffer.original_height = height;
                return Ok(buffer);
            }
        }

        // Convert to RGBA8
        let rgba = dynamic_image.to_rgba8();
        Ok(RasterBuffer::new(rgba, config.dpi))
    }
}

impl DocumentLoader for ImageLoader {
    fn can_load(&self, path: &Path) -> bool {
        path.extension()
            .and_then(|e| e.to_str())
            .map(|e| Self::format_from_extension(e).is_some())
            .unwrap_or(false)
    }

    fn supported_extensions(&self) -> &[&str] {
        &["png", "jpg", "jpeg", "tiff", "tif", "bmp", "gif", "webp"]
    }

    fn capabilities(&self) -> LoaderCapabilities {
        LoaderCapabilities {
            supports_dpi_scaling: false, // Raster images have fixed resolution
            supports_pages: false,
            supports_text_extraction: false,
            supports_layers: false,
            supports_alpha: true,
        }
    }

    #[instrument(skip(self, config), fields(path = %path.display()))]
    #[instrument(skip(self, config), fields(path = %path.display()))]
    fn load(&self, path: &Path, config: &LoadConfig) -> CoreResult<RasterBuffer> {
        debug!("Loading image file");

        // Verify file exists
        if !path.exists() {
            return Err(CoreError::FileNotFound {
                path: path.to_path_buf(),
            });
        }

        // Open and decode the image
        let reader = ImageReader::open(path).map_err(|e| CoreError::FileReadError {
            path: path.to_path_buf(),
            source: e.into(),
        })?;

        // Try to determine format from file contents (magic bytes)
        let reader = reader
            .with_guessed_format()
            .map_err(|e| CoreError::FileReadError {
                path: path.to_path_buf(),
                source: e,
            })?;

        // Remove default memory limits — engineering drawings can be very large
        let mut reader = reader;
        reader.no_limits();

        let dynamic_image = reader.decode().map_err(|e| CoreError::ImageDecodeError {
            reason: e.to_string(),
        })?;

        self.process_image(dynamic_image, config)
    }

    fn load_from_memory(
        &self,
        data: &[u8],
        _name_hint: &str,
        config: &LoadConfig,
    ) -> CoreResult<RasterBuffer> {
        debug!("Loading image from memory ({} bytes)", data.len());

        let cursor = std::io::Cursor::new(data);
        let reader = ImageReader::new(cursor)
            .with_guessed_format()
            .map_err(|e| CoreError::ImageDecodeError {
                reason: format!("Failed to guess format: {}", e),
            })?;

        // Remove default memory limits — engineering drawings can be very large
        let mut reader = reader;
        reader.no_limits();

        let dynamic_image = reader.decode().map_err(|e| CoreError::ImageDecodeError {
            reason: e.to_string(),
        })?;

        self.process_image(dynamic_image, config)
    }

    #[instrument(skip(self), fields(path = %path.display()))]
    fn get_metadata(&self, path: &Path) -> CoreResult<LoaderMetadata> {
        if !path.exists() {
            return Err(CoreError::FileNotFound {
                path: path.to_path_buf(),
            });
        }

        // Get file size
        let file_size = fs::metadata(path)
            .map_err(|e| CoreError::FileReadError {
                path: path.to_path_buf(),
                source: e,
            })?
            .len();

        let filename = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();

        let extension = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        // We need to decode to get dimensions and format info
        // This is somewhat expensive but necessary for accurate metadata
        let reader = ImageReader::open(path).map_err(|e| CoreError::FileReadError {
            path: path.to_path_buf(),
            source: e.into(),
        })?;

        let reader = reader
            .with_guessed_format()
            .map_err(|e| CoreError::FileReadError {
                path: path.to_path_buf(),
                source: e,
            })?;

        let format = reader.format();
        let dynamic_image = reader.decode().map_err(|e| CoreError::ImageDecodeError {
            reason: e.to_string(),
        })?;

        let (width, height) = dynamic_image.dimensions();
        let format_info = format
            .map(|f| Self::format_info(f, &dynamic_image))
            .unwrap_or_else(|| "Unknown format".to_string());

        Ok(LoaderMetadata {
            filename,
            extension,
            file_size,
            page_count: 1,
            native_width: Some(width),
            native_height: Some(height),
            is_vector: false,
            format_info,
        })
    }

    fn name(&self) -> &str {
        "Image Loader (PNG, JPEG, TIFF, BMP)"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_supported_extensions() {
        let loader = ImageLoader::new();
        let exts = loader.supported_extensions();

        assert!(exts.contains(&"png"));
        assert!(exts.contains(&"jpg"));
        assert!(exts.contains(&"jpeg"));
        assert!(exts.contains(&"tiff"));
        assert!(exts.contains(&"bmp"));
    }

    #[test]
    fn test_can_load() {
        let loader = ImageLoader::new();

        assert!(loader.can_load(Path::new("test.png")));
        assert!(loader.can_load(Path::new("test.PNG")));
        assert!(loader.can_load(Path::new("test.jpg")));
        assert!(loader.can_load(Path::new("test.JPEG")));
        assert!(!loader.can_load(Path::new("test.pdf")));
        assert!(!loader.can_load(Path::new("test.dwg")));
    }

    #[test]
    fn test_capabilities() {
        let loader = ImageLoader::new();
        let caps = loader.capabilities();

        assert!(!caps.supports_dpi_scaling);
        assert!(!caps.supports_pages);
        assert!(caps.supports_alpha);
    }

    #[test]
    fn test_format_from_extension() {
        assert!(ImageLoader::format_from_extension("png").is_some());
        assert!(ImageLoader::format_from_extension("PNG").is_some());
        assert!(ImageLoader::format_from_extension("pdf").is_none());
    }
}
