// Grid-to-particle with PIC/FLIP blend, advect (RK1), recover.
//   PIC  = interp(v_final)
//   FLIP = particle.vel + interp(v_final - v_saved)
//   v_new = mix(PIC, FLIP, flip_blend)
// Each sample_* returns vec2(final, saved) from one trilinear gather (so the pass
// stays within 8 storage buffers). v_saved is the post-P2G, pre-force grid velocity.

struct Particle { pos: vec4<f32>, vel: vec4<f32> };

struct Params {
    dims: vec4<u32>,   // nx, particle_count, ...
    geom: vec4<f32>,   // h, inv_h, dt, fixed_scale
    phys: vec4<f32>,   // gravity_y, rho, flip_blend, _
    origin: vec4<f32>,
    grav: vec4<f32>,
    spc:  vec4<f32>,
    cls:  vec4<f32>,
    gdim: vec4<u32>,   // nx, ny, nz, _
};
@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read_write> particles: array<Particle>;
@group(0) @binding(2) var<storage, read> u_vel: array<f32>;
@group(0) @binding(3) var<storage, read> v_vel: array<f32>;
@group(0) @binding(4) var<storage, read> w_vel: array<f32>;
@group(0) @binding(5) var<storage, read> u_saved: array<f32>;
@group(0) @binding(6) var<storage, read> v_saved: array<f32>;
@group(0) @binding(7) var<storage, read> w_saved: array<f32>;

fn sample_u(P: vec3<f32>) -> vec2<f32> {
    let dim = vec3<i32>(i32(params.gdim.x) + 1, i32(params.gdim.y), i32(params.gdim.z));
    let g = (P - params.origin.xyz) / params.geom.x + vec3<f32>(0.0, -0.5, -0.5);
    let base = vec3<i32>(floor(g));
    let t = g - vec3<f32>(base);
    var fin = 0.0; var sav = 0.0; var wsum = 0.0;
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
                let idx = ii + dim.x * (jj + dim.y * kk);
                fin += w * u_vel[idx]; sav += w * u_saved[idx]; wsum += w;
            }
        }
    }
    return select(vec2<f32>(0.0), vec2<f32>(fin, sav) / wsum, wsum > 0.0);
}

fn sample_v(P: vec3<f32>) -> vec2<f32> {
    let dim = vec3<i32>(i32(params.gdim.x), i32(params.gdim.y) + 1, i32(params.gdim.z));
    let g = (P - params.origin.xyz) / params.geom.x + vec3<f32>(-0.5, 0.0, -0.5);
    let base = vec3<i32>(floor(g));
    let t = g - vec3<f32>(base);
    var fin = 0.0; var sav = 0.0; var wsum = 0.0;
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
                let idx = ii + dim.x * (jj + dim.y * kk);
                fin += w * v_vel[idx]; sav += w * v_saved[idx]; wsum += w;
            }
        }
    }
    return select(vec2<f32>(0.0), vec2<f32>(fin, sav) / wsum, wsum > 0.0);
}

fn sample_w(P: vec3<f32>) -> vec2<f32> {
    let dim = vec3<i32>(i32(params.gdim.x), i32(params.gdim.y), i32(params.gdim.z) + 1);
    let g = (P - params.origin.xyz) / params.geom.x + vec3<f32>(-0.5, -0.5, 0.0);
    let base = vec3<i32>(floor(g));
    let t = g - vec3<f32>(base);
    var fin = 0.0; var sav = 0.0; var wsum = 0.0;
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
                let idx = ii + dim.x * (jj + dim.y * kk);
                fin += w * w_vel[idx]; sav += w * w_saved[idx]; wsum += w;
            }
        }
    }
    return select(vec2<f32>(0.0), vec2<f32>(fin, sav) / wsum, wsum > 0.0);
}

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let p = gid.x;
    if (p >= params.dims.y) { return; }

    let h = params.geom.x;
    let dt = params.geom.z;
    let alpha = params.phys.z; // flip_blend
    let P = particles[p].pos.xyz;
    let pv = particles[p].vel.xyz;

    let su = sample_u(P);
    let sv = sample_v(P);
    let sw = sample_w(P);
    let pic = vec3<f32>(su.x, sv.x, sw.x);
    let saved = vec3<f32>(su.y, sv.y, sw.y);
    let flip = pv + (pic - saved);
    var nv = mix(pic, flip, alpha);

    // CFL clamp. params.cls.z = CFL number = max cells a particle may cross per
    // step. At 1.0 the ceiling is h/dt, which scales DOWN as the grid is refined
    // (finer h → lower max speed → shallower splash). Allowing a few cells/step
    // decouples the achievable splash height from grid resolution; the wall-contact
    // clamp below still prevents particles escaping the tank.
    let cfl = max(params.cls.z, 1.0);
    let maxs = cfl * h / dt;
    let sp = length(nv);
    if (sp > maxs && sp > 0.0) { nv = nv * (maxs / sp); }

    // Per-axis world extent of the (possibly rectangular) tank: n_a * h.
    let extent = vec3<f32>(f32(params.gdim.x), f32(params.gdim.y), f32(params.gdim.z)) * h;
    let lo = params.origin.xyz + vec3<f32>(h * 1.05);
    let hi = params.origin.xyz + extent - vec3<f32>(h * 1.05);

    // Wall friction (phys.w): within a band of a wall, damp the velocity components
    // that are TANGENTIAL to that wall — so flow ALONG walls slows but gravity (the
    // wall-NORMAL component) is untouched (water still falls onto/off surfaces; no
    // ceiling cling). A component is tangential to the walls of the other two axes.
    let friction = params.phys.w;
    if (friction > 0.0) {
        let band = h * 2.5;
        let dlo = (P - lo) / band;
        let dhi = (hi - P) / band;
        let near_x = clamp(1.0 - min(dlo.x, dhi.x), 0.0, 1.0); // nearness to x-walls
        let near_y = clamp(1.0 - min(dlo.y, dhi.y), 0.0, 1.0); // nearness to y-walls
        let near_z = clamp(1.0 - min(dlo.z, dhi.z), 0.0, 1.0); // nearness to z-walls
        nv.x = nv.x * (1.0 - friction * max(near_y, near_z));
        nv.y = nv.y * (1.0 - friction * max(near_x, near_z));
        nv.z = nv.z * (1.0 - friction * max(near_x, near_y));
    }

    var np = P + nv * dt;

    // Wall contact: clamp inside the tank and zero the wall-normal velocity.
    if (np.x < lo.x) { np.x = lo.x; nv.x = 0.0; }
    if (np.x > hi.x) { np.x = hi.x; nv.x = 0.0; }
    if (np.y < lo.y) { np.y = lo.y; nv.y = 0.0; }
    if (np.y > hi.y) { np.y = hi.y; nv.y = 0.0; }
    if (np.z < lo.z) { np.z = lo.z; nv.z = 0.0; }
    if (np.z > hi.z) { np.z = hi.z; nv.z = 0.0; }

    particles[p].pos = vec4<f32>(np, 0.0);
    particles[p].vel = vec4<f32>(nv, 0.0);
}
