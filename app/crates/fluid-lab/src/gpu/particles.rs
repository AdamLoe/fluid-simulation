//! Particle billboard renderer (Phase 0.3). Draws the simulation's particle
//! position buffer directly as instanced camera-facing quads — no readback.

use glam::{Mat4, Vec3};

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct CameraUniform {
    view_proj: [[f32; 4]; 4],
    right: [f32; 4],      // xyz = camera right, w = particle radius
    up: [f32; 4],         // xyz = camera up,    w = speed_scale
    slow_color: [f32; 4], // xyz = RGB, w = optical density
    fast_color: [f32; 4], // xyz = RGB, w = unused
    extra: [f32; 4], // x = edge_inner_radius, y = shading_strength, z = volume_scale, w = simple alpha
}

pub struct ParticleRenderer {
    pipeline: wgpu::RenderPipeline,
    simple_pipeline: wgpu::RenderPipeline,
    thickness_pipeline: wgpu::RenderPipeline,
    camera_buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    base_radius: f32,
    radius_scale: f32,
    particle_volume: f32,
    volume_scale: f32,
    speed_scale: f32,
    slow_color: [f32; 3],
    fast_color: [f32; 3],
    water_optical_density: f32,
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
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let simple_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("simple particles pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_simple"),
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
        let thickness_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("particle thickness pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_thickness"),
                targets: &[
                    Some(wgpu::ColorTargetState {
                        format: wgpu::TextureFormat::R16Float,
                        blend: Some(wgpu::BlendState {
                            color: wgpu::BlendComponent {
                                src_factor: wgpu::BlendFactor::One,
                                dst_factor: wgpu::BlendFactor::One,
                                operation: wgpu::BlendOperation::Add,
                            },
                            alpha: wgpu::BlendComponent {
                                src_factor: wgpu::BlendFactor::One,
                                dst_factor: wgpu::BlendFactor::One,
                                operation: wgpu::BlendOperation::Add,
                            },
                        }),
                        write_mask: wgpu::ColorWrites::ALL,
                    }),
                    Some(wgpu::ColorTargetState {
                        format: wgpu::TextureFormat::R16Float,
                        blend: Some(wgpu::BlendState {
                            color: wgpu::BlendComponent {
                                src_factor: wgpu::BlendFactor::One,
                                dst_factor: wgpu::BlendFactor::One,
                                operation: wgpu::BlendOperation::Min,
                            },
                            alpha: wgpu::BlendComponent {
                                src_factor: wgpu::BlendFactor::One,
                                dst_factor: wgpu::BlendFactor::One,
                                operation: wgpu::BlendOperation::Min,
                            },
                        }),
                        write_mask: wgpu::ColorWrites::ALL,
                    }),
                    Some(wgpu::ColorTargetState {
                        format: wgpu::TextureFormat::R16Float,
                        blend: Some(wgpu::BlendState {
                            color: wgpu::BlendComponent {
                                src_factor: wgpu::BlendFactor::One,
                                dst_factor: wgpu::BlendFactor::One,
                                operation: wgpu::BlendOperation::Add,
                            },
                            alpha: wgpu::BlendComponent {
                                src_factor: wgpu::BlendFactor::One,
                                dst_factor: wgpu::BlendFactor::One,
                                operation: wgpu::BlendOperation::Add,
                            },
                        }),
                        write_mask: wgpu::ColorWrites::ALL,
                    }),
                ],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: depth_format,
                depth_write_enabled: Some(false),
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
            simple_pipeline,
            thickness_pipeline,
            camera_buffer,
            bind_group,
            base_radius: radius,
            radius_scale: 1.0,
            particle_volume: 1.0,
            volume_scale: kernel_volume_scale(radius, 0.6, 1.0),
            speed_scale: 4.0,
            slow_color: [0.10, 0.30, 0.80],
            fast_color: [0.70, 0.92, 1.00],
            water_optical_density: 1.25,
            edge_inner: 0.6,
            shading: 0.25,
        }
    }

    pub fn set_radius_scale(&mut self, s: f32) {
        self.radius_scale = s;
        self.recompute_volume_scale();
    }

    pub fn set_particle_volume(&mut self, volume: f32) {
        self.particle_volume = volume.max(1e-12);
        self.recompute_volume_scale();
    }

    pub fn set_speed_scale(&mut self, s: f32) {
        self.speed_scale = s;
    }

    pub fn set_particle_colors(&mut self, slow: [f32; 3], fast: [f32; 3]) {
        self.slow_color = slow;
        self.fast_color = fast;
    }

    pub fn set_water_optical_density(&mut self, density: f32) {
        self.water_optical_density = density;
    }

    pub fn set_edge_inner(&mut self, v: f32) {
        self.edge_inner = v.clamp(0.0, 0.99);
        self.recompute_volume_scale();
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
                self.water_optical_density,
            ],
            fast_color: [
                self.fast_color[0],
                self.fast_color[1],
                self.fast_color[2],
                0.0,
            ],
            extra: [self.edge_inner, self.shading, self.volume_scale, 1.0],
        };
        queue.write_buffer(&self.camera_buffer, 0, bytemuck::bytes_of(&u));
    }

    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>, particle_count: u32) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        // 6 vertices per particle, instanced.
        pass.draw(0..6, 0..particle_count);
    }

    pub fn draw_simple(&self, pass: &mut wgpu::RenderPass<'_>, particle_count: u32) {
        pass.set_pipeline(&self.simple_pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.draw(0..6, 0..particle_count);
    }

    pub fn draw_thickness(&self, pass: &mut wgpu::RenderPass<'_>, particle_count: u32) {
        pass.set_pipeline(&self.thickness_pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.draw(0..6, 0..particle_count);
    }

    fn recompute_volume_scale(&mut self) {
        let radius = (self.base_radius * self.radius_scale).max(1e-6);
        self.volume_scale = kernel_volume_scale(radius, self.edge_inner, self.particle_volume);
    }
}

fn kernel_volume_scale(radius: f32, edge_inner: f32, particle_volume: f32) -> f32 {
    let kernel_volume = tapered_kernel_volume(radius, edge_inner).max(1e-12);
    particle_volume.max(1e-12) / kernel_volume
}

fn tapered_kernel_volume(radius: f32, edge_inner: f32) -> f32 {
    let edge_inner = edge_inner.clamp(0.0, 0.99);
    const SAMPLES: usize = 256;
    let mut integral = 0.0f32;
    for i in 0..SAMPLES {
        let s = (i as f32 + 0.5) / SAMPLES as f32;
        let nz = (1.0 - s * s).max(0.0).sqrt();
        let edge = 1.0 - smoothstep(edge_inner, 1.0, s);
        integral += s * nz * edge;
    }
    integral /= SAMPLES as f32;
    4.0 * std::f32::consts::PI * radius * radius * radius * integral
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0).max(1e-6)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}
