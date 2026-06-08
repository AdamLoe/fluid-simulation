//! WebGPU context: adapter/device init, surface configuration, boot diagnostics,
//! the compute/integer-atomic smoke test, the GPU fluid sim, and rendering.

mod fluid;
mod particles;
mod renderer;
mod slice;
mod smoke;
mod timing;

pub use timing::Readout as GpuReadout;
pub use timing::FINE_SECTIONS;

use crate::log;
use crate::scene::SceneConfig;
use crate::settings::Registry;
use glam::{Mat4, Vec3};

const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// Boot facts probed once at startup and used by later phases.
#[derive(Clone)]
pub struct GpuCaps {
    pub adapter_name: String,
    pub backend: String,
    pub max_storage_buffers_per_stage: u32,
    pub max_compute_workgroup_storage_size: u32,
    pub max_compute_workgroups_per_dimension: u32,
    pub max_buffer_size: u64,
    pub max_storage_buffer_binding_size: u64,
    pub timestamp_query: bool,
}

pub struct GpuContext {
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    depth_view: wgpu::TextureView,
    fluid: fluid::GpuFluid,
    wireframe: renderer::WireframeRenderer,
    particles: particles::ParticleRenderer,
    slice: slice::SliceRenderer,
    pressure_enabled: bool,
    slice_enabled: bool,
    slice_mode: u32,
    particle_size: f32,
    speed_scale: f32,
    particle_slow_rgb: [f32; 3],
    particle_fast_rgb: [f32; 3],
    particle_alpha: f32,
    particle_edge: f32,
    particle_shading: f32,
    timers: Option<timing::GpuTimers>,
    caps: GpuCaps,
    requested_particles: u32,
    estimated_particles: u32,
    scale_status: &'static str,
}

impl GpuContext {
    pub async fn new(
        canvas: web_sys::HtmlCanvasElement,
        settings: &Registry,
        scene: &SceneConfig,
    ) -> Result<Self, String> {
        let width = canvas.width().max(1);
        let height = canvas.height().max(1);

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::BROWSER_WEBGPU,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });

        let surface = instance
            .create_surface(wgpu::SurfaceTarget::Canvas(canvas))
            .map_err(|e| format!("create_surface: {e}"))?;

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .map_err(|e| format!("no suitable WebGPU adapter: {e}"))?;

        let info = adapter.get_info();
        let adapter_limits = adapter.limits();
        let adapter_features = adapter.features();
        let timestamp_query = adapter_features.contains(wgpu::Features::TIMESTAMP_QUERY);

        let mut required_features = wgpu::Features::empty();
        if timestamp_query {
            required_features |= wgpu::Features::TIMESTAMP_QUERY;
        }

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("fluid-lab device"),
                required_features,
                required_limits: adapter_limits.clone(),
                experimental_features: wgpu::ExperimentalFeatures::default(),
                memory_hints: wgpu::MemoryHints::Performance,
                trace: wgpu::Trace::Off,
            })
            .await
            .map_err(|e| format!("request_device: {e}"))?;

        let caps = GpuCaps {
            adapter_name: info.name.clone(),
            backend: format!("{:?}", info.backend),
            max_storage_buffers_per_stage: adapter_limits.max_storage_buffers_per_shader_stage,
            max_compute_workgroup_storage_size: adapter_limits.max_compute_workgroup_storage_size,
            max_compute_workgroups_per_dimension: adapter_limits
                .max_compute_workgroups_per_dimension,
            max_buffer_size: adapter_limits.max_buffer_size,
            max_storage_buffer_binding_size: adapter_limits.max_storage_buffer_binding_size,
            timestamp_query,
        };

        log_boot_diagnostics(&info, &adapter_limits, &caps, width, height);

        match smoke::run_atomic_smoke_test(&device, &queue).await {
            Ok(report) => log(&format!("[fluid-lab][smoke] PASS — {report}")),
            Err(e) => log(&format!("[fluid-lab][smoke] FAIL — {e}")),
        }

        let surface_caps = surface.get_capabilities(&adapter);
        let format = surface_caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(surface_caps.formats[0]);

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width,
            height,
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        let depth_view = create_depth(&device, width, height);

        let fluid = fluid::GpuFluid::new(&device, &queue, settings, scene);
        let estimated_particles = fluid.particle_count();
        let particle_radius = crate::sim::H * 0.35;

        let (tank_lo, tank_hi) = fluid.tank_bounds();
        let wireframe =
            renderer::WireframeRenderer::new(&device, format, DEPTH_FORMAT, tank_lo, tank_hi);
        let particles = particles::ParticleRenderer::new(
            &device,
            format,
            DEPTH_FORMAT,
            fluid.particle_buffer(),
            particle_radius,
        );

        let [grid_nx, grid_ny, grid_nz] = fluid.grid_dims();
        let slice_h = crate::sim::H;
        let slice_origin = tank_lo;
        let slice = slice::SliceRenderer::new(
            &device,
            format,
            DEPTH_FORMAT,
            fluid.cell_type_buffer(),
            fluid.pressure_buffer(),
            fluid.u_vel_buffer(),
            fluid.v_vel_buffer(),
            fluid.w_vel_buffer(),
            grid_nx,
            grid_ny,
            grid_nz,
            slice_h,
            slice_origin,
        );

        let timers = if timestamp_query {
            Some(timing::GpuTimers::new(
                &device,
                &queue,
                settings.max_substeps(),
                settings.detailed_gpu_profiling(),
                settings.pressure_iterations(),
            ))
        } else {
            None
        };

        Ok(GpuContext {
            device,
            queue,
            surface,
            config,
            depth_view,
            fluid,
            wireframe,
            particles,
            slice,
            pressure_enabled: true,
            slice_enabled: false,
            slice_mode: 0,
            particle_size: 1.0,
            speed_scale: 4.0,
            particle_slow_rgb: settings.particle_slow_color(),
            particle_fast_rgb: settings.particle_fast_color(),
            particle_alpha: settings.particle_alpha(),
            particle_edge: settings.particle_edge(),
            particle_shading: settings.particle_shading(),
            timers,
            caps,
            requested_particles: settings.particle_count(),
            estimated_particles,
            scale_status: "ok",
        })
    }

    /// Rebuild the simulation + particle/slice renderers from the current registry
    /// (the `recreate-fluid-from-settings` path). Applies Reset-class settings —
    /// grid resolution, particle count, fixed dt — by reallocating all
    /// fluid buffers. The device/surface/format are unchanged; the wireframe tank
    /// is rebuilt since per-axis grid resolution can change the tank's box size.
    /// Live params (gravity/flip/pressure_iters/spacing/classify) come fresh from `settings`.
    pub fn recreate_fluid(
        &mut self,
        settings: &Registry,
        scene: &SceneConfig,
    ) -> Result<(), String> {
        let estimated = fluid::estimated_particle_count(settings, scene);
        self.requested_particles = settings.particle_count();
        self.estimated_particles = estimated;
        let dispatch_limit = self.max_particle_dispatch_count();
        let storage_limit = self.max_particle_storage_count();
        if estimated > dispatch_limit {
            self.scale_status = "rejected-dispatch-limit";
            return Err(format!(
                "requested {} particles seeds {}, exceeding the one-dimensional particle dispatch limit {} ({} workgroups x {})",
                self.requested_particles,
                estimated,
                dispatch_limit,
                self.caps.max_compute_workgroups_per_dimension,
                fluid::PARTICLE_WG,
            ));
        }
        if estimated > storage_limit {
            self.scale_status = "rejected-storage-binding-limit";
            return Err(format!(
                "requested {} particles seeds {}, exceeding the single particle storage binding limit {}",
                self.requested_particles, estimated, storage_limit,
            ));
        }
        let fluid = fluid::GpuFluid::new(&self.device, &self.queue, settings, scene);
        let particle_radius = crate::sim::H * 0.35;
        let (tank_lo, tank_hi) = fluid.tank_bounds();
        self.wireframe = renderer::WireframeRenderer::new(
            &self.device,
            self.config.format,
            DEPTH_FORMAT,
            tank_lo,
            tank_hi,
        );
        let particles = particles::ParticleRenderer::new(
            &self.device,
            self.config.format,
            DEPTH_FORMAT,
            fluid.particle_buffer(),
            particle_radius,
        );
        let [grid_nx, grid_ny, grid_nz] = fluid.grid_dims();
        let slice_h = crate::sim::H;
        let slice = slice::SliceRenderer::new(
            &self.device,
            self.config.format,
            DEPTH_FORMAT,
            fluid.cell_type_buffer(),
            fluid.pressure_buffer(),
            fluid.u_vel_buffer(),
            fluid.v_vel_buffer(),
            fluid.w_vel_buffer(),
            grid_nx,
            grid_ny,
            grid_nz,
            slice_h,
            tank_lo,
        );
        self.fluid = fluid;
        self.particles = particles;
        self.slice = slice;
        // Timers carry Reset-class layout (max_substeps / detailed / pressure_iters);
        // rebuild them here so a Reset resizes the query set. Only when the adapter
        // supports timestamp queries (i.e. timers existed before).
        if self.caps.timestamp_query {
            self.timers = Some(timing::GpuTimers::new(
                &self.device,
                &self.queue,
                settings.max_substeps(),
                settings.detailed_gpu_profiling(),
                settings.pressure_iterations(),
            ));
        }
        self.slice.set_mode(self.slice_mode);
        self.particles.set_radius_scale(self.particle_size);
        self.particles.set_speed_scale(self.speed_scale);
        self.particles.set_particle_look(
            self.particle_slow_rgb,
            self.particle_fast_rgb,
            self.particle_alpha,
        );
        self.particles.set_edge_inner(self.particle_edge);
        self.particles.set_shading(self.particle_shading);
        log(&format!(
            "[fluid-lab][gpu] recreated fluid: dims={}x{}x{} particles={}",
            grid_nx,
            grid_ny,
            grid_nz,
            self.fluid.particle_count()
        ));
        self.scale_status = "ok";
        Ok(())
    }

    pub fn aspect(&self) -> f32 {
        self.config.width as f32 / self.config.height.max(1) as f32
    }

    pub fn particle_count(&self) -> u32 {
        self.fluid.particle_count()
    }

    /// Total grid cells (nx*ny*nz).
    pub fn total_cells(&self) -> u32 {
        self.fluid.total_cells()
    }

    /// Per-axis grid resolution [nx, ny, nz].
    pub fn grid_res(&self) -> [u32; 3] {
        self.fluid.grid_dims()
    }

    /// Total GPU storage-buffer memory owned by the fluid simulation.
    pub fn buffer_memory_bytes(&self) -> u64 {
        self.fluid.buffer_memory_bytes()
    }

    /// Number of compute dispatches issued per substep (prep+pressure+finish).
    pub fn dispatches_per_substep(&self) -> u32 {
        self.fluid.dispatches_per_substep()
    }

    pub fn requested_particles(&self) -> u32 {
        self.requested_particles
    }

    pub fn estimated_particles(&self) -> u32 {
        self.estimated_particles
    }

    pub fn scale_status(&self) -> &'static str {
        self.scale_status
    }

    pub fn max_compute_workgroups_per_dimension(&self) -> u32 {
        self.caps.max_compute_workgroups_per_dimension
    }

    pub fn max_particle_dispatch_count(&self) -> u32 {
        self.caps
            .max_compute_workgroups_per_dimension
            .saturating_mul(fluid::PARTICLE_WG)
    }

    pub fn max_particle_storage_count(&self) -> u32 {
        let bytes = self
            .caps
            .max_storage_buffer_binding_size
            .min(self.caps.max_buffer_size);
        (bytes / 32).min(u32::MAX as u64) as u32
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
        self.depth_view = create_depth(&self.device, width, height);
    }

    pub fn reset(&mut self) {
        self.fluid.reset(&self.queue);
    }

    pub fn set_pressure_enabled(&mut self, enabled: bool) {
        self.pressure_enabled = enabled;
    }

    pub fn set_slice_enabled(&mut self, on: bool) {
        self.slice_enabled = on;
    }

    pub fn set_slice_mode(&mut self, m: u32) {
        self.slice_mode = m;
        self.slice.set_mode(m);
    }

    pub fn set_flip_blend(&mut self, blend: f32) {
        self.fluid.set_flip_blend(&self.queue, blend);
    }

    pub fn set_wall_friction(&mut self, f: f32) {
        self.fluid.set_wall_friction(&self.queue, f);
    }

    pub fn set_rest_density(&mut self, v: f32) {
        self.fluid.set_rest_density(&self.queue, v);
    }

    pub fn set_volume_stiffness(&mut self, v: f32) {
        self.fluid.set_volume_stiffness(&self.queue, v);
    }

    pub fn set_drift_clamp(&mut self, v: f32) {
        self.fluid.set_drift_clamp(&self.queue, v);
    }

    pub fn set_liquid_threshold(&mut self, v: u32) {
        self.fluid.set_liquid_threshold(&self.queue, v);
    }

    pub fn set_surface_dilation(&mut self, v: u32) {
        self.fluid.set_surface_dilation(&self.queue, v);
    }

    pub fn set_cfl(&mut self, v: f32) {
        self.fluid.set_cfl(&self.queue, v);
    }

    pub fn set_gravity_vec(&mut self, gx: f32, gy: f32, gz: f32) {
        self.fluid.set_gravity_vec(&self.queue, gx, gy, gz);
    }

    pub fn set_pressure_iters(&mut self, n: u32) {
        self.fluid.set_pressure_iters(&self.queue, n);
    }

    pub fn apply_impulse(&self, dv: [f32; 3]) {
        self.fluid.apply_impulse(&self.device, &self.queue, dv);
    }

    pub fn set_particle_size(&mut self, s: f32) {
        self.particle_size = s;
        self.particles.set_radius_scale(s);
    }

    pub fn set_speed_scale(&mut self, s: f32) {
        self.speed_scale = s;
        self.particles.set_speed_scale(s);
    }

    pub fn set_particle_slow_color(&mut self, rgb: [f32; 3]) {
        self.particle_slow_rgb = rgb;
        self.particles.set_particle_look(
            self.particle_slow_rgb,
            self.particle_fast_rgb,
            self.particle_alpha,
        );
    }

    pub fn set_particle_fast_color(&mut self, rgb: [f32; 3]) {
        self.particle_fast_rgb = rgb;
        self.particles.set_particle_look(
            self.particle_slow_rgb,
            self.particle_fast_rgb,
            self.particle_alpha,
        );
    }

    pub fn set_particle_alpha(&mut self, a: f32) {
        self.particle_alpha = a;
        self.particles.set_particle_look(
            self.particle_slow_rgb,
            self.particle_fast_rgb,
            self.particle_alpha,
        );
    }

    pub fn set_particle_edge(&mut self, v: f32) {
        self.particle_edge = v;
        self.particles.set_edge_inner(v);
    }

    pub fn set_particle_shading(&mut self, v: f32) {
        self.particle_shading = v;
        self.particles.set_shading(v);
    }

    pub fn gpu_timing(&self) -> Option<GpuReadout> {
        self.timers.as_ref().map(|t| t.latest())
    }

    /// Advance the simulation by `substeps` fixed physics steps. Each substep is
    /// recorded as three timestamped compute passes (prep / pressure / finish) so
    /// the pressure-solve GPU cost is measured separately.
    pub fn step(&mut self, substeps: u32) {
        // Record how many substeps run this frame so the throttled readback only
        // sums the valid per-substep timing slots (the rest are stale/zero).
        if let Some(t) = &self.timers {
            t.set_frame_substeps(substeps);
        }
        let detailed = self.timers.as_ref().map(|t| t.detailed()).unwrap_or(false);
        for i in 0..substeps {
            let mut encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("sim step"),
                });
            if detailed {
                self.record_substep_detailed(&mut encoder, i);
            } else {
                self.record_substep_coarse(&mut encoder, i);
            }
            self.queue.submit(std::iter::once(encoder.finish()));
        }
    }

    /// COARSE substep: three monolithic timestamped compute passes
    /// (prep / pressure / finish), one begin/end pair each, owned by substep `i`.
    fn record_substep_coarse(&self, encoder: &mut wgpu::CommandEncoder, i: u32) {
        {
            let mut p = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("sim.prep"),
                timestamp_writes: self.timers.as_ref().map(|t| t.prep_writes(i)),
            });
            self.fluid.record_prep(&mut p);
        }
        {
            let mut p = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("sim.pressure"),
                timestamp_writes: self.timers.as_ref().map(|t| t.pressure_writes(i)),
            });
            if self.pressure_enabled {
                self.fluid.record_pressure(&mut p);
            }
        }
        {
            let mut p = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("sim.finish"),
                timestamp_writes: self.timers.as_ref().map(|t| t.finish_writes(i)),
            });
            self.fluid.record_finish(&mut p, self.pressure_enabled);
        }
    }

    /// DETAILED substep: one timestamped compute pass per fine SECTION so each
    /// section gets its own begin/end pair. Section indices match
    /// `timing::FINE_SECTIONS`; CG-iteration category passes follow the fixed
    /// sections (see `timing::CG_CATS`).
    fn record_substep_detailed(&self, encoder: &mut wgpu::CommandEncoder, i: u32) {
        let timers = self.timers.as_ref().expect("detailed mode requires timers");
        let f = &self.fluid;

        // One pass per fixed fine section. `sec!` opens a timestamped pass and runs
        // the body closure against it.
        macro_rules! sec {
            ($idx:expr, $label:expr, $body:expr) => {{
                let mut p = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some($label),
                    timestamp_writes: Some(timers.fine_section_writes(i, $idx)),
                });
                $body(&mut p);
            }};
        }

        // 0..18 = prep sections (clear..bound_pre_w).
        sec!(0, "d.clear", |p: &mut wgpu::ComputePass| f
            .dispatch_clear(p));
        sec!(1, "d.mark", |p: &mut wgpu::ComputePass| f.dispatch_mark(p));
        sec!(2, "d.classify", |p: &mut wgpu::ComputePass| f
            .dispatch_classify(p));
        sec!(3, "d.scatter_u", |p: &mut wgpu::ComputePass| f
            .dispatch_scatter(p, 0));
        sec!(4, "d.scatter_v", |p: &mut wgpu::ComputePass| f
            .dispatch_scatter(p, 1));
        sec!(5, "d.scatter_w", |p: &mut wgpu::ComputePass| f
            .dispatch_scatter(p, 2));
        sec!(6, "d.normalize_u", |p: &mut wgpu::ComputePass| f
            .dispatch_normalize(p, 0));
        sec!(7, "d.normalize_v", |p: &mut wgpu::ComputePass| f
            .dispatch_normalize(p, 1));
        sec!(8, "d.normalize_w", |p: &mut wgpu::ComputePass| f
            .dispatch_normalize(p, 2));
        sec!(9, "d.savevel_u", |p: &mut wgpu::ComputePass| f
            .dispatch_savevel(p, 0));
        sec!(10, "d.savevel_v", |p: &mut wgpu::ComputePass| f
            .dispatch_savevel(p, 1));
        sec!(11, "d.savevel_w", |p: &mut wgpu::ComputePass| f
            .dispatch_savevel(p, 2));
        sec!(12, "d.forces_u", |p: &mut wgpu::ComputePass| f
            .dispatch_forces(p, 0));
        sec!(13, "d.forces_v", |p: &mut wgpu::ComputePass| f
            .dispatch_forces(p, 1));
        sec!(14, "d.forces_w", |p: &mut wgpu::ComputePass| f
            .dispatch_forces(p, 2));
        sec!(15, "d.bound_pre_u", |p: &mut wgpu::ComputePass| f
            .dispatch_enforce(p, 0));
        sec!(16, "d.bound_pre_v", |p: &mut wgpu::ComputePass| f
            .dispatch_enforce(p, 1));
        sec!(17, "d.bound_pre_w", |p: &mut wgpu::ComputePass| f
            .dispatch_enforce(p, 2));

        // 18,19 = pressure-prelude sections; the CG iterations follow.
        if self.pressure_enabled {
            sec!(18, "d.divergence", |p: &mut wgpu::ComputePass| f
                .dispatch_divergence(p));
            sec!(19, "d.cg_init", |p: &mut wgpu::ComputePass| f
                .dispatch_cg_init(p));

            // CG iterations — clamp the timed count to the allocated slots (extra
            // live iters still run, just outside a fresh timed pass).
            let live = f.pressure_iters();
            let timed = timers.clamp_cg_iters(live);
            // Six honest contiguous passes per iteration, in solver order. The CPU
            // buckets them (timing.rs CG_BUCKET): reductions = both dots, updates =
            // the vector update only, scalars = alpha + beta + dir.
            for it in 0..timed {
                macro_rules! cgpass {
                    ($tpass:expr, $label:expr, $body:expr) => {{
                        let mut p = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                            label: Some($label),
                            timestamp_writes: Some(timers.fine_cg_writes(i, it, $tpass)),
                        });
                        let body: &dyn Fn(&mut wgpu::ComputePass) = &$body;
                        body(&mut p);
                    }};
                }
                cgpass!(0, "d.cg_spmv", |p: &mut wgpu::ComputePass| f
                    .dispatch_cg_spmv(p));
                cgpass!(1, "d.cg_dot_dq", |p: &mut wgpu::ComputePass| f
                    .dispatch_cg_reduce_dq(p));
                cgpass!(2, "d.cg_alpha", |p: &mut wgpu::ComputePass| f
                    .dispatch_cg_alpha(p));
                cgpass!(3, "d.cg_update", |p: &mut wgpu::ComputePass| f
                    .dispatch_cg_update(p));
                cgpass!(4, "d.cg_dot_rr", |p: &mut wgpu::ComputePass| f
                    .dispatch_cg_reduce_rr(p));
                cgpass!(5, "d.cg_beta_dir", |p: &mut wgpu::ComputePass| f
                    .dispatch_cg_beta_dir(p));
            }
            // Any live iters beyond `timed` still execute (untimed) for correctness.
            if live > timed {
                let mut p = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("d.cg_overflow"),
                    timestamp_writes: None,
                });
                for _ in timed..live {
                    f.dispatch_cg_spmv(&mut p);
                    f.dispatch_cg_reduce_dq(&mut p);
                    f.dispatch_cg_alpha(&mut p);
                    f.dispatch_cg_update(&mut p);
                    f.dispatch_cg_reduce_rr(&mut p);
                    f.dispatch_cg_beta_dir(&mut p);
                }
            }
        }

        // 20..27 = finish sections (gradient_*, bound_post_*, g2p).
        if self.pressure_enabled {
            sec!(20, "d.gradient_u", |p: &mut wgpu::ComputePass| f
                .dispatch_gradient(p, 0));
            sec!(21, "d.gradient_v", |p: &mut wgpu::ComputePass| f
                .dispatch_gradient(p, 1));
            sec!(22, "d.gradient_w", |p: &mut wgpu::ComputePass| f
                .dispatch_gradient(p, 2));
            sec!(23, "d.bound_post_u", |p: &mut wgpu::ComputePass| f
                .dispatch_enforce(p, 0));
            sec!(24, "d.bound_post_v", |p: &mut wgpu::ComputePass| f
                .dispatch_enforce(p, 1));
            sec!(25, "d.bound_post_w", |p: &mut wgpu::ComputePass| f
                .dispatch_enforce(p, 2));
        }
        sec!(26, "d.g2p", |p: &mut wgpu::ComputePass| f.dispatch_g2p(p));
    }

    pub fn render(
        &mut self,
        view_proj: &Mat4,
        cam_right: Vec3,
        cam_up: Vec3,
    ) -> Result<(), String> {
        use wgpu::CurrentSurfaceTexture as Cur;
        let frame = match self.surface.get_current_texture() {
            Cur::Success(t) | Cur::Suboptimal(t) => t,
            Cur::Outdated | Cur::Lost => {
                self.surface.configure(&self.device, &self.config);
                return Ok(());
            }
            Cur::Timeout | Cur::Occluded => return Ok(()),
            Cur::Validation => return Err("surface validation error".to_string()),
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        self.wireframe.update_camera(&self.queue, view_proj);
        self.particles
            .update_camera(&self.queue, view_proj, cam_right, cam_up);
        self.slice.update_camera(&self.queue, view_proj);

        const CLEAR: wgpu::Color = wgpu::Color {
            r: 0.04,
            g: 0.05,
            b: 0.08,
            a: 1.0,
        };

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame"),
            });

        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("scene pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(CLEAR),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &self.depth_view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: self.timers.as_ref().map(|t| t.render_writes()),
            occlusion_query_set: None,
            multiview_mask: None,
        });
        self.wireframe.draw(&mut pass);
        self.particles.draw(&mut pass, self.fluid.particle_count());
        if self.slice_enabled {
            self.slice.draw(&mut pass);
        }
        drop(pass);

        // Throttled GPU timing + liveness readback (the only allowed readback).
        let initiated = self
            .timers
            .as_ref()
            .map(|t| t.record_resolve_and_maybe_copy(&mut encoder, self.fluid.stats_buffer()))
            .unwrap_or(false);

        self.queue.submit(std::iter::once(encoder.finish()));
        frame.present();

        if initiated {
            if let Some(t) = &self.timers {
                t.map_readback();
            }
        }
        Ok(())
    }
}

fn create_depth(device: &wgpu::Device, width: u32, height: u32) -> wgpu::TextureView {
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("depth"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: DEPTH_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    tex.create_view(&wgpu::TextureViewDescriptor::default())
}

fn log_boot_diagnostics(
    info: &wgpu::AdapterInfo,
    limits: &wgpu::Limits,
    caps: &GpuCaps,
    width: u32,
    height: u32,
) {
    let dpr = web_sys::window()
        .map(|w| w.device_pixel_ratio())
        .unwrap_or(1.0);
    log("==================== [fluid-lab] WebGPU boot diagnostics ====================");
    log(&format!(
        "adapter   : {} (vendor=0x{:x} device=0x{:x} type={:?})",
        info.name, info.vendor, info.device, info.device_type
    ));
    log(&format!(
        "backend   : {:?}   driver: {} {}",
        info.backend, info.driver, info.driver_info
    ));
    log(&format!(
        "limits    : max_storage_buffers_per_shader_stage={}",
        limits.max_storage_buffers_per_shader_stage
    ));
    log(&format!(
        "            max_compute_invocations_per_workgroup={} max_compute_workgroup_size=({},{},{})",
        limits.max_compute_invocations_per_workgroup,
        limits.max_compute_workgroup_size_x,
        limits.max_compute_workgroup_size_y,
        limits.max_compute_workgroup_size_z,
    ));
    log(&format!(
        "            max_compute_workgroups_per_dimension={} particle_dispatch_limit={}",
        limits.max_compute_workgroups_per_dimension,
        limits
            .max_compute_workgroups_per_dimension
            .saturating_mul(fluid::PARTICLE_WG),
    ));
    log(&format!(
        "            max_buffer_size={} max_storage_buffer_binding_size={}",
        limits.max_buffer_size, limits.max_storage_buffer_binding_size
    ));
    log(&format!(
        "timestamp-query available : {}  ({})",
        caps.timestamp_query,
        if caps.timestamp_query {
            "real per-pass GPU timing usable in 0.3"
        } else {
            "0.3 must use minimum-honest fallback profiler"
        }
    ));
    log(&format!(
        "canvas    : {}x{} device_pixel_ratio={}",
        width, height, dpr
    ));
    log("============================================================================");
}
