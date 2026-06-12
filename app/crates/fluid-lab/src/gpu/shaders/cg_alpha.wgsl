// Compute alpha = rs_old / dot(d,q).
// scalars[0] = rs_old, scalars[1] = dot(d,q) (just written by reduce_final),
// scalars[2] = alpha (output), scalars[5] = active.
// Guard against division by near-zero.
// Full layout:
// 0 rs_old, 1 dot_scratch, 2 alpha, 3 beta, 4 rs_initial, 5 active, 6 tol_sq

@group(0) @binding(0) var<storage, read_write> cg_scalars: array<f32>;

@compute @workgroup_size(1)
fn main() {
    if (cg_scalars[5] == 0.0) {
        cg_scalars[2] = 0.0;
        return;
    }

    let dq = cg_scalars[1];
    if (abs(dq) > 1e-30) {
        cg_scalars[2] = cg_scalars[0] / dq;
    } else {
        cg_scalars[2] = 0.0;
    }
}
