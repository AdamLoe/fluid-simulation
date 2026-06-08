//! Screen-space water depth smoothing. This pass reads nearest positive eye
//! distance with point `textureLoad` reads and writes R16 targets.

use wgpu::util::DeviceExt;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct SmoothUniform {
    axis_radius: [f32; 4], // xy = integer pixel axis as f32, z = radius, w = unused
}

pub struct WaterSmoothRenderer {
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    uniform_x: wgpu::Buffer,
    uniform_y: wgpu::Buffer,
    bind_x: wgpu::BindGroup,
    bind_y: wgpu::BindGroup,
}

impl WaterSmoothRenderer {
    pub fn new(
        device: &wgpu::Device,
        nearest_z_view: &wgpu::TextureView,
        ping_view: &wgpu::TextureView,
    ) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("water smooth shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/water_smooth.wgsl").into()),
        });
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("water smooth bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let uniform_x = create_uniform(device, [1.0, 0.0, 3.0, 0.0], "water smooth x uniform");
        let uniform_y = create_uniform(device, [0.0, 1.0, 3.0, 0.0], "water smooth y uniform");
        let bind_x = create_bind_group(
            device,
            &bind_group_layout,
            nearest_z_view,
            &uniform_x,
            "water smooth x bind group",
        );
        let bind_y = create_bind_group(
            device,
            &bind_group_layout,
            ping_view,
            &uniform_y,
            "water smooth y bind group",
        );
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("water smooth layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("water smooth pipeline"),
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
                    format: wgpu::TextureFormat::R16Float,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        Self {
            pipeline,
            bind_group_layout,
            uniform_x,
            uniform_y,
            bind_x,
            bind_y,
        }
    }

    pub fn set_views(
        &mut self,
        device: &wgpu::Device,
        nearest_z_view: &wgpu::TextureView,
        ping_view: &wgpu::TextureView,
    ) {
        self.bind_x = create_bind_group(
            device,
            &self.bind_group_layout,
            nearest_z_view,
            &self.uniform_x,
            "water smooth x bind group",
        );
        self.bind_y = create_bind_group(
            device,
            &self.bind_group_layout,
            ping_view,
            &self.uniform_y,
            "water smooth y bind group",
        );
    }

    pub fn draw_x(&self, pass: &mut wgpu::RenderPass<'_>) {
        self.draw(pass, &self.bind_x);
    }

    pub fn draw_y(&self, pass: &mut wgpu::RenderPass<'_>) {
        self.draw(pass, &self.bind_y);
    }

    fn draw(&self, pass: &mut wgpu::RenderPass<'_>, bind_group: &wgpu::BindGroup) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
}

fn create_uniform(device: &wgpu::Device, axis_radius: [f32; 4], label: &str) -> wgpu::Buffer {
    let uniform = SmoothUniform { axis_radius };
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(label),
        contents: bytemuck::bytes_of(&uniform),
        usage: wgpu::BufferUsages::UNIFORM,
    })
}

fn create_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    source_view: &wgpu::TextureView,
    uniform: &wgpu::Buffer,
    label: &str,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(label),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(source_view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: uniform.as_entire_binding(),
            },
        ],
    })
}
