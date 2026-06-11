// Wall-fill: dense per-wall-cell flat water sheet against glass.
//
// TWO entry points in this file:
//   - `cs_occupancy`:  compute pass — writes dense per-wall-cell occupancy into
//                      the occ_buf storage buffer (see layout below).
//   - `vs` / `fs_fill`: render pass  — full-screen triangle that injects flat glass-
//                      plane surface into the thickness/nearest_z/whitewater MRT.
//
// ============================================================
// COMPUTE: occupancy
// ============================================================
//
// Buffer layout (one f32 occupancy value per supersampled wall texel):
//   Face 0 (back  z=lo, vertical wall): nx_ss*ny_ss, index j*nx_ss + i, inward cells k=1..3
//   Face 1 (left  x=lo, vertical wall): nz_ss*ny_ss, index j*nz_ss + k, inward cells i=1..3
//   Face 2 (right x=hi, vertical wall): nz_ss*ny_ss, index j*nz_ss + k, inward cells i=nx-2..nx-4
//   Face 3 (front z=hi, vertical wall): nx_ss*ny_ss, index j*nx_ss + i, inward cells k=nz-2..nz-4
//   Face 4 (floor y=lo, floor plane)  : nx_ss*nz_ss, index k*nx_ss + i, inward cells j=1..3
//
// One 1D or 2D workgroup dispatch covers all wall texels across all 5 faces.

struct OccUniform {
    // x=nx_ss, y=ny_ss, z=nz_ss, w=total occupancy entries
    dims:     vec4<u32>,
    // x=nx, y=ny, z=nz, w=ss
    orig:     vec4<u32>,
    // x=back=nx*ny, y=left=nz*ny, z=right=nz*ny, w=front=nx*ny
    nc:       vec4<u32>,
    // x=nc_floor (nx*nz), y=dispatch row stride in invocations, zw=unused
    nc_floor: vec4<u32>,
    // x=fill_enabled(0/1), y=fill_strength, z=fill_slab, w=waterline_softness
    fill:     vec4<f32>,
    // tank world-space bounds
    tank_lo:  vec4<f32>,
    tank_hi:  vec4<f32>,
    // x=particle_count, yzw=unused
    psplat:   vec4<u32>,
    // x=splat band (cells from wall), y=splat radius (texels), z=threshold lo, w=threshold hi
    sparams:  vec4<f32>,
};

@group(0) @binding(0) var<uniform>        occ_u:     OccUniform;
@group(0) @binding(1) var<storage, read>  cell_type: array<u32>;
@group(0) @binding(2) var<storage, read_write> occ_buf: array<f32>; // 1 f32 per wall texel/cell

// Instead of reading coarse per-cell cell_type (which stair-steps the waterline
// at the 64-grid), we splat the actual CONTINUOUS particle positions of the
// near-wall particles into the supersampled wall atlas. The waterline then
// follows real particle positions at atlas resolution (precise), not grid cells.
struct WFParticle { pos: vec4<f32>, vel: vec4<f32> };
@group(0) @binding(5) var<storage, read>       wf_particles: array<WFParticle>;
@group(0) @binding(6) var<storage, read_write> splat_buf:    array<atomic<i32>>;
const SPLAT_SCALE: f32 = 256.0;

// Splat a Gaussian of coverage `wt` centred at (fx,fy) (atlas-texel coords) into
// the face starting at `base`, with face dimensions width x hgt.
fn wf_splat_face(base: u32, width: u32, hgt: u32, fx: f32, fy: f32, wt: f32) {
    let R = max(occ_u.sparams.y, 0.5);
    let sig = max(R * 0.5, 0.5);
    let inv2s = 1.0 / (2.0 * sig * sig);
    let x0 = max(0, i32(floor(fx - R)));
    let x1 = min(i32(width) - 1, i32(ceil(fx + R)));
    let y0 = max(0, i32(floor(fy - R)));
    let y1 = min(i32(hgt) - 1, i32(ceil(fy + R)));
    for (var yy = y0; yy <= y1; yy = yy + 1) {
        for (var xx = x0; xx <= x1; xx = xx + 1) {
            let dx = f32(xx) + 0.5 - fx;
            let dy = f32(yy) + 0.5 - fy;
            let g = exp(-(dx * dx + dy * dy) * inv2s);
            let v = i32(g * wt * SPLAT_SCALE);
            if v > 0 {
                atomicAdd(&splat_buf[base + u32(yy) * width + u32(xx)], v);
            }
        }
    }
}

@compute @workgroup_size(64, 1, 1)
fn cs_splat(@builtin(workgroup_id) wg: vec3<u32>,
            @builtin(num_workgroups) nwg: vec3<u32>,
            @builtin(local_invocation_index) li: u32) {
    let p = (wg.y * nwg.x + wg.x) * 64u + li;
    if p >= occ_u.psplat.x { return; }
    let pos = wf_particles[p].pos.xyz;
    let lo = occ_u.tank_lo.xyz;
    let hi = occ_u.tank_hi.xyz;
    let nxf = f32(occ_u.orig.x);
    let nyf = f32(occ_u.orig.y);
    let nzf = f32(occ_u.orig.z);
    let nx_ss = occ_u.dims.x;
    let ny_ss = occ_u.dims.y;
    let nz_ss = occ_u.dims.z;
    let band = max(occ_u.sparams.x, 0.5);
    let u = clamp((pos - lo) / max(hi - lo, vec3<f32>(1.0e-6)), vec3<f32>(0.0), vec3<f32>(1.0));
    let fx = u.x * f32(nx_ss);
    let fy = u.y * f32(ny_ss);
    let fz = u.z * f32(nz_ss);
    // cell-space distance from each wall
    let cdx0 = u.x * nxf;          // from x=lo (left)
    let cdx1 = (1.0 - u.x) * nxf;  // from x=hi (right)
    let cdy0 = u.y * nyf;          // from y=lo (floor)
    let cdz0 = u.z * nzf;          // from z=lo (back)
    let cdz1 = (1.0 - u.z) * nzf;  // from z=hi (front)
    let base_left  = occ_u.nc.x;
    let base_right = base_left + occ_u.nc.y;
    let base_front = base_right + occ_u.nc.z;
    let base_floor = base_front + occ_u.nc.w;
    if cdz0 < band { wf_splat_face(0u,         nx_ss, ny_ss, fx, fy, 1.0 - smoothstep(0.5, band, cdz0)); }
    if cdx0 < band { wf_splat_face(base_left,  nz_ss, ny_ss, fz, fy, 1.0 - smoothstep(0.5, band, cdx0)); }
    if cdx1 < band { wf_splat_face(base_right, nz_ss, ny_ss, fz, fy, 1.0 - smoothstep(0.5, band, cdx1)); }
    if cdz1 < band { wf_splat_face(base_front, nx_ss, ny_ss, fx, fy, 1.0 - smoothstep(0.5, band, cdz1)); }
    if cdy0 < band { wf_splat_face(base_floor, nx_ss, nz_ss, fx, fz, 1.0 - smoothstep(0.5, band, cdy0)); }
}

// Convert the accumulated i32 splat field to f32 [0,1] coverage with a soft
// threshold (the waterline is where coverage crosses ~0.5).
@compute @workgroup_size(64, 1, 1)
fn cs_normalize(@builtin(global_invocation_id) gid: vec3<u32>) {
    let row_stride = select(occ_u.nc_floor.y, occ_u.dims.w, occ_u.nc_floor.y == 0u);
    let tid = gid.x + gid.y * row_stride;
    if tid >= occ_u.dims.w { return; }
    let acc = f32(atomicLoad(&splat_buf[tid])) / SPLAT_SCALE;
    occ_buf[tid] = clamp(smoothstep(occ_u.sparams.z, occ_u.sparams.w, acc), 0.0, 1.0);
}

const WG: u32 = 64u;
const LIQUID: u32 = 1u;

// cell_type linear index: i + j*nx + k*nx*ny
fn ct_idx(i: u32, j: u32, k: u32, nx: u32, ny: u32) -> u32 {
    return i + j * nx + k * nx * ny;
}

fn liquid_at(i: u32, j: u32, k: u32, nx: u32, ny: u32) -> f32 {
    return select(0.0, 1.0, cell_type[ct_idx(i, j, k, nx, ny)] == LIQUID);
}

fn sample_liquid_x_y(xf: f32, yf: f32, k: u32, nx: u32, ny: u32) -> f32 {
    let x = clamp(xf, 0.0, f32(nx - 1u));
    let y = clamp(yf, 0.0, f32(ny - 1u));
    let x0 = u32(floor(x));
    let y0 = u32(floor(y));
    let x1 = min(x0 + 1u, nx - 1u);
    let y1 = min(y0 + 1u, ny - 1u);
    let tx = x - f32(x0);
    let ty = y - f32(y0);
    let a = liquid_at(x0, y0, k, nx, ny);
    let b = liquid_at(x1, y0, k, nx, ny);
    let c = liquid_at(x0, y1, k, nx, ny);
    let d = liquid_at(x1, y1, k, nx, ny);
    return mix(mix(a, b, tx), mix(c, d, tx), ty);
}

fn sample_liquid_z_y(zf: f32, yf: f32, i: u32, nx: u32, ny: u32, nz: u32) -> f32 {
    let z = clamp(zf, 0.0, f32(nz - 1u));
    let y = clamp(yf, 0.0, f32(ny - 1u));
    let z0 = u32(floor(z));
    let y0 = u32(floor(y));
    let z1 = min(z0 + 1u, nz - 1u);
    let y1 = min(y0 + 1u, ny - 1u);
    let tz = z - f32(z0);
    let ty = y - f32(y0);
    let a = liquid_at(i, y0, z0, nx, ny);
    let b = liquid_at(i, y0, z1, nx, ny);
    let c = liquid_at(i, y1, z0, nx, ny);
    let d = liquid_at(i, y1, z1, nx, ny);
    return mix(mix(a, b, tz), mix(c, d, tz), ty);
}

fn sample_liquid_x_z(xf: f32, zf: f32, j: u32, nx: u32, ny: u32, nz: u32) -> f32 {
    let x = clamp(xf, 0.0, f32(nx - 1u));
    let z = clamp(zf, 0.0, f32(nz - 1u));
    let x0 = u32(floor(x));
    let z0 = u32(floor(z));
    let x1 = min(x0 + 1u, nx - 1u);
    let z1 = min(z0 + 1u, nz - 1u);
    let tx = x - f32(x0);
    let tz = z - f32(z0);
    let a = liquid_at(x0, j, z0, nx, ny);
    let b = liquid_at(x1, j, z0, nx, ny);
    let c = liquid_at(x0, j, z1, nx, ny);
    let d = liquid_at(x1, j, z1, nx, ny);
    return mix(mix(a, b, tx), mix(c, d, tx), tz);
}

fn contact_layer_count(n: u32) -> u32 {
    if n <= 2u {
        return 0u;
    }
    return min(3u, n - 2u);
}

fn contact_layer_weight(layer: u32) -> f32 {
    if layer == 1u {
        return 1.0;
    }
    if layer == 2u {
        return 0.85;
    }
    return 0.65;
}

fn sample_liquid_x_y_band_low_z(xf: f32, yf: f32, nx: u32, ny: u32, nz: u32) -> f32 {
    var best = 0.0;
    let layers = contact_layer_count(nz);
    for (var layer = 1u; layer <= layers; layer = layer + 1u) {
        best = max(best, sample_liquid_x_y(xf, yf, layer, nx, ny) * contact_layer_weight(layer));
    }
    return best;
}

fn sample_liquid_x_y_band_high_z(xf: f32, yf: f32, nx: u32, ny: u32, nz: u32) -> f32 {
    var best = 0.0;
    let layers = contact_layer_count(nz);
    for (var layer = 1u; layer <= layers; layer = layer + 1u) {
        let k = (nz - 1u) - layer;
        best = max(best, sample_liquid_x_y(xf, yf, k, nx, ny) * contact_layer_weight(layer));
    }
    return best;
}

fn sample_liquid_z_y_band_low_x(zf: f32, yf: f32, nx: u32, ny: u32, nz: u32) -> f32 {
    var best = 0.0;
    let layers = contact_layer_count(nx);
    for (var layer = 1u; layer <= layers; layer = layer + 1u) {
        best = max(best, sample_liquid_z_y(zf, yf, layer, nx, ny, nz) * contact_layer_weight(layer));
    }
    return best;
}

fn sample_liquid_z_y_band_high_x(zf: f32, yf: f32, nx: u32, ny: u32, nz: u32) -> f32 {
    var best = 0.0;
    let layers = contact_layer_count(nx);
    for (var layer = 1u; layer <= layers; layer = layer + 1u) {
        let i = (nx - 1u) - layer;
        best = max(best, sample_liquid_z_y(zf, yf, i, nx, ny, nz) * contact_layer_weight(layer));
    }
    return best;
}

fn sample_liquid_x_z_band_floor(xf: f32, zf: f32, nx: u32, ny: u32, nz: u32) -> f32 {
    var best = 0.0;
    let layers = contact_layer_count(ny);
    for (var layer = 1u; layer <= layers; layer = layer + 1u) {
        best = max(best, sample_liquid_x_z(xf, zf, layer, nx, ny, nz) * contact_layer_weight(layer));
    }
    return best;
}

@compute @workgroup_size(WG)
fn cs_occupancy(@builtin(global_invocation_id) gid: vec3<u32>) {
    let row_stride = select(occ_u.nc_floor.y, occ_u.dims.w, occ_u.nc_floor.y == 0u);
    let tid = gid.x + gid.y * row_stride;
    let total = occ_u.dims.w;
    if tid >= total { return; }

    let nx_ss = occ_u.dims.x;
    let ny_ss = occ_u.dims.y;
    let nz_ss = occ_u.dims.z;
    let nx    = occ_u.orig.x;
    let ny    = occ_u.orig.y;
    let nz    = occ_u.orig.z;
    let ss    = max(occ_u.orig.w, 1u);
    let inv_ss = 1.0 / f32(ss);

    let nc_back  = occ_u.nc.x; // = nx*ny
    let nc_left  = occ_u.nc.y; // = nz*ny
    let nc_right = occ_u.nc.z; // = nz*ny
    let nc_front = occ_u.nc.w; // = nx*ny

    // Determine which face + wall texel this thread handles.
    // Faces are laid out consecutively: back, left, right, front, floor.
    var occ: f32 = 0.0;

    let base_left  = nc_back;
    let base_right = base_left  + nc_left;
    let base_front = base_right + nc_right;
    let base_floor = base_front + nc_front;

    if tid < nc_back {
        // Face 0: back wall (z=lo). Texel = (i,j).
        let local = tid;
        let i = local % nx_ss;
        let j = local / nx_ss;
        let xf = (f32(i) + 0.5) * inv_ss - 0.5;
        let yf = (f32(j) + 0.5) * inv_ss - 0.5;
        occ = sample_liquid_x_y_band_low_z(xf, yf, nx, ny, nz);
    } else if tid < base_right {
        // Face 1: left wall (x=lo). Texel = (k,j).
        let local = tid - base_left;
        let k = local % nz_ss;
        let j = local / nz_ss;
        let zf = (f32(k) + 0.5) * inv_ss - 0.5;
        let yf = (f32(j) + 0.5) * inv_ss - 0.5;
        occ = sample_liquid_z_y_band_low_x(zf, yf, nx, ny, nz);
    } else if tid < base_front {
        // Face 2: right wall (x=hi). Texel = (k,j).
        let local = tid - base_right;
        let k = local % nz_ss;
        let j = local / nz_ss;
        let zf = (f32(k) + 0.5) * inv_ss - 0.5;
        let yf = (f32(j) + 0.5) * inv_ss - 0.5;
        occ = sample_liquid_z_y_band_high_x(zf, yf, nx, ny, nz);
    } else if tid < base_floor {
        // Face 3: front wall (z=hi). Texel = (i,j).
        let local = tid - base_front;
        let i = local % nx_ss;
        let j = local / nx_ss;
        let xf = (f32(i) + 0.5) * inv_ss - 0.5;
        let yf = (f32(j) + 0.5) * inv_ss - 0.5;
        occ = sample_liquid_x_y_band_high_z(xf, yf, nx, ny, nz);
    } else {
        // Face 4: floor (y=lo). Cell (i,k).
        let local = tid - base_floor;
        let i = local % nx_ss;
        let k = local / nx_ss;
        let xf = (f32(i) + 0.5) * inv_ss - 0.5;
        let zf = (f32(k) + 0.5) * inv_ss - 0.5;
        occ = sample_liquid_x_z_band_floor(xf, zf, nx, ny, nz);
    }

    occ_buf[tid] = occ;
}

// ============================================================
// RENDER: wall-fill injection pass
// ============================================================
//
// Full-screen triangle that, for each pixel, intersects all 5 tank planes,
// checks occupancy, and outputs MRT values matching the thickness pipeline:
//   target 0: thickness (R16Float, Add)  — write fill_slab
//   target 1: nearest_z (R16Float, Min)  — write glass-plane eye distance
//   target 2: whitewater (R16Float, Add) — write 0.0 (foam untouched)
//   target 3: wallfill_mask (R16Float, Replace) — fill coverage for composite tuning
//
// Fragments that are NOT occupied output 0.0 thickness and a large sentinel
// nearest_z (so Min doesn't change anything).

struct FillUniform {
    // x=fill_enabled(0/1 as f32), y=fill_strength, z=fill_slab, w=waterline_softness
    fill:          vec4<f32>,
    // x=nx_ss, y=ny_ss, z=nz_ss, w=total occupancy entries
    dims:          vec4<u32>,
    // x=back=nx*ny, y=left=nz*ny, z=right=nz*ny, w=front=nx*ny
    nc:            vec4<u32>,
    // x=nc_floor, yzw=unused
    nc_floor:      vec4<u32>,
    // tan(fov_y/2), width, height, unused
    cam_params:    vec4<f32>,
    // tank world-space lo corner (xyz, w=unused)
    tank_lo:       vec4<f32>,
    // tank world-space hi corner (xyz, w=unused)
    tank_hi:       vec4<f32>,
    // camera eye in box-local space (xyz, w=unused)
    box_eye_local: vec4<f32>,
    // box-local → world rotation columns (padded vec4)
    box_rot_col0:  vec4<f32>,
    box_rot_col1:  vec4<f32>,
    box_rot_col2:  vec4<f32>,
    // eye → world rotation (mat4x4, upper-left 3x3 used)
    eye_to_world:  mat4x4<f32>,
    // flat_water epsilon (x), unused yzw
    flat_epsilon:  vec4<f32>,
};

struct FillVsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0)       uv:   vec2<f32>,
};

struct FillOut {
    @location(0) thickness: f32,
    @location(1) nearest_z: f32,
    @location(2) whitewater: f32,
    @location(3) wallfill_mask: f32,
};

@group(0) @binding(3) var<uniform>       fill_u:  FillUniform;
@group(0) @binding(4) var<storage, read> fill_occ: array<f32>; // 1 f32 per wall texel/cell

@vertex
fn vs_fill(@builtin(vertex_index) vi: u32) -> FillVsOut {
    var pos = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -3.0),
        vec2<f32>( 3.0,  1.0),
        vec2<f32>(-1.0,  1.0),
    );
    let p = pos[vi];
    var out: FillVsOut;
    out.clip = vec4<f32>(p, 0.0, 1.0);
    out.uv   = p * vec2<f32>(0.5, -0.5) + vec2<f32>(0.5, 0.5);
    return out;
}

fn occ_texel(base: u32, col: i32, row: i32, width: u32, height: u32) -> f32 {
    if col < 0 || row < 0 || u32(col) >= width || u32(row) >= height {
        return 0.0;
    }
    return fill_occ[base + u32(row) * width + u32(col)];
}

fn sample_wall_occ_bilinear(base: u32, col_f: f32, row_f: f32, width: u32, height: u32) -> f32 {
    let cf = clamp(col_f, 0.0, f32(width) - 1.0);
    let rf = clamp(row_f, 0.0, f32(height) - 1.0);
    let c0 = i32(floor(cf));
    let r0 = i32(floor(rf));
    let tx = cf - f32(c0);
    let ty = rf - f32(r0);
    let c1 = min(c0 + 1, i32(width) - 1);
    let r1 = min(r0 + 1, i32(height) - 1);
    let a = occ_texel(base, c0, r0, width, height);
    let b = occ_texel(base, c1, r0, width, height);
    let c = occ_texel(base, c0, r1, width, height);
    let d = occ_texel(base, c1, r1, width, height);
    return mix(mix(a, b, tx), mix(c, d, tx), ty);
}

fn coverage_smooth(t: f32) -> f32 {
    let x = clamp(t, 0.0, 1.0);
    return x * x * (3.0 - 2.0 * x);
}

// Sample a vertical wall with true 2D interpolation in atlas space so diagonal
// liquid edges do not render as stair-stepped rows.
fn sample_wall_occ_soft(base: u32, col_f: f32, y_frac: f32, width: u32, ny: u32) -> f32 {
    let row_f = y_frac * f32(ny) - 0.5;
    let center = sample_wall_occ_bilinear(base, col_f, row_f, width, ny);
    let blur = clamp(fill_u.fill.w * 24.0, 0.0, 2.0);
    var sum = center * 4.0;
    var wsum = 4.0;
    for (var dj = -1; dj <= 1; dj++) {
        for (var di = -1; di <= 1; di++) {
            if (di == 0 && dj == 0) { continue; }
            let axis = abs(f32(di)) + abs(f32(dj));
            let w = select(1.0, 0.55, axis > 1.5);
            sum += sample_wall_occ_bilinear(base, col_f + f32(di) * blur, row_f + f32(dj) * blur, width, ny) * w;
            wsum += w;
        }
    }
    let raw = sum / wsum;
    let smooth_amount = clamp(fill_u.fill.w * 12.0, 0.0, 1.0);
    return mix(raw, coverage_smooth(raw), smooth_amount);
}

fn wall_fill_visual_weight(weight: f32) -> f32 {
    // Suppress isolated low-coverage contacts. They are useful in the occupancy
    // atlas but read as individual wall pixels once projected onto the glass.
    return smoothstep(0.08, 0.55, clamp(weight, 0.0, 1.0));
}

fn visible_corner_repair(current: f32, edge_coord: f32, y_frac: f32, nx: u32, ny: u32, nz: u32) -> f32 {
    let corner_width = 10.0;
    let corner_t = 1.0 - smoothstep(corner_width, corner_width * 2.0, edge_coord);
    if corner_t <= 0.0 {
        return current;
    }
    let back_inset = min(corner_width, f32(nx) - 1.0);
    let left_inset = min(corner_width, f32(nz) - 1.0);
    let back = sample_wall_occ_soft(0u, back_inset, y_frac, nx, ny);
    let left = sample_wall_occ_soft(fill_u.nc.x, left_inset, y_frac, nz, ny);
    let repaired = max(back, left);
    return max(current, repaired * corner_t);
}

// Interpolate occupancy for a floor cell (i,k).
fn sample_floor_occ(base: u32, i_f: f32, k_f: f32, nx: u32, nz: u32) -> f32 {
    let i0 = u32(clamp(floor(i_f), 0.0, f32(nx) - 1.0));
    let k0 = u32(clamp(floor(k_f), 0.0, f32(nz) - 1.0));
    let i1 = min(i0 + 1u, nx - 1u);
    let k1 = min(k0 + 1u, nz - 1u);
    let fi  = clamp(i_f - f32(i0), 0.0, 1.0);
    let fk  = clamp(k_f - f32(k0), 0.0, 1.0);
    let o00 = fill_occ[base + k0 * nx + i0];
    let o10 = fill_occ[base + k0 * nx + i1];
    let o01 = fill_occ[base + k1 * nx + i0];
    let o11 = fill_occ[base + k1 * nx + i1];
    return mix(mix(o00, o10, fi), mix(o01, o11, fi), fk);
}

@fragment
fn fs_fill(in: FillVsOut) -> FillOut {
    let SENTINEL: f32 = 65504.0;

    var out: FillOut;
    out.thickness = 0.0;
    out.nearest_z = SENTINEL;
    out.whitewater = 0.0;
    out.wallfill_mask = 0.0;

    let enabled = fill_u.fill.x > 0.5;
    if !enabled { return out; }

    let fill_strength = clamp(fill_u.fill.y, 0.0, 1.0);
    let fill_slab     = fill_u.fill.z * fill_strength;
    if fill_slab <= 0.0 { return out; }

    let width  = max(fill_u.cam_params.y, 1.0);
    let height = max(fill_u.cam_params.z, 1.0);
    let thf    = fill_u.cam_params.x; // tan(fov_y/2)

    // Reconstruct eye ray direction (eye space, z = -1 convention).
    let ndc    = vec2<f32>(in.uv.x * 2.0 - 1.0, 1.0 - 2.0 * in.uv.y);
    let aspect = width / height;
    let ray_eye = vec3<f32>(ndc.x * thf * aspect, ndc.y * thf, -1.0); // unnormalized

    // Transform to box-local: ray_bl = box_rot_t * eye3 * ray_eye (unnormalized direction).
    let box_rot = mat3x3<f32>(
        fill_u.box_rot_col0.xyz,
        fill_u.box_rot_col1.xyz,
        fill_u.box_rot_col2.xyz,
    );
    let eye3 = mat3x3<f32>(
        fill_u.eye_to_world[0].xyz,
        fill_u.eye_to_world[1].xyz,
        fill_u.eye_to_world[2].xyz,
    );
    let box_rot_t  = transpose(box_rot);
    let eye3_t     = transpose(eye3);
    let dir_world  = eye3 * ray_eye;       // world-space (unnormalized)
    let dir_bl     = box_rot_t * dir_world; // box-local   (unnormalized)
    let o_bl       = fill_u.box_eye_local.xyz;

    let lo_bl = fill_u.tank_lo.xyz;
    let hi_bl = fill_u.tank_hi.xyz;

    let nx   = fill_u.dims.x;
    let ny   = fill_u.dims.y;
    let nz   = fill_u.dims.z;

    let nc_back  = fill_u.nc.x;
    let nc_left  = fill_u.nc.y;
    let nc_right = fill_u.nc.z;
    let nc_front = fill_u.nc.w;
    let nc_floor = fill_u.nc_floor.x;

    let base_left  = nc_back;
    let base_right = base_left  + nc_left;
    let base_front = base_right + nc_right;
    let base_floor = base_front + nc_front;

    // Tank size in box-local units (usually 2.0 each axis for [-1,1]^3).
    let tank_size = hi_bl - lo_bl;

    // Epsilon for thin surface detection (match composite flat_water band).
    let eps = max(fill_u.flat_epsilon.x, 0.005);

    // --- Test each of the 5 tank planes ---
    // For each hit on a wall plane that is inside the tank face bounds:
    //   1. Map hit position to column index.
    //   2. Map hit y to a continuous wall-atlas coordinate.
    //   3. Bilinearly sample coverage in both wall axes and ease it by
    //      waterline_softness to anti-alias stair-stepped wet/dry edges.
    //   4. Accumulate best (thinnest) hit into output.

    // Best-so-far: the hit that contributes the nearest_z = minimum eye distance.
    var best_z:   f32 = SENTINEL;
    var best_slab: f32 = 0.0;
    var best_mask: f32 = 0.0;

    // Helper: convert box-local hit point to eye-space z (positive = in front of camera).
    // pos_bl is the hit point. We subtract camera origin (which is o_bl in box-local),
    // rotate back to world, then to eye space.
    // eye_z = -( eye3_t * (box_rot * (pos_bl - o_bl)) ).z
    // Since dir_world = eye3 * dir_eye, eye3_t * dir_world = dir_eye, so:
    // eye_pos = eye3_t * (box_rot * delta_bl)  => z_eye = ..., front_z = -z_eye

    // --- Face 0: back wall z=lo_bl.z, inward normal +z ---
    {
        let denom = dir_bl.z;
        if abs(denom) > 1.0e-5 {
            let ray_t = (lo_bl.z - o_bl.z) / denom;
            if ray_t > 0.0 {
                let hit = o_bl + ray_t * dir_bl;
                // Check hit is within face bounds (x in [lo.x,hi.x], y in [lo.y,hi.y]).
                if hit.x >= lo_bl.x && hit.x <= hi_bl.x &&
                   hit.y >= lo_bl.y && hit.y <= hi_bl.y {
                    // Column = i dimension (x). Subtract 0.5: cell centers are at integer
                    // indices in the occupancy buffer (cell i spans [i*dx,(i+1)*dx],
                    // center at (i+0.5)*dx => raw frac = i+0.5, so i+0.5 - 0.5 = i).
                    let i_frac = (hit.x - lo_bl.x) / tank_size.x * f32(nx) - 0.5;
                    let y_frac = (hit.y - lo_bl.y) / tank_size.y;
                    let raw_weight = sample_wall_occ_soft(0u, i_frac, y_frac, nx, ny);
                    let repaired = visible_corner_repair(raw_weight, i_frac, y_frac, nx, ny, nz);
                    let weight = wall_fill_visual_weight(repaired);
                    if weight > 0.001 {
                        // Eye-space z at this hit.
                        let delta_bl  = hit - o_bl;
                        let dw        = box_rot * delta_bl;
                        let de        = eye3_t * dw;
                        let fz        = -de.z;
                        if fz > 0.0 && fz < best_z {
                            best_z    = fz;
                            best_slab = fill_slab * weight;
                            best_mask = weight;
                        }
                    }
                }
            }
        }
    }

    // --- Face 1: left wall x=lo_bl.x, inward normal +x ---
    {
        let denom = dir_bl.x;
        if abs(denom) > 1.0e-5 {
            let ray_t = (lo_bl.x - o_bl.x) / denom;
            if ray_t > 0.0 {
                let hit = o_bl + ray_t * dir_bl;
                if hit.z >= lo_bl.z && hit.z <= hi_bl.z &&
                   hit.y >= lo_bl.y && hit.y <= hi_bl.y {
                    // Column = k dimension (z). Subtract 0.5 for cell-center alignment.
                    let k_frac = (hit.z - lo_bl.z) / tank_size.z * f32(nz) - 0.5;
                    let y_frac = (hit.y - lo_bl.y) / tank_size.y;
                    let raw_weight = sample_wall_occ_soft(base_left, k_frac, y_frac, nz, ny);
                    let repaired = visible_corner_repair(raw_weight, k_frac, y_frac, nx, ny, nz);
                    let weight = wall_fill_visual_weight(repaired);
                    if weight > 0.001 {
                        let delta_bl = hit - o_bl;
                        let dw = box_rot * delta_bl;
                        let de = eye3_t * dw;
                        let fz = -de.z;
                        if fz > 0.0 && fz < best_z {
                            best_z    = fz;
                            best_slab = fill_slab * weight;
                            best_mask = weight;
                        }
                    }
                }
            }
        }
    }

    // The environment intentionally leaves the right and front walls open for
    // viewing. Injecting fill sheets on those hidden planes creates apparent
    // vertical seams that move with camera/box rotation, so only the rendered
    // back and left wall planes contribute visible wall fill.

    // --- Face 4: floor y=lo_bl.y ---
    // NOTE (nit): The floor is NOT a glass wall, so injecting fill_slab here does not
    // contribute to the glass-flatness goal but does add thickness across the whole wetted
    // floor area via Add-blend, tinting and darkening the floor. The nearest_z injection
    // is harmless (floor is always farthest). Floor branch is disabled to avoid gratuitous
    // over-darkening; re-enable if a flat-floor sheet effect is desired in future.
    // {
    //     let denom = dir_bl.y;
    //     ...floor branch omitted...
    // }

    // Output: if we found a hit, inject the fill surface.
    // nearest_z uses Min blend → outputting best_z makes it win for against-glass pixels.
    // thickness uses Add blend → output fill_slab so the sheet has body.
    //   NOTE: Add-blend means fill_slab is added on top of any existing particle thickness.
    //   For against-glass pixels that already have particle splats, this slightly over-darkens
    //   them vs pure open water, creating a faint seam. Kept intentionally small (default 0.03)
    //   to minimize the artefact; Min-style thickness would need a separate target.
    // whitewater uses Add blend → output 0.0 (foam untouched).
    if best_z < SENTINEL - 1.0 {
        out.thickness  = best_slab;
        out.nearest_z  = best_z;
        out.whitewater = 0.0;
        out.wallfill_mask = clamp(best_mask, 0.0, 1.0);
    } else {
        // No hit: output 0 thickness and sentinel nearest_z — Min leaves existing intact.
        out.thickness  = 0.0;
        out.nearest_z  = SENTINEL;
        out.whitewater = 0.0;
        out.wallfill_mask = 0.0;
    }
    return out;
}
