// Body forces: gravity on faces of one staggered axis adjacent to at least one
// liquid cell. Parameterised by AXIS (0=u, 1=v, 2=w), mirroring boundaries.wgsl.
//
// Params layout (8 vec4 = 128 bytes):
//   dims   : vec4<u32>  — nx (legacy "n"), particle_count, pressure_iters, _
//   geom   : vec4<f32>  — h, inv_h, dt, fixed_scale
//   phys   : vec4<f32>  — gravity_y (legacy), rho, flip_blend, _
//   origin : vec4<f32>  — ox, oy, oz, _
//   grav   : vec4<f32>  — gx, gy, gz, _  (3-axis gravity vector)
//   spc    : vec4<f32>  — spacing params
//   cls    : vec4<f32>  — classify params
//   gdim   : vec4<u32>  — nx, ny, nz, _  (appended; byte offset 112)

override AXIS: u32 = 0u;

struct Params {
    dims:   vec4<u32>,
    geom:   vec4<f32>,
    phys:   vec4<f32>,
    origin: vec4<f32>,
    grav:   vec4<f32>,
    spc:    vec4<f32>,
    cls:    vec4<f32>,
    gdim:   vec4<u32>,   // nx, ny, nz, _
};
@group(0) @binding(0) var<uniform>  params:    Params;
@group(0) @binding(1) var<storage, read_write> vel: array<f32>;
@group(0) @binding(2) var<storage, read>       cell_type: array<u32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let nx = params.gdim.x;
    let ny = params.gdim.y;
    let nz = params.gdim.z;

    // Staggered dims: the axis component gets n+1 faces. nax = cell count on AXIS.
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

    // Coordinate along the stacking axis.
    var comp: u32;
    if (AXIS == 0u) { comp = i; }
    else if (AXIS == 1u) { comp = j; }
    else { comp = k; }

    // A face between lo cell (comp-1) and hi cell (comp).
    // The face is "wet" if either adjacent in-range cell is Liquid (cell_type==1).
    var liquid = false;

    // lo cell: skip when comp==0 (out of domain)
    if (comp > 0u) {
        var ci = i; var cj = j; var ck = k;
        if (AXIS == 0u) { ci = i - 1u; }
        else if (AXIS == 1u) { cj = j - 1u; }
        else { ck = k - 1u; }
        if (cell_type[ci + nx * (cj + ny * ck)] == 1u) { liquid = true; }
    }

    // hi cell: skip when comp==nax (out of domain)
    if (comp < nax) {
        // (i,j,k) is already the hi cell index along the axis
        if (cell_type[i + nx * (j + ny * k)] == 1u) { liquid = true; }
    }

    if (liquid) {
        // Pick the gravity component for this axis and accumulate g*dt.
        var g_axis: f32;
        if (AXIS == 0u) { g_axis = params.grav.x; }
        else if (AXIS == 1u) { g_axis = params.grav.y; }
        else { g_axis = params.grav.z; }

        vel[idx] = vel[idx] + g_axis * params.geom.z;
    }
}
