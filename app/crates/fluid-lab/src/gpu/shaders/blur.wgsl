// Issue 4: Marching-cubes surface smoothing. One Gaussian-ish blur pass over the
// scalar density field, reading `src` and writing `dst`. The MeshExtractor runs
// this in ping-pong pairs (scalar→scalar2→scalar) `mesh_smooth` times, so the raw
// per-cell occupancy blobs round off into a smooth water surface.
//
// 7-tap separable-equivalent kernel: center weight 2, each of the 6 face
// neighbours weight 1, zero-padded at the domain boundary (so the surface relaxes
// toward the walls rather than ballooning past them).

struct MeshParams {
    isolevel: f32,
    h: f32,
    foam_scale: f32,
    _pad1: f32,
    dims: vec4<u32>,   // nx, ny, nz, _
    origin: vec4<f32>,
};

@group(0) @binding(0) var<uniform> mesh_params: MeshParams;
@group(0) @binding(1) var<storage, read> src: array<f32>;
@group(0) @binding(2) var<storage, read_write> dst: array<f32>;

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

    var sum = src[c] * 2.0;
    var weight = 2.0;

    if (i > 0u)        { sum += src[c - 1u];        weight += 1.0; } else { weight += 1.0; }
    if (i < nx - 1u)   { sum += src[c + 1u];        weight += 1.0; } else { weight += 1.0; }
    if (j > 0u)        { sum += src[c - nx];        weight += 1.0; } else { weight += 1.0; }
    if (j < ny - 1u)   { sum += src[c + nx];        weight += 1.0; } else { weight += 1.0; }
    if (k > 0u)        { sum += src[c - nx * ny];   weight += 1.0; } else { weight += 1.0; }
    if (k < nz - 1u)   { sum += src[c + nx * ny];   weight += 1.0; } else { weight += 1.0; }

    dst[c] = sum / weight;
}
