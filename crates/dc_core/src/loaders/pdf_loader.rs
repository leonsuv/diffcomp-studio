// =============================================================================
// dc_core/loaders/pdf_loader - Pure-Rust PDF Loader (via hayro)
// =============================================================================
// Loads PDF files using hayro, a pure-Rust PDF rasterizer.
// =============================================================================

use crate::types::{CoreError, CoreResult, RasterBuffer};
use image::RgbaImage;
use std::path::Path;
use std::sync::Arc;
use tracing::{debug, info, instrument, warn};

use super::traits::{DocumentLoader, LoadConfig, LoaderCapabilities, LoaderMetadata};

use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_syntax::Pdf;
use hayro::{render, RenderSettings};

/// Default DPI for PDF rasterization.
pub const DEFAULT_PDF_DPI: u32 = 300;

/// PDF document loader using pure-Rust libraries.
pub struct PdfLoader;

impl PdfLoader {
    /// Create a new PDF loader instance.
    pub fn new() -> Self {
        Self
    }

    /// Render a single PDF page to an RGBA image at the specified DPI.
    fn render_page(page: &hayro::hayro_syntax::page::Page, dpi: u32) -> CoreResult<RgbaImage> {
        // PDF points are 1/72 inch.
        let scale = dpi as f32 / 72.0;

        let mut render_settings = RenderSettings::default();
        render_settings.x_scale = scale;
        render_settings.y_scale = scale;

        let interp_settings = InterpreterSettings::default();

        let pixmap = render(page, &interp_settings, &render_settings);

        // pixmap.width() and height() might return u32 or u16 depending on version.
        // We cast to u32 to be safe for RgbaImage.
        let w = pixmap.width() as u32;
        let h = pixmap.height() as u32;

        if w == 0 || h == 0 {
            return Err(CoreError::InvalidDimensions {
                width: w,
                height: h,
                reason: "PDF page rendered to zero dimensions".into(),
            });
        }

        // Rendered pixels are premultiplied. Composite over white before handing
        // them to straight-alpha image processing; transparent PDF paper is white.
        let mut data_vec = pixmap.data_as_u8_slice().to_vec();
        for pixel in data_vec.chunks_exact_mut(4) {
            let paper = 255 - pixel[3];
            for channel in &mut pixel[..3] {
                *channel = channel.saturating_add(paper);
            }
            pixel[3] = 255;
        }

        let image =
            RgbaImage::from_raw(w, h, data_vec).ok_or_else(|| CoreError::ImageDecodeError {
                reason: format!("Failed to create RGBA image from pixmap data ({}x{})", w, h)
                    .into(),
            })?;

        Ok(image)
    }
}

impl DocumentLoader for PdfLoader {
    fn can_load(&self, path: &Path) -> bool {
        path.extension()
            .map(|e| e.to_ascii_lowercase() == "pdf")
            .unwrap_or(false)
    }

    fn supported_extensions(&self) -> &[&str] {
        &["pdf"]
    }

    fn capabilities(&self) -> LoaderCapabilities {
        LoaderCapabilities {
            supports_dpi_scaling: true,
            supports_pages: true,
            supports_text_extraction: false,
            supports_layers: false,
            supports_alpha: true,
        }
    }

    #[instrument(skip(self, config), fields(path = %path.display()))]
    fn load(&self, path: &Path, config: &LoadConfig) -> CoreResult<RasterBuffer> {
        debug!("Loading PDF file");

        let bytes = std::fs::read(path).map_err(|e| CoreError::FileReadError {
            path: path.to_path_buf(),
            source: e,
        })?;

        self.load_from_memory(&bytes, path.to_str().unwrap_or("unknown.pdf"), config)
    }

    fn load_from_memory(
        &self,
        data: &[u8],
        name_hint: &str,
        config: &LoadConfig,
    ) -> CoreResult<RasterBuffer> {
        debug!(name = name_hint, "Loading PDF from memory");

        let data_vec = data.to_vec();
        // hayro::Pdf::new returns Result<Pdf, LoadPdfError>
        let pdf = Pdf::new(Arc::new(data_vec)).map_err(|e| CoreError::ImageDecodeError {
            reason: format!("Failed to parse PDF: {:?}", e),
        })?;

        let pages = pdf.pages();
        let page_count = pages.len();
        info!(page_count = page_count, "PDF parsed successfully");

        if page_count == 0 {
            return Err(CoreError::ImageDecodeError {
                reason: "PDF has no pages".into(),
            });
        }

        if config.page_index >= page_count {
            return Err(CoreError::ImageDecodeError {
                reason: format!(
                    "PDF page {} is out of range ({} pages)",
                    config.page_index + 1,
                    page_count
                ),
            });
        }
        let page_idx = config.page_index;
        let page = &pages[page_idx];

        let rgba = Self::render_page(page, config.dpi)?;
        let mut buffer = RasterBuffer::new(rgba, config.dpi);
        buffer.page_index = Some(config.page_index);
        Ok(buffer)
    }

    fn load_all_pages(&self, path: &Path, config: &LoadConfig) -> CoreResult<Vec<RasterBuffer>> {
        let bytes = std::fs::read(path).map_err(|e| CoreError::FileReadError {
            path: path.to_path_buf(),
            source: e,
        })?;
        render_all_pages(bytes, config.dpi)
    }

    fn load_all_pages_from_memory(
        &self,
        data: &[u8],
        _name_hint: &str,
        config: &LoadConfig,
    ) -> CoreResult<Vec<RasterBuffer>> {
        render_all_pages(data.to_vec(), config.dpi)
    }

    fn get_metadata(&self, path: &Path) -> CoreResult<LoaderMetadata> {
        let bytes = std::fs::read(path).map_err(|e| CoreError::FileReadError {
            path: path.to_path_buf(),
            source: e,
        })?;

        let file_size = bytes.len() as u64;
        let data_vec = bytes;

        let pdf = Pdf::new(Arc::new(data_vec)).map_err(|e| CoreError::ImageDecodeError {
            reason: format!("Failed to parse PDF for metadata: {:?}", e),
        })?;

        let page_count = pdf.pages().len();

        Ok(LoaderMetadata {
            filename: path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string(),
            extension: "pdf".to_string(),
            file_size,
            page_count,
            native_width: None,
            native_height: None,
            is_vector: true,
            format_info: format!("PDF ({} pages) - hayro", page_count),
        })
    }

    fn name(&self) -> &str {
        "PDF Loader (Pure Rust - hayro)"
    }
}

/// Load all pages from a PDF file, returning one `RasterBuffer` per page.
///
/// This function loads the PDF from the given path, parses it, and renders each page
/// individually at the specified DPI. Each page is returned as a separate `RasterBuffer`.
pub fn load_pdf_all_pages(path: &Path, dpi: u32) -> CoreResult<Vec<(usize, RasterBuffer)>> {
    let bytes = std::fs::read(path).map_err(|e| CoreError::FileReadError {
        path: path.to_path_buf(),
        source: e,
    })?;

    Ok(render_all_pages(bytes, dpi)?
        .into_iter()
        .map(|buffer| (buffer.page_index.unwrap_or(0) + 1, buffer))
        .collect())
}

fn render_all_pages(data: Vec<u8>, dpi: u32) -> CoreResult<Vec<RasterBuffer>> {
    let pdf = Pdf::new(Arc::new(data)).map_err(|e| CoreError::ImageDecodeError {
        reason: format!("Failed to parse PDF: {:?}", e),
    })?;

    let pages = pdf.pages();
    let page_count = pages.len();
    info!(page_count = page_count, dpi = dpi, "Loading all PDF pages");
    if page_count == 0 {
        return Err(CoreError::ImageDecodeError {
            reason: "PDF has no pages".into(),
        });
    }

    let mut results = Vec::with_capacity(page_count);
    for (idx, page) in pages.iter().enumerate() {
        let rgba = PdfLoader::render_page(page, dpi)?;
        let mut buffer = RasterBuffer::new(rgba, dpi);
        buffer.page_index = Some(idx);
        results.push(buffer);
    }
    Ok(results)
}
