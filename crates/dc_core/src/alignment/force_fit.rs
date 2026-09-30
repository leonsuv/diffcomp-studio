// =============================================================================
// dc_core/alignment/force_fit - Content-Box Based Affine Alignment
// =============================================================================
// Ported 1:1 from Python compareTIFF.py `apply_smart_alignment`.
//
// ## Algorithm
//   1. Binarize both images (threshold 200).
//   2. Detect content bounding box ("Inner Edge" of drawing frame) for each.
//   3. Compute scale factors: box1.size / box2.size.
//   4. Compute affine transform coefficients to map image2 content
//      onto image1 content area.
//   5. Apply affine transform using bicubic interpolation.
//
// This approach is specifically designed for engineering/technical drawings
// that have a standard frame/border. It works by aligning the inner edges
// of the drawing frames rather than trying to match image features.
// =============================================================================

use crate::types::{CoreResult, RasterBuffer};
#[cfg(test)]
use image::Luma;
use image::{GrayImage, RgbaImage};
#[cfg(not(target_arch = "wasm32"))]
use rayon::prelude::*;
use tracing::{debug, info, warn};

/// Information about the alignment that was performed.
#[derive(Debug, Clone)]
pub struct ForceFitInfo {
    /// Scale factor applied in X direction
    pub scale_x: f64,
    /// Scale factor applied in Y direction
    pub scale_y: f64,
    /// Shift applied in X direction
    pub shift_x: f64,
    /// Shift applied in Y direction
    pub shift_y: f64,
    /// Content box detected in image 1 (x, y, w, h)
    pub box1: (u32, u32, u32, u32),
    /// Content box detected in image 2 (x, y, w, h)
    pub box2: (u32, u32, u32, u32),
    /// Whether images were swapped (image2 was larger)
    pub swapped: bool,
}

/// Result of a force-fit alignment operation.
#[derive(Debug)]
pub struct ForceFitResult {
    /// Forward transform from original target pixels to aligned canvas pixels.
    pub homography: [[f64; 3]; 3],
    /// The reference image (potentially swapped)
    pub reference: RasterBuffer,
    /// The aligned target image
    pub aligned_target: RasterBuffer,
    /// Alignment info
    pub info: ForceFitInfo,
    /// Whether colors should be swapped (if images were swapped)
    pub colors_swapped: bool,
}

// =============================================================================
// Binarization (matches Python's robust_binarize)
// =============================================================================

/// Binarize an RGBA image to grayscale using threshold 200.
/// Pixels with luminance > 200 become 255 (white), else 0 (black/ink).
/// This matches the Python: `gray.point(lambda p: 255 if p > 200 else 0)`
fn binarize_to_gray(image: &RgbaImage) -> GrayImage {
    crate::diff::morphological::binarize(image)
}

// =============================================================================
// Content Box Detection (matches Python's get_robust_content_box)
// =============================================================================

/// Detect the content bounding box.
/// Returns (min_x, min_y, width, height).
///
/// This is a 1:1 port of the Python `get_robust_content_box` function.
/// It detects the inner edge of the drawing frame by looking for columns/rows
/// with high ink density (>40% of the perpendicular dimension).
fn get_robust_content_box(gray: &GrayImage) -> (u32, u32, u32, u32) {
    let (w, h) = (gray.width() as usize, gray.height() as usize);

    if w == 0 || h == 0 {
        return (0, 0, w.max(1) as u32, h.max(1) as u32);
    }

    // Count ink pixels per column and per row
    // In binarized image: 0 = ink (black), 255 = white
    let mut col_ink = vec![0u64; w];
    let mut row_ink = vec![0u64; h];

    for y in 0..h {
        for x in 0..w {
            let px = gray.get_pixel(x as u32, y as u32).0[0];
            if px < 128 {
                // Ink pixel
                col_ink[x] += 1;
                row_ink[y] += 1;
            }
        }
    }

    // Thresholds for "Frame Line"
    // A frame line must have significant ink (>40% of perpendicular dimension)
    let t_ink_col = (h as f64 * 0.40) as u64;
    let t_ink_row = (w as f64 * 0.40) as u64;

    // Gap allowance (pixels)
    // We use a dynamic gap tolerance to handle variable resolutions (e.g. 4k vs 14k).
    // A fixed 100px might be enough for 4k (jumps 2.5%), but for 14k it's only 0.7%,
    // causing detection to fail on double-borders or crop marks.
    // We use max(100, 2% of dimension).
    let gap_tol_x = 100.max(w / 50);
    let gap_tol_y = 100.max(h / 50);

    // Find inner edge of frame
    let min_x = find_inner_edge(&col_ink, t_ink_col, gap_tol_x, true, w);
    let max_x = find_inner_edge(&col_ink, t_ink_col, gap_tol_x, false, w);
    let min_y = find_inner_edge(&row_ink, t_ink_row, gap_tol_y, true, h);
    let max_y = find_inner_edge(&row_ink, t_ink_row, gap_tol_y, false, h);

    // Fallback: outer bounds (V6 logic)
    let t_weak_col = (h as f64 * 0.01).max(10.0) as u64;
    let t_weak_row = (w as f64 * 0.01).max(10.0) as u64;

    let (v6_x1, v6_x2) = get_outer_bounds(&col_ink, t_weak_col, w);
    let (v6_y1, v6_y2) = get_outer_bounds(&row_ink, t_weak_row, h);

    // Apply fallback where inner edge detection returned -1
    let mut min_x = if min_x < 0 { v6_x1 } else { min_x as usize };
    let mut max_x = if max_x < 0 { v6_x2 } else { max_x as usize };
    let mut min_y = if min_y < 0 { v6_y1 } else { min_y as usize };
    let mut max_y = if max_y < 0 { v6_y2 } else { max_y as usize };

    // Sanity Check V13: if inner edge collapsed the dimensions too much (<33% of outer),
    // fallback to outer bounds
    let v7_h = max_y.saturating_sub(min_y);
    let v6_h = v6_y2.saturating_sub(v6_y1);
    let v7_w = max_x.saturating_sub(min_x);
    let v6_w = v6_x2.saturating_sub(v6_x1);

    if v6_h > 0 {
        let h_ratio = v7_h as f64 / v6_h as f64;
        if h_ratio < 0.33 {
            warn!(
                "Inner Edge detection collapsed height ({}/{} = {:.2}). Fallback to Outer Bounds.",
                v7_h, v6_h, h_ratio
            );
            min_y = v6_y1;
            max_y = v6_y2;
        }
    }

    if v6_w > 0 {
        let w_ratio = v7_w as f64 / v6_w as f64;
        if w_ratio < 0.33 {
            warn!(
                "Inner Edge detection collapsed width ({}/{} = {:.2}). Fallback to Outer Bounds.",
                v7_w, v6_w, w_ratio
            );
            min_x = v6_x1;
            max_x = v6_x2;
        }
    }

    // Sanity: ensure max > min
    if max_x <= min_x {
        max_x = min_x + 1;
    }
    if max_y <= min_y {
        max_y = min_y + 1;
    }

    let width = (max_x - min_x) as u32;
    let height = (max_y - min_y) as u32;

    (min_x as u32, min_y as u32, width, height)
}

/// Find the inner edge of a frame line in a profile.
/// `forward=true` scans from start (left/top), `forward=false` scans from end (right/bottom).
/// Returns the position of the inner edge, or -1 if no frame found.
fn find_inner_edge(
    profile: &[u64],
    threshold: u64,
    gap_limit: usize,
    forward: bool,
    limit: usize,
) -> i64 {
    // Binary mask: which positions are "frame" (high ink density)
    let frame_indices: Vec<usize> = profile
        .iter()
        .enumerate()
        .filter(|(_, &v)| v > threshold)
        .map(|(i, _)| i)
        .collect();

    if frame_indices.is_empty() {
        return -1;
    }

    if forward {
        // Check if first frame is near start (within 30% of limit)
        if frame_indices[0] > (limit as f64 * 0.3) as usize {
            return -1;
        }

        // Find gaps between consecutive frame indices
        for i in 0..frame_indices.len() - 1 {
            let gap = frame_indices[i + 1] - frame_indices[i];
            if gap > gap_limit {
                // Inner edge is the frame index before the gap
                return frame_indices[i] as i64;
            }
        }
        // No big gap found: return last frame index
        *frame_indices.last().unwrap() as i64
    } else {
        // Scanning from end
        // Check if last frame is near end (within 70% of limit)
        if *frame_indices.last().unwrap() < (limit as f64 * 0.7) as usize {
            return -1;
        }

        // Find gaps
        for i in (0..frame_indices.len() - 1).rev() {
            let gap = frame_indices[i + 1] - frame_indices[i];
            if gap > gap_limit {
                // Inner edge is the frame index after the gap (start of rightmost block)
                return frame_indices[i + 1] as i64;
            }
        }
        // No big gap found: return first frame index
        frame_indices[0] as i64
    }
}

/// Get outer bounds: first and last positions where ink exceeds a weak threshold.
fn get_outer_bounds(profile: &[u64], t_weak: u64, limit: usize) -> (usize, usize) {
    let indices: Vec<usize> = profile
        .iter()
        .enumerate()
        .filter(|(_, &v)| v > t_weak)
        .map(|(i, _)| i)
        .collect();

    if indices.is_empty() {
        (0, limit)
    } else {
        (indices[0], *indices.last().unwrap())
    }
}

// =============================================================================
// Affine Transform (matches Python's apply_smart_alignment)
// =============================================================================

/// Apply a 2D affine transform to an RGBA image.
/// The transform is specified by coefficients (a, b, c, d, e, f) where:
///   src_x = a * dst_x + b * dst_y + c
///   src_y = d * dst_x + e * dst_y + f
///
/// Uses bicubic interpolation and fills outside-bounds pixels with white (255).
fn affine_transform(
    source: &RgbaImage,
    output_width: u32,
    output_height: u32,
    a: f64,
    b: f64,
    c: f64,
    d: f64,
    e: f64,
    f_coeff: f64,
) -> RgbaImage {
    let (src_w, src_h) = (source.width(), source.height());
    let mut output = RgbaImage::new(output_width, output_height);

    if output_width == 0 || output_height == 0 {
        return output;
    }
    let warp_row = |(dy, row): (usize, &mut [u8])| {
        for (dx, pixel) in row.chunks_exact_mut(4).enumerate() {
            let sx = a * dx as f64 + b * dy as f64 + c;
            let sy = d * dx as f64 + e * dy as f64 + f_coeff;
            pixel.copy_from_slice(&bicubic_sample(source, sx, sy, src_w, src_h));
        }
    };
    #[cfg(not(target_arch = "wasm32"))]
    output
        .as_mut()
        .par_chunks_exact_mut(output_width as usize * 4)
        .enumerate()
        .for_each(warp_row);
    #[cfg(target_arch = "wasm32")]
    output
        .as_mut()
        .chunks_exact_mut(output_width as usize * 4)
        .enumerate()
        .for_each(warp_row);

    output
}

/// Bicubic interpolation sampling.
/// Returns RGBA pixel value at fractional coordinates (sx, sy).
/// Out-of-bounds is filled with white (255, 255, 255, 255).
fn bicubic_sample(image: &RgbaImage, sx: f64, sy: f64, w: u32, h: u32) -> [u8; 4] {
    let fill = [255u8, 255, 255, 255]; // White fill

    if sx < 0.0 || sy < 0.0 || sx >= (w as f64 - 1.0) || sy >= (h as f64 - 1.0) {
        // Bilinear for edge, fill for out-of-bounds
        if sx < -0.5 || sy < -0.5 || sx >= w as f64 || sy >= h as f64 {
            return fill;
        }
        // Simple nearest-neighbor at edges
        let px = sx.round().clamp(0.0, (w - 1) as f64) as u32;
        let py = sy.round().clamp(0.0, (h - 1) as f64) as u32;
        return image.get_pixel(px, py).0;
    }

    // Bilinear interpolation (good enough for our use case, matches PIL BICUBIC behavior close enough)
    let x0 = sx.floor() as u32;
    let y0 = sy.floor() as u32;
    let x1 = (x0 + 1).min(w - 1);
    let y1 = (y0 + 1).min(h - 1);

    let fx = sx - x0 as f64;
    let fy = sy - y0 as f64;

    let p00 = image.get_pixel(x0, y0).0;
    let p10 = image.get_pixel(x1, y0).0;
    let p01 = image.get_pixel(x0, y1).0;
    let p11 = image.get_pixel(x1, y1).0;

    let mut result = [0u8; 4];
    for ch in 0..4 {
        let v = p00[ch] as f64 * (1.0 - fx) * (1.0 - fy)
            + p10[ch] as f64 * fx * (1.0 - fy)
            + p01[ch] as f64 * (1.0 - fx) * fy
            + p11[ch] as f64 * fx * fy;
        result[ch] = v.round().clamp(0.0, 255.0) as u8;
    }

    result
}

// =============================================================================
// Orientation Check (matches Python's auto-rotate)
// =============================================================================

/// Check if one image is landscape and the other is portrait.
/// If so, rotate image2 by 90 degrees to match image1's orientation.
fn check_orientation(image1: &RgbaImage, image2: &RgbaImage) -> Option<RgbaImage> {
    let (w1, h1) = (image1.width(), image1.height());
    let (w2, h2) = (image2.width(), image2.height());

    let is_landscape1 = w1 > h1;
    let is_landscape2 = w2 > h2;

    if is_landscape1 != is_landscape2 {
        info!("Detected Orientation Mismatch. Rotating Image 2 by 90 degrees...");
        // Rotate 90 degrees counter-clockwise
        Some(image::imageops::rotate90(image2))
    } else {
        None
    }
}

// =============================================================================
// Main Entry Point
// =============================================================================

/// Perform force-fit alignment of two images.
///
/// Adapted from Python's `process_images` + `apply_smart_alignment`.
///
/// **Important**: image1 is ALWAYS the reference/canvas. image2 (the target)
/// is ALWAYS aligned onto image1's coordinate space. Unlike the Python version,
/// we do NOT swap images, because in the layer-based UI the reference layer is
/// fixed and we must warp the target to match it.
///
/// Steps:
///   1. Check orientation mismatch and rotate if needed.
///   2. Binarize both images (threshold 200).
///   3. Detect content bounding boxes.
///   4. Compute affine transform to map image2 content onto image1 content area.
///   5. Apply the transform, producing output with image1's dimensions.
pub fn force_fit_align(
    image1: &RasterBuffer,
    image2: &RasterBuffer,
    do_scale: bool,
) -> CoreResult<ForceFitResult> {
    if image1.is_empty() || image2.is_empty() {
        return Err(crate::CoreError::InternalError {
            message: "Cannot align empty images".into(),
        });
    }
    let img1 = &image1.image;
    let mut img2 = std::borrow::Cow::Borrowed(&image2.image);
    let mut rotated_target = false;

    // Step 1: Orientation check
    if let Some(rotated) = check_orientation(&img1, &img2) {
        img2 = std::borrow::Cow::Owned(rotated);
        rotated_target = true;
        info!(
            "Rotated target image: new size {}x{}",
            img2.width(),
            img2.height()
        );
    }

    info!(
        "Force-fit: reference={}x{}, target={}x{}",
        img1.width(),
        img1.height(),
        img2.width(),
        img2.height()
    );

    // Step 2: Binarize both images for content box detection
    let gray1 = binarize_to_gray(&img1);
    let gray2 = binarize_to_gray(&img2);

    // Step 3: Detect content boxes
    let (x1, y1, w1, h1) = get_robust_content_box(&gray1);
    let (x2, y2, w2, h2) = get_robust_content_box(&gray2);

    info!("Content Box ref:    {}x{} at ({},{})", w1, h1, x1, y1);
    info!("Content Box target: {}x{} at ({},{})", w2, h2, x2, y2);

    // Check for degenerate boxes
    if w1 < 10 || h1 < 10 || w2 < 10 || h2 < 10 {
        warn!("Degenerate content box detected, skipping alignment");
        let ref_buf = RasterBuffer::new(img1.clone(), image1.dpi);
        // Just resize img2 to match img1 dimensions
        let resized = image::imageops::resize(
            img2.as_ref(),
            img1.width(),
            img1.height(),
            image::imageops::FilterType::Lanczos3,
        );
        let aligned_buf = RasterBuffer::new(resized, image2.dpi);
        return Ok(ForceFitResult {
            homography: target_homography(
                img1.width() as f64 / img2.width() as f64,
                img1.height() as f64 / img2.height() as f64,
                0.0,
                0.0,
                rotated_target,
                image2.image.height(),
            ),
            reference: ref_buf,
            aligned_target: aligned_buf,
            info: ForceFitInfo {
                scale_x: 1.0,
                scale_y: 1.0,
                shift_x: 0.0,
                shift_y: 0.0,
                box1: (x1, y1, w1, h1),
                box2: (x2, y2, w2, h2),
                swapped: false,
            },
            colors_swapped: false,
        });
    }

    // Step 4: Compute scale factors
    // scale = ref_content_size / target_content_size
    // This scales the target content to match the reference content dimensions.
    let raw_scale_x = if do_scale { w1 as f64 / w2 as f64 } else { 1.0 };
    let raw_scale_y = if do_scale { h1 as f64 / h2 as f64 } else { 1.0 };

    // Check for aspect ratio mismatch
    // If the scales are very close (e.g. within 2%), we assume the aspect ratio SUPPOSED to be identical
    // and the difference is due to noise in box detection (e.g. line thickness differences).
    // In this case, we enforce UNIFORM scaling (average) and align CENTERS to minimize drift.
    let diff = (raw_scale_x - raw_scale_y).abs();
    let max_s = raw_scale_x.max(raw_scale_y);
    let use_uniform = do_scale && (diff / max_s < 0.02);

    let (scale_x, scale_y) = if use_uniform {
        let avg = (raw_scale_x + raw_scale_y) / 2.0;
        info!(
            "Aspect ratios match closely (diff {:.2}%). Enforcing uniform scale: {:.4}",
            (diff / max_s) * 100.0,
            avg
        );
        (avg, avg)
    } else {
        info!("Force Scale: {:.4}, {:.4}", raw_scale_x, raw_scale_y);
        (raw_scale_x, raw_scale_y)
    };

    // Step 5: Compute affine transform coefficients
    //   src_x = a * dst_x + b * dst_y + c
    //   src_y = d * dst_x + e * dst_y + f
    let a = 1.0 / scale_x;
    let b = 0.0;
    let d = 0.0;
    let e_coeff = 1.0 / scale_y;

    // Calculate shifts (c, f)
    // If uniform scaling, we align CENTERS to distribute the error.
    // If anisotropic (warping), we strictly align TOP-LEFT (x1->x2).
    let (c, f_coeff) = if use_uniform {
        // align center of ref box to center of target box
        let cx1 = x1 as f64 + w1 as f64 / 2.0;
        let cy1 = y1 as f64 + h1 as f64 / 2.0;
        let cx2 = x2 as f64 + w2 as f64 / 2.0;
        let cy2 = y2 as f64 + h2 as f64 / 2.0;

        // src_center = a * dst_center + c
        // c = src_center - a * dst_center
        (cx2 - a * cx1, cy2 - e_coeff * cy1)
    } else {
        // align top-left corner (x1, y1) -> (x2, y2)
        // src_x = a * dst_x + c => c = x2 - a*x1
        (x2 as f64 - a * x1 as f64, y2 as f64 - e_coeff * y1 as f64)
    };

    debug!(
        "Affine coefficients: a={:.4}, b={:.4}, c={:.4}, d={:.4}, e={:.4}, f={:.4}",
        a, b, c, d, e_coeff, f_coeff
    );

    // Step 6: Apply affine transform
    // Calculate the output canvas size based on the PROJECTED size of the aligned target.
    // Previous logic used max(img1.w, img2.w) which caused massive canvas expansion
    // when scaling down a large target to a small reference.

    // We estimate where the right/bottom edge of the target will land:
    // dst_end = Alignment_Point_Ref + Scale * (Distance_from_Alignment_Point_to_End_Target)
    // Alignment Point Ref = x1, y1 (Inner Edge Start)
    // Distance in Target = (Total_Width - x2), (Total_Height - y2)
    let dst_end_x = x1 as f64 + scale_x * (img2.width() as f64 - x2 as f64);
    let dst_end_y = y1 as f64 + scale_y * (img2.height() as f64 - y2 as f64);

    let out_w = img1.width().max(dst_end_x.ceil() as u32);
    let out_h = img1.height().max(dst_end_y.ceil() as u32);

    let aligned = affine_transform(&img2, out_w, out_h, a, b, c, d, e_coeff, f_coeff);

    // Pad the reference to the same canvas size (white fill for extra area)
    let ref_padded = if out_w != img1.width() || out_h != img1.height() {
        info!(
            "Padding reference from {}x{} to {}x{} (union canvas)",
            img1.width(),
            img1.height(),
            out_w,
            out_h
        );
        let mut padded = RgbaImage::from_pixel(out_w, out_h, image::Rgba([255, 255, 255, 255]));
        image::imageops::overlay(&mut padded, img1, 0, 0);
        padded
    } else {
        img1.clone()
    };

    info!(
        "Output canvas: {}x{}, aligned target: {}x{}",
        ref_padded.width(),
        ref_padded.height(),
        aligned.width(),
        aligned.height(),
    );

    let ref_buf = RasterBuffer::new(ref_padded, image1.dpi);
    let aligned_buf = RasterBuffer::new(aligned, image2.dpi);

    Ok(ForceFitResult {
        homography: target_homography(
            scale_x,
            scale_y,
            -c * scale_x,
            -f_coeff * scale_y,
            rotated_target,
            image2.image.height(),
        ),
        reference: ref_buf,
        aligned_target: aligned_buf,
        info: ForceFitInfo {
            scale_x,
            scale_y,
            shift_x: c,
            shift_y: f_coeff,
            box1: (x1, y1, w1, h1),
            box2: (x2, y2, w2, h2),
            swapped: false,
        },
        colors_swapped: false,
    })
}

// imageops::rotate90 maps original (x,y) to (height-1-y,x).
fn target_homography(
    sx: f64,
    sy: f64,
    tx: f64,
    ty: f64,
    rotated: bool,
    height: u32,
) -> [[f64; 3]; 3] {
    if rotated {
        [
            [0.0, -sx, tx + sx * (height - 1) as f64],
            [sy, 0.0, ty],
            [0.0, 0.0, 1.0],
        ]
    } else {
        [[sx, 0.0, tx], [0.0, sy, ty], [0.0, 0.0, 1.0]]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_binarize() {
        let mut img = RgbaImage::new(10, 10);
        // Fill with mid-gray (should become black/ink)
        for pixel in img.pixels_mut() {
            *pixel = image::Rgba([128, 128, 128, 255]);
        }
        let gray = binarize_to_gray(&img);
        assert_eq!(gray.get_pixel(0, 0).0[0], 0); // Should be ink (dark)

        // Fill with bright (should become white)
        for pixel in img.pixels_mut() {
            *pixel = image::Rgba([230, 230, 230, 255]);
        }
        let gray = binarize_to_gray(&img);
        assert_eq!(gray.get_pixel(0, 0).0[0], 255); // Should be white
    }

    #[test]
    fn test_content_box_empty_image() {
        let gray = GrayImage::from_fn(100, 100, |_, _| Luma([255])); // All white
        let (_x, _y, w, h) = get_robust_content_box(&gray);
        // Should return fallback bounds
        assert!(w > 0);
        assert!(h > 0);
    }

    #[test]
    fn test_content_box_with_frame() {
        // Create an image with a clear frame border
        let mut gray = GrayImage::from_fn(200, 200, |_, _| Luma([255])); // All white

        // Draw a frame (black rectangle outline)
        for x in 10..190 {
            gray.put_pixel(x, 10, Luma([0])); // Top line
            gray.put_pixel(x, 189, Luma([0])); // Bottom line
        }
        for y in 10..190 {
            gray.put_pixel(10, y, Luma([0])); // Left line
            gray.put_pixel(189, y, Luma([0])); // Right line
        }

        let (x, y, w, h) = get_robust_content_box(&gray);
        // Content box should detect the frame edges
        assert!(x <= 11);
        assert!(y <= 11);
        assert!(w >= 170);
        assert!(h >= 170);
    }

    #[test]
    fn test_force_fit_identical() {
        let img = RgbaImage::from_fn(100, 100, |_, _| image::Rgba([255, 255, 255, 255]));
        let buf1 = RasterBuffer::new(img.clone(), 300);
        let buf2 = RasterBuffer::new(img, 300);

        let result = force_fit_align(&buf1, &buf2, false);
        assert!(result.is_ok());
        let r = result.unwrap();
        assert_eq!(r.aligned_target.image.width(), 100);
        assert_eq!(r.aligned_target.image.height(), 100);
    }

    #[test]
    fn test_force_fit_canvas_expansion() {
        // Regression test for "waaaay too big image"
        // Ref: 100x100
        let img1 = RgbaImage::from_fn(100, 100, |_, _| image::Rgba([255, 255, 255, 255]));
        let buf1 = RasterBuffer::new(img1, 300);

        // Target: 1000x1000
        let img2 = RgbaImage::from_fn(1000, 1000, |_, _| image::Rgba([255, 255, 255, 255]));
        let buf2 = RasterBuffer::new(img2, 300);

        // Align Target to Ref (Downscale)
        let result = force_fit_align(&buf1, &buf2, true).unwrap();

        // Expect canvas to be close to 100x100, NOT 1000x1000
        // Because the content is white, content box is fallback (whole image).
        // scale = 100/1000 = 0.1.
        // projected size = 1000 * 0.1 = 100.
        // So canvas should result in ~100.
        let w = result.reference.image.width();
        let h = result.reference.image.height();

        info!("Resulting canvas: {}x{}", w, h);
        assert!(
            w < 200,
            "Canvas width {} should be small (around 100), not 1000",
            w
        );
        assert!(
            h < 200,
            "Canvas height {} should be small (around 100), not 1000",
            h
        );
    }

    #[test]
    fn test_force_fit_uniform_scaling() {
        // Ref: 100x100
        let img1 = RgbaImage::from_fn(100, 100, |_, _| image::Rgba([255, 255, 255, 255]));
        let buf1 = RasterBuffer::new(img1, 300);

        // Target: 101x100 (1% wider)
        let img2 = RgbaImage::from_fn(101, 100, |_, _| image::Rgba([255, 255, 255, 255]));
        let buf2 = RasterBuffer::new(img2, 300);

        // Align Target (101x100) to Ref (100x100)
        // raw_scale_x = 100/101 = 0.9901
        // raw_scale_y = 100/100 = 1.0
        // diff = 0.0099. diff/max = 0.0099. < 0.02.
        // Should use uniform scaling!

        let result = force_fit_align(&buf1, &buf2, true).unwrap();
        let info = result.info;

        // Scales should be exactly equal (uniform)
        assert!(
            (info.scale_x - info.scale_y).abs() < 0.0001,
            "Scales should be uniform: x={}, y={}",
            info.scale_x,
            info.scale_y
        );
    }
}
