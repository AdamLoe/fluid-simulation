struct Params {
    axis_radius: vec4<f32>, // xy = axis, z = radius (f32), w = sigma_spatial
};

@group(0) @binding(0) var src_z: texture_2d<f32>;
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

fn load_z(p: vec2<i32>, dims: vec2<i32>) -> f32 {
    let q = clamp(p, vec2<i32>(0, 0), dims - vec2<i32>(1, 1));
    return textureLoad(src_z, q, 0).r;
}

@fragment
fn fs(in: VsOut) -> @location(0) f32 {
    let dims_u = textureDimensions(src_z);
    let dims = vec2<i32>(i32(dims_u.x), i32(dims_u.y));
    let p = vec2<i32>(floor(in.pos.xy));
    let center = load_z(p, dims);
    if center >= 60000.0 {
        return 65504.0;
    }

    let axis = vec2<i32>(i32(params.axis_radius.x), i32(params.axis_radius.y));
    let radius = i32(params.axis_radius.z);
    // sigma_spatial is stored in w; derived from radius on the Rust side so the
    // Gaussian is never hard-truncated (sigma ≈ radius / 2).
    let sigma_spatial = max(params.axis_radius.w, 0.5);
    let sigma_range = max(0.035, center * 0.018);
    var sum = 0.0;
    var weight_sum = 0.0;

    for (var i = -radius; i <= radius; i = i + 1) {
        let z = load_z(p + axis * i, dims);
        if z < 60000.0 {
            let fi = f32(i);
            let dz = z - center;
            let spatial_w = exp(-(fi * fi) / (2.0 * sigma_spatial * sigma_spatial));
            let range_w = exp(-(dz * dz) / (2.0 * sigma_range * sigma_range));
            let w = spatial_w * range_w;
            sum += z * w;
            weight_sum += w;
        }
    }

    if weight_sum <= 0.0 {
        return center;
    }
    return clamp(sum / weight_sum, 0.0, 65504.0);
}
