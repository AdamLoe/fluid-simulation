//! v1.18 Temporal stabilization for screen-space water targets.
//!
//! Maintains ping-pong history for up to three full-res R16Float targets:
//!   - `thickness_view` (water thickness)
//!   - `smooth_z_view`  (smoothed front depth, from which normals are derived)
//!   - `whitewater_view` (foam signal)
//!
//! Each target gets one render pass that blends `current` (written this frame
//! by the thickness/smooth-y pass) with `history` (previous stable frame):
//!   out = mix(current, history, history_alpha)
//!
//! The smooth_z pass also performs depth-reject + normal-reject so a large
//! depth or normal change suppresses ghosting.
//!
//! Camera-motion reset: the caller computes `cam_motion` each frame from
//! `prev_eye_to_world` vs the current `eye_to_world`; when it exceeds
//! `camera_motion_reset_threshold` a `reset_flag` is pushed so all blend
//! passes output raw-current for that one frame.
//!
//! The v1.16 caustics temporal blend is unified through the same
//! `hero.temporal.*` settings; its reset path reuses the existing
//! `CausticsSystem::history_valid` mechanism.

use crate::settings::HeroParams;
use wgpu::util::DeviceExt;

// ── Uniform layout ────────────────────────────────────────────────────────────

/// Per-target blend uniform (one buffer per pass).
/// Layout: params vec4 + size vec4 = 32 bytes.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct BlendUniform {
    /// x = history_alpha, y = depth_threshold, z = normal_threshold, w = reset_flag
    params: [f32; 4],
    /// x = width, y = height, z = unused, w = unused
    size: [f32; 4],
}

// ── Per-target state ──────────────────────────────────────────────────────────

/// One ping-pong stabilized target.
struct TemporalTarget {
    /// The stable (blended) output view, exposed to downstream passes.
    stable_ping_view: wgpu::TextureView,
    stable_pong_view: wgpu::TextureView,
    /// false = writes ping this frame; true = writes pong.
    frame_parity: bool,

    // Bind groups: ping_bind reads pong as history; pong_bind reads ping.
    ping_bind: wgpu::BindGroup,
    pong_bind: wgpu::BindGroup,

    uniform_buf: wgpu::Buffer,
}

impl TemporalTarget {
    fn stable_view(&self) -> &wgpu::TextureView {
        // After draw() flips parity, the freshly written target is the "old" parity.
        // Before draw(), stable = last frame's written target.
        // We expose the ping view when parity will write ping, so the *last written*
        // (stable) is the opposite.
        if self.frame_parity {
            // Next write goes to pong → current stable is ping.
            &self.stable_ping_view
        } else {
            // Next write goes to ping → current stable is pong.
            &self.stable_pong_view
        }
    }

    /// The render target view for the current frame's write.
    fn write_view(&self) -> &wgpu::TextureView {
        if !self.frame_parity {
            &self.stable_ping_view
        } else {
            &self.stable_pong_view
        }
    }

    /// The bind group for the current frame (reads the history = opposite of write).
    fn bind(&self) -> &wgpu::BindGroup {
        if !self.frame_parity {
            &self.ping_bind
        } else {
            &self.pong_bind
        }
    }

    fn flip(&mut self) {
        self.frame_parity = !self.frame_parity;
    }
}

// ── Main struct ───────────────────────────────────────────────────────────────

/// Manages temporal stabilization for thickness, smooth_z, and optionally
/// whitewater.
pub struct TemporalSystem {
    pipeline:         wgpu::RenderPipeline,
    bgl:              wgpu::BindGroupLayout,
    sampler:          wgpu::Sampler,

    thickness:        TemporalTarget,
    smooth_z:         TemporalTarget,
    whitewater:       TemporalTarget,

    full_width:  u32,
    full_height: u32,

    /// true once at least one frame has been written to each target.
    history_valid: bool,
}

impl TemporalSystem {
    /// Allocate the temporal system. `thickness_view`, `smooth_z_view`, and
    /// `whitewater_view` are the **raw** (un-stabilized) inputs written this frame.
    /// The system owns the stable ping-pong outputs.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        device:          &wgpu::Device,
        thickness_view:  &wgpu::TextureView,
        smooth_z_view:   &wgpu::TextureView,
        whitewater_view: &wgpu::TextureView,
        hero:            &HeroParams,
        width:           u32,
        height:          u32,
    ) -> Self {
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("temporal sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("temporal blend shader"),
            source: wgpu::ShaderSource::Wgsl(
                include_str!("shaders/temporal_blend.wgsl").into(),
            ),
        });

        // BGL: sampler(0), current(1, non-filterable), history(2, filterable),
        //       uniform(3), history_z(4, non-filterable — textureLoad in shader to avoid
        //       uniform-control-flow restriction on textureSample inside conditionals)
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("temporal bgl"),
            entries: &[
                bgl_sampler(0),
                bgl_texture_nf(1),
                bgl_texture_f(2),
                bgl_uniform(3),
                bgl_texture_nf(4),
            ],
        });

        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("temporal layout"),
            bind_group_layouts: &[Some(&bgl)],
            immediate_size: 0,
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("temporal blend pipeline"),
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

        let thickness  = build_target(device, &bgl, &sampler, thickness_view,  smooth_z_view, hero, width, height, false);
        let smooth_z   = build_target(device, &bgl, &sampler, smooth_z_view,   smooth_z_view, hero, width, height, true);
        let whitewater = build_target(device, &bgl, &sampler, whitewater_view, smooth_z_view, hero, width, height, false);

        Self {
            pipeline,
            bgl,
            sampler,
            thickness,
            smooth_z,
            whitewater,
            full_width: width,
            full_height: height,
            history_valid: false,
        }
    }

    // ── Stable views exposed to downstream passes ─────────────────────────

    /// The stabilized thickness view (or raw thickness if temporal disabled).
    pub fn stable_thickness(&self) -> &wgpu::TextureView {
        self.thickness.stable_view()
    }

    /// The stabilized smooth_z view (or raw smooth_z if temporal disabled).
    pub fn stable_smooth_z(&self) -> &wgpu::TextureView {
        self.smooth_z.stable_view()
    }

    /// The stabilized whitewater view (or raw whitewater if temporal disabled).
    pub fn stable_whitewater(&self) -> &wgpu::TextureView {
        self.whitewater.stable_view()
    }

    /// Return (thickness, smooth_z, whitewater) stable views. Callers that build
    /// downstream bind groups must re-call this AFTER draw() each frame, because
    /// draw() flips ping-pong parity so stable_view() returns the freshly-written
    /// texture. Binding a stale view at construction is the ping-pong desync bug.
    pub fn stable_views(&self) -> (&wgpu::TextureView, &wgpu::TextureView, &wgpu::TextureView) {
        (self.thickness.stable_view(), self.smooth_z.stable_view(), self.whitewater.stable_view())
    }

    // ── Per-frame draw ────────────────────────────────────────────────────

    /// Record the temporal blend passes into `encoder`. Always runs all three
    /// passes; per-target alpha is baked into uniforms by update_params (0 when
    /// disabled → output = raw current, history still maintained for re-enable).
    pub fn draw(&mut self, encoder: &mut wgpu::CommandEncoder) {
        Self::draw_target_for(encoder, &self.pipeline, &self.thickness);
        self.thickness.flip();
        Self::draw_target_for(encoder, &self.pipeline, &self.smooth_z);
        self.smooth_z.flip();
        Self::draw_target_for(encoder, &self.pipeline, &self.whitewater);
        self.whitewater.flip();
        if !self.history_valid {
            self.history_valid = true;
        }
    }

    /// Draw one temporal blend pass for a single target. The reset_flag and alpha
    /// are already baked into the target's uniform_buf by update_params.
    fn draw_target_for(
        encoder:  &mut wgpu::CommandEncoder,
        pipeline: &wgpu::RenderPipeline,
        target:   &TemporalTarget,
    ) {
        let write_view = target.write_view();
        let bind = target.bind();
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("temporal blend pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: write_view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, bind, &[]);
        pass.draw(0..3, 0..1);
    }

    // ── Params update ─────────────────────────────────────────────────────

    /// Push updated hero params + current reset flag to all per-target uniform
    /// buffers. Call once per frame from GpuContext::render before draw().
    /// When temporal is globally disabled or a per-target flag is off, alpha is
    /// forced to 0 so the pass outputs raw current (history is still valid for
    /// when the feature is re-enabled).
    pub fn update_params(
        &self,
        queue:     &wgpu::Queue,
        hero:      &HeroParams,
        cam_reset: bool,
        width:     u32,
        height:    u32,
    ) {
        let reset_flag: f32 = if cam_reset || !self.history_valid { 1.0 } else { 0.0 };
        let base_alpha = if hero.temporal_enabled {
            hero.temporal_history_alpha.clamp(0.0, 1.0)
        } else {
            0.0_f32
        };
        let w = width.max(1) as f32;
        let h = height.max(1) as f32;

        // Thickness: no depth/normal reject; alpha gated by thickness_history flag.
        let thick_alpha = if hero.temporal_thickness_history { base_alpha } else { 0.0 };
        let thickness_u = BlendUniform {
            params: [thick_alpha, 0.0, 0.0, reset_flag],
            size:   [w, h, 0.0, 0.0],
        };
        // Smooth_z: depth + normal reject enabled; alpha gated by normal_history flag.
        let sz_alpha = if hero.temporal_normal_history { base_alpha } else { 0.0 };
        let smooth_z_u = BlendUniform {
            params: [sz_alpha, hero.temporal_depth_reject_threshold, hero.temporal_normal_reject_threshold, reset_flag],
            size:   [w, h, 0.0, 0.0],
        };
        // Whitewater: no depth/normal reject; alpha gated by foam_history flag.
        let ww_alpha = if hero.temporal_foam_history { base_alpha } else { 0.0 };
        let whitewater_u = BlendUniform {
            params: [ww_alpha, 0.0, 0.0, reset_flag],
            size:   [w, h, 0.0, 0.0],
        };

        queue.write_buffer(&self.thickness.uniform_buf,  0, bytemuck::bytes_of(&thickness_u));
        queue.write_buffer(&self.smooth_z.uniform_buf,   0, bytemuck::bytes_of(&smooth_z_u));
        queue.write_buffer(&self.whitewater.uniform_buf, 0, bytemuck::bytes_of(&whitewater_u));
    }

    // ── Resize ────────────────────────────────────────────────────────────

    /// Rebuild all size-dependent textures and bind groups after a canvas resize.
    pub fn set_views(
        &mut self,
        device:          &wgpu::Device,
        thickness_view:  &wgpu::TextureView,
        smooth_z_view:   &wgpu::TextureView,
        whitewater_view: &wgpu::TextureView,
        hero:            &HeroParams,
        width:           u32,
        height:          u32,
    ) {
        self.full_width  = width;
        self.full_height = height;

        self.thickness  = build_target(device, &self.bgl, &self.sampler, thickness_view,  smooth_z_view, hero, width, height, false);
        self.smooth_z   = build_target(device, &self.bgl, &self.sampler, smooth_z_view,   smooth_z_view, hero, width, height, true);
        self.whitewater = build_target(device, &self.bgl, &self.sampler, whitewater_view, smooth_z_view, hero, width, height, false);

        self.history_valid = false;
    }

    /// Drop history (e.g. on user-facing Reset): next frame will output raw current.
    pub fn invalidate_history(&mut self) {
        self.history_valid = false;
        self.thickness.frame_parity  = false;
        self.smooth_z.frame_parity   = false;
        self.whitewater.frame_parity = false;
    }
}

// ── Helper: build one TemporalTarget ─────────────────────────────────────────

fn create_r16_stable(device: &wgpu::Device, width: u32, height: u32, label: &str) -> wgpu::TextureView {
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::R16Float,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    tex.create_view(&wgpu::TextureViewDescriptor::default())
}

fn build_target(
    device:       &wgpu::Device,
    bgl:          &wgpu::BindGroupLayout,
    sampler:      &wgpu::Sampler,
    current_view: &wgpu::TextureView,
    history_z:    &wgpu::TextureView, // smooth_z for depth/normal reject
    hero:         &HeroParams,
    width:        u32,
    height:       u32,
    is_smooth_z:  bool,
) -> TemporalTarget {
    let ping = create_r16_stable(device, width, height, "temporal stable ping");
    let pong = create_r16_stable(device, width, height, "temporal stable pong");

    let alpha = hero.temporal_history_alpha.clamp(0.0, 1.0);
    let w = width.max(1) as f32;
    let h = height.max(1) as f32;

    let (depth_thresh, normal_thresh) = if is_smooth_z {
        (hero.temporal_depth_reject_threshold, hero.temporal_normal_reject_threshold)
    } else {
        (0.0_f32, 0.0_f32)
    };

    let uniform_data = BlendUniform {
        params: [alpha, depth_thresh, normal_thresh, 1.0], // reset_flag=1 on first frame
        size:   [w, h, 0.0, 0.0],
    };
    let uniform_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("temporal blend uniform"),
        contents: bytemuck::bytes_of(&uniform_data),
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
    });

    // ping_bind: current=current_view, history=pong, writes ping.
    // For depth/normal reject (is_smooth_z), binding 4 must be the HISTORY smooth_z,
    // i.e. the same texture as binding 2. Passing the raw smooth_z_view was wrong:
    // delta = abs(cur - hist_z) would be ~0 always because both came from this frame.
    let ping_history_z: &wgpu::TextureView = if is_smooth_z { &pong } else { history_z };
    let pong_history_z: &wgpu::TextureView = if is_smooth_z { &ping } else { history_z };
    let ping_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("temporal ping bind"),
        layout: bgl,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::Sampler(sampler) },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(current_view) },
            wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&pong) },
            wgpu::BindGroupEntry { binding: 3, resource: uniform_buf.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(ping_history_z) },
        ],
    });
    // pong_bind: current=current_view, history=ping, writes pong.
    let pong_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("temporal pong bind"),
        layout: bgl,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::Sampler(sampler) },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(current_view) },
            wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&ping) },
            wgpu::BindGroupEntry { binding: 3, resource: uniform_buf.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(pong_history_z) },
        ],
    });

    TemporalTarget {
        stable_ping_view: ping,
        stable_pong_view: pong,
        frame_parity:     false,
        ping_bind,
        pong_bind,
        uniform_buf,
    }
}

// ── BGL helpers ───────────────────────────────────────────────────────────────

fn bgl_sampler(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
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
