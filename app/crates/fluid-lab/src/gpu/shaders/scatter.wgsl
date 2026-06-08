// P2G scatter for one face component (AXIS = 0:u, 1:v, 2:w). Trilinear (tent)
// weights, fixed-point i32 atomic accumulation. See docs/p2g-strategy-note.md.
//
// num += round(w * v_component * FIXED_SCALE);  den += round(w * FIXED_SCALE)
// The whole accumulate path is integer (determinism invariant).

override AXIS: u32 = 0u;

struct Params {
    dims: vec4<u32>,   // nx, particle_count, pressure_iters, _
    geom: vec4<f32>,   // h, inv_h, dt, fixed_scale
    phys: vec4<f32>,
    origin: vec4<f32>,
    grav: vec4<f32>,
    spc:  vec4<f32>,
    cls:  vec4<f32>,
    gdim: vec4<u32>,   // nx, ny, nz, _
};
struct Particle { pos: vec4<f32>, vel: vec4<f32> };

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> particles: array<Particle>;
@group(0) @binding(2) var<storage, read_write> num: array<atomic<i32>>;
@group(0) @binding(3) var<storage, read_write> den: array<atomic<i32>>;

const PARTICLE_WG: u32 = 64u;

fn particle_index(wid: vec3<u32>, lid: u32, nwg: vec3<u32>) -> u32 {
    return ((wid.y * nwg.x + wid.x) * PARTICLE_WG) + lid;
}

@compute @workgroup_size(64, 1, 1)
fn main(
    @builtin(workgroup_id) wid: vec3<u32>,
    @builtin(local_invocation_index) lid: u32,
    @builtin(num_workgroups) nwg: vec3<u32>,
) {
    let p = particle_index(wid, lid, nwg);
    if (p >= params.dims.y) { return; }

    let nx = i32(params.gdim.x);
    let ny = i32(params.gdim.y);
    let nz = i32(params.gdim.z);
    let h = params.geom.x;
    let scale = params.geom.w;

    // Per-axis staggering: the AXIS component sits on integer faces (offset 0),
    // the other two are cell-centered (offset -0.5). Face grid dims add +1 on AXIS.
    var off = vec3<f32>(-0.5, -0.5, -0.5);
    var dim = vec3<i32>(nx, ny, nz);
    var vcomp: f32;
    let pv = particles[p].vel.xyz;
    if (AXIS == 0u) { off.x = 0.0; dim.x = nx + 1; vcomp = pv.x; }
    else if (AXIS == 1u) { off.y = 0.0; dim.y = ny + 1; vcomp = pv.y; }
    else { off.z = 0.0; dim.z = nz + 1; vcomp = pv.z; }

    let g = (particles[p].pos.xyz - params.origin.xyz) / h + off;
    let base = vec3<i32>(floor(g));
    let t = g - vec3<f32>(base);

    for (var dk = 0; dk < 2; dk = dk + 1) {
        let kk = base.z + dk;
        if (kk < 0 || kk >= dim.z) { continue; }
        let wz = select(1.0 - t.z, t.z, dk == 1);
        for (var dj = 0; dj < 2; dj = dj + 1) {
            let jj = base.y + dj;
            if (jj < 0 || jj >= dim.y) { continue; }
            let wy = select(1.0 - t.y, t.y, dj == 1);
            for (var di = 0; di < 2; di = di + 1) {
                let ii = base.x + di;
                if (ii < 0 || ii >= dim.x) { continue; }
                let wx = select(1.0 - t.x, t.x, di == 1);

                let w = wx * wy * wz;
                if (w <= 0.0) { continue; }
                let idx = ii + dim.x * (jj + dim.y * kk);
                atomicAdd(&num[idx], i32(round(w * vcomp * scale)));
                atomicAdd(&den[idx], i32(round(w * scale)));
            }
        }
    }
}
