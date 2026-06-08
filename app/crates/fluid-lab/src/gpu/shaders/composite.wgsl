struct Params {
    tint_density: vec4<f32>,
    params: vec4<f32>,      // x = shading, y = tan(fov_y/2), z = width, w = height
    whitewater: vec4<f32>,
};

struct Hero {
    refr: vec4<f32>,   // x = effective strength, y = thickness scale, z = max offset px, w = f0
    absorb: vec4<f32>, // rgb = absorption color, w = absorption strength
    tint: vec4<f32>,   // rgb = base tint, w = transparency
    misc: vec4<f32>,   // x = deep darkening, y = invalid fallback, z = debug view, w = mode enabled
};

@group(0) @binding(0) var thickness_sampler: sampler;
@group(0) @binding(1) var thickness_tex: texture_2d<f32>;
@group(0) @binding(2) var whitewater_tex: texture_2d<f32>;
@group(0) @binding(3) var smoothed_z_tex: texture_2d<f32>;
@group(0) @binding(4) var<uniform> params: Params;
@group(0) @binding(5) var<uniform> hero: Hero;
@group(0) @binding(6) var scene_color_tex: texture_2d<f32>;
@group(0) @binding(7) var scene_depth_tex: texture_2d<f32>;

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

fn water_normal(pixel: vec2<i32>, dims: vec2<i32>) -> vec3<f32> {
    let c = load_z(pixel, dims);
    if c >= 60000.0 {
        return vec3<f32>(0.0, 0.0, 1.0);
    }

    let l_raw = load_z(pixel + vec2<i32>(-1, 0), dims);
    let r_raw = load_z(pixel + vec2<i32>(1, 0), dims);
    let d_raw = load_z(pixel + vec2<i32>(0, -1), dims);
    let u_raw = load_z(pixel + vec2<i32>(0, 1), dims);
    let l = select(l_raw, c, l_raw >= 60000.0);
    let r = select(r_raw, c, r_raw >= 60000.0);
    let d = select(d_raw, c, d_raw >= 60000.0);
    let u = select(u_raw, c, u_raw >= 60000.0);

    let width = max(params.params.z, 1.0);
    let height = max(params.params.w, 1.0);
    let tan_half_fovy = params.params.y;
    let aspect = width / height;
    let world_per_px_y = max(1.0e-4, 2.0 * c * tan_half_fovy / height);
    let world_per_px_x = max(1.0e-4, 2.0 * c * tan_half_fovy * aspect / width);
    let dzdx = (r - l) * 0.5 / world_per_px_x;
    let dzdy = (u - d) * 0.5 / world_per_px_y;

    return normalize(vec3<f32>(-dzdx, -dzdy, 1.0));
}

@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    let width = max(params.params.z, 1.0);
    let height = max(params.params.w, 1.0);
    let dims_u = textureDimensions(smoothed_z_tex);
    let dims = vec2<i32>(i32(dims_u.x), i32(dims_u.y));
    let pixel = clamp(vec2<i32>(floor(in.clip.xy)), vec2<i32>(0, 0), dims - vec2<i32>(1, 1));

    let thickness = max(0.0, textureSample(thickness_tex, thickness_sampler, in.uv).r);
    let front_z = load_z(pixel, dims);
    let has_water = thickness > 1.0e-4 && front_z < 60000.0;
    let n = water_normal(pixel, dims);

    // Fresnel (Schlick), view direction is +z in eye space.
    let f0 = hero.refr.w;
    let fresnel = f0 + (1.0 - f0) * pow(clamp(1.0 - n.z, 0.0, 1.0), 5.0);

    // Refraction UV offset: along the surface normal's xy, scaled by thickness,
    // clamped to a pixel budget so grazing angles don't smear.
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
    let use_tint_fallback = invalid && hero.misc.y > 0.5;
    if invalid && hero.misc.y < 0.5 {
        refract_uv = in.uv;
    }

    let bg_direct = textureSample(scene_color_tex, thickness_sampler, in.uv).rgb;
    var bg = textureSample(scene_color_tex, thickness_sampler, refract_uv).rgb;
    if use_tint_fallback {
        bg = hero.tint.rgb;
    }

    // Beer-Lambert absorption of the refracted background through the water.
    let ext = max(vec3<f32>(0.0), hero.absorb.rgb * hero.absorb.w);
    let trans = exp(-ext * thickness);
    let bg_through = bg * trans;

    // Water body color, growing with thickness; lit by a fixed key light.
    let key = normalize(vec3<f32>(-0.35, 0.55, 0.75));
    let diffuse = max(0.0, dot(n, key));
    let refl = reflect(-key, n);
    let spec = pow(max(0.0, refl.z), 48.0);
    let body_amt = 1.0 - exp(-hero.misc.x * thickness);
    let body_col = hero.tint.rgb * (0.6 + 0.4 * diffuse);
    let opacity = clamp(body_amt * (1.0 - hero.tint.w), 0.0, 1.0);
    var color = mix(bg_through, body_col, opacity);

    // Fresnel reflection of a sky-ish constant (env reflection lands in v1.15).
    let sky = vec3<f32>(0.55, 0.72, 0.95);
    color = mix(color, sky, fresnel * 0.5);
    color += vec3<f32>(1.0, 0.96, 0.86) * (spec * 0.5);

    // Whitewater foam.
    let whitewater = max(0.0, textureSample(whitewater_tex, thickness_sampler, in.uv).r);
    let speed_fraction = clamp(whitewater / max(thickness, 1.0e-4), 0.0, 1.0);
    let threshold = clamp(params.whitewater.y, 0.0, 1.0);
    let softness = max(params.whitewater.z, 0.01);
    let fast_mask = smoothstep(threshold, min(1.0, threshold + softness), speed_fraction);
    let amount = clamp(params.whitewater.x, 0.0, 1.0) * fast_mask;
    let foam_body = clamp(1.0 - exp(-5.0 * whitewater), 0.0, 1.0);
    let foam = amount * foam_body;
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
        // Final water only (water contribution over black).
        var wonly = mix(vec3<f32>(0.0), body_col, opacity)
            + vec3<f32>(1.0, 0.96, 0.86) * (spec * 0.5)
            + sky * (fresnel * 0.5);
        wonly = mix(vec3<f32>(0.0), wonly, select(0.0, 1.0, has_water));
        return vec4<f32>(wonly, 1.0);
    }

    if !has_water {
        return vec4<f32>(bg_direct, 1.0);
    }
    return vec4<f32>(color, 1.0);
}
