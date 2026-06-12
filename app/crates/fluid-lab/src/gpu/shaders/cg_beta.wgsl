// Compute beta = rs_new / rs_old, then advance rs_old = rs_new.
// scalars[0] = rs_old, scalars[1] = rs_new (just written by reduce_final),
// scalars[3] = beta (output), scalars[0] updated to rs_new.
// Full layout:
// 0 rs_old, 1 dot_scratch, 2 alpha, 3 beta, 4 rs_initial, 5 active, 6 tol_sq

@group(0) @binding(0) var<storage, read_write> cg_scalars: array<f32>;

@compute @workgroup_size(1)
fn main() {
    if (cg_scalars[5] == 0.0) {
        cg_scalars[3] = 0.0;
        return;
    }

    let rs_old = cg_scalars[0];
    let rs_new = cg_scalars[1];
    if (rs_old > 1e-30) {
        cg_scalars[3] = rs_new / rs_old;
    } else {
        cg_scalars[3] = 0.0;
    }
    cg_scalars[0] = rs_new;

    let tol_sq = cg_scalars[6];
    let rs_initial = cg_scalars[4];
    if (tol_sq > 0.0 && rs_new <= tol_sq * rs_initial) {
        cg_scalars[5] = 0.0;
        cg_scalars[3] = 0.0;
    }
}
