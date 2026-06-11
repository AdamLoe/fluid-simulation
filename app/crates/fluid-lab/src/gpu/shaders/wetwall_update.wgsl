// Wet-wall update pass (v1.17). One invocation per wall-surface texel.
//
// Layout of the wetness buffer:
//   texels [0 .. nx_ss*ny_ss)                         — back wall (z = lo.z)
//   texels [nx_ss*ny_ss .. + nz_ss*ny_ss)             — left wall (x = lo.x)
//   texels [nx_ss*ny_ss+nz_ss*ny_ss .. total)         — floor     (y = lo.y)
//
// Contact detection: a wall texel is "wet" based on bilinear liquid coverage from
// the adjacent interior cell layer (just one cell inward from the boundary Solid
// row/column). Supersampled texels sample neighboring cell_type values in the two
// wall axes, giving fractional wet/dry edges instead of identical ss*ss blocks.
// Solid boundary cells occupy the outermost ring (i/j/k == 0 or n-1). The one-cell-
// inward neighbour is at index 1 or n-2 on that axis.
//
// Decay: wetness[t] = max(new_contact * contact_gain, wetness[t-1] * decay_per_frame)
// where decay_per_frame = pow(wetness_decay, dt * 60.0).

struct WetWallUniform {
    // x=nx_ss, y=ny_ss, z=nz_ss, w=total_wetness_texels (supersampled counts)
    dims: vec4<u32>,
    // x=back_count_ss, y=supersample, z=left_count_ss, w=nx (original sim-grid cell count)
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
// IMPORTANT: wu.dims.x/y/z are SUPERSAMPLED counts (nx_ss = nx * ss).
// The cell_type buffer is indexed with the ORIGINAL sim-grid dims (nx, ny, nz).
// Recover them by dividing by ss (the division is exact because nx_ss = nx * ss).
fn cell_idx(i: u32, j: u32, k: u32) -> u32 {
    let ss = wu.face_counts.y;
    let nx = wu.dims.x / ss;
    let ny = wu.dims.y / ss;
    return i + j * nx + k * nx * ny;
}

fn liquid_cell(i: i32, j: i32, k: i32) -> f32 {
    let ss = wu.face_counts.y;
    let nx = wu.dims.x / ss;
    let ny = wu.dims.y / ss;
    let nz = wu.dims.z / ss;
    if i < 0 || j < 0 || k < 0 {
        return 0.0;
    }
    if u32(i) >= nx || u32(j) >= ny || u32(k) >= nz {
        return 0.0;
    }
    let ct = cell_type[cell_idx(u32(i), u32(j), u32(k))];
    return select(0.0, 1.0, ct == 1u);
}

fn ss_source_coord(coord_ss: u32, ss: u32) -> f32 {
    return (f32(coord_ss) + 0.5) / f32(ss) - 0.5;
}

fn bilinear_liquid_xy(i_ss: u32, j_ss: u32, k: i32, ss: u32) -> f32 {
    let fi = ss_source_coord(i_ss, ss);
    let fj = ss_source_coord(j_ss, ss);
    let i0 = i32(floor(fi));
    let j0 = i32(floor(fj));
    let tx = fract(fi);
    let ty = fract(fj);
    let w00 = liquid_cell(i0,      j0,      k);
    let w10 = liquid_cell(i0 + 1,  j0,      k);
    let w01 = liquid_cell(i0,      j0 + 1,  k);
    let w11 = liquid_cell(i0 + 1,  j0 + 1,  k);
    return mix(mix(w00, w10, tx), mix(w01, w11, tx), ty);
}

fn bilinear_liquid_zy(k_ss: u32, j_ss: u32, i: i32, ss: u32) -> f32 {
    let fk = ss_source_coord(k_ss, ss);
    let fj = ss_source_coord(j_ss, ss);
    let k0 = i32(floor(fk));
    let j0 = i32(floor(fj));
    let tk = fract(fk);
    let ty = fract(fj);
    let w00 = liquid_cell(i, j0,      k0);
    let w10 = liquid_cell(i, j0,      k0 + 1);
    let w01 = liquid_cell(i, j0 + 1,  k0);
    let w11 = liquid_cell(i, j0 + 1,  k0 + 1);
    return mix(mix(w00, w10, tk), mix(w01, w11, tk), ty);
}

fn bilinear_liquid_xz(i_ss: u32, k_ss: u32, j: i32, ss: u32) -> f32 {
    let fi = ss_source_coord(i_ss, ss);
    let fk = ss_source_coord(k_ss, ss);
    let i0 = i32(floor(fi));
    let k0 = i32(floor(fk));
    let tx = fract(fi);
    let tz = fract(fk);
    let w00 = liquid_cell(i0,      j, k0);
    let w10 = liquid_cell(i0 + 1,  j, k0);
    let w01 = liquid_cell(i0,      j, k0 + 1);
    let w11 = liquid_cell(i0 + 1,  j, k0 + 1);
    return mix(mix(w00, w10, tx), mix(w01, w11, tx), tz);
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

    let nx_ss = wu.dims.x;      // supersampled x count
    let ny_ss = wu.dims.y;      // supersampled y count
    let nz_ss = wu.dims.z;      // supersampled z count
    let ss         = wu.face_counts.y; // supersample factor (1-4)
    let back_count = wu.face_counts.x; // nx_ss * ny_ss
    let left_count = wu.face_counts.z; // nz_ss * ny_ss

    // Determine fractional liquid contact for this supersampled wall texel.
    var new_contact: f32 = 0.0;

    if tid < back_count {
        // Back wall (z = lo.z = 0). Solid row is k=0; inward cell is k=1.
        let i_ss = tid % nx_ss;
        let j_ss = tid / nx_ss;
        new_contact = bilinear_liquid_xy(i_ss, j_ss, 1, ss);
    } else if tid < back_count + left_count {
        // Left wall (x = lo.x = 0). Solid column is i=0; inward cell is i=1.
        let local = tid - back_count;
        let k_ss = local % nz_ss;
        let j_ss = local / nz_ss;
        new_contact = bilinear_liquid_zy(k_ss, j_ss, 1, ss);
    } else {
        // Floor (y = lo.y = 0). Solid row is j=0; inward cell is j=1.
        let local = tid - back_count - left_count;
        let i_ss = local % nx_ss;
        let k_ss = local / nx_ss;
        new_contact = bilinear_liquid_xz(i_ss, k_ss, 1, ss);
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
