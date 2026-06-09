// Diffuse-water render (v1.13). Instanced camera-facing billboards for the
// persistent foam / spray / bubble particles, drawn over the water composite and
// depth-tested against the shared scene depth. Dead slots collapse to a degenerate
// quad in the vertex stage so the whole capacity can be drawn cheaply.

struct Camera {
    view_proj: mat4x4<f32>,
    right: vec4<f32>,   // xyz = camera right, w = radius
    up: vec4<f32>,      // xyz = camera up,    w = peak alpha
    misc: vec4<f32>,    // x = debug_view, yzw _
};

@group(0) @binding(0) var<uniform> cam: Camera;
@group(0) @binding(1) var<storage, read> particles: array<vec4<f32>>;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec3<f32>,
    @location(2) alpha: f32,
};

@vertex
fn vs(@builtin(vertex_index) vid: u32, @builtin(instance_index) iid: u32) -> VsOut {
    var out: VsOut;
    let base = iid * 3u;
    let pt = particles[base + 0u];
    let ptype = pt.w;

    // Dead slot → degenerate (off-screen) triangle.
    if (ptype < 0.0) {
        out.pos = vec4<f32>(2.0, 2.0, 2.0, 1.0);
        out.uv = vec2<f32>(0.0);
        out.color = vec3<f32>(0.0);
        out.alpha = 0.0;
        return out;
    }

    let age = particles[base + 1u].w;
    let life = particles[base + 2u].x;
    let frac = clamp(age / max(life, 1e-4), 0.0, 1.0);
    // Quick fade-in over the first 12%, linear fade-out to end of life.
    let fade = min(frac / 0.12, 1.0) * (1.0 - frac);

    var corner = vec2<f32>(0.0);
    switch (vid) {
        case 0u: { corner = vec2<f32>(-1.0, -1.0); }
        case 1u: { corner = vec2<f32>( 1.0, -1.0); }
        case 2u: { corner = vec2<f32>( 1.0,  1.0); }
        case 3u: { corner = vec2<f32>(-1.0, -1.0); }
        case 4u: { corner = vec2<f32>( 1.0,  1.0); }
        default: { corner = vec2<f32>(-1.0,  1.0); }
    }

    let radius = cam.right.w;
    let world = pt.xyz + (corner.x * cam.right.xyz + corner.y * cam.up.xyz) * radius;
    out.pos = cam.view_proj * vec4<f32>(world, 1.0);
    out.uv = corner;

    // Type tints: natural look, or color-coded debug.
    var col: vec3<f32>;
    if (cam.misc.x > 0.5) {
        if (ptype < 0.5)      { col = vec3<f32>(0.2, 1.0, 0.3); }  // foam = green
        else if (ptype < 1.5) { col = vec3<f32>(1.0, 0.25, 0.2); } // spray = red
        else                  { col = vec3<f32>(0.3, 0.5, 1.0); }  // bubble = blue
    } else {
        if (ptype < 0.5)      { col = vec3<f32>(0.95, 0.97, 1.0); }  // foam = white
        else if (ptype < 1.5) { col = vec3<f32>(0.90, 0.95, 1.0); }  // spray = bright
        else                  { col = vec3<f32>(0.70, 0.82, 0.95); } // bubble = milky
    }
    out.color = col;
    out.alpha = fade * cam.up.w;
    return out;
}

@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    let r = length(in.uv);
    if (r > 1.0) { discard; }
    // Feathered blob: no hard core (a solid disc reads as a "ball"). A soft falloff
    // from center to edge lets many overlapping particles accumulate into froth
    // rather than a cluster of distinct spheres.
    let s = 1.0 - r;
    let edge = s * s;
    let a = in.alpha * edge;
    if (a <= 0.0) { discard; }
    return vec4<f32>(in.color * a, a);
}
