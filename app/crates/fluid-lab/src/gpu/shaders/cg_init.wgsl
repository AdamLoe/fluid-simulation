// CG init: p=0, r=b, d=b where b=-scale*div on liquid cells, 0 elsewhere.
// p0=0 => r0 = b - A*0 = b.  d0 = r0 = b.

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
@group(0) @binding(1) var<storage, read> divergence: array<f32>;
@group(0) @binding(2) var<storage, read> cell_type: array<u32>;
@group(0) @binding(3) var<storage, read_write> pressure_a: array<f32>;  // p
@group(0) @binding(4) var<storage, read_write> pressure_b: array<f32>;  // r
@group(0) @binding(5) var<storage, read_write> cg_d: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let cells = params.gdim.x * params.gdim.y * params.gdim.z;
    let c = gid.x;
    if (c >= cells) { return; }

    if (cell_type[c] == 1u) {
        let h = params.geom.x;
        let scale = params.phys.y * h * h / params.geom.z; // rho*h^2/dt
        let rc = -scale * divergence[c];
        pressure_a[c] = 0.0;
        pressure_b[c] = rc;
        cg_d[c] = rc;
    } else {
        pressure_a[c] = 0.0;
        pressure_b[c] = 0.0;
        cg_d[c] = 0.0;
    }
}
