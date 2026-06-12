// CG init: either zero-starts p, or warm-starts from the previous pressure.
// r = b - A*p_initial, d = r. Non-liquid pressure is always cleaned to 0.

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
@group(0) @binding(3) var<storage, read_write> pressure_a: array<f32>;  // p
@group(0) @binding(4) var<storage, read_write> pressure_b: array<f32>;  // r
@group(0) @binding(5) var<storage, read_write> cg_d: array<f32>;
// cg_scalars layout:
// 0 rs_old, 1 dot_scratch, 2 alpha, 3 beta, 4 rs_initial, 5 active, 6 tol_sq
@group(0) @binding(6) var<storage, read_write> cg_scalars: array<f32>;

fn apply_pressure(c: u32, p_center: f32) -> f32 {
    let nx = params.gdim.x;
    let ny = params.gdim.y;
    let i = c % nx;
    let j = (c / nx) % ny;
    let k = c / (nx * ny);

    var cnt = 0.0;
    var sum = 0.0;

    let nb0 = (i - 1u) + nx * (j + ny * k);
    if (cell_type[nb0] != 0u) { cnt += 1.0; if (cell_type[nb0] == 1u) { sum += pressure_a[nb0]; } }
    let nb1 = (i + 1u) + nx * (j + ny * k);
    if (cell_type[nb1] != 0u) { cnt += 1.0; if (cell_type[nb1] == 1u) { sum += pressure_a[nb1]; } }
    let nb2 = i + nx * ((j - 1u) + ny * k);
    if (cell_type[nb2] != 0u) { cnt += 1.0; if (cell_type[nb2] == 1u) { sum += pressure_a[nb2]; } }
    let nb3 = i + nx * ((j + 1u) + ny * k);
    if (cell_type[nb3] != 0u) { cnt += 1.0; if (cell_type[nb3] == 1u) { sum += pressure_a[nb3]; } }
    let nb4 = i + nx * (j + ny * (k - 1u));
    if (cell_type[nb4] != 0u) { cnt += 1.0; if (cell_type[nb4] == 1u) { sum += pressure_a[nb4]; } }
    let nb5 = i + nx * (j + ny * (k + 1u));
    if (cell_type[nb5] != 0u) { cnt += 1.0; if (cell_type[nb5] == 1u) { sum += pressure_a[nb5]; } }

    return cnt * p_center - sum;
}

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let cells = params.gdim.x * params.gdim.y * params.gdim.z;
    let c = gid.x;

    if (c == 0u) {
        cg_scalars[0] = 0.0;
        cg_scalars[1] = 0.0;
        cg_scalars[2] = 0.0;
        cg_scalars[3] = 0.0;
        cg_scalars[4] = 0.0;
        cg_scalars[5] = 1.0;
    }

    if (c >= cells) { return; }

    if (cell_type[c] == 1u) {
        let h = params.geom.x;
        let scale = params.phys.y * h * h / params.geom.z; // rho*h^2/dt
        let rhs = -scale * divergence[c];
        let warm_start = params.dims.w != 0u;
        var p_initial = 0.0;
        var ap = 0.0;
        if (warm_start) {
            p_initial = pressure_a[c];
            ap = apply_pressure(c, p_initial);
        } else {
            pressure_a[c] = 0.0;
        }
        let rc = rhs - ap;
        pressure_b[c] = rc;
        cg_d[c] = rc;
    } else {
        pressure_a[c] = 0.0;
        pressure_b[c] = 0.0;
        cg_d[c] = 0.0;
    }
}
