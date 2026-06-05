// Final reduction: sum cg_partials[0..num_wg] into cg_scalars[1].
// Dispatched as ONE workgroup of 256 threads.
// Each thread strides through the partials array to handle num_wg > 256 cases.

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
@group(0) @binding(1) var<storage, read> cg_partials: array<f32>;
@group(0) @binding(2) var<storage, read_write> cg_scalars: array<f32>;

var<workgroup> sdata: array<f32, 256>;

@compute @workgroup_size(256)
fn main(@builtin(local_invocation_index) li: u32) {
    let cells = params.gdim.x * params.gdim.y * params.gdim.z;
    let num_wg = (cells + 255u) / 256u;

    var acc = 0.0;
    var idx = li;
    loop {
        if (idx >= num_wg) { break; }
        acc += cg_partials[idx];
        idx += 256u;
    }
    sdata[li] = acc;
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
        cg_scalars[1] = sdata[0];
    }
}
