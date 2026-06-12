// Update search direction: d = r + beta*d
// scalars[3] = beta, scalars[5] = active

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
@group(0) @binding(1) var<storage, read> cg_scalars: array<f32>;
@group(0) @binding(2) var<storage, read> pressure_b: array<f32>;  // r
@group(0) @binding(3) var<storage, read_write> cg_d: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (cg_scalars[5] == 0.0) {
        return;
    }

    let cells = params.gdim.x * params.gdim.y * params.gdim.z;
    let c = gid.x;
    if (c >= cells) { return; }

    let bta = cg_scalars[3];
    cg_d[c] = pressure_b[c] + bta * cg_d[c];
}
