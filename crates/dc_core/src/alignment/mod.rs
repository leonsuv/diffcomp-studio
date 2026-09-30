// =============================================================================
// dc_core/alignment - Image Alignment Module (v2: Pure Rust)
// =============================================================================
// Provides feature-based image alignment without any OpenCV dependency.
//
// ## Algorithm Pipeline
// 1. FAST corner detection → keypoints
// 2. Rotated binary descriptors (rBRIEF-like) → descriptors
// 3. Brute-force Hamming matching + Lowe's ratio test → correspondences
// 4. DLT + RANSAC → robust 3×3 homography matrix
// 5. Pure Rust bilinear interpolation warp → aligned image
//
// All code is pure Rust, no system dependencies, WASM-compatible.
// =============================================================================

mod engine;
mod features;
/// Force-fit alignment based on content bounding boxes (ported from Python compareTIFF.py).
pub mod force_fit;
mod homography;

// Re-export the public API
pub use engine::{AlignmentConfig, AlignmentEngine, AlignmentResult};
pub use features::{
    BinaryDescriptor, DetectedFeatures, FeatureDetector, FeatureDetectorConfig, FeatureDetectorType,
};
pub use force_fit::{force_fit_align, ForceFitInfo, ForceFitResult};
pub use homography::{HomographyMatrix, HomographySolver};
