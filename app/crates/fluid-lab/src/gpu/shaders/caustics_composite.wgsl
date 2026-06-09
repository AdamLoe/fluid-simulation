// v1.16 Caustics composite pass (B) — additively paints caustic light onto
// scene_color. Runs AFTER the caustic generation+blur pass and BEFORE the water
// composite pass so that the water's refracted background picks up the caustic
// lighting (composite.wgsl line ~170 samples scene_color at refract_uv).
//
// Receiver discrimination: reconstructs world-space hit position from
// scene_depth (positive eye distance) along the per-pixel eye ray using
// tan(fov_y/2) + the composite camera's eye_to_world, then gates on
//   floor:     world_y ≈ tank_lo.y   (|Δy| < threshold)
//   back_wall: world_z ≈ tank_lo.z   (|Δz| < threshold)
//   side_walls enabled/disabled separately
//
// Output: only the caustic light contribution (sun_color * caustic_val).
// The render pipeline uses additive blending (src=ONE, dst=ONE) so this is
// added to scene_color by the blender — no scene_color_t sampler binding needed,
// eliminating the read-write hazard that would arise from binding scene_color as
// both a sampled texture and the render-pass color attachment.

struct Uniform {
    params:    vec4<f32>, // x=unused, y=tan(fov_y/2), z=width, w=height
    sun:       vec4<f32>, // xyz=world sun dir, w=sun intensity
    // Receiver bounds (from fluid.tank_bounds())
    tank_lo:   vec4<f32>, // xyz=tank lower corner, w=unused
    tank_hi:   vec4<f32>, // xyz=tank upper corner, w=unused
    // Caustic switches
    switches:  vec4<f32>, // x=enabled, y=floor_enabled, z=back_wall_enabled, w=side_walls_enabled
    // Camera
    // eye_to_world is passed via cam uniform (separate binding, same as composite.rs)
};

struct Cam {
    eye_to_world: mat4x4<f32>,
};

@group(0) @binding(0) var samp:          sampler;
// binding 1: scene_depth (non-filterable; used for world-pos reconstruction)
@group(0) @binding(1) var scene_depth_t: texture_2d<f32>;
// binding 2: caustic map (half-res R16Float)
@group(0) @binding(2) var caustic_t:     texture_2d<f32>;
@group(0) @binding(3) var<uniform> u:    Uniform;
@group(0) @binding(4) var<uniform> cam:  Cam;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs(@builtin(vertex_index) vi: u32) -> VsOut {
    var positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -3.0),
        vec2<f32>( 3.0,  1.0),
        vec2<f32>(-1.0,  1.0),
    );
    let p = positions[vi];
    var out: VsOut;
    out.pos = vec4<f32>(p, 0.0, 1.0);
    out.uv  = p * vec2<f32>(0.5, -0.5) + vec2<f32>(0.5);
    return out;
}

// Reconstruct world position from eye-distance depth and screen UV.
fn reconstruct_world(uv: vec2<f32>, eye_dist: f32) -> vec3<f32> {
    let width  = max(u.params.z, 1.0);
    let height = max(u.params.w, 1.0);
    let thf    = u.params.y; // tan(fov_y/2)
    let aspect = width / height;
    // NDC in [-1,1].
    let ndc = vec2<f32>(uv.x * 2.0 - 1.0, 1.0 - 2.0 * uv.y);
    // Eye-space ray direction (un-normalized; z = -1 for a perspective camera
    // where the camera looks toward -z in eye space).
    let ray_eye = vec3<f32>(ndc.x * thf * aspect, ndc.y * thf, -1.0);
    // Scale so that the z component reaches -eye_dist (scene_depth is positive eye distance).
    let scale = eye_dist / 1.0; // |ray_eye.z| = 1 so scale = eye_dist
    let pos_eye = ray_eye * scale;
    // Rotate to world space using the camera's eye->world matrix.
    let m3 = mat3x3<f32>(
        cam.eye_to_world[0].xyz,
        cam.eye_to_world[1].xyz,
        cam.eye_to_world[2].xyz,
    );
    // Eye position is the camera origin in world space = eye_to_world[3].xyz.
    let eye_world = cam.eye_to_world[3].xyz;
    return eye_world + m3 * pos_eye;
}

fn is_receiver(world_pos: vec3<f32>) -> bool {
    let tank_lo = u.tank_lo.xyz;
    let tank_hi = u.tank_hi.xyz;
    let eps_floor = 0.06;
    let eps_wall  = 0.06;

    // Floor: world y near tank bottom.
    if u.switches.y > 0.5 {
        if abs(world_pos.y - tank_lo.y) < eps_floor {
            // Also confirm x/z within tank bounds (with small margin).
            if world_pos.x >= tank_lo.x - eps_floor && world_pos.x <= tank_hi.x + eps_floor &&
               world_pos.z >= tank_lo.z - eps_floor && world_pos.z <= tank_hi.z + eps_floor {
                return true;
            }
        }
    }

    // Back wall: world z near tank minimum z (back face in standard orientation).
    if u.switches.z > 0.5 {
        if abs(world_pos.z - tank_lo.z) < eps_wall {
            if world_pos.x >= tank_lo.x - eps_wall && world_pos.x <= tank_hi.x + eps_wall &&
               world_pos.y >= tank_lo.y - eps_wall && world_pos.y <= tank_hi.y + eps_wall {
                return true;
            }
        }
    }

    // Side walls: world x near tank min/max x.
    if u.switches.w > 0.5 {
        let on_left  = abs(world_pos.x - tank_lo.x) < eps_wall;
        let on_right = abs(world_pos.x - tank_hi.x) < eps_wall;
        if on_left || on_right {
            if world_pos.y >= tank_lo.y - eps_wall && world_pos.y <= tank_hi.y + eps_wall &&
               world_pos.z >= tank_lo.z - eps_wall && world_pos.z <= tank_hi.z + eps_wall {
                return true;
            }
        }
    }

    return false;
}

@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    // Sample the caustic map upfront (before any non-uniform control flow) to
    // satisfy WGSL's uniform control-flow rule for textureSample.
    let caustic_val = max(0.0, textureSample(caustic_t, samp, in.uv).r);

    // Gate: caustics enabled.
    // When disabled output black (zero additive contribution).
    if u.switches.x < 0.5 {
        return vec4<f32>(0.0, 0.0, 0.0, 0.0);
    }

    // Read scene depth to reconstruct world hit position.
    let dims_u    = textureDimensions(scene_depth_t);
    let dims      = vec2<i32>(i32(dims_u.x), i32(dims_u.y));
    let pixel     = clamp(vec2<i32>(floor(in.pos.xy)), vec2<i32>(0), dims - vec2<i32>(1));
    let eye_dist  = textureLoad(scene_depth_t, pixel, 0).r;

    // No geometry here (sky/background) → no caustic receiver.
    if eye_dist >= 60000.0 {
        return vec4<f32>(0.0, 0.0, 0.0, 0.0);
    }

    let world_pos = reconstruct_world(in.uv, eye_dist);
    if !is_receiver(world_pos) {
        return vec4<f32>(0.0, 0.0, 0.0, 0.0);
    }

    if caustic_val < 1.0e-5 {
        return vec4<f32>(0.0, 0.0, 0.0, 0.0);
    }

    // Output only the caustic light contribution (warm sun color).
    // The pipeline uses additive blending so the blender adds this to scene_color,
    // avoiding the read-write hazard of binding scene_color as both sampler and RT.
    let sun_color = vec3<f32>(1.0, 0.96, 0.82);
    let caustic_light = sun_color * caustic_val;
    return vec4<f32>(caustic_light, 1.0);
}
