// Enforce solid boundaries for one face component (AXIS 0:u,1:v,2:w):
// any face adjacent to a solid cell or the domain edge has zero normal velocity.

override AXIS: u32 = 0u;

struct Params {
    dims: vec4<u32>,
    geom: vec4<f32>,
    phys: vec4<f32>,
    origin: vec4<f32>,
    grav: vec4<f32>,
    spc:  vec4<f32>,
    cls:  vec4<f32>,
    gdim: vec4<u32>,   // nx, ny, nz, _
};
@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> cell_type: array<u32>;
@group(0) @binding(2) var<storage, read_write> vel: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let nx = params.gdim.x;
    let ny = params.gdim.y;
    let nz = params.gdim.z;
    var dim = vec3<u32>(nx, ny, nz);
    // Cell count along the stacking AXIS (face index `comp == nax` is the hi wall).
    var nax: u32;
    if (AXIS == 0u) { dim.x = nx + 1u; nax = nx; }
    else if (AXIS == 1u) { dim.y = ny + 1u; nax = ny; }
    else { dim.z = nz + 1u; nax = nz; }

    let idx = gid.x;
    if (idx >= dim.x * dim.y * dim.z) { return; }

    let i = idx % dim.x;
    let j = (idx / dim.x) % dim.y;
    let k = idx / (dim.x * dim.y);

    // Coordinate along the stacking axis, and the lo/hi cell coords.
    var comp: u32;
    if (AXIS == 0u) { comp = i; } else if (AXIS == 1u) { comp = j; } else { comp = k; }

    var lo_solid = (comp == 0u);
    var hi_solid = (comp == nax);
    if (!lo_solid) {
        var ci = i; var cj = j; var ck = k;
        if (AXIS == 0u) { ci = i - 1u; } else if (AXIS == 1u) { cj = j - 1u; } else { ck = k - 1u; }
        if (cell_type[ci + nx * (cj + ny * ck)] == 0u) { lo_solid = true; }
    }
    if (!hi_solid) {
        // hi cell coord == (i,j,k) but clamped into cell range along axis
        let ci = min(i, nx - 1u);
        let cj = min(j, ny - 1u);
        let ck = min(k, nz - 1u);
        if (cell_type[ci + nx * (cj + ny * ck)] == 0u) { hi_solid = true; }
    }

    if (lo_solid || hi_solid) {
        vel[idx] = 0.0;
    }
}
