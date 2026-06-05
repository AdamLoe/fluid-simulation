// Copy a face-velocity buffer (post-P2G, pre-force) into its "saved" buffer for
// the FLIP delta in G2P. Plain element copy.

struct Params {
    dims: vec4<u32>,
    geom: vec4<f32>,
    phys: vec4<f32>,
    origin: vec4<f32>,
};
@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> src: array<f32>;
@group(0) @binding(2) var<storage, read_write> dst: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (params.dims.x == 0u) { return; } // keep params binding
    let i = gid.x;
    if (i < arrayLength(&dst)) { dst[i] = src[i]; }
}
