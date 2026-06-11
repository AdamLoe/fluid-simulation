//! Wet-wall system (v1.17): persistent wetness field on tank walls.
//!
//! A GPU-only compute pass that maintains a `f32` wetness scalar per wall-surface
//! texel, decay-blended from the current frame's liquid-cell-adjacent-to-solid
//! contact signal. The wetness buffer is read by `environment.wgsl` to darken/gloss
//! walls and render a meniscus highlight and contact shadow.
//!
//! Buffer layout (one `f32` per texel, concatenated):
//!   [0 .. nx_ss*ny_ss)           back wall  (z = lo.z): i in [0,nx_ss), j in [0,ny_ss)
//!   [.. + nz_ss*ny_ss)           left wall  (x = lo.x): k in [0,nz_ss), j in [0,ny_ss)
//!   [.. + nx_ss*nz_ss)           floor      (y = lo.y): i in [0,nx_ss), k in [0,nz_ss)
//!
//! The write-pass mapping and the environment FS mapping share the `WetWallUniform`
//! so the indices are identical.
//!
//! Construction + reset: built once in `GpuContext::new`; rebuilt (fresh zeroed
//! buffer) in `GpuContext::recreate_fluid` to clear on user-facing Reset.

use crate::settings::HeroParams;

const WG: u32 = 64;

/// Shared uniform between the wetwall compute pass and `environment.wgsl`.
/// Both bind this as binding 0.  Must stay in sync with WetWallUniform in
/// wetwall_update.wgsl and the env uniform in environment.wgsl.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct WetWallUniform {
    /// [nx_ss, ny_ss, nz_ss, total_texels_ss]
    /// nx_ss = nx * supersample (supersampled texel dimensions).
    pub dims: [u32; 4],
    /// [back_count_ss (nx_ss*ny_ss), supersample, left_count_ss (nz_ss*ny_ss), nx (original cell grid)]
    /// supersample at .y, original nx at .w — both previously unused zero fields.
    pub face_counts: [u32; 4],
    /// [wetness_decay, dt, contact_gain, enabled (0/1 as f32)]
    pub params: [f32; 4],
    /// tank world-space lower corner, w=unused
    pub tank_lo: [f32; 4],
    /// tank world-space upper corner, w=unused
    pub tank_hi: [f32; 4],
    /// [darkening_strength, gloss_strength, streak_strength, meniscus_enabled]
    pub render0: [f32; 4],
    /// [meniscus_width, meniscus_strength, meniscus_fresnel_boost, contact_shadow_enabled]
    pub render1: [f32; 4],
    /// [contact_shadow_strength, contact_shadow_radius, debug_view (0=off), blur_radius]
    pub render2: [f32; 4],
}

pub struct WetWallSystem {
    /// Flat `f32` storage for the wetness field — one value per wall texel.
    pub wetness_buf: wgpu::Buffer,
    /// Shared uniform buffer (WetWallUniform) written each `record_step`.
    pub uniform_buf: wgpu::Buffer,
    update_pipeline: wgpu::ComputePipeline,
    update_bg: wgpu::BindGroup,
    alloc_dims: [u32; 4],
    alloc_faces: [u32; 4],
    total_texels: u32,
}

impl WetWallSystem {
    pub fn new(
        device: &wgpu::Device,
        cell_type_buf: &wgpu::Buffer,
        nx: u32,
        ny: u32,
        nz: u32,
        tank_lo: [f32; 3],
        tank_hi: [f32; 3],
        supersample: u32,
    ) -> Self {
        let ss = supersample.max(1).min(32);
        let nx_ss = nx * ss;
        let ny_ss = ny * ss;
        let nz_ss = nz * ss;
        let back_count = nx_ss * ny_ss;
        let left_count = nz_ss * ny_ss;
        let floor_count = nx_ss * nz_ss;
        let total_texels = back_count + left_count + floor_count;

        // Zeroed wetness storage (fresh buffer = clean slate on Reset).
        // Size scales with ss^2 per face (both axes supersampled).
        let wetness_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("wetwall wetness"),
            size: (total_texels as u64) * 4,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Seed the uniform with stable dims/tank data immediately so environment.wgsl
        // can read it before the first `record_step`.
        // dims stores the supersampled counts; face_counts.y = ss, .w = original nx.
        let init_uniform = WetWallUniform {
            dims: [nx_ss, ny_ss, nz_ss, total_texels],
            face_counts: [back_count, ss, left_count, nx],
            params: [0.97, 1.0 / 60.0, 1.0, 1.0], // decay, dt, contact_gain, enabled
            tank_lo: [tank_lo[0], tank_lo[1], tank_lo[2], 0.0],
            tank_hi: [tank_hi[0], tank_hi[1], tank_hi[2], 0.0],
            render0: [0.18, 0.25, 0.12, 1.0], // darkening, gloss, streak, meniscus_en
            render1: [0.04, 0.15, 0.12, 1.0], // meniscus_width, strength, fresnel, shadow_en
            render2: [0.15, 0.08, 0.0, 1.0],  // shadow_strength, shadow_radius, debug, blur_radius
        };
        let uniform_buf = {
            use wgpu::util::DeviceExt;
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("wetwall uniform"),
                contents: bytemuck::bytes_of(&init_uniform),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            })
        };

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("wetwall update shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/wetwall_update.wgsl").into()),
        });

        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("wetwall update bgl"),
            entries: &[
                // binding 0: WetWallUniform
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                // binding 1: cell_type (read-only storage)
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                // binding 2: wetness (read_write storage)
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: false },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let update_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("wetwall update bg"),
            layout: &bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: cell_type_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wetness_buf.as_entire_binding(),
                },
            ],
        });

        let pl_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("wetwall update layout"),
            bind_group_layouts: &[Some(&bgl)],
            immediate_size: 0,
        });

        let update_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("wetwall update"),
            layout: Some(&pl_layout),
            module: &shader,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });

        Self {
            wetness_buf,
            uniform_buf,
            update_pipeline,
            update_bg,
            alloc_dims: [nx_ss, ny_ss, nz_ss, total_texels],
            alloc_faces: [back_count, ss, left_count, nx],
            total_texels,
        }
    }

    /// Write the uniform with current hero params + dt, then dispatch the update.
    /// Own encoder, run after `update_diffuse` and before the prepass.
    pub fn record_step(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        dt: f32,
        hero: &HeroParams,
        _nx: u32,
        _ny: u32,
        _nz: u32,
        tank_lo: [f32; 3],
        tank_hi: [f32; 3],
    ) {
        let total = self.total_texels;

        let uniform = WetWallUniform {
            dims: self.alloc_dims,
            face_counts: self.alloc_faces,
            params: [
                hero.wet_wall_wetness_decay.clamp(0.0, 1.0),
                dt,
                hero.wet_wall_contact_gain.max(0.0),
                if hero.wet_wall_enabled { 1.0 } else { 0.0 },
            ],
            tank_lo: [tank_lo[0], tank_lo[1], tank_lo[2], 0.0],
            tank_hi: [tank_hi[0], tank_hi[1], tank_hi[2], 0.0],
            render0: [
                hero.wet_wall_darkening_strength.max(0.0),
                hero.wet_wall_gloss_strength.max(0.0),
                hero.wet_wall_streak_strength.max(0.0),
                if hero.wet_wall_meniscus_enabled {
                    1.0
                } else {
                    0.0
                },
            ],
            render1: [
                hero.wet_wall_meniscus_width.max(0.0),
                hero.wet_wall_meniscus_strength.max(0.0),
                hero.wet_wall_meniscus_fresnel_boost.max(0.0),
                if hero.wet_wall_contact_shadow_enabled {
                    1.0
                } else {
                    0.0
                },
            ],
            render2: [
                hero.wet_wall_contact_shadow_strength.max(0.0),
                hero.wet_wall_contact_shadow_radius.max(0.0),
                hero.wet_wall_debug_view as f32,
                hero.wet_wall_blur.max(0.0),
            ],
        };
        queue.write_buffer(&self.uniform_buf, 0, bytemuck::bytes_of(&uniform));

        if !hero.wet_wall_enabled || total == 0 {
            return;
        }

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("wetwall update"),
        });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("wetwall update"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.update_pipeline);
            pass.set_bind_group(0, &self.update_bg, &[]);
            let groups = (total + WG - 1) / WG;
            pass.dispatch_workgroups(groups, 1, 1);
        }
        queue.submit(std::iter::once(encoder.finish()));
    }

    /// Push render params + enabled/decay/contact_gain when hero changes but dt
    /// isn't available (e.g. set_hero_params outside a frame). The compute pass is
    /// NOT run here; that happens in `record_step`.
    ///
    /// Uniform layout (each field is one vec4 = 16 bytes):
    ///   dims(0) + face_counts(16) + params(32) + tank_lo(48) + tank_hi(64)
    ///   + render0(80) + render1(96) + render2(112)
    ///
    /// We write params at offset 32 (enabled, decay, contact_gain) and the
    /// three render vec4s at offsets 80/96/112.  dt (byte 36) is left at its
    /// last value from record_step — it is only used by the compute pass which
    /// won't run here, so leaving it stale is correct.
    pub fn set_params(&self, queue: &wgpu::Queue, hero: &HeroParams) {
        // Update params: [decay, dt_unchanged, contact_gain, enabled].
        // Write only the three words we own: offset 32 (decay), 40 (contact_gain), 44 (enabled).
        // Doing it as a single vec4 write at offset 32 is simpler and avoids sub-word writes.
        // dt (offset 36) is overwritten here to 0.0; record_step will correct it on the next
        // active frame.  The compute pass is skipped while paused, so stale dt is harmless.
        let params: [f32; 4] = [
            hero.wet_wall_wetness_decay.clamp(0.0, 1.0),
            0.0, // dt placeholder — corrected by record_step on the next active frame
            hero.wet_wall_contact_gain.max(0.0),
            if hero.wet_wall_enabled { 1.0 } else { 0.0 },
        ];
        queue.write_buffer(&self.uniform_buf, 32, bytemuck::cast_slice(&params));

        // render0 begins at byte 80: dims(16)+face_counts(16)+params(16)+tank_lo(16)+tank_hi(16)
        let render0: [f32; 4] = [
            hero.wet_wall_darkening_strength.max(0.0),
            hero.wet_wall_gloss_strength.max(0.0),
            hero.wet_wall_streak_strength.max(0.0),
            if hero.wet_wall_meniscus_enabled {
                1.0
            } else {
                0.0
            },
        ];
        let render1: [f32; 4] = [
            hero.wet_wall_meniscus_width.max(0.0),
            hero.wet_wall_meniscus_strength.max(0.0),
            hero.wet_wall_meniscus_fresnel_boost.max(0.0),
            if hero.wet_wall_contact_shadow_enabled {
                1.0
            } else {
                0.0
            },
        ];
        let render2: [f32; 4] = [
            hero.wet_wall_contact_shadow_strength.max(0.0),
            hero.wet_wall_contact_shadow_radius.max(0.0),
            hero.wet_wall_debug_view as f32,
            hero.wet_wall_blur.max(0.0),
        ];
        queue.write_buffer(&self.uniform_buf, 80, bytemuck::cast_slice(&render0));
        queue.write_buffer(&self.uniform_buf, 96, bytemuck::cast_slice(&render1));
        queue.write_buffer(&self.uniform_buf, 112, bytemuck::cast_slice(&render2));
    }
}
