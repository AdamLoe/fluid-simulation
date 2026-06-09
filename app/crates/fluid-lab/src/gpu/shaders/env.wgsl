// Shared procedural sky/room environment (v1.15). Sampled by a WORLD-space
// direction; concatenated ahead of both `skybox.wgsl` (the fullscreen world
// background) and `composite.wgsl` (the water's reflected environment), so the
// background and the reflection are the same function and stay coherent.
//
// The direction is world-space and camera-derived, so it is independent of the
// tank's baked rotation: rotating the box never moves the sky, but orbiting the
// camera pans it. `ctrl = (rotation, mode, brightness, _)`, `sun = (dir.xyz,
// intensity)`.

fn env_rotate_y(v: vec3<f32>, a: f32) -> vec3<f32> {
    let c = cos(a);
    let s = sin(a);
    return vec3<f32>(c * v.x + s * v.z, v.y, -s * v.x + c * v.z);
}

fn env_sample(dir_in: vec3<f32>, ctrl: vec4<f32>, sun: vec4<f32>) -> vec3<f32> {
    let rotation = ctrl.x;
    let mode = ctrl.y;
    let brightness = ctrl.z;

    let dir = normalize(env_rotate_y(normalize(dir_in), rotation));
    let up = clamp(dir.y, -1.0, 1.0);

    // Palette per environment mode: 0 = Sky, 1 = Room, 2 = Studio.
    var zenith = vec3<f32>(0.16, 0.33, 0.62);
    var horizon = vec3<f32>(0.74, 0.83, 0.95);
    var ground = vec3<f32>(0.07, 0.08, 0.10);
    if mode > 1.5 {
        zenith = vec3<f32>(0.45, 0.47, 0.52);
        horizon = vec3<f32>(0.93, 0.94, 0.97);
        ground = vec3<f32>(0.20, 0.20, 0.22);
    } else if mode > 0.5 {
        zenith = vec3<f32>(0.18, 0.20, 0.27);
        horizon = vec3<f32>(0.60, 0.58, 0.64);
        ground = vec3<f32>(0.12, 0.10, 0.10);
    }

    let sky = mix(horizon, zenith, smoothstep(0.0, 0.65, up));
    var col = mix(ground, sky, smoothstep(-0.12, 0.04, up));

    // Soft horizon band glow.
    col += vec3<f32>(exp(-abs(up) * 9.0) * 0.10);

    // Room mode: faint vertical "wall panel" bands by azimuth above the horizon.
    if mode > 0.5 && mode < 1.5 {
        let az = atan2(dir.z, dir.x);
        let panels = 0.5 + 0.5 * cos(az * 8.0);
        col *= mix(0.85, 1.12, panels * smoothstep(-0.1, 0.3, up));
    }

    // Sun: a broad glow here; the tight specular disc is added in the composite.
    let sd = normalize(sun.xyz);
    let sdot = max(dot(dir, sd), 0.0);
    col += vec3<f32>(1.0, 0.94, 0.82) * pow(sdot, 80.0) * sun.w;
    col += vec3<f32>(1.0, 0.95, 0.85) * pow(sdot, 8.0) * sun.w * 0.10;

    return max(col * brightness, vec3<f32>(0.0));
}
