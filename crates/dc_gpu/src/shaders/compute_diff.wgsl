// =============================================================================
// compute_diff.wgsl - GPU Compute Shader for Multi-Layer Image Comparison
// =============================================================================
// Runs per-pixel on the GPU, computing the difference between a reference
// image and a target layer. Designed for iterative accumulation: each dispatch
// reads the current accumulation value and max()s it with the new diff result.
//
// ## Multi-Layer Workflow
//   1. Clear accumulation texture to transparent black (0,0,0,0)
//   2. For each visible target layer:
//      a. Bind reference + target + accumulation texture
//      b. Set per-layer uniform (with that layer's blend color)
//      c. Dispatch this shader
//      d. Shader computes per-pixel diff, then max() with existing accumulation
//   3. Read back final accumulation texture
//
// Dispatch: ceil(width/16) x ceil(height/16) workgroups
// Each workgroup thread processes ONE pixel.
//
// ## Color Scheme
//   - No difference:  Transparent (alpha = 0)
//   - Missing (A∖B):  ref_color tinted
//   - Added   (B∖A):  target_color tinted
//   - Modified:       target_color intensity scaled by diff magnitude
//
// ## Diff Metrics
//   - Euclidean RGB distance in [0, 1] per channel
//   - Threshold below which diff is ignored (anti-aliasing noise)
// =============================================================================

// --- Uniforms ---
struct DiffParams {
    threshold: f32,
    opacity: f32,
    show_missing: f32,
    show_added: f32,
    blend_mode: u32,
    context_opacity: f32,
    _pad2: u32,
    _pad3: u32,
    ref_color_and_offset_x: vec4<f32>, // .rgb = color, .w = offset_x
    target_color_and_offset_y: vec4<f32>, // .rgb = color, .w = offset_y
}

@group(0) @binding(0) var texture_a: texture_2d<f32>;
@group(0) @binding(1) var texture_b: texture_2d<f32>;
@group(0) @binding(2) var accum_in: texture_2d<f32>;
@group(0) @binding(3) var output: texture_storage_2d<rgba8unorm, write>;
@group(0) @binding(4) var<uniform> params: DiffParams;

// Workgroup size: 16x16 = 256 threads per workgroup (GPU-friendly)
// Helper: heatmap ramp
fn heatmap_color(t: f32) -> vec3<f32> {
    if (t < 0.25) { return mix(vec3(0.0,0.0,1.0), vec3(0.0,1.0,1.0), t*4.0); }
    if (t < 0.50) { return mix(vec3(0.0,1.0,1.0), vec3(0.0,1.0,0.0), (t-0.25)*4.0); }
    if (t < 0.75) { return mix(vec3(0.0,1.0,0.0), vec3(1.0,1.0,0.0), (t-0.50)*4.0); }
    return mix(vec3(1.0,1.0,0.0), vec3(1.0,0.0,0.0), (t-0.75)*4.0);
}

fn blend_over(src: vec4<f32>, dst: vec4<f32>) -> vec4<f32> {
    let out_a = src.a + dst.a * (1.0 - src.a);
    if (out_a == 0.0) { return vec4(0.0); }
    let out_rgb = (src.rgb * src.a + dst.rgb * dst.a * (1.0 - src.a)) / out_a;
    return vec4(out_rgb, out_a);
}

@compute @workgroup_size(16, 16)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let dims = textureDimensions(texture_a);
    let x = gid.x;
    let y = gid.y;

    if (x >= dims.x || y >= dims.y) {
        return;
    }

    // Extract params
    let ref_color = params.ref_color_and_offset_x.rgb;
    let offset_x = params.ref_color_and_offset_x.w;
    let target_color = params.target_color_and_offset_y.rgb;
    let offset_y = params.target_color_and_offset_y.w;

    let coord = vec2<i32>(i32(x), i32(y));
    var pixel_a = textureLoad(texture_a, coord, 0);

    // Calculate target coordinate with offset
    // world_pixel(x,y) corresponds to layer_pixel(x - offset_x, y - offset_y)
    let coord_b = vec2<i32>(i32(f32(x) - offset_x), i32(f32(y) - offset_y));

    // Bounds check for B
    var pixel_b = vec4<f32>(0.0, 0.0, 0.0, 0.0);
    let dims_b = textureDimensions(texture_b);
    let b_in_bounds = (coord_b.x >= 0 && coord_b.y >= 0 && coord_b.x < i32(dims_b.x) && coord_b.y < i32(dims_b.y));
    if (b_in_bounds) {
        pixel_b = textureLoad(texture_b, coord_b, 0);
    }
    // Read from previous accumulation buffer (read-only input)
    let old_diff = textureLoad(accum_in, coord, 0);

    // If the target pixel is outside the layer bounds (due to offset),
    // this layer simply has no coverage here — skip it entirely.
    // Do NOT treat out-of-bounds offset areas as "missing" differences.
    if (!b_in_bounds) {
        textureStore(output, coord, old_diff);
        return;
    }

    var new_diff = vec4<f32>(0.0);
    pixel_a = vec4(pixel_a.rgb * pixel_a.a + vec3(1.0 - pixel_a.a), 1.0);
    pixel_b = vec4(pixel_b.rgb * pixel_b.a + vec3(1.0 - pixel_b.a), 1.0);

    // --- Mode Logic ---
    switch params.blend_mode {
        case 0u: { // Overlay
            // Simple mix
            new_diff = mix(pixel_a, pixel_b, params.opacity);
            // Ensure alpha is sufficient to see
            new_diff.a = max(pixel_a.a, pixel_b.a);
        }
        case 1u: { // Color Difference (Red-Green)
            // Use luminance to detect change
            let lum_a = dot(pixel_a.rgb, vec3(0.299, 0.587, 0.114));
            let lum_b = dot(pixel_b.rgb, vec3(0.299, 0.587, 0.114));
            let rgb_delta = abs(pixel_a.rgb - pixel_b.rgb);
            let diff = max(rgb_delta.r, max(rgb_delta.g, rgb_delta.b));

            if (diff <= params.threshold) {
                // No change -> Gray (or transparent if context_opacity is 0)
                let g = lum_a;
                if (old_diff.a == 0.0) { new_diff = vec4(g, g, g, params.context_opacity); }
            } else {
                let color = select(target_color, ref_color, lum_a < lum_b);
                new_diff = vec4(color * diff, 1.0);
            }
        }
        case 2u: { // Heatmap
            let delta = abs(pixel_a.rgb - pixel_b.rgb);
            let diff = max(delta.r, max(delta.g, delta.b));
            if (diff <= params.threshold) {
                // Show original desaturated
                let g = dot(pixel_a.rgb, vec3(0.299, 0.587, 0.114));
                if (old_diff.a == 0.0) { new_diff = vec4(g, g, g, params.context_opacity); }
            } else {
                let t = diff;
                new_diff = vec4(heatmap_color(t), 1.0);
            }
        }
        case 3u: { // Binary
            let delta = abs(pixel_a.rgb - pixel_b.rgb);
            let diff = max(delta.r, max(delta.g, delta.b));
             if (diff > params.threshold) {
                new_diff = vec4(1.0, 1.0, 1.0, 1.0);
            } else {
                if (old_diff.a == 0.0) { new_diff = vec4(0.0, 0.0, 0.0, params.context_opacity); }
            }
        }
        case 4u: { // Subtract
             new_diff = vec4(max(abs(pixel_a.rgb - pixel_b.rgb), old_diff.rgb), 1.0);
        }
        case 5u: { // XOR
             let ia = vec3<u32>(round(pixel_a.rgb * 255.0));
             let ib = vec3<u32>(round(pixel_b.rgb * 255.0));
             let ix = ia ^ ib;
             new_diff = vec4(max(vec3<f32>(ix)/255.0, old_diff.rgb), 1.0);
        }
        default: {
             new_diff = vec4(1.0, 0.0, 1.0, 1.0); // Error magenta
        }
    }

    let final_color = blend_over(new_diff, old_diff);
    textureStore(output, coord, final_color);
}
