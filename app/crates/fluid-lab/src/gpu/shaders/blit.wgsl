// Fullscreen blit: copy the offscreen scene-color target onto the swapchain so the
// water pass has the rendered background (tank wireframe + clear) to draw over and
// to refract. A single oversized triangle covers the framebuffer; point-sampled via
// textureLoad (no sampler needed). Used only in the marching-cubes water path.

@group(0) @binding(0) var src: texture_2d<f32>;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
};

@vertex
fn vs(@builtin(vertex_index) vi: u32) -> VsOut {
    var p = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 3.0, -1.0),
        vec2<f32>(-1.0,  3.0),
    );
    var out: VsOut;
    out.pos = vec4<f32>(p[vi], 0.0, 1.0);
    return out;
}

@fragment
fn fs(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let c = textureLoad(src, vec2<i32>(frag.xy), 0);
    return vec4<f32>(c.rgb, 1.0);
}
