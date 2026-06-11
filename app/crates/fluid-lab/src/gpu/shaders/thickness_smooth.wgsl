// Separable plain-Gaussian blur for the screen-space water THICKNESS target.
//
// The depth (nearest_z) target gets an edge-preserving bilateral filter
// (water_smooth.wgsl) so the reconstructed surface normal stays crisp. The
// thickness target, by contrast, drives Beer-Lambert opacity/body colour in the
// composite and was previously left RAW — so the per-particle splat noise showed
// up directly as a speckled, "sandy" water body and let the dark wall show
// through the gaps between splats near the glass. A plain Gaussian here makes
// opacity spatially coherent (solid body, no speckle) and fills inter-splat
// holes so water reads as a continuous sheet up to the wall. Feathering across
// the silhouette into the (thickness=0) background is harmless: the composite
// still gates visible water on the smoothed front depth, not on thickness.

struct Params {
    axis_radius: vec4<f32>, // xy = axis (1,0) or (0,1), z = radius (f32), w = sigma_spatial
};

@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var<uniform> params: Params;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
};

@vertex
fn vs(@builtin(vertex_index) vi: u32) -> VsOut {
    var pos = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -3.0),
        vec2<f32>(3.0, 1.0),
        vec2<f32>(-1.0, 1.0),
    );
    var out: VsOut;
    out.pos = vec4<f32>(pos[vi], 0.0, 1.0);
    return out;
}

fn load_t(p: vec2<i32>, dims: vec2<i32>) -> f32 {
    let q = clamp(p, vec2<i32>(0, 0), dims - vec2<i32>(1, 1));
    return textureLoad(src, q, 0).r;
}

@fragment
fn fs(in: VsOut) -> @location(0) f32 {
    let dims_u = textureDimensions(src);
    let dims = vec2<i32>(i32(dims_u.x), i32(dims_u.y));
    let p = vec2<i32>(floor(in.pos.xy));

    let axis = vec2<i32>(i32(params.axis_radius.x), i32(params.axis_radius.y));
    let radius = i32(params.axis_radius.z);
    let sigma = max(params.axis_radius.w, 0.5);

    var sum = 0.0;
    var weight_sum = 0.0;
    for (var i = -radius; i <= radius; i = i + 1) {
        let fi = f32(i);
        let w = exp(-(fi * fi) / (2.0 * sigma * sigma));
        sum += load_t(p + axis * i, dims) * w;
        weight_sum += w;
    }
    return clamp(sum / max(weight_sum, 1.0e-6), 0.0, 65504.0);
}
