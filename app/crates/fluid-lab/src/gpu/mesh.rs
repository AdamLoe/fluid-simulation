//! MeshExtractor: GPU marching-cubes water surface (Phase 1.4).
//!
//! Pipeline:
//!   1. density.wgsl  — occupancy → scalar[] f32 field + per-cell speed[] (foam)
//!   2. blur.wgsl     — N ping-pong Gaussian iterations smoothing scalar[] (issue 4)
//!   3. mc.wgsl       — march (nx-1)(ny-1)(nz-1) cubes → append verts[] (pos+normal+foam) + counter
//!   4. mc_args.wgsl  — write indirect draw args from counter
//!   5. mesh.wgsl     — translucent glassy water render pass (storage-buffer vertex fetch)
//!
//! Buffers owned here:
//!   scalar[]       — f32 per cell (density field, smoothed in place by blur)
//!   scalar2[]      — f32 per cell (blur ping-pong scratch)
//!   speed[]        — f32 per cell (surface speed → foam)
//!   verts[]        — Vertex{pos:vec4, nrm:vec4} × MAX_VERTS (32 B each); nrm.w = foam
//!   counter[]      — atomic<u32> × 1 (vertex count, cleared each frame)
//!   indirect_args[]— u32 × 4 (draw indirect: vertex_count, 1, 0, 0)
//!   mesh_params_buf— uniform: isolevel, h, foam_scale, _, dims(vec4<u32>), origin(vec4)
//!   camera_buf     — uniform: view_proj(mat4) + cam_pos + sun_dir + water + tint
//!
//! MAX_TRIS = 800_000  →  MAX_VERTS = 2_400_000  →  ~73 MB vertex buffer.

use glam::Mat4;
use wgpu::util::DeviceExt;

const WG: u32 = 64;

/// MAX_TRIS = 800_000 → MAX_VERTS = 2_400_000.
pub const MAX_VERTS: u32 = 2_400_000;

/// Vertex stride = 2 × vec4 = 32 bytes.
const VERTEX_STRIDE: u64 = 32;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct MeshParams {
    isolevel: f32,
    h: f32,
    foam_scale: f32, // speed (u/s) mapped to full foam
    _pad1: f32,
    dims: [u32; 4],   // nx, ny, nz, _
    origin: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct MeshCamera {
    view_proj:     [[f32; 4]; 4],
    inv_view_proj: [[f32; 4]; 4], // reconstruct tank-local pos from pixel+depth (thickness)
    cam_pos:   [f32; 4], // xyz = eye position IN TANK-LOCAL space (for fresnel/specular)
    sun_dir:   [f32; 4], // xyz = sun direction (normalized in shader), w = 0
    water:     [f32; 4], // opacity, fresnel_strength, foam_strength, absorb_strength
    tint:      [f32; 4], // rgb base water color, w = refract_strength
    misc:      [f32; 4], // xy = render-target resolution (px), zw = _
}

/// Tunable look of the water surface, preserved across mesh (re)allocation and
/// re-sent in `update_camera` each frame.
#[derive(Clone, Copy)]
pub struct MeshLook {
    pub opacity: f32,
    pub fresnel: f32,
    pub foam: f32,
    pub absorb: f32, // Beer-Lambert absorption strength (depth-tint per unit thickness)
    pub refract: f32, // screen-space refraction offset strength
    pub tint: [f32; 3],
    pub smooth_iters: u32,
}

impl Default for MeshLook {
    fn default() -> Self {
        Self {
            opacity: 0.55,
            fresnel: 1.0,
            foam: 0.8,
            absorb: 2.5,
            refract: 0.6,
            tint: [0.10, 0.32, 0.55],
            smooth_iters: 2,
        }
    }
}

pub struct MeshExtractor {
    nx: u32,
    ny: u32,
    nz: u32,
    cell_count: u32,

    // --- owned buffers ---
    scalar_buf:       wgpu::Buffer,
    scalar2_buf:      wgpu::Buffer,
    speed_buf:        wgpu::Buffer,
    verts_buf:        wgpu::Buffer,
    counter_buf:      wgpu::Buffer,
    indirect_args_buf:wgpu::Buffer,
    mesh_params_buf:  wgpu::Buffer,
    camera_buf:       wgpu::Buffer,

    // --- params mirror (for live iso/foam_scale update) ---
    mesh_params: MeshParams,
    look: MeshLook,
    sun_dir: [f32; 4],

    // --- compute pipelines ---
    density_pl:  wgpu::ComputePipeline,
    blur_pl:     wgpu::ComputePipeline,
    mc_pl:       wgpu::ComputePipeline,
    mc_args_pl:  wgpu::ComputePipeline,

    // --- render pipelines ---
    render_pl: wgpu::RenderPipeline,     // shaded water (samples scene_color + back_depth)
    back_pl:   wgpu::RenderPipeline,     // depth-only prepass → water_back_depth (far surface)

    // --- render bind-group layout (rebuilt each resize for scene targets) ---
    render_bgl: wgpu::BindGroupLayout,

    // --- bind groups ---
    density_bg:  wgpu::BindGroup,
    blur_bg_ab:  wgpu::BindGroup, // scalar → scalar2
    blur_bg_ba:  wgpu::BindGroup, // scalar2 → scalar
    mc_bg:       wgpu::BindGroup,
    mc_args_bg:  wgpu::BindGroup,
    render_bg:   wgpu::BindGroup, // camera + verts + scene_color + back_depth
    back_bg:     wgpu::BindGroup, // camera + verts (depth-only prepass)
}

impl MeshExtractor {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        device: &wgpu::Device,
        surface_format: wgpu::TextureFormat,
        depth_format: wgpu::TextureFormat,
        occupancy_buf: &wgpu::Buffer, // i32 × cell_count (from GpuFluid)
        u_vel_buf: &wgpu::Buffer,
        v_vel_buf: &wgpu::Buffer,
        w_vel_buf: &wgpu::Buffer,
        nx: u32,
        ny: u32,
        nz: u32,
        origin: [f32; 3],
        look: MeshLook,
        isolevel: f32,
        scene_color: &wgpu::TextureView, // offscreen background (refraction source)
        back_depth: &wgpu::TextureView,  // water far-surface depth (thickness source)
    ) -> Self {
        let h = crate::sim::H;
        let origin = [origin[0], origin[1], origin[2], 0.0];
        let cell_count = nx * ny * nz;

        let mesh_params = MeshParams {
            isolevel,
            h,
            foam_scale: 4.0,
            _pad1: 0.0,
            dims: [nx, ny, nz, 0],
            origin,
        };

        // ── Buffers ─────────────────────────────────────────────────────────────
        let mesh_params_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("mesh_params"),
            contents: bytemuck::bytes_of(&mesh_params),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let mk_scalar = |label: &str| device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: (cell_count as u64) * 4,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let scalar_buf = mk_scalar("mc_scalar");
        let scalar2_buf = mk_scalar("mc_scalar2");
        let speed_buf = mk_scalar("mc_speed");

        let verts_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("mc_verts"),
            size: (MAX_VERTS as u64) * VERTEX_STRIDE,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });

        let counter_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("mc_counter"),
            contents: bytemuck::cast_slice(&[0u32]),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        });

        let indirect_args_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("mc_indirect_args"),
            contents: bytemuck::cast_slice(&[0u32, 1u32, 0u32, 0u32]),
            usage: wgpu::BufferUsages::INDIRECT | wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        });

        let sun_dir = [0.4, 1.0, 0.3, 0.0];
        let camera_uniform = MeshCamera {
            view_proj: Mat4::IDENTITY.to_cols_array_2d(),
            inv_view_proj: Mat4::IDENTITY.to_cols_array_2d(),
            cam_pos: [0.0; 4],
            sun_dir,
            water: [look.opacity, look.fresnel, look.foam, look.absorb],
            tint: [look.tint[0], look.tint[1], look.tint[2], look.refract],
            misc: [1.0, 1.0, 0.0, 0.0],
        };
        let camera_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("mesh_camera"),
            contents: bytemuck::bytes_of(&camera_uniform),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        // ── Compute pipelines (auto-layout; one bind group per pipeline) ────────
        let density_pl = make_compute(device, "density", include_str!("shaders/density.wgsl"), "main");
        let blur_pl    = make_compute(device, "blur",    include_str!("shaders/blur.wgsl"),    "main");
        let mc_pl      = make_compute(device, "mc",      include_str!("shaders/mc.wgsl"),      "main");
        let mc_args_pl = make_compute(device, "mc_args", include_str!("shaders/mc_args.wgsl"), "main");

        // ── Compute bind groups ──────────────────────────────────────────────────
        let density_bg = make_bg(device, "density_bg", &density_pl,
            &[mesh_params_buf.as_entire_binding(), occupancy_buf.as_entire_binding(),
              u_vel_buf.as_entire_binding(), v_vel_buf.as_entire_binding(),
              w_vel_buf.as_entire_binding(), scalar_buf.as_entire_binding(),
              speed_buf.as_entire_binding()]);

        let blur_bg_ab = make_bg(device, "blur_bg_ab", &blur_pl,
            &[mesh_params_buf.as_entire_binding(), scalar_buf.as_entire_binding(),
              scalar2_buf.as_entire_binding()]);
        let blur_bg_ba = make_bg(device, "blur_bg_ba", &blur_pl,
            &[mesh_params_buf.as_entire_binding(), scalar2_buf.as_entire_binding(),
              scalar_buf.as_entire_binding()]);

        let mc_bg = make_bg(device, "mc_bg", &mc_pl,
            &[mesh_params_buf.as_entire_binding(), scalar_buf.as_entire_binding(),
              verts_buf.as_entire_binding(), counter_buf.as_entire_binding(),
              speed_buf.as_entire_binding()]);

        let mc_args_bg = make_bg(device, "mc_args_bg", &mc_args_pl,
            &[counter_buf.as_entire_binding(), indirect_args_buf.as_entire_binding()]);

        // ── Render pipeline (explicit layout) ────────────────────────────────────
        // binding 0: camera uniform · 1: verts storage · 2: scene_color (refraction)
        // · 3: water back-surface depth (thickness).
        let cam_entry = wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let verts_entry = wgpu::BindGroupLayoutEntry {
            binding: 1,
            visibility: wgpu::ShaderStages::VERTEX,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let render_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("mesh_render_bgl"),
            entries: &[
                cam_entry,
                verts_entry,
                // binding 2: scene_color (offscreen background, refraction source)
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                // binding 3: water back-surface depth (thickness source)
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });

        let render_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("mesh_render_layout"),
            bind_group_layouts: &[Some(&render_bgl)],
            immediate_size: 0,
        });

        // Depth-only prepass layout: camera + verts only (it CANNOT bind back_depth,
        // which is its own render target).
        let back_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("mesh_back_bgl"),
            entries: &[cam_entry, verts_entry],
        });
        let back_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("mesh_back_layout"),
            bind_group_layouts: &[Some(&back_bgl)],
            immediate_size: 0,
        });

        let mesh_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("mesh shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/mesh.wgsl").into()),
        });

        let render_pl = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("mesh render pipeline"),
            layout: Some(&render_layout),
            vertex: wgpu::VertexState {
                module: &mesh_module,
                entry_point: Some("vs"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &mesh_module,
                entry_point: Some("fs"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    // Opaque: the shader composites the refracted/absorbed background
                    // itself (the background is already blitted underneath), so no
                    // blending — depth resolves the nearest water surface.
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None, // no culling: winding from MC is ambiguous
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: depth_format,
                // Depth-test against the scene but still write depth so layered
                // water surfaces resolve to the nearest one.
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        // Depth-only prepass: reuse the mesh vertex shader, no fragment stage. Writes
        // the FARTHEST water surface (compare Greater, cleared to 0.0) into a depth
        // target sampled by the main pass for per-pixel water thickness.
        let back_pl = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("mesh back-depth pipeline"),
            layout: Some(&back_layout),
            vertex: wgpu::VertexState {
                module: &mesh_module,
                entry_point: Some("vs"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: None,
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: depth_format,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Greater),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        let render_bg = make_render_bg(device, &render_bgl, &camera_buf, &verts_buf, scene_color, back_depth);

        let back_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("mesh_back_bg"),
            layout: &back_bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: camera_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: verts_buf.as_entire_binding() },
            ],
        });

        Self {
            nx,
            ny,
            nz,
            cell_count,
            scalar_buf,
            scalar2_buf,
            speed_buf,
            verts_buf,
            counter_buf,
            indirect_args_buf,
            mesh_params_buf,
            camera_buf,
            mesh_params,
            look,
            sun_dir,
            density_pl,
            blur_pl,
            mc_pl,
            mc_args_pl,
            render_pl,
            back_pl,
            render_bgl,
            density_bg,
            blur_bg_ab,
            blur_bg_ba,
            mc_bg,
            mc_args_bg,
            render_bg,
            back_bg,
        }
    }

    /// Re-point the main render bind group at freshly (re)created scene targets.
    /// Called whenever `scene_color` / `water_back_depth` are recreated (resize).
    pub fn rebuild_render_bg(
        &mut self,
        device: &wgpu::Device,
        scene_color: &wgpu::TextureView,
        back_depth: &wgpu::TextureView,
    ) {
        self.render_bg = make_render_bg(
            device,
            &self.render_bgl,
            &self.camera_buf,
            &self.verts_buf,
            scene_color,
            back_depth,
        );
    }

    /// Total bytes of the storage buffers owned by the mesh extractor (the
    /// ~73 MB vertex buffer dominates). scalar+scalar2+speed (3·cell_count·4) +
    /// verts (MAX_VERTS·32) + counter (4) + indirect_args (16).
    pub fn buffer_memory_bytes(&self) -> u64 {
        (self.cell_count as u64) * 4 * 3 + (MAX_VERTS as u64) * VERTEX_STRIDE + 4 + 16
    }

    /// Live update of the isosurface level.
    pub fn set_iso(&mut self, queue: &wgpu::Queue, iso: f32) {
        self.mesh_params.isolevel = iso;
        queue.write_buffer(&self.mesh_params_buf, 0, bytemuck::bytes_of(&self.mesh_params));
    }

    /// Live update of the surface-smoothing iteration count (blur passes).
    pub fn set_smooth_iters(&mut self, n: u32) {
        self.look.smooth_iters = n;
    }

    /// Live update of the water-surface look (opacity / reflectivity / foam).
    /// Applied on the next `update_camera` (which rewrites the whole uniform).
    pub fn set_opacity(&mut self, v: f32) { self.look.opacity = v; }
    pub fn set_fresnel(&mut self, v: f32) { self.look.fresnel = v; }
    pub fn set_foam(&mut self, v: f32) { self.look.foam = v; }
    pub fn set_absorb(&mut self, v: f32) { self.look.absorb = v; }
    pub fn set_refract(&mut self, v: f32) { self.look.refract = v; }

    /// Upload camera matrices, eye (tank-local), sun, look, and target resolution for
    /// this frame. `view_proj` maps tank-local → clip; its inverse reconstructs the
    /// background position (water thickness) from the depth prepass.
    pub fn update_camera(
        &self,
        queue: &wgpu::Queue,
        view_proj: &Mat4,
        cam_pos_local: glam::Vec3,
        sun_dir: glam::Vec3,
        resolution: (f32, f32),
    ) {
        let u = MeshCamera {
            view_proj: view_proj.to_cols_array_2d(),
            inv_view_proj: view_proj.inverse().to_cols_array_2d(),
            cam_pos: [cam_pos_local.x, cam_pos_local.y, cam_pos_local.z, 0.0],
            sun_dir: [sun_dir.x, sun_dir.y, sun_dir.z, 0.0],
            water: [self.look.opacity, self.look.fresnel, self.look.foam, self.look.absorb],
            tint: [self.look.tint[0], self.look.tint[1], self.look.tint[2], self.look.refract],
            misc: [resolution.0, resolution.1, 0.0, 0.0],
        };
        let _ = self.sun_dir; // retained for symmetry / future static-sun paths
        queue.write_buffer(&self.camera_buf, 0, bytemuck::bytes_of(&u));
    }

    /// Record the extract passes (density+speed → N×blur smoothing → MC → args).
    /// Must be called AFTER the sim step so occupancy/velocity are populated.
    pub fn record_extract(&self, device: &wgpu::Device, queue: &wgpu::Queue) {
        // Clear counter (write_buffer is fine here; tiny upload).
        queue.write_buffer(&self.counter_buf, 0, bytemuck::cast_slice(&[0u32]));

        let cells = self.cell_count.div_ceil(WG);
        let cubes = (self.nx - 1) * (self.ny - 1) * (self.nz - 1);
        let cubes_wg = cubes.div_ceil(WG);

        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("mc extract"),
        });
        {
            let mut p = enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("density"),
                timestamp_writes: None,
            });
            p.set_pipeline(&self.density_pl);
            p.set_bind_group(0, &self.density_bg, &[]);
            p.dispatch_workgroups(cells, 1, 1);
        }
        // Each smoothing iteration is two blur passes (scalar→scalar2→scalar) so
        // the final smoothed field always lands back in `scalar` (what MC reads).
        for _ in 0..self.look.smooth_iters {
            let mut p = enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("blur"),
                timestamp_writes: None,
            });
            p.set_pipeline(&self.blur_pl);
            p.set_bind_group(0, &self.blur_bg_ab, &[]);
            p.dispatch_workgroups(cells, 1, 1);
            p.set_bind_group(0, &self.blur_bg_ba, &[]);
            p.dispatch_workgroups(cells, 1, 1);
        }
        {
            let mut p = enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("mc"),
                timestamp_writes: None,
            });
            p.set_pipeline(&self.mc_pl);
            p.set_bind_group(0, &self.mc_bg, &[]);
            p.dispatch_workgroups(cubes_wg, 1, 1);
        }
        {
            let mut p = enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("mc_args"),
                timestamp_writes: None,
            });
            p.set_pipeline(&self.mc_args_pl);
            p.set_bind_group(0, &self.mc_args_bg, &[]);
            p.dispatch_workgroups(1, 1, 1);
        }
        queue.submit(std::iter::once(enc.finish()));
    }

    /// Depth-only prepass writing the water's FAR surface into `water_back_depth`.
    /// Call in a depth-only render pass (cleared to 0.0) BEFORE the main water pass.
    pub fn draw_back(&self, pass: &mut wgpu::RenderPass<'_>) {
        pass.set_pipeline(&self.back_pl);
        pass.set_bind_group(0, &self.back_bg, &[]);
        pass.draw_indirect(&self.indirect_args_buf, 0);
    }

    /// Draw the shaded water (indirect). Call inside the water render pass after
    /// updating the camera uniform and the background blit.
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        pass.set_pipeline(&self.render_pl);
        pass.set_bind_group(0, &self.render_bg, &[]);
        pass.draw_indirect(&self.indirect_args_buf, 0);
    }
}

/// Build the main render bind group (camera + verts + scene_color + back_depth).
/// Shared by `new` and `rebuild_render_bg` (scene targets change on resize).
fn make_render_bg(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    camera_buf: &wgpu::Buffer,
    verts_buf: &wgpu::Buffer,
    scene_color: &wgpu::TextureView,
    back_depth: &wgpu::TextureView,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("mesh_render_bg"),
        layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: camera_buf.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: verts_buf.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(scene_color) },
            wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(back_depth) },
        ],
    })
}

// ── helpers ──────────────────────────────────────────────────────────────────

fn make_compute(device: &wgpu::Device, label: &str, src: &str, entry: &str) -> wgpu::ComputePipeline {
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(src.into()),
    });
    device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some(label),
        layout: None,
        module: &module,
        entry_point: Some(entry),
        compilation_options: Default::default(),
        cache: None,
    })
}

fn make_bg<'a>(
    device: &wgpu::Device,
    label: &str,
    pl: &wgpu::ComputePipeline,
    resources: &[wgpu::BindingResource<'a>],
) -> wgpu::BindGroup {
    let entries: Vec<wgpu::BindGroupEntry> = resources
        .iter()
        .enumerate()
        .map(|(i, r)| wgpu::BindGroupEntry { binding: i as u32, resource: r.clone() })
        .collect();
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(label),
        layout: &pl.get_bind_group_layout(0),
        entries: &entries,
    })
}
