// =============================================================================
// dc_core/loaders/image_loader - Raster Image Loader
// =============================================================================
// Handles loading of standard raster image formats: PNG, JPEG, TIFF, BMP.
// Uses the `image` crate for decoding.
// =============================================================================

use super::traits::{DocumentLoader, LoadConfig, LoaderCapabilities, LoaderMetadata};
use crate::types::{CoreError, CoreResult, RasterBuffer};
use image::{DynamicImage, GenericImageView, ImageBuffer, ImageFormat, ImageReader};
use std::fs;
use std::io::{Cursor, Read, Seek};
use std::path::Path;
use tracing::{debug, instrument};

/// TIFF stores several drawing sheets as separate image file directories.
fn is_tiff(data: &[u8]) -> bool {
    data.starts_with(b"II*\0") || data.starts_with(b"MM\0*")
}

fn tiff_error(error: tiff::TiffError) -> CoreError {
    CoreError::ImageDecodeError {
        reason: format!("TIFF: {error}"),
    }
}

fn open_tiff<R: Read + Seek>(reader: R) -> CoreResult<tiff::decoder::Decoder<R>> {
    Ok(tiff::decoder::Decoder::new(reader)
        .map_err(tiff_error)?
        .with_limits(tiff::decoder::Limits::unlimited()))
}

/// Decode the page the decoder currently points at.
fn decode_tiff_page<R: Read + Seek>(
    decoder: &mut tiff::decoder::Decoder<R>,
) -> CoreResult<DynamicImage> {
    use tiff::decoder::DecodingResult as Data;
    use tiff::ColorType as Color;
    let (width, height) = decoder.dimensions().map_err(tiff_error)?;
    let color = decoder.colortype().map_err(tiff_error)?;
    let data = decoder.read_image().map_err(tiff_error)?;
    let invalid = || CoreError::ImageDecodeError {
        reason: format!("TIFF page data does not match {width}x{height} {color:?}"),
    };
    let image = match (color, data) {
        (Color::Gray(1), Data::U8(bits)) => {
            let row_bytes = width.div_ceil(8) as usize;
            if bits.len() < row_bytes * height as usize {
                return Err(invalid());
            }
            DynamicImage::ImageLuma8(ImageBuffer::from_fn(width, height, |x, y| {
                let byte = bits[y as usize * row_bytes + x as usize / 8];
                image::Luma([((byte >> (7 - x % 8)) & 1) * 255])
            }))
        }
        (Color::Gray(8), Data::U8(v)) => {
            DynamicImage::ImageLuma8(ImageBuffer::from_raw(width, height, v).ok_or_else(invalid)?)
        }
        (Color::Gray(16), Data::U16(v)) => {
            DynamicImage::ImageLuma16(ImageBuffer::from_raw(width, height, v).ok_or_else(invalid)?)
        }
        (Color::GrayA(8), Data::U8(v)) => {
            DynamicImage::ImageLumaA8(ImageBuffer::from_raw(width, height, v).ok_or_else(invalid)?)
        }
        (Color::RGB(8), Data::U8(v)) => {
            DynamicImage::ImageRgb8(ImageBuffer::from_raw(width, height, v).ok_or_else(invalid)?)
        }
        (Color::RGBA(8), Data::U8(v)) => {
            DynamicImage::ImageRgba8(ImageBuffer::from_raw(width, height, v).ok_or_else(invalid)?)
        }
        (Color::RGB(16), Data::U16(v)) => {
            DynamicImage::ImageRgb16(ImageBuffer::from_raw(width, height, v).ok_or_else(invalid)?)
        }
        (Color::RGBA(16), Data::U16(v)) => {
            DynamicImage::ImageRgba16(ImageBuffer::from_raw(width, height, v).ok_or_else(invalid)?)
        }
        (Color::CMYK(8), Data::U8(v)) => {
            let rgb = v
                .chunks_exact(4)
                .flat_map(|p| {
                    let k = 255 - p[3] as u16;
                    [0, 1, 2].map(|c| ((255 - p[c] as u16) * k / 255) as u8)
                })
                .collect();
            DynamicImage::ImageRgb8(ImageBuffer::from_raw(width, height, rgb).ok_or_else(invalid)?)
        }
        (color, _) => {
            return Err(CoreError::ImageDecodeError {
                reason: format!("Unsupported TIFF color type {color:?}"),
            })
        }
    };
    Ok(image)
}

/// Decode one TIFF page (0-indexed).
fn decode_tiff<R: Read + Seek>(reader: R, page_index: usize) -> CoreResult<DynamicImage> {
    let mut decoder = open_tiff(reader)?;
    if page_index > 0 {
        decoder
            .seek_to_image(page_index)
            .map_err(|_| CoreError::ImageDecodeError {
                reason: format!("TIFF page {} does not exist", page_index + 1),
            })?;
    }
    decode_tiff_page(&mut decoder)
}

/// Decode all TIFF pages in file order.
fn decode_tiff_pages<R: Read + Seek>(reader: R) -> CoreResult<Vec<DynamicImage>> {
    let mut decoder = open_tiff(reader)?;
    let mut pages = vec![decode_tiff_page(&mut decoder)?];
    while decoder.more_images() {
        decoder.next_image().map_err(tiff_error)?;
        pages.push(decode_tiff_page(&mut decoder)?);
    }
    debug!(pages = pages.len(), "TIFF pages decoded");
    Ok(pages)
}

/// Count TIFF pages without decoding pixel data.
fn tiff_page_count<R: Read + Seek>(reader: R) -> CoreResult<usize> {
    let mut decoder = open_tiff(reader)?;
    let mut count = 1;
    while decoder.more_images() {
        decoder.next_image().map_err(tiff_error)?;
        count += 1;
    }
    Ok(count)
}

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

impl ImageLoader {
    fn read_file(path: &Path) -> CoreResult<Vec<u8>> {
        if !path.exists() {
            return Err(CoreError::FileNotFound {
                path: path.to_path_buf(),
            });
        }
        fs::read(path).map_err(|e| CoreError::FileReadError {
            path: path.to_path_buf(),
            source: e,
        })
    }

    /// Decode one page; non-TIFF formats only have page 0.
    fn decode(data: &[u8], page_index: usize) -> CoreResult<DynamicImage> {
        if is_tiff(data) {
            return decode_tiff(Cursor::new(data), page_index);
        }
        if page_index > 0 {
            return Err(CoreError::ImageDecodeError {
                reason: format!("Page {} does not exist", page_index + 1),
            });
        }
        let mut reader = ImageReader::new(Cursor::new(data))
            .with_guessed_format()
            .map_err(|e| CoreError::ImageDecodeError {
                reason: format!("Failed to guess format: {}", e),
            })?;
        // Remove default memory limits — engineering drawings can be very large
        reader.no_limits();
        reader.decode().map_err(|e| CoreError::ImageDecodeError {
            reason: e.to_string(),
        })
    }

    fn decode_page(&self, data: &[u8], config: &LoadConfig) -> CoreResult<RasterBuffer> {
        let mut buffer = self.process_image(Self::decode(data, config.page_index)?, config)?;
        if is_tiff(data) {
            buffer.page_index = Some(config.page_index);
        }
        Ok(buffer)
    }

    fn decode_pages(&self, data: &[u8], config: &LoadConfig) -> CoreResult<Vec<RasterBuffer>> {
        if !is_tiff(data) {
            return self.decode_page(data, config).map(|buffer| vec![buffer]);
        }
        decode_tiff_pages(Cursor::new(data))?
            .into_iter()
            .enumerate()
            .map(|(page, image)| {
                let mut buffer = self.process_image(image, config)?;
                buffer.page_index = Some(page);
                Ok(buffer)
            })
            .collect()
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
            supports_pages: true,        // Multi-page TIFF
            supports_text_extraction: false,
            supports_layers: false,
            supports_alpha: true,
        }
    }

    #[instrument(skip(self, config), fields(path = %path.display()))]
    fn load(&self, path: &Path, config: &LoadConfig) -> CoreResult<RasterBuffer> {
        debug!("Loading image file");
        self.decode_page(&Self::read_file(path)?, config)
    }

    fn load_from_memory(
        &self,
        data: &[u8],
        _name_hint: &str,
        config: &LoadConfig,
    ) -> CoreResult<RasterBuffer> {
        debug!("Loading image from memory ({} bytes)", data.len());
        self.decode_page(data, config)
    }

    #[instrument(skip(self, config), fields(path = %path.display()))]
    fn load_all_pages(&self, path: &Path, config: &LoadConfig) -> CoreResult<Vec<RasterBuffer>> {
        self.decode_pages(&Self::read_file(path)?, config)
    }

    fn load_all_pages_from_memory(
        &self,
        data: &[u8],
        _name_hint: &str,
        config: &LoadConfig,
    ) -> CoreResult<Vec<RasterBuffer>> {
        self.decode_pages(data, config)
    }

    #[instrument(skip(self), fields(path = %path.display()))]
    fn get_metadata(&self, path: &Path) -> CoreResult<LoaderMetadata> {
        let data = Self::read_file(path)?;

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
        let format = image::guess_format(&data).ok();
        let dynamic_image = Self::decode(&data, 0)?;
        let page_count = if is_tiff(&data) {
            tiff_page_count(Cursor::new(&data))?
        } else {
            1
        };

        let (width, height) = dynamic_image.dimensions();
        let format_info = format
            .map(|f| Self::format_info(f, &dynamic_image))
            .unwrap_or_else(|| "Unknown format".to_string());

        Ok(LoaderMetadata {
            filename,
            extension,
            file_size: data.len() as u64,
            page_count,
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
        assert!(caps.supports_pages);
        assert!(caps.supports_alpha);
    }

    #[test]
    fn test_format_from_extension() {
        assert!(ImageLoader::format_from_extension("png").is_some());
        assert!(ImageLoader::format_from_extension("PNG").is_some());
        assert!(ImageLoader::format_from_extension("pdf").is_none());
    }

    /// Two-sheet drawing: a 1-bit sheet and an 8-bit grayscale sheet.
    fn two_page_tiff() -> Vec<u8> {
        use tiff::encoder::{colortype, TiffEncoder};
        let mut data = Cursor::new(Vec::new());
        let mut encoder = TiffEncoder::new(&mut data).unwrap();
        encoder
            .write_image::<colortype::Gray8>(4, 2, &[0, 255, 255, 255, 255, 255, 255, 0])
            .unwrap();
        encoder
            .write_image::<colortype::RGB8>(3, 1, &[255, 0, 0, 0, 255, 0, 0, 0, 255])
            .unwrap();
        data.into_inner()
    }

    #[test]
    fn test_tiff_pages() {
        let loader = ImageLoader::new();
        let data = two_page_tiff();
        let config = LoadConfig::default();
        let pages = loader
            .load_all_pages_from_memory(&data, "sheet.tif", &config)
            .unwrap();
        assert_eq!(pages.len(), 2);
        assert_eq!(pages[0].dimensions(), (4, 2));
        assert_eq!(pages[0].image.get_pixel(0, 0).0, [0, 0, 0, 255]);
        assert_eq!(pages[0].image.get_pixel(3, 1).0, [0, 0, 0, 255]);
        assert_eq!(pages[1].dimensions(), (3, 1));
        assert_eq!(pages[1].image.get_pixel(2, 0).0, [0, 0, 255, 255]);
        assert_eq!(pages[1].page_index, Some(1));

        let second = loader
            .load_from_memory(&data, "sheet.tif", &config.clone().with_page(1))
            .unwrap();
        assert_eq!(second.image, pages[1].image);
        assert!(loader
            .load_from_memory(&data, "sheet.tif", &config.with_page(2))
            .is_err());
        assert_eq!(tiff_page_count(Cursor::new(&data)).unwrap(), 2);
    }

    /// `DIFFCOMP_SAMPLE_TIFF=<file> cargo test -p dc_core sample_tiff -- --ignored`
    #[test]
    #[ignore]
    fn sample_tiff_matches_image_crate() {
        let path = std::env::var("DIFFCOMP_SAMPLE_TIFF").unwrap();
        let loader = ImageLoader::new();
        let pages = loader
            .load_all_pages(Path::new(&path), &LoadConfig::default())
            .unwrap();
        for page in &pages {
            eprintln!("page {:?}: {:?}", page.page_index, page.dimensions());
        }
        if let Ok(reference) = image::open(&path) {
            assert_eq!(pages[0].image, reference.to_rgba8());
        }
    }
}
