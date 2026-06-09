// v1.18 Temporal stabilization — per-target history blend.
//
// One full-screen triangle pass per target. Reads `current_t` (the raw
// output from the depth/thickness/whitewater pass this frame) and
// `history_t` (the previous stable frame), then writes:
//
//   out = mix(current, history, history_alpha)
//
// v1.18 polarity: history_alpha = 0 → all-current (no smoothing),
//                 history_alpha = 1 → frozen (all history).
//
// Depth-reject: if |current_z - history_z| > depth_threshold the blend
// is suppressed (alpha → 0) to avoid ghosting at depth discontinuities.
// Used for smooth_z; pass depth_threshold = 0.0 to disable.
//
// Normal-reject: if current normal differs from history normal (both
// derived from smooth_z neighbours) beyond normal_threshold the blend
// is suppressed similarly.  Pass normal_threshold = 0.0 to disable.
//
// Camera reset: when reset_flag > 0.5 the blend is suppressed entirely
// (alpha forced to 0) so the first frame after a camera jump is clean.

struct Uniform {
    // x = history_alpha, y = depth_threshold, z = normal_threshold, w = reset_flag
    params: vec4<f32>,
    // Viewport size (width, height) for normal finite-differencing
    size: vec4<f32>, // x=width, y=height, z=unused, w=unused
}

@group(0) @binding(0) var samp:      sampler;
@group(0) @binding(1) var current_t: texture_2d<f32>; // non-filterable: textureLoad
@group(0) @binding(2) var history_t: texture_2d<f32>; // filterable: textureSample
@group(0) @binding(3) var<uniform>  u: Uniform;

// History smooth_z texture for depth-reject and normal-reject.
// For smooth_z target: bound to the history ping/pong (the previous frame's smooth_z).
// For other targets: bound to the same history smooth_z (binding 4 is always
// the history smooth_z regardless of target type, but only read when thresholds > 0).
// Declared non-filterable so textureLoad can be used inside conditional branches
// without violating the WGSL uniform-control-flow rule for textureSample.
@group(0) @binding(4) var history_z_t: texture_2d<f32>; // non-filterable: textureLoad

struct VsOut { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> }

@vertex fn vs(@builtin(vertex_index) vi: u32) -> VsOut {
    // Full-screen triangle (NDC corners).
    var xy = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -3.0),
        vec2<f32>( 3.0,  1.0),
        vec2<f32>(-1.0,  1.0),
    );
    let p = xy[vi];
    return VsOut(vec4<f32>(p, 0.0, 1.0), p * vec2<f32>(0.5, -0.5) + vec2<f32>(0.5));
}

@fragment fn fs(in: VsOut) -> @location(0) f32 {
    let history_alpha    = u.params.x;
    let depth_threshold  = u.params.y;
    let normal_threshold = u.params.z;
    let reset_flag       = u.params.w;

    // Immediate reset: history dropped, output raw current.
    if reset_flag > 0.5 {
        let coord = vec2<i32>(in.pos.xy);
        return textureLoad(current_t, coord, 0).r;
    }

    let coord = vec2<i32>(in.pos.xy);
    let cur = textureLoad(current_t, coord, 0).r;
    let hist = textureSample(history_t, samp, in.uv).r;

    var alpha = history_alpha;

    // Depth reject: compare current vs history smooth_z.
    // Use textureLoad (non-filterable) to stay within uniform-control-flow rules.
    if depth_threshold > 0.0 {
        let hist_z = textureLoad(history_z_t, coord, 0).r;
        // cur is itself smooth_z in the smooth_z pass.
        let delta = abs(cur - hist_z);
        if delta > depth_threshold {
            alpha = 0.0;
        }
    }

    // Normal reject: if the normal derived from smooth_z neighbours differs too
    // much between current and history, suppress blending.
    // textureLoad with integer coords avoids uniform-control-flow constraint.
    if normal_threshold > 0.0 && alpha > 0.0 {
        let dx = 1.0 / u.size.x;

        // Current frame normal (from smooth_z = cur and its neighbours).
        let r = textureLoad(current_t, coord + vec2<i32>(1,  0), 0).r;
        let l = textureLoad(current_t, coord + vec2<i32>(-1, 0), 0).r;
        let d = textureLoad(current_t, coord + vec2<i32>(0,  1), 0).r;
        let t2 = textureLoad(current_t, coord + vec2<i32>(0, -1), 0).r;
        let cur_n = normalize(vec3<f32>(l - r, d - t2, 2.0 * dx));

        // History normal (from history smooth_z, using integer-coord neighbors).
        let hr = textureLoad(history_z_t, coord + vec2<i32>(1,  0), 0).r;
        let hl = textureLoad(history_z_t, coord + vec2<i32>(-1, 0), 0).r;
        let hd = textureLoad(history_z_t, coord + vec2<i32>(0,  1), 0).r;
        let ht = textureLoad(history_z_t, coord + vec2<i32>(0, -1), 0).r;
        let hist_n = normalize(vec3<f32>(hl - hr, hd - ht, 2.0 * dx));

        let ndot = dot(cur_n, hist_n);
        // 1 - dot(N_cur, N_hist) measures angular difference (0 = same, 2 = opposite).
        if (1.0 - ndot) > normal_threshold {
            alpha = 0.0;
        }
    }

    return mix(cur, hist, alpha);
}
