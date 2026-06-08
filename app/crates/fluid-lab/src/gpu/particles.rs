//! Particle billboard renderer (Phase 0.3). Draws the simulation's particle
//! position buffer directly as instanced camera-facing quads — no readback.

use glam::{Mat4, Vec3};

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct CameraUniform {
    view_proj: [[f32; 4]; 4],
    right: [f32; 4],      // xyz = camera right, w = particle radius
    up: [f32; 4],         // xyz = camera up,    w = speed_scale
    slow_color: [f32; 4], // xyz = RGB, w = alpha
    fast_color: [f32; 4], // xyz = RGB, w = unused
    extra: [f32; 4],      // x = edge_inner_radius, y = shading_strength, zw = 0
}

pub struct ParticleRenderer {
    pipeline: wgpu::RenderPipeline,
    camera_buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    base_radius: f32,
    radius_scale: f32,
    speed_scale: f32,
    slow_color: [f32; 3],
    fast_color: [f32; 3],
    particle_alpha: f32,
    edge_inner: f32,
    shading: f32,
}

impl ParticleRenderer {
    pub fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        depth_format: wgpu::TextureFormat,
        positions: &wgpu::Buffer,
        radius: f32,
    ) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("particles shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/particles.wgsl").into()),
        });

        let camera_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("particle camera uniform"),
            size: std::mem::size_of::<CameraUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("particles bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    // Camera uniform is read in both VS (transforms/colors) and FS (edge/shading).
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("particles bind group"),
            layout: &bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: camera_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: positions.as_entire_binding(),
                },
            ],
        });

        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("particles layout"),
            bind_group_layouts: &[Some(&bgl)],
            immediate_size: 0,
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("particles pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
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
            camera_buffer,
            bind_group,
            base_radius: radius,
            radius_scale: 1.0,
            speed_scale: 4.0,
            slow_color: [0.10, 0.30, 0.80],
            fast_color: [0.70, 0.92, 1.00],
            particle_alpha: 1.0,
            edge_inner: 0.6,
            shading: 0.5,
        }
    }

    pub fn set_radius_scale(&mut self, s: f32) {
        self.radius_scale = s;
    }

    pub fn set_speed_scale(&mut self, s: f32) {
        self.speed_scale = s;
    }

    pub fn set_particle_look(&mut self, slow: [f32; 3], fast: [f32; 3], alpha: f32) {
        self.slow_color = slow;
        self.fast_color = fast;
        self.particle_alpha = alpha;
    }

    pub fn set_edge_inner(&mut self, v: f32) {
        self.edge_inner = v;
    }

    pub fn set_shading(&mut self, v: f32) {
        self.shading = v;
    }

    pub fn update_camera(&self, queue: &wgpu::Queue, view_proj: &Mat4, right: Vec3, up: Vec3) {
        let u = CameraUniform {
            view_proj: view_proj.to_cols_array_2d(),
            right: [
                right.x,
                right.y,
                right.z,
                self.base_radius * self.radius_scale,
            ],
            up: [up.x, up.y, up.z, self.speed_scale],
            slow_color: [
                self.slow_color[0],
                self.slow_color[1],
                self.slow_color[2],
                self.particle_alpha,
            ],
            fast_color: [
                self.fast_color[0],
                self.fast_color[1],
                self.fast_color[2],
                0.0,
            ],
            extra: [self.edge_inner, self.shading, 0.0, 0.0],
        };
        queue.write_buffer(&self.camera_buffer, 0, bytemuck::bytes_of(&u));
    }

    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>, particle_count: u32) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        // 6 vertices per particle, instanced.
        pass.draw(0..6, 0..particle_count);
    }
}
