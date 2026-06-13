//! Screen-space water composite. Samples normalized thickness and smoothed front
//! depth, then composites a lit, refracting Beer-Lambert water over the offscreen
//! `scene_color`/`scene_depth` hero-water prepass. The refracted background is
//! tapped from `scene_color` at a normal-driven UV offset; a depth guard against
//! `scene_depth` keeps geometry in front of the water from smearing.

use crate::settings::HeroParams;
use wgpu::util::DeviceExt;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct CompositeUniform {
    tint_density: [f32; 4], // rgb = water tint, a = optical density
    params: [f32; 4],       // x = shading strength, y = tan(fov_y/2), zw = target size
    whitewater: [f32; 4],   // x = strength, y = threshold, z = softness, w = unused
}

/// Hero-water (Water-tab) parameters, mirrored from the settings registry each
/// time a `render.hero.*` slider changes. `f0` is derived from `ior` here so the
/// shader never sees two disagreeing knobs.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct HeroUniform {
    refr: [f32; 4], // x = effective strength, y = thickness scale, z = max offset px, w = f0
    absorb: [f32; 4], // rgb = absorption color, w = absorption strength
    tint: [f32; 4], // rgb = base tint, w = transparency
    misc: [f32; 4], // x = deep darkening, y = invalid fallback, z = debug view, w = body color enabled
    // --- Environment reflection (v1.15) ---
    refl: [f32; 4], // x = effective reflection strength, y = environment strength, z = environment brightness, w = skybox enabled
    envc: [f32; 4], // x = environment rotation, y = environment mode, z = roughness base, w = unused
    rough: [f32; 4], // x = velocity scale, y = normal-variance scale, z = foam scale, w = unused
    sun: [f32; 4],  // xyz = world sun direction, w = sun intensity
    micro: [f32; 4], // x = enabled, y = strength, z = scale, w = velocity scale
    spec: [f32; 4], // x = specular strength, yzw = unused
    // --- Surface normal quality (v1.19 round-2) ---
    norm: [f32; 4], // x = normal_stencil (as f32), y = normal_smooth_strength, z = feature_preservation, w = unused
}

/// Per-frame camera uniform for composite.wgsl.
/// - eye_to_world: camera-only eye->world rotation (mat4x4, upper-left 3x3 used).
/// - box_eye_local: camera eye in box-local space (xyz, w=unused).
/// - box_rot_col0/1/2: box-local→world rotation columns (mat3 padded to vec4s).
/// - tank_lo/hi: tank bounds in box-local space (xyz, w=unused).
/// - flat: flat_water params (x=strength, y=epsilon, z=depth_strength, w=unused).
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct CamUniform {
    eye_to_world: [[f32; 4]; 4],
    box_eye_local: [f32; 4],
    box_rot_col0: [f32; 4],
    box_rot_col1: [f32; 4],
    box_rot_col2: [f32; 4],
    tank_lo: [f32; 4],
    tank_hi: [f32; 4],
    flat: [f32; 4], // x=strength, y=epsilon, z=depth_strength, w=unused
}

fn hero_uniform(hero: &HeroParams) -> HeroUniform {
    let ratio = (hero.ior - 1.0) / (hero.ior + 1.0);
    let f0 = (ratio * ratio).clamp(0.0, 1.0);
    let effective_strength = if hero.refraction_enabled {
        hero.refraction_strength.max(0.0)
    } else {
        0.0
    };
    let effective_reflection = if hero.reflection_enabled {
        hero.reflection_strength.max(0.0)
    } else {
        0.0
    };
    HeroUniform {
        refr: [
            effective_strength,
            hero.refraction_thickness_scale.max(0.0),
            hero.refraction_max_offset_px.max(0.0),
            f0,
        ],
        absorb: [
            hero.absorption_color[0],
            hero.absorption_color[1],
            hero.absorption_color[2],
            hero.absorption_strength.max(0.0),
        ],
        tint: [
            hero.base_tint[0],
            hero.base_tint[1],
            hero.base_tint[2],
            hero.transparency.clamp(0.0, 1.0),
        ],
        misc: [
            hero.deep_water_darkening.max(0.0),
            hero.invalid_refraction_fallback as f32,
            hero.debug_view as f32,
            if hero.body_color_enabled { 1.0 } else { 0.0 },
        ],
        refl: [
            effective_reflection,
            hero.environment_strength.max(0.0),
            hero.environment_brightness.max(0.0),
            if hero.skybox_enabled { 1.0 } else { 0.0 },
        ],
        envc: [
            hero.environment_rotation,
            hero.environment_mode as f32,
            hero.roughness_base.clamp(0.0, 1.0),
            0.0,
        ],
        rough: [
            hero.roughness_velocity_scale.max(0.0),
            hero.roughness_normal_variance_scale.max(0.0),
            hero.roughness_foam_scale.max(0.0),
            0.0,
        ],
        sun: [
            hero.sun_direction[0],
            hero.sun_direction[1],
            hero.sun_direction[2],
            hero.sun_intensity.max(0.0),
        ],
        micro: [
            if hero.micro_normal_enabled { 1.0 } else { 0.0 },
            hero.micro_normal_strength.max(0.0),
            hero.micro_normal_scale.max(1.0),
            hero.micro_normal_velocity_scale.max(0.0),
        ],
        spec: [hero.specular_strength.max(0.0), 0.0, 0.0, 0.0],
        norm: [
            hero.normal_stencil.clamp(1, 3) as f32,
            hero.normal_smooth_strength.clamp(0.0, 1.0),
            hero.feature_preservation.clamp(0.0, 1.0),
            0.0,
        ],
    }
}

pub struct CompositeRenderer {
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    uniform_buf: wgpu::Buffer,
    hero_buf: wgpu::Buffer,
    cam_buf: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    tint: [f32; 3],
    optical_density: f32,
    shading: f32,
    whitewater_strength: f32,
    whitewater_threshold: f32,
    whitewater_softness: f32,
    size: [f32; 2],
}

impl CompositeRenderer {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        device: &wgpu::Device,
        color_format: wgpu::TextureFormat,
        thickness_view: &wgpu::TextureView,
        whitewater_view: &wgpu::TextureView,
        smoothed_z_view: &wgpu::TextureView,
        scene_color_view: &wgpu::TextureView,
        scene_depth_view: &wgpu::TextureView,
        hero: &HeroParams,
        tint: [f32; 3],
        optical_density: f32,
        shading: f32,
        whitewater_strength: f32,
        whitewater_threshold: f32,
        whitewater_softness: f32,
        size: [u32; 2],
    ) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("water composite shader"),
            // env.wgsl (shared procedural environment) is concatenated ahead of the
            // composite so the reflection uses the same sky/room as the skybox.
            source: wgpu::ShaderSource::Wgsl(
                concat!(
                    include_str!("shaders/env.wgsl"),
                    include_str!("shaders/composite.wgsl"),
                )
                .into(),
            ),
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("thickness sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });
        let uniform = CompositeUniform {
            tint_density: [tint[0], tint[1], tint[2], optical_density.max(0.0)],
            params: [
                shading.max(0.0),
                (50.0_f32.to_radians() * 0.5).tan(),
                size[0].max(1) as f32,
                size[1].max(1) as f32,
            ],
            whitewater: [
                whitewater_strength.clamp(0.0, 1.0),
                whitewater_threshold.clamp(0.0, 1.0),
                whitewater_softness.clamp(0.01, 1.0),
                0.0,
            ],
        };
        let uniform_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("water composite uniform"),
            contents: bytemuck::bytes_of(&uniform),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let hero_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("hero water uniform"),
            contents: bytemuck::bytes_of(&hero_uniform(hero)),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let cam_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("composite camera uniform"),
            contents: bytemuck::bytes_of(&CamUniform {
                eye_to_world: glam::Mat4::IDENTITY.to_cols_array_2d(),
                box_eye_local: [0.0; 4],
                box_rot_col0: [1.0, 0.0, 0.0, 0.0],
                box_rot_col1: [0.0, 1.0, 0.0, 0.0],
                box_rot_col2: [0.0, 0.0, 1.0, 0.0],
                tank_lo: [-1.0, -1.0, -1.0, 0.0],
                tank_hi: [1.0, 1.0, 1.0, 0.0],
                flat: [0.0; 4],
            }),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("water composite bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 5,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                // scene_color (refractable background, filterable).
                wgpu::BindGroupLayoutEntry {
                    binding: 6,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                // scene_depth (eye distance for the refraction depth guard).
                wgpu::BindGroupLayoutEntry {
                    binding: 7,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                // camera eye->world rotation (env reflection).
                wgpu::BindGroupLayoutEntry {
                    binding: 8,
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
        let bind_group = create_bind_group(
            device,
            &bind_group_layout,
            &sampler,
            thickness_view,
            whitewater_view,
            smoothed_z_view,
            &uniform_buf,
            &hero_buf,
            scene_color_view,
            scene_depth_view,
            &cam_buf,
        );
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("water composite layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("water composite pipeline"),
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
                // Opaque: the composite samples scene_color itself and outputs the
                // final pixel (refracted background where there's no water, blended
                // water where there is), so there is no separate blit pass.
                targets: &[Some(wgpu::ColorTargetState {
                    format: color_format,
                    blend: Some(wgpu::BlendState::REPLACE),
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
            sampler,
            uniform_buf,
            hero_buf,
            cam_buf,
            bind_group,
            tint,
            optical_density: optical_density.max(0.0),
            shading: shading.max(0.0),
            whitewater_strength: whitewater_strength.clamp(0.0, 1.0),
            whitewater_threshold: whitewater_threshold.clamp(0.0, 1.0),
            whitewater_softness: whitewater_softness.clamp(0.01, 1.0),
            size: [size[0].max(1) as f32, size[1].max(1) as f32],
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn set_views(
        &mut self,
        device: &wgpu::Device,
        thickness_view: &wgpu::TextureView,
        whitewater_view: &wgpu::TextureView,
        smoothed_z_view: &wgpu::TextureView,
        scene_color_view: &wgpu::TextureView,
        scene_depth_view: &wgpu::TextureView,
    ) {
        self.bind_group = create_bind_group(
            device,
            &self.bind_group_layout,
            &self.sampler,
            thickness_view,
            whitewater_view,
            smoothed_z_view,
            &self.uniform_buf,
            &self.hero_buf,
            scene_color_view,
            scene_depth_view,
            &self.cam_buf,
        );
    }

    /// Mirror the latest Water-tab settings into the hero uniform. Called whenever
    /// a `render.hero.*` slider changes (Live, no pipeline rebuild).
    pub fn set_hero_params(&self, queue: &wgpu::Queue, hero: &HeroParams) {
        queue.write_buffer(&self.hero_buf, 0, bytemuck::bytes_of(&hero_uniform(hero)));
    }

    /// Push the per-frame camera uniforms needed by composite.wgsl.
    /// - `eye_to_world`: camera-only eye->world rotation (env reflection stays world-fixed).
    /// - `eye_world_local`: camera eye in box-local space (for flat-water plane tests).
    /// - `box_rot`: box-local→world rotation (for env sample direction from box-local).
    /// - `tank_lo`/`tank_hi`: tank bounds in box-local space.
    /// - `hero`: hero params carrying flat_water_strength, flat_water_epsilon, and flat_water_depth_strength.
    pub fn set_camera(
        &self,
        queue: &wgpu::Queue,
        eye_to_world: &glam::Mat4,
        eye_world_local: glam::Vec3,
        box_rot: glam::Mat3,
        tank_lo: [f32; 3],
        tank_hi: [f32; 3],
        hero: &crate::settings::HeroParams,
    ) {
        let cam = CamUniform {
            eye_to_world: eye_to_world.to_cols_array_2d(),
            box_eye_local: [eye_world_local.x, eye_world_local.y, eye_world_local.z, 0.0],
            box_rot_col0: [box_rot.x_axis.x, box_rot.x_axis.y, box_rot.x_axis.z, 0.0],
            box_rot_col1: [box_rot.y_axis.x, box_rot.y_axis.y, box_rot.y_axis.z, 0.0],
            box_rot_col2: [box_rot.z_axis.x, box_rot.z_axis.y, box_rot.z_axis.z, 0.0],
            tank_lo: [tank_lo[0], tank_lo[1], tank_lo[2], 0.0],
            tank_hi: [tank_hi[0], tank_hi[1], tank_hi[2], 0.0],
            flat: [
                if hero.wall_contact_enabled {
                    hero.flat_water_strength.clamp(0.0, 1.0)
                } else {
                    0.0
                },
                hero.flat_water_epsilon.max(0.0),
                if hero.wall_contact_enabled {
                    hero.flat_water_depth_strength.clamp(0.0, 1.0)
                } else {
                    0.0
                },
                0.0,
            ],
        };
        queue.write_buffer(&self.cam_buf, 0, bytemuck::bytes_of(&cam));
    }

    pub fn set_tint(&mut self, queue: &wgpu::Queue, tint: [f32; 3]) {
        self.tint = tint;
        self.write_uniform(queue);
    }

    pub fn set_optical_density(&mut self, queue: &wgpu::Queue, density: f32) {
        self.optical_density = density.max(0.0);
        self.write_uniform(queue);
    }

    pub fn set_shading(&mut self, queue: &wgpu::Queue, shading: f32) {
        self.shading = shading.max(0.0);
        self.write_uniform(queue);
    }

    pub fn set_whitewater_strength(&mut self, queue: &wgpu::Queue, strength: f32) {
        self.whitewater_strength = strength.clamp(0.0, 1.0);
        self.write_uniform(queue);
    }

    pub fn set_whitewater_threshold(&mut self, queue: &wgpu::Queue, threshold: f32) {
        self.whitewater_threshold = threshold.clamp(0.0, 1.0);
        self.write_uniform(queue);
    }

    pub fn set_whitewater_softness(&mut self, queue: &wgpu::Queue, softness: f32) {
        self.whitewater_softness = softness.clamp(0.01, 1.0);
        self.write_uniform(queue);
    }

    pub fn set_size(&mut self, queue: &wgpu::Queue, width: u32, height: u32) {
        self.size = [width.max(1) as f32, height.max(1) as f32];
        self.write_uniform(queue);
    }

    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.draw(0..3, 0..1);
    }

    fn write_uniform(&self, queue: &wgpu::Queue) {
        let uniform = CompositeUniform {
            tint_density: [
                self.tint[0],
                self.tint[1],
                self.tint[2],
                self.optical_density,
            ],
            params: [
                self.shading,
                (50.0_f32.to_radians() * 0.5).tan(),
                self.size[0],
                self.size[1],
            ],
            whitewater: [
                self.whitewater_strength,
                self.whitewater_threshold,
                self.whitewater_softness,
                0.0,
            ],
        };
        queue.write_buffer(&self.uniform_buf, 0, bytemuck::bytes_of(&uniform));
    }
}

#[allow(clippy::too_many_arguments)]
fn create_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    thickness_view: &wgpu::TextureView,
    whitewater_view: &wgpu::TextureView,
    smoothed_z_view: &wgpu::TextureView,
    uniform_buf: &wgpu::Buffer,
    hero_buf: &wgpu::Buffer,
    scene_color_view: &wgpu::TextureView,
    scene_depth_view: &wgpu::TextureView,
    cam_buf: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("water composite bind group"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(thickness_view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(whitewater_view),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::TextureView(smoothed_z_view),
            },
            wgpu::BindGroupEntry {
                binding: 4,
                resource: uniform_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 5,
                resource: hero_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 6,
                resource: wgpu::BindingResource::TextureView(scene_color_view),
            },
            wgpu::BindGroupEntry {
                binding: 7,
                resource: wgpu::BindingResource::TextureView(scene_depth_view),
            },
            wgpu::BindGroupEntry {
                binding: 8,
                resource: cam_buf.as_entire_binding(),
            },
        ],
    })
}
