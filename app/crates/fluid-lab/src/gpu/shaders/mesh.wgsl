// Mesh render shader — translucent glassy water surface (issues 1, 2, 5).
//
// Vertices come from the MC output storage buffer (indexed by vertex_index):
//   pos.xyz = world position (TANK-LOCAL space; the model matrix is baked into
//             view_proj only), nrm.xyz = normal, nrm.w = foam factor (0..1).
//
// The camera eye (cam.cam_pos) is supplied IN THE SAME TANK-LOCAL SPACE as the
// vertices (see gpu/mod.rs / lib.rs), so the view vector is correct even while the
// tank is moved/rotated — that was the cause of the white Fresnel flashes.
//
// Look: a deep blue body, Schlick-Fresnel reflection toward a sky tint (more
// reflective + opaque at grazing angles), a tight white sun specular, and white
// foam only where the water is actually moving fast (nrm.w). Output is alpha-
// blended (see mesh.rs) so the tank/background reads through.

struct Camera {
    view_proj: mat4x4<f32>,
    cam_pos: vec4<f32>,  // xyz = eye in tank-local space
    sun_dir: vec4<f32>,  // xyz = sun direction (normalized in shader)
    water:   vec4<f32>,  // x=opacity, y=fresnel_strength, z=foam_strength, w=_
    tint:    vec4<f32>,  // xyz = base water color
};

struct Vertex {
    pos: vec4<f32>,
    nrm: vec4<f32>,
};

@group(0) @binding(0) var<uniform> cam: Camera;
@group(0) @binding(1) var<storage, read> verts: array<Vertex>;

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world_pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) foam: f32,
};

@vertex
fn vs(@builtin(vertex_index) vi: u32) -> VsOut {
    let v = verts[vi];
    var out: VsOut;
    out.clip = cam.view_proj * vec4<f32>(v.pos.xyz, 1.0);
    out.world_pos = v.pos.xyz;
    out.normal = v.nrm.xyz;
    out.foam = v.nrm.w;
    return out;
}

@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    var N = normalize(in.normal);
    let V = normalize(cam.cam_pos.xyz - in.world_pos);
    // MC winding/normals are ambiguous; always face the normal toward the camera
    // so shading and Fresnel are stable (no flashing when the tank moves).
    if (dot(N, V) < 0.0) { N = -N; }
    let L = normalize(cam.sun_dir.xyz);

    let n_dot_v = clamp(dot(N, V), 0.0, 1.0);
    let n_dot_l = clamp(dot(N, L), 0.0, 1.0);

    // Diffuse-lit water body: deep tint in shadow, brighter tint toward the light.
    let base = cam.tint.xyz;
    let deep = base * 0.45;
    let body = mix(deep, base, n_dot_l) + base * 0.20; // + ambient

    // Schlick Fresnel (F0 ~ 0.02 for water), scaled by the reflectivity knob.
    let fresnel = (0.02 + 0.98 * pow(1.0 - n_dot_v, 5.0)) * cam.water.y;

    // Reflected environment: a soft sky/horizon tint (no cubemap available).
    let sky = vec3<f32>(0.55, 0.70, 0.92);
    var color = mix(body, sky, clamp(fresnel, 0.0, 1.0));

    // Tight white sun specular (Blinn-Phong), only on the lit side.
    let H = normalize(L + V);
    let spec = pow(max(dot(N, H), 0.0), 90.0) * n_dot_l;
    color += vec3<f32>(spec);

    // Foam: white only where the water is actually moving fast (issue 5).
    let foam = clamp(in.foam, 0.0, 1.0) * cam.water.z;
    let foam_amt = clamp(foam, 0.0, 1.0);
    color = mix(color, vec3<f32>(0.95, 0.97, 1.0), foam_amt);

    // Opacity: base translucency, more opaque at grazing (Fresnel) and where foamy.
    let alpha = clamp(cam.water.x + fresnel * 0.5 + foam_amt * 0.5, 0.0, 1.0);

    return vec4<f32>(clamp(color, vec3<f32>(0.0), vec3<f32>(1.0)), alpha);
}
