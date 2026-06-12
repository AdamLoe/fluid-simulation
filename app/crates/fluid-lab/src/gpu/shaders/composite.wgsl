struct Params {
    tint_density: vec4<f32>,
    params: vec4<f32>,      // x = shading, y = tan(fov_y/2), z = width, w = height
    whitewater: vec4<f32>,
};

struct Hero {
    refr: vec4<f32>,   // x = effective strength, y = thickness scale, z = max offset px, w = f0
    absorb: vec4<f32>, // rgb = absorption color, w = absorption strength
    tint: vec4<f32>,   // rgb = base tint, w = transparency
    misc: vec4<f32>,   // x = deep darkening, y = invalid fallback, z = debug view, w = body color enabled
    // --- Environment reflection (v1.15) ---
    refl: vec4<f32>,   // x = effective reflection strength, y = environment strength, z = environment brightness, w = skybox enabled
    envc: vec4<f32>,   // x = environment rotation, y = environment mode, z = roughness base, w = unused
    rough: vec4<f32>,  // x = velocity scale, y = normal-variance scale, z = foam scale, w = unused
    sun: vec4<f32>,    // xyz = world sun direction, w = sun intensity
    micro: vec4<f32>,  // x = enabled, y = strength, z = scale, w = velocity scale
    spec: vec4<f32>,   // x = specular strength, yzw = unused
    // --- Surface normal quality (v1.19 round-2) ---
    norm: vec4<f32>,   // x = normal_stencil (px), y = normal_smooth_strength, zw = unused
    // --- Wall-fill-only composite controls ---
    wallfill: vec4<f32>, // x=color strength, y=reflection multiplier, z=roughness, w=absorption strength
};

// Per-frame camera uniform for composite.wgsl.
// eye_to_world: camera-only eye->world rotation (mat4x4).
// box_eye_local: camera eye in box-local space (xyz, w=unused).
// box_rot_col0/1/2: box-local→world rotation columns.
// tank_lo/hi: tank bounds in box-local space (xyz, w=unused).
// flat: x=flat_water_strength, y=flat_water_epsilon, z=depth_strength, w=unused.
struct Cam {
    eye_to_world:  mat4x4<f32>,
    box_eye_local: vec4<f32>,
    box_rot_col0:  vec4<f32>,
    box_rot_col1:  vec4<f32>,
    box_rot_col2:  vec4<f32>,
    tank_lo:       vec4<f32>,
    tank_hi:       vec4<f32>,
    flat:          vec4<f32>, // x=strength, y=epsilon, z=depth_strength, w=unused
};

@group(0) @binding(0) var thickness_sampler: sampler;
@group(0) @binding(1) var thickness_tex: texture_2d<f32>;
@group(0) @binding(2) var whitewater_tex: texture_2d<f32>;
@group(0) @binding(3) var smoothed_z_tex: texture_2d<f32>;
@group(0) @binding(4) var<uniform> params: Params;
@group(0) @binding(5) var<uniform> hero: Hero;
@group(0) @binding(6) var scene_color_tex: texture_2d<f32>;
@group(0) @binding(7) var scene_depth_tex: texture_2d<f32>;
@group(0) @binding(8) var<uniform> cam: Cam;
@group(0) @binding(9) var wallfill_mask_tex: texture_2d<f32>;

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
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
    out.clip = vec4<f32>(p, 0.0, 1.0);
    out.uv = p * vec2<f32>(0.5, -0.5) + vec2<f32>(0.5, 0.5);
    return out;
}

fn load_z(p: vec2<i32>, dims: vec2<i32>) -> f32 {
    let q = clamp(p, vec2<i32>(0, 0), dims - vec2<i32>(1, 1));
    return textureLoad(smoothed_z_tex, q, 0).r;
}

// Reconstruct the eye-space surface normal from the smoothed depth buffer.
// Uses a tunable central-difference half-width (normal_stencil, 1-3 px) to
// low-pass the derivative and suppress per-splat lobes that survive bilateral
// smoothing. Optionally blends the result toward a cross-averaged normal
// (normal_smooth_strength) for further smoothing.
fn compute_normal_at(pixel: vec2<i32>, s: i32, dims: vec2<i32>) -> vec3<f32> {
    let c = load_z(pixel, dims);
    if c >= 60000.0 {
        return vec3<f32>(0.0, 0.0, 1.0);
    }
    let l_raw = load_z(pixel + vec2<i32>(-s, 0), dims);
    let r_raw = load_z(pixel + vec2<i32>( s, 0), dims);
    let d_raw = load_z(pixel + vec2<i32>(0, -s), dims);
    let u_raw = load_z(pixel + vec2<i32>(0,  s), dims);
    let l = select(l_raw, c, l_raw >= 60000.0);
    let r = select(r_raw, c, r_raw >= 60000.0);
    let d = select(d_raw, c, d_raw >= 60000.0);
    let u = select(u_raw, c, u_raw >= 60000.0);

    let width = max(params.params.z, 1.0);
    let height = max(params.params.w, 1.0);
    let tan_half_fovy = params.params.y;
    let aspect = width / height;
    // World units per pixel at depth c, scaled by stencil half-width.
    let fs = f32(s);
    let world_per_px_y = max(1.0e-4, 2.0 * c * tan_half_fovy / height);
    let world_per_px_x = max(1.0e-4, 2.0 * c * tan_half_fovy * aspect / width);
    let dzdx = (r - l) * 0.5 / (fs * world_per_px_x);
    let dzdy = (u - d) * 0.5 / (fs * world_per_px_y);
    return normalize(vec3<f32>(-dzdx, -dzdy, 1.0));
}

fn water_normal(pixel: vec2<i32>, dims: vec2<i32>) -> vec3<f32> {
    let c = load_z(pixel, dims);
    if c >= 60000.0 {
        return vec3<f32>(0.0, 0.0, 1.0);
    }
    // normal_stencil knob (1-3); clamp to safe integer.
    let stencil = i32(clamp(hero.norm.x, 1.0, 3.0));
    let n = compute_normal_at(pixel, stencil, dims);

    // Optional normal smoothing: average with normals sampled at diagonal
    // offsets of stencil+1 px to further suppress residual lobes.
    let smooth_str = clamp(hero.norm.y, 0.0, 1.0);
    if smooth_str < 0.001 {
        return n;
    }
    let os = stencil + 1;
    let n_ul = compute_normal_at(pixel + vec2<i32>(-os, -os), stencil, dims);
    let n_ur = compute_normal_at(pixel + vec2<i32>( os, -os), stencil, dims);
    let n_ll = compute_normal_at(pixel + vec2<i32>(-os,  os), stencil, dims);
    let n_lr = compute_normal_at(pixel + vec2<i32>( os,  os), stencil, dims);
    let n_avg = normalize(n_ul + n_ur + n_ll + n_lr);
    return normalize(mix(n, n_avg, smooth_str));
}

@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    let width = max(params.params.z, 1.0);
    let height = max(params.params.w, 1.0);
    let dims_u = textureDimensions(smoothed_z_tex);
    let dims = vec2<i32>(i32(dims_u.x), i32(dims_u.y));
    let pixel = clamp(vec2<i32>(floor(in.clip.xy)), vec2<i32>(0, 0), dims - vec2<i32>(1, 1));

    let thickness = max(0.0, textureSample(thickness_tex, thickness_sampler, in.uv).r);
    let wall_fill_mask = clamp(textureSample(wallfill_mask_tex, thickness_sampler, in.uv).r, 0.0, 1.0);
    var front_z = load_z(pixel, dims);
    let has_water = thickness > 1.0e-4 && front_z < 60000.0;
    var n = water_normal(pixel, dims);

    // --- Flat-water-against-walls: snap the surface DEPTH and NORMAL toward the nearest
    // tank wall/floor plane when the front surface is within epsilon of it.
    // This makes water pressed against the glass read as a flat sheet, not bumpy spheres.
    // The depth snap (silhouette flatten) removes sphere bumps at the glass edge;
    // the normal snap ensures shading, refraction, and lighting are consistent.
    let flat_strength = cam.flat.x;
    let flat_epsilon  = cam.flat.y;
    let flat_depth    = cam.flat.z;
    if has_water && flat_strength > 0.001 && flat_epsilon > 0.0 {
        // Reconstruct eye-space surface position from NDC + front_z.
        let ndc = vec2<f32>(in.uv.x * 2.0 - 1.0, 1.0 - 2.0 * in.uv.y);
        let aspect_fw = width / height;
        let thf_fw = params.params.y;
        let fdir_eye = vec3<f32>(ndc.x * thf_fw * aspect_fw, ndc.y * thf_fw, -1.0);
        // Scale so z_eye = -front_z: pos_eye = fdir_eye * (front_z / abs(fdir_eye.z))
        // This gives the eye-space position: z = -front_z, xy proportional to NDC.
        let scale = front_z / abs(fdir_eye.z);
        let pos_eye = fdir_eye * scale;  // eye-space surface position

        // Transform relative-to-camera eye-space pos to world-space direction:
        //   pos_world - cam_world = (eye_to_world 3x3) * pos_eye
        // Then to box-local:
        //   pos_box_local = cam.box_eye_local + box_world_to_local_rot * (pos_world - cam_world)
        // box_world_to_local = transpose(box_rot) = [col0,col1,col2]^T
        let box_rot = mat3x3<f32>(
            cam.box_rot_col0.xyz,
            cam.box_rot_col1.xyz,
            cam.box_rot_col2.xyz,
        );
        let eye3 = mat3x3<f32>(
            cam.eye_to_world[0].xyz,
            cam.eye_to_world[1].xyz,
            cam.eye_to_world[2].xyz,
        );
        // delta_world = eye3 * pos_eye  (relative to camera in world space)
        let delta_world = eye3 * pos_eye;
        // box_rot^T (world->box_local) applied to delta_world, added to box_eye_local:
        let box_rot_t = transpose(box_rot);
        let pos_bl = cam.box_eye_local.xyz + box_rot_t * delta_world;

        // Find min signed distance to the five visible tank planes in box-local coords.
        // Tank spans [lo, hi]; Solid boundaries at lo.x, hi.x (left/right walls),
        // lo.z, hi.z (back/front walls), lo.y (floor). We test proximity to each plane
        // and accumulate the flattest blend if multiple planes are close.
        let lo_bl = cam.tank_lo.xyz;
        let hi_bl = cam.tank_hi.xyz;

        // Plane normals IN BOX-LOCAL space (pointing inward = outward from the wall face):
        // left wall  x=lo.x: normal = +x = (1,0,0)
        // right wall x=hi.x: normal = -x = (-1,0,0)
        // back wall  z=lo.z: normal = +z = (0,0,1)
        // front wall z=hi.z: normal = -z = (0,0,-1)
        // floor      y=lo.y: normal = +y = (0,1,0)
        let d_left  = pos_bl.x - lo_bl.x;
        let d_right = hi_bl.x - pos_bl.x;
        let d_back  = pos_bl.z - lo_bl.z;
        let d_front = hi_bl.z - pos_bl.z;
        let d_floor = pos_bl.y - lo_bl.y;

        // For each close plane, blend n toward its inward normal (eye-space).
        // Plane normal in eye space = (eye_to_world 3x3)^{-1} * box_rot * plane_normal_bl
        //                           = transpose(eye3) * box_rot * plane_normal_bl
        // (eye3 is rotation, so transpose = inverse)
        let eye3_t = transpose(eye3);

        // Allow a small negative tolerance so water reconstructed right at (or
        // slightly past) the glass plane still snaps: smooth_z noise can push
        // pos_bl just outside the wall boundary (d < 0) for genuinely on-glass
        // water fragments, disabling the snap exactly where it is most wanted.
        let neg_tol = -flat_epsilon * 0.5;

        // Track the nearest in-range plane for depth snap (single dominant plane).
        // We pick the plane with smallest d (tightest against the wall) to avoid
        // snapping to two walls simultaneously in a corner.
        var best_d     = flat_epsilon; // threshold — only update if d < this
        var best_t     = 0.0;
        var best_plane = 0; // 1=left,2=right,3=back,4=front,5=floor

        var n_blend = n;

        if d_left < flat_epsilon && d_left >= neg_tol {
            let t = 1.0 - smoothstep(0.0, flat_epsilon, clamp(d_left, 0.0, flat_epsilon));
            let plane_n_eye = normalize(eye3_t * (box_rot * vec3<f32>(1.0, 0.0, 0.0)));
            n_blend = normalize(mix(n_blend, plane_n_eye, flat_strength * t));
            if d_left < best_d { best_d = d_left; best_t = t; best_plane = 1; }
        }
        if d_right < flat_epsilon && d_right >= neg_tol {
            let t = 1.0 - smoothstep(0.0, flat_epsilon, clamp(d_right, 0.0, flat_epsilon));
            let plane_n_eye = normalize(eye3_t * (box_rot * vec3<f32>(-1.0, 0.0, 0.0)));
            n_blend = normalize(mix(n_blend, plane_n_eye, flat_strength * t));
            if d_right < best_d { best_d = d_right; best_t = t; best_plane = 2; }
        }
        if d_back < flat_epsilon && d_back >= neg_tol {
            let t = 1.0 - smoothstep(0.0, flat_epsilon, clamp(d_back, 0.0, flat_epsilon));
            let plane_n_eye = normalize(eye3_t * (box_rot * vec3<f32>(0.0, 0.0, 1.0)));
            n_blend = normalize(mix(n_blend, plane_n_eye, flat_strength * t));
            if d_back < best_d { best_d = d_back; best_t = t; best_plane = 3; }
        }
        if d_front < flat_epsilon && d_front >= neg_tol {
            let t = 1.0 - smoothstep(0.0, flat_epsilon, clamp(d_front, 0.0, flat_epsilon));
            let plane_n_eye = normalize(eye3_t * (box_rot * vec3<f32>(0.0, 0.0, -1.0)));
            n_blend = normalize(mix(n_blend, plane_n_eye, flat_strength * t));
            if d_front < best_d { best_d = d_front; best_t = t; best_plane = 4; }
        }
        if d_floor < flat_epsilon && d_floor >= neg_tol {
            let t = 1.0 - smoothstep(0.0, flat_epsilon, clamp(d_floor, 0.0, flat_epsilon));
            let plane_n_eye = normalize(eye3_t * (box_rot * vec3<f32>(0.0, 1.0, 0.0)));
            n_blend = normalize(mix(n_blend, plane_n_eye, flat_strength * t));
            if d_floor < best_d { best_d = d_floor; best_t = t; best_plane = 5; }
        }
        n = n_blend;

        // --- Depth / silhouette snap ---
        // If a nearest plane was found and depth_strength > threshold, intersect
        // the box-local eye ray with that plane and snap front_z to the hit depth.
        // This makes the silhouette of the water coplanar with the glass, removing
        // the bumpy per-sphere depth variation at the wall.
        if best_plane > 0 && flat_depth > 0.001 && best_t > 0.0 {
            // Ray origin = camera in box-local space; direction = (pos_bl - origin).
            // pos_bl already lies on this exact pixel's eye ray (reconstructed above),
            // so dir_bl is the exact unscaled ray direction.
            let o = cam.box_eye_local.xyz;
            let dir_bl = normalize(pos_bl - o);

            // Axis-aligned plane intersection: t = (plane_coord - origin_coord) / dir_coord
            var ray_t = -1.0; // negative = degenerate/invalid
            if best_plane == 1 { // left  x=lo.x
                let denom = dir_bl.x;
                if abs(denom) > 1.0e-5 { ray_t = (lo_bl.x - o.x) / denom; }
            } else if best_plane == 2 { // right x=hi.x
                let denom = dir_bl.x;
                if abs(denom) > 1.0e-5 { ray_t = (hi_bl.x - o.x) / denom; }
            } else if best_plane == 3 { // back  z=lo.z
                let denom = dir_bl.z;
                if abs(denom) > 1.0e-5 { ray_t = (lo_bl.z - o.z) / denom; }
            } else if best_plane == 4 { // front z=hi.z
                let denom = dir_bl.z;
                if abs(denom) > 1.0e-5 { ray_t = (hi_bl.z - o.z) / denom; }
            } else { // floor  y=lo.y
                let denom = dir_bl.y;
                if abs(denom) > 1.0e-5 { ray_t = (lo_bl.y - o.y) / denom; }
            }

            if ray_t > 0.0 {
                // hit_bl: box-local hit point on the wall plane.
                let hit_bl = o + ray_t * dir_bl;
                // Convert hit_bl back to eye-space depth (front_z = -z_eye).
                // Reverse of the forward transform:
                //   delta_world = box_rot * (hit_bl - cam.box_eye_local.xyz)
                //   pos_eye_flat = eye3_t * delta_world   (world->eye rotation)
                //   front_z_flat = -pos_eye_flat.z
                let hw = box_rot * (hit_bl - o);
                let pos_eye_flat = eye3_t * hw;
                var front_z_flat = -pos_eye_flat.z;

                // Clamp against the local scene/glass depth so the snap never pushes
                // the surface behind solid geometry. Without this, grazing-angle pixels
                // can land front_z_flat > scene_z, which makes the downstream refraction
                // depth guard (scene_z_refr < front_z - 0.02) fire and produce a hard
                // seam at the glass edge. We add a small tolerance (0.05 world units)
                // to allow the snapped surface to sit at the glass face itself.
                let scene_z_at_pixel = textureLoad(scene_depth_tex, pixel, 0).r;
                if scene_z_at_pixel > 0.0 && scene_z_at_pixel < 60000.0 {
                    front_z_flat = min(front_z_flat, scene_z_at_pixel + 0.05);
                }

                if front_z_flat > 0.0 {
                    // Decouple depth snap from best_t for a harder glass-coplanar result:
                    // pixels within the inner half of epsilon (d < 0.5*epsilon) get the
                    // full flat_depth weight so wall-pressed water is truly coplanar;
                    // the outer half still ramps through best_t to feather the band edge.
                    let inner = step(best_d, flat_epsilon * 0.5); // 1.0 if in inner half
                    let snap_w = flat_depth * mix(best_t, 1.0, inner);
                    front_z = mix(front_z, front_z_flat, snap_w);
                }
            }
        }
    }

    // --- Whitewater / foam (also a velocity + roughness proxy) ---
    let whitewater = max(0.0, textureSample(whitewater_tex, thickness_sampler, in.uv).r);
    let speed_fraction = clamp(whitewater / max(thickness, 1.0e-4), 0.0, 1.0);
    let ww_threshold = clamp(params.whitewater.y, 0.0, 1.0);
    let ww_softness = max(params.whitewater.z, 0.01);
    let fast_mask = smoothstep(ww_threshold, min(1.0, ww_threshold + ww_softness), speed_fraction);
    let foam_amount = clamp(params.whitewater.x, 0.0, 1.0) * fast_mask;
    let foam_body = clamp(1.0 - exp(-5.0 * whitewater), 0.0, 1.0);
    let foam = foam_amount * foam_body;

    // --- Local surface curvature (chop) for the roughness model ---
    let zc = load_z(pixel, dims);
    let zl2 = load_z(pixel + vec2<i32>(-1, 0), dims);
    let zr2 = load_z(pixel + vec2<i32>(1, 0), dims);
    let zd2 = load_z(pixel + vec2<i32>(0, -1), dims);
    let zu2 = load_z(pixel + vec2<i32>(0, 1), dims);
    var curv = 0.0;
    if zc < 60000.0 && zl2 < 60000.0 && zr2 < 60000.0 && zd2 < 60000.0 && zu2 < 60000.0 {
        curv = abs((zl2 + zr2 + zu2 + zd2) - 4.0 * zc);
    }
    let n_var = clamp(curv * 4.0, 0.0, 1.0);

    // --- Roughness: base + velocity proxy + chop + foam ---
    var roughness = clamp(
        hero.envc.z + speed_fraction * hero.rough.x + n_var * hero.rough.y + foam * hero.rough.z,
        0.0,
        1.0,
    );
    roughness = mix(roughness, clamp(hero.wallfill.z, 0.0, 1.0), wall_fill_mask);

    // --- Micro-normals: optional screen-space surface "tooth" ---
    var nr = n;
    if hero.micro.x > 0.5 {
        let ms = hero.micro.z;
        let amp = hero.micro.y * (1.0 + speed_fraction * hero.micro.w);
        let jx = sin(in.uv.x * ms) * cos(in.uv.y * ms * 1.3 + 1.7);
        let jy = cos(in.uv.x * ms * 1.1 + 0.6) * sin(in.uv.y * ms);
        nr = normalize(n + vec3<f32>(jx, jy, 0.0) * amp * 0.15);
    }

    // Fresnel (Schlick), view direction is +z in eye space.
    let f0 = hero.refr.w;
    let fresnel = f0 + (1.0 - f0) * pow(clamp(1.0 - nr.z, 0.0, 1.0), 5.0);

    // Refraction UV offset (un-perturbed normal): along the surface normal's xy,
    // scaled by thickness, clamped to a pixel budget so grazing angles don't smear.
    let bend = hero.refr.x * thickness * hero.refr.y;
    var offset_px = n.xy * bend * 90.0;
    let len = length(offset_px);
    let maxo = hero.refr.z;
    if len > maxo && len > 1.0e-5 {
        offset_px = offset_px * (maxo / len);
    }
    let offset_uv = vec2<f32>(offset_px.x / width, offset_px.y / height);
    var refract_uv = clamp(in.uv + offset_uv, vec2<f32>(0.0), vec2<f32>(1.0));

    // Depth guard: refracted texel must sit behind the water front surface.
    let refr_pixel = clamp(
        vec2<i32>(floor(refract_uv * vec2<f32>(width, height))),
        vec2<i32>(0, 0),
        dims - vec2<i32>(1, 1),
    );
    let scene_z_refr = textureLoad(scene_depth_tex, refr_pixel, 0).r;
    let invalid = has_water && scene_z_refr < front_z - 0.02;
    let body_enabled = hero.misc.w > 0.5;
    let use_tint_fallback = invalid && hero.misc.y > 0.5 && body_enabled;
    if invalid && !use_tint_fallback {
        refract_uv = in.uv;
    }

    let bg_direct = textureSample(scene_color_tex, thickness_sampler, in.uv).rgb;
    var bg = textureSample(scene_color_tex, thickness_sampler, refract_uv).rgb;
    if use_tint_fallback {
        bg = hero.tint.rgb;
    }

    // Beer-Lambert absorption of the refracted background through the water.
    let body_gate = select(0.0, 1.0, body_enabled);
    let fill_absorb_scale = mix(1.0, clamp(hero.wallfill.w, 0.0, 1.0), wall_fill_mask) * body_gate;
    let fill_color_scale = mix(1.0, clamp(hero.wallfill.x, 0.0, 1.0), wall_fill_mask) * body_gate;
    let absorption_thickness = thickness * fill_absorb_scale;
    let color_thickness = thickness * fill_color_scale;
    let ext = max(vec3<f32>(0.0), hero.absorb.rgb * hero.absorb.w);
    let trans = exp(-ext * absorption_thickness);
    let bg_through = bg * trans;

    // Water body color, growing with thickness; lit by a fixed key light.
    let key = normalize(vec3<f32>(-0.35, 0.55, 0.75));
    let diffuse = max(0.0, dot(n, key));
    let body_amt = 1.0 - exp(-hero.misc.x * color_thickness);
    let body_col = hero.tint.rgb * (0.6 + 0.4 * diffuse);
    let opacity = clamp(body_amt * (1.0 - hero.tint.w), 0.0, 1.0);
    var color = mix(bg_through, body_col, opacity);

    // --- Environment reflection (v1.15) ---
    // eye-space -> world-space rotation (camera only, box-independent), so the
    // reflected sky/room stays fixed to the world while the box rotates.
    let m3 = mat3x3<f32>(
        cam.eye_to_world[0].xyz,
        cam.eye_to_world[1].xyz,
        cam.eye_to_world[2].xyz,
    );
    let env_ctrl = vec4<f32>(hero.envc.x, hero.envc.y, hero.refl.z, 0.0);
    let r_eye = reflect(vec3<f32>(0.0, 0.0, -1.0), nr);
    let r_world = m3 * r_eye;
    var reflected = env_sample(r_world, env_ctrl, hero.sun);
    // Roughness softening: blend toward an averaged (upward) sky sample.
    let env_avg = env_sample(m3 * vec3<f32>(0.0, 0.0, 1.0), env_ctrl, hero.sun);
    reflected = mix(reflected, env_avg, roughness) * hero.refl.y;
    let fill_refl_scale = mix(1.0, max(hero.wallfill.y, 0.0), wall_fill_mask);
    let refl_amt = clamp(fresnel * hero.refl.x * fill_refl_scale, 0.0, 1.0);
    color = mix(color, reflected, refl_amt);

    // Sun specular highlight along the reflection vector; width follows roughness.
    let sun_dir = normalize(hero.sun.xyz);
    let sun_d = max(dot(r_world, sun_dir), 0.0);
    let shininess = mix(16.0, 600.0, clamp(1.0 - roughness, 0.0, 1.0));
    let sun_spec = pow(sun_d, shininess) * hero.spec.x * hero.sun.w;
    color += vec3<f32>(1.0, 0.96, 0.88) * sun_spec;

    // Whitewater foam over everything.
    color = mix(color, vec3<f32>(0.90, 0.97, 1.0), foam);

    // Debug routing (render.hero.debug_view).
    let dbg = hero.misc.z;
    if dbg > 0.5 {
        if dbg < 1.5 {
            return vec4<f32>(bg_direct, 1.0);
        }
        if dbg < 2.5 {
            let d = textureLoad(scene_depth_tex, pixel, 0).r;
            return vec4<f32>(vec3<f32>(clamp(d / 20.0, 0.0, 1.0)), 1.0);
        }
        if dbg < 3.5 {
            return vec4<f32>(vec3<f32>(clamp(thickness, 0.0, 1.0)), 1.0);
        }
        if dbg < 4.5 {
            return vec4<f32>(0.5 + offset_uv * 8.0, 0.5, 1.0);
        }
        if dbg < 5.5 {
            return vec4<f32>(vec3<f32>(fresnel), 1.0);
        }
        if dbg < 6.5 {
            return vec4<f32>(trans, 1.0);
        }
        if dbg < 7.5 {
            // Final water only (water contribution over black).
            var wonly = mix(vec3<f32>(0.0), body_col, opacity);
            wonly = mix(wonly, reflected, refl_amt);
            wonly += vec3<f32>(1.0, 0.96, 0.88) * sun_spec;
            wonly = mix(vec3<f32>(0.0), wonly, select(0.0, 1.0, has_water));
            return vec4<f32>(wonly, 1.0);
        }
        if dbg < 8.5 {
            // Reflection: the Fresnel-weighted reflected environment.
            return vec4<f32>(reflected * refl_amt, 1.0);
        }
        if dbg < 9.5 {
            // Env only: the procedural skybox along the per-pixel view ray.
            let ndc = vec2<f32>(in.uv.x * 2.0 - 1.0, 1.0 - 2.0 * in.uv.y);
            let aspect2 = width / height;
            let thf = params.params.y;
            let fdir_eye = normalize(vec3<f32>(ndc.x * thf * aspect2, ndc.y * thf, -1.0));
            return vec4<f32>(env_sample(m3 * fdir_eye, env_ctrl, hero.sun), 1.0);
        }
        if dbg < 10.5 {
            // Caustics (debug_view=10): show scene_color after the caustics composite pass
            // has additively painted into it (pass B runs before this composite pass).
            return vec4<f32>(textureSample(scene_color_tex, thickness_sampler, in.uv).rgb, 1.0);
        }
        if dbg < 11.5 {
            // nearest_z (debug_view=11): smoothed front-surface eye distance (post flat-snap),
            // the exact depth the composite reconstructs the normal from. Near=dark, far=white.
            return vec4<f32>(vec3<f32>(clamp(front_z / 6.0, 0.0, 1.0)), 1.0);
        }
        if dbg < 12.5 {
            // whitewater (debug_view=12): the speed-weighted thickness target.
            return vec4<f32>(vec3<f32>(clamp(whitewater, 0.0, 1.0)), 1.0);
        }
        // wallfill_mask (debug_view=13): the screen-space wall-fill sheet coverage mask.
        return vec4<f32>(vec3<f32>(wall_fill_mask), 1.0);
    }

    if !has_water {
        return vec4<f32>(bg_direct, 1.0);
    }
    return vec4<f32>(color, 1.0);
}
