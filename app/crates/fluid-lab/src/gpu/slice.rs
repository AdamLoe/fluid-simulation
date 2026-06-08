//! Grid-slice debug renderer (Phase 1.0.3+). Draws a flat XY cross-section at
//! k = nz/2 of the MAC-grid. Supports three inspection modes:
//!   0 = cell-type  (solid gray / liquid blue / air hidden)
//!   1 = pressure   (liquid only; diverging blue→white→red)
//!   2 = speed      (liquid only; sequential blue→cyan→yellow→red)
//!
//! No readback — reads live GPU buffers directly as read-only storage bindings.

use glam::Mat4;

/// Uniform struct uploaded each frame (64 + 16 + 16 + 16 = 112 bytes).
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct SliceUniform {
    view_proj: [[f32; 4]; 4], // 64 bytes
    /// x=nx, y=ny, z=nz (per-axis cell counts), w=0  (16 bytes)
    dims: [u32; 4],
    /// x=slice_k, y=h (cell size), z=mode (0/1/2 as f32), w=0  (16 bytes)
    grid: [f32; 4],
    origin: [f32; 4], // xyz=world origin, w=0  (16 bytes)
}

pub struct SliceRenderer {
    pipeline: wgpu::RenderPipeline,
    uniform_buf: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    /// Per-axis cell counts; the XY slice has nx*ny instances.
    nx: u32,
    ny: u32,
    nz: u32,
    /// Cell size h = sim::H (uniform across axes).
    h: f32,
    /// World-space grid origin (centered, from tank_bounds()).
    origin: [f32; 3],
    /// Current inspection mode (0=cell-type, 1=pressure, 2=speed).
    mode: u32,
}

impl SliceRenderer {
    pub fn new(
        device: &wgpu::Device,
        color_format: wgpu::TextureFormat,
        depth_format: wgpu::TextureFormat,
        cell_type: &wgpu::Buffer,
        pressure: &wgpu::Buffer,
        u_vel: &wgpu::Buffer,
        v_vel: &wgpu::Buffer,
        w_vel: &wgpu::Buffer,
        nx: u32,
        ny: u32,
        nz: u32,
        h: f32,
        origin: [f32; 3],
    ) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("slice shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/slice.wgsl").into()),
        });

        let uniform_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("slice uniform"),
            size: std::mem::size_of::<SliceUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Explicit BGL:
        //   binding 0: uniform              (VERTEX)
        //   binding 1: cell_type  storage   (VERTEX, read-only)
        //   binding 2: pressure   storage   (VERTEX, read-only)
        //   binding 3: u_vel      storage   (VERTEX, read-only)
        //   binding 4: v_vel      storage   (VERTEX, read-only)
        //   binding 5: w_vel      storage   (VERTEX, read-only)
        let storage_entry = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::VERTEX,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };

        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("slice bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                storage_entry(1), // cell_type
                storage_entry(2), // pressure
                storage_entry(3), // u_vel
                storage_entry(4), // v_vel
                storage_entry(5), // w_vel
            ],
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("slice bind group"),
            layout: &bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: cell_type.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: pressure.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: u_vel.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: v_vel.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: w_vel.as_entire_binding(),
                },
            ],
        });

        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("slice layout"),
            bind_group_layouts: &[Some(&bgl)],
            immediate_size: 0,
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("slice pipeline"),
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
                    format: color_format,
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
            uniform_buf,
            bind_group,
            nx,
            ny,
            nz,
            h,
            origin,
            mode: 0,
        }
    }

    /// Set the inspection mode (0=cell-type, 1=pressure, 2=speed).
    pub fn set_mode(&mut self, m: u32) {
        self.mode = m.min(2);
    }

    /// Write the uniform buffer before the render pass begins.
    pub fn update_camera(&self, queue: &wgpu::Queue, view_proj: &Mat4) {
        let slice_k = self.nz / 2;
        let u = SliceUniform {
            view_proj: view_proj.to_cols_array_2d(),
            dims: [self.nx, self.ny, self.nz, 0],
            grid: [slice_k as f32, self.h, self.mode as f32, 0.0],
            origin: [self.origin[0], self.origin[1], self.origin[2], 0.0],
        };
        queue.write_buffer(&self.uniform_buf, 0, bytemuck::bytes_of(&u));
    }

    /// Draw the slice inside an active render pass.
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        // 6 vertices per quad, nx*ny instances (one per cell in the XY slice).
        pass.draw(0..6, 0..(self.nx * self.ny));
    }
}
