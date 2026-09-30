// =============================================================================
// dc_core/alignment/engine - Main Alignment Engine
// =============================================================================
// The high-level alignment interface that orchestrates:
// 1. Feature detection
// 2. Feature matching
// 3. Homography computation
// 4. Image warping
//
// This is the API that the UI layer calls for "align these images".
// =============================================================================

use super::features::{FeatureDetector, FeatureDetectorConfig, FeatureDetectorType};
use super::homography::{HomographyMatrix, HomographySolver};
use crate::types::{CoreError, CoreResult, RasterBuffer};
use rayon::prelude::*;
use tracing::{debug, info, instrument, warn};

/// Configuration for the alignment engine.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AlignmentConfig {
    /// Feature detector type
    pub detector_type: FeatureDetectorType,

    /// Maximum features to detect per image
    pub max_features: usize,

    /// Ratio test threshold for feature matching
    pub ratio_threshold: f32,

    /// Minimum matches required
    pub min_matches: usize,

    /// RANSAC threshold in pixels
    pub ransac_threshold: f64,

    /// Minimum inlier ratio to accept alignment (0.0 - 1.0)
    pub min_inlier_ratio: f64,

    /// Whether to use parallel processing
    pub use_parallel: bool,

    /// FAST corner detection threshold (higher = fewer but stronger corners)
    pub fast_threshold: u8,
}

impl Default for AlignmentConfig {
    fn default() -> Self {
        Self {
            detector_type: FeatureDetectorType::Orb,
            max_features: 5000,
            fast_threshold: 20,
            ratio_threshold: 0.75,
            min_matches: 10,
            ransac_threshold: 5.0,
            min_inlier_ratio: 0.15, // At least 15% of matches must be inliers
            use_parallel: true,
        }
    }
}

impl AlignmentConfig {
    /// Create a config for high-precision alignment (slower but more accurate).
    pub fn high_precision() -> Self {
        Self {
            detector_type: FeatureDetectorType::Orb,
            max_features: 10000,
            fast_threshold: 15,
            ratio_threshold: 0.65, // Stricter matching
            min_matches: 20,
            ransac_threshold: 2.0, // Tighter tolerance
            min_inlier_ratio: 0.20,
            use_parallel: true,
        }
    }

    /// Create a config for fast preview alignment.
    pub fn fast_preview() -> Self {
        Self {
            detector_type: FeatureDetectorType::Orb,
            max_features: 2000,
            fast_threshold: 25,
            ratio_threshold: 0.80,
            min_matches: 8,
            ransac_threshold: 10.0,
            min_inlier_ratio: 0.10,
            use_parallel: true,
        }
    }
}

/// Result of an alignment operation.
#[derive(Debug)]
pub struct AlignmentResult {
    /// The aligned (warped) target image
    pub aligned_image: RasterBuffer,

    /// The computed homography matrix
    pub homography: HomographyMatrix,

    /// Number of features found in reference image
    pub reference_features: usize,

    /// Number of features found in target image
    pub target_features: usize,

    /// Number of matched feature pairs
    pub matched_pairs: usize,

    /// Number of inliers after RANSAC
    pub inlier_count: usize,

    /// Inlier ratio (quality metric, 0.0 - 1.0)
    pub inlier_ratio: f64,

    /// Approximate rotation applied (degrees)
    pub rotation_degrees: f64,

    /// Approximate scale factor applied
    pub scale_factor: f64,
}

impl AlignmentResult {
    /// Get a confidence score (0.0 - 1.0) for the alignment quality.
    ///
    /// The score considers:
    /// - Inlier ratio (higher = better)
    /// - Number of inliers (more = more confident)
    /// - How close to identity the transformation is (small changes = more plausible)
    pub fn confidence_score(&self) -> f64 {
        // Base score from inlier ratio
        let ratio_score = self.inlier_ratio;

        // Bonus for having more inliers (diminishing returns)
        let count_score = (self.inlier_count as f64 / 100.0).min(1.0);

        // Penalize extreme transformations
        let rotation_penalty = 1.0 - (self.rotation_degrees.abs() / 180.0).min(1.0) * 0.3;
        let scale_penalty = if self.scale_factor > 0.0 {
            1.0 - ((self.scale_factor - 1.0).abs() / 5.0).min(1.0) * 0.2
        } else {
            0.0
        };

        // Combined score
        (ratio_score * 0.5 + count_score * 0.3) * rotation_penalty * scale_penalty
    }

    /// Check if the alignment is considered "good".
    pub fn is_good_alignment(&self) -> bool {
        self.confidence_score() > 0.3
    }
}

/// The main alignment engine.
///
/// # Usage
///
/// ```ignore
/// let engine = AlignmentEngine::new(AlignmentConfig::default());
/// let result = engine.align(&reference, &target)?;
/// let aligned_image = result.aligned_image;
/// ```
///
/// # Thread Safety
///
/// The engine is designed to be used from a single thread, but internally
/// uses rayon for parallel feature detection when configured.
pub struct AlignmentEngine {
    config: AlignmentConfig,
    detector: FeatureDetector,
    solver: HomographySolver,
}

impl AlignmentEngine {
    /// Create a new alignment engine with the given configuration.
    pub fn new(config: AlignmentConfig) -> Self {
        let detector_config = FeatureDetectorConfig {
            detector_type: config.detector_type,
            max_features: config.max_features,
            ratio_threshold: config.ratio_threshold,
            min_matches: config.min_matches,
            fast_threshold: config.fast_threshold,
        };

        let solver = HomographySolver {
            ransac_threshold: config.ransac_threshold,
            ..HomographySolver::default()
        };

        Self {
            config,
            detector: FeatureDetector::new(detector_config),
            solver,
        }
    }

    /// Create an engine with default settings.
    pub fn with_defaults() -> Self {
        Self::new(AlignmentConfig::default())
    }

    /// Align a target image to a reference image.
    ///
    /// # Algorithm Overview
    ///
    /// ```text
    /// ┌─────────────────┐     ┌─────────────────┐
    /// │  Reference Img  │     │   Target Img    │
    /// └────────┬────────┘     └────────┬────────┘
    ///          │                       │
    ///          ▼                       ▼
    /// ┌─────────────────┐     ┌─────────────────┐
    /// │ Feature Detect  │     │ Feature Detect  │
    /// │  (ORB/AKAZE)    │     │  (ORB/AKAZE)    │
    /// └────────┬────────┘     └────────┬────────┘
    ///          │                       │
    ///          └──────────┬────────────┘
    ///                     ▼
    ///          ┌─────────────────────┐
    ///          │  Feature Matching   │
    ///          │  (Brute Force +     │
    ///          │   Ratio Test)       │
    ///          └──────────┬──────────┘
    ///                     ▼
    ///          ┌─────────────────────┐
    ///          │  RANSAC Homography  │
    ///          │  (Filter Outliers)  │
    ///          └──────────┬──────────┘
    ///                     ▼
    ///          ┌─────────────────────┐
    ///          │  Warp Perspective   │
    ///          │  (Apply Transform)  │
    ///          └──────────┬──────────┘
    ///                     ▼
    ///          ┌─────────────────────┐
    ///          │   Aligned Image     │
    ///          └─────────────────────┘
    /// ```
    ///
    /// # Arguments
    /// * `reference` - The base image to align to
    /// * `target` - The image to be transformed
    ///
    /// # Returns
    /// * `Ok(AlignmentResult)` - Contains the aligned image and quality metrics
    /// * `Err(CoreError)` - If alignment fails
    #[instrument(skip(self, reference, target), fields(
        ref_size = format!("{}x{}", reference.image.width(), reference.image.height()),
        target_size = format!("{}x{}", target.image.width(), target.image.height())
    ))]
    pub fn align(
        &self,
        reference: &RasterBuffer,
        target: &RasterBuffer,
    ) -> CoreResult<AlignmentResult> {
        info!("Starting alignment process");

        // =======================================================================
        // Step 1: Feature Detection
        // =======================================================================
        // Detect keypoints and compute descriptors for both images.
        // ORB finds corners and distinctive patterns that are invariant to
        // rotation and somewhat invariant to scale.
        // =======================================================================

        debug!("Step 1: Detecting features in both images");

        let (features_ref, features_target) = if self.config.use_parallel {
            // Detect features in parallel using rayon
            rayon::join(
                || self.detector.detect(reference),
                || self.detector.detect(target),
            )
        } else {
            (
                self.detector.detect(reference),
                self.detector.detect(target),
            )
        };

        let features_ref = features_ref?;
        let features_target = features_target?;

        let ref_count = features_ref.count();
        let target_count = features_target.count();

        info!(
            reference_features = ref_count,
            target_features = target_count,
            "Feature detection complete"
        );

        // Validate we have enough features
        if ref_count < self.config.min_matches {
            return Err(CoreError::InsufficientFeatures {
                found: ref_count,
                required: self.config.min_matches,
            });
        }
        if target_count < self.config.min_matches {
            return Err(CoreError::InsufficientFeatures {
                found: target_count,
                required: self.config.min_matches,
            });
        }

        // =======================================================================
        // Step 2: Feature Matching
        // =======================================================================
        // Match features between the two images using brute-force matching
        // with Lowe's ratio test to filter ambiguous matches.
        // =======================================================================

        debug!("Step 2: Matching features between images");

        let matches = self
            .detector
            .match_features(&features_ref, &features_target)?;
        let match_count = matches.len();

        info!(matched_pairs = match_count, "Feature matching complete");

        if match_count < self.config.min_matches {
            return Err(CoreError::InsufficientFeatures {
                found: match_count,
                required: self.config.min_matches,
            });
        }

        // =======================================================================
        // Step 3: Compute Homography with RANSAC
        // =======================================================================
        // Use RANSAC to robustly estimate the 3x3 homography matrix.
        // RANSAC handles outliers by iteratively finding the best model
        // that explains the majority of correspondences.
        // =======================================================================

        debug!("Step 3: Computing homography matrix with RANSAC");

        let (homography, inlier_ratio) = self.solver.compute_homography(
            &features_ref.keypoints,
            &features_target.keypoints,
            &matches,
        )?;

        let inlier_count = (match_count as f64 * inlier_ratio).round() as usize;

        info!(
            inlier_ratio = format!("{:.1}%", inlier_ratio * 100.0),
            inlier_count, "Homography computed"
        );

        // Validate alignment quality
        if inlier_ratio < self.config.min_inlier_ratio {
            return Err(CoreError::AlignmentConfidenceTooLow {
                confidence: inlier_ratio * 100.0,
                threshold: self.config.min_inlier_ratio * 100.0,
            });
        }

        // Extract transformation parameters for logging
        let rotation = homography.approximate_rotation();
        let scale = homography.approximate_scale();

        info!(
            rotation = format!("{:.2}°", rotation),
            scale = format!("{:.4}x", scale),
            "Transformation parameters extracted"
        );

        // Warn about extreme transformations
        if rotation.abs() > 45.0 {
            warn!(
                rotation = format!("{:.2}°", rotation),
                "Large rotation detected - verify alignment correctness"
            );
        }
        if scale < 0.1 || scale > 10.0 {
            warn!(
                scale = format!("{:.4}x", scale),
                "Extreme scale factor detected - verify alignment correctness"
            );
        }

        // =======================================================================
        // Step 4: Warp Perspective
        // =======================================================================
        // Apply the homography matrix to transform the target image.
        // The output will have the same dimensions as the reference image,
        // with the target content warped to align with the reference.
        // =======================================================================

        debug!("Step 4: Warping target image to align with reference");

        let output_size = reference.dimensions();
        let aligned_image = self.solver.warp_image(target, &homography, output_size)?;

        info!("Alignment complete");

        Ok(AlignmentResult {
            aligned_image,
            homography,
            reference_features: ref_count,
            target_features: target_count,
            matched_pairs: match_count,
            inlier_count,
            inlier_ratio,
            rotation_degrees: rotation,
            scale_factor: scale,
        })
    }

    /// Align multiple images to a reference.
    ///
    /// Uses parallel processing for efficiency when aligning many images.
    #[instrument(skip(self, reference, targets))]
    pub fn align_multiple(
        &self,
        reference: &RasterBuffer,
        targets: &[RasterBuffer],
    ) -> Vec<CoreResult<AlignmentResult>> {
        info!(target_count = targets.len(), "Aligning multiple images");

        if self.config.use_parallel {
            targets
                .par_iter()
                .map(|target| self.align(reference, target))
                .collect()
        } else {
            targets
                .iter()
                .map(|target| self.align(reference, target))
                .collect()
        }
    }

    /// Get the current configuration.
    pub fn config(&self) -> &AlignmentConfig {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_alignment_config_defaults() {
        let config = AlignmentConfig::default();
        assert!(config.max_features > 0);
        assert!(config.min_matches > 0);
        assert!(config.ransac_threshold > 0.0);
        assert!(config.min_inlier_ratio > 0.0);
    }

    #[test]
    fn test_alignment_config_presets() {
        let fast = AlignmentConfig::fast_preview();
        let precise = AlignmentConfig::high_precision();

        // Fast should use fewer features
        assert!(fast.max_features < precise.max_features);

        // Precise should be stricter
        assert!(precise.ratio_threshold < fast.ratio_threshold);
    }

    #[test]
    fn test_alignment_result_confidence() {
        let result = AlignmentResult {
            aligned_image: RasterBuffer::new(image::RgbaImage::new(100, 100), 300),
            homography: HomographyMatrix::identity(),
            reference_features: 1000,
            target_features: 1000,
            matched_pairs: 500,
            inlier_count: 400,
            inlier_ratio: 0.8,
            rotation_degrees: 0.5,
            scale_factor: 1.01,
        };

        let confidence = result.confidence_score();
        assert!(confidence > 0.0);
        assert!(confidence <= 1.0);
        assert!(result.is_good_alignment());
    }

    #[test]
    fn test_alignment_engine_creation() {
        let engine = AlignmentEngine::with_defaults();
        assert_eq!(engine.config().detector_type, FeatureDetectorType::Orb);
    }

    #[test]
    fn test_poor_alignment_detection() {
        let result = AlignmentResult {
            aligned_image: RasterBuffer::new(image::RgbaImage::new(100, 100), 300),
            homography: HomographyMatrix::identity(),
            reference_features: 100,
            target_features: 100,
            matched_pairs: 10,
            inlier_count: 2,        // Very few inliers
            inlier_ratio: 0.1,      // Poor ratio
            rotation_degrees: 90.0, // Extreme rotation
            scale_factor: 5.0,      // Extreme scale
        };

        // Should not be considered a good alignment
        assert!(!result.is_good_alignment());
    }
}
