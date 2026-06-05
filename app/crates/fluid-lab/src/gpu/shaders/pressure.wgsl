// Red-Black Gauss-Seidel pressure solver (in-place, single buffer).
// Cells are colored by parity (i+j+k)&1: red=0, black=1.
// Each dispatch updates only cells of one color; neighbors are the other color
// so all reads/writes within a phase are race-free.
// Formula identical to Jacobi: p[c] = (Σ p[nb_nonsolid] - scale*div) / n_nonsolid
// scale = rho*h^2/dt.  Solid -> Neumann (excluded).  Air -> Dirichlet p=0.
// Non-liquid cells are NOT written (pre-zeroed by the clear pass each step).

override PHASE: f32 = 0.0; // 0.0 = red, 1.0 = black

struct Params {
    dims: vec4<u32>,
    geom: vec4<f32>,   // h, inv_h, dt, fixed_scale
    phys: vec4<f32>,   // gravity_y, rho, flip_blend, _
    origin: vec4<f32>,
    grav: vec4<f32>,
    spc:  vec4<f32>,
    cls:  vec4<f32>,
    gdim: vec4<u32>,   // nx, ny, nz, _
};
@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> divergence: array<f32>;
@group(0) @binding(2) var<storage, read> cell_type: array<u32>;
@group(0) @binding(3) var<storage, read_write> pressure: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let phase = u32(PHASE);
    let nx = params.gdim.x;
    let ny = params.gdim.y;
    let nz = params.gdim.z;
    let c = gid.x;
    if (c >= nx * ny * nz) { return; }
    if (cell_type[c] != 1u) { return; } // non-liquid: do not write (already 0)

    let i = c % nx;
    let j = (c / nx) % ny;
    let k = c / (nx * ny);

    if (((i + j + k) & 1u) != phase) { return; } // wrong color this phase

    let h = params.geom.x;
    let scale = params.phys.y * h * h / params.geom.z; // rho*h^2/dt

    var sum = 0.0;
    var cnt = 0.0;
    var nb: u32;

    nb = (i - 1u) + nx * (j + ny * k); if (cell_type[nb] != 0u) { cnt += 1.0; if (cell_type[nb] == 1u) { sum += pressure[nb]; } }
    nb = (i + 1u) + nx * (j + ny * k); if (cell_type[nb] != 0u) { cnt += 1.0; if (cell_type[nb] == 1u) { sum += pressure[nb]; } }
    nb = i + nx * ((j - 1u) + ny * k); if (cell_type[nb] != 0u) { cnt += 1.0; if (cell_type[nb] == 1u) { sum += pressure[nb]; } }
    nb = i + nx * ((j + 1u) + ny * k); if (cell_type[nb] != 0u) { cnt += 1.0; if (cell_type[nb] == 1u) { sum += pressure[nb]; } }
    nb = i + nx * (j + ny * (k - 1u)); if (cell_type[nb] != 0u) { cnt += 1.0; if (cell_type[nb] == 1u) { sum += pressure[nb]; } }
    nb = i + nx * (j + ny * (k + 1u)); if (cell_type[nb] != 0u) { cnt += 1.0; if (cell_type[nb] == 1u) { sum += pressure[nb]; } }

    if (cnt > 0.0) {
        pressure[c] = (sum - scale * divergence[c]) / cnt;
    } else {
        pressure[c] = 0.0;
    }
}
