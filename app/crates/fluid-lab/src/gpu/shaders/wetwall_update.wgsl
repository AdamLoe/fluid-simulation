// Wet-wall update pass (v1.17). One invocation per wall-surface texel.
//
// Layout of the wetness buffer:
//   texels [0 .. nx*ny)               — back wall  (z = lo.z):  i in [0,nx), j in [0,ny)
//   texels [nx*ny .. nx*ny + nz*ny)   — left wall  (x = lo.x):  k in [0,nz), j in [0,ny)
//   texels [nx*ny+nz*ny .. total)     — floor       (y = lo.y):  i in [0,nx), k in [0,nz)
//
// Contact detection: a wall texel is "wet" if the adjacent interior cell (just one
// cell inward from the boundary Solid row/column) has cell_type == 1 (Liquid).
// Solid boundary cells occupy the outermost ring (i/j/k == 0 or n-1). The one-cell-
// inward neighbour is at index 1 or n-2 on that axis.
//
// Decay: wetness[t] = max(new_contact * contact_gain, wetness[t-1] * decay_per_frame)
// where decay_per_frame = pow(wetness_decay, dt * 60.0).

struct WetWallUniform {
    // x=nx, y=ny, z=nz, w=total_wetness_texels
    dims: vec4<u32>,
    // xy = back_wall count (nx*ny), zw = left_wall count (nz*ny)  (floor fills the rest)
    face_counts: vec4<u32>,
    // x=wetness_decay, y=dt, z=contact_gain, w=enabled(0/1)
    params: vec4<f32>,
    // tank world-space bounds (unused in compute, kept for symmetry with env uniform)
    tank_lo: vec4<f32>,
    tank_hi: vec4<f32>,
};

@group(0) @binding(0) var<uniform>        wu:         WetWallUniform;
@group(0) @binding(1) var<storage, read>  cell_type:  array<u32>;
@group(0) @binding(2) var<storage, read_write> wetness: array<f32>;

// cell_type buffer layout: linear index = i + j*nx + k*nx*ny  (i=x, j=y, k=z)
fn cell_idx(i: u32, j: u32, k: u32) -> u32 {
    return i + j * wu.dims.x + k * wu.dims.x * wu.dims.y;
}

const WG: u32 = 64u;

@compute @workgroup_size(WG)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let tid = gid.x;
    let total = wu.dims.w;
    if tid >= total { return; }

    let enabled = wu.params.w > 0.5;
    if !enabled {
        return;
    }

    let nx = wu.dims.x;
    let ny = wu.dims.y;
    let nz = wu.dims.z;
    let back_count = wu.face_counts.x; // nx * ny
    let left_count = wu.face_counts.z; // nz * ny

    // Determine if this texel contacts a Liquid cell.
    var new_contact: f32 = 0.0;

    if tid < back_count {
        // Back wall (z = lo.z = 0). Solid row is k=0; inward cell is k=1.
        let i = tid % nx;
        let j = tid / nx;
        let ct = cell_type[cell_idx(i, j, 1u)];
        if ct == 1u { new_contact = 1.0; }
    } else if tid < back_count + left_count {
        // Left wall (x = lo.x = 0). Solid column is i=0; inward cell is i=1.
        let local = tid - back_count;
        let k = local % nz;
        let j = local / nz;
        let ct = cell_type[cell_idx(1u, j, k)];
        if ct == 1u { new_contact = 1.0; }
    } else {
        // Floor (y = lo.y = 0). Solid row is j=0; inward cell is j=1.
        let local = tid - back_count - left_count;
        let i = local % nx;
        let k = local / nx;
        let ct = cell_type[cell_idx(i, 1u, k)];
        if ct == 1u { new_contact = 1.0; }
    }

    let contact_gain    = wu.params.z;
    let decay_base      = wu.params.x; // per-second decay factor (e.g. 0.97 at 60 fps)
    let dt              = wu.params.y;
    // Framerate-independent decay: decay^(dt*60) so that at 60 fps decay=0.97
    // gives per-frame multiplier 0.97, at 30 fps gives 0.97^2, etc.
    let decay_per_frame = pow(decay_base, dt * 60.0);

    let prev = wetness[tid];
    wetness[tid] = max(new_contact * contact_gain, prev * decay_per_frame);
}
