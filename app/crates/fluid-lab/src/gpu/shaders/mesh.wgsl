// Mesh render shader — water as a VOLUME (absorption + refraction + reflection).
//
// Vertices come from the MC output storage buffer (indexed by vertex_index):
//   pos.xyz = TANK-LOCAL position (model matrix is baked into view_proj only),
//   nrm.xyz = normal, nrm.w = foam factor (0..1).
//
// The camera eye (cam.cam_pos) is supplied IN THE SAME TANK-LOCAL SPACE as the
// vertices, so the view vector / Fresnel stay correct as the tank is moved/rotated.
//
// Unlike the old thin-shell shader, the water no longer alpha-blends a constant
// opacity. It is drawn OPAQUE over a background blit of the offscreen scene color
// and composites that background itself:
//   - THICKNESS comes from the water's own back surface: a depth-only prepass writes
//     the farthest water depth into `back_depth`; here we reconstruct that position
//     (via inv_view_proj) and take the distance to this (front) fragment. That path
//     length drives Beer-Lambert absorption, so deep water tints far more than a thin
//     film — the fix for "clear as glass".
//   - REFRACTION offsets the background sample along the screen-projected surface
//     normal (screen-space refraction; no scene reconstruction needed).
//   - REFLECTION is a procedural sky gradient + sun, mixed in by Fresnel (replacing
//     the old flat sky constant).

struct Camera {
    view_proj:     mat4x4<f32>,
    inv_view_proj: mat4x4<f32>,
    cam_pos: vec4<f32>,  // xyz = eye in tank-local space
    sun_dir: vec4<f32>,  // xyz = sun direction (normalized in shader)
    water:   vec4<f32>,  // x=opacity, y=fresnel, z=foam, w=absorb_strength
    tint:    vec4<f32>,  // xyz = base water color, w=refract_strength
    misc:    vec4<f32>,  // xy = render-target resolution (px)
};

struct Vertex {
    pos: vec4<f32>,
    nrm: vec4<f32>,
};

@group(0) @binding(0) var<uniform> cam: Camera;
@group(0) @binding(1) var<storage, read> verts: array<Vertex>;
@group(0) @binding(2) var scene_color: texture_2d<f32>;
@group(0) @binding(3) var back_depth:  texture_depth_2d;

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

// Reconstruct tank-local position from a pixel coordinate + its stored depth.
fn reconstruct(px: vec2<i32>, depth: f32) -> vec3<f32> {
    let uv = (vec2<f32>(px) + vec2<f32>(0.5)) / cam.misc.xy; // 0..1, origin top-left
    let ndc = vec3<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, depth);
    let p = cam.inv_view_proj * vec4<f32>(ndc, 1.0);
    return p.xyz / p.w;
}

// Procedural sky for the reflection (no cubemap): a horizon→zenith gradient in
// tank-local up plus a soft sun disc/glow.
fn sky(dir: vec3<f32>) -> vec3<f32> {
    let up = clamp(dir.y * 0.5 + 0.5, 0.0, 1.0);
    let horizon = vec3<f32>(0.62, 0.70, 0.80);
    let zenith  = vec3<f32>(0.18, 0.40, 0.78);
    var c = mix(horizon, zenith, up);
    let s = clamp(dot(normalize(dir), normalize(cam.sun_dir.xyz)), 0.0, 1.0);
    c += vec3<f32>(1.0, 0.95, 0.85) * pow(s, 350.0) * 2.0; // sun disc
    c += vec3<f32>(1.0, 0.90, 0.75) * pow(s, 8.0) * 0.10;  // sun glow
    return c;
}

@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    var N = normalize(in.normal);
    let V = normalize(cam.cam_pos.xyz - in.world_pos);
    // MC winding/normals are ambiguous; always face the normal toward the camera.
    if (dot(N, V) < 0.0) { N = -N; }
    let L = normalize(cam.sun_dir.xyz);
    let n_dot_v = clamp(dot(N, V), 0.0, 1.0);

    let px = vec2<i32>(in.clip.xy);
    let res = cam.misc.xy;

    // ── Thickness: front = this fragment, back = the water's far surface ──────────
    let back_pos = reconstruct(px, textureLoad(back_depth, px, 0));
    let thickness = clamp(length(back_pos - in.world_pos), 0.0, 4.0);

    // ── Screen-space refraction: offset along the screen-projected surface normal ──
    let base_uv = (vec2<f32>(px) + vec2<f32>(0.5)) / res;
    let c0 = cam.view_proj * vec4<f32>(in.world_pos, 1.0);
    let c1 = cam.view_proj * vec4<f32>(in.world_pos + N * 0.02, 1.0);
    let sn = (c1.xy / c1.w) - (c0.xy / c0.w); // NDC-space normal direction
    let refr = cam.tint.w * (0.4 + 0.6 * clamp(thickness, 0.0, 1.0));
    let ruv = clamp(base_uv + vec2<f32>(sn.x, -sn.y) * refr, vec2<f32>(0.0), vec2<f32>(1.0));
    let bg = textureLoad(scene_color, vec2<i32>(ruv * res), 0).rgb;

    // ── Beer-Lambert absorption: red absorbed most, blue least → blue-green deepens ─
    let sigma = cam.water.w * (vec3<f32>(1.0) - cam.tint.xyz);
    let trans = exp(-sigma * thickness);
    var color = bg * trans + cam.tint.xyz * (1.0 - trans); // absorb + in-scatter

    // ── Reflection (Fresnel) toward the procedural sky ───────────────────────────
    let R = reflect(-V, N);
    let fres = clamp((0.02 + 0.98 * pow(1.0 - n_dot_v, 5.0)) * cam.water.y, 0.0, 1.0);
    color = mix(color, sky(R), fres);

    // ── Sun specular (Blinn-Phong) + velocity foam ───────────────────────────────
    let H = normalize(L + V);
    let spec = pow(max(dot(N, H), 0.0), 120.0) * clamp(dot(N, L), 0.0, 1.0);
    color += vec3<f32>(spec) * 0.8;

    let foam = clamp(in.foam, 0.0, 1.0) * cam.water.z;
    color = mix(color, vec3<f32>(0.95, 0.97, 1.0), clamp(foam, 0.0, 1.0));

    return vec4<f32>(clamp(color, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
}
