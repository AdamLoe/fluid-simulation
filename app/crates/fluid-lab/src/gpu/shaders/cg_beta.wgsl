// Compute beta = rs_new / rs_old, then advance rs_old = rs_new.
// scalars[0] = rs_old, scalars[1] = rs_new (just written by reduce_final),
// scalars[3] = beta (output), scalars[0] updated to rs_new.

@group(0) @binding(0) var<storage, read_write> cg_scalars: array<f32>;

@compute @workgroup_size(1)
fn main() {
    let rs_old = cg_scalars[0];
    let rs_new = cg_scalars[1];
    cg_scalars[3] = select(0.0, rs_new / rs_old, rs_old > 1e-30);
    cg_scalars[0] = rs_new;
}
