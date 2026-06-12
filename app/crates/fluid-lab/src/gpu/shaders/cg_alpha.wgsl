// Compute alpha = rs_old / dot(d,q).
// scalars[0] = rs_old, scalars[1] = dot(d,q) (just written by reduce_final),
// scalars[2] = alpha (output).
// Guard against division by near-zero.

@group(0) @binding(0) var<storage, read_write> cg_scalars: array<f32>;

@compute @workgroup_size(1)
fn main() {
    let dq = cg_scalars[1];
    if (abs(dq) > 1e-30) {
        cg_scalars[2] = cg_scalars[0] / dq;
    } else {
        cg_scalars[2] = 0.0;
    }
}
