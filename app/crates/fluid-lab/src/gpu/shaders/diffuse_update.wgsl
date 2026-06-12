// Surface-foam update. One invocation per active foam particle slot. Ages each
// particle, advects it with local MAC flow while it remains on a liquid-air
// surface, kills stranded/wall-hugging particles, and recounts alive foam.
// Render-only; reads the live sim buffers but never writes them.

struct Params {
    dims: vec4<u32>,
    geom: vec4<f32>,   // h, inv_h, dt, fixed_scale
    phys: vec4<f32>,
    origin: vec4<f32>,
    grav: vec4<f32>,   // gx, gy, gz, _ (local-frame gravity)
    spc:  vec4<f32>,
    cls:  vec4<f32>,
    gdim: vec4<u32>,   // nx, ny, nz, _
};

struct DiffuseU {
    f0: vec4<f32>,  // dt, emit_rate, radius, alpha
    f1: vec4<f32>,  // surf_thresh, surf_gain, _, _
    f2: vec4<f32>,  // foam_life, _, _, _
    f3: vec4<f32>,  // _, _, _, _
    u0: vec4<u32>,  // frame_index, max_particles, emit_budget, random_seed
    u1: vec4<u32>,  // enabled, _, _, _
};

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<uniform> du: DiffuseU;
@group(0) @binding(2) var<storage, read> cell_type: array<u32>;
@group(0) @binding(3) var<storage, read> u_vel: array<f32>;
@group(0) @binding(4) var<storage, read> v_vel: array<f32>;
@group(0) @binding(5) var<storage, read> w_vel: array<f32>;
@group(0) @binding(6) var<storage, read_write> particles: array<vec4<f32>>;
@group(0) @binding(7) var<storage, read_write> counters: array<atomic<u32>>;

const LIQUID: u32 = 1u;
const AIR: u32 = 2u;
const SOLID: u32 = 0u;

fn sample_u(P: vec3<f32>) -> f32 {
    let dim = vec3<i32>(i32(params.gdim.x) + 1, i32(params.gdim.y), i32(params.gdim.z));
    let g = (P - params.origin.xyz) * params.geom.y + vec3<f32>(0.0, -0.5, -0.5);
    let base = vec3<i32>(floor(g));
    let t = g - vec3<f32>(base);
    var acc = 0.0; var wsum = 0.0;
    for (var dk = 0; dk < 2; dk++) {
        let kk = base.z + dk; if (kk < 0 || kk >= dim.z) { continue; }
        let wz = select(1.0 - t.z, t.z, dk == 1);
        for (var dj = 0; dj < 2; dj++) {
            let jj = base.y + dj; if (jj < 0 || jj >= dim.y) { continue; }
            let wy = select(1.0 - t.y, t.y, dj == 1);
            for (var di = 0; di < 2; di++) {
                let ii = base.x + di; if (ii < 0 || ii >= dim.x) { continue; }
                let wx = select(1.0 - t.x, t.x, di == 1);
                let w = wx * wy * wz;
                acc += w * u_vel[ii + dim.x * (jj + dim.y * kk)]; wsum += w;
            }
        }
    }
    return select(0.0, acc / wsum, wsum > 0.0);
}
fn sample_v(P: vec3<f32>) -> f32 {
    let dim = vec3<i32>(i32(params.gdim.x), i32(params.gdim.y) + 1, i32(params.gdim.z));
    let g = (P - params.origin.xyz) * params.geom.y + vec3<f32>(-0.5, 0.0, -0.5);
    let base = vec3<i32>(floor(g));
    let t = g - vec3<f32>(base);
    var acc = 0.0; var wsum = 0.0;
    for (var dk = 0; dk < 2; dk++) {
        let kk = base.z + dk; if (kk < 0 || kk >= dim.z) { continue; }
        let wz = select(1.0 - t.z, t.z, dk == 1);
        for (var dj = 0; dj < 2; dj++) {
            let jj = base.y + dj; if (jj < 0 || jj >= dim.y) { continue; }
            let wy = select(1.0 - t.y, t.y, dj == 1);
            for (var di = 0; di < 2; di++) {
                let ii = base.x + di; if (ii < 0 || ii >= dim.x) { continue; }
                let wx = select(1.0 - t.x, t.x, di == 1);
                let w = wx * wy * wz;
                acc += w * v_vel[ii + dim.x * (jj + dim.y * kk)]; wsum += w;
            }
        }
    }
    return select(0.0, acc / wsum, wsum > 0.0);
}
fn sample_w(P: vec3<f32>) -> f32 {
    let dim = vec3<i32>(i32(params.gdim.x), i32(params.gdim.y), i32(params.gdim.z) + 1);
    let g = (P - params.origin.xyz) * params.geom.y + vec3<f32>(-0.5, -0.5, 0.0);
    let base = vec3<i32>(floor(g));
    let t = g - vec3<f32>(base);
    var acc = 0.0; var wsum = 0.0;
    for (var dk = 0; dk < 2; dk++) {
        let kk = base.z + dk; if (kk < 0 || kk >= dim.z) { continue; }
        let wz = select(1.0 - t.z, t.z, dk == 1);
        for (var dj = 0; dj < 2; dj++) {
            let jj = base.y + dj; if (jj < 0 || jj >= dim.y) { continue; }
            let wy = select(1.0 - t.y, t.y, dj == 1);
            for (var di = 0; di < 2; di++) {
                let ii = base.x + di; if (ii < 0 || ii >= dim.x) { continue; }
                let wx = select(1.0 - t.x, t.x, di == 1);
                let w = wx * wy * wz;
                acc += w * w_vel[ii + dim.x * (jj + dim.y * kk)]; wsum += w;
            }
        }
    }
    return select(0.0, acc / wsum, wsum > 0.0);
}

fn cell_at_pos(P: vec3<f32>) -> u32 {
    let nx = i32(params.gdim.x); let ny = i32(params.gdim.y); let nz = i32(params.gdim.z);
    let g = (P - params.origin.xyz) * params.geom.y;
    let i = clamp(i32(floor(g.x)), 0, nx - 1);
    let j = clamp(i32(floor(g.y)), 0, ny - 1);
    let k = clamp(i32(floor(g.z)), 0, nz - 1);
    return cell_type[i + nx * (j + ny * k)];
}

fn cell_at(i: i32, j: i32, k: i32) -> u32 {
    let nx = i32(params.gdim.x); let ny = i32(params.gdim.y); let nz = i32(params.gdim.z);
    if (i < 0 || j < 0 || k < 0 || i >= nx || j >= ny || k >= nz) { return SOLID; }
    return cell_type[i + nx * (j + ny * k)];
}

fn surface_at_pos(P: vec3<f32>) -> bool {
    let nx = i32(params.gdim.x); let ny = i32(params.gdim.y); let nz = i32(params.gdim.z);
    let g = (P - params.origin.xyz) * params.geom.y;
    let i = clamp(i32(floor(g.x)), 0, nx - 1);
    let j = clamp(i32(floor(g.y)), 0, ny - 1);
    let k = clamp(i32(floor(g.z)), 0, nz - 1);
    if (cell_at(i, j, k) != LIQUID) { return false; }
    return cell_at(i + 1, j, k) == AIR || cell_at(i - 1, j, k) == AIR ||
           cell_at(i, j + 1, k) == AIR || cell_at(i, j - 1, k) == AIR ||
           cell_at(i, j, k + 1) == AIR || cell_at(i, j, k - 1) == AIR;
}

@compute @workgroup_size(64, 1, 1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let p = gid.x;
    if (p >= du.u0.y) { return; }                 // beyond active cap → inactive slot

    let pt = particles[p * 3u + 0u];
    if (pt.w < 0.0) { return; }                   // dead slot

    let dt = du.f0.x;
    var pos = pt.xyz;
    var vel = particles[p * 3u + 1u].xyz;
    let life = particles[p * 3u + 2u];
    var age = particles[p * 3u + 1u].w + dt;

    // Lifetime kill.
    if (age >= life.x) {
        particles[p * 3u + 0u].w = -1.0;
        return;
    }

    if (!surface_at_pos(pos)) {
        particles[p * 3u + 0u].w = -1.0;
        return;
    }

    let gv = vec3<f32>(sample_u(pos), sample_v(pos), sample_w(pos));
    vel = mix(vel, gv, clamp(8.0 * dt, 0.0, 1.0));
    pos += vel * dt;

    // Clamp to the tank (matches the sim's wall-contact convention).
    let h = params.geom.x;
    let extent = vec3<f32>(f32(params.gdim.x), f32(params.gdim.y), f32(params.gdim.z)) * h;
    let lo = params.origin.xyz + vec3<f32>(h * 1.05);
    let hi = params.origin.xyz + extent - vec3<f32>(h * 1.05);
    if (pos.x < lo.x) { pos.x = lo.x; vel.x = 0.0; }
    if (pos.x > hi.x) { pos.x = hi.x; vel.x = 0.0; }
    if (pos.y < lo.y) { pos.y = lo.y; vel.y = 0.0; }
    if (pos.y > hi.y) { pos.y = hi.y; vel.y = 0.0; }
    if (pos.z < lo.z) { pos.z = lo.z; vel.z = 0.0; }
    if (pos.z > hi.z) { pos.z = hi.z; vel.z = 0.0; }

    // Kill foam that has been carried into the exposed vertical-wall band.
    let near_vertical_wall =
        pos.x < lo.x + 6.0 * h || pos.x > hi.x - 6.0 * h ||
        pos.z < lo.z + 6.0 * h || pos.z > hi.z - 6.0 * h;
    let above_floor_band = pos.y > lo.y + 1.25 * h;
    if (near_vertical_wall && above_floor_band) {
        particles[p * 3u + 0u].w = -1.0;
        return;
    }
    if (!surface_at_pos(pos)) {
        particles[p * 3u + 0u].w = -1.0;
        return;
    }

    // Recount alive foam (integer atomic; read back throttled for stats).
    atomicAdd(&counters[3u], 1u);

    particles[p * 3u + 0u] = vec4<f32>(pos, 0.0);
    particles[p * 3u + 1u] = vec4<f32>(vel, age);
    // life (lifetime, per-particle random) is preserved.
}
