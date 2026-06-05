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

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let p = gid.x;
    // Reference both params.dims.y and imp so naga keeps both bindings.
    if (p >= params.dims.y) { return; }
    particles[p].vel = particles[p].vel + vec4<f32>(imp.xyz, 0.0);
}
