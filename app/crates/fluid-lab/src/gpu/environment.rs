//! Refractable environment for the hero-water scene prepass. Draws a minimal but
//! real backdrop — a textured tank floor (procedural grid/checker), a gradient
//! backdrop quad, and matte side/back walls — into the offscreen `scene_color`
//! (linear HDR) + `scene_depth` (linear eye distance) targets, ahead of the water
//! passes. Refraction in [`super::composite`] samples these so the floor/backdrop
//! visibly bend through the liquid.
//!
//! v1.17: wetness buffer + WetWallUniform bound to group 1 so the wall FS can
//! read per-texel wetness for darkening/gloss/meniscus/contact-shadow. World
//! position is now threaded VS→FS for the index mapping.

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
}

pub struct EnvironmentRenderer {
    pipeline: wgpu::RenderPipeline,
    vertex_buffer: wgpu::Buffer,
    vertex_count: u32,
    uniform_buf: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    /// Bind group for the wetness buffer + WetWallUniform (group 1).
    wetwall_bind_group: wgpu::BindGroup,
}

impl EnvironmentRenderer {
    pub fn new(
        device: &wgpu::Device,
        scene_color_format: wgpu::TextureFormat,
        scene_depth_format: wgpu::TextureFormat,
        depth_format: wgpu::TextureFormat,
        lo: [f32; 3],
        hi: [f32; 3],
        wetwall_uniform_buf: &wgpu::Buffer,
        wetwall_wetness_buf: &wgpu::Buffer,
    ) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("environment shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/environment.wgsl").into()),
        });

        let verts = environment_mesh(lo, hi);
        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("environment vertices"),
            contents: bytemuck::cast_slice(&verts),
            usage: wgpu::BufferUsages::VERTEX,
        });

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

        // Group 1: wetwall uniform (binding 0) + wetness buffer (binding 1)
        let bgl1 = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("environment bgl1 (wetwall)"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let wetwall_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("environment bind group 1 (wetwall)"),
            layout: &bgl1,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wetwall_uniform_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wetwall_wetness_buf.as_entire_binding(),
                },
            ],
        });

        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("environment layout"),
            bind_group_layouts: &[Some(&bgl0), Some(&bgl1)],
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
            wetwall_bind_group,
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
    }

    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_bind_group(1, &self.wetwall_bind_group, &[]);
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
    let va = EnvVertex { pos: a.to_array(), kind, uv: [0.0, 0.0], _pad: [0.0, 0.0] };
    let vb = EnvVertex { pos: b.to_array(), kind, uv: [1.0, 0.0], _pad: [0.0, 0.0] };
    let vc = EnvVertex { pos: c.to_array(), kind, uv: [1.0, 1.0], _pad: [0.0, 0.0] };
    let vd = EnvVertex { pos: d.to_array(), kind, uv: [0.0, 1.0], _pad: [0.0, 0.0] };
    v.extend_from_slice(&[va, vb, vc, va, vc, vd]);
}
