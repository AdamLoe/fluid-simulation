// Partial dot product of vecA and vecB.
// Each workgroup of 256 threads reduces one chunk of 256 elements.
// Result partial sums written to cg_partials[workgroup_id.x].

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
@group(0) @binding(1) var<storage, read> vecA: array<f32>;
@group(0) @binding(2) var<storage, read> vecB: array<f32>;
@group(0) @binding(3) var<storage, read_write> cg_partials: array<f32>;
// cg_scalars layout:
// 0 rs_old, 1 dot_scratch, 2 alpha, 3 beta, 4 rs_initial, 5 active, 6 tol_sq
@group(0) @binding(4) var<storage, read> cg_scalars: array<f32>;

var<workgroup> sdata: array<f32, 256>;

@compute @workgroup_size(256)
fn main(
    @builtin(global_invocation_id) gid: vec3<u32>,
    @builtin(local_invocation_index) li: u32,
    @builtin(workgroup_id) wid: vec3<u32>,
) {
    let is_active = cg_scalars[5] != 0.0;
    let cells = params.gdim.x * params.gdim.y * params.gdim.z;
    let idx = gid.x;
    var prod = 0.0;
    if (is_active && idx < cells) {
        prod = vecA[idx] * vecB[idx];
    }
    sdata[li] = prod;
    workgroupBarrier();

    // Tree reduction
    if (li < 128u) { sdata[li] += sdata[li + 128u]; } workgroupBarrier();
    if (li <  64u) { sdata[li] += sdata[li +  64u]; } workgroupBarrier();
    if (li <  32u) { sdata[li] += sdata[li +  32u]; } workgroupBarrier();
    if (li <  16u) { sdata[li] += sdata[li +  16u]; } workgroupBarrier();
    if (li <   8u) { sdata[li] += sdata[li +   8u]; } workgroupBarrier();
    if (li <   4u) { sdata[li] += sdata[li +   4u]; } workgroupBarrier();
    if (li <   2u) { sdata[li] += sdata[li +   2u]; } workgroupBarrier();
    if (li <   1u) { sdata[li] += sdata[li +   1u]; } workgroupBarrier();

    if (li == 0u) {
        cg_partials[wid.x] = sdata[0];
    }
}
