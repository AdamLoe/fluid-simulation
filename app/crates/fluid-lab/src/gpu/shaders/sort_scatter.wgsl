// Particle spatial sort — pass 4: scatter each particle into its sorted slot.
// Recomputes the linear cell key (IDENTICAL math to mark.wgsl), claims the next
// slot in its cell's bucket via a running-cursor atomic on cell_offset, and copies
// its full record (pos + vel) from particles_src into particles_dst at that slot.
//
// cell_offset enters holding the EXCLUSIVE prefix sum (bucket starts); atomicAdd
// advances the per-cell cursor so every particle lands in a distinct slot. The
// resulting permutation is a bijection (each particle written exactly once) and is
// deterministic at the SIMULATION level: P2G is order-independent integer atomics
// and g2p is per-particle, so any in-bucket ordering yields bit-identical state.

struct Params {
    dims: vec4<u32>,   // nx, particle_count, ...
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
@group(0) @binding(1) var<storage, read> particles_src: array<Particle>;
@group(0) @binding(2) var<storage, read_write> particles_dst: array<Particle>;
@group(0) @binding(3) var<storage, read_write> cell_offset: array<atomic<u32>>;

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
    let g = (particles_src[p].pos.xyz - params.origin.xyz) / h;
    let i = u32(clamp(floor(g.x), 0.0, f32(nx - 1u)));
    let j = u32(clamp(floor(g.y), 0.0, f32(ny - 1u)));
    let k = u32(clamp(floor(g.z), 0.0, f32(nz - 1u)));
    let c = i + nx * (j + ny * k);
    let dst = atomicAdd(&cell_offset[c], 1u);
    particles_dst[dst] = particles_src[p];
}
