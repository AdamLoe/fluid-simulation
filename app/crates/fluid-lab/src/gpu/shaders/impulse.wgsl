// Apply a uniform velocity impulse to every particle.
// Binding 0: params uniform (dims.y = particle_count)
// Binding 1: imp  uniform  (xyz = impulse vector)
// Binding 2: particles storage read_write

struct Params {
    dims:   vec4<u32>,
    geom:   vec4<f32>,
    phys:   vec4<f32>,
    origin: vec4<f32>,
    grav:   vec4<f32>,
};

struct Particle { pos: vec4<f32>, vel: vec4<f32> };

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<uniform> imp: vec4<f32>;
@group(0) @binding(2) var<storage, read_write> particles: array<Particle>;

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
    // Reference both params.dims.y and imp so naga keeps both bindings.
    if (p >= params.dims.y) { return; }
    particles[p].vel = particles[p].vel + vec4<f32>(imp.xyz, 0.0);
}
