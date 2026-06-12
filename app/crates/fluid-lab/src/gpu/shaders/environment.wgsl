// Hero-water refractable environment. Writes linear HDR color into scene_color
// (location 0) and positive eye distance (clip.w = -z_eye) into scene_depth
// (location 1), matching the eye-distance convention used by water passes.

struct Env {
    view_proj: mat4x4<f32>,
    params:     vec4<f32>, // x=floor_scale, y=floor_strength, z=backdrop_strength, w=wall_visibility
    eye_world:  vec4<f32>, // xyz=camera eye in BOX-LOCAL space, w=unused
    env_ctrl:   vec4<f32>, // x=env_rotation, y=env_mode, z=env_brightness, w=unused
    sun:        vec4<f32>, // xyz=world sun direction, w=sun_intensity
    // Box-local to world rotation columns. Rotates a box-local direction into
    // world space for env_sample.
    box_rot_col0: vec4<f32>,
    box_rot_col1: vec4<f32>,
    box_rot_col2: vec4<f32>,
};

@group(0) @binding(0) var<uniform> env: Env;

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) kind:      f32,
    @location(1) uv:        vec2<f32>,
    @location(2) eye:       f32,
    @location(3) pos:       vec3<f32>,
};

@vertex
fn vs(
    @location(0) pos:  vec3<f32>,
    @location(1) kind: f32,
    @location(2) uv:   vec2<f32>,
) -> VsOut {
    var out: VsOut;
    let clip = env.view_proj * vec4<f32>(pos, 1.0);
    out.clip = clip;
    out.kind = kind;
    out.uv = uv;
    out.eye = clip.w;
    out.pos = pos;
    return out;
}

struct FsOut {
    @location(0) color: vec4<f32>,
    @location(1) eye:   f32,
};

fn box_to_world_dir(v: vec3<f32>) -> vec3<f32> {
    let rot = mat3x3<f32>(
        env.box_rot_col0.xyz,
        env.box_rot_col1.xyz,
        env.box_rot_col2.xyz,
    );
    return normalize(rot * v);
}

@fragment
fn fs(in: VsOut) -> FsOut {
    let floor_scale = env.params.x;
    let floor_strength = env.params.y;
    let backdrop_strength = env.params.z;
    let wall_visibility = env.params.w;

    var color = vec3<f32>(0.04, 0.05, 0.08);
    if in.kind < 0.5 {
        let g = in.uv * floor_scale;
        let cell = floor(g);
        let parity = (cell.x + cell.y) - 2.0 * floor((cell.x + cell.y) * 0.5);
        let base = vec3<f32>(0.14, 0.16, 0.20);
        let alt = vec3<f32>(0.32, 0.36, 0.42);
        color = mix(base, mix(base, alt, parity), floor_strength);

        let f = fract(g);
        let line = min(min(f.x, 1.0 - f.x), min(f.y, 1.0 - f.y));
        let grid = smoothstep(0.0, 0.04, line);
        color = mix(color, vec3<f32>(0.55, 0.62, 0.72), (1.0 - grid) * floor_strength * 0.6);
    } else if in.kind < 1.5 {
        color = vec3<f32>(0.10, 0.12, 0.16) * (0.4 + wall_visibility);
    } else {
        let view_dir_box = normalize(in.pos - env.eye_world.xyz);
        let view_dir_world = box_to_world_dir(view_dir_box);
        color = env_sample(view_dir_world, env.env_ctrl, env.sun) * backdrop_strength;
    }

    var out: FsOut;
    out.color = vec4<f32>(color, 1.0);
    out.eye = in.eye;
    return out;
}
