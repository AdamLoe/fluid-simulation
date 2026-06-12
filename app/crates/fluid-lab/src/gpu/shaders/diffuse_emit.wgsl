// Surface-foam emission. One invocation per grid cell. Liquid cells touching air
// at sufficient speed stochastically spawn a persistent foam particle into a
// shared ring buffer.
//
// Render-only: this never touches the sim particle buffer, conserves no mass, and
// affects no pressure. Spawning is deterministic per (cell, frame, seed) via an
// integer hash — no wall-clock randomness — and bounded per frame by an integer
// atomic budget (no float atomics, no readback), matching the scatter.wgsl rule.

struct Params {
    dims: vec4<u32>,
    geom: vec4<f32>,   // h, inv_h, dt, fixed_scale
    phys: vec4<f32>,
    origin: vec4<f32>,
    grav: vec4<f32>,
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
// pos_type (xyz pos, w = type: 0 foam; <0 dead),
// vel_age (xyz vel, w = age), life (x = lifetime, y = per-particle random, zw _).
@group(0) @binding(6) var<storage, read_write> particles: array<vec4<f32>>;
// 0 ring cursor (persistent), 1 emitted this frame, 2 clamped this frame,
// 3 alive foam; 4/5 are legacy-zero spray/bubble counters.
@group(0) @binding(7) var<storage, read_write> counters: array<atomic<u32>>;

const LIQUID: u32 = 1u;
const AIR: u32 = 2u;
const SOLID: u32 = 0u;

fn hash_u32(x: u32) -> u32 {
    var h = x;
    h ^= h >> 16u;
    h *= 0x7feb352du;
    h ^= h >> 15u;
    h *= 0x846ca68bu;
    h ^= h >> 16u;
    return h;
}
fn hash01(seed: u32) -> f32 {
    return f32(hash_u32(seed) & 0xffffffu) / f32(0x1000000u);
}

// Reuse the wall-aware-free trilinear MAC sample (final velocity only). Bounds are
// guarded; near-wall faces are included (cell-center taps are interior).
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

fn cell_at(i: i32, j: i32, k: i32) -> u32 {
    let nx = i32(params.gdim.x); let ny = i32(params.gdim.y); let nz = i32(params.gdim.z);
    if (i < 0 || j < 0 || k < 0 || i >= nx || j >= ny || k >= nz) { return SOLID; }
    return cell_type[i + nx * (j + ny * k)];
}

@compute @workgroup_size(64, 1, 1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let nx = params.gdim.x; let ny = params.gdim.y; let nz = params.gdim.z;
    let total = nx * ny * nz;
    let c = gid.x;
    if (c >= total) { return; }
    if (du.u1.x == 0u) { return; }            // disabled
    if (cell_type[c] != LIQUID) { return; }

    let i = i32(c % nx);
    let j = i32((c / nx) % ny);
    let k = i32(c / (nx * ny));
    let h = params.geom.x;

    // Cell-center velocity from the live MAC faces.
    let center = params.origin.xyz + (vec3<f32>(f32(i), f32(j), f32(k)) + 0.5) * h;
    let vel = vec3<f32>(sample_u(center), sample_v(center), sample_w(center));
    let spd = length(vel);

    // Surface = a liquid cell touching Air. Foam is not born from interior flow,
    // bubbles, ballistic spray, or hard wall impacts.
    var is_surface = false;
    if (cell_at(i + 1, j, k) == AIR || cell_at(i - 1, j, k) == AIR ||
        cell_at(i, j + 1, k) == AIR || cell_at(i, j - 1, k) == AIR ||
        cell_at(i, j, k + 1) == AIR || cell_at(i, j, k - 1) == AIR) { is_surface = true; }
    if (!is_surface) { return; }

    let surf_thresh = du.f1.x; let surf_gain = du.f1.y;

    var p = max(spd - surf_thresh, 0.0) * surf_gain;
    p = p * du.f0.y * du.f0.x;        // * emit_rate * dt
    if (p <= 0.0) { return; }

    let seed = c * 0x9e3779b1u + du.u0.x * 0x85ebca6bu + du.u0.w * 0x27d4eb2fu;
    if (hash01(seed) >= clamp(p, 0.0, 1.0)) { return; }

    // Per-frame budget (integer atomic). Over budget → count and bail.
    let n = atomicAdd(&counters[1], 1u);
    if (n >= du.u0.z) { atomicAdd(&counters[2], 1u); return; }

    let max_p = max(du.u0.y, 1u);
    let slot = atomicAdd(&counters[0], 1u) % max_p;

    // Spawn. Jitter within the cell; velocity is damped toward the local flow so
    // foam stays on the liquid surface.
    let jx = (hash01(seed ^ 0x1111u) - 0.5);
    let jy = (hash01(seed ^ 0x2222u) - 0.5);
    let jz = (hash01(seed ^ 0x3333u) - 0.5);
    let pos = center + vec3<f32>(jx, jy, jz) * h;
    let rlife = 0.4 + 0.6 * hash01(seed ^ 0x4444u);

    let pvel = vel * 0.55;
    let life = du.f2.x;

    particles[slot * 3u + 0u] = vec4<f32>(pos, 0.0);
    particles[slot * 3u + 1u] = vec4<f32>(pvel, 0.0);
    particles[slot * 3u + 2u] = vec4<f32>(life * rlife, hash01(seed ^ 0x5555u), 0.0, 0.0);
}
