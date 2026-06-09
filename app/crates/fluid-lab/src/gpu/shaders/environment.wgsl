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
//   [0 .. nx*ny)               back wall  (z = lo.z): i in [0,nx), j in [0,ny)
//   [nx*ny .. nx*ny + nz*ny)   left wall  (x = lo.x): k in [0,nz), j in [0,ny)
//   [nx*ny+nz*ny .. total)     floor       (y = lo.y): i in [0,nx), k in [0,nz)

// ─── Group 0: camera/material ────────────────────────────────────────────────

struct Env {
    view_proj: mat4x4<f32>,
    params: vec4<f32>, // x=floor_scale, y=floor_strength, z=backdrop_strength, w=wall_visibility
};

@group(0) @binding(0) var<uniform> env: Env;

// ─── Group 1: wetwall ─────────────────────────────────────────────────────────

struct WetWallUniform {
    dims:        vec4<u32>, // x=nx, y=ny, z=nz, w=total_texels
    face_counts: vec4<u32>, // x=back_count (nx*ny), y=0, z=left_count (nz*ny), w=0
    params:      vec4<f32>, // x=decay, y=dt, z=contact_gain, w=enabled
    tank_lo:     vec4<f32>, // xyz=tank lower corner, w=unused
    tank_hi:     vec4<f32>, // xyz=tank upper corner, w=unused
    render0:     vec4<f32>, // x=darkening_strength, y=gloss_strength, z=streak_strength, w=meniscus_enabled
    render1:     vec4<f32>, // x=meniscus_width, y=meniscus_strength, z=meniscus_fresnel_boost, w=contact_shadow_enabled
    render2:     vec4<f32>, // x=contact_shadow_strength, y=contact_shadow_radius, z=debug_view, w=pad
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

// Map a world position on the back wall (z≈lo.z) to its wetness-buffer index.
fn back_wall_wetness(p: vec3<f32>) -> f32 {
    let nx = ww.dims.x;
    let ny = ww.dims.y;
    let lo = ww.tank_lo.xyz;
    let hi = ww.tank_hi.xyz;
    let span_x = hi.x - lo.x;
    let span_y = hi.y - lo.y;
    // Map [lo.x, hi.x] -> [0, nx-1], [lo.y, hi.y] -> [0, ny-1]
    let fi = clamp((p.x - lo.x) / span_x, 0.0, 1.0) * f32(nx - 1u);
    let fj = clamp((p.y - lo.y) / span_y, 0.0, 1.0) * f32(ny - 1u);
    let i = u32(fi);
    let j = u32(fj);
    let back_count = ww.face_counts.x; // nx*ny
    let idx = j * nx + i;
    if idx >= back_count { return 0.0; }
    return wetness[idx];
}

// Wetness for the back wall at a given normalized y in [0,1].
// Returns wetness at the cell just above (j+1) and just below (j) for gradient.
fn back_wall_wetness_pair(p: vec3<f32>) -> vec2<f32> {
    let nx = ww.dims.x;
    let ny = ww.dims.y;
    let lo = ww.tank_lo.xyz;
    let hi = ww.tank_hi.xyz;
    let span_x = hi.x - lo.x;
    let span_y = hi.y - lo.y;
    let fi = clamp((p.x - lo.x) / span_x, 0.0, 1.0) * f32(nx - 1u);
    let fj_raw = clamp((p.y - lo.y) / span_y, 0.0, 1.0) * f32(ny - 1u);
    let i   = u32(fi);
    let j   = u32(fj_raw);
    let j1  = min(j + 1u, ny - 1u);
    let back_count = ww.face_counts.x;
    let w0_idx = j  * nx + i;
    let w1_idx = j1 * nx + i;
    let w0 = select(0.0, wetness[w0_idx], w0_idx < back_count);
    let w1 = select(0.0, wetness[w1_idx], w1_idx < back_count);
    return vec2<f32>(w0, w1);
}

// Map a world position on the left wall (x≈lo.x) to its wetness-buffer index.
fn left_wall_wetness(p: vec3<f32>) -> f32 {
    let nz = ww.dims.z;
    let ny = ww.dims.y;
    let lo = ww.tank_lo.xyz;
    let hi = ww.tank_hi.xyz;
    let span_z = hi.z - lo.z;
    let span_y = hi.y - lo.y;
    let fk = clamp((p.z - lo.z) / span_z, 0.0, 1.0) * f32(nz - 1u);
    let fj = clamp((p.y - lo.y) / span_y, 0.0, 1.0) * f32(ny - 1u);
    let k = u32(fk);
    let j = u32(fj);
    let back_count = ww.face_counts.x;
    let left_count = ww.face_counts.z; // nz*ny
    let local_idx  = j * nz + k;
    let idx = back_count + local_idx;
    if local_idx >= left_count { return 0.0; }
    return wetness[idx];
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
    let k   = u32(fk);
    let j   = u32(fj_raw);
    let j1  = min(j + 1u, ny - 1u);
    let back_count = ww.face_counts.x;
    let left_count = ww.face_counts.z;
    let w0_idx = j  * nz + k;
    let w1_idx = j1 * nz + k;
    let w0 = select(0.0, wetness[back_count + w0_idx], w0_idx < left_count);
    let w1 = select(0.0, wetness[back_count + w1_idx], w1_idx < left_count);
    return vec2<f32>(w0, w1);
}

// Map a world position on the floor (y≈lo.y) to its wetness.
fn floor_wetness(p: vec3<f32>) -> f32 {
    let nx = ww.dims.x;
    let nz = ww.dims.z;
    let lo = ww.tank_lo.xyz;
    let hi = ww.tank_hi.xyz;
    let span_x = hi.x - lo.x;
    let span_z = hi.z - lo.z;
    let fi = clamp((p.x - lo.x) / span_x, 0.0, 1.0) * f32(nx - 1u);
    let fk = clamp((p.z - lo.z) / span_z, 0.0, 1.0) * f32(nz - 1u);
    let i = u32(fi);
    let k = u32(fk);
    let back_count = ww.face_counts.x;
    let left_count = ww.face_counts.z;
    let floor_count = nx * nz;
    let local_idx = k * nx + i;
    if local_idx >= floor_count { return 0.0; }
    return wetness[back_count + left_count + local_idx];
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
        // ── WALL: matte + wetness darkening/gloss/streak + meniscus ─────────
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

            // Darkening: wet walls absorb more light.
            let darkening = 1.0 - wet * dark_str * streak_mod;
            color = color * darkening;

            // Gloss: simple Blinn-Phong-ish brightening (no real view vector
            // available here, use a fixed approximation from uv-based fresnel).
            if gloss_str > 0.001 {
                let gloss_uv = clamp(1.0 - in.uv.y, 0.0, 1.0);
                let gloss = pow(gloss_uv, 6.0) * wet * gloss_str * 0.4;
                color = color + vec3<f32>(gloss * 0.9, gloss, gloss * 1.05);
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
