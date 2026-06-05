// Cell typing: boundary -> Solid(0); interior occupied -> Liquid(1); else Air(2).
// Matches host CellType repr (Solid=0, Liquid=1, Air=2).

struct Params {
    dims: vec4<u32>,
    geom: vec4<f32>,
    phys: vec4<f32>,
    origin: vec4<f32>,
    grav: vec4<f32>,
    spc:  vec4<f32>,
    cls:  vec4<f32>,   // liquid_threshold, surface_dilation, _, _
    gdim: vec4<u32>,   // nx, ny, nz, _
};
@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> occ: array<u32>;
@group(0) @binding(2) var<storage, read_write> cell_type: array<u32>;
// stats[0] = liquid cell count (liveness counter; cleared each step).
@group(0) @binding(3) var<storage, read_write> stats: array<atomic<u32>>;

// True if interior cell `c` holds at least `thresh` particles.
fn is_filled(c: u32, thresh: u32) -> bool {
    return occ[c] >= thresh;
}

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let nx = params.gdim.x;
    let ny = params.gdim.y;
    let nz = params.gdim.z;
    let c = gid.x;
    if (c >= nx * ny * nz) { return; }
    let i = c % nx;
    let j = (c / nx) % ny;
    let k = c / (nx * ny);
    let boundary = (i == 0u || j == 0u || k == 0u || i == nx - 1u || j == ny - 1u || k == nz - 1u);

    if (boundary) {
        cell_type[c] = 0u; // Solid
        return;
    }

    let thresh = max(u32(params.cls.x), 1u);
    var liquid = is_filled(c, thresh);

    // Surface dilation: an interior cell adjacent (6-neighbour) to a filled cell
    // also becomes liquid, sealing thin sheets/pinholes. occ is read-only here so
    // reading neighbours is race-free (one pass, no in-place feedback).
    if (!liquid && u32(params.cls.y) >= 1u) {
        liquid =
            is_filled(c - 1u,       thresh) || is_filled(c + 1u,       thresh) ||
            is_filled(c - nx,       thresh) || is_filled(c + nx,       thresh) ||
            is_filled(c - nx * ny,  thresh) || is_filled(c + nx * ny,  thresh);
    }

    if (liquid) {
        cell_type[c] = 1u; // Liquid
        atomicAdd(&stats[0], 1u);
    } else {
        cell_type[c] = 2u; // Air
    }
}
