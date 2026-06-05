// SpMV: q = A*d  (the pressure-Poisson operator applied to the search direction d)
// (A d)_c = n_c*d_c - sum_{liquid nb} d_nb
// where n_c = count of non-solid (liquid+air) neighbours.
// Non-liquid cells: q[c] = 0.

struct Params {
    dims: vec4<u32>,
    geom: vec4<f32>,
    phys: vec4<f32>,
    origin: vec4<f32>,
    grav: vec4<f32>,
    spc:  vec4<f32>,
    cls:  vec4<f32>,
    gdim: vec4<u32>,   // nx, ny, nz, _
};
@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> cell_type: array<u32>;
@group(0) @binding(2) var<storage, read> cg_d: array<f32>;
@group(0) @binding(3) var<storage, read_write> cg_q: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let nx = params.gdim.x;
    let ny = params.gdim.y;
    let nz = params.gdim.z;
    let cells = nx * ny * nz;
    let c = gid.x;
    if (c >= cells) { return; }

    if (cell_type[c] != 1u) {
        cg_q[c] = 0.0;
        return;
    }

    let i = c % nx;
    let j = (c / nx) % ny;
    let k = c / (nx * ny);

    var cnt = 0.0;
    var sum = 0.0;

    // -x
    let nb0 = (i - 1u) + nx * (j + ny * k);
    if (cell_type[nb0] != 0u) { cnt += 1.0; if (cell_type[nb0] == 1u) { sum += cg_d[nb0]; } }
    // +x
    let nb1 = (i + 1u) + nx * (j + ny * k);
    if (cell_type[nb1] != 0u) { cnt += 1.0; if (cell_type[nb1] == 1u) { sum += cg_d[nb1]; } }
    // -y
    let nb2 = i + nx * ((j - 1u) + ny * k);
    if (cell_type[nb2] != 0u) { cnt += 1.0; if (cell_type[nb2] == 1u) { sum += cg_d[nb2]; } }
    // +y
    let nb3 = i + nx * ((j + 1u) + ny * k);
    if (cell_type[nb3] != 0u) { cnt += 1.0; if (cell_type[nb3] == 1u) { sum += cg_d[nb3]; } }
    // -z
    let nb4 = i + nx * (j + ny * (k - 1u));
    if (cell_type[nb4] != 0u) { cnt += 1.0; if (cell_type[nb4] == 1u) { sum += cg_d[nb4]; } }
    // +z
    let nb5 = i + nx * (j + ny * (k + 1u));
    if (cell_type[nb5] != 0u) { cnt += 1.0; if (cell_type[nb5] == 1u) { sum += cg_d[nb5]; } }

    cg_q[c] = cnt * cg_d[c] - sum;
}
