// Particle billboard renderer. Each particle is a camera-facing quad (6 verts,
// instanced). Positions come from the simulation's storage buffer directly — no
// readback. Color encodes height (y) for readability.

struct Camera {
    view_proj: mat4x4<f32>,
    right: vec4<f32>, // xyz = camera right, w = particle radius (world units, scaled)
    up: vec4<f32>,    // xyz = camera up,    w = speed_scale (speed that maps to full white)
};

struct Particle { pos: vec4<f32>, vel: vec4<f32> };

@group(0) @binding(0) var<uniform> cam: Camera;
@group(0) @binding(1) var<storage, read> particles: array<Particle>;

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec3<f32>,
};

@vertex
fn vs(@builtin(vertex_index) vi: u32, @builtin(instance_index) ii: u32) -> VsOut {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0),
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, 1.0), vec2<f32>(-1.0, 1.0),
    );
    let c = corners[vi];
    let center = particles[ii].pos.xyz;
    let radius = cam.right.w;
    let world = center + c.x * radius * cam.right.xyz + c.y * radius * cam.up.xyz;

    // Color by speed: slow = deep blue, fast = cyan/white (shows the motion/waves).
    let speed = length(particles[ii].vel.xyz);
    let t = clamp(speed / max(cam.up.w, 0.001), 0.0, 1.0);
    let color = mix(vec3<f32>(0.10, 0.30, 0.80), vec3<f32>(0.70, 0.92, 1.0), t);

    var out: VsOut;
    out.clip = cam.view_proj * vec4<f32>(world, 1.0);
    out.uv = c;
    out.color = color;
    return out;
}

@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    // Round point with soft edge.
    let r = length(in.uv);
    if (r > 1.0) {
        discard;
    }
    let a = smoothstep(1.0, 0.6, r);
    return vec4<f32>(in.color, a);
}
