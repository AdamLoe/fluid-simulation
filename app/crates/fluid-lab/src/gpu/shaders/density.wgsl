// Gate 1: Scalar density field from occupancy (particle count per cell) + a
// per-cell surface speed used to drive foam (issue 5: "white only where the
// water is white").
//
// One invocation per cell. Reads occupancy (i32) and the staggered MAC face
// velocities, writes:
//   scalar[c] — a light 6-face blurred density (further smoothed by blur.wgsl)
//   speed[c]  — |cell-centered velocity| (avg of the two opposing faces / axis)
//
// Cell index: c = i + nx*(j + ny*k),  i in 0..nx, j in 0..ny, k in 0..nz
// Boundary cells (i/j/k == 0 or n-1) are Solid; we skip reading OOB neighbours
// so they just get 0 density (no surface there anyway).

struct MeshParams {
    isolevel: f32,
    h: f32,
    foam_scale: f32,
    _pad1: f32,
    dims: vec4<u32>,   // nx, ny, nz, _
    origin: vec4<f32>,
};

@group(0) @binding(0) var<uniform> mesh_params: MeshParams;
// occupancy is stored as atomic<u32> in mark.wgsl but exposed as plain array<i32>
// for read-only access here. The WGSL type must match the buffer layout.
@group(0) @binding(1) var<storage, read> occupancy: array<i32>;
// Staggered MAC face velocities (read-only) for the foam speed field.
@group(0) @binding(2) var<storage, read> u_vel: array<f32>; // (nx+1)*ny*nz
@group(0) @binding(3) var<storage, read> v_vel: array<f32>; // nx*(ny+1)*nz
@group(0) @binding(4) var<storage, read> w_vel: array<f32>; // nx*ny*(nz+1)
@group(0) @binding(5) var<storage, read_write> scalar: array<f32>;
@group(0) @binding(6) var<storage, read_write> speed: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let c = gid.x;
    let nx = mesh_params.dims.x;
    let ny = mesh_params.dims.y;
    let nz = mesh_params.dims.z;
    let cell_count = nx * ny * nz;
    if (c >= cell_count) { return; }

    let i = c % nx;
    let j = (c / nx) % ny;
    let k = c / (nx * ny);

    // Read center
    let center_val = f32(max(occupancy[c], 0));

    // Blur: sum neighbors, skip if at boundary (treat outside as 0)
    var sum = center_val * 2.0;
    var weight = 2.0;

    // -x neighbor
    if (i > 0u) {
        sum += f32(max(occupancy[c - 1u], 0));
        weight += 1.0;
    } else {
        weight += 1.0; // add weight but not value (zero pad)
    }
    // +x neighbor
    if (i < nx - 1u) {
        sum += f32(max(occupancy[c + 1u], 0));
        weight += 1.0;
    } else {
        weight += 1.0;
    }
    // -y neighbor
    if (j > 0u) {
        sum += f32(max(occupancy[c - nx], 0));
        weight += 1.0;
    } else {
        weight += 1.0;
    }
    // +y neighbor
    if (j < ny - 1u) {
        sum += f32(max(occupancy[c + nx], 0));
        weight += 1.0;
    } else {
        weight += 1.0;
    }
    // -z neighbor
    if (k > 0u) {
        sum += f32(max(occupancy[c - nx * ny], 0));
        weight += 1.0;
    } else {
        weight += 1.0;
    }
    // +z neighbor
    if (k < nz - 1u) {
        sum += f32(max(occupancy[c + nx * ny], 0));
        weight += 1.0;
    } else {
        weight += 1.0;
    }

    scalar[c] = sum / weight;

    // ── Cell-centered velocity magnitude → foam ──────────────────────────────
    // Average the two opposing staggered faces on each axis.
    let u_lo = i + (nx + 1u) * (j + ny * k);
    let u_hi = (i + 1u) + (nx + 1u) * (j + ny * k);
    let v_lo = i + nx * (j + (ny + 1u) * k);
    let v_hi = i + nx * ((j + 1u) + (ny + 1u) * k);
    let w_lo = i + nx * (j + ny * k);
    let w_hi = i + nx * (j + ny * (k + 1u));
    let vx = 0.5 * (u_vel[u_lo] + u_vel[u_hi]);
    let vy = 0.5 * (v_vel[v_lo] + v_vel[v_hi]);
    let vz = 0.5 * (w_vel[w_lo] + w_vel[w_hi]);
    speed[c] = length(vec3<f32>(vx, vy, vz));
}
