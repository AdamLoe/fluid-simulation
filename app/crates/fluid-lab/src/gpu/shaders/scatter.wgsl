// Fused P2G scatter for all three MAC face components (u, v, w) in ONE pass.
// Reads each particle exactly once, computes the cell-space position once, and
// scatters all three velocity components into their respective num/den buffers,
// each axis using its own correct half-cell staggering offset and +1 face dim.
// Trilinear (tent) weights; fixed-point i32 atomic accumulation. See
// docs/p2g-strategy-note.md and docs/architecture/simulation.md.
//
// num += round(w * v_component * FIXED_SCALE);  den += round(w * FIXED_SCALE)
// The whole accumulate path is integer (determinism invariant): bit-identical
// to the prior three per-axis passes (same atomicAdds, same buffers, integer
// add stays associative/commutative).

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
@group(0) @binding(2) var<storage, read_write> u_num: array<atomic<i32>>;
@group(0) @binding(3) var<storage, read_write> u_den: array<atomic<i32>>;
@group(0) @binding(4) var<storage, read_write> v_num: array<atomic<i32>>;
@group(0) @binding(5) var<storage, read_write> v_den: array<atomic<i32>>;
@group(0) @binding(6) var<storage, read_write> w_num: array<atomic<i32>>;
@group(0) @binding(7) var<storage, read_write> w_den: array<atomic<i32>>;

const PARTICLE_WG: u32 = 64u;

fn particle_index(wid: vec3<u32>, lid: u32, nwg: vec3<u32>) -> u32 {
    return ((wid.y * nwg.x + wid.x) * PARTICLE_WG) + lid;
}

// Scatter one velocity component to one face buffer pair. `off` is the per-axis
// staggering (0 on the component's own axis, -0.5 on the other two) and `dim`
// is the face grid dims (n+1 on the component's own axis). Identical math to the
// AXIS-parameterized pass, evaluated once per component for a single particle.
fn scatter_component(
    g: vec3<f32>,
    dim: vec3<i32>,
    vcomp: f32,
    scale: f32,
    num: ptr<storage, array<atomic<i32>>, read_write>,
    den: ptr<storage, array<atomic<i32>>, read_write>,
) {
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
    let inv_h = params.geom.y;
    let scale = params.geom.w;

    // Read the particle once; share the cell-space conversion across all axes.
    let pos = (particles[p].pos.xyz - params.origin.xyz) * inv_h;
    let pv = particles[p].vel.xyz;

    // Per-axis staggering: the component's own axis sits on integer faces
    // (offset 0); the other two are cell-centered (offset -0.5). The face grid
    // adds +1 on the component's own axis.
    // u: x on faces                          -> off = ( 0.0, -0.5, -0.5), dim = (nx+1, ny,   nz)
    scatter_component(
        pos + vec3<f32>(0.0, -0.5, -0.5),
        vec3<i32>(nx + 1, ny, nz),
        pv.x, scale, &u_num, &u_den,
    );
    // v: y on faces                          -> off = (-0.5,  0.0, -0.5), dim = (nx,   ny+1, nz)
    scatter_component(
        pos + vec3<f32>(-0.5, 0.0, -0.5),
        vec3<i32>(nx, ny + 1, nz),
        pv.y, scale, &v_num, &v_den,
    );
    // w: z on faces                          -> off = (-0.5, -0.5,  0.0), dim = (nx,   ny,   nz+1)
    scatter_component(
        pos + vec3<f32>(-0.5, -0.5, 0.0),
        vec3<i32>(nx, ny, nz + 1),
        pv.z, scale, &w_num, &w_den,
    );
}
