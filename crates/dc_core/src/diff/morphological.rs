// =============================================================================
// dc_core/diff/morphological - Morphological Tolerance Diff Engine
// =============================================================================
// Ported 1:1 from Python compareTIFF.py V11 "Morphological Tolerance" logic.
//
// ## Algorithm
//   1. Binarize both images (threshold 200 → black/white).
//   2. Dilate ink in each image by 3x3 kernel (morphological tolerance buffer).
//   3. Compute unique ink:
//      - Unique1 = Ink1 AND NOT Dilated(Ink2)  (only in ref, not even close to target)
//      - Unique2 = Ink2 AND NOT Dilated(Ink1)  (only in target, not even close to ref)
//   4. Compose the final visualization:
//      - Background: gray context (all ink from either image)
//      - Unique1: overlaid in color1 (usually red)
//      - Unique2: overlaid in color2 (usually blue)
//
// This approach provides 1-pixel tolerance for minor alignment differences,
// which is essential for scanned engineering drawings.
// =============================================================================

use crate::types::{CoreError, CoreResult, RasterBuffer};
use image::{GrayImage, RgbaImage};
#[cfg(not(target_arch = "wasm32"))]
use rayon::prelude::*;

/// Composed drawing comparison and counts of unique ink.
#[derive(Debug)]
pub struct MorphDiffResult {
    pub image: RasterBuffer,
    pub diff1_count: u64,
    pub diff2_count: u64,
    pub total_pixels: u64,
}

// Composite alpha over white before classifying ink. Transparent black is background.
pub(crate) fn binarize(image: &RgbaImage) -> GrayImage {
    let mut gray = GrayImage::new(image.width(), image.height());
    let classify = |(out, p): (&mut u8, &[u8])| {
        let lum = (299 * p[0] as u32 + 587 * p[1] as u32 + 114 * p[2] as u32) / 1000;
        let lum = (lum * p[3] as u32 + 255 * (255 - p[3] as u32)) / 255;
        *out = if lum > 200 { 255 } else { 0 };
    };
    #[cfg(not(target_arch = "wasm32"))]
    gray.as_mut()
        .par_iter_mut()
        .zip(image.as_raw().par_chunks_exact(4))
        .for_each(classify);
    #[cfg(target_arch = "wasm32")]
    gray.as_mut()
        .iter_mut()
        .zip(image.as_raw().chunks_exact(4))
        .for_each(classify);
    gray
}

// Separable 3x3 minimum filter: two contiguous passes, no cloned source buffer.
fn fast_dilate_black(img: &GrayImage) -> GrayImage {
    let (w, h) = img.dimensions();
    if w == 0 || h == 0 {
        return img.clone();
    }
    let w = w as usize;
    let mut horizontal = GrayImage::new(w as u32, h);
    for (src, dst) in img
        .as_raw()
        .chunks_exact(w)
        .zip(horizontal.as_mut().chunks_exact_mut(w))
    {
        for x in 0..w {
            dst[x] = src[x.saturating_sub(1)] & src[x] & src[(x + 1).min(w - 1)];
        }
    }
    let mut out = GrayImage::new(w as u32, h);
    let source = horizontal.as_raw();
    let filter = |(y, row): (usize, &mut [u8])| {
        let prev = y.saturating_sub(1) * w;
        let curr = y * w;
        let next = (y + 1).min(h as usize - 1) * w;
        for x in 0..w {
            row[x] = source[prev + x] & source[curr + x] & source[next + x];
        }
    };
    #[cfg(not(target_arch = "wasm32"))]
    out.as_mut()
        .par_chunks_exact_mut(w)
        .enumerate()
        .for_each(filter);
    #[cfg(target_arch = "wasm32")]
    out.as_mut()
        .chunks_exact_mut(w)
        .enumerate()
        .for_each(filter);
    out
}

/// Compare drawings on the reference canvas; uncovered areas are ignored, never stretched.
pub fn compute_morphological_diff(
    image1: &RgbaImage,
    image2: &RgbaImage,
    color1: [u8; 3],
    color2: [u8; 3],
) -> CoreResult<MorphDiffResult> {
    compute_morphological_diff_batch(image1, &[image2], &[color2], &[[0.0, 0.0]], color1)
}

/// Reuse reference masks across all targets and retain every target's own highlight color.
/// Offsets are relative to the reference origin, rounded to the nearest raster pixel.
pub fn compute_morphological_diff_batch(
    reference: &RgbaImage,
    targets: &[&RgbaImage],
    colors: &[[u8; 3]],
    offsets: &[[f32; 2]],
    ref_color: [u8; 3],
) -> CoreResult<MorphDiffResult> {
    let (w, h) = reference.dimensions();
    if w == 0
        || h == 0
        || targets.is_empty()
        || targets.iter().any(|t| t.width() == 0 || t.height() == 0)
    {
        return Err(CoreError::InternalError {
            message: "Comparison requires non-empty images and targets".into(),
        });
    }
    if offsets.iter().flatten().any(|v| !v.is_finite()) {
        return Err(CoreError::InternalError {
            message: "Layer offsets must be finite".into(),
        });
    }
    let bin_ref = binarize(reference);
    let dil_ref = fast_dilate_black(&bin_ref);
    let mut output = RgbaImage::from_fn(w, h, |x, y| {
        let gray = if bin_ref.get_pixel(x, y)[0] == 0 {
            170
        } else {
            255
        };
        image::Rgba([gray, gray, gray, 255])
    });
    // Track classification separately from RGB so gray/white highlight colors work too.
    let mut classes = vec![0u8; w as usize * h as usize];
    for (i, target) in targets.iter().enumerate() {
        let bin_target = binarize(target);
        let dil_target = fast_dilate_black(&bin_target);
        let offset = offsets.get(i).copied().unwrap_or([0.0; 2]);
        let ox = offset[0].round() as i64;
        let oy = offset[1].round() as i64;
        let color = colors.get(i).copied().unwrap_or([0, 100, 255]);
        let compose = |(y, (row, classes)): (usize, (&mut [u8], &mut [u8]))| {
            let ty = (y as i64).saturating_sub(oy);
            if ty < 0 || ty >= target.height() as i64 {
                return;
            }
            for (x, (pixel, class)) in row.chunks_exact_mut(4).zip(classes.iter_mut()).enumerate() {
                let tx = (x as i64).saturating_sub(ox);
                if tx < 0 || tx >= target.width() as i64 {
                    continue;
                }
                let r = y * w as usize + x;
                let t = ty as usize * target.width() as usize + tx as usize;
                let unique_ref = bin_ref.as_raw()[r] == 0 && dil_target.as_raw()[t] != 0;
                let unique_target = bin_target.as_raw()[t] == 0 && dil_ref.as_raw()[r] != 0;
                if unique_target {
                    pixel[..3].copy_from_slice(&color);
                    *class |= 2;
                } else if unique_ref {
                    if *class & 2 == 0 {
                        pixel[..3].copy_from_slice(&ref_color);
                    }
                    *class |= 1;
                } else if *class == 0 && bin_target.as_raw()[t] == 0 {
                    pixel[..3].fill(170);
                }
            }
        };
        #[cfg(not(target_arch = "wasm32"))]
        output
            .as_mut()
            .par_chunks_exact_mut(w as usize * 4)
            .zip(classes.par_chunks_exact_mut(w as usize))
            .enumerate()
            .for_each(compose);
        #[cfg(target_arch = "wasm32")]
        output
            .as_mut()
            .chunks_exact_mut(w as usize * 4)
            .zip(classes.chunks_exact_mut(w as usize))
            .enumerate()
            .for_each(compose);
    }
    Ok(MorphDiffResult {
        diff1_count: classes.iter().filter(|c| **c & 1 != 0).count() as u64,
        diff2_count: classes.iter().filter(|c| **c & 2 != 0).count() as u64,
        total_pixels: w as u64 * h as u64,
        image: RasterBuffer::new(output, 300),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Luma;

    #[test]
    fn test_binarize() {
        let mut img = RgbaImage::new(4, 4);
        // Dark pixels
        for y in 0..2 {
            for x in 0..4 {
                img.put_pixel(x, y, image::Rgba([50, 50, 50, 255]));
            }
        }
        // Light pixels
        for y in 2..4 {
            for x in 0..4 {
                img.put_pixel(x, y, image::Rgba([220, 220, 220, 255]));
            }
        }

        let gray = binarize(&img);
        assert_eq!(gray.get_pixel(0, 0).0[0], 0); // Dark → ink
        assert_eq!(gray.get_pixel(0, 3).0[0], 255); // Light → white
    }

    #[test]
    fn test_dilation() {
        // Create an image with a single ink pixel in the center
        let mut gray = GrayImage::from_fn(5, 5, |_, _| Luma([255]));
        gray.put_pixel(2, 2, Luma([0])); // Single ink pixel

        let dilated = fast_dilate_black(&gray);

        // The center and all 8 neighbors should be ink (0)
        for dy in -1..=1i32 {
            for dx in -1..=1i32 {
                let x = (2 + dx) as u32;
                let y = (2 + dy) as u32;
                assert_eq!(
                    dilated.get_pixel(x, y).0[0],
                    0,
                    "Pixel ({},{}) should be dilated to ink",
                    x,
                    y
                );
            }
        }

        // Corners should still be white
        assert_eq!(dilated.get_pixel(0, 0).0[0], 255);
        assert_eq!(dilated.get_pixel(4, 4).0[0], 255);
    }

    #[test]
    fn test_identical_images_no_diff() {
        let img = RgbaImage::from_fn(50, 50, |x, y| {
            if x > 10 && x < 40 && y > 10 && y < 40 {
                image::Rgba([0, 0, 0, 255]) // Black rectangle
            } else {
                image::Rgba([255, 255, 255, 255]) // White background
            }
        });

        let result = compute_morphological_diff(&img, &img, [255, 0, 0], [0, 100, 255]).unwrap();

        assert_eq!(result.diff1_count, 0);
        assert_eq!(result.diff2_count, 0);
    }

    #[test]
    fn test_slightly_shifted_small_diff() {
        // Create image with a single line
        let img1 = RgbaImage::from_fn(50, 50, |x, _y| {
            if x == 25 {
                image::Rgba([0, 0, 0, 255])
            } else {
                image::Rgba([255, 255, 255, 255])
            }
        });
        // Shifted by 1 pixel
        let img2 = RgbaImage::from_fn(50, 50, |x, _y| {
            if x == 26 {
                image::Rgba([0, 0, 0, 255])
            } else {
                image::Rgba([255, 255, 255, 255])
            }
        });

        let result = compute_morphological_diff(&img1, &img2, [255, 0, 0], [0, 100, 255]).unwrap();

        // With 3x3 dilation, a 1px shift should be within tolerance
        assert_eq!(
            result.diff1_count, 0,
            "1px shift should be within morphological tolerance"
        );
        assert_eq!(
            result.diff2_count, 0,
            "1px shift should be within morphological tolerance"
        );
    }
}

#[cfg(test)]
mod regression_tests {
    use super::*;
    use image::{Luma, Rgba};
    fn white(w: u32, h: u32) -> RgbaImage {
        RgbaImage::from_pixel(w, h, Rgba([255; 4]))
    }
    #[test]
    fn empty_images_return_error_without_panicking() {
        assert!(
            compute_morphological_diff(&white(0, 0), &white(0, 0), [255, 0, 0], [0, 0, 255])
                .is_err()
        );
        assert_eq!(
            fast_dilate_black(&GrayImage::new(0, 0)).dimensions(),
            (0, 0)
        );
    }
    #[test]
    fn separable_dilation_matches_neighborhood_at_every_border() {
        for w in 1..8 {
            for h in 1..8 {
                let mask = GrayImage::from_fn(w, h, |x, y| {
                    Luma([if (x * 13 + y * 7) % 5 == 0 { 0 } else { 255 }])
                });
                let dilated = fast_dilate_black(&mask);
                for y in 0..h {
                    for x in 0..w {
                        let mut expected = 255;
                        for sy in y.saturating_sub(1)..=(y + 1).min(h - 1) {
                            for sx in x.saturating_sub(1)..=(x + 1).min(w - 1) {
                                expected &= mask.get_pixel(sx, sy)[0];
                            }
                        }
                        assert_eq!(dilated.get_pixel(x, y)[0], expected);
                    }
                }
            }
        }
    }
    #[test]
    fn transparent_black_is_background() {
        let result = compute_morphological_diff(
            &white(4, 4),
            &RgbaImage::new(4, 4),
            [255, 0, 0],
            [0, 0, 255],
        )
        .unwrap();
        assert_eq!((result.diff1_count, result.diff2_count), (0, 0));
    }
    #[test]
    fn every_target_retains_its_color_and_reference_dimensions() {
        let reference = white(12, 6);
        let mut a = white(12, 6);
        a.put_pixel(2, 2, Rgba([0, 0, 0, 255]));
        let mut b = white(6, 6);
        b.put_pixel(4, 2, Rgba([0, 0, 0, 255]));
        let result = compute_morphological_diff_batch(
            &reference,
            &[&a, &b],
            &[[255, 0, 0], [0, 0, 255]],
            &[[0.0, 0.0], [3.0, 0.0]],
            [0, 255, 0],
        )
        .unwrap();
        assert_eq!(result.image.dimensions(), (12, 6));
        assert_eq!(result.image.image.get_pixel(2, 2).0, [255, 0, 0, 255]);
        assert_eq!(result.image.image.get_pixel(7, 2).0, [0, 0, 255, 255]);
        assert_eq!(result.diff2_count, 2);
    }
    #[test]
    fn offset_gaps_are_not_false_deletions() {
        let mut reference = white(5, 5);
        reference.put_pixel(0, 2, Rgba([0, 0, 0, 255]));
        let result = compute_morphological_diff_batch(
            &reference,
            &[&white(5, 5)],
            &[],
            &[[2.0, 0.0]],
            [255, 0, 0],
        )
        .unwrap();
        assert_eq!(result.diff1_count, 0);
    }
}
