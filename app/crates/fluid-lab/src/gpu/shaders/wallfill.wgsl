// Wall-fill: dense per-wall-cell flat water sheet against glass (v1.21).
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
//   Face 0 (back  z=lo, vertical wall): nx_ss*ny_ss, index j*nx_ss + i, inward cell k=1
//   Face 1 (left  x=lo, vertical wall): nz_ss*ny_ss, index j*nz_ss + k, inward cell i=1
//   Face 2 (right x=hi, vertical wall): nz_ss*ny_ss, index j*nz_ss + k, inward cell i=nx-2
//   Face 3 (front z=hi, vertical wall): nx_ss*ny_ss, index j*nx_ss + i, inward cell k=nz-2
//   Face 4 (floor y=lo, floor plane)  : nx_ss*nz_ss, index k*nx_ss + i, inward cell j=1
//
// One workgroup dispatch covers all wall texels across all 5 faces.

struct OccUniform {
    // x=nx_ss, y=ny_ss, z=nz_ss, w=total occupancy entries
    dims:     vec4<u32>,
    // x=nx, y=ny, z=nz, w=ss
    orig:     vec4<u32>,
    // x=back=nx*ny, y=left=nz*ny, z=right=nz*ny, w=front=nx*ny
    nc:       vec4<u32>,
    // x=nc_floor (nx*nz), yzw=unused
    nc_floor: vec4<u32>,
    // x=fill_enabled(0/1), y=fill_strength, z=fill_slab, w=waterline_softness
    fill:     vec4<f32>,
    // tank world-space bounds
    tank_lo:  vec4<f32>,
    tank_hi:  vec4<f32>,
};

@group(0) @binding(0) var<uniform>        occ_u:     OccUniform;
@group(0) @binding(1) var<storage, read>  cell_type: array<u32>;
@group(0) @binding(2) var<storage, read_write> occ_buf: array<f32>; // 1 f32 per wall texel/cell

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

@compute @workgroup_size(WG)
fn cs_occupancy(@builtin(global_invocation_id) gid: vec3<u32>) {
    let tid = gid.x;
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
        // Face 0: back wall (z=lo, inward cell k=1). Texel = (i,j).
        let local = tid;
        let i = local % nx_ss;
        let j = local / nx_ss;
        let xf = (f32(i) + 0.5) * inv_ss - 0.5;
        let yf = (f32(j) + 0.5) * inv_ss - 0.5;
        occ = sample_liquid_x_y(xf, yf, 1u, nx, ny);
    } else if tid < base_right {
        // Face 1: left wall (x=lo, inward cell i=1). Texel = (k,j).
        let local = tid - base_left;
        let k = local % nz_ss;
        let j = local / nz_ss;
        let zf = (f32(k) + 0.5) * inv_ss - 0.5;
        let yf = (f32(j) + 0.5) * inv_ss - 0.5;
        occ = sample_liquid_z_y(zf, yf, 1u, nx, ny, nz);
    } else if tid < base_front {
        // Face 2: right wall (x=hi, inward cell i=nx-2). Texel = (k,j).
        let local = tid - base_right;
        let k = local % nz_ss;
        let j = local / nz_ss;
        let zf = (f32(k) + 0.5) * inv_ss - 0.5;
        let yf = (f32(j) + 0.5) * inv_ss - 0.5;
        let ii = nx - 2u;
        occ = sample_liquid_z_y(zf, yf, ii, nx, ny, nz);
    } else if tid < base_floor {
        // Face 3: front wall (z=hi, inward cell k=nz-2). Texel = (i,j).
        let local = tid - base_front;
        let i = local % nx_ss;
        let j = local / nx_ss;
        let xf = (f32(i) + 0.5) * inv_ss - 0.5;
        let yf = (f32(j) + 0.5) * inv_ss - 0.5;
        let kk = nz - 2u;
        occ = sample_liquid_x_y(xf, yf, kk, nx, ny);
    } else {
        // Face 4: floor (y=lo, inward cell j=1). Cell (i,k).
        let local = tid - base_floor;
        let i = local % nx_ss;
        let k = local / nx_ss;
        let xf = (f32(i) + 0.5) * inv_ss - 0.5;
        let zf = (f32(k) + 0.5) * inv_ss - 0.5;
        occ = sample_liquid_x_z(xf, zf, 1u, nx, ny, nz);
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

// Sample a vertical wall with true 2D interpolation in atlas space. Earlier versions
// snapped y to one atlas row and only interpolated horizontally, which made diagonal
// liquid edges render as stair-stepped rows even at higher atlas resolutions.
fn sample_wall_occ_soft(base: u32, col_f: f32, y_frac: f32, width: u32, ny: u32) -> f32 {
    let row_f = y_frac * f32(ny) - 0.5;
    let raw = sample_wall_occ_bilinear(base, col_f, row_f, width, ny);
    let smooth_amount = clamp(fill_u.fill.w * 8.0, 0.0, 1.0);
    return mix(raw, coverage_smooth(raw), smooth_amount);
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
                    let weight = sample_wall_occ_soft(0u, i_frac, y_frac, nx, ny);
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
                    let weight = sample_wall_occ_soft(base_left, k_frac, y_frac, nz, ny);
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

    // --- Face 2: right wall x=hi_bl.x, inward normal -x ---
    {
        let denom = dir_bl.x;
        if abs(denom) > 1.0e-5 {
            let ray_t = (hi_bl.x - o_bl.x) / denom;
            if ray_t > 0.0 {
                let hit = o_bl + ray_t * dir_bl;
                if hit.z >= lo_bl.z && hit.z <= hi_bl.z &&
                   hit.y >= lo_bl.y && hit.y <= hi_bl.y {
                    let k_frac = (hit.z - lo_bl.z) / tank_size.z * f32(nz) - 0.5;
                    let y_frac = (hit.y - lo_bl.y) / tank_size.y;
                    let weight = sample_wall_occ_soft(base_right, k_frac, y_frac, nz, ny);
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

    // --- Face 3: front wall z=hi_bl.z, inward normal -z ---
    {
        let denom = dir_bl.z;
        if abs(denom) > 1.0e-5 {
            let ray_t = (hi_bl.z - o_bl.z) / denom;
            if ray_t > 0.0 {
                let hit = o_bl + ray_t * dir_bl;
                if hit.x >= lo_bl.x && hit.x <= hi_bl.x &&
                   hit.y >= lo_bl.y && hit.y <= hi_bl.y {
                    let i_frac = (hit.x - lo_bl.x) / tank_size.x * f32(nx) - 0.5;
                    let y_frac = (hit.y - lo_bl.y) / tank_size.y;
                    let weight = sample_wall_occ_soft(base_front, i_frac, y_frac, nx, ny);
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
