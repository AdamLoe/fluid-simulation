//! World-background procedural skybox (v1.15). A fullscreen triangle drawn first
//! into the hero-water scene-color prepass, filling the background with the shared
//! procedural environment ([`super::environment`] / `shaders/env.wgsl`) sampled by
//! the per-pixel world-space view ray.
//!
//! Crucially the ray uses the camera's eye->world rotation ONLY — never the tank
//! model matrix — so the background stays fixed to the world when the box rotates
//! (which only changes the source of gravity) and pans only when the camera orbits.
//! The water reflects this same environment, so the two stay coherent.

use crate::settings::HeroParams;
use wgpu::util::DeviceExt;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct SkyUniform {
    eye_to_world: [[f32; 4]; 4],
    cam: [f32; 4],  // x = tan(fov_y/2), y = aspect, z = brightness, w = rotation
    sun: [f32; 4],  // xyz = world sun direction, w = sun intensity
    mode: [f32; 4], // x = environment mode, yzw = unused
}

impl Default for SkyUniform {
    fn default() -> Self {
        Self {
            eye_to_world: glam::Mat4::IDENTITY.to_cols_array_2d(),
            cam: [(50.0_f32.to_radians() * 0.5).tan(), 1.0, 1.0, 0.0],
            sun: [0.4, 0.7, 0.5, 1.2],
            mode: [0.0, 0.0, 0.0, 0.0],
        }
    }
}

pub struct SkyboxRenderer {
    pipeline: wgpu::RenderPipeline,
    uniform_buf: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    uniform: SkyUniform,
    enabled: bool,
}

impl SkyboxRenderer {
    pub fn new(
        device: &wgpu::Device,
        scene_color_format: wgpu::TextureFormat,
        scene_depth_format: wgpu::TextureFormat,
        depth_format: wgpu::TextureFormat,
        hero: &HeroParams,
    ) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("skybox shader"),
            // env.wgsl (shared procedural environment) is concatenated ahead.
            source: wgpu::ShaderSource::Wgsl(
                concat!(
                    include_str!("shaders/env.wgsl"),
                    include_str!("shaders/skybox.wgsl"),
                )
                .into(),
            ),
        });

        let mut uniform = SkyUniform::default();
        apply_hero(&mut uniform, hero);
        let uniform_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("skybox uniform"),
            contents: bytemuck::bytes_of(&uniform),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("skybox bgl"),
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
            label: Some("skybox bind group"),
            layout: &bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buf.as_entire_binding(),
            }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("skybox layout"),
            bind_group_layouts: &[Some(&bgl)],
            immediate_size: 0,
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("skybox pipeline"),
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
                ..Default::default()
            },
            // Background: never writes or rejects depth; geometry draws over it.
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

        Self {
            pipeline,
            uniform_buf,
            bind_group,
            uniform,
            enabled: hero.skybox_enabled,
        }
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// Mirror the Water-tab env settings (brightness/rotation/mode/sun + enable).
    pub fn set_params(&mut self, queue: &wgpu::Queue, hero: &HeroParams) {
        self.enabled = hero.skybox_enabled;
        apply_hero(&mut self.uniform, hero);
        queue.write_buffer(&self.uniform_buf, 0, bytemuck::bytes_of(&self.uniform));
    }

    /// Push the per-frame camera eye->world rotation + aspect (camera only).
    pub fn set_camera(&mut self, queue: &wgpu::Queue, eye_to_world: &glam::Mat4, aspect: f32) {
        self.uniform.eye_to_world = eye_to_world.to_cols_array_2d();
        self.uniform.cam[1] = aspect.max(0.01);
        queue.write_buffer(&self.uniform_buf, 0, bytemuck::bytes_of(&self.uniform));
    }

    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
}

fn apply_hero(u: &mut SkyUniform, hero: &HeroParams) {
    u.cam[2] = hero.environment_brightness.max(0.0);
    u.cam[3] = hero.environment_rotation;
    u.sun = [
        hero.sun_direction[0],
        hero.sun_direction[1],
        hero.sun_direction[2],
        hero.sun_intensity.max(0.0),
    ];
    u.mode[0] = hero.environment_mode as f32;
}
