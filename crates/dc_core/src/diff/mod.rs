// =============================================================================
// dc_core/diff - Pixel Difference Computation
// =============================================================================
// Computes visual differences between aligned images.
// Supports multiple visualization modes for engineering review.
// =============================================================================

mod batch;
mod engine;
pub use batch::compute_cpu_diff_batch;
/// Morphological tolerance diff engine (ported from Python compareTIFF.py).
pub mod morphological;

pub use engine::{BlendMode, DiffConfig, DiffEngine, DiffResult};
pub use morphological::{
    compute_morphological_diff, compute_morphological_diff_batch, MorphDiffResult,
};
