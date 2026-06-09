// Hero-water refractable environment (v1.17: wet-wall read). Writes linear HDR
// color into scene_color (location 0) and positive eye distance (clip.w = -z_eye)
// into scene_depth (location 1), matching the eye-distance convention used by the
// water thickness pass. The composite samples both for the refracted background +
// depth guard.
//
// Group 0: camera + material params.
// Group 1: WetWallUniform + wetness buffer (v1.17).
//
// Wetness buffer layout (one f32 per texel, concatenated):
//   [0 .. nx_ss*ny_ss)                           back wall  (z = lo.z)
//   [nx_ss*ny_ss .. nx_ss*ny_ss + nz_ss*ny_ss)   left wall  (x = lo.x)
//   [nx_ss*ny_ss+nz_ss*ny_ss .. total)            floor       (y = lo.y)
// where nx_ss = nx * supersample (stored in ww.dims.x etc.).

// ─── Group 0: camera/material ────────────────────────────────────────────────

struct Env {
    view_proj: mat4x4<f32>,
    params:     vec4<f32>, // x=floor_scale, y=floor_strength, z=backdrop_strength, w=wall_visibility
    eye_world:  vec4<f32>, // xyz=camera eye in BOX-LOCAL space, w=unused
    env_ctrl:   vec4<f32>, // x=env_rotation, y=env_mode, z=env_brightness, w=unused
    sun:        vec4<f32>, // xyz=world sun direction, w=sun_intensity
    wet_refl:   vec4<f32>, // x=wet_reflectivity, y=wet_specular, zw=unused
    // Box-local→world rotation columns (mat3 padded to 3 vec4s).
    // Rotates a box-local direction into world space for env_sample.
    box_rot_col0: vec4<f32>,
    box_rot_col1: vec4<f32>,
    box_rot_col2: vec4<f32>,
};

@group(0) @binding(0) var<uniform> env: Env;

// ─── Group 1: wetwall ─────────────────────────────────────────────────────────

struct WetWallUniform {
    dims:        vec4<u32>, // x=nx_ss, y=ny_ss, z=nz_ss, w=total_texels (supersampled)
    face_counts: vec4<u32>, // x=back_count_ss, y=supersample, z=left_count_ss, w=nx_orig
    params:      vec4<f32>, // x=decay, y=dt, z=contact_gain, w=enabled
    tank_lo:     vec4<f32>, // xyz=tank lower corner, w=unused
    tank_hi:     vec4<f32>, // xyz=tank upper corner, w=unused
    render0:     vec4<f32>, // x=darkening_strength, y=gloss_strength, z=streak_strength, w=meniscus_enabled
    render1:     vec4<f32>, // x=meniscus_width, y=meniscus_strength, z=meniscus_fresnel_boost, w=contact_shadow_enabled
    render2:     vec4<f32>, // x=contact_shadow_strength, y=contact_shadow_radius, z=debug_view, w=blur_radius
};

@group(1) @binding(0) var<uniform>        ww:      WetWallUniform;
@group(1) @binding(1) var<storage, read>  wetness: array<f32>;

// ─── Vertex stage ─────────────────────────────────────────────────────────────

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) kind:       f32,
    @location(1) uv:         vec2<f32>,
    @location(2) eye:        f32,
    @location(3) world_pos:  vec3<f32>,  // v1.17: passed to FS for wetness index
};

@vertex
fn vs(
    @location(0) pos:  vec3<f32>,
    @location(1) kind: f32,
    @location(2) uv:   vec2<f32>,
) -> VsOut {
    var out: VsOut;
    let clip = env.view_proj * vec4<f32>(pos, 1.0);
    out.clip      = clip;
    out.kind      = kind;
    out.uv        = uv;
    out.eye       = clip.w; // -z_eye, positive eye distance
    out.world_pos = pos;
    return out;
}

// ─── Wetness helpers ──────────────────────────────────────────────────────────

// Cubic-Hermite ("smoothstep") blend weight — makes wetness transitions
// between grid cells smooth rather than linearly pixelated at the grid scale.
fn wet_smooth(t: f32) -> f32 {
    return t * t * (3.0 - 2.0 * t);
}

// Read a single back-wall texel safely (returns 0 if out-of-bounds).
fn back_texel(i: i32, j: i32, nx: u32, ny: u32, back_count: u32) -> f32 {
    if i < 0 || j < 0 || u32(i) >= nx || u32(j) >= ny { return 0.0; }
    let idx = u32(j) * nx + u32(i);
    return select(0.0, wetness[idx], idx < back_count);
}

// Read a single left-wall texel safely (returns 0 if out-of-bounds).
fn left_texel(k: i32, j: i32, nz: u32, ny: u32, back_count: u32, left_count: u32) -> f32 {
    if k < 0 || j < 0 || u32(k) >= nz || u32(j) >= ny { return 0.0; }
    let idx = u32(j) * nz + u32(k);
    return select(0.0, wetness[back_count + idx], idx < left_count);
}

// Read a single floor texel safely (returns 0 if out-of-bounds).
fn floor_texel(i: i32, k: i32, nx: u32, nz: u32, back_count: u32, left_count: u32, floor_count: u32) -> f32 {
    if i < 0 || k < 0 || u32(i) >= nx || u32(k) >= nz { return 0.0; }
    let idx = u32(k) * nx + u32(i);
    return select(0.0, wetness[back_count + left_count + idx], idx < floor_count);
}

// Map a world position on the back wall (z≈lo.z) to its wetness-buffer index.
// Uses bicubic-smooth interpolation + an optional box blur for smoother look.
fn back_wall_wetness(p: vec3<f32>) -> f32 {
    let nx = ww.dims.x;
    let ny = ww.dims.y;
    let lo = ww.tank_lo.xyz;
    let hi = ww.tank_hi.xyz;
    let span_x = hi.x - lo.x;
    let span_y = hi.y - lo.y;
    let fi = clamp((p.x - lo.x) / span_x, 0.0, 1.0) * f32(nx - 1u);
    let fj = clamp((p.y - lo.y) / span_y, 0.0, 1.0) * f32(ny - 1u);
    let back_count = ww.face_counts.x;
    let blur_r = i32(clamp(ww.render2.w, 0.0, 2.0));
    if blur_r <= 0 {
        let i0 = u32(fi);
        let i1 = min(i0 + 1u, nx - 1u);
        let j0 = u32(fj);
        let j1 = min(j0 + 1u, ny - 1u);
        let tx = wet_smooth(fract(fi));
        let ty = wet_smooth(fract(fj));
        let w00 = select(0.0, wetness[j0 * nx + i0], (j0 * nx + i0) < back_count);
        let w10 = select(0.0, wetness[j0 * nx + i1], (j0 * nx + i1) < back_count);
        let w01 = select(0.0, wetness[j1 * nx + i0], (j1 * nx + i0) < back_count);
        let w11 = select(0.0, wetness[j1 * nx + i1], (j1 * nx + i1) < back_count);
        return mix(mix(w00, w10, tx), mix(w01, w11, tx), ty);
    }
    // Box blur: average over [-blur_r, blur_r] in each axis around the nearest texel.
    // Round to nearest center (not truncate) so the kernel is symmetric about fi/fj.
    // Weight each tap by its bilinear closeness to fi/fj so sub-texel smoothness is
    // preserved within the blur: taps near the fractional position count more than
    // taps at the far edge of the kernel, preventing blocky step artifacts.
    let ci = i32(floor(fi + 0.5));
    let cj = i32(floor(fj + 0.5));
    let fi_frac = fract(fi);
    let fj_frac = fract(fj);
    var sum = 0.0;
    var cnt = 0.0;
    for (var dj = -blur_r; dj <= blur_r; dj++) {
        let wj = 1.0 - clamp(abs(f32(dj) - (fj_frac - 0.5)), 0.0, 1.0);
        for (var di = -blur_r; di <= blur_r; di++) {
            let wi = 1.0 - clamp(abs(f32(di) - (fi_frac - 0.5)), 0.0, 1.0);
            let w = wi * wj + 1.0e-4; // small floor so cnt stays positive
            sum += back_texel(ci + di, cj + dj, nx, ny, back_count) * w;
            cnt += w;
        }
    }
    return sum / cnt;
}

// Wetness for the back wall at a given world position.
// Returns bilinear-sampled wetness at rows j and j+1 for the meniscus gradient.
// Note: the blur (render.hero.wet_wall.blur) affects the main wetness readers only
// (darkening/gloss/reflection). The _pair helpers used for the meniscus edge-detection
// intentionally read raw bilinear texels — blur has no effect on the meniscus highlight.
fn back_wall_wetness_pair(p: vec3<f32>) -> vec2<f32> {
    let nx = ww.dims.x;
    let ny = ww.dims.y;
    let lo = ww.tank_lo.xyz;
    let hi = ww.tank_hi.xyz;
    let span_x = hi.x - lo.x;
    let span_y = hi.y - lo.y;
    let fi = clamp((p.x - lo.x) / span_x, 0.0, 1.0) * f32(nx - 1u);
    let fj_raw = clamp((p.y - lo.y) / span_y, 0.0, 1.0) * f32(ny - 1u);
    let i0  = u32(fi);
    let i1  = min(i0 + 1u, nx - 1u);
    let tx  = fract(fi);
    let j0  = u32(fj_raw);
    let j1  = min(j0 + 1u, ny - 1u);
    let back_count = ww.face_counts.x;
    // Row j0 (below): bilinear in x
    let w00 = select(0.0, wetness[j0 * nx + i0], (j0 * nx + i0) < back_count);
    let w01 = select(0.0, wetness[j0 * nx + i1], (j0 * nx + i1) < back_count);
    let w0 = mix(w00, w01, tx);
    // Row j1 (above): bilinear in x
    let w10 = select(0.0, wetness[j1 * nx + i0], (j1 * nx + i0) < back_count);
    let w11 = select(0.0, wetness[j1 * nx + i1], (j1 * nx + i1) < back_count);
    let w1 = mix(w10, w11, tx);
    return vec2<f32>(w0, w1);
}

// Map a world position on the left wall (x≈lo.x) to its wetness.
// Uses bicubic-smooth interpolation + optional box blur for smooth look.
fn left_wall_wetness(p: vec3<f32>) -> f32 {
    let nz = ww.dims.z;
    let ny = ww.dims.y;
    let lo = ww.tank_lo.xyz;
    let hi = ww.tank_hi.xyz;
    let span_z = hi.z - lo.z;
    let span_y = hi.y - lo.y;
    let fk = clamp((p.z - lo.z) / span_z, 0.0, 1.0) * f32(nz - 1u);
    let fj = clamp((p.y - lo.y) / span_y, 0.0, 1.0) * f32(ny - 1u);
    let back_count = ww.face_counts.x;
    let left_count = ww.face_counts.z; // nz*ny
    let blur_r = i32(clamp(ww.render2.w, 0.0, 2.0));
    if blur_r <= 0 {
        let k0 = u32(fk);
        let k1 = min(k0 + 1u, nz - 1u);
        let j0 = u32(fj);
        let j1 = min(j0 + 1u, ny - 1u);
        let tk = wet_smooth(fract(fk));
        let tj = wet_smooth(fract(fj));
        let idx00 = j0 * nz + k0;
        let idx10 = j0 * nz + k1;
        let idx01 = j1 * nz + k0;
        let idx11 = j1 * nz + k1;
        let w00 = select(0.0, wetness[back_count + idx00], idx00 < left_count);
        let w10 = select(0.0, wetness[back_count + idx10], idx10 < left_count);
        let w01 = select(0.0, wetness[back_count + idx01], idx01 < left_count);
        let w11 = select(0.0, wetness[back_count + idx11], idx11 < left_count);
        return mix(mix(w00, w10, tk), mix(w01, w11, tk), tj);
    }
    // Round to nearest center; bilinear-weight each tap (same scheme as back_wall_wetness).
    let ck = i32(floor(fk + 0.5));
    let cj = i32(floor(fj + 0.5));
    let fk_frac = fract(fk);
    let fj_frac = fract(fj);
    var sum = 0.0;
    var cnt = 0.0;
    for (var dj = -blur_r; dj <= blur_r; dj++) {
        let wj = 1.0 - clamp(abs(f32(dj) - (fj_frac - 0.5)), 0.0, 1.0);
        for (var dk = -blur_r; dk <= blur_r; dk++) {
            let wk = 1.0 - clamp(abs(f32(dk) - (fk_frac - 0.5)), 0.0, 1.0);
            let w = wk * wj + 1.0e-4;
            sum += left_texel(ck + dk, cj + dj, nz, ny, back_count, left_count) * w;
            cnt += w;
        }
    }
    return sum / cnt;
}

fn left_wall_wetness_pair(p: vec3<f32>) -> vec2<f32> {
    let nz = ww.dims.z;
    let ny = ww.dims.y;
    let lo = ww.tank_lo.xyz;
    let hi = ww.tank_hi.xyz;
    let span_z = hi.z - lo.z;
    let span_y = hi.y - lo.y;
    let fk = clamp((p.z - lo.z) / span_z, 0.0, 1.0) * f32(nz - 1u);
    let fj_raw = clamp((p.y - lo.y) / span_y, 0.0, 1.0) * f32(ny - 1u);
    let k0  = u32(fk);
    let k1  = min(k0 + 1u, nz - 1u);
    let tk  = fract(fk);
    let j0  = u32(fj_raw);
    let j1  = min(j0 + 1u, ny - 1u);
    let back_count = ww.face_counts.x;
    let left_count = ww.face_counts.z;
    // Row j0 (below): bilinear in k
    let w00 = select(0.0, wetness[back_count + j0 * nz + k0], (j0 * nz + k0) < left_count);
    let w01 = select(0.0, wetness[back_count + j0 * nz + k1], (j0 * nz + k1) < left_count);
    let w0 = mix(w00, w01, tk);
    // Row j1 (above): bilinear in k
    let w10 = select(0.0, wetness[back_count + j1 * nz + k0], (j1 * nz + k0) < left_count);
    let w11 = select(0.0, wetness[back_count + j1 * nz + k1], (j1 * nz + k1) < left_count);
    let w1 = mix(w10, w11, tk);
    return vec2<f32>(w0, w1);
}

// Map a world position on the floor (y≈lo.y) to its wetness.
// Uses bicubic-smooth interpolation + optional box blur for smooth look.
fn floor_wetness(p: vec3<f32>) -> f32 {
    let nx = ww.dims.x;
    let nz = ww.dims.z;
    let lo = ww.tank_lo.xyz;
    let hi = ww.tank_hi.xyz;
    let span_x = hi.x - lo.x;
    let span_z = hi.z - lo.z;
    let fi = clamp((p.x - lo.x) / span_x, 0.0, 1.0) * f32(nx - 1u);
    let fk = clamp((p.z - lo.z) / span_z, 0.0, 1.0) * f32(nz - 1u);
    let back_count = ww.face_counts.x;
    let left_count = ww.face_counts.z;
    let floor_count = nx * nz;
    let blur_r = i32(clamp(ww.render2.w, 0.0, 2.0));
    if blur_r <= 0 {
        let i0 = u32(fi);
        let i1 = min(i0 + 1u, nx - 1u);
        let k0 = u32(fk);
        let k1 = min(k0 + 1u, nz - 1u);
        let ti = wet_smooth(fract(fi));
        let tk = wet_smooth(fract(fk));
        let base = back_count + left_count;
        let idx00 = k0 * nx + i0;
        let idx10 = k0 * nx + i1;
        let idx01 = k1 * nx + i0;
        let idx11 = k1 * nx + i1;
        let w00 = select(0.0, wetness[base + idx00], idx00 < floor_count);
        let w10 = select(0.0, wetness[base + idx10], idx10 < floor_count);
        let w01 = select(0.0, wetness[base + idx01], idx01 < floor_count);
        let w11 = select(0.0, wetness[base + idx11], idx11 < floor_count);
        return mix(mix(w00, w10, ti), mix(w01, w11, ti), tk);
    }
    // Round to nearest center; bilinear-weight each tap (same scheme as back_wall_wetness).
    let ci = i32(floor(fi + 0.5));
    let ck = i32(floor(fk + 0.5));
    let fi_frac = fract(fi);
    let fk_frac = fract(fk);
    var sum = 0.0;
    var cnt = 0.0;
    for (var dk = -blur_r; dk <= blur_r; dk++) {
        let wk = 1.0 - clamp(abs(f32(dk) - (fk_frac - 0.5)), 0.0, 1.0);
        for (var di = -blur_r; di <= blur_r; di++) {
            let wi = 1.0 - clamp(abs(f32(di) - (fi_frac - 0.5)), 0.0, 1.0);
            let w = wi * wk + 1.0e-4;
            sum += floor_texel(ci + di, ck + dk, nx, nz, back_count, left_count, floor_count) * w;
            cnt += w;
        }
    }
    return sum / cnt;
}

// ─── Fragment stage ───────────────────────────────────────────────────────────

struct FsOut {
    @location(0) color: vec4<f32>,
    @location(1) eye:   f32,
};

@fragment
fn fs(in: VsOut) -> FsOut {
    let floor_scale    = env.params.x;
    let floor_strength = env.params.y;
    let backdrop_strength = env.params.z;
    let wall_visibility   = env.params.w;

    let ww_enabled   = ww.params.w > 0.5;
    let dark_str     = ww.render0.x;
    let gloss_str    = ww.render0.y;
    let streak_str   = ww.render0.z;
    let meniscus_en  = ww.render0.w > 0.5;
    let men_width    = ww.render1.x;
    let men_str      = ww.render1.y;
    let men_fresnel  = ww.render1.z;
    let shadow_en    = ww.render1.w > 0.5;
    let shadow_str   = ww.render2.x;
    let shadow_rad   = ww.render2.y;
    let debug_view   = u32(ww.render2.z + 0.5);

    let lo = ww.tank_lo.xyz;
    let hi = ww.tank_hi.xyz;
    let p  = in.world_pos;

    var color = vec3<f32>(0.04, 0.05, 0.08);

    if in.kind < 0.5 {
        // ── FLOOR: checker base + grid lines + contact shadow ───────────────
        let g = in.uv * floor_scale;
        let cell = floor(g);
        let parity = (cell.x + cell.y) - 2.0 * floor((cell.x + cell.y) * 0.5);
        let base = vec3<f32>(0.14, 0.16, 0.20);
        let alt  = vec3<f32>(0.32, 0.36, 0.42);
        color = mix(base, mix(base, alt, parity), floor_strength);
        let f = fract(g);
        let line = min(min(f.x, 1.0 - f.x), min(f.y, 1.0 - f.y));
        let grid = smoothstep(0.0, 0.04, line);
        color = mix(color, vec3<f32>(0.55, 0.62, 0.72), (1.0 - grid) * floor_strength * 0.6);

        // v1.17: floor wetness (for visual completeness; the floor contact shadow
        // blends naturally with wall contact)
        if ww_enabled {
            let fw = floor_wetness(p);
            color = color * (1.0 - fw * dark_str * 0.5);

            // Contact shadow: darken near the back/left wall edges.
            if shadow_en {
                // Distance from the back wall (z = lo.z)
                let dist_back = abs(p.z - lo.z);
                let dist_left = abs(p.x - lo.x);
                let d_min = min(dist_back, dist_left);
                let shadow = shadow_str * (1.0 - smoothstep(0.0, shadow_rad, d_min));
                color = color * (1.0 - shadow);
            }

            if debug_view == 1u {
                color = mix(color, vec3<f32>(0.1, 0.4, 0.9), fw);
            }
        }

    } else if in.kind < 1.5 {
        // ── WALL: matte + reflective wet shading + meniscus ──────────────────
        let wall_base = vec3<f32>(0.10, 0.12, 0.16);
        color = wall_base * (0.4 + wall_visibility);

        if ww_enabled {
            // Determine whether this is the back wall or left wall and read wetness.
            // The back wall has z≈lo.z; the left wall has x≈lo.x.
            let is_back = abs(p.z - lo.z) < abs(p.x - lo.x);

            var wet: f32;
            var wet_pair: vec2<f32>;  // (below, above)
            if is_back {
                wet      = back_wall_wetness(p);
                wet_pair = back_wall_wetness_pair(p);
            } else {
                wet      = left_wall_wetness(p);
                wet_pair = left_wall_wetness_pair(p);
            }

            // Streak: subtle vertical procedural modulation on wetness.
            var streak_mod = 1.0;
            if streak_str > 0.001 {
                let freq = 18.0;
                let streak_phase = select(p.x, p.z, is_back) * freq;
                let streak = 0.5 + 0.5 * sin(streak_phase);
                streak_mod = 1.0 + streak_str * (streak - 0.5) * wet;
            }

            // Darkening: wet walls absorb more light (keeps meniscus context).
            let darkening = 1.0 - wet * dark_str * streak_mod;
            color = color * darkening;

            // ── Wet reflectivity: mirror the environment/skybox on wet areas ──
            // This is the PRIMARY wet-wall visible signal on a near-black wall —
            // wet patches should look like wet glass reflecting the world.
            let wet_reflectivity = env.wet_refl.x;
            let wet_spec_str     = env.wet_refl.y;
            if wet > 0.001 && (wet_reflectivity > 0.001 || wet_spec_str > 0.001) {
                // Per-face constant outward normal in BOX-LOCAL space.
                let face_normal_local = select(vec3<f32>(1.0, 0.0, 0.0),
                                               vec3<f32>(0.0, 0.0, 1.0), is_back);
                // View direction: eye and surface point are BOTH in box-local space,
                // so this subtraction is geometrically correct regardless of box
                // rotation/translation (env.eye_world is pre-transformed to box-local
                // on the Rust side: box_orient.inverse() * (cam_eye - box_pos)).
                let view_dir = normalize(p - env.eye_world.xyz);
                // Reflection vector in box-local space.
                let refl_dir_local = reflect(view_dir, face_normal_local);
                // Rotate box-local reflection into world space for env_sample.
                // The box_rot columns form a mat3 that maps box-local→world.
                let box_rot = mat3x3<f32>(
                    env.box_rot_col0.xyz,
                    env.box_rot_col1.xyz,
                    env.box_rot_col2.xyz,
                );
                let refl_dir = box_rot * refl_dir_local;
                // Environment sample along the world-space reflection direction.
                let reflected_col = env_sample(refl_dir, env.env_ctrl, env.sun);
                // Fresnel term: raised floor (0.15) so wet areas read as shiny
                // even when viewed head-on (not just at grazing angles).
                let ndotv = clamp(-dot(face_normal_local, view_dir), 0.0, 1.0);
                let fresnel = 0.15 + 0.85 * pow(1.0 - ndotv, 5.0);
                let refl_amt = clamp(wet * wet_reflectivity * fresnel, 0.0, 1.0);
                color = mix(color, reflected_col, refl_amt);

                // Sun specular sheen: sharp highlight from the sun direction.
                if wet_spec_str > 0.001 {
                    let sun_dir = normalize(env.sun.xyz);
                    let sun_dot = max(dot(refl_dir, sun_dir), 0.0);
                    let shine = pow(sun_dot, 48.0) * wet * wet_spec_str * env.sun.w;
                    color = color + vec3<f32>(1.0, 0.95, 0.88) * shine;
                }
            }

            // Meniscus: highlight band at the wet/dry waterline.
            if meniscus_en && men_str > 0.001 {
                // Wetness gradient between lower and upper texel rows.
                let w0 = wet_pair.x; // wetness at/below this fragment
                let w1 = wet_pair.y; // wetness one row above
                // dW/dy approximation: positive where w0 > w1 (waterline just
                // above us), i.e. we are in the transitional band.
                let dwdy = w0 - w1;
                let band = smoothstep(0.0, 0.3, dwdy) * smoothstep(0.0, men_width, w0);
                let men_highlight = band * men_str;
                let men_fresnel_term = band * men_fresnel * 0.5;
                color = color + vec3<f32>(
                    men_highlight * 0.85 + men_fresnel_term,
                    men_highlight       + men_fresnel_term,
                    men_highlight * 1.1 + men_fresnel_term,
                );
            }

            // Contact shadow: darken near the floor edge (y ≈ lo.y).
            if shadow_en {
                let dist_floor = abs(p.y - lo.y);
                let shadow = shadow_str * (1.0 - smoothstep(0.0, shadow_rad, dist_floor));
                color = color * (1.0 - shadow);
            }

            // Debug views.
            if debug_view == 1u {
                // Wetness field (white = fully wet).
                color = mix(color, vec3<f32>(1.0, 1.0, 1.0), wet);
            } else if debug_view == 2u {
                // Meniscus mask.
                let w0 = wet_pair.x;
                let w1 = wet_pair.y;
                let dwdy = w0 - w1;
                let band = smoothstep(0.0, 0.3, dwdy) * smoothstep(0.0, men_width, w0);
                color = mix(color, vec3<f32>(1.0, 0.2, 0.2), band);
            }
        }

    } else {
        // ── BACKDROP: vertical gradient ──────────────────────────────────────
        let top = vec3<f32>(0.08, 0.11, 0.18);
        let bot = vec3<f32>(0.02, 0.03, 0.05);
        color = mix(bot, top, in.uv.y) * (0.3 + backdrop_strength);
    }

    var out: FsOut;
    out.color = vec4<f32>(color, 1.0);
    out.eye   = in.eye;
    return out;
}
