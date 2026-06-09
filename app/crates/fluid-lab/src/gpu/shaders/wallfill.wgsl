// Wall-fill: continuous gap-filled flat water sheet against glass (v1.21).
//
// TWO entry points in this file:
//   - `cs_occupancy`:  compute pass — writes per-column occupancy + waterline into
//                      the occ_buf storage buffer (see layout below).
//   - `vs` / `fs_fill`: render pass  — full-screen triangle that injects flat glass-
//                      plane surface into the thickness/nearest_z/whitewater MRT.
//
// ============================================================
// COMPUTE: occupancy + waterline
// ============================================================
//
// Buffer layout (two f32 per column, interleaved as [occ, waterline_frac]):
//   Face 0 (back  z=lo, vertical wall): columns i in [0,nx) — nc_back  = nx
//   Face 1 (left  x=lo, vertical wall): columns k in [0,nz) — nc_left  = nz
//   Face 2 (right x=hi, vertical wall): columns k in [0,nz) — nc_right = nz
//   Face 3 (front z=hi, vertical wall): columns i in [0,nx) — nc_front = nx
//   Face 4 (floor y=lo, floor plane)  : cell (i,k)          — nc_floor = nx*nz
//
// For vertical walls each "column" is a vertical strip (fixed i or k, j varies 0..ny-2).
// We scan j from top (ny-2) down to 1 and record:
//   occ          = 1.0 if ANY interior cell in the column is Liquid, else 0.0
//   waterline    = (topmost_liquid_j + 1) / ny  as a fraction [0,1]
//              This is the CELL-TOP fraction: the top face of cell j sits at (j+1)/ny
//              in the same [0,1] y_frac coordinate the render uses for hit.y.
//              (0.0 = floor, 1.0 = full tank height; 0.0 when no liquid)
// For the floor each (i,k) cell stores occ=1 if that cell is Liquid, waterline unused (1.0).
//
// One workgroup dispatch covers all columns across all 5 faces.

struct OccUniform {
    // x=nx, y=ny, z=nz, w=total_columns (sum of all face column counts)
    dims:     vec4<u32>,
    // x=nc_back=nx, y=nc_left=nz, z=nc_right=nz, w=nc_front=nx
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
@group(0) @binding(2) var<storage, read_write> occ_buf: array<f32>; // 2 f32 per column

const WG: u32 = 64u;
const LIQUID: u32 = 1u;

// cell_type linear index: i + j*nx + k*nx*ny
fn ct_idx(i: u32, j: u32, k: u32, nx: u32, ny: u32) -> u32 {
    return i + j * nx + k * nx * ny;
}

@compute @workgroup_size(WG)
fn cs_occupancy(@builtin(global_invocation_id) gid: vec3<u32>) {
    let tid = gid.x;
    let total = occ_u.dims.w;
    if tid >= total { return; }

    let nx   = occ_u.dims.x;
    let ny   = occ_u.dims.y;
    let nz   = occ_u.dims.z;

    let nc_back  = occ_u.nc.x; // = nx
    let nc_left  = occ_u.nc.y; // = nz
    let nc_right = occ_u.nc.z; // = nz
    let nc_front = occ_u.nc.w; // = nx
    let nc_floor = occ_u.nc_floor.x; // = nx*nz

    // Determine which face + column index this thread handles.
    // Faces are laid out consecutively: back, left, right, front, floor.
    var occ: f32      = 0.0;
    var wl_frac: f32  = 0.0;

    let base_left  = nc_back;
    let base_right = base_left  + nc_left;
    let base_front = base_right + nc_right;
    let base_floor = base_front + nc_front;

    if tid < nc_back {
        // Face 0: back wall (z=lo, inward cell k=1). Column = i (horizontal).
        let i = tid;
        var top_j: u32 = 0u;
        var found: bool = false;
        // Scan j from ny-2 down to 1 (interior rows; rows 0 and ny-1 are Solid boundary).
        // Loop: j steps ny-2, ny-3, ..., 1. Exit when j wraps past 0 (u32 underflow guard).
        var j: u32 = ny - 2u;
        loop {
            if cell_type[ct_idx(i, j, 1u, nx, ny)] == LIQUID {
                if !found { top_j = j; found = true; }
                occ = 1.0;
            }
            if j == 1u { break; }
            j -= 1u;
        }
        if found {
            // Cell-top fraction: top face of cell top_j sits at (top_j+1)/ny in [0,1].
            wl_frac = f32(top_j + 1u) / f32(ny);
        }
    } else if tid < base_right {
        // Face 1: left wall (x=lo, inward cell i=1). Column = k (depth).
        let k = tid - base_left;
        var top_j: u32 = 0u;
        var found: bool = false;
        var j: u32 = ny - 2u;
        loop {
            if cell_type[ct_idx(1u, j, k, nx, ny)] == LIQUID {
                if !found { top_j = j; found = true; }
                occ = 1.0;
            }
            if j == 1u { break; }
            j -= 1u;
        }
        if found {
            wl_frac = f32(top_j + 1u) / f32(ny);
        }
    } else if tid < base_front {
        // Face 2: right wall (x=hi, inward cell i=nx-2). Column = k.
        let k = tid - base_right;
        let ii = nx - 2u;
        var top_j: u32 = 0u;
        var found: bool = false;
        var j: u32 = ny - 2u;
        loop {
            if cell_type[ct_idx(ii, j, k, nx, ny)] == LIQUID {
                if !found { top_j = j; found = true; }
                occ = 1.0;
            }
            if j == 1u { break; }
            j -= 1u;
        }
        if found {
            wl_frac = f32(top_j + 1u) / f32(ny);
        }
    } else if tid < base_floor {
        // Face 3: front wall (z=hi, inward cell k=nz-2). Column = i.
        let i = tid - base_front;
        let kk = nz - 2u;
        var top_j: u32 = 0u;
        var found: bool = false;
        var j: u32 = ny - 2u;
        loop {
            if cell_type[ct_idx(i, j, kk, nx, ny)] == LIQUID {
                if !found { top_j = j; found = true; }
                occ = 1.0;
            }
            if j == 1u { break; }
            j -= 1u;
        }
        if found {
            wl_frac = f32(top_j + 1u) / f32(ny);
        }
    } else {
        // Face 4: floor (y=lo, inward cell j=1). Cell (i,k).
        let local = tid - base_floor;
        let i = local % nx;
        let k = local / nx;
        if cell_type[ct_idx(i, 1u, k, nx, ny)] == LIQUID {
            occ = 1.0;
        }
        wl_frac = 1.0; // floor: no vertical waterline needed
    }

    occ_buf[tid * 2u]      = occ;
    occ_buf[tid * 2u + 1u] = wl_frac;
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
//
// Fragments that are NOT occupied / above waterline output 0.0 thickness
// and a large sentinel nearest_z (so Min doesn't change anything).

struct FillUniform {
    // x=fill_enabled(0/1 as f32), y=fill_strength, z=fill_slab, w=waterline_softness
    fill:          vec4<f32>,
    // x=nx, y=ny, z=nz, w=total_columns
    dims:          vec4<u32>,
    // x=nc_back, y=nc_left, z=nc_right, w=nc_front
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
};

@group(0) @binding(3) var<uniform>       fill_u:  FillUniform;
@group(0) @binding(4) var<storage, read> fill_occ: array<f32>; // 2 f32 per column

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

// Sample occupancy buffer with bilinear-ish interpolation across columns.
// col_f: fractional column index (0..nc-1). Returns vec2(occ, waterline_frac).
fn sample_occ(base: u32, col_f: f32, nc: u32) -> vec2<f32> {
    let nc_f  = f32(nc);
    let cf    = clamp(col_f, 0.0, nc_f - 1.0);
    let c0    = u32(floor(cf));
    let c1    = min(c0 + 1u, nc - 1u);
    let frac  = cf - f32(c0);
    let a     = vec2<f32>(fill_occ[(base + c0) * 2u], fill_occ[(base + c0) * 2u + 1u]);
    let b     = vec2<f32>(fill_occ[(base + c1) * 2u], fill_occ[(base + c1) * 2u + 1u]);
    return mix(a, b, frac);
}

// Interpolate occupancy for a floor cell (i,k).
fn sample_floor_occ(base: u32, i_f: f32, k_f: f32, nx: u32, nz: u32) -> f32 {
    let i0 = u32(clamp(floor(i_f), 0.0, f32(nx) - 1.0));
    let k0 = u32(clamp(floor(k_f), 0.0, f32(nz) - 1.0));
    let i1 = min(i0 + 1u, nx - 1u);
    let k1 = min(k0 + 1u, nz - 1u);
    let fi  = clamp(i_f - f32(i0), 0.0, 1.0);
    let fk  = clamp(k_f - f32(k0), 0.0, 1.0);
    let o00 = fill_occ[(base + k0 * nx + i0) * 2u];
    let o10 = fill_occ[(base + k0 * nx + i1) * 2u];
    let o01 = fill_occ[(base + k1 * nx + i0) * 2u];
    let o11 = fill_occ[(base + k1 * nx + i1) * 2u];
    return mix(mix(o00, o10, fi), mix(o01, o11, fi), fk);
}

@fragment
fn fs_fill(in: FillVsOut) -> FillOut {
    let SENTINEL: f32 = 65504.0;

    var out: FillOut;
    out.thickness = 0.0;
    out.nearest_z = SENTINEL;
    out.whitewater = 0.0;

    let enabled = fill_u.fill.x > 0.5;
    if !enabled { return out; }

    let fill_strength = fill_u.fill.y;
    let fill_slab     = fill_u.fill.z * fill_strength;
    let wl_softness   = max(fill_u.fill.w, 0.001);
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
    //   2. Bilinearly sample occupancy + waterline.
    //   3. If occupied, compute feathered weight.
    //   4. Accumulate best (thinnest) hit into output.

    // Best-so-far: the hit that contributes the nearest_z = minimum eye distance.
    var best_z:   f32 = SENTINEL;
    var best_slab: f32 = 0.0;

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
                    let wdata  = sample_occ(0u, i_frac, nc_back);
                    let occ    = wdata.x;
                    let wl_frac = wdata.y;
                    // Vertical position fraction in [0,1].
                    let y_frac = (hit.y - lo_bl.y) / tank_size.y;
                    // Waterline feather: fade out above wl_frac.
                    let above_wl = y_frac - wl_frac;
                    let wl_t = 1.0 - smoothstep(0.0, wl_softness, above_wl);
                    let weight = occ * wl_t;
                    if weight > 0.001 {
                        // Eye-space z at this hit.
                        let delta_bl  = hit - o_bl;
                        let dw        = box_rot * delta_bl;
                        let de        = eye3_t * dw;
                        let fz        = -de.z;
                        if fz > 0.0 && fz < best_z {
                            best_z    = fz;
                            best_slab = fill_slab * weight;
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
                    let wdata  = sample_occ(base_left, k_frac, nc_left);
                    let occ    = wdata.x;
                    let wl_frac = wdata.y;
                    let y_frac = (hit.y - lo_bl.y) / tank_size.y;
                    let above_wl = y_frac - wl_frac;
                    let wl_t   = 1.0 - smoothstep(0.0, wl_softness, above_wl);
                    let weight = occ * wl_t;
                    if weight > 0.001 {
                        let delta_bl = hit - o_bl;
                        let dw = box_rot * delta_bl;
                        let de = eye3_t * dw;
                        let fz = -de.z;
                        if fz > 0.0 && fz < best_z {
                            best_z    = fz;
                            best_slab = fill_slab * weight;
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
                    let wdata  = sample_occ(base_right, k_frac, nc_right);
                    let occ    = wdata.x;
                    let wl_frac = wdata.y;
                    let y_frac = (hit.y - lo_bl.y) / tank_size.y;
                    let above_wl = y_frac - wl_frac;
                    let wl_t   = 1.0 - smoothstep(0.0, wl_softness, above_wl);
                    let weight = occ * wl_t;
                    if weight > 0.001 {
                        let delta_bl = hit - o_bl;
                        let dw = box_rot * delta_bl;
                        let de = eye3_t * dw;
                        let fz = -de.z;
                        if fz > 0.0 && fz < best_z {
                            best_z    = fz;
                            best_slab = fill_slab * weight;
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
                    let wdata  = sample_occ(base_front, i_frac, nc_front);
                    let occ    = wdata.x;
                    let wl_frac = wdata.y;
                    let y_frac = (hit.y - lo_bl.y) / tank_size.y;
                    let above_wl = y_frac - wl_frac;
                    let wl_t   = 1.0 - smoothstep(0.0, wl_softness, above_wl);
                    let weight = occ * wl_t;
                    if weight > 0.001 {
                        let delta_bl = hit - o_bl;
                        let dw = box_rot * delta_bl;
                        let de = eye3_t * dw;
                        let fz = -de.z;
                        if fz > 0.0 && fz < best_z {
                            best_z    = fz;
                            best_slab = fill_slab * weight;
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
    } else {
        // No hit: output 0 thickness and sentinel nearest_z — Min leaves existing intact.
        out.thickness  = 0.0;
        out.nearest_z  = SENTINEL;
        out.whitewater = 0.0;
    }
    return out;
}
