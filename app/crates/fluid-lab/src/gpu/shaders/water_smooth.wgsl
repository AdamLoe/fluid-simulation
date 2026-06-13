struct Params {
    axis_radius: vec4<f32>, // xy = axis, z = radius (f32), w = sigma_spatial
    feature: vec4<f32>,     // x = feature_preservation strength (0..1), yzw unused
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
    let sigma_spatial_base = max(params.axis_radius.w, 0.5);
    let sigma_range = max(0.035, center * 0.018);

    // --- Feature-preserving (curvature-flow) modulation ---
    // An isotropic bilateral rounds *everything* equally, so smooth sheets and
    // sharp crests cannot coexist. Here the spatial Gaussian is narrowed where the
    // surface has high curvature (crests, ridges, droplet tips) and left wide where
    // it is flat (glassy sheets). Curvature is measured at a COARSE stencil so a
    // genuine multi-pixel ridge registers but single-splat noise does not — this is
    // what keeps the filter from preserving the per-splat speckle it is meant to
    // remove. feature_strength = 0 reproduces the plain isotropic bilateral exactly.
    let feature_strength = clamp(params.feature.x, 0.0, 1.0);
    var sigma_spatial = sigma_spatial_base;
    if feature_strength > 0.001 {
        let cs = max(2, radius / 2);
        let zl = load_z(p + vec2<i32>(-cs, 0), dims);
        let zr = load_z(p + vec2<i32>( cs, 0), dims);
        let zd = load_z(p + vec2<i32>(0, -cs), dims);
        let zu = load_z(p + vec2<i32>(0,  cs), dims);
        // Clamp invalid (background/silhouette) taps to center so curvature stays
        // finite; the range Gaussian below still protects the silhouette itself.
        let zlc = select(zl, center, zl >= 60000.0);
        let zrc = select(zr, center, zr >= 60000.0);
        let zdc = select(zd, center, zd >= 60000.0);
        let zuc = select(zu, center, zu >= 60000.0);
        let lap = abs((zlc + zrc + zdc + zuc) - 4.0 * center) / max(center, 0.5);
        let curv = smoothstep(0.004, 0.03, lap);
        let feat = feature_strength * curv;
        // High curvature -> kernel collapses toward ~0.3x sigma (preserve); flat -> full.
        sigma_spatial = sigma_spatial_base * mix(1.0, 0.3, feat);
    }

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
