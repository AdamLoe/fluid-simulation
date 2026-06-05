// Subtract pressure gradient from one face component (AXIS 0:u,1:v,2:w):
// vel -= (dt/rho)*(p_hi - p_lo)/h, only on faces between two non-solid cells.
// Air-cell pressure is 0 (jacobi writes 0 for non-liquid), so reads are direct.
// Faces touching a solid are left for the boundary pass to zero.

override AXIS: u32 = 0u;

struct Params {
    dims: vec4<u32>,
    geom: vec4<f32>,   // h, inv_h, dt, fixed_scale
    phys: vec4<f32>,   // gravity_y, rho, flip_blend, _
    origin: vec4<f32>,
    grav: vec4<f32>,
    spc:  vec4<f32>,
    cls:  vec4<f32>,
    gdim: vec4<u32>,   // nx, ny, nz, _
};
@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> pressure: array<f32>;
@group(0) @binding(2) var<storage, read> cell_type: array<u32>;
@group(0) @binding(3) var<storage, read_write> vel: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let nx = params.gdim.x;
    let ny = params.gdim.y;
    let nz = params.gdim.z;
    var dim = vec3<u32>(nx, ny, nz);
    var nax: u32;
    if (AXIS == 0u) { dim.x = nx + 1u; nax = nx; }
    else if (AXIS == 1u) { dim.y = ny + 1u; nax = ny; }
    else { dim.z = nz + 1u; nax = nz; }

    let idx = gid.x;
    if (idx >= dim.x * dim.y * dim.z) { return; }

    let i = idx % dim.x;
    let j = (idx / dim.x) % dim.y;
    let k = idx / (dim.x * dim.y);

    var comp: u32;
    if (AXIS == 0u) { comp = i; } else if (AXIS == 1u) { comp = j; } else { comp = k; }

    // Interior faces only (both adjacent cells exist).
    if (comp == 0u || comp == nax) { return; }

    var lo: u32; var hi: u32;
    if (AXIS == 0u) {
        lo = (i - 1u) + nx * (j + ny * k);
        hi = i + nx * (j + ny * k);
    } else if (AXIS == 1u) {
        lo = i + nx * ((j - 1u) + ny * k);
        hi = i + nx * (j + ny * k);
    } else {
        lo = i + nx * (j + ny * (k - 1u));
        hi = i + nx * (j + ny * k);
    }

    let lo_t = cell_type[lo];
    let hi_t = cell_type[hi];
    if (lo_t == 0u || hi_t == 0u) { return; } // touches solid -> boundary pass
    // At least one must be liquid for a meaningful gradient.
    if (lo_t != 1u && hi_t != 1u) { return; }

    let coeff = (params.geom.z / params.phys.y) / params.geom.x; // (dt/rho)/h
    vel[idx] = vel[idx] - coeff * (pressure[hi] - pressure[lo]);
}
