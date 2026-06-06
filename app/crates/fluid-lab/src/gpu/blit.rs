//! Blitter: a fullscreen triangle that copies the offscreen `scene_color` target
//! onto the swapchain. Used only in the marching-cubes water path so the water pass
//! has a rendered background to draw over and refract (see `gpu/mod.rs::render`).
//!
//! The bind group references the (resize-recreated) `scene_color` view, so it is
//! rebuilt via [`Blitter::rebuild`] whenever that texture is recreated.

pub struct Blitter {
    pl: wgpu::RenderPipeline,
    bgl: wgpu::BindGroupLayout,
    bg: Option<wgpu::BindGroup>,
}

impl Blitter {
    pub fn new(
        device: &wgpu::Device,
        color_format: wgpu::TextureFormat,
        depth_format: wgpu::TextureFormat,
    ) -> Self {
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("blit_bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
        });

        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("blit_layout"),
            bind_group_layouts: &[Some(&bgl)],
            immediate_size: 0,
        });

        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("blit shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/blit.wgsl").into()),
        });

        let pl = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("blit pipeline"),
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
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            // The water pass owns a depth attachment; the blit must declare a
            // compatible depth state but neither tests nor writes depth (it is the
            // background fill, drawn before the depth-tested water).
            depth_stencil: Some(wgpu::DepthStencilState {
                format: depth_format,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        Self { pl, bgl, bg: None }
    }

    /// Point the blit at the current `scene_color` view (call after (re)creating it).
    pub fn rebuild(&mut self, device: &wgpu::Device, scene_color: &wgpu::TextureView) {
        self.bg = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("blit_bg"),
            layout: &self.bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(scene_color),
            }],
        }));
    }

    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        if let Some(bg) = &self.bg {
            pass.set_pipeline(&self.pl);
            pass.set_bind_group(0, bg, &[]);
            pass.draw(0..3, 0..1);
        }
    }
}
