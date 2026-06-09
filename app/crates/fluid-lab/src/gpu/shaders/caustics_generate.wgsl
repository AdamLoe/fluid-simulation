// v1.16 Caustics generation pass (A) — half-res R16Float output.
//
// Reads the smoothed front-depth (smooth_z) and water thickness, recomputes
// the water surface normal using the same finite-difference formula as
// composite.wgsl::water_normal so refraction and caustics agree, then derives
// a focusing term from the Laplacian curvature (concave/convergent regions
// brighten) and outputs a scalar caustic intensity to a half-res R16Float
// target. Motion shift along normal.xy slides the pattern with the surface.
//
// Caustic formula:
//   focus    = clamp(focus_strength * (1 - curv_norm), 0, 1)  (concave = high)
//   thickness_vis = 1 - exp(-thickness * thickness_scale)
//   caustic  = clamp(focus * sun_ndotl * thickness_vis * intensity, 0, max_intensity)
//            + motion_shift contribution
//
// Temporal blend (when enabled): out = mix(current, history, history_alpha).
//   history_alpha = 0 → all-current / no history (v1.18 polarity).
// History texture is read here; the caller decides to write to the ping or pong view.

struct Uniform {
    // Camera / projection
    params:   vec4<f32>,  // x=shading(unused), y=tan(fov_y/2), z=width, w=height
    // Caustic knobs
    caustics: vec4<f32>,  // x=enabled(0/1), y=intensity, z=focus_strength, w=thickness_scale
    caustics2: vec4<f32>, // x=max_intensity, y=motion_scale, z=temporal_enabled, w=history_alpha
    // Sun (from hero uniform)
    sun:      vec4<f32>,  // xyz=world sun dir, w=sun intensity
    // eye→world rotation matrix; upper-left 3x3 used to transform eye-space
    // normals to world space so the N·L dot against the world sun is in a
    // consistent frame (mirrors composite.wgsl's m3 construction).
    eye_to_world: mat4x4<f32>,
};

@group(0) @binding(0) var samp:        sampler;
@group(0) @binding(1) var smooth_z:    texture_2d<f32>;
@group(0) @binding(2) var thickness_t: texture_2d<f32>;
@group(0) @binding(3) var history_t:   texture_2d<f32>;
@group(0) @binding(4) var<uniform> u:  Uniform;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs(@builtin(vertex_index) vi: u32) -> VsOut {
    var positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -3.0),
        vec2<f32>( 3.0,  1.0),
        vec2<f32>(-1.0,  1.0),
    );
    let p = positions[vi];
    var out: VsOut;
    out.pos = vec4<f32>(p, 0.0, 1.0);
    out.uv  = p * vec2<f32>(0.5, -0.5) + vec2<f32>(0.5);
    return out;
}

fn load_z(p: vec2<i32>, dims: vec2<i32>) -> f32 {
    let q = clamp(p, vec2<i32>(0), dims - vec2<i32>(1));
    return textureLoad(smooth_z, q, 0).r;
}

fn water_normal_gen(pixel: vec2<i32>, dims: vec2<i32>) -> vec3<f32> {
    let c = load_z(pixel, dims);
    if c >= 60000.0 {
        return vec3<f32>(0.0, 0.0, 1.0);
    }

    let l_raw = load_z(pixel + vec2<i32>(-1, 0), dims);
    let r_raw = load_z(pixel + vec2<i32>( 1, 0), dims);
    let d_raw = load_z(pixel + vec2<i32>( 0,-1), dims);
    let up_raw = load_z(pixel + vec2<i32>( 0, 1), dims);
    let l  = select(l_raw,  c, l_raw  >= 60000.0);
    let r  = select(r_raw,  c, r_raw  >= 60000.0);
    let d  = select(d_raw,  c, d_raw  >= 60000.0);
    let up = select(up_raw, c, up_raw >= 60000.0);

    let width        = max(u.params.z, 1.0);
    let height       = max(u.params.w, 1.0);
    let tan_half_fovy = u.params.y;
    let aspect       = width / height;
    let world_per_px_y = max(1.0e-4, 2.0 * c * tan_half_fovy / height);
    let world_per_px_x = max(1.0e-4, 2.0 * c * tan_half_fovy * aspect / width);
    let dzdx = (r - l)  * 0.5 / world_per_px_x;
    let dzdy = (up - d) * 0.5 / world_per_px_y;
    return normalize(vec3<f32>(-dzdx, -dzdy, 1.0));
}

@fragment
fn fs(in: VsOut) -> @location(0) f32 {
    // Sample all gradient-sampled textures upfront (before any non-uniform
    // control flow) to satisfy WGSL's uniform control-flow rule for textureSample.
    let thickness     = max(0.0, textureSample(thickness_t, samp, in.uv).r);
    let hist          = textureSample(history_t,   samp, in.uv).r;

    if u.caustics.x < 0.5 {
        return 0.0;
    }

    // smooth_z is full-res; sample it from the half-res UV.
    let full_dims_u = textureDimensions(smooth_z);
    let full_dims = vec2<i32>(i32(full_dims_u.x), i32(full_dims_u.y));
    // Map half-res pixel to full-res pixel coordinates.
    let pixel = clamp(vec2<i32>(floor(in.pos.xy)) * 2, vec2<i32>(0), full_dims - vec2<i32>(1));

    let front_z  = load_z(pixel, full_dims);
    let has_water = front_z < 60000.0;
    if !has_water {
        return 0.0;
    }

    // water_normal_gen returns an eye-space normal (+z toward camera).
    // Transform it to world space using the camera's eye→world rotation (upper
    // 3x3 of eye_to_world, same as composite.wgsl's m3) so the subsequent N·L
    // dot against the world-space sun direction is in a consistent frame.
    let n_eye = water_normal_gen(pixel, full_dims);
    let m3 = mat3x3<f32>(
        u.eye_to_world[0].xyz,
        u.eye_to_world[1].xyz,
        u.eye_to_world[2].xyz,
    );
    let n = normalize(m3 * n_eye);

    // Laplacian curvature (same as composite.wgsl lines 111-121).
    let zc  = load_z(pixel, full_dims);
    let zl2 = load_z(pixel + vec2<i32>(-1, 0), full_dims);
    let zr2 = load_z(pixel + vec2<i32>( 1, 0), full_dims);
    let zd2 = load_z(pixel + vec2<i32>( 0,-1), full_dims);
    let zu2 = load_z(pixel + vec2<i32>( 0, 1), full_dims);
    var curv = 0.0;
    if zc < 60000.0 && zl2 < 60000.0 && zr2 < 60000.0 && zd2 < 60000.0 && zu2 < 60000.0 {
        curv = abs((zl2 + zr2 + zu2 + zd2) - 4.0 * zc);
    }
    let n_var = clamp(curv * 4.0, 0.0, 1.0);

    // Focusing: concave (curved) regions focus light → higher intensity.
    // focus_strength boosts from the curvature; 1 = flat surface baseline.
    let focus_strength = u.caustics.z;
    let focus = clamp(1.0 + focus_strength * n_var, 0.0, 8.0);

    // N·L: sun light must hit the surface from above to produce caustics.
    // Both n and sun_dir are now in world space.
    let sun_dir   = normalize(u.sun.xyz);
    let sun_ndotl = max(0.0, dot(n, sun_dir));

    // Thickness visibility.
    let thickness_scale = u.caustics.w;
    let thickness_vis = 1.0 - exp(-thickness * thickness_scale);

    // Base caustic value.
    let sun_intensity = u.sun.w;
    let intensity     = u.caustics.y;
    let max_intensity = u.caustics2.x;
    var caustic_val   = clamp(
        focus * sun_ndotl * sun_intensity * thickness_vis * intensity,
        0.0, max_intensity
    );

    // Motion: shift sampling point along normal.xy so the caustic pattern slides
    // coherently with the surface. Uses textureLoad (integer coords) which does
    // not require uniform control flow, avoiding the textureSample restriction.
    let motion_scale = u.caustics2.y;
    if motion_scale > 0.001 {
        let thickness_dims_u = textureDimensions(thickness_t);
        let tdims = vec2<i32>(i32(thickness_dims_u.x), i32(thickness_dims_u.y));
        let shift_uv = in.uv + n.xy * motion_scale * 0.02;
        let shift_px = clamp(
            vec2<i32>(floor(shift_uv * vec2<f32>(tdims))),
            vec2<i32>(0), tdims - vec2<i32>(1)
        );
        let thickness2 = max(0.0, textureLoad(thickness_t, shift_px, 0).r);
        let tv2 = 1.0 - exp(-thickness2 * thickness_scale);
        caustic_val = mix(caustic_val, clamp(
            focus * sun_ndotl * sun_intensity * tv2 * intensity,
            0.0, max_intensity
        ), 0.5 * motion_scale);
    }

    // Temporal blend (v1.18 polarity: out = mix(current, history, history_alpha)).
    //   history_alpha = 0 → all-current / no smoothing.
    //   history_alpha = 1 → all-history / frozen.
    // This matches the convention that v1.18 will re-home as the unified
    // history_alpha knob so the stored value stays forward-compatible.
    let temporal_enabled = u.caustics2.z;
    let history_alpha    = clamp(u.caustics2.w, 0.0, 1.0);
    var out_val = caustic_val;
    if temporal_enabled > 0.5 {
        out_val = mix(caustic_val, hist, history_alpha);
    }

    return out_val;
}
