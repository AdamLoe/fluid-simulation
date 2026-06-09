// World-background procedural skybox (v1.15). A fullscreen triangle that fills the
// hero-water scene-color prepass behind the geometry with the shared procedural
// environment (env.wgsl), sampled by the per-pixel WORLD-space view ray.
//
// The ray uses the camera's eye->world rotation ONLY (no tank model matrix), so the
// background stays fixed to the world when the box rotates but pans when the camera
// orbits. It writes the far eye-distance sentinel into scene_depth so the refraction
// depth guard treats it as background, and depth-write is off so floor/walls/wireframe
// draw over it. env.wgsl is concatenated ahead of this file.

struct Sky {
    eye_to_world: mat4x4<f32>,
    cam: vec4<f32>,  // x = tan(fov_y/2), y = aspect, z = brightness, w = rotation
    sun: vec4<f32>,  // xyz = world sun direction, w = sun intensity
    mode: vec4<f32>, // x = environment mode, yzw = unused
};

@group(0) @binding(0) var<uniform> sky: Sky;

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) ndc: vec2<f32>,
};

@vertex
fn vs(@builtin(vertex_index) vi: u32) -> VsOut {
    var pos = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -3.0),
        vec2<f32>(3.0, 1.0),
        vec2<f32>(-1.0, 1.0),
    );
    let p = pos[vi];
    var out: VsOut;
    out.clip = vec4<f32>(p, 1.0, 1.0); // z = 1 → far plane
    out.ndc = p;
    return out;
}

struct FsOut {
    @location(0) color: vec4<f32>,
    @location(1) eye: f32,
};

@fragment
fn fs(in: VsOut) -> FsOut {
    let thf = sky.cam.x;
    let aspect = sky.cam.y;
    let dir_eye = normalize(vec3<f32>(in.ndc.x * thf * aspect, in.ndc.y * thf, -1.0));
    let m = mat3x3<f32>(
        sky.eye_to_world[0].xyz,
        sky.eye_to_world[1].xyz,
        sky.eye_to_world[2].xyz,
    );
    let dir_world = m * dir_eye;
    let ctrl = vec4<f32>(sky.cam.w, sky.mode.x, sky.cam.z, 0.0);
    let col = env_sample(dir_world, ctrl, sky.sun);

    var out: FsOut;
    out.color = vec4<f32>(col, 1.0);
    out.eye = 65504.0; // far sentinel (matches the scene_depth "no geometry" clear)
    return out;
}
