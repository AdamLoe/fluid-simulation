//! Refractable environment for the hero-water scene prepass. Draws a minimal but
//! real backdrop — a textured tank floor (procedural grid/checker), a gradient
//! backdrop quad, and matte side/back walls — into the offscreen `scene_color`
//! (linear HDR) + `scene_depth` (linear eye distance) targets, ahead of the water
//! passes. Refraction in [`super::composite`] samples these so the floor/backdrop
//! visibly bend through the liquid.

use crate::settings::HeroParams;
use glam::Vec3;
use wgpu::util::DeviceExt;

const FLOOR: f32 = 0.0;
const WALL: f32 = 1.0;
const BACKDROP: f32 = 2.0;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct EnvVertex {
    pos: [f32; 3],
    kind: f32,
    uv: [f32; 2],
    _pad: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct EnvUniform {
    view_proj: [[f32; 4]; 4],
    // x = floor_pattern_scale, y = floor_pattern_strength,
    // z = backdrop_strength, w = wall_visibility
    params: [f32; 4],
    // xyz = camera world eye position in BOX-LOCAL space, w = unused.
    // Stored box-local so that `view_dir = normalize(p - eye_world)` is
    // geometrically correct when p is a box-local wall position.
    eye_world: [f32; 4],
    // x = env_rotation (rad), y = env_mode (0=Sky,1=Room,2=Studio),
    // z = env_brightness, w = unused
    env_ctrl: [f32; 4],
    // xyz = world sun direction (unnormalized from settings), w = sun_intensity
    sun: [f32; 4],
    // Box-local → world rotation: the three columns of the 3x3 rotation matrix
    // (from_quat(box_orient)).  w of each is padding.  Used to rotate the
    // box-local reflection direction into world space before env_sample.
    box_rot_col0: [f32; 4],
    box_rot_col1: [f32; 4],
    box_rot_col2: [f32; 4],
}

pub struct EnvironmentRenderer {
    pipeline: wgpu::RenderPipeline,
    vertex_buffer: wgpu::Buffer,
    vertex_count: u32,
    uniform_buf: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
}

impl EnvironmentRenderer {
    pub fn new(
        device: &wgpu::Device,
        scene_color_format: wgpu::TextureFormat,
        scene_depth_format: wgpu::TextureFormat,
        depth_format: wgpu::TextureFormat,
        lo: [f32; 3],
        hi: [f32; 3],
    ) -> Self {
        // env.wgsl (shared procedural environment) is concatenated ahead of the
        // environment shader so the backdrop uses the same sky/room function as
        // the composite and skybox.
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("environment shader"),
            source: wgpu::ShaderSource::Wgsl(
                concat!(
                    include_str!("shaders/env.wgsl"),
                    include_str!("shaders/environment.wgsl"),
                )
                .into(),
            ),
        });

        let verts = environment_mesh(lo, hi);
        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("environment vertices"),
            contents: bytemuck::cast_slice(&verts),
            usage: wgpu::BufferUsages::VERTEX,
        });

        // Buffer is sized to the full EnvUniform struct (view_proj + params +
        // eye_world + env_ctrl + sun + box_rot = 64+16+16+16+16+48 = 176 bytes).
        let uniform_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("environment uniform"),
            size: std::mem::size_of::<EnvUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Group 0: camera/env uniform
        let bgl0 = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("environment bgl0"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("environment bind group 0"),
            layout: &bgl0,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buf.as_entire_binding(),
            }],
        });

        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("environment layout"),
            bind_group_layouts: &[Some(&bgl0)],
            immediate_size: 0,
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("environment pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<EnvVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32, 2 => Float32x2],
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs"),
                targets: &[
                    Some(wgpu::ColorTargetState {
                        format: scene_color_format,
                        blend: Some(wgpu::BlendState::REPLACE),
                        write_mask: wgpu::ColorWrites::ALL,
                    }),
                    Some(wgpu::ColorTargetState {
                        format: scene_depth_format,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    }),
                ],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                // Faces wind inward toward the camera; disable culling so floor +
                // walls are visible regardless of orientation.
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: depth_format,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        Self {
            pipeline,
            vertex_buffer,
            vertex_count: verts.len() as u32,
            uniform_buf,
            bind_group,
        }
    }

    pub fn update_camera(&self, queue: &wgpu::Queue, view_proj: &glam::Mat4) {
        // Only the view_proj portion changes per frame; params come from set_params.
        queue.write_buffer(
            &self.uniform_buf,
            0,
            bytemuck::cast_slice(&view_proj.to_cols_array()),
        );
    }

    pub fn set_params(&self, queue: &wgpu::Queue, hero: &HeroParams) {
        let params = [
            hero.floor_pattern_scale.max(1.0),
            hero.floor_pattern_strength.clamp(0.0, 1.0),
            hero.backdrop_strength.clamp(0.0, 1.0),
            hero.wall_visibility.clamp(0.0, 1.0),
        ];
        // params live right after the 64-byte mat4x4.
        queue.write_buffer(&self.uniform_buf, 64, bytemuck::cast_slice(&params));
        // EnvUniform layout after view_proj(64) + params(16) = 80:
        //   eye_world  at  80 (16 bytes)
        //   env_ctrl   at  96 (16 bytes)
        //   sun        at 112 (16 bytes)
        //   box_rot    at 128 (48 bytes)
        // env_ctrl: rotation, mode, brightness.
        let env_ctrl: [f32; 4] = [
            hero.environment_rotation,
            hero.environment_mode as f32,
            hero.environment_brightness.max(0.0),
            0.0,
        ];
        queue.write_buffer(&self.uniform_buf, 96, bytemuck::cast_slice(&env_ctrl));
        // sun: direction + intensity.
        let sun: [f32; 4] = [
            hero.sun_direction[0],
            hero.sun_direction[1],
            hero.sun_direction[2],
            hero.sun_intensity.max(0.0),
        ];
        queue.write_buffer(&self.uniform_buf, 112, bytemuck::cast_slice(&sun));
    }

    /// Push the box-local camera eye position and the box-local→world rotation.
    /// Called each frame from render() after the camera matrices are computed.
    ///
    /// `eye_world_local` is the camera eye in box-local space:
    ///   `box_orient.inverse() * (camera_eye_world - box_pos)`
    /// This ensures `view_dir = normalize(wall_pos - eye_world_local)` is
    /// computed in a consistent frame (both sides are box-local).
    ///
    /// `box_rot` is the box-local→world 3×3 rotation matrix (from_quat(box_orient)).
    /// Used in the FS to rotate the box-local reflection direction into world space
    /// before sampling the environment.
    pub fn set_eye_world(
        &self,
        queue: &wgpu::Queue,
        eye_world_local: glam::Vec3,
        box_rot: glam::Mat3,
    ) {
        // eye_world at byte 80: view_proj(64) + params(16) = 80.
        let data: [f32; 4] = [eye_world_local.x, eye_world_local.y, eye_world_local.z, 0.0];
        queue.write_buffer(&self.uniform_buf, 80, bytemuck::cast_slice(&data));
        // box_rot columns at bytes 128, 144, 160.
        let c0: [f32; 4] = [box_rot.x_axis.x, box_rot.x_axis.y, box_rot.x_axis.z, 0.0];
        let c1: [f32; 4] = [box_rot.y_axis.x, box_rot.y_axis.y, box_rot.y_axis.z, 0.0];
        let c2: [f32; 4] = [box_rot.z_axis.x, box_rot.z_axis.y, box_rot.z_axis.z, 0.0];
        queue.write_buffer(&self.uniform_buf, 128, bytemuck::cast_slice(&c0));
        queue.write_buffer(&self.uniform_buf, 144, bytemuck::cast_slice(&c1));
        queue.write_buffer(&self.uniform_buf, 160, bytemuck::cast_slice(&c2));
    }

    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        pass.draw(0..self.vertex_count, 0..1);
    }
}

/// Build the environment mesh: the tank floor (patterned) and two matte walls
/// (back + left). The world background is the procedural skybox ([`super::skybox`]),
/// not a backdrop quad. Both the front (+z, camera side) AND the right (+x) faces
/// are intentionally left open, so they form a clear vertical corner you can look
/// straight down into the tank through.
fn environment_mesh(lo: [f32; 3], hi: [f32; 3]) -> Vec<EnvVertex> {
    let lo = Vec3::from_array(lo);
    let hi = Vec3::from_array(hi);
    let mut v = Vec::with_capacity(6 * 3);

    // Floor (y = lo.y), uv across the x/z footprint.
    push_quad(
        &mut v,
        FLOOR,
        Vec3::new(lo.x, lo.y, lo.z),
        Vec3::new(hi.x, lo.y, lo.z),
        Vec3::new(hi.x, lo.y, hi.z),
        Vec3::new(lo.x, lo.y, hi.z),
    );

    // Back wall (z = lo.z).
    push_quad(
        &mut v,
        WALL,
        Vec3::new(lo.x, lo.y, lo.z),
        Vec3::new(hi.x, lo.y, lo.z),
        Vec3::new(hi.x, hi.y, lo.z),
        Vec3::new(lo.x, hi.y, lo.z),
    );

    // Left wall (x = lo.x). The right wall (x = hi.x) is intentionally omitted so
    // the +x and +z faces form an open corner facing the default camera.
    push_quad(
        &mut v,
        WALL,
        Vec3::new(lo.x, lo.y, lo.z),
        Vec3::new(lo.x, lo.y, hi.z),
        Vec3::new(lo.x, hi.y, hi.z),
        Vec3::new(lo.x, hi.y, lo.z),
    );

    v
}

/// Append two triangles (a,b,c,d wound as a quad) with planar [0,1]^2 uv.
fn push_quad(v: &mut Vec<EnvVertex>, kind: f32, a: Vec3, b: Vec3, c: Vec3, d: Vec3) {
    let va = EnvVertex {
        pos: a.to_array(),
        kind,
        uv: [0.0, 0.0],
        _pad: [0.0, 0.0],
    };
    let vb = EnvVertex {
        pos: b.to_array(),
        kind,
        uv: [1.0, 0.0],
        _pad: [0.0, 0.0],
    };
    let vc = EnvVertex {
        pos: c.to_array(),
        kind,
        uv: [1.0, 1.0],
        _pad: [0.0, 0.0],
    };
    let vd = EnvVertex {
        pos: d.to_array(),
        kind,
        uv: [0.0, 1.0],
        _pad: [0.0, 0.0],
    };
    v.extend_from_slice(&[va, vb, vc, va, vc, vd]);
}
