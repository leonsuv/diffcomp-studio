//! Regressions for accumulation and tile boundaries.
use dc_gpu::{DiffBlendMode, GpuDiffEngine, GpuDiffParams, GpuError};
use image::{Rgba, RgbaImage};

fn engine() -> Option<GpuDiffEngine> {
    match GpuDiffEngine::new_blocking() {
        Ok(engine) => Some(engine),
        Err(GpuError::NoAdapter) => None,
        Err(error) => panic!("GPU initialization failed: {error}"),
    }
}

#[test]
fn later_unchanged_revision_preserves_difference_in_every_diff_mode() {
    let Some(engine) = engine() else { return };
    let reference = RgbaImage::from_pixel(8, 8, Rgba([255; 4]));
    let mut changed = reference.clone();
    changed.put_pixel(3, 4, Rgba([0, 0, 0, 255]));
    for mode in [
        DiffBlendMode::ColorDifference,
        DiffBlendMode::Heatmap,
        DiffBlendMode::Binary,
        DiffBlendMode::Subtract,
        DiffBlendMode::Xor,
    ] {
        let params = GpuDiffParams {
            blend_mode: mode,
            context_opacity: 1.0,
            ..Default::default()
        };
        let single =
            pollster::block_on(engine.compute_diff(&reference, &changed, &params)).unwrap();
        let batch = pollster::block_on(engine.compute_diff_batch(
            &reference,
            &[&changed, &reference],
            &[[0.0, 1.0, 0.0]; 2],
            &[[0.0, 0.0]; 2],
            &params,
        ))
        .unwrap();
        assert_eq!(
            batch.get_pixel(3, 4),
            single.get_pixel(3, 4),
            "mode {mode:?}"
        );
    }
}

#[test]
fn shifted_ink_crosses_tile_boundary_without_stretching() {
    let Some(engine) = engine() else { return };
    let reference = RgbaImage::from_pixel(2050, 4, Rgba([255; 4]));
    let mut revision = RgbaImage::from_pixel(2048, 4, Rgba([255; 4]));
    for x in [1022, 1023, 2046] {
        revision.put_pixel(x, 2, Rgba([0, 0, 0, 255]));
    }
    let params = GpuDiffParams {
        context_opacity: 1.0,
        ..Default::default()
    };
    let result = pollster::block_on(engine.compute_diff_batch(
        &reference,
        &[&revision],
        &[[0.0, 1.0, 0.0]],
        &[[1.0, 0.0]],
        &params,
    ))
    .unwrap();
    for x in [1023, 1024, 2047] {
        assert_eq!(
            result.get_pixel(x, 2).0,
            [0, 255, 0, 255],
            "shifted pixel {x}"
        );
    }
    assert_eq!(result.get_pixel(1022, 2).0, [255; 4]);
}
