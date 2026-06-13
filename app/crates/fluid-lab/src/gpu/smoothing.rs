//! Screen-space water depth smoothing. This pass reads nearest positive eye
//! distance with point `textureLoad` reads and writes R16 targets.

use wgpu::util::DeviceExt;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct SmoothUniform {
    axis_radius: [f32; 4], // xy = integer pixel axis as f32, z = radius, w = sigma_spatial
    feature: [f32; 4],     // x = feature_preservation strength (0..1), yzw unused
}

pub struct WaterSmoothRenderer {
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    uniform_x: wgpu::Buffer,
    uniform_y: wgpu::Buffer,
    /// First iteration: reads nearest_z_view.
    bind_x: wgpu::BindGroup,
    bind_y: wgpu::BindGroup,
    /// Subsequent iterations: reads smooth_z_view (the accumulated output).
    bind_x_iter: wgpu::BindGroup,
}

impl WaterSmoothRenderer {
    pub fn new(
        device: &wgpu::Device,
        nearest_z_view: &wgpu::TextureView,
        ping_view: &wgpu::TextureView,
        smooth_z_view: &wgpu::TextureView,
        radius: u32,
        feature: f32,
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
        let (r, sigma) = radius_sigma(radius);
        let uniform_x = create_uniform(device, [1.0, 0.0, r, sigma], feature, "water smooth x uniform");
        let uniform_y = create_uniform(device, [0.0, 1.0, r, sigma], feature, "water smooth y uniform");
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
        let bind_x_iter = create_bind_group(
            device,
            &bind_group_layout,
            smooth_z_view,
            &uniform_x,
            "water smooth x iter bind group",
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
            bind_x_iter,
        }
    }

    pub fn set_views(
        &mut self,
        device: &wgpu::Device,
        nearest_z_view: &wgpu::TextureView,
        ping_view: &wgpu::TextureView,
        smooth_z_view: &wgpu::TextureView,
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
        self.bind_x_iter = create_bind_group(
            device,
            &self.bind_group_layout,
            smooth_z_view,
            &self.uniform_x,
            "water smooth x iter bind group",
        );
    }

    /// Update the bilateral kernel radius (and derived sigma_spatial) and the
    /// feature-preservation strength. Call whenever the `render.hero.smooth_radius`
    /// or `render.hero.feature_preservation` Live settings change.
    pub fn update_radius(&self, queue: &wgpu::Queue, radius: u32, feature: f32) {
        let (r, sigma) = radius_sigma(radius);
        let ux = SmoothUniform {
            axis_radius: [1.0, 0.0, r, sigma],
            feature: [feature, 0.0, 0.0, 0.0],
        };
        let uy = SmoothUniform {
            axis_radius: [0.0, 1.0, r, sigma],
            feature: [feature, 0.0, 0.0, 0.0],
        };
        queue.write_buffer(&self.uniform_x, 0, bytemuck::bytes_of(&ux));
        queue.write_buffer(&self.uniform_y, 0, bytemuck::bytes_of(&uy));
    }

    /// Draw the first X pass (reads nearest_z).
    pub fn draw_x_first(&self, pass: &mut wgpu::RenderPass<'_>) {
        self.draw(pass, &self.bind_x);
    }

    /// Draw an X pass for iteration >= 2 (reads accumulated smooth_z).
    pub fn draw_x_iter(&self, pass: &mut wgpu::RenderPass<'_>) {
        self.draw(pass, &self.bind_x_iter);
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

/// Plain separable Gaussian blur for a screen-space scalar R16 target (not
/// bilateral). Used for **both** the thickness target and the whitewater/foam
/// target — both are per-particle accumulation signals that, left raw, show the
/// individual splats as speckle (a sandy water body for thickness; a field of
/// white foam dots for whitewater). Unlike the depth bilateral filter there is no
/// edge-stop term: blurring across the silhouette is desirable (soft edges,
/// filled inter-splat holes) and the composite still gates visible water on the
/// smoothed depth. The X pass reads the source target and writes the shared
/// scratch (`ping`) target; the Y pass reads `ping` and writes back into the
/// source target in place. It reuses the depth pass's `smooth_z_ping` scratch
/// (each instance runs to completion before the next reuses it), so no extra
/// render target is allocated.
pub struct ThicknessSmoothRenderer {
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    uniform_x: wgpu::Buffer,
    uniform_y: wgpu::Buffer,
    /// X pass: reads the thickness target.
    bind_x: wgpu::BindGroup,
    /// Y pass: reads the shared `ping` scratch target.
    bind_y: wgpu::BindGroup,
}

impl ThicknessSmoothRenderer {
    pub fn new(
        device: &wgpu::Device,
        thickness_view: &wgpu::TextureView,
        ping_view: &wgpu::TextureView,
        radius: u32,
    ) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("thickness smooth shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/thickness_smooth.wgsl").into()),
        });
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("thickness smooth bgl"),
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
        let (r, sigma) = radius_sigma(radius);
        let uniform_x =
            create_uniform(device, [1.0, 0.0, r, sigma], 0.0, "thickness smooth x uniform");
        let uniform_y =
            create_uniform(device, [0.0, 1.0, r, sigma], 0.0, "thickness smooth y uniform");
        let bind_x = create_bind_group(
            device,
            &bind_group_layout,
            thickness_view,
            &uniform_x,
            "thickness smooth x bind group",
        );
        let bind_y = create_bind_group(
            device,
            &bind_group_layout,
            ping_view,
            &uniform_y,
            "thickness smooth y bind group",
        );
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("thickness smooth layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("thickness smooth pipeline"),
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
        thickness_view: &wgpu::TextureView,
        ping_view: &wgpu::TextureView,
    ) {
        self.bind_x = create_bind_group(
            device,
            &self.bind_group_layout,
            thickness_view,
            &self.uniform_x,
            "thickness smooth x bind group",
        );
        self.bind_y = create_bind_group(
            device,
            &self.bind_group_layout,
            ping_view,
            &self.uniform_y,
            "thickness smooth y bind group",
        );
    }

    /// Match the kernel to the (shared) `render.hero.smooth_radius` Live setting.
    /// The plain Gaussian ignores the feature field; it stays 0.
    pub fn update_radius(&self, queue: &wgpu::Queue, radius: u32) {
        let (r, sigma) = radius_sigma(radius);
        let ux = SmoothUniform {
            axis_radius: [1.0, 0.0, r, sigma],
            feature: [0.0; 4],
        };
        let uy = SmoothUniform {
            axis_radius: [0.0, 1.0, r, sigma],
            feature: [0.0; 4],
        };
        queue.write_buffer(&self.uniform_x, 0, bytemuck::bytes_of(&ux));
        queue.write_buffer(&self.uniform_y, 0, bytemuck::bytes_of(&uy));
    }

    /// X pass: reads the thickness target, writes the shared `ping` scratch.
    pub fn draw_x(&self, pass: &mut wgpu::RenderPass<'_>) {
        self.draw(pass, &self.bind_x);
    }

    /// Y pass: reads `ping`, writes back into the thickness target.
    pub fn draw_y(&self, pass: &mut wgpu::RenderPass<'_>) {
        self.draw(pass, &self.bind_y);
    }

    fn draw(&self, pass: &mut wgpu::RenderPass<'_>, bind_group: &wgpu::BindGroup) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
}

/// Derive radius (f32) and sigma_spatial from an integer radius setting.
/// sigma_spatial scales with radius so the Gaussian is never truncated too
/// aggressively: sigma = radius / 2.0 (so the kernel edge is ~2.7 sigma;
/// at radius 3 this gives sigma=1.5, slightly tighter than the old hardcoded 1.65).
fn radius_sigma(radius: u32) -> (f32, f32) {
    let r = radius.max(1) as f32;
    let sigma = (r / 2.0_f32).max(0.5);
    (r, sigma)
}

fn create_uniform(
    device: &wgpu::Device,
    axis_radius: [f32; 4],
    feature: f32,
    label: &str,
) -> wgpu::Buffer {
    let uniform = SmoothUniform {
        axis_radius,
        feature: [feature, 0.0, 0.0, 0.0],
    };
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(label),
        contents: bytemuck::bytes_of(&uniform),
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
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
