// =============================================================================
// dc_core/alignment/features - Pure Rust Feature Detection
// =============================================================================
// Detects distinctive keypoints in images for alignment.
// Uses a pure-Rust ORB-like implementation:
//   1. FAST corner detection for keypoints
//   2. Oriented patch descriptors (rBRIEF-like binary descriptors)
//   3. Brute-force Hamming distance matching with Lowe's ratio test
//
// ## Why Custom Instead of OpenCV?
// - Zero system dependencies (cargo build just works, including WASM)
// - No C++ interop, no DLL shipping, no vcpkg
// - Same algorithmic foundation (FAST + binary descriptors + ratio test)
// =============================================================================

use crate::types::{CoreError, CoreResult, Keypoint, KeypointMatch, RasterBuffer};
use tracing::{debug, instrument};

/// Type of feature detector to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum FeatureDetectorType {
    /// ORB-like: FAST corners + binary descriptors
    /// Fast, rotation-invariant, good for most cases
    #[default]
    Orb,
}

/// Configuration for feature detection.
#[derive(Debug, Clone)]
pub struct FeatureDetectorConfig {
    /// Type of detector to use
    pub detector_type: FeatureDetectorType,
    /// Maximum number of features to detect
    pub max_features: usize,
    /// Ratio test threshold for feature matching (lower = stricter)
    /// Range: 0.0 - 1.0, typical: 0.7-0.8
    pub ratio_threshold: f32,
    /// Minimum number of matches required for valid alignment
    pub min_matches: usize,
    /// FAST threshold for corner detection
    pub fast_threshold: u8,
}

impl Default for FeatureDetectorConfig {
    fn default() -> Self {
        Self {
            detector_type: FeatureDetectorType::Orb,
            max_features: 5000,
            ratio_threshold: 0.75,
            min_matches: 10,
            fast_threshold: 20,
        }
    }
}

/// A 256-bit binary descriptor (32 bytes), similar to ORB/BRIEF.
#[derive(Clone)]
pub struct BinaryDescriptor {
    /// The binary descriptor data (32 bytes)
    pub data: [u8; 32],
}

impl BinaryDescriptor {
    /// Compute Hamming distance between two descriptors.
    pub fn hamming_distance(&self, other: &BinaryDescriptor) -> u32 {
        self.data
            .iter()
            .zip(other.data.iter())
            .map(|(a, b)| (a ^ b).count_ones())
            .sum()
    }
}

/// Feature detector wrapper — pure Rust, no OpenCV.
pub struct FeatureDetector {
    config: FeatureDetectorConfig,
}

impl FeatureDetector {
    /// Create a new feature detector with the given configuration.
    pub fn new(config: FeatureDetectorConfig) -> Self {
        Self { config }
    }

    /// Create a feature detector with default settings.
    pub fn with_defaults() -> Self {
        Self::new(FeatureDetectorConfig::default())
    }

    /// Convert a RasterBuffer to a grayscale image (Vec<u8>, width, height).
    fn to_grayscale(buffer: &RasterBuffer) -> (Vec<u8>, u32, u32) {
        let (width, height) = buffer.dimensions();
        let rgba = buffer.image.as_raw();
        let mut gray = vec![0u8; (width * height) as usize];

        for i in 0..(width * height) as usize {
            let idx = i * 4;
            // ITU-R BT.601 luma
            gray[i] = ((rgba[idx] as f32 * 0.299)
                + (rgba[idx + 1] as f32 * 0.587)
                + (rgba[idx + 2] as f32 * 0.114)) as u8;
        }

        (gray, width, height)
    }

    /// FAST-9 corner detection (Rosten & Drummond).
    /// Returns (x, y, response) tuples sorted by response strength.
    fn detect_fast_corners(
        gray: &[u8],
        width: u32,
        height: u32,
        threshold: u8,
        max_features: usize,
    ) -> Vec<(f32, f32, f32)> {
        let w = width as i32;
        let h = height as i32;

        // Circle offsets for FAST-9 (16 pixels circumference)
        let circle: [(i32, i32); 16] = [
            (0, -3),
            (1, -3),
            (2, -2),
            (3, -1),
            (3, 0),
            (3, 1),
            (2, 2),
            (1, 3),
            (0, 3),
            (-1, 3),
            (-2, 2),
            (-3, 1),
            (-3, 0),
            (-3, -1),
            (-2, -2),
            (-1, -3),
        ];

        let t = threshold as i32;
        let mut corners: Vec<(f32, f32, f32)> = Vec::new();

        // Scan image (skip 3-pixel border for the circle)
        for y in 3..(h - 3) {
            for x in 3..(w - 3) {
                let center = gray[(y * w + x) as usize] as i32;

                // Quick rejection: check pixels at 0°, 90°, 180°, 270°
                let p0 = gray[((y + circle[0].1) * w + (x + circle[0].0)) as usize] as i32;
                let p4 = gray[((y + circle[4].1) * w + (x + circle[4].0)) as usize] as i32;
                let p8 = gray[((y + circle[8].1) * w + (x + circle[8].0)) as usize] as i32;
                let p12 = gray[((y + circle[12].1) * w + (x + circle[12].0)) as usize] as i32;

                // At least 3 of these 4 must be brighter or darker than center ± t
                let mut count_bright = 0u32;
                let mut count_dark = 0u32;
                for &p in &[p0, p4, p8, p12] {
                    if p > center + t {
                        count_bright += 1;
                    }
                    if p < center - t {
                        count_dark += 1;
                    }
                }

                if count_bright < 3 && count_dark < 3 {
                    continue;
                }

                // Full circle check: need 9 contiguous brighter or darker pixels
                let mut bright_count = 0u32;
                let mut dark_count = 0u32;
                let mut max_bright_run = 0u32;
                let mut max_dark_run = 0u32;
                let mut response_sum = 0i32;

                // Check all 16 + wrap-around
                for i in 0..32 {
                    let idx = i % 16;
                    let (dx, dy) = circle[idx];
                    let p = gray[((y + dy) * w + (x + dx)) as usize] as i32;

                    if p > center + t {
                        bright_count += 1;
                        dark_count = 0;
                        if bright_count > max_bright_run {
                            max_bright_run = bright_count;
                        }
                        response_sum += (p - center).abs();
                    } else if p < center - t {
                        dark_count += 1;
                        bright_count = 0;
                        if dark_count > max_dark_run {
                            max_dark_run = dark_count;
                        }
                        response_sum += (p - center).abs();
                    } else {
                        bright_count = 0;
                        dark_count = 0;
                    }
                }

                if max_bright_run >= 9 || max_dark_run >= 9 {
                    corners.push((x as f32, y as f32, response_sum as f32));
                }
            }
        }

        // Non-maximum suppression: only keep local maxima in a 5x5 window
        let mut suppressed: Vec<bool> = vec![false; corners.len()];
        for i in 0..corners.len() {
            if suppressed[i] {
                continue;
            }
            for j in (i + 1)..corners.len() {
                if suppressed[j] {
                    continue;
                }
                let dx = corners[i].0 - corners[j].0;
                let dy = corners[i].1 - corners[j].1;
                if dx * dx + dy * dy < 25.0 {
                    // Within 5-pixel radius
                    if corners[i].2 >= corners[j].2 {
                        suppressed[j] = true;
                    } else {
                        suppressed[i] = true;
                        break;
                    }
                }
            }
        }

        let mut result: Vec<(f32, f32, f32)> = corners
            .into_iter()
            .enumerate()
            .filter(|(i, _)| !suppressed[*i])
            .map(|(_, c)| c)
            .collect();

        // Sort by response (descending) and take top N
        result.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
        result.truncate(max_features);

        result
    }

    /// Compute a binary descriptor for a keypoint using intensity comparisons
    /// in a rotated patch (rBRIEF-like).
    fn compute_descriptor(
        gray: &[u8],
        width: u32,
        height: u32,
        x: f32,
        y: f32,
    ) -> BinaryDescriptor {
        let w = width as i32;
        let h = height as i32;
        let cx = x as i32;
        let cy = y as i32;

        // Compute orientation using intensity centroid
        let mut m01: i32 = 0;
        let mut m10: i32 = 0;
        let patch_r = 15i32;
        for dy in -patch_r..=patch_r {
            for dx in -patch_r..=patch_r {
                let px = cx + dx;
                let py = cy + dy;
                if px >= 0 && px < w && py >= 0 && py < h {
                    let val = gray[(py * w + px) as usize] as i32;
                    m10 += dx * val;
                    m01 += dy * val;
                }
            }
        }
        let angle = (m01 as f64).atan2(m10 as f64);
        let cos_a = angle.cos();
        let sin_a = angle.sin();

        // Pre-defined comparison test pairs (deterministic, reproducible)
        // These are sampled from a Gaussian distribution around the patch center
        // Using a fixed seed pattern for reproducibility
        let test_pairs: [(i8, i8, i8, i8); 256] = Self::get_test_pairs();

        let mut desc = BinaryDescriptor { data: [0u8; 32] };

        for (i, &(dx1, dy1, dx2, dy2)) in test_pairs.iter().enumerate() {
            // Rotate sample points by the patch orientation
            let rx1 = (dx1 as f64 * cos_a - dy1 as f64 * sin_a).round() as i32;
            let ry1 = (dx1 as f64 * sin_a + dy1 as f64 * cos_a).round() as i32;
            let rx2 = (dx2 as f64 * cos_a - dy2 as f64 * sin_a).round() as i32;
            let ry2 = (dx2 as f64 * sin_a + dy2 as f64 * cos_a).round() as i32;

            let px1 = (cx + rx1).clamp(0, w - 1);
            let py1 = (cy + ry1).clamp(0, h - 1);
            let px2 = (cx + rx2).clamp(0, w - 1);
            let py2 = (cy + ry2).clamp(0, h - 1);

            let v1 = gray[(py1 * w + px1) as usize];
            let v2 = gray[(py2 * w + px2) as usize];

            if v1 < v2 {
                desc.data[i / 8] |= 1 << (i % 8);
            }
        }

        desc
    }

    /// Get deterministic test pair coordinates for binary descriptor.
    /// These are pre-computed offsets in a ~31x31 patch.
    fn get_test_pairs() -> [(i8, i8, i8, i8); 256] {
        // Pseudo-random but deterministic pairs generated from a fixed pattern.
        // Each pair (dx1, dy1, dx2, dy2) specifies two sample points relative
        // to the keypoint center. Values range from -15 to +15.
        let mut pairs = [(0i8, 0i8, 0i8, 0i8); 256];
        // Use a simple linear congruential generator for reproducible "random" pairs
        let mut seed: u32 = 0xDEAD_BEEF;
        for pair in pairs.iter_mut() {
            seed = seed.wrapping_mul(1103515245).wrapping_add(12345);
            let a = ((seed >> 16) % 31) as i8 - 15;
            seed = seed.wrapping_mul(1103515245).wrapping_add(12345);
            let b = ((seed >> 16) % 31) as i8 - 15;
            seed = seed.wrapping_mul(1103515245).wrapping_add(12345);
            let c = ((seed >> 16) % 31) as i8 - 15;
            seed = seed.wrapping_mul(1103515245).wrapping_add(12345);
            let d = ((seed >> 16) % 31) as i8 - 15;
            *pair = (a, b, c, d);
        }
        pairs
    }

    /// Detect features in an image.
    #[instrument(skip(self, buffer))]
    pub fn detect(&self, buffer: &RasterBuffer) -> CoreResult<DetectedFeatures> {
        let (gray, width, height) = Self::to_grayscale(buffer);

        // Detect FAST corners
        let corners = Self::detect_fast_corners(
            &gray,
            width,
            height,
            self.config.fast_threshold,
            self.config.max_features,
        );

        debug!(num_keypoints = corners.len(), "Features detected");

        // Compute descriptors for each keypoint
        let mut keypoints = Vec::with_capacity(corners.len());
        let mut descriptors = Vec::with_capacity(corners.len());

        for &(x, y, response) in &corners {
            // Skip keypoints too close to the border for descriptor computation
            if x < 16.0 || y < 16.0 || x >= (width as f32 - 16.0) || y >= (height as f32 - 16.0) {
                continue;
            }

            keypoints.push(Keypoint {
                x,
                y,
                size: 31.0, // Patch size
                angle: 0.0, // Will be computed in descriptor
                response,
                octave: 0,
            });

            descriptors.push(Self::compute_descriptor(&gray, width, height, x, y));
        }

        debug!(
            num_keypoints = keypoints.len(),
            "Features with descriptors computed"
        );

        Ok(DetectedFeatures {
            keypoints,
            descriptors,
            image_dimensions: (width, height),
        })
    }

    /// Match features between two images using brute-force Hamming distance
    /// with Lowe's ratio test.
    #[instrument(skip(self, features_a, features_b))]
    pub fn match_features(
        &self,
        features_a: &DetectedFeatures,
        features_b: &DetectedFeatures,
    ) -> CoreResult<Vec<KeypointMatch>> {
        if features_a.keypoints.is_empty() || features_b.keypoints.is_empty() {
            return Err(CoreError::InsufficientFeatures {
                found: features_a.keypoints.len().min(features_b.keypoints.len()),
                required: self.config.min_matches,
            });
        }

        let mut good_matches = Vec::new();

        // For each descriptor in A, find the two best matches in B
        for (i, desc_a) in features_a.descriptors.iter().enumerate() {
            let mut best_dist = u32::MAX;
            let mut second_dist = u32::MAX;
            let mut best_idx = 0usize;

            for (j, desc_b) in features_b.descriptors.iter().enumerate() {
                let dist = desc_a.hamming_distance(desc_b);
                if dist < best_dist {
                    second_dist = best_dist;
                    best_dist = dist;
                    best_idx = j;
                } else if dist < second_dist {
                    second_dist = dist;
                }
            }

            // Lowe's ratio test
            if second_dist > 0
                && (best_dist as f32) < self.config.ratio_threshold * (second_dist as f32)
            {
                good_matches.push(KeypointMatch {
                    reference_idx: i,
                    target_idx: best_idx,
                    distance: best_dist as f32,
                });
            }
        }

        debug!(
            total_a = features_a.keypoints.len(),
            total_b = features_b.keypoints.len(),
            good_matches = good_matches.len(),
            "Feature matching complete"
        );

        if good_matches.len() < self.config.min_matches {
            return Err(CoreError::InsufficientFeatures {
                found: good_matches.len(),
                required: self.config.min_matches,
            });
        }

        Ok(good_matches)
    }

    /// Get the minimum required matches from config.
    pub fn min_matches(&self) -> usize {
        self.config.min_matches
    }
}

/// Result of feature detection on a single image.
pub struct DetectedFeatures {
    /// Detected keypoints in our internal format
    pub keypoints: Vec<Keypoint>,
    /// Binary descriptors (one per keypoint)
    pub descriptors: Vec<BinaryDescriptor>,
    /// Original image dimensions (width, height)
    pub image_dimensions: (u32, u32),
}

impl DetectedFeatures {
    /// Get the number of detected features.
    pub fn count(&self) -> usize {
        self.keypoints.len()
    }

    /// Check if features were detected.
    pub fn is_empty(&self) -> bool {
        self.keypoints.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_feature_detector_config_defaults() {
        let config = FeatureDetectorConfig::default();
        assert_eq!(config.detector_type, FeatureDetectorType::Orb);
        assert!(config.max_features > 0);
        assert!(config.ratio_threshold > 0.0 && config.ratio_threshold < 1.0);
    }

    #[test]
    fn test_feature_detector_creation() {
        let detector = FeatureDetector::with_defaults();
        assert_eq!(detector.config.detector_type, FeatureDetectorType::Orb);
    }

    #[test]
    fn test_min_matches_accessor() {
        let config = FeatureDetectorConfig {
            min_matches: 42,
            ..Default::default()
        };
        let detector = FeatureDetector::new(config);
        assert_eq!(detector.min_matches(), 42);
    }

    #[test]
    fn test_hamming_distance() {
        let a = BinaryDescriptor { data: [0xFF; 32] };
        let b = BinaryDescriptor { data: [0x00; 32] };
        assert_eq!(a.hamming_distance(&b), 256); // All bits differ

        let c = BinaryDescriptor { data: [0xFF; 32] };
        assert_eq!(a.hamming_distance(&c), 0); // Identical

        let mut d = BinaryDescriptor { data: [0xFF; 32] };
        d.data[0] = 0xFE; // 1 bit different
        assert_eq!(a.hamming_distance(&d), 1);
    }

    #[test]
    fn test_test_pairs_deterministic() {
        let pairs1 = FeatureDetector::get_test_pairs();
        let pairs2 = FeatureDetector::get_test_pairs();
        for i in 0..256 {
            assert_eq!(pairs1[i], pairs2[i], "Pair {} differs between calls", i);
        }
    }

    #[test]
    fn test_grayscale_conversion() {
        use image::RgbaImage;
        // White pixel
        let mut img = RgbaImage::new(2, 2);
        img.put_pixel(0, 0, image::Rgba([255, 255, 255, 255]));
        img.put_pixel(1, 0, image::Rgba([0, 0, 0, 255]));
        img.put_pixel(0, 1, image::Rgba([255, 0, 0, 255]));
        img.put_pixel(1, 1, image::Rgba([0, 255, 0, 255]));

        let buffer = RasterBuffer::new(img, 300);
        let (gray, w, h) = FeatureDetector::to_grayscale(&buffer);
        assert_eq!(w, 2);
        assert_eq!(h, 2);
        assert_eq!(gray[0], 255); // White -> 255
        assert_eq!(gray[1], 0); // Black -> 0
        assert!(gray[2] > 50 && gray[2] < 100); // Red -> ~76
        assert!(gray[3] > 130 && gray[3] < 170); // Green -> ~150
    }
}
