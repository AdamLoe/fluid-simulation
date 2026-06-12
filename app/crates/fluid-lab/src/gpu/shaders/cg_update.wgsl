// CG update: p += alpha*d  ;  r -= alpha*q
// scalars[2] = alpha, scalars[5] = active
// Non-liquid entries of d and q are 0, so p and r remain 0 there — no liquid check needed.

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
@group(0) @binding(2) var<storage, read> cg_d: array<f32>;
@group(0) @binding(3) var<storage, read> cg_q: array<f32>;
@group(0) @binding(4) var<storage, read_write> pressure_a: array<f32>;  // p
@group(0) @binding(5) var<storage, read_write> pressure_b: array<f32>;  // r

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (cg_scalars[5] == 0.0) {
        return;
    }

    let cells = params.gdim.x * params.gdim.y * params.gdim.z;
    let c = gid.x;
    if (c >= cells) { return; }

    let a = cg_scalars[2];
    pressure_a[c] += a * cg_d[c];
    pressure_b[c] -= a * cg_q[c];
}
