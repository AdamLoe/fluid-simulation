// Diffuse-water update (v1.13). One invocation per active diffuse particle slot.
// Ages each particle, advects it by type (foam clings to the flow, spray flies
// ballistically with drag, bubbles rise by buoyancy), handles type transitions at
// the surface, clamps to the tank, and recounts alive-per-type via integer atomics.
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
    f1: vec4<f32>,  // surf_thresh, surf_gain, wall_thresh, wall_gain
    f2: vec4<f32>,  // foam_life, spray_life, bubble_life, bubble_buoyancy
    f3: vec4<f32>,  // spray_drag, _, _, _
    u0: vec4<u32>,  // frame_index, max_particles, emit_budget, random_seed
    u1: vec4<u32>,  // enabled, debug_view, _, _
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

@compute @workgroup_size(64, 1, 1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let p = gid.x;
    if (p >= du.u0.y) { return; }                 // beyond active cap → inactive slot

    let pt = particles[p * 3u + 0u];
    var ptype = pt.w;
    if (ptype < 0.0) { return; }                  // dead slot

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

    let gv = vec3<f32>(sample_u(pos), sample_v(pos), sample_w(pos));
    let grav = params.grav.xyz;
    let up = select(vec3<f32>(0.0, 1.0, 0.0), -normalize(grav), length(grav) > 1e-4);
    // Cell the particle currently sits in (before advection) — decides whether foam
    // is still on the water or has been stranded in the air.
    let here0 = cell_at_pos(pos);

    if (ptype < 0.5) {
        // Foam clings to the surface flow ONLY while it's in/at liquid. Once the
        // water drops away and it is left in an air cell, it falls back down
        // ballistically (with mild drag) instead of hanging in midair.
        if (here0 == LIQUID) {
            vel = mix(vel, gv, clamp(8.0 * dt, 0.0, 1.0));
        } else {
            vel += grav * dt;
            vel *= max(1.0 - du.f3.x * 0.5 * dt, 0.0);
        }
    } else if (ptype < 1.5) {
        // Spray: ballistic with linear air drag.
        vel += grav * dt;
        vel *= max(1.0 - du.f3.x * dt, 0.0);
    } else {
        // Bubble: rises by buoyancy, lightly dragged by the flow.
        vel = mix(vel, gv, clamp(3.0 * dt, 0.0, 1.0));
        vel += up * du.f2.w * dt;
    }

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

    // Type transitions at the interface.
    let here = cell_at_pos(pos);
    if (ptype > 1.5 && here == AIR) {
        ptype = 0.0;                  // bubble surfaced → becomes foam
    } else if (ptype > 0.5 && ptype < 1.5 && here == LIQUID) {
        ptype = 0.0;                  // spray fell back in → becomes foam
        vel *= 0.3;
    }

    // Recount alive-per-type (integer atomics; read back throttled for stats).
    let ti = u32(clamp(ptype, 0.0, 2.0));
    atomicAdd(&counters[3u + ti], 1u);

    particles[p * 3u + 0u] = vec4<f32>(pos, ptype);
    particles[p * 3u + 1u] = vec4<f32>(vel, age);
    // life (lifetime, per-particle random) is preserved.
}
