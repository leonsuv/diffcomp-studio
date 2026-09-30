// =============================================================================
// dc_core/diff/engine - Difference Computation Engine
// =============================================================================
// Computes pixel-level differences between aligned images.
//
// ## Visualization Modes
//
// 1. **Side-by-Side**: No computation, just display both images
// 2. **Overlay**: Blend images with configurable opacity
// 3. **Color Difference**: Reference=Red channel, Target=Green channel
// 4. **Heatmap**: Magnitude of difference shown as heat colors
// 5. **Binary Mask**: Black/White showing changed regions
// =============================================================================

use crate::types::{CoreResult, LayerColor, RasterBuffer};
use image::RgbaImage;
#[cfg(not(target_arch = "wasm32"))]
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use tracing::{debug, instrument};

/// Blend mode for difference visualization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum BlendMode {
    /// Standard overlay with opacity blending
    #[default]
    Overlay,

    /// Red-Green color difference (Reference=Red, Target=Green, Same=Black/Gray)
    ColorDifference,

    /// Heatmap showing magnitude of difference
    Heatmap,

    /// Binary black/white mask of changed regions
    BinaryMask,

    /// Subtract target from reference (absolute difference)
    Subtract,

    /// XOR mode - shows any difference as white
    Xor,
}

/// Configuration for difference computation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DiffConfig {
    /// Apply one-pixel ink tolerance in automatic/CPU color comparison.
    pub morphological_tolerance: bool,
    /// Blend mode for visualization
    pub blend_mode: BlendMode,

    /// Opacity of the overlay layer (0.0 - 1.0)
    pub overlay_opacity: f32,

    /// Threshold for binary mask mode (0 - 255)
    pub binary_threshold: u8,

    /// Color for reference layer in color difference mode
    pub reference_color: LayerColor,

    /// Color for target layer in color difference mode
    pub target_color: LayerColor,

    /// Whether to use parallel processing
    pub use_parallel: bool,

    /// Ignore small differences below this threshold (0-255)
    pub noise_threshold: u8,
}

impl Default for DiffConfig {
    fn default() -> Self {
        Self {
            morphological_tolerance: true,
            blend_mode: BlendMode::ColorDifference,
            overlay_opacity: 0.5,
            binary_threshold: 25,
            reference_color: LayerColor::new(232, 92, 82), // Reference coral
            target_color: LayerColor::new(78, 145, 218),   // Revision blue
            use_parallel: true,
            noise_threshold: 15,
        }
    }
}

/// Result of a difference computation.
#[derive(Debug)]
pub struct DiffResult {
    /// The computed difference image
    pub image: RasterBuffer,

    /// Total number of pixels compared
    pub total_pixels: u64,

    /// Number of pixels that are different
    pub different_pixels: u64,

    /// Percentage of pixels that changed (0.0 - 100.0)
    pub change_percentage: f64,

    /// Average difference magnitude (0.0 - 255.0)
    pub average_difference: f64,

    /// Maximum difference found
    pub max_difference: u8,
}

impl DiffResult {
    /// Check if the images are essentially identical (within noise threshold).
    pub fn is_identical(&self, tolerance_percent: f64) -> bool {
        self.change_percentage <= tolerance_percent
    }

    /// Get a human-readable summary of the difference.
    pub fn summary(&self) -> String {
        if self.change_percentage < 0.01 {
            "Images are identical".to_string()
        } else if self.change_percentage < 1.0 {
            format!(
                "Minor differences: {:.2}% of pixels changed",
                self.change_percentage
            )
        } else if self.change_percentage < 10.0 {
            format!(
                "Moderate differences: {:.1}% of pixels changed",
                self.change_percentage
            )
        } else {
            format!(
                "Significant differences: {:.1}% of pixels changed",
                self.change_percentage
            )
        }
    }
}

/// Engine for computing visual differences between images.
pub struct DiffEngine {
    config: DiffConfig,
}

impl DiffEngine {
    /// Create a new diff engine with the given configuration.
    pub fn new(config: DiffConfig) -> Self {
        Self { config }
    }

    /// Create an engine with default settings.
    pub fn with_defaults() -> Self {
        Self::new(DiffConfig::default())
    }

    /// Update the engine configuration.
    pub fn set_config(&mut self, config: DiffConfig) {
        self.config = config;
    }

    /// Get a mutable reference to the configuration.
    pub fn config_mut(&mut self) -> &mut DiffConfig {
        &mut self.config
    }

    /// Compute the difference between two aligned images.
    ///
    /// # Arguments
    /// * `reference` - The base/reference image
    /// * `target` - The image to compare against reference (should be aligned)
    ///
    /// # Returns
    /// * `Ok(DiffResult)` - Contains the visualization and statistics
    /// * `Err(CoreError)` - If computation fails
    #[instrument(skip(self, reference, target))]
    pub fn compute_diff(
        &self,
        reference: &RasterBuffer,
        target: &RasterBuffer,
    ) -> CoreResult<DiffResult> {
        let (ref_w, ref_h) = reference.dimensions();
        if reference.is_empty() || target.is_empty() {
            return Err(crate::types::CoreError::InternalError {
                message: "Cannot compare empty images".into(),
            });
        }
        let (tar_w, tar_h) = target.dimensions();

        debug!(
            reference_size = format!("{}x{}", ref_w, ref_h),
            target_size = format!("{}x{}", tar_w, tar_h),
            blend_mode = ?self.config.blend_mode,
            "Computing difference"
        );

        // If dimensions differ, resize the target to match reference.
        // This handles cases where alignment failed or was skipped.
        let target = if ref_w != tar_w || ref_h != tar_h {
            debug!(
                from = format!("{}x{}", tar_w, tar_h),
                to = format!("{}x{}", ref_w, ref_h),
                "Target dimensions differ from reference, resizing to match"
            );
            let resized = image::imageops::resize(
                &target.image,
                ref_w,
                ref_h,
                image::imageops::FilterType::Lanczos3,
            );
            std::borrow::Cow::Owned(RasterBuffer::new(resized, target.dpi))
        } else {
            std::borrow::Cow::Borrowed(target)
        };
        let target = target.as_ref();

        let (result_image, stats) = self.compute_pixels(reference, target);

        let buffer = RasterBuffer::new(result_image, reference.dpi);

        Ok(DiffResult {
            image: buffer,
            total_pixels: stats.total_pixels,
            different_pixels: stats.different_pixels,
            change_percentage: stats.change_percentage,
            average_difference: stats.average_difference,
            max_difference: stats.max_difference,
        })
    }

    /// Render and collect statistics in one pass, using disjoint chunks on native CPUs.
    fn compute_pixels(
        &self,
        reference: &RasterBuffer,
        target: &RasterBuffer,
    ) -> (RgbaImage, DiffStats) {
        let mut output = RgbaImage::new(reference.image.width(), reference.image.height());
        let threshold = if self.config.blend_mode == BlendMode::BinaryMask {
            self.config.binary_threshold
        } else {
            self.config.noise_threshold
        };
        let process = |((out, a), b): ((&mut [u8], &[u8]), &[u8])| {
            let mut count = 0u64;
            let mut sum = 0u64;
            let mut max = 0u8;
            for ((dst, a), b) in out
                .chunks_exact_mut(4)
                .zip(a.chunks_exact(4))
                .zip(b.chunks_exact(4))
            {
                let (pixel, magnitude) = self.render_pixel(a, b);
                dst.copy_from_slice(&pixel);
                count += u64::from(magnitude > threshold);
                sum += magnitude as u64;
                max = max.max(magnitude);
            }
            (count, sum, max)
        };
        let combine = |a: (u64, u64, u8), b: (u64, u64, u8)| (a.0 + b.0, a.1 + b.1, a.2.max(b.2));
        let serial = || (0u64, 0u64, 0u8);
        let stats;
        #[cfg(not(target_arch = "wasm32"))]
        if self.config.use_parallel && reference.pixel_count() >= 65536 {
            stats = output
                .as_mut()
                .par_chunks_mut(4096)
                .zip(reference.image.as_raw().par_chunks(4096))
                .zip(target.image.as_raw().par_chunks(4096))
                .map(process)
                .reduce(serial, combine);
        } else {
            stats = output
                .as_mut()
                .chunks_mut(4096)
                .zip(reference.image.as_raw().chunks(4096))
                .zip(target.image.as_raw().chunks(4096))
                .map(process)
                .fold(serial(), combine);
        }
        #[cfg(target_arch = "wasm32")]
        {
            stats = output
                .as_mut()
                .chunks_mut(4096)
                .zip(reference.image.as_raw().chunks(4096))
                .zip(target.image.as_raw().chunks(4096))
                .map(process)
                .fold(serial(), combine);
        }
        let total = reference.pixel_count();
        (
            output,
            DiffStats {
                total_pixels: total,
                different_pixels: stats.0,
                average_difference: stats.1 as f64 / total as f64,
                change_percentage: stats.0 as f64 / total as f64 * 100.0,
                max_difference: stats.2,
            },
        )
    }

    /// One renderer shared by pair comparison and the allocation-free batch path.
    pub(super) fn render_pixel(&self, a: &[u8], b: &[u8]) -> ([u8; 4], u8) {
        let a = paper_pixel(a);
        let b = paper_pixel(b);
        let magnitude = Self::pixel_difference(&a, &b);
        let luminance = Self::luminance(a[0], a[1], a[2]);
        let mut out = [0, 0, 0, 255];
        match self.config.blend_mode {
            BlendMode::Overlay => {
                let opacity = self.config.overlay_opacity.clamp(0.0, 1.0);
                for c in 0..3 {
                    out[c] = (a[c] as f32 * (1.0 - opacity) + b[c] as f32 * opacity) as u8;
                }
            }
            BlendMode::ColorDifference => {
                if magnitude <= self.config.noise_threshold {
                    out[..3].fill(luminance);
                } else {
                    let target_lum = Self::luminance(b[0], b[1], b[2]);
                    let color = if luminance < target_lum {
                        self.config.reference_color
                    } else {
                        self.config.target_color
                    };
                    out[0] = Self::blend_channel(0, color.r, magnitude);
                    out[1] = Self::blend_channel(0, color.g, magnitude);
                    out[2] = Self::blend_channel(0, color.b, magnitude);
                }
            }
            BlendMode::Heatmap => {
                if magnitude <= self.config.noise_threshold {
                    out[..3].fill(luminance);
                } else {
                    let (r, g, b) = Self::difference_to_heatmap(magnitude);
                    out[..3].copy_from_slice(&[r, g, b]);
                }
            }
            BlendMode::BinaryMask => {
                out[..3].fill(if magnitude > self.config.binary_threshold {
                    255
                } else {
                    0
                });
            }
            BlendMode::Subtract => {
                for c in 0..3 {
                    out[c] = a[c].abs_diff(b[c]);
                }
            }
            BlendMode::Xor => {
                for c in 0..3 {
                    out[c] = a[c] ^ b[c];
                }
            }
        }
        (out, magnitude)
    }

    // =========================================================================
    // Helper Functions
    // =========================================================================

    /// Calculate luminance from RGB values (ITU-R BT.601 weights).
    fn luminance(r: u8, g: u8, b: u8) -> u8 {
        ((r as f32 * 0.299) + (g as f32 * 0.587) + (b as f32 * 0.114)) as u8
    }

    /// Calculate the maximum channel difference between two pixels.
    fn pixel_difference(a: &[u8], b: &[u8]) -> u8 {
        let mut max_diff = 0u8;
        for i in 0..3 {
            let diff = (a[i] as i16 - b[i] as i16).unsigned_abs() as u8;
            if diff > max_diff {
                max_diff = diff;
            }
        }
        max_diff
    }

    /// Blend a channel value with a weight.
    fn blend_channel(base: u8, color: u8, weight: u8) -> u8 {
        let w = weight as f32 / 255.0;
        ((base as f32 * (1.0 - w)) + (color as f32 * w)) as u8
    }

    /// Convert a difference value (0-255) to a heatmap color.
    fn difference_to_heatmap(diff: u8) -> (u8, u8, u8) {
        let t = diff as f32 / 255.0;

        // Heatmap: blue (0) -> cyan -> green -> yellow -> red (1)
        let (r, g, b) = if t < 0.25 {
            // Blue to cyan
            let local_t = t * 4.0;
            (0.0, local_t, 1.0)
        } else if t < 0.5 {
            // Cyan to green
            let local_t = (t - 0.25) * 4.0;
            (0.0, 1.0, 1.0 - local_t)
        } else if t < 0.75 {
            // Green to yellow
            let local_t = (t - 0.5) * 4.0;
            (local_t, 1.0, 0.0)
        } else {
            // Yellow to red
            let local_t = (t - 0.75) * 4.0;
            (1.0, 1.0 - local_t, 0.0)
        };

        ((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8)
    }

    /// Get the current configuration.
    pub fn config(&self) -> &DiffConfig {
        &self.config
    }
}

/// Composite straight-alpha pixels on white document paper.
pub(super) fn paper_pixel(pixel: &[u8]) -> [u8; 4] {
    let alpha = pixel[3] as u32;
    let mut result = [255; 4];
    for c in 0..3 {
        result[c] = ((pixel[c] as u32 * alpha + 255 * (255 - alpha)) / 255) as u8;
    }
    result
}

/// Internal statistics structure.
struct DiffStats {
    total_pixels: u64,
    different_pixels: u64,
    change_percentage: f64,
    average_difference: f64,
    max_difference: u8,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diff_config_defaults() {
        let config = DiffConfig::default();
        assert!(config.overlay_opacity > 0.0 && config.overlay_opacity <= 1.0);
        assert!(config.binary_threshold > 0);
    }

    #[test]
    fn test_luminance_calculation() {
        // Pure white
        assert_eq!(DiffEngine::luminance(255, 255, 255), 255);
        // Pure black
        assert_eq!(DiffEngine::luminance(0, 0, 0), 0);
        // Green should be brightest (highest weight)
        let green_lum = DiffEngine::luminance(0, 255, 0);
        let red_lum = DiffEngine::luminance(255, 0, 0);
        let blue_lum = DiffEngine::luminance(0, 0, 255);
        assert!(green_lum > red_lum);
        assert!(red_lum > blue_lum);
    }

    #[test]
    fn test_heatmap_range() {
        // Test that heatmap produces valid colors across full range
        for diff in 0..=255 {
            // Colors are u8, so they are always within range 0-255
            let (_r, _g, _b) = DiffEngine::difference_to_heatmap(diff);
        }

        // Low diff should be bluish
        let (r, _, b) = DiffEngine::difference_to_heatmap(10);
        assert!(b > r);

        // High diff should be reddish
        let (r, _, b) = DiffEngine::difference_to_heatmap(250);
        assert!(r > b);
    }

    #[test]
    fn test_pixel_difference() {
        // Identical pixels
        assert_eq!(
            DiffEngine::pixel_difference(&[100, 100, 100, 255], &[100, 100, 100, 255]),
            0
        );

        // Different pixels
        assert_eq!(
            DiffEngine::pixel_difference(&[0, 0, 0, 255], &[255, 255, 255, 255]),
            255
        );

        // Partial difference
        assert_eq!(
            DiffEngine::pixel_difference(&[100, 100, 100, 255], &[100, 150, 100, 255]),
            50
        );
    }

    #[test]
    fn test_diff_result_summary() {
        let result = DiffResult {
            image: RasterBuffer::new(RgbaImage::new(10, 10), 300),
            total_pixels: 100,
            different_pixels: 0,
            change_percentage: 0.0,
            average_difference: 0.0,
            max_difference: 0,
        };
        assert!(result.summary().contains("identical"));
        assert!(result.is_identical(0.1));
    }

    #[test]
    fn test_diff_engine_creation() {
        let engine = DiffEngine::with_defaults();
        assert_eq!(engine.config().blend_mode, BlendMode::ColorDifference);
    }
}

#[cfg(test)]
mod regression_tests {
    use super::*;
    #[test]
    fn parallel_and_serial_output_and_statistics_match_for_every_mode() {
        let reference = RasterBuffer::new(
            RgbaImage::from_fn(400, 300, |x, y| {
                image::Rgba([(x % 256) as u8, (y % 256) as u8, 100, 255])
            }),
            150,
        );
        let target = RasterBuffer::new(
            RgbaImage::from_fn(400, 300, |x, y| {
                image::Rgba([(y % 256) as u8, (x % 256) as u8, 100, 128])
            }),
            150,
        );
        for mode in [
            BlendMode::Overlay,
            BlendMode::ColorDifference,
            BlendMode::Heatmap,
            BlendMode::BinaryMask,
            BlendMode::Subtract,
            BlendMode::Xor,
        ] {
            let serial = DiffEngine::new(DiffConfig {
                blend_mode: mode,
                use_parallel: false,
                ..Default::default()
            })
            .compute_diff(&reference, &target)
            .unwrap();
            let parallel = DiffEngine::new(DiffConfig {
                blend_mode: mode,
                use_parallel: true,
                ..Default::default()
            })
            .compute_diff(&reference, &target)
            .unwrap();
            assert_eq!(serial.image.image, parallel.image.image);
            assert_eq!(serial.different_pixels, parallel.different_pixels);
            assert_eq!(serial.average_difference, parallel.average_difference);
            assert_eq!(serial.max_difference, parallel.max_difference);
        }
    }
    #[test]
    fn transparent_paper_does_not_count_as_black_ink() {
        let reference = RasterBuffer::new(RgbaImage::from_pixel(2, 2, image::Rgba([255; 4])), 72);
        let target = RasterBuffer::new(RgbaImage::new(2, 2), 72);
        assert_eq!(
            DiffEngine::with_defaults()
                .compute_diff(&reference, &target)
                .unwrap()
                .different_pixels,
            0
        );
    }
    #[test]
    fn empty_images_return_errors_instead_of_nan_statistics() {
        let buffer = RasterBuffer::new(RgbaImage::new(0, 0), 72);
        assert!(DiffEngine::with_defaults()
            .compute_diff(&buffer, &buffer)
            .is_err());
    }
}
