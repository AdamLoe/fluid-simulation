// Occupancy mark (P2G-style scatter for cell typing): each particle increments
// its containing cell's count. atomicAdd gives a particle count per cell,
// which is used by classify (threshold/dilation) and the divergence volume term.

struct Params {
    dims: vec4<u32>,   // nx, particle_count, pressure_iters, _
    geom: vec4<f32>,   // h, inv_h, dt, fixed_scale
    phys: vec4<f32>,   // gravity_y, rho, flip_blend, _
    origin: vec4<f32>,
    grav: vec4<f32>,
    spc:  vec4<f32>,
    cls:  vec4<f32>,
    gdim: vec4<u32>,   // nx, ny, nz, _
};
struct Particle { pos: vec4<f32>, vel: vec4<f32> };

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> particles: array<Particle>;
@group(0) @binding(2) var<storage, read_write> occ: array<atomic<u32>>;

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
    let nx = params.gdim.x;
    let ny = params.gdim.y;
    let nz = params.gdim.z;
    let h = params.geom.x;
    let g = (particles[p].pos.xyz - params.origin.xyz) / h;
    let i = u32(clamp(floor(g.x), 0.0, f32(nx - 1u)));
    let j = u32(clamp(floor(g.y), 0.0, f32(ny - 1u)));
    let k = u32(clamp(floor(g.z), 0.0, f32(nz - 1u)));
    let c = i + nx * (j + ny * k);
    atomicAdd(&occ[c], 1u);
}
