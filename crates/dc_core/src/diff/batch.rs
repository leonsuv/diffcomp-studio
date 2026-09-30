//! CPU comparison of all visible layers on the reference canvas.
use super::{BlendMode, DiffConfig, DiffEngine};
use crate::{CoreError, CoreResult};
use image::RgbaImage;

/// Compare each target with its own color and relative offset. The strongest
/// change at each pixel wins; unchanged context cannot erase earlier changes.
pub fn compute_cpu_diff_batch(
    reference: &RgbaImage,
    targets: &[&RgbaImage],
    colors: &[[u8; 3]],
    offsets: &[[f32; 2]],
    config: &DiffConfig,
) -> CoreResult<RgbaImage> {
    if reference.width() == 0
        || reference.height() == 0
        || targets.is_empty()
        || targets.iter().any(|t| t.width() == 0 || t.height() == 0)
        || offsets.iter().flatten().any(|v| !v.is_finite())
    {
        return Err(CoreError::InternalError {
            message: "Comparison requires valid images and offsets".into(),
        });
    }
    let mut output = RgbaImage::new(reference.width(), reference.height());
    let mut strongest = vec![0u8; reference.width() as usize * reference.height() as usize];
    for (i, target) in targets.iter().enumerate() {
        let offset = offsets.get(i).copied().unwrap_or([0.0; 2]);
        let (ox, oy) = (offset[0].round() as i64, offset[1].round() as i64);
        let mut pair_config = config.clone();
        if let Some(color) = colors.get(i) {
            pair_config.target_color = crate::LayerColor::new(color[0], color[1], color[2]);
        }
        let engine = DiffEngine::new(pair_config);
        let render_row = |(y, (row, scores)): (usize, (&mut [u8], &mut [u8]))| {
            for (x, (out, score)) in row.chunks_exact_mut(4).zip(scores.iter_mut()).enumerate() {
                let a = reference.get_pixel(x as u32, y as u32);
                let (tx, ty) = ((x as i64).saturating_sub(ox), (y as i64).saturating_sub(oy));
                let b = if tx >= 0
                    && ty >= 0
                    && tx < target.width() as i64
                    && ty < target.height() as i64
                {
                    target.get_pixel(tx as u32, ty as u32)
                } else {
                    a
                };
                let (rendered, magnitude) = engine.render_pixel(&a.0, &b.0);
                if i == 0 || magnitude > *score || config.blend_mode == BlendMode::Overlay {
                    out.copy_from_slice(&rendered);
                    *score = magnitude;
                }
            }
        };
        #[cfg(not(target_arch = "wasm32"))]
        if config.use_parallel && reference.width() as u64 * reference.height() as u64 >= 65536 {
            use rayon::prelude::*;
            output
                .as_mut()
                .par_chunks_exact_mut(reference.width() as usize * 4)
                .zip(strongest.par_chunks_exact_mut(reference.width() as usize))
                .enumerate()
                .for_each(render_row);
        } else {
            output
                .as_mut()
                .chunks_exact_mut(reference.width() as usize * 4)
                .zip(strongest.chunks_exact_mut(reference.width() as usize))
                .enumerate()
                .for_each(render_row);
        }
        #[cfg(target_arch = "wasm32")]
        output
            .as_mut()
            .chunks_exact_mut(reference.width() as usize * 4)
            .zip(strongest.chunks_exact_mut(reference.width() as usize))
            .enumerate()
            .for_each(render_row);
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;
    #[test]
    fn later_context_does_not_erase_earlier_changes() {
        let reference = RgbaImage::from_pixel(10, 10, Rgba([255; 4]));
        let mut a = reference.clone();
        a.put_pixel(2, 2, Rgba([0, 0, 0, 255]));
        let mut b = reference.clone();
        b.put_pixel(6, 2, Rgba([0, 0, 0, 255]));
        let result = compute_cpu_diff_batch(
            &reference,
            &[&a, &b],
            &[[255, 0, 0], [0, 0, 255]],
            &[],
            &DiffConfig::default(),
        )
        .unwrap();
        assert_eq!(result.get_pixel(2, 2).0, [255, 0, 0, 255]);
        assert_eq!(result.get_pixel(6, 2).0, [0, 0, 255, 255]);
    }
    #[test]
    fn offset_and_all_six_modes_are_supported() {
        let reference = RgbaImage::from_pixel(5, 5, Rgba([255; 4]));
        let target = RgbaImage::from_pixel(1, 1, Rgba([0, 0, 0, 255]));
        for mode in [
            BlendMode::Overlay,
            BlendMode::ColorDifference,
            BlendMode::Heatmap,
            BlendMode::BinaryMask,
            BlendMode::Subtract,
            BlendMode::Xor,
        ] {
            let config = DiffConfig {
                blend_mode: mode,
                ..Default::default()
            };
            let result = compute_cpu_diff_batch(
                &reference,
                &[&target],
                &[[255, 0, 0]],
                &[[2.0, 3.0]],
                &config,
            )
            .unwrap();
            assert_eq!(result.dimensions(), (5, 5));
            assert_ne!(result.get_pixel(2, 3), result.get_pixel(0, 0), "{mode:?}");
        }
    }
}
