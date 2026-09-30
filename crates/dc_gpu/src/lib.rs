// =============================================================================
// dc_gpu - GPU Compute Engine for Image Comparison
// =============================================================================
// Provides a wgpu-based compute pipeline for real-time image differencing.
//
// ## Usage
//
// ```ignore
// let gpu = GpuDiffEngine::new().await?;
// let result = gpu.compute_diff(&image_a, &image_b, &params)?;
// ```
//
// The engine handles:
// 1. wgpu device/queue initialization
// 2. Texture upload (RGBA8 images → GPU textures)
// 3. Compute shader dispatch
// 4. Result readback (GPU → CPU)
// =============================================================================

#![warn(clippy::all, missing_docs)]
#![allow(
    clippy::module_name_repetitions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::too_many_lines,
    clippy::must_use_candidate,
    clippy::missing_errors_doc
)]

//! # dc_gpu — GPU-Accelerated Image Differencing
//!
//! Uses wgpu compute shaders to perform per-pixel image comparison
//! at 60fps on any GPU backend (Metal, Vulkan, DX12, WebGPU).

mod pipeline;

pub use pipeline::{DiffBlendMode, GpuDiffEngine, GpuDiffParams, GpuError};
