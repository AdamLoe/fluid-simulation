//! v1.16 Approximate screen-space caustics.
//!
//! Two render passes inserted into the Water-mode pipeline:
//!
//! (A) **Generation pass** (`draw_generate`) — half-res R16Float. Reads
//!     `smooth_z` + `thickness` + history, emits a scalar caustic intensity
//!     driven by surface curvature, sun N·L, and water thickness.
//!
//! (B) **Composite pass** (`draw_composite`) — full-res, additively writes
//!     back into `scene_color`. Reconstructs world hit-pos from `scene_depth`
//!     and gates on floor / back-wall / side-wall receivers before adding the
//!     caustic light.
//!
//! All knobs live under `render.hero.caustics.*`, ride the existing Live batch
//! route (`set_hero_params`), and are stored in `HeroParams`.

use crate::settings::HeroParams;
use wgpu::util::DeviceExt;

// ─── Uniforms ────────────────────────────────────────────────────────────────

/// Uniform for the generation pass (A).
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct GenerateUniform {
    params:    [f32; 4], // x=unused, y=tan(fov_y/2), z=width, w=height
    caustics:  [f32; 4], // x=enabled, y=intensity, z=focus_strength, w=thickness_scale
    caustics2: [f32; 4], // x=max_intensity, y=motion_scale, z=temporal_enabled, w=history_alpha
    sun:       [f32; 4], // xyz=world sun dir, w=sun intensity
    // eye→world rotation for transforming eye-space normals to world space so
    // the N·L dot against the world sun direction is in a consistent frame.
    eye_to_world: [[f32; 4]; 4],
}

/// Uniform for the composite pass (B).
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct CompositeUniform {
    params:   [f32; 4], // x=unused, y=tan(fov_y/2), z=width, w=height
    sun:      [f32; 4], // xyz=world sun dir, w=sun intensity
    tank_lo:  [f32; 4], // xyz=tank lower corner, w=unused
    tank_hi:  [f32; 4], // xyz=tank upper corner, w=unused
    switches: [f32; 4], // x=enabled, y=floor_en, z=back_wall_en, w=side_walls_en
}

/// Camera uniform (eye→world matrix) — same layout as composite.rs::CamUniform.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct CamUniform {
    eye_to_world: [[f32; 4]; 4],
}

// ─── Helper: half-res R16Float target ────────────────────────────────────────

fn create_half_r16(device: &wgpu::Device, full_width: u32, full_height: u32, label: &str)
    -> (wgpu::Texture, wgpu::TextureView)
{
    let w = (full_width / 2).max(1);
    let h = (full_height / 2).max(1);
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::R16Float,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
    (tex, view)
}

// ─── Main struct ─────────────────────────────────────────────────────────────

pub struct CausticsSystem {
    // Generation pass (A)
    gen_pipeline:    wgpu::RenderPipeline,
    gen_bgl:         wgpu::BindGroupLayout,
    gen_uniform_buf: wgpu::Buffer,
    gen_sampler:     wgpu::Sampler,

    // Half-res caustic ping/pong targets.
    // ping: written on even frames (frame_parity=false); pong on odd (true).
    caustic_ping_tex:  wgpu::Texture,
    caustic_ping_view: wgpu::TextureView,
    caustic_pong_tex:  wgpu::Texture,
    caustic_pong_view: wgpu::TextureView,

    // Generation bind groups:
    // ping_bind: reads pong (history) → caller writes ping target
    gen_ping_bind: wgpu::BindGroup,
    // pong_bind: reads ping (history) → caller writes pong target
    gen_pong_bind: wgpu::BindGroup,

    // Composite pass (B)
    comp_pipeline:    wgpu::RenderPipeline,
    comp_bgl:         wgpu::BindGroupLayout,
    comp_uniform_buf: wgpu::Buffer,
    comp_cam_buf:     wgpu::Buffer,
    comp_sampler:     wgpu::Sampler,

    // Composite bind groups (one per ping/pong so it reads the freshly written half).
    comp_ping_bind: wgpu::BindGroup, // reads ping target (used after ping was written)
    comp_pong_bind: wgpu::BindGroup, // reads pong target (used after pong was written)

    // Cached for rebuild on resize / hero update.
    full_width:  u32,
    full_height: u32,
    tank_lo:     [f32; 3],
    tank_hi:     [f32; 3],
    /// false = this frame writes ping; true = writes pong.
    frame_parity: bool,
    /// true once the history buffers contain a valid frame (avoids blending
    /// garbage from the uninitialized opposite ping/pong on the first frame
    /// after construction or resize).
    history_valid: bool,
    /// Cached temporal_enabled value so we can restore the gen uniform after
    /// the first-frame history-valid override suppresses it.
    temporal_enabled_cached: bool,
    /// Cached eye→world matrix so set_hero_params can rebuild gen uniforms
    /// with the correct camera orientation when called outside set_camera.
    eye_to_world: glam::Mat4,
}

impl CausticsSystem {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        device:             &wgpu::Device,
        scene_color_format: wgpu::TextureFormat,
        smooth_z_view:      &wgpu::TextureView,
        thickness_view:     &wgpu::TextureView,
        scene_depth_view:   &wgpu::TextureView,
        hero:               &HeroParams,
        tank_lo:            [f32; 3],
        tank_hi:            [f32; 3],
        width:              u32,
        height:             u32,
    ) -> Self {
        let gen_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("caustics gen sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let comp_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("caustics comp sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        // Half-res ping/pong targets.
        let (caustic_ping_tex, caustic_ping_view) = create_half_r16(device, width, height, "caustic ping");
        let (caustic_pong_tex, caustic_pong_view) = create_half_r16(device, width, height, "caustic pong");

        // ── Generation pipeline ──
        let gen_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("caustics generate shader"),
            source: wgpu::ShaderSource::Wgsl(
                include_str!("shaders/caustics_generate.wgsl").into(),
            ),
        });
        let gen_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("caustics gen bgl"),
            entries: &[
                bgl_sampler(0),
                bgl_texture_nf(1), // smooth_z (non-filterable: textureLoad)
                bgl_texture_f(2),  // thickness (filterable)
                bgl_texture_f(3),  // history (filterable)
                bgl_uniform(4),
            ],
        });
        let gen_uniform_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("caustics gen uniform"),
            contents: bytemuck::bytes_of(&make_gen_uniform(hero, width, height, &glam::Mat4::IDENTITY)),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        // ping_bind reads pong as history.
        let gen_ping_bind = create_gen_bind_group(
            device, &gen_bgl, &gen_sampler,
            smooth_z_view, thickness_view, &caustic_pong_view,
            &gen_uniform_buf,
        );
        // pong_bind reads ping as history.
        let gen_pong_bind = create_gen_bind_group(
            device, &gen_bgl, &gen_sampler,
            smooth_z_view, thickness_view, &caustic_ping_view,
            &gen_uniform_buf,
        );
        let gen_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("caustics gen layout"),
            bind_group_layouts: &[Some(&gen_bgl)],
            immediate_size: 0,
        });
        let gen_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("caustics gen pipeline"),
            layout: Some(&gen_layout),
            vertex: wgpu::VertexState {
                module: &gen_module,
                entry_point: Some("vs"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &gen_module,
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

        // ── Composite pipeline ──
        let comp_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("caustics composite shader"),
            source: wgpu::ShaderSource::Wgsl(
                include_str!("shaders/caustics_composite.wgsl").into(),
            ),
        });
        let comp_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("caustics comp bgl"),
            entries: &[
                bgl_sampler(0),
                bgl_texture_nf(1), // scene_depth (non-filterable)
                bgl_texture_f(2),  // caustic map (filterable, half-res)
                bgl_uniform(3),
                bgl_uniform(4),    // cam
            ],
        });
        let comp_uniform_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("caustics comp uniform"),
            contents: bytemuck::bytes_of(&make_comp_uniform(hero, tank_lo, tank_hi, width, height)),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let comp_cam_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("caustics comp cam uniform"),
            contents: bytemuck::bytes_of(&CamUniform {
                eye_to_world: glam::Mat4::IDENTITY.to_cols_array_2d(),
            }),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        // comp_ping_bind reads caustic_ping (used after ping was written this frame).
        let comp_ping_bind = create_comp_bind_group(
            device, &comp_bgl, &comp_sampler,
            scene_depth_view, &caustic_ping_view,
            &comp_uniform_buf, &comp_cam_buf,
        );
        // comp_pong_bind reads caustic_pong.
        let comp_pong_bind = create_comp_bind_group(
            device, &comp_bgl, &comp_sampler,
            scene_depth_view, &caustic_pong_view,
            &comp_uniform_buf, &comp_cam_buf,
        );
        let comp_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("caustics comp layout"),
            bind_group_layouts: &[Some(&comp_bgl)],
            immediate_size: 0,
        });
        let comp_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("caustics comp pipeline"),
            layout: Some(&comp_layout),
            vertex: wgpu::VertexState {
                module: &comp_module,
                entry_point: Some("vs"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &comp_module,
                entry_point: Some("fs"),
                // ADDITIVE blend: src=ONE, dst=ONE so the shader outputs only the
                // caustic light contribution and the GPU blender adds it to scene_color.
                // This avoids binding scene_color as both a sampled texture and the
                // render-pass color attachment (WebGPU read-write hazard).
                targets: &[Some(wgpu::ColorTargetState {
                    format: scene_color_format,
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
            gen_pipeline,
            gen_bgl,
            gen_uniform_buf,
            gen_sampler,
            caustic_ping_tex,
            caustic_ping_view,
            caustic_pong_tex,
            caustic_pong_view,
            gen_ping_bind,
            gen_pong_bind,
            comp_pipeline,
            comp_bgl,
            comp_uniform_buf,
            comp_cam_buf,
            comp_sampler,
            comp_ping_bind,
            comp_pong_bind,
            full_width: width,
            full_height: height,
            tank_lo,
            tank_hi,
            frame_parity: false,
            history_valid: false,
            temporal_enabled_cached: hero.caustics_temporal_enabled,
            eye_to_world: glam::Mat4::IDENTITY,
        }
    }

    // ─── Per-frame draw calls ─────────────────────────────────────────────

    /// Current frame parity. false = will write ping; true = will write pong.
    pub fn frame_parity(&self) -> bool {
        self.frame_parity
    }

    /// The ping half-res target view.
    pub fn ping_view(&self) -> &wgpu::TextureView {
        &self.caustic_ping_view
    }

    /// The pong half-res target view.
    pub fn pong_view(&self) -> &wgpu::TextureView {
        &self.caustic_pong_view
    }

    /// Returns the half-res target the generation pass WRITES INTO this frame.
    /// frame_parity=false → write ping; true → write pong.
    pub fn gen_target_view(&self) -> &wgpu::TextureView {
        if !self.frame_parity {
            &self.caustic_ping_view
        } else {
            &self.caustic_pong_view
        }
    }

    /// Drop history on camera-motion reset or scene reset. The next frame's
    /// generation pass will output raw current (no blend) and then re-arm.
    pub fn invalidate_history(&mut self) {
        self.history_valid = false;
    }

    /// Draw generation pass (A) into the render pass. The caller must have
    /// created the render pass with `gen_target_view()` as its color attachment.
    /// `queue` is used to disable temporal blending on the first frame after
    /// construction or resize (when history buffers are uninitialized), then
    /// restore the correct value on the second frame.
    /// `cam_reset` additionally suppresses temporal for one frame when the
    /// camera moved beyond the motion threshold.
    pub fn draw_generate(&mut self, queue: &wgpu::Queue, pass: &mut wgpu::RenderPass<'_>, cam_reset: bool) {
        // GenerateUniform layout (each field is a vec4<f32> = 16 bytes):
        //   params[4]    offset 0
        //   caustics[4]  offset 16
        //   caustics2[4] offset 32  — caustics2.z = temporal_enabled at byte 40, .w at 44
        //   sun[4]       offset 48
        //   eye_to_world offset 64
        const TEMPORAL_ENABLED_OFFSET: u64 = 32 + 2 * 4; // caustics2.z = byte 40

        if !self.history_valid || cam_reset {
            // Suppress temporal blend so the uninitialized history buffer doesn't ghost,
            // or when the camera has moved beyond the motion reset threshold.
            let zero: f32 = 0.0;
            queue.write_buffer(&self.gen_uniform_buf, TEMPORAL_ENABLED_OFFSET,
                bytemuck::bytes_of(&zero));
        }

        let bind = if !self.frame_parity {
            &self.gen_ping_bind
        } else {
            &self.gen_pong_bind
        };
        pass.set_pipeline(&self.gen_pipeline);
        pass.set_bind_group(0, bind, &[]);
        pass.draw(0..3, 0..1);
        // Flip parity: next frame the roles reverse.
        self.frame_parity = !self.frame_parity;

        if !self.history_valid {
            self.history_valid = true;
            // Restore the actual temporal_enabled value for the next frame
            // (only when transitioning from invalid → valid; cam_reset restores
            // automatically on the next frame because history_valid is already true).
            if !cam_reset {
                let val: f32 = if self.temporal_enabled_cached { 1.0 } else { 0.0 };
                queue.write_buffer(&self.gen_uniform_buf, TEMPORAL_ENABLED_OFFSET,
                    bytemuck::bytes_of(&val));
            }
        } else if cam_reset {
            // Camera reset on a valid frame: restore temporal for next frame.
            let val: f32 = if self.temporal_enabled_cached { 1.0 } else { 0.0 };
            queue.write_buffer(&self.gen_uniform_buf, TEMPORAL_ENABLED_OFFSET,
                bytemuck::bytes_of(&val));
        }
    }

    /// Draw composite pass (B) additively onto scene_color (LoadOp::Load render pass
    /// targeting scene_color_view). The shader outputs only the caustic light contribution;
    /// the additive blend state (ONE+ONE) adds it to scene_color without binding it.
    ///
    /// Must be called AFTER `draw_generate` this frame (parity has already flipped).
    pub fn draw_composite(&self, pass: &mut wgpu::RenderPass<'_>) {
        // After draw_generate flipped parity: if parity is now true, we just wrote ping.
        let bind = if self.frame_parity {
            &self.comp_ping_bind
        } else {
            &self.comp_pong_bind
        };
        pass.set_pipeline(&self.comp_pipeline);
        pass.set_bind_group(0, bind, &[]);
        pass.draw(0..3, 0..1);
    }

    // ─── Live-setting updates ─────────────────────────────────────────────

    /// Mirror hero params into both uniforms (called from `set_hero_params`).
    pub fn set_hero_params(&mut self, queue: &wgpu::Queue, hero: &HeroParams) {
        // Unified v1.18: caustics temporal is enabled when the master temporal
        // toggle is on and caustic_history is enabled, OR the legacy toggle is on.
        self.temporal_enabled_cached = (hero.temporal_enabled && hero.temporal_caustic_history)
            || hero.caustics_temporal_enabled;
        queue.write_buffer(
            &self.gen_uniform_buf, 0,
            bytemuck::bytes_of(&make_gen_uniform(
                hero, self.full_width, self.full_height, &self.eye_to_world,
            )),
        );
        queue.write_buffer(
            &self.comp_uniform_buf, 0,
            bytemuck::bytes_of(&make_comp_uniform(
                hero, self.tank_lo, self.tank_hi,
                self.full_width, self.full_height,
            )),
        );
    }

    /// Push the per-frame camera eye→world rotation.
    /// Updates both the composite cam buffer (pass B world-pos reconstruction)
    /// and the generation uniform (pass A N·L coordinate-frame fix).
    /// Must be called via `set_camera(&mut self, ...)` each frame.
    pub fn set_camera(&self, queue: &wgpu::Queue, eye_to_world: &glam::Mat4) {
        let cam = CamUniform { eye_to_world: eye_to_world.to_cols_array_2d() };
        queue.write_buffer(&self.comp_cam_buf, 0, bytemuck::bytes_of(&cam));
        // Patch only the eye_to_world portion of the gen uniform buffer.
        // Layout: params[4] + caustics[4] + caustics2[4] + sun[4] = 16 floats = 64 bytes.
        // eye_to_world[16] follows at offset 64.
        let mat_bytes: [[f32; 4]; 4] = eye_to_world.to_cols_array_2d();
        queue.write_buffer(&self.gen_uniform_buf, 64, bytemuck::bytes_of(&mat_bytes));
    }

    /// Cache the eye→world matrix so `set_hero_params` can rebuild the full gen
    /// uniform with the correct frame when called between camera updates.
    pub fn cache_eye_to_world(&mut self, eye_to_world: &glam::Mat4) {
        self.eye_to_world = *eye_to_world;
    }

    /// Rebind only the smooth_z and thickness input views in the generation bind
    /// groups, without touching the caustic ping/pong targets or their parity.
    /// Call this each frame after the temporal system's draw() flips parity so
    /// the generation pass reads the freshly-stabilized smooth_z and thickness.
    pub fn rebind_input_views(
        &mut self,
        device:        &wgpu::Device,
        smooth_z_view: &wgpu::TextureView,
        thickness_view: &wgpu::TextureView,
    ) {
        // gen_ping_bind reads pong as history; update only smooth_z + thickness.
        self.gen_ping_bind = create_gen_bind_group(
            device, &self.gen_bgl, &self.gen_sampler,
            smooth_z_view, thickness_view, &self.caustic_pong_view,
            &self.gen_uniform_buf,
        );
        self.gen_pong_bind = create_gen_bind_group(
            device, &self.gen_bgl, &self.gen_sampler,
            smooth_z_view, thickness_view, &self.caustic_ping_view,
            &self.gen_uniform_buf,
        );
    }

    // ─── Resize ──────────────────────────────────────────────────────────

    /// Rebuild all size-dependent textures and bind groups after a canvas resize.
    /// Caller MUST call `set_hero_params` afterwards to refresh the size in uniforms.
    #[allow(clippy::too_many_arguments)]
    pub fn set_views(
        &mut self,
        device:           &wgpu::Device,
        smooth_z_view:    &wgpu::TextureView,
        thickness_view:   &wgpu::TextureView,
        scene_depth_view: &wgpu::TextureView,
        width:            u32,
        height:           u32,
    ) {
        self.full_width  = width;
        self.full_height = height;

        // Re-create half-res targets.
        let (pt, pv) = create_half_r16(device, width, height, "caustic ping");
        let (qt, qv) = create_half_r16(device, width, height, "caustic pong");
        self.caustic_ping_tex  = pt;
        self.caustic_ping_view = pv;
        self.caustic_pong_tex  = qt;
        self.caustic_pong_view = qv;

        // Rebuild gen bind groups.
        self.gen_ping_bind = create_gen_bind_group(
            device, &self.gen_bgl, &self.gen_sampler,
            smooth_z_view, thickness_view, &self.caustic_pong_view,
            &self.gen_uniform_buf,
        );
        self.gen_pong_bind = create_gen_bind_group(
            device, &self.gen_bgl, &self.gen_sampler,
            smooth_z_view, thickness_view, &self.caustic_ping_view,
            &self.gen_uniform_buf,
        );

        // Rebuild comp bind groups (no scene_color binding — additive blend).
        self.comp_ping_bind = create_comp_bind_group(
            device, &self.comp_bgl, &self.comp_sampler,
            scene_depth_view, &self.caustic_ping_view,
            &self.comp_uniform_buf, &self.comp_cam_buf,
        );
        self.comp_pong_bind = create_comp_bind_group(
            device, &self.comp_bgl, &self.comp_sampler,
            scene_depth_view, &self.caustic_pong_view,
            &self.comp_uniform_buf, &self.comp_cam_buf,
        );

        self.frame_parity = false;
        // History is invalid after resize (new uninitialized ping/pong targets).
        self.history_valid = false;
    }
}

// ─── Uniform constructors ────────────────────────────────────────────────────

fn make_gen_uniform(
    hero:         &HeroParams,
    width:        u32,
    height:       u32,
    eye_to_world: &glam::Mat4,
) -> GenerateUniform {
    GenerateUniform {
        params: [
            0.0,
            (50.0_f32.to_radians() * 0.5).tan(),
            width.max(1) as f32,
            height.max(1) as f32,
        ],
        caustics: [
            if hero.caustics_enabled { 1.0 } else { 0.0 },
            hero.caustics_intensity.max(0.0),
            hero.caustics_focus_strength.max(0.0),
            hero.caustics_thickness_scale.max(0.0),
        ],
        caustics2: [
            hero.caustics_max_intensity.max(0.0),
            hero.caustics_motion_scale.max(0.0),
            // v1.18: caustics temporal blend is unified under hero.temporal.*.
            // When temporal_enabled + caustic_history, use the unified alpha.
            // Fallback to the legacy caustics_temporal_enabled/alpha if the
            // unified system is disabled (backward-compat with old saves).
            if hero.temporal_enabled && hero.temporal_caustic_history {
                1.0
            } else if hero.caustics_temporal_enabled {
                1.0
            } else {
                0.0
            },
            // history_alpha: 0 = all-current, 1 = all-history (v1.18 polarity).
            if hero.temporal_enabled && hero.temporal_caustic_history {
                hero.temporal_history_alpha.clamp(0.0, 1.0)
            } else {
                hero.caustics_temporal_alpha.clamp(0.0, 1.0)
            },
        ],
        sun: [
            hero.sun_direction[0],
            hero.sun_direction[1],
            hero.sun_direction[2],
            hero.sun_intensity.max(0.0),
        ],
        eye_to_world: eye_to_world.to_cols_array_2d(),
    }
}

fn make_comp_uniform(
    hero:    &HeroParams,
    tank_lo: [f32; 3],
    tank_hi: [f32; 3],
    width:   u32,
    height:  u32,
) -> CompositeUniform {
    CompositeUniform {
        params: [
            0.0,
            (50.0_f32.to_radians() * 0.5).tan(),
            width.max(1) as f32,
            height.max(1) as f32,
        ],
        sun: [
            hero.sun_direction[0],
            hero.sun_direction[1],
            hero.sun_direction[2],
            hero.sun_intensity.max(0.0),
        ],
        tank_lo: [tank_lo[0], tank_lo[1], tank_lo[2], 0.0],
        tank_hi: [tank_hi[0], tank_hi[1], tank_hi[2], 0.0],
        switches: [
            if hero.caustics_enabled { 1.0 } else { 0.0 },
            if hero.caustics_floor_enabled { 1.0 } else { 0.0 },
            if hero.caustics_back_wall_enabled { 1.0 } else { 0.0 },
            if hero.caustics_side_walls_enabled { 1.0 } else { 0.0 },
        ],
    }
}

// ─── BGL helpers ─────────────────────────────────────────────────────────────

fn bgl_sampler(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
        count: None,
    }
}

fn bgl_texture_f(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

fn bgl_texture_nf(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: false },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

fn bgl_uniform(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

// ─── Bind group constructors ─────────────────────────────────────────────────

fn create_gen_bind_group(
    device:      &wgpu::Device,
    layout:      &wgpu::BindGroupLayout,
    sampler:     &wgpu::Sampler,
    smooth_z:    &wgpu::TextureView,
    thickness:   &wgpu::TextureView,
    history:     &wgpu::TextureView,
    uniform_buf: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("caustics gen bind group"),
        layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::Sampler(sampler) },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(smooth_z) },
            wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(thickness) },
            wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(history) },
            wgpu::BindGroupEntry { binding: 4, resource: uniform_buf.as_entire_binding() },
        ],
    })
}

fn create_comp_bind_group(
    device:      &wgpu::Device,
    layout:      &wgpu::BindGroupLayout,
    sampler:     &wgpu::Sampler,
    scene_depth: &wgpu::TextureView,
    caustic:     &wgpu::TextureView,
    uniform_buf: &wgpu::Buffer,
    cam_buf:     &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("caustics comp bind group"),
        layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::Sampler(sampler) },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(scene_depth) },
            wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(caustic) },
            wgpu::BindGroupEntry { binding: 3, resource: uniform_buf.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 4, resource: cam_buf.as_entire_binding() },
        ],
    })
}
