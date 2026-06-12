//! Surface-foam particle system.
//!
//! A render-only GPU particle system that reads the live sim buffers (cell types +
//! MAC face velocities) but never writes them — it conserves no mass and affects no
//! pressure. Particles are born at fast/breaking liquid-air surfaces, advect/age
//! briefly near the surface, and decay. The screen-space whitewater target remains
//! the fallback; this adds persistent surface flecks only.
//!
//! Three passes, all GPU-side (no normal-frame readback):
//!   * emit   — one invocation per grid cell; stochastic spawn into a ring buffer
//!              with an integer-atomic per-frame budget (no float atomics).
//!   * update — one invocation per active slot; age/advect on the surface and
//!              integer-atomic alive recount.
//!   * render — instanced camera-facing billboards over the water composite.
//!
//! `render.diffuse.max_particles` is an ACTIVE CAP within this fixed buffer
//! capacity, so it stays Live (no realloc). The whole system is rebuilt by
//! `GpuContext::recreate_fluid` (which clears it and rebinds the fresh sim buffers).

use crate::settings::DiffuseParams;
use glam::{Mat4, Vec3};

use super::fluid::GpuFluid;

/// Fixed GPU capacity for diffuse particles (48 B each → ~12.6 MB). The Live
/// `render.diffuse.max_particles` cap rides within this.
pub const DIFFUSE_CAPACITY: u32 = 262_144;
/// vec4<f32> slots per particle: pos_type, vel_age, life.
const VEC4S_PER_PARTICLE: u32 = 3;
/// u32 counter slots: 0 ring cursor (persistent), 1 emitted, 2 clamped,
/// 3 alive foam, 4/5 legacy-zero spray/bubble counters, 6/7 pad.
const COUNTERS: u32 = 8;
const WG: u32 = 64;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct DiffuseUniform {
    f0: [f32; 4], // dt, emit_rate, radius, alpha
    f1: [f32; 4], // surf_thresh, surf_gain, _, _
    f2: [f32; 4], // foam_life, _, _, _
    f3: [f32; 4], // _, _, _, _
    u0: [u32; 4], // frame_index, max_particles, emit_budget, random_seed
    u1: [u32; 4], // enabled, _, _, _
}

fn build_uniform(d: &DiffuseParams, dt: f32, frame_index: u32) -> DiffuseUniform {
    let max_p = d.max_particles.clamp(1, DIFFUSE_CAPACITY);
    DiffuseUniform {
        f0: [
            dt,
            d.emit_rate.max(0.0),
            d.radius.max(0.0),
            d.alpha.clamp(0.0, 1.0),
        ],
        f1: [
            d.surface_speed_threshold.max(0.0),
            d.surface_speed_gain.max(0.0),
            0.0,
            0.0,
        ],
        f2: [d.foam_lifetime.max(0.05), 0.0, 0.0, 0.0],
        f3: [0.0, 0.0, 0.0, 0.0],
        u0: [frame_index, max_p, d.emit_budget_per_frame, d.random_seed],
        u1: [u32::from(d.enabled), 0, 0, 0],
    }
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct DiffuseCamera {
    view_proj: [[f32; 4]; 4],
    right: [f32; 4], // xyz right, w radius
    up: [f32; 4],    // xyz up, w peak alpha
    misc: [f32; 4],  // unused padding
}

pub struct DiffuseSystem {
    particles: wgpu::Buffer,
    counters: wgpu::Buffer,
    du_buf: wgpu::Buffer,
    cam_buf: wgpu::Buffer,
    emit_pl: wgpu::ComputePipeline,
    update_pl: wgpu::ComputePipeline,
    emit_bg: wgpu::BindGroup,
    update_bg: wgpu::BindGroup,
    render_pl: wgpu::RenderPipeline,
    render_bg: wgpu::BindGroup,
    cell_count: u32,
    params: DiffuseParams,
}

impl DiffuseSystem {
    pub fn new(
        device: &wgpu::Device,
        color_format: wgpu::TextureFormat,
        depth_format: wgpu::TextureFormat,
        fluid: &GpuFluid,
        params: DiffuseParams,
    ) -> Self {
        // Particle storage (zeroed → all slots start dead-ish: lifetime 0, killed on
        // first update; the render fade also yields alpha 0 for a zeroed slot).
        let particles = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("diffuse particles"),
            size: (DIFFUSE_CAPACITY as u64) * (VEC4S_PER_PARTICLE as u64) * 16,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let counters = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("diffuse counters"),
            size: (COUNTERS as u64) * 4,
            usage: wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::COPY_DST
                | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let du_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("diffuse uniform"),
            size: std::mem::size_of::<DiffuseUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let cam_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("diffuse camera uniform"),
            size: std::mem::size_of::<DiffuseCamera>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let emit_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("diffuse emit"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/diffuse_emit.wgsl").into()),
        });
        let update_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("diffuse update"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/diffuse_update.wgsl").into()),
        });
        let emit_pl = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("diffuse emit"),
            layout: None,
            module: &emit_module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        let update_pl = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("diffuse update"),
            layout: None,
            module: &update_module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });

        // emit + update share the same 8-binding layout against the same buffers.
        let compute_buffers: [&wgpu::Buffer; 8] = [
            fluid.params_buffer(),
            &du_buf,
            fluid.cell_type_buffer(),
            fluid.u_vel_buffer(),
            fluid.v_vel_buffer(),
            fluid.w_vel_buffer(),
            &particles,
            &counters,
        ];
        let make_compute_bg = |label: &str, pl: &wgpu::ComputePipeline| -> wgpu::BindGroup {
            let entries: Vec<wgpu::BindGroupEntry> = compute_buffers
                .iter()
                .enumerate()
                .map(|(b, buf)| wgpu::BindGroupEntry {
                    binding: b as u32,
                    resource: buf.as_entire_binding(),
                })
                .collect();
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(label),
                layout: &pl.get_bind_group_layout(0),
                entries: &entries,
            })
        };
        let emit_bg = make_compute_bg("diffuse emit bg", &emit_pl);
        let update_bg = make_compute_bg("diffuse update bg", &update_pl);

        // Render pipeline.
        let render_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("diffuse render"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/diffuse_render.wgsl").into()),
        });
        let render_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("diffuse render bgl"),
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
        let render_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("diffuse render bg"),
            layout: &render_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: cam_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: particles.as_entire_binding(),
                },
            ],
        });
        let render_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("diffuse render layout"),
            bind_group_layouts: &[Some(&render_bgl)],
            immediate_size: 0,
        });
        let render_pl = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("diffuse render pipeline"),
            layout: Some(&render_layout),
            vertex: wgpu::VertexState {
                module: &render_module,
                entry_point: Some("vs"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &render_module,
                entry_point: Some("fs"),
                // Premultiplied-alpha over the composite (fragment outputs color*a, a).
                targets: &[Some(wgpu::ColorTargetState {
                    format: color_format,
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::One,
                            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                            operation: wgpu::BlendOperation::Add,
                        },
                        alpha: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::One,
                            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                            operation: wgpu::BlendOperation::Add,
                        },
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            // Depth-test against the shared scene depth (environment + wireframe);
            // no depth write so overlapping diffuse particles blend smoothly.
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

        let [nx, ny, nz] = fluid.grid_dims();
        Self {
            particles,
            counters,
            du_buf,
            cam_buf,
            emit_pl,
            update_pl,
            emit_bg,
            update_bg,
            render_pl,
            render_bg,
            cell_count: nx * ny * nz,
            params,
        }
    }

    pub fn set_params(&mut self, params: DiffuseParams) {
        self.params = params;
    }

    pub fn enabled(&self) -> bool {
        self.params.enabled
    }

    /// Active particle cap (clamped into the fixed capacity).
    pub fn active_cap(&self) -> u32 {
        self.params.max_particles.clamp(1, DIFFUSE_CAPACITY)
    }

    pub fn counters_buffer(&self) -> &wgpu::Buffer {
        &self.counters
    }

    /// Run one emit + update step (own command encoder, outside the timestamped sim
    /// passes). Resets the per-frame counters first (the ring cursor persists).
    pub fn record_step(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        dt: f32,
        frame_index: u32,
    ) {
        // Reset per-frame counters [1..=5] (emitted, clamped, alive×3); keep [0]
        // (persistent ring cursor) and [6..7] (pad).
        queue.write_buffer(&self.counters, 4, bytemuck::cast_slice(&[0u32; 5]));
        if !self.params.enabled || dt <= 0.0 {
            return;
        }
        queue.write_buffer(
            &self.du_buf,
            0,
            bytemuck::bytes_of(&build_uniform(&self.params, dt, frame_index)),
        );

        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("diffuse step"),
        });
        {
            let mut p = enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("diffuse emit"),
                timestamp_writes: None,
            });
            p.set_pipeline(&self.emit_pl);
            p.set_bind_group(0, &self.emit_bg, &[]);
            p.dispatch_workgroups(self.cell_count.div_ceil(WG), 1, 1);
        }
        {
            let mut p = enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("diffuse update"),
                timestamp_writes: None,
            });
            p.set_pipeline(&self.update_pl);
            p.set_bind_group(0, &self.update_bg, &[]);
            p.dispatch_workgroups(self.active_cap().div_ceil(WG), 1, 1);
        }
        queue.submit(std::iter::once(enc.finish()));
    }

    pub fn update_camera(&self, queue: &wgpu::Queue, view_proj: &Mat4, right: Vec3, up: Vec3) {
        let cam = DiffuseCamera {
            view_proj: view_proj.to_cols_array_2d(),
            right: [right.x, right.y, right.z, self.params.radius.max(0.0)],
            up: [up.x, up.y, up.z, self.params.alpha.clamp(0.0, 1.0)],
            misc: [0.0, 0.0, 0.0, 0.0],
        };
        queue.write_buffer(&self.cam_buf, 0, bytemuck::bytes_of(&cam));
    }

    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        pass.set_pipeline(&self.render_pl);
        pass.set_bind_group(0, &self.render_bg, &[]);
        pass.draw(0..6, 0..self.active_cap());
    }
}
