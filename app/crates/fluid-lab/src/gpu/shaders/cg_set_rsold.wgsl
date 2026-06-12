// Copy initial dot(r,r) from scalars[1] into rs_old slot scalars[0].
// Called once after the initial reduce(r,r) before the CG loop begins.
// Full layout:
// 0 rs_old, 1 dot_scratch, 2 alpha, 3 beta, 4 rs_initial, 5 active, 6 tol_sq

@group(0) @binding(0) var<storage, read_write> cg_scalars: array<f32>;

@compute @workgroup_size(1)
fn main() {
    cg_scalars[0] = cg_scalars[1];
    cg_scalars[4] = cg_scalars[1];
    cg_scalars[5] = 1.0;
}
