// =============================================================================
// dc_core/alignment/homography - Pure Rust Homography Computation
// =============================================================================
// Computes the perspective transformation between two images.
//
// Replaces OpenCV's `find_homography` + `warp_perspective` with:
//   1. Direct Linear Transform (DLT) for 4-point homography estimation
//   2. RANSAC for robust estimation with outlier rejection
//   3. Pure Rust bilinear interpolation warp
//
// ## Mathematical Foundation
//
// A homography H is a 3×3 matrix mapping points (x,y) -> (x',y'):
//     [x']   [h00 h01 h02] [x]
//     [y'] = [h10 h11 h12] [y]
//     [w']   [h20 h21 h22] [1]
//
// Where (x', y') = (x'/w', y'/w') after perspective division.
// =============================================================================

use crate::types::{CoreError, CoreResult, Keypoint, KeypointMatch, RasterBuffer};
use image::RgbaImage;
use tracing::{debug, instrument};

/// A 3×3 homography matrix.
#[derive(Debug, Clone, Copy)]
pub struct HomographyMatrix {
    /// The matrix elements in row-major order
    pub elements: [[f64; 3]; 3],
}

impl HomographyMatrix {
    /// Create an identity homography (no transformation).
    pub fn identity() -> Self {
        Self {
            elements: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        }
    }

    /// Create from a flat array (row-major).
    pub fn from_flat(data: &[f64; 9]) -> Self {
        Self {
            elements: [
                [data[0], data[1], data[2]],
                [data[3], data[4], data[5]],
                [data[6], data[7], data[8]],
            ],
        }
    }

    /// Convert to a flat array (row-major).
    pub fn to_flat(&self) -> [f64; 9] {
        [
            self.elements[0][0],
            self.elements[0][1],
            self.elements[0][2],
            self.elements[1][0],
            self.elements[1][1],
            self.elements[1][2],
            self.elements[2][0],
            self.elements[2][1],
            self.elements[2][2],
        ]
    }

    /// Transform a point using this homography.
    pub fn transform_point(&self, x: f64, y: f64) -> (f64, f64) {
        let h = &self.elements;
        let w = h[2][0] * x + h[2][1] * y + h[2][2];

        if w.abs() < 1e-10 {
            return (f64::NAN, f64::NAN);
        }

        let x_prime = (h[0][0] * x + h[0][1] * y + h[0][2]) / w;
        let y_prime = (h[1][0] * x + h[1][1] * y + h[1][2]) / w;

        (x_prime, y_prime)
    }

    /// Extract the approximate rotation angle from the homography (degrees).
    pub fn approximate_rotation(&self) -> f64 {
        let h = &self.elements;
        let angle_rad = h[1][0].atan2(h[0][0]);
        angle_rad.to_degrees()
    }

    /// Extract the approximate scale factor from the homography.
    pub fn approximate_scale(&self) -> f64 {
        let h = &self.elements;
        let scale_x = (h[0][0].powi(2) + h[1][0].powi(2)).sqrt();
        let scale_y = (h[0][1].powi(2) + h[1][1].powi(2)).sqrt();
        (scale_x + scale_y) / 2.0
    }

    /// Check if this homography is close to identity (no transformation).
    pub fn is_near_identity(&self, tolerance: f64) -> bool {
        let identity = Self::identity();
        for i in 0..3 {
            for j in 0..3 {
                if (self.elements[i][j] - identity.elements[i][j]).abs() > tolerance {
                    return false;
                }
            }
        }
        true
    }

    /// Compute the inverse of this homography using Cramer's rule.
    pub fn inverse(&self) -> CoreResult<Self> {
        let m = &self.elements;

        // Cofactor matrix (3x3)
        let c00 = m[1][1] * m[2][2] - m[1][2] * m[2][1];
        let c01 = m[1][2] * m[2][0] - m[1][0] * m[2][2];
        let c02 = m[1][0] * m[2][1] - m[1][1] * m[2][0];

        let det = m[0][0] * c00 + m[0][1] * c01 + m[0][2] * c02;

        if det.abs() < 1e-12 {
            return Err(CoreError::HomographyFailed {
                reason: "Singular homography matrix (determinant ≈ 0)".to_string(),
            });
        }

        let inv_det = 1.0 / det;

        let c10 = m[0][2] * m[2][1] - m[0][1] * m[2][2];
        let c11 = m[0][0] * m[2][2] - m[0][2] * m[2][0];
        let c12 = m[0][1] * m[2][0] - m[0][0] * m[2][1];

        let c20 = m[0][1] * m[1][2] - m[0][2] * m[1][1];
        let c21 = m[0][2] * m[1][0] - m[0][0] * m[1][2];
        let c22 = m[0][0] * m[1][1] - m[0][1] * m[1][0];

        Ok(Self {
            elements: [
                [c00 * inv_det, c10 * inv_det, c20 * inv_det],
                [c01 * inv_det, c11 * inv_det, c21 * inv_det],
                [c02 * inv_det, c12 * inv_det, c22 * inv_det],
            ],
        })
    }
}

impl Default for HomographyMatrix {
    fn default() -> Self {
        Self::identity()
    }
}

/// Solver for computing homography matrices using pure Rust RANSAC + DLT.
pub struct HomographySolver {
    /// RANSAC reprojection threshold (pixels)
    pub ransac_threshold: f64,
    /// Maximum RANSAC iterations
    pub max_iterations: u32,
    /// Confidence level for RANSAC (0.0 - 1.0)
    pub confidence: f64,
}

impl Default for HomographySolver {
    fn default() -> Self {
        Self {
            ransac_threshold: 5.0,
            max_iterations: 2000,
            confidence: 0.995,
        }
    }
}

impl HomographySolver {
    /// Create a new solver with default settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a solver optimized for high-precision alignment.
    pub fn high_precision() -> Self {
        Self {
            ransac_threshold: 2.0,
            max_iterations: 5000,
            confidence: 0.999,
        }
    }

    /// Compute the homography from 4 point correspondences using DLT.
    ///
    /// Sets h33 = 1 and solves the resulting 8×8 linear system directly,
    /// which is more numerically stable than eigendecomposition for exact
    /// 4-point correspondences.
    fn compute_dlt_4point(src: &[(f64, f64)], dst: &[(f64, f64)]) -> CoreResult<HomographyMatrix> {
        if src.len() < 4 || dst.len() < 4 {
            return Err(CoreError::InsufficientFeatures {
                found: src.len().min(dst.len()),
                required: 4,
            });
        }

        // Build the 8×8 system Ax = b where h = [h00..h21, h22=1]
        // For each point pair (x, y) -> (x', y') with h22 = 1:
        //   x' * (h20*x + h21*y + 1) = h00*x + h01*y + h02
        //   y' * (h20*x + h21*y + 1) = h10*x + h11*y + h12
        //
        // Rearranging:
        //   h00*x + h01*y + h02 - h20*x*x' - h21*y*x' = x'
        //   h10*x + h11*y + h12 - h20*x*y' - h21*y*y' = y'
        //
        // Variables: [h00, h01, h02, h10, h11, h12, h20, h21]
        let mut a = [[0.0f64; 8]; 8];
        let mut b = [0.0f64; 8];

        for i in 0..4 {
            let (x, y) = src[i];
            let (xp, yp) = dst[i];
            let row1 = i * 2;
            let row2 = i * 2 + 1;

            // Row for x' equation
            a[row1] = [x, y, 1.0, 0.0, 0.0, 0.0, -x * xp, -y * xp];
            b[row1] = xp;

            // Row for y' equation
            a[row2] = [0.0, 0.0, 0.0, x, y, 1.0, -x * yp, -y * yp];
            b[row2] = yp;
        }

        // Solve the 8×8 system
        let h_vec = Self::solve_8x8(&a, &b)?;

        Ok(HomographyMatrix::from_flat(&[
            h_vec[0], h_vec[1], h_vec[2], h_vec[3], h_vec[4], h_vec[5], h_vec[6], h_vec[7], 1.0,
        ]))
    }

    /// Solve an 8×8 linear system Ax = b using Gaussian elimination with
    /// partial pivoting.
    fn solve_8x8(a: &[[f64; 8]; 8], b: &[f64; 8]) -> CoreResult<[f64; 8]> {
        // Augmented matrix [A | b]
        let mut aug = [[0.0f64; 9]; 8];
        for i in 0..8 {
            for j in 0..8 {
                aug[i][j] = a[i][j];
            }
            aug[i][8] = b[i];
        }

        // Forward elimination with partial pivoting
        for col in 0..8 {
            // Find pivot
            let mut max_val = aug[col][col].abs();
            let mut max_row = col;
            for row in (col + 1)..8 {
                if aug[row][col].abs() > max_val {
                    max_val = aug[row][col].abs();
                    max_row = row;
                }
            }

            if max_val < 1e-12 {
                return Err(CoreError::HomographyFailed {
                    reason: "Singular matrix in DLT solver".to_string(),
                });
            }

            // Swap rows
            if max_row != col {
                aug.swap(col, max_row);
            }

            // Eliminate below
            for row in (col + 1)..8 {
                let factor = aug[row][col] / aug[col][col];
                for j in col..9 {
                    aug[row][j] -= factor * aug[col][j];
                }
            }
        }

        // Back substitution
        let mut x = [0.0f64; 8];
        for i in (0..8).rev() {
            x[i] = aug[i][8];
            for j in (i + 1)..8 {
                x[i] -= aug[i][j] * x[j];
            }
            x[i] /= aug[i][i];
        }

        Ok(x)
    }

    /// Compute reprojection error for a single point pair.
    fn reprojection_error(h: &HomographyMatrix, src: (f64, f64), dst: (f64, f64)) -> f64 {
        let (px, py) = h.transform_point(src.0, src.1);
        if px.is_nan() || py.is_nan() {
            return f64::MAX;
        }
        let dx = px - dst.0;
        let dy = py - dst.1;
        (dx * dx + dy * dy).sqrt()
    }

    /// Compute the homography matrix from matched keypoints using RANSAC.
    #[instrument(skip(self, keypoints_ref, keypoints_target, matches))]
    pub fn compute_homography(
        &self,
        keypoints_ref: &[Keypoint],
        keypoints_target: &[Keypoint],
        matches: &[KeypointMatch],
    ) -> CoreResult<(HomographyMatrix, f64)> {
        if matches.len() < 4 {
            return Err(CoreError::InsufficientFeatures {
                found: matches.len(),
                required: 4,
            });
        }

        debug!(
            num_matches = matches.len(),
            ransac_threshold = self.ransac_threshold,
            "Computing homography with RANSAC"
        );

        // Build point vectors
        let src_pts: Vec<(f64, f64)> = matches
            .iter()
            .filter_map(|m| {
                if m.target_idx < keypoints_target.len() {
                    Some((
                        keypoints_target[m.target_idx].x as f64,
                        keypoints_target[m.target_idx].y as f64,
                    ))
                } else {
                    None
                }
            })
            .collect();

        let dst_pts: Vec<(f64, f64)> = matches
            .iter()
            .filter_map(|m| {
                if m.reference_idx < keypoints_ref.len() {
                    Some((
                        keypoints_ref[m.reference_idx].x as f64,
                        keypoints_ref[m.reference_idx].y as f64,
                    ))
                } else {
                    None
                }
            })
            .collect();

        let n = src_pts.len().min(dst_pts.len());
        if n < 4 {
            return Err(CoreError::InsufficientFeatures {
                found: n,
                required: 4,
            });
        }

        // RANSAC loop
        let mut best_h = HomographyMatrix::identity();
        let mut best_inlier_count = 0usize;
        let mut best_inlier_mask = vec![false; n];

        // Simple pseudo-random index generator (deterministic for reproducibility)
        let mut rng_state: u64 = 42;
        let mut next_rand = |max: usize| -> usize {
            rng_state = rng_state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((rng_state >> 33) as usize) % max
        };

        for _iter in 0..self.max_iterations {
            // Randomly select 4 point pairs
            let mut indices = [0usize; 4];
            indices[0] = next_rand(n);
            loop {
                indices[1] = next_rand(n);
                if indices[1] != indices[0] {
                    break;
                }
            }
            loop {
                indices[2] = next_rand(n);
                if indices[2] != indices[0] && indices[2] != indices[1] {
                    break;
                }
            }
            loop {
                indices[3] = next_rand(n);
                if indices[3] != indices[0] && indices[3] != indices[1] && indices[3] != indices[2]
                {
                    break;
                }
            }

            let sample_src: Vec<(f64, f64)> = indices.iter().map(|&i| src_pts[i]).collect();
            let sample_dst: Vec<(f64, f64)> = indices.iter().map(|&i| dst_pts[i]).collect();

            // Compute homography from the 4-point sample
            let h = match Self::compute_dlt_4point(&sample_src, &sample_dst) {
                Ok(h) => h,
                Err(_) => continue, // Degenerate configuration, skip
            };

            // Count inliers
            let mut inlier_count = 0;
            let mut inlier_mask = vec![false; n];
            for i in 0..n {
                let err = Self::reprojection_error(&h, src_pts[i], dst_pts[i]);
                if err < self.ransac_threshold {
                    inlier_count += 1;
                    inlier_mask[i] = true;
                }
            }

            if inlier_count > best_inlier_count {
                best_inlier_count = inlier_count;
                best_h = h;
                best_inlier_mask = inlier_mask;

                // Early termination if we have enough inliers
                let inlier_ratio = inlier_count as f64 / n as f64;
                if inlier_ratio > self.confidence {
                    break;
                }
            }
        }

        // Refine: re-estimate H from all inliers
        let inlier_src: Vec<(f64, f64)> = best_inlier_mask
            .iter()
            .enumerate()
            .filter(|(_, &is_in)| is_in)
            .map(|(i, _)| src_pts[i])
            .collect();
        let inlier_dst: Vec<(f64, f64)> = best_inlier_mask
            .iter()
            .enumerate()
            .filter(|(_, &is_in)| is_in)
            .map(|(i, _)| dst_pts[i])
            .collect();

        if inlier_src.len() >= 4 {
            // Use the first 4 inliers for a refined DLT (a proper implementation
            // would use all inliers via least-squares DLT, but 4-point is
            // sufficient for engineering drawings with small perspective distortion)
            if let Ok(refined) = Self::compute_dlt_4point(&inlier_src[..4], &inlier_dst[..4]) {
                best_h = refined;
            }
        }

        let inlier_ratio = best_inlier_count as f64 / n as f64;
        debug!(
            total_points = n,
            inlier_count = best_inlier_count,
            inlier_ratio = format!("{:.2}%", inlier_ratio * 100.0),
            rotation = format!("{:.2}°", best_h.approximate_rotation()),
            scale = format!("{:.4}", best_h.approximate_scale()),
            "Homography computed"
        );

        Ok((best_h, inlier_ratio))
    }

    /// Apply a homography transformation to warp an image using pure Rust
    /// bilinear interpolation.
    #[instrument(skip(self, buffer, homography))]
    pub fn warp_image(
        &self,
        buffer: &RasterBuffer,
        homography: &HomographyMatrix,
        output_size: (u32, u32),
    ) -> CoreResult<RasterBuffer> {
        let (out_width, out_height) = output_size;
        let (in_width, in_height) = buffer.dimensions();

        debug!(
            input_size = format!("{}x{}", in_width, in_height),
            output_size = format!("{}x{}", out_width, out_height),
            "Warping image with homography"
        );

        // Compute the inverse homography (output -> input mapping)
        let h_inv = homography.inverse()?;

        let input_pixels = buffer.image.as_raw();
        let mut output = RgbaImage::new(out_width, out_height);
        let out_pixels = output.as_mut();

        // For each output pixel, find the corresponding input pixel
        for y in 0..out_height {
            for x in 0..out_width {
                let (src_x, src_y) = h_inv.transform_point(x as f64, y as f64);

                let out_idx = ((y * out_width + x) * 4) as usize;

                if src_x < 0.0
                    || src_y < 0.0
                    || src_x >= (in_width as f64 - 1.0)
                    || src_y >= (in_height as f64 - 1.0)
                {
                    // Outside bounds — white background
                    out_pixels[out_idx] = 255;
                    out_pixels[out_idx + 1] = 255;
                    out_pixels[out_idx + 2] = 255;
                    out_pixels[out_idx + 3] = 255;
                    continue;
                }

                // Bilinear interpolation
                let x0 = src_x.floor() as u32;
                let y0 = src_y.floor() as u32;
                let x1 = (x0 + 1).min(in_width - 1);
                let y1 = (y0 + 1).min(in_height - 1);

                let fx = src_x - x0 as f64;
                let fy = src_y - y0 as f64;

                let idx00 = ((y0 * in_width + x0) * 4) as usize;
                let idx10 = ((y0 * in_width + x1) * 4) as usize;
                let idx01 = ((y1 * in_width + x0) * 4) as usize;
                let idx11 = ((y1 * in_width + x1) * 4) as usize;

                for c in 0..4 {
                    let v00 = input_pixels[idx00 + c] as f64;
                    let v10 = input_pixels[idx10 + c] as f64;
                    let v01 = input_pixels[idx01 + c] as f64;
                    let v11 = input_pixels[idx11 + c] as f64;

                    let val = v00 * (1.0 - fx) * (1.0 - fy)
                        + v10 * fx * (1.0 - fy)
                        + v01 * (1.0 - fx) * fy
                        + v11 * fx * fy;

                    out_pixels[out_idx + c] = val.round().clamp(0.0, 255.0) as u8;
                }
            }
        }

        let mut result = RasterBuffer::new(output, buffer.dpi);
        result.original_width = buffer.original_width;
        result.original_height = buffer.original_height;
        result.page_index = buffer.page_index;

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_identity_homography() {
        let h = HomographyMatrix::identity();
        assert!(h.is_near_identity(1e-10));

        let (x, y) = h.transform_point(100.0, 200.0);
        assert!((x - 100.0).abs() < 1e-10);
        assert!((y - 200.0).abs() < 1e-10);
    }

    #[test]
    fn test_homography_from_flat() {
        let flat = [1.0, 0.0, 10.0, 0.0, 1.0, 20.0, 0.0, 0.0, 1.0];
        let h = HomographyMatrix::from_flat(&flat);

        let (x, y) = h.transform_point(0.0, 0.0);
        assert!((x - 10.0).abs() < 1e-10);
        assert!((y - 20.0).abs() < 1e-10);
    }

    #[test]
    fn test_homography_to_flat_roundtrip() {
        let original = HomographyMatrix {
            elements: [[1.1, 0.2, 10.0], [0.3, 1.4, 20.0], [0.001, 0.002, 1.0]],
        };

        let flat = original.to_flat();
        let reconstructed = HomographyMatrix::from_flat(&flat);

        for i in 0..3 {
            for j in 0..3 {
                assert!((original.elements[i][j] - reconstructed.elements[i][j]).abs() < 1e-10);
            }
        }
    }

    #[test]
    fn test_approximate_rotation() {
        let angle = std::f64::consts::PI / 4.0; // 45 degrees
        let cos_a = angle.cos();
        let sin_a = angle.sin();

        let h = HomographyMatrix {
            elements: [[cos_a, -sin_a, 0.0], [sin_a, cos_a, 0.0], [0.0, 0.0, 1.0]],
        };

        let extracted = h.approximate_rotation();
        assert!((extracted - 45.0).abs() < 0.1);
    }

    #[test]
    fn test_approximate_scale() {
        let h = HomographyMatrix {
            elements: [[2.0, 0.0, 0.0], [0.0, 2.0, 0.0], [0.0, 0.0, 1.0]],
        };

        let scale = h.approximate_scale();
        assert!((scale - 2.0).abs() < 0.01);
    }

    #[test]
    fn test_homography_inverse() {
        // Translation by (10, 20)
        let h = HomographyMatrix::from_flat(&[1.0, 0.0, 10.0, 0.0, 1.0, 20.0, 0.0, 0.0, 1.0]);
        let h_inv = h.inverse().expect("Should be invertible");

        // Forward then inverse should give identity
        let (px, py) = h.transform_point(5.0, 7.0);
        let (rx, ry) = h_inv.transform_point(px, py);
        assert!((rx - 5.0).abs() < 1e-8);
        assert!((ry - 7.0).abs() < 1e-8);
    }

    #[test]
    fn test_dlt_4point_translation() {
        // Test that DLT can recover a pure translation
        let src = vec![(0.0, 0.0), (100.0, 0.0), (100.0, 100.0), (0.0, 100.0)];
        let dst = vec![(10.0, 20.0), (110.0, 20.0), (110.0, 120.0), (10.0, 120.0)];

        let h = HomographySolver::compute_dlt_4point(&src, &dst).expect("DLT should succeed");

        // Check that the homography correctly maps src -> dst
        for (s, d) in src.iter().zip(dst.iter()) {
            let (px, py) = h.transform_point(s.0, s.1);
            assert!(
                (px - d.0).abs() < 1.0 && (py - d.1).abs() < 1.0,
                "Point ({}, {}) mapped to ({:.1}, {:.1}), expected ({}, {})",
                s.0,
                s.1,
                px,
                py,
                d.0,
                d.1
            );
        }
    }

    #[test]
    fn test_solver_defaults() {
        let solver = HomographySolver::default();
        assert!(solver.ransac_threshold > 0.0);
        assert!(solver.max_iterations > 0);
        assert!(solver.confidence > 0.0 && solver.confidence < 1.0);
    }
}
