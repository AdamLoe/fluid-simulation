// Particle billboard renderer. Each particle is a camera-facing quad (6 verts,
// instanced). Positions come from the simulation's storage buffer directly — no
// readback. The shaded fragment is the v1.10 optical-depth particle view, the
// simple fragment is the pre-v1.10 alpha billboard view, and the thickness fragment
// writes normalized world-space path length and nearest positive eye distance for
// the screen-space water path. The camera uses right-handed eye space; visible
// eye-space z is negative, and the current perspective matrix exposes -z_eye as
// clip.w.

struct Camera {
    view_proj: mat4x4<f32>,
    right: vec4<f32>,      // xyz = camera right, w = particle radius (world units, scaled)
    up: vec4<f32>,         // xyz = camera up,    w = speed_scale
    slow_color: vec4<f32>, // xyz = slow-end RGB, w = water optical density
    fast_color: vec4<f32>, // xyz = fast-end RGB, w = unused
    extra: vec4<f32>,      // x = edge_inner_radius, y = shading_strength, z = volume_scale, w = simple alpha
};

struct Particle { pos: vec4<f32>, vel: vec4<f32> };

@group(0) @binding(0) var<uniform> cam: Camera;
@group(0) @binding(1) var<storage, read> particles: array<Particle>;

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec3<f32>,
    @location(2) optical_density: f32,
    @location(3) center_eye_distance: f32,
    @location(4) speed_fraction: f32,
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

    let speed = length(particles[ii].vel.xyz);
    let t = clamp(speed / max(cam.up.w, 0.001), 0.0, 1.0);
    let color = mix(cam.slow_color.xyz, cam.fast_color.xyz, t);

    var out: VsOut;
    let center_clip = cam.view_proj * vec4<f32>(center, 1.0);
    out.clip  = cam.view_proj * vec4<f32>(world, 1.0);
    out.uv    = c;
    out.color = color;
    out.optical_density = cam.slow_color.w;
    out.center_eye_distance = center_clip.w;
    out.speed_fraction = t;
    return out;
}

@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    let r = length(in.uv);
    if r > 1.0 { discard; }

    let edge_inner = cam.extra.x; // [0, 0.99] — inner radius where soft fade begins
    let shading    = cam.extra.y; // [0, 1]    — sphere shading strength

    // Sphere-like chord length through the billboard volume. The soft-edge control
    // modulates thickness near the rim instead of directly multiplying opacity.
    let nz = sqrt(max(0.0, 1.0 - r * r));
    let edge = 1.0 - smoothstep(edge_inner, 1.0, r);
    let thickness = nz * edge;
    let a = 1.0 - exp(-max(0.0, in.optical_density) * thickness);

    // Sphere shading: treat the billboard as a sphere surface.
    // Normal in billboard local space: (u, v, z) where z points toward the viewer.
    let n = normalize(vec3<f32>(in.uv.x, in.uv.y, nz));

    // Fixed key light in billboard space: slightly above, slightly behind viewer.
    // This gives a consistent top-left highlight regardless of world orientation.
    let key = normalize(vec3<f32>(-0.3, 0.5, 1.0));
    let diffuse = max(0.0, dot(n, key));

    // Small specular highlight for gloss.
    // View direction is (0,0,1) in billboard space (camera-facing).
    let refl = reflect(-key, n);
    let spec = pow(max(0.0, refl.z), 16.0) * 0.4;

    let ambient = 0.25;
    let shade = ambient + (1.0 - ambient) * diffuse + spec;
    let shaded_color = in.color * mix(1.0, shade, shading);

    return vec4<f32>(shaded_color, a);
}

@fragment
fn fs_simple(in: VsOut) -> @location(0) vec4<f32> {
    let r = length(in.uv);
    if r > 1.0 { discard; }

    let edge_inner = cam.extra.x;
    let shading    = cam.extra.y;

    let a = smoothstep(1.0, edge_inner, r) * clamp(cam.extra.w, 0.0, 1.0);
    let nz = sqrt(max(0.0, 1.0 - r * r));
    let n = normalize(vec3<f32>(in.uv.x, in.uv.y, nz));
    let key = normalize(vec3<f32>(-0.3, 0.5, 1.0));
    let diffuse = max(0.0, dot(n, key));
    let refl = reflect(-key, n);
    let spec = pow(max(0.0, refl.z), 16.0) * 0.4;
    let ambient = 0.25;
    let shade = ambient + (1.0 - ambient) * diffuse + spec;
    let shaded_color = in.color * mix(1.0, shade, shading);

    return vec4<f32>(shaded_color, a);
}

struct ThicknessOut {
    @location(0) thickness: f32,
    @location(1) nearest_z: f32,
    @location(2) whitewater: f32,
};

@fragment
fn fs_thickness(in: VsOut) -> ThicknessOut {
    let r2 = dot(in.uv, in.uv);
    if r2 > 1.0 { discard; }

    let r = sqrt(r2);
    let edge_inner = cam.extra.x;
    let nz = sqrt(max(0.0, 1.0 - r2));
    let edge = 1.0 - smoothstep(edge_inner, 1.0, r);

    let radius_world = cam.right.w;
    let kernel_len = 2.0 * radius_world * nz * edge;
    let volume_scale = cam.extra.z;

    var out: ThicknessOut;
    let thickness = max(0.0, kernel_len * volume_scale);
    out.thickness = thickness;
    out.nearest_z = clamp(in.center_eye_distance - radius_world * nz, 0.0, 65504.0);
    out.whitewater = thickness * in.speed_fraction;
    return out;
}
