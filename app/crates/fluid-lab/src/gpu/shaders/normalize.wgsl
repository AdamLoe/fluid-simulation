// P2G normalize (axis-agnostic): face velocity = num/den (FIXED_SCALE cancels).
// den == 0 -> no fluid contribution -> velocity 0 (invalid face).

struct Params {
    dims: vec4<u32>,
    geom: vec4<f32>,
    phys: vec4<f32>,
    origin: vec4<f32>,
};
@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> num: array<i32>;
@group(0) @binding(2) var<storage, read> den: array<i32>;
@group(0) @binding(3) var<storage, read_write> vel: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    // Reference params so binding 0 stays in the auto-derived bind group layout.
    if (params.dims.x == 0u) { return; }
    let idx = gid.x;
    if (idx >= arrayLength(&vel)) { return; }
    let d = den[idx];
    if (d > 0) {
        vel[idx] = f32(num[idx]) / f32(d);
    } else {
        vel[idx] = 0.0;
    }
}
