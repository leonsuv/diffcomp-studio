// =============================================================================
// dc_gpu/pipeline - wgpu Compute Pipeline: Accumulation Buffer Architecture
// =============================================================================
// Supports two modes:
//   1. Single pair:  compute_diff(ref, target, params) → RgbaImage
//   2. Batch:        compute_diff_batch(ref, &[targets], params) → RgbaImage
//
// The batch path uses an iterative accumulation buffer:
//   - One AccumulationTexture (Rgba8Unorm, read_write) initialized to (0,0,0,0)
//   - For each target layer, dispatch the compute shader with:
//       Binding 0: ReferenceTexture (read)
//       Binding 1: TargetTexture    (read)
//       Binding 2: AccumulationTexture (read_write)
//       Binding 3: Params uniform
//   - The shader max-blends new diffs into the accumulation texture
//   - After all dispatches, copy accumulation → staging → CPU readback
// =============================================================================

use bytemuck::{Pod, Zeroable};
use futures::channel::oneshot;
use futures::future::join_all;
use image::{GenericImageView, RgbaImage};
use thiserror::Error;
use tracing::{debug, info, instrument};
use wgpu::util::DeviceExt;

/// Errors from GPU operations.
#[derive(Error, Debug)]
pub enum GpuError {
    /// Failed to find a suitable GPU adapter
    #[error("No suitable GPU adapter found")]
    NoAdapter,

    /// Failed to request a device from the adapter
    #[error("Failed to request GPU device: {0}")]
    DeviceRequest(String),

    /// Shader compilation or pipeline creation failed
    #[error("Pipeline creation failed: {0}")]
    PipelineError(String),

    /// Texture upload or readback failed
    #[error("Texture operation failed: {0}")]
    TextureError(String),

    /// Image dimensions don't match
    #[error("Image dimensions mismatch: A is {a_w}x{a_h}, B is {b_w}x{b_h}")]
    DimensionMismatch {
        /// Width of image A
        a_w: u32,
        /// Height of image A
        a_h: u32,
        /// Width of image B
        b_w: u32,
        /// Height of image B
        b_h: u32,
    },

    /// Buffer mapping failed
    #[error("Buffer mapping failed: {0}")]
    BufferMapError(String),

    /// No layers to compare
    #[error("No target layers provided for batch comparison")]
    NoTargets,
}

/// Blend mode for the diff output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffBlendMode {
    /// Overlay: Blend images with opacity
    Overlay = 0,
    /// Color Difference: Red-Green style
    ColorDifference = 1,
    /// Heatmap: blue → red magnitude
    Heatmap = 2,
    /// Binary: Black/White mask
    Binary = 3,
    /// Subtract: Absolute difference
    Subtract = 4,
    /// XOR: Bitwise difference (simulated)
    Xor = 5,
}

impl Default for DiffBlendMode {
    fn default() -> Self {
        Self::ColorDifference
    }
}

/// Parameters for the GPU diff computation.
#[derive(Debug, Clone)]
pub struct GpuDiffParams {
    /// Minimum difference threshold (0.0 - 1.0). Differences below this
    /// are treated as identical (useful for anti-aliasing noise).
    pub threshold: f32,
    /// Opacity of the diff overlay (0.0 - 1.0).
    pub opacity: f32,
    /// Whether to highlight pixels present in A but missing in B (red).
    pub show_missing: bool,
    /// Whether to highlight pixels present in B but missing in A (green).
    pub show_added: bool,
    /// The diff visualization mode.
    pub blend_mode: DiffBlendMode,
    /// Reference layer color (RGB, 0.0-1.0).
    pub ref_color: [f32; 3],
    /// Target layer color (RGB, 0.0-1.0).
    pub target_color: [f32; 3],
    /// Opacity of the context (reference image) in the "No Diff" areas.
    /// 1.0 = Visible (dimmed), 0.0 = Transparent.
    pub context_opacity: f32,
}

impl Default for GpuDiffParams {
    fn default() -> Self {
        Self {
            threshold: 0.05,
            opacity: 0.85,
            show_missing: true,
            show_added: true,
            blend_mode: DiffBlendMode::ColorDifference,
            ref_color: [1.0, 0.0, 0.0],    // Red
            target_color: [0.0, 1.0, 0.0], // Green
            context_opacity: 0.0,
        }
    }
}

/// GPU-side uniform buffer layout (must match WGSL struct).
/// Total size: 48 bytes (3 x vec4<f32> = 3 x 16 bytes)
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct DiffParamsGpu {
    // vec4<f32> block 1
    threshold: f32,
    opacity: f32,
    show_missing: f32,
    show_added: f32,
    // vec4<f32> block 2
    blend_mode: u32,
    context_opacity: f32,
    _pad2: u32,
    _pad3: u32,
    // vec4<f32> block 3: ref_color.rgb + pad
    ref_color_r: f32,
    ref_color_g: f32,
    ref_color_b: f32,
    offset_x: f32,
    // vec4<f32> block 4: target_color.rgb + pad
    target_color_r: f32,
    target_color_g: f32,
    target_color_b: f32,
    offset_y: f32,
}

impl DiffParamsGpu {
    fn from_params(p: &GpuDiffParams) -> Self {
        Self {
            threshold: p.threshold,
            opacity: p.opacity,
            show_missing: if p.show_missing { 1.0 } else { 0.0 },
            show_added: if p.show_added { 1.0 } else { 0.0 },
            blend_mode: p.blend_mode as u32,
            context_opacity: p.context_opacity,
            _pad2: 0,
            _pad3: 0,
            ref_color_r: p.ref_color[0],
            ref_color_g: p.ref_color[1],
            ref_color_b: p.ref_color[2],
            offset_x: 0.0,
            target_color_r: p.target_color[0],
            target_color_g: p.target_color[1],
            target_color_b: p.target_color[2],
            offset_y: 0.0,
        }
    }

    /// Create with a specific target color and offset override (for per-layer adjustment).
    fn from_params_with_layer_info(
        p: &GpuDiffParams,
        target_color: [f32; 3],
        offset: [f32; 2],
    ) -> Self {
        let mut gpu = Self::from_params(p);
        gpu.target_color_r = target_color[0];
        gpu.target_color_g = target_color[1];
        gpu.target_color_b = target_color[2];
        gpu.offset_x = offset[0];
        gpu.offset_y = offset[1];
        gpu
    }
}

/// The GPU diff engine. Manages the wgpu device, pipeline, and textures.
pub struct GpuDiffEngine {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::ComputePipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    max_texture_size: u32,
}

impl GpuDiffEngine {
    /// Create a new GPU diff engine.
    ///
    /// This initializes the wgpu device and compiles the compute shader.
    /// Call this once at application startup.
    /// Create a new GPU diff engine (asynchronous).
    ///
    /// This initializes the wgpu device and compiles the compute shader.
    /// Call this once at application startup.
    #[instrument]
    pub async fn new_async() -> Result<Self, GpuError> {
        info!("Initializing GPU compute engine");

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: None,
                force_fallback_adapter: false,
            })
            .await
            .ok_or(GpuError::NoAdapter)?;

        info!(
            adapter_name = %adapter.get_info().name,
            backend = ?adapter.get_info().backend,
            "GPU adapter selected"
        );

        // Request high limits if available, but fallback to downlevel defaults
        let mut limits = wgpu::Limits::downlevel_defaults();
        limits = limits.using_resolution(adapter.limits());

        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("DiffComp GPU Device"),
                    // WebGPU-compatible: No special features required.
                    // We use ping-pong textures instead of ReadWrite storage.
                    required_features: wgpu::Features::default(),
                    required_limits: limits,
                    memory_hints: wgpu::MemoryHints::Performance,
                },
                None,
            )
            .await
            .map_err(|e| GpuError::DeviceRequest(e.to_string()))?;

        let max_texture_size = device.limits().max_texture_dimension_2d;

        // Load the compute shader
        let shader_source = include_str!("shaders/compute_diff.wgsl");
        let shader_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Diff Compute Shader"),
            source: wgpu::ShaderSource::Wgsl(shader_source.into()),
        });

        // Create bind group layout — output is ReadWrite for accumulation
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Diff Bind Group Layout"),
            entries: &[
                // Texture A: Reference (read)
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                // Texture B: Current target layer (read)
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                // Accumulation Input (Read-Only)
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                // Accumulation Output (Write-Only)
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::StorageTexture {
                        access: wgpu::StorageTextureAccess::WriteOnly,
                        format: wgpu::TextureFormat::Rgba8Unorm,
                        view_dimension: wgpu::TextureViewDimension::D2,
                    },
                    count: None,
                },
                // Uniform params
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        // Create pipeline layout
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Diff Pipeline Layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        // Create compute pipeline
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("Diff Compute Pipeline"),
            layout: Some(&pipeline_layout),
            module: &shader_module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });

        info!("GPU compute pipeline created successfully");

        Ok(Self {
            device,
            queue,
            pipeline,
            bind_group_layout,
            max_texture_size,
        })
    }

    /// Create a new GPU diff engine (blocking).
    /// PROHIBITED on WASM main thread.
    pub fn new_blocking() -> Result<Self, GpuError> {
        pollster::block_on(Self::new_async())
    }

    /// Upload an RGBA8 image to a GPU texture (read-only).
    fn create_texture(&self, image: &RgbaImage, label: &str) -> wgpu::Texture {
        let (width, height) = image.dimensions();
        let size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };

        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        self.queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            image.as_raw(),
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(4 * width),
                rows_per_image: Some(height),
            },
            size,
        );

        texture
    }

    /// Create the accumulation storage texture (read-write, zero-initialized).
    fn create_accumulation_texture(&self, width: u32, height: u32) -> wgpu::Texture {
        self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Accumulation Texture"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            // STORAGE_BINDING for write, TEXTURE_BINDING for read, COPY_SRC for readback, COPY_DST for clear
            usage: wgpu::TextureUsages::STORAGE_BINDING
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        })
    }

    /// Read back an accumulation texture to CPU as an RgbaImage.
    ///
    /// Handles `COPY_BYTES_PER_ROW_ALIGNMENT` (256) by padding the row stride
    /// during the GPU→buffer copy, then stripping the padding when building
    /// the final `RgbaImage`.
    /// Read back an accumulation texture to CPU as an RgbaImage (Async).
    ///
    /// Handles `COPY_BYTES_PER_ROW_ALIGNMENT` (256) by padding the row stride
    /// during the GPU→buffer copy, then stripping the padding when building
    /// the final `RgbaImage`.
    #[allow(dead_code)]
    async fn readback_texture(
        &self,
        texture: &wgpu::Texture,
        width: u32,
        height: u32,
    ) -> Result<RgbaImage, GpuError> {
        // wgpu requires bytes_per_row to be a multiple of COPY_BYTES_PER_ROW_ALIGNMENT (256).
        let unpadded_bytes_per_row = 4 * width;
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let padded_bytes_per_row = (unpadded_bytes_per_row + align - 1) / align * align;
        let output_buffer_size = (padded_bytes_per_row * height) as u64;

        let staging_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Diff Output Staging Buffer"),
            size: output_buffer_size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Readback Encoder"),
            });

        encoder.copy_texture_to_buffer(
            wgpu::ImageCopyTexture {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::ImageCopyBuffer {
                buffer: &staging_buffer,
                layout: wgpu::ImageDataLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_bytes_per_row),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );

        self.queue.submit(std::iter::once(encoder.finish()));

        // Map and read (Async)
        let buffer_slice = staging_buffer.slice(..);
        let (tx, rx) = oneshot::channel();
        buffer_slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });

        // Poll the device to ensure the callback is fired.
        // On native, we must poll manually. On web, the browser handles it (and polling panics).
        #[cfg(not(target_arch = "wasm32"))]
        self.device.poll(wgpu::Maintain::Wait);

        // Await the channel result
        rx.await
            .map_err(|_| GpuError::TextureError("Async readback channel closed".to_string()))?
            .map_err(|e| GpuError::BufferMapError(format!("{e:?}")))?;

        let data = buffer_slice.get_mapped_range();

        // Strip row padding if present
        let result = if padded_bytes_per_row == unpadded_bytes_per_row {
            // No padding — direct copy
            RgbaImage::from_raw(width, height, data.to_vec()).ok_or_else(|| {
                GpuError::TextureError("Failed to create image from GPU readback data".to_string())
            })?
        } else {
            // Strip padding bytes from each row
            let mut pixels = Vec::with_capacity((unpadded_bytes_per_row * height) as usize);
            for row in 0..height {
                let start = (row * padded_bytes_per_row) as usize;
                let end = start + unpadded_bytes_per_row as usize;
                pixels.extend_from_slice(&data[start..end]);
            }
            RgbaImage::from_raw(width, height, pixels).ok_or_else(|| {
                GpuError::TextureError(
                    "Failed to create image from padded GPU readback data".to_string(),
                )
            })?
        };

        drop(data);
        staging_buffer.unmap();

        Ok(result)
    }

    /// Compute the diff between two images on the GPU (single-pair convenience).
    ///
    /// Compute the difference between a reference image and a target image (Async).
    /// Returns the result as a new image.
    #[instrument(skip(self, reference, target, params))]
    pub async fn compute_diff(
        &self,
        reference: &RgbaImage,
        target: &RgbaImage,
        params: &GpuDiffParams,
    ) -> Result<RgbaImage, GpuError> {
        self.compute_diff_batch(
            reference,
            &[target],
            &[params.target_color],
            &[[0.0, 0.0]],
            params,
        )
        .await
    }

    /// Compute the accumulated diff of one reference against multiple targets.
    ///
    /// This uses a **Tiled Processing** strategy to support massive images exceeding
    /// GPU texture limits (usually 8192 or 16384 pixels).
    ///
    /// The image is broken into `TILE_SIZE x TILE_SIZE` chunks. Each chunk is
    /// processed on the GPU (accumulating diffs from all targets), read back,
    /// and stitched into the final CPU buffer.
    ///
    /// `target_colors` provides a per-target RGB color (0.0-1.0). If shorter
    /// than `targets`, the base `params.target_color` is used as fallback.
    ///
    /// All images must have the same dimensions as the reference.
    #[instrument(skip(self, reference, targets, target_colors, params), fields(num_targets = targets.len()))]
    pub async fn compute_diff_batch(
        &self,
        reference: &RgbaImage,
        targets: &[&RgbaImage],
        target_colors: &[[f32; 3]],
        target_offsets: &[[f32; 2]],
        params: &GpuDiffParams,
    ) -> Result<RgbaImage, GpuError> {
        if targets.is_empty() {
            return Err(GpuError::NoTargets);
        }

        // Configure params for transparent background accumulation
        let batch_params = params.clone();

        let (full_w, full_h) = reference.dimensions();

        if full_w == 0
            || full_h == 0
            || targets.iter().any(|t| t.width() == 0 || t.height() == 0)
            || target_offsets.iter().flatten().any(|v| !v.is_finite())
        {
            return Err(GpuError::TextureError(
                "Comparison requires non-empty images and finite offsets".into(),
            ));
        }

        // Output buffer for the full stitched image
        let mut final_image = RgbaImage::new(full_w, full_h);

        // Dynamic tile size: reduce overhead by using larger tiles (up to 4096px)
        let tile_size = u32::min(1024, self.max_texture_size);

        debug!(
            width = full_w,
            height = full_h,
            tile_size = tile_size,
            "Starting tiled GPU batch diff (Async - Parallel)"
        );

        // Generate all tile coordinates first
        let mut tile_coords = Vec::new();
        for y in (0..full_h).step_by(tile_size as usize) {
            for x in (0..full_w).step_by(tile_size as usize) {
                tile_coords.push((x, y));
            }
        }

        // Process in chunks (Batch Size = 6)
        // Each tile needs reference, target, two accumulation textures and staging.
        // Two 1024px tiles keep working GPU allocations near 40 MiB.
        const BATCH_SIZE: usize = 2;

        for batch in tile_coords.chunks(BATCH_SIZE) {
            let mut batch_futures = Vec::new();

            for &(x, y) in batch {
                // Determine current tile dimensions (handle edges)
                let tile_w = u32::min(tile_size, full_w - x);
                let tile_h = u32::min(tile_size, full_h - y);

                // --- 1. Prepare Reference Tile ---
                // Note: cloning tile data to upload. Ideally we'd upload full image once, but tiling saves VRAM.
                let ref_tile = reference.view(x, y, tile_w, tile_h).to_image();
                let tex_ref = self.create_texture(&ref_tile, "Reference Tile");
                let tex_ref_view = tex_ref.create_view(&wgpu::TextureViewDescriptor::default());

                // --- 2. Prepare Accumulation Textures (Ping-Pong) ---
                let tex_accum_a = self.create_accumulation_texture(tile_w, tile_h);
                let tex_accum_b = self.create_accumulation_texture(tile_w, tile_h);
                let tex_accum_a_view =
                    tex_accum_a.create_view(&wgpu::TextureViewDescriptor::default());
                let tex_accum_b_view =
                    tex_accum_b.create_view(&wgpu::TextureViewDescriptor::default());

                // wgpu initializes fresh textures to zero on first use.
                let mut encoder =
                    self.device
                        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                            label: Some("Tile Diff Encoder"),
                        });

                let mut last_output_was_b = false; // Initial state

                // --- 3. Process Each Target Layer for this Tile ---
                for (i, target) in targets.iter().enumerate() {
                    // Extract target tile with integer offset shift
                    let current_offset = target_offsets.get(i).copied().unwrap_or([0.0, 0.0]);
                    let (ox, oy) = (current_offset[0], current_offset[1]);

                    // Split into integer shift and fractional sub-pixel shift
                    let int_ox = ox.round() as i64;
                    let int_oy = oy.round() as i64;
                    let fract_ox = 0.0;
                    let fract_oy = 0.0;

                    // Calculate source region in target image
                    // We want target pixel at (x - int_ox, y - int_oy) to be at tile (0, 0)
                    let src_x = (x as i64).saturating_sub(int_ox);
                    let src_y = (y as i64).saturating_sub(int_oy);

                    // Create target tile: if there's an integer offset, pre-fill with
                    // reference pixels so that offset-gap areas (where the target has no
                    // data) match the reference and produce zero diff, instead of being
                    // transparent and triggering the "Missing" detection.
                    let mut target_tile = ref_tile.clone();

                    // Calculate overlap between requested source rect and valid image rect
                    let img_w = target.width() as i64;
                    let img_h = target.height() as i64;

                    let valid_src_x = src_x.max(0);
                    let valid_src_y = src_y.max(0);
                    let valid_end_x = src_x.saturating_add(tile_w as i64).min(img_w);
                    let valid_end_y = src_y.saturating_add(tile_h as i64).min(img_h);

                    if valid_src_x < valid_end_x && valid_src_y < valid_end_y {
                        let copy_w = (valid_end_x - valid_src_x) as u32;
                        let copy_h = (valid_end_y - valid_src_y) as u32;

                        let dest_x = (valid_src_x - src_x) as u32;
                        let dest_y = (valid_src_y - src_y) as u32;

                        // Copy valid region
                        let sub_img = target
                            .view(valid_src_x as u32, valid_src_y as u32, copy_w, copy_h)
                            .to_image();
                        image::imageops::replace(
                            &mut target_tile,
                            &sub_img,
                            dest_x.into(),
                            dest_y.into(),
                        );
                    }

                    let tex_target = self.create_texture(&target_tile, "Target Tile");
                    let tex_target_view =
                        tex_target.create_view(&wgpu::TextureViewDescriptor::default());

                    // Params buffer
                    let current_target_color =
                        target_colors.get(i).copied().unwrap_or([0.0, 1.0, 0.0]);

                    let gpu_params_adjusted = DiffParamsGpu::from_params_with_layer_info(
                        &batch_params,
                        current_target_color,
                        [fract_ox, fract_oy],
                    );
                    let params_buffer =
                        self.device
                            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                                label: Some("Diff Params Buffer"),
                                contents: bytemuck::bytes_of(&gpu_params_adjusted),
                                usage: wgpu::BufferUsages::UNIFORM,
                            });

                    // Ping-Pong Logic
                    let (in_view, out_view) = if i % 2 == 0 {
                        (&tex_accum_a_view, &tex_accum_b_view)
                    } else {
                        (&tex_accum_b_view, &tex_accum_a_view)
                    };
                    last_output_was_b = i % 2 == 0;

                    let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("Diff Bind Group"),
                        layout: &self.bind_group_layout,
                        entries: &[
                            wgpu::BindGroupEntry {
                                binding: 0,
                                resource: wgpu::BindingResource::TextureView(&tex_ref_view),
                            },
                            wgpu::BindGroupEntry {
                                binding: 1,
                                resource: wgpu::BindingResource::TextureView(&tex_target_view),
                            },
                            wgpu::BindGroupEntry {
                                binding: 2,
                                resource: wgpu::BindingResource::TextureView(in_view),
                            },
                            wgpu::BindGroupEntry {
                                binding: 3,
                                resource: wgpu::BindingResource::TextureView(out_view),
                            },
                            wgpu::BindGroupEntry {
                                binding: 4,
                                resource: params_buffer.as_entire_binding(),
                            },
                        ],
                    });

                    {
                        let mut cpass =
                            encoder.begin_compute_pass(&wgpu::ComputePassDescriptor::default());
                        cpass.set_pipeline(&self.pipeline);
                        cpass.set_bind_group(0, &bind_group, &[]);
                        // Dispatch 16x16 workgroups
                        let wg_x = (tile_w + 15) / 16;
                        let wg_y = (tile_h + 15) / 16;
                        cpass.dispatch_workgroups(wg_x, wg_y, 1);
                    }
                }
                self.queue.submit(std::iter::once(encoder.finish()));

                // --- 4. Readback Preparation (Enqueued) ---
                // We do NOT wait here. We prepare the readback and move the future.

                // Identify final texture
                // We must CLONE the texture reference (Arc-like handle) to move to async block?
                // Actually `readback_texture` logic: copy to staging buffer.
                // We perform the COPY here (submission), then map the BUFFER later.

                let final_tex = if last_output_was_b {
                    &tex_accum_b
                } else {
                    &tex_accum_a
                };

                // Staging buffer creation
                let unpadded_bytes_per_row = 4 * tile_w;
                let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
                let padded_bytes_per_row = (unpadded_bytes_per_row + align - 1) / align * align;
                let output_buffer_size = (padded_bytes_per_row * tile_h) as u64;

                let staging_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("Diff Output Staging Buffer"),
                    size: output_buffer_size,
                    usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });

                let mut encoder =
                    self.device
                        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                            label: Some("Readback Encoder"),
                        });

                encoder.copy_texture_to_buffer(
                    wgpu::ImageCopyTexture {
                        texture: final_tex,
                        mip_level: 0,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    wgpu::ImageCopyBuffer {
                        buffer: &staging_buffer,
                        layout: wgpu::ImageDataLayout {
                            offset: 0,
                            bytes_per_row: Some(padded_bytes_per_row),
                            rows_per_image: Some(tile_h),
                        },
                    },
                    wgpu::Extent3d {
                        width: tile_w,
                        height: tile_h,
                        depth_or_array_layers: 1,
                    },
                );
                self.queue.submit(std::iter::once(encoder.finish()));

                // Map Async (with oneshot)
                let (tx, rx) = oneshot::channel();
                let buffer_slice = staging_buffer.slice(..);
                buffer_slice.map_async(wgpu::MapMode::Read, move |res| {
                    let _ = tx.send(res);
                });

                // create future
                batch_futures.push(async move {
                    // Wait for mapping
                    rx.await
                        .map_err(|_| GpuError::TextureError("Async channel closed".into()))?
                        .map_err(|e| GpuError::BufferMapError(format!("{e:?}")))?;

                    // Read data
                    let buffer_slice = staging_buffer.slice(..);
                    let data = buffer_slice.get_mapped_range();

                    // Strip row padding
                    let tile_img = if padded_bytes_per_row == unpadded_bytes_per_row {
                        RgbaImage::from_raw(tile_w, tile_h, data.to_vec())
                    } else {
                        let mut pixels =
                            Vec::with_capacity((unpadded_bytes_per_row * tile_h) as usize);
                        for row in 0..tile_h {
                            let start = (row * padded_bytes_per_row) as usize;
                            let end = start + unpadded_bytes_per_row as usize;
                            pixels.extend_from_slice(&data[start..end]);
                        }
                        RgbaImage::from_raw(tile_w, tile_h, pixels)
                    };

                    drop(data);
                    staging_buffer.unmap();

                    let img = tile_img
                        .ok_or(GpuError::TextureError("Failed to create tile image".into()))?;
                    Ok::<_, GpuError>((x, y, img))
                });
            }

            // --- Await Batch ---
            // Poll once (Native) to drive all submissions in this batch
            #[cfg(not(target_arch = "wasm32"))]
            self.device.poll(wgpu::Maintain::Wait);

            let results = join_all(batch_futures).await;

            // --- Stitch Batch ---
            for res in results {
                let (x, y, tile_img) = res?;
                image::imageops::replace(&mut final_image, &tile_img, x as i64, y as i64);
            }
        }

        Ok(final_image)
    }

    /// Get information about the GPU adapter being used.
    pub fn adapter_info(&self) -> String {
        format!("wgpu device: {}", self.device.features().bits())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diff_params_default() {
        let params = GpuDiffParams::default();
        assert!(params.threshold > 0.0);
        assert!(params.opacity > 0.0);
        assert!(params.show_missing);
        assert!(params.show_added);
    }

    #[test]
    fn test_diff_params_gpu_conversion() {
        let params = GpuDiffParams {
            threshold: 0.1,
            opacity: 0.5,
            show_missing: true,
            show_added: false,
            blend_mode: DiffBlendMode::Binary,
            ref_color: [1.0, 0.0, 0.0],
            target_color: [0.0, 1.0, 0.0],
            context_opacity: 1.0,
        };

        let gpu = DiffParamsGpu::from_params(&params);
        assert_eq!(gpu.threshold, 0.1);
        assert_eq!(gpu.opacity, 0.5);
        assert_eq!(gpu.show_missing, 1.0);
        assert_eq!(gpu.show_added, 0.0);
        assert_eq!(gpu.blend_mode, 3);
        assert_eq!(gpu.context_opacity, 1.0);
    }

    // Note: GPU tests require a GPU adapter. These will be skipped in CI
    // unless a GPU is available.
    #[test]
    fn test_gpu_engine_creation() {
        // This test may fail on headless CI — that's expected
        match GpuDiffEngine::new_blocking() {
            Ok(engine) => {
                println!("GPU engine created: {}", engine.adapter_info());
            }
            Err(GpuError::NoAdapter) => {
                println!("No GPU adapter available (headless CI?), skipping");
            }
            Err(e) => {
                panic!("Unexpected GPU error: {}", e);
            }
        }
    }

    #[test]
    fn test_gpu_diff_identical_images() {
        let engine = match GpuDiffEngine::new_blocking() {
            Ok(e) => e,
            Err(GpuError::NoAdapter) => {
                println!("No GPU adapter, skipping");
                return;
            }
            Err(e) => panic!("GPU error: {}", e),
        };

        // Create two identical 64x64 red images
        let img = RgbaImage::from_fn(64, 64, |_, _| image::Rgba([255, 0, 0, 255]));

        let result = pollster::block_on(engine.compute_diff(&img, &img, &GpuDiffParams::default()))
            .expect("Diff should succeed");

        // Identical images should produce mostly transparent output
        let mut opaque_count = 0u32;
        for pixel in result.pixels() {
            if pixel.0[3] > 10 {
                opaque_count += 1;
            }
        }
        assert_eq!(opaque_count, 0, "Identical images should have no diff");
    }

    #[test]
    fn test_gpu_diff_different_images() {
        let engine = match GpuDiffEngine::new_blocking() {
            Ok(e) => e,
            Err(GpuError::NoAdapter) => {
                println!("No GPU adapter, skipping");
                return;
            }
            Err(e) => panic!("GPU error: {}", e),
        };

        // Create two fully different images
        let img_a = RgbaImage::from_fn(64, 64, |_, _| image::Rgba([255, 0, 0, 255]));
        let img_b = RgbaImage::from_fn(64, 64, |_, _| image::Rgba([0, 0, 255, 255]));

        let result =
            pollster::block_on(engine.compute_diff(&img_a, &img_b, &GpuDiffParams::default()))
                .expect("Diff should succeed");

        // Different images should produce visible diff
        let mut opaque_count = 0u32;
        for pixel in result.pixels() {
            if pixel.0[3] > 10 {
                opaque_count += 1;
            }
        }
        assert!(
            opaque_count > 0,
            "Different images should produce visible diff"
        );
    }

    #[test]
    fn test_dimension_mismatch_reference_canvas() {
        let engine = match GpuDiffEngine::new_blocking() {
            Ok(e) => e,
            Err(GpuError::NoAdapter) => {
                println!("No GPU adapter, skipping");
                return;
            }
            Err(e) => panic!("GPU error: {}", e),
        };

        let img_a = RgbaImage::new(64, 64);
        let img_b = RgbaImage::new(32, 32);

        // Use the reference canvas and preserve target coordinates.
        let result =
            pollster::block_on(engine.compute_diff(&img_a, &img_b, &GpuDiffParams::default()));
        assert!(
            result.is_ok(),
            "Different-sized images should compare on the reference canvas"
        );
        let img = result.unwrap();
        assert_eq!(
            img.dimensions(),
            (64, 64),
            "Output should match reference dimensions"
        );
    }

    #[test]
    fn test_batch_diff_multi_target() {
        let engine = match GpuDiffEngine::new_blocking() {
            Ok(e) => e,
            Err(GpuError::NoAdapter) => {
                println!("No GPU adapter, skipping");
                return;
            }
            Err(e) => panic!("GPU error: {}", e),
        };

        // Reference: red. Targets: blue and green.
        let reference = RgbaImage::from_fn(64, 64, |_, _| image::Rgba([255, 0, 0, 255]));
        let target_1 = RgbaImage::from_fn(64, 64, |_, _| image::Rgba([0, 0, 255, 255]));
        let target_2 = RgbaImage::from_fn(64, 64, |_, _| image::Rgba([0, 255, 0, 255]));

        let colors = [[0.0, 0.0, 1.0], [0.0, 1.0, 0.0]];
        let result = pollster::block_on(engine.compute_diff_batch(
            &reference,
            &[&target_1, &target_2],
            &colors,
            &[[0.0, 0.0], [0.0, 0.0]],
            &GpuDiffParams::default(),
        ))
        .expect("Batch diff should succeed");

        // All pixels should show diff (max of two different diffs)
        let mut opaque_count = 0u32;
        for pixel in result.pixels() {
            if pixel.0[3] > 10 {
                opaque_count += 1;
            }
        }
        assert!(
            opaque_count > 0,
            "Multi-target batch should produce visible accumulated diff"
        );
    }

    #[test]
    fn test_batch_no_targets() {
        let engine = match GpuDiffEngine::new_blocking() {
            Ok(e) => e,
            Err(GpuError::NoAdapter) => {
                println!("No GPU adapter, skipping");
                return;
            }
            Err(e) => panic!("GPU error: {}", e),
        };

        let reference = RgbaImage::new(64, 64);
        let result = pollster::block_on(engine.compute_diff_batch(
            &reference,
            &[],
            &[],
            &[],
            &GpuDiffParams::default(),
        ));
        assert!(matches!(result, Err(GpuError::NoTargets)));
    }

    #[test]
    fn test_gpu_diff_unaligned_width() {
        // Regression test: image widths that don't satisfy
        // COPY_BYTES_PER_ROW_ALIGNMENT (256) must still work.
        // 3299 * 4 = 13196, not a multiple of 256.
        let engine = match GpuDiffEngine::new_blocking() {
            Ok(e) => e,
            Err(GpuError::NoAdapter) => {
                println!("No GPU adapter, skipping");
                return;
            }
            Err(e) => panic!("GPU error: {}", e),
        };

        let img_a = RgbaImage::from_fn(3299, 100, |_, _| image::Rgba([255, 0, 0, 255]));
        let img_b = RgbaImage::from_fn(3299, 100, |_, _| image::Rgba([0, 0, 255, 255]));

        let result =
            pollster::block_on(engine.compute_diff(&img_a, &img_b, &GpuDiffParams::default()))
                .expect("Diff should succeed with unaligned width");

        assert_eq!(result.dimensions(), (3299, 100));
    }
}
