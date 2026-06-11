//! WebGPU context: adapter/device init, surface configuration, boot diagnostics,
//! the compute/integer-atomic smoke test, the GPU fluid sim, and rendering.

mod caustics;
mod composite;
mod diffuse;
mod environment;
mod fluid;
mod particles;
mod renderer;
mod skybox;
mod slice;
mod smoke;
mod smoothing;
mod temporal;
mod timing;
mod wallfill;
mod wetwall;

pub use timing::Readout as GpuReadout;
pub use timing::FINE_SECTIONS;

use crate::log;
use crate::scene::SceneConfig;
use crate::settings::{self, Registry};
use glam::{Mat4, Vec3};

const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
/// Offscreen scene-color format for the hero-water prepass: linear HDR so the
/// environment + wireframe can be sampled (and refraction-tapped) before the
/// water composites over it. See [`composite`].
const SCENE_COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// How the fluid is drawn. Replaces the bare `u32 particle_view` dispatch
/// (`render.particle_view` still maps 0/1/2 to these for compatibility). The
/// optical/simple particle views are the explicit fallbacks the hero-water
/// series must never break; hero features are Live sub-features of `Water`, not
/// new modes here.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RenderMode {
    /// Screen-space water composite — the hero path (refraction, environment, …).
    Water,
    /// v1.10 optical-depth particle billboards.
    OpticalParticles,
    /// pre-v1.10 simple alpha billboards.
    SimpleParticles,
}

impl RenderMode {
    /// Map the `render.particle_view` u32 (0/1/2) to a mode, defaulting unknown
    /// values to `Water`.
    fn from_u32(v: u32) -> Self {
        match v {
            1 => RenderMode::OpticalParticles,
            2 => RenderMode::SimpleParticles,
            _ => RenderMode::Water,
        }
    }
}

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
    thickness_view: wgpu::TextureView,
    whitewater_view: wgpu::TextureView,
    wallfill_mask_view: wgpu::TextureView,
    nearest_z_view: wgpu::TextureView,
    smooth_z_ping_view: wgpu::TextureView,
    smooth_z_view: wgpu::TextureView,
    /// Hero-water scene prepass targets: the environment + wireframe are drawn
    /// here (linear HDR color + linear eye-distance) so the composite can sample
    /// them for the refracted background.
    scene_color_view: wgpu::TextureView,
    scene_depth_view: wgpu::TextureView,
    fluid: fluid::GpuFluid,
    wireframe: renderer::WireframeRenderer,
    particles: particles::ParticleRenderer,
    environment: environment::EnvironmentRenderer,
    /// World-background procedural skybox (camera-driven, box-independent).
    skybox: skybox::SkyboxRenderer,
    composite: composite::CompositeRenderer,
    smoothing: smoothing::WaterSmoothRenderer,
    thickness_smoothing: smoothing::ThicknessSmoothRenderer,
    whitewater_smoothing: smoothing::ThicknessSmoothRenderer,
    slice: slice::SliceRenderer,
    /// Persistent diffuse-water particles (foam/spray/bubbles) — render-only.
    diffuse: diffuse::DiffuseSystem,
    /// v1.16 approximate screen-space caustics — two-pass generation + composite.
    caustics: caustics::CausticsSystem,
    /// v1.17 wet-wall + meniscus — persistent wetness field on tank walls.
    wetwall: wetwall::WetWallSystem,
    /// v1.21 gap-filled flat water sheet — per-wall occupancy compute + MRT fill pass.
    wallocc: wallfill::WallOccupancySystem,
    wallfill: wallfill::WallFillRenderer,
    /// v1.18 temporal stabilization — ping-pong history blend for depth/thickness/whitewater.
    temporal: temporal::TemporalSystem,
    /// Previous frame's eye_to_world matrix for camera-motion delta computation.
    prev_eye_to_world: Mat4,
    /// Monotonic frame counter feeding the diffuse spawn hash (no wall-clock RNG).
    diffuse_frame: u32,
    pressure_enabled: bool,
    slice_enabled: bool,
    slice_mode: u32,
    particle_size: f32,
    speed_scale: f32,
    render_mode: RenderMode,
    hero: settings::HeroParams,
    particle_slow_rgb: [f32; 3],
    particle_fast_rgb: [f32; 3],
    water_optical_density: f32,
    particle_edge: f32,
    particle_shading: f32,
    whitewater_strength: f32,
    whitewater_threshold: f32,
    whitewater_softness: f32,
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
        let thickness_view = create_r16_target(&device, width, height, "water thickness");
        let whitewater_view = create_r16_target(&device, width, height, "water whitewater");
        let wallfill_mask_view = create_r16_target(&device, width, height, "wallfill mask");
        let nearest_z_view = create_r16_target(&device, width, height, "water nearest z");
        let smooth_z_ping_view = create_r16_target(&device, width, height, "water smooth z ping");
        let smooth_z_view = create_r16_target(&device, width, height, "water smooth z");
        let scene_color_view = create_scene_color_target(&device, width, height);
        let scene_depth_view = create_r16_target(&device, width, height, "hero scene depth");

        let requested_particles = settings.particle_count();
        let estimated_particles = fluid::estimated_particle_count(settings, scene);
        validate_particle_scale(
            requested_particles,
            estimated_particles,
            caps.max_compute_workgroups_per_dimension,
            max_particle_storage_count_for(&caps),
        )?;

        let fluid = fluid::GpuFluid::new(
            &device,
            &queue,
            settings,
            scene,
            caps.max_compute_workgroups_per_dimension,
        );
        let particle_radius = crate::sim::H * 0.35;

        let (tank_lo, tank_hi) = fluid.tank_bounds();
        let [grid_nx_init, grid_ny_init, grid_nz_init] = fluid.grid_dims();
        // WetWallSystem is constructed here and after recreate_fluid (for Reset).
        // Its wetness buffer + uniform are shared with EnvironmentRenderer (group 1).
        // Derive hero params early so we can pass wet_wall_supersample here
        // (the named `hero` binding is later below with the skybox/composite).
        let hero_init = settings.hero_params();
        let wetwall = wetwall::WetWallSystem::new(
            &device,
            fluid.cell_type_buffer(),
            grid_nx_init,
            grid_ny_init,
            grid_nz_init,
            tank_lo,
            tank_hi,
            hero_init.wet_wall_supersample,
        );
        let wallocc = wallfill::WallOccupancySystem::new(
            &device,
            fluid.cell_type_buffer(),
            fluid.particle_buffer(),
            fluid.particle_count(),
            grid_nx_init,
            grid_ny_init,
            grid_nz_init,
            tank_lo,
            tank_hi,
            &hero_init,
        );
        let wallfill_renderer = wallfill::WallFillRenderer::new(&device, &wallocc, &hero_init);
        let wireframe = renderer::WireframeRenderer::new(
            &device,
            format,
            SCENE_COLOR_FORMAT,
            wgpu::TextureFormat::R16Float,
            DEPTH_FORMAT,
            tank_lo,
            tank_hi,
        );
        let environment = environment::EnvironmentRenderer::new(
            &device,
            SCENE_COLOR_FORMAT,
            wgpu::TextureFormat::R16Float,
            DEPTH_FORMAT,
            tank_lo,
            tank_hi,
            &wetwall.uniform_buf,
            &wetwall.wetness_buf,
        );
        let mut particles = particles::ParticleRenderer::new(
            &device,
            format,
            DEPTH_FORMAT,
            fluid.particle_buffer(),
            particle_radius,
        );
        particles.set_radius_scale(settings.particle_size());
        particles.set_particle_volume(
            represented_liquid_volume(scene) / (fluid.particle_count().max(1) as f32),
        );
        particles.set_speed_scale(settings.speed_scale());
        particles.set_particle_colors(
            settings.particle_slow_color(),
            settings.particle_fast_color(),
        );
        particles.set_water_optical_density(settings.water_optical_density());
        particles.set_edge_inner(settings.particle_edge());
        particles.set_shading(settings.particle_shading());
        let hero = settings.hero_params();
        let skybox = skybox::SkyboxRenderer::new(
            &device,
            SCENE_COLOR_FORMAT,
            wgpu::TextureFormat::R16Float,
            DEPTH_FORMAT,
            &hero,
        );
        environment.set_params(&queue, &hero);
        let smoothing = smoothing::WaterSmoothRenderer::new(
            &device,
            &nearest_z_view,
            &smooth_z_ping_view,
            &smooth_z_view,
            hero.smooth_radius,
        );
        // Plain-Gaussian thickness + whitewater blurs sharing the depth pass's
        // ping scratch. Whitewater is blurred so foam reads as coherent regions
        // instead of per-particle speckle dots on moving water.
        let thickness_smoothing = smoothing::ThicknessSmoothRenderer::new(
            &device,
            &thickness_view,
            &smooth_z_ping_view,
            hero.smooth_radius,
        );
        let whitewater_smoothing = smoothing::ThicknessSmoothRenderer::new(
            &device,
            &whitewater_view,
            &smooth_z_ping_view,
            hero.smooth_radius,
        );

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
            grid_nx_init,
            grid_ny_init,
            grid_nz_init,
            slice_h,
            slice_origin,
        );

        let diffuse = diffuse::DiffuseSystem::new(
            &device,
            format,
            DEPTH_FORMAT,
            &fluid,
            settings.diffuse_params(),
        );

        // v1.18 TemporalSystem — always allocated. When disabled, alpha=0 → stable views
        // pass through raw current each frame. Caustics + composite always read stable views.
        let temporal_sys = temporal::TemporalSystem::new(
            &device,
            &thickness_view,
            &smooth_z_view,
            &whitewater_view,
            &hero,
            width,
            height,
        );

        // Composite reads stabilized views (temporal stable = raw current on first frame
        // since history_valid=false → reset_flag=1.0 → pass-through).
        let composite = composite::CompositeRenderer::new(
            &device,
            format,
            temporal_sys.stable_thickness(),
            temporal_sys.stable_whitewater(),
            temporal_sys.stable_smooth_z(),
            &wallfill_mask_view,
            &scene_color_view,
            &scene_depth_view,
            &hero,
            settings.particle_slow_color(),
            settings.water_optical_density(),
            settings.particle_shading(),
            settings.whitewater_strength(),
            settings.whitewater_threshold(),
            settings.whitewater_softness(),
            [width, height],
        );

        // CausticsSystem — always allocated (OFF by default). Reads stabilized
        // depth/thickness from the temporal system (pass-through on first frame).
        let caustics_sys = caustics::CausticsSystem::new(
            &device,
            SCENE_COLOR_FORMAT,
            temporal_sys.stable_smooth_z(),
            temporal_sys.stable_thickness(),
            &scene_depth_view,
            &hero,
            tank_lo,
            tank_hi,
            width,
            height,
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
            thickness_view,
            whitewater_view,
            wallfill_mask_view,
            nearest_z_view,
            smooth_z_ping_view,
            smooth_z_view,
            scene_color_view,
            scene_depth_view,
            fluid,
            wireframe,
            particles,
            environment,
            skybox,
            composite,
            smoothing,
            thickness_smoothing,
            whitewater_smoothing,
            slice,
            diffuse,
            caustics: caustics_sys,
            wetwall,
            wallocc,
            wallfill: wallfill_renderer,
            temporal: temporal_sys,
            prev_eye_to_world: Mat4::IDENTITY,
            diffuse_frame: 0,
            pressure_enabled: true,
            slice_enabled: false,
            slice_mode: 0,
            particle_size: settings.particle_size(),
            speed_scale: settings.speed_scale(),
            render_mode: RenderMode::from_u32(settings.particle_view()),
            hero,
            particle_slow_rgb: settings.particle_slow_color(),
            particle_fast_rgb: settings.particle_fast_color(),
            water_optical_density: settings.water_optical_density(),
            particle_edge: settings.particle_edge(),
            particle_shading: settings.particle_shading(),
            whitewater_strength: settings.whitewater_strength(),
            whitewater_threshold: settings.whitewater_threshold(),
            whitewater_softness: settings.whitewater_softness(),
            timers,
            caps,
            requested_particles,
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
        if let Err(e) = validate_particle_scale(
            self.requested_particles,
            estimated,
            self.caps.max_compute_workgroups_per_dimension,
            storage_limit,
        ) {
            self.scale_status = if estimated > dispatch_limit {
                "rejected-dispatch-capacity"
            } else {
                "rejected-storage-binding-limit"
            };
            return Err(e);
        }
        let fluid = fluid::GpuFluid::new(
            &self.device,
            &self.queue,
            settings,
            scene,
            self.caps.max_compute_workgroups_per_dimension,
        );
        let particle_radius = crate::sim::H * 0.35;
        let (tank_lo, tank_hi) = fluid.tank_bounds();
        let [grid_nx, grid_ny, grid_nz] = fluid.grid_dims();
        // Rebuild WetWallSystem with a fresh zeroed wetness buffer (clears wetness on Reset).
        // Must be built before EnvironmentRenderer so we can pass the fresh buffer refs.
        let wetwall = wetwall::WetWallSystem::new(
            &self.device,
            fluid.cell_type_buffer(),
            grid_nx,
            grid_ny,
            grid_nz,
            tank_lo,
            tank_hi,
            self.hero.wet_wall_supersample,
        );
        self.wireframe = renderer::WireframeRenderer::new(
            &self.device,
            self.config.format,
            SCENE_COLOR_FORMAT,
            wgpu::TextureFormat::R16Float,
            DEPTH_FORMAT,
            tank_lo,
            tank_hi,
        );
        self.environment = environment::EnvironmentRenderer::new(
            &self.device,
            SCENE_COLOR_FORMAT,
            wgpu::TextureFormat::R16Float,
            DEPTH_FORMAT,
            tank_lo,
            tank_hi,
            &wetwall.uniform_buf,
            &wetwall.wetness_buf,
        );
        self.environment.set_params(&self.queue, &self.hero);
        let particles = particles::ParticleRenderer::new(
            &self.device,
            self.config.format,
            DEPTH_FORMAT,
            fluid.particle_buffer(),
            particle_radius,
        );
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
        // Rebuild the diffuse system against the fresh sim buffers; this also
        // clears all diffuse particles (a fresh, zeroed buffer) on Reset.
        let diffuse = diffuse::DiffuseSystem::new(
            &self.device,
            self.config.format,
            DEPTH_FORMAT,
            &fluid,
            settings.diffuse_params(),
        );
        // Rebuild WallOccupancySystem and rebind the fill renderer.
        let wallocc = wallfill::WallOccupancySystem::new(
            &self.device,
            fluid.cell_type_buffer(),
            fluid.particle_buffer(),
            fluid.particle_count(),
            grid_nx,
            grid_ny,
            grid_nz,
            tank_lo,
            tank_hi,
            &self.hero,
        );
        self.wallfill.rebind_occ(&self.device, &wallocc);
        self.fluid = fluid;
        self.particles = particles;
        self.slice = slice;
        self.diffuse = diffuse;
        self.wetwall = wetwall;
        self.wallocc = wallocc;
        // Drop temporal history on Reset so the first post-reset frame is clean.
        self.temporal.invalidate_history();
        self.caustics.invalidate_history();
        self.prev_eye_to_world = Mat4::IDENTITY;
        self.diffuse_frame = 0;
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
        self.particles.set_particle_volume(
            represented_liquid_volume(scene) / (self.fluid.particle_count().max(1) as f32),
        );
        self.particles.set_radius_scale(self.particle_size);
        self.particles.set_speed_scale(self.speed_scale);
        self.particles
            .set_particle_colors(self.particle_slow_rgb, self.particle_fast_rgb);
        self.particles
            .set_water_optical_density(self.water_optical_density);
        self.particles.set_edge_inner(self.particle_edge);
        self.particles.set_shading(self.particle_shading);
        // Restore splat_scale so a user-tuned render.hero.smooth_thickness_splat_scale
        // survives Reset / scene change (ParticleRenderer::new resets it to 1.3).
        self.particles.splat_scale = self.hero.smooth_thickness_splat_scale;
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
        fluid::max_tiled_particle_dispatch_count(self.caps.max_compute_workgroups_per_dimension)
    }

    pub fn max_particle_storage_count(&self) -> u32 {
        max_particle_storage_count_for(&self.caps)
    }

    pub fn particle_dispatch_groups(&self) -> [u32; 2] {
        let shape = self.fluid.particle_dispatch_shape();
        [shape.groups_x, shape.groups_y]
    }

    pub fn particle_dispatch_capacity(&self) -> u32 {
        self.fluid.particle_dispatch_shape().capacity
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
        self.depth_view = create_depth(&self.device, width, height);
        self.thickness_view = create_r16_target(&self.device, width, height, "water thickness");
        self.whitewater_view = create_r16_target(&self.device, width, height, "water whitewater");
        self.wallfill_mask_view = create_r16_target(&self.device, width, height, "wallfill mask");
        self.nearest_z_view = create_r16_target(&self.device, width, height, "water nearest z");
        self.smooth_z_ping_view =
            create_r16_target(&self.device, width, height, "water smooth z ping");
        self.smooth_z_view = create_r16_target(&self.device, width, height, "water smooth z");
        self.scene_color_view = create_scene_color_target(&self.device, width, height);
        self.scene_depth_view = create_r16_target(&self.device, width, height, "hero scene depth");
        // Rebuild temporal system first so stable views are available for caustics/composite.
        self.temporal.set_views(
            &self.device,
            &self.thickness_view,
            &self.smooth_z_view,
            &self.whitewater_view,
            &self.hero,
            width,
            height,
        );
        // Always bind the stable views regardless of temporal_enabled.
        // When temporal is disabled, alpha=0 makes stable==raw each frame (pass-through).
        // Gating here on temporal_enabled causes inconsistency: construction and the
        // Outdated branch always use stable views, but after a resize with temporal off
        // composite would be reading raw views and enabling temporal Live would have no
        // effect until the next resize. Remove the gate to keep all code-paths consistent.
        self.composite.set_views(
            &self.device,
            self.temporal.stable_thickness(),
            self.temporal.stable_whitewater(),
            self.temporal.stable_smooth_z(),
            &self.wallfill_mask_view,
            &self.scene_color_view,
            &self.scene_depth_view,
        );
        self.composite.set_size(&self.queue, width, height);
        self.smoothing.set_views(
            &self.device,
            &self.nearest_z_view,
            &self.smooth_z_ping_view,
            &self.smooth_z_view,
        );
        self.thickness_smoothing.set_views(
            &self.device,
            &self.thickness_view,
            &self.smooth_z_ping_view,
        );
        self.whitewater_smoothing.set_views(
            &self.device,
            &self.whitewater_view,
            &self.smooth_z_ping_view,
        );
        self.caustics.set_views(
            &self.device,
            self.temporal.stable_smooth_z(),
            self.temporal.stable_thickness(),
            &self.scene_depth_view,
            width,
            height,
        );
        self.caustics.set_hero_params(&self.queue, &self.hero);
        self.prev_eye_to_world = Mat4::IDENTITY;
    }

    pub fn reset(&mut self) {
        self.fluid.reset(&self.queue);
        // Drop temporal history on Reset so the first post-reset frame is clean.
        self.temporal.invalidate_history();
        self.caustics.invalidate_history();
        self.prev_eye_to_world = Mat4::IDENTITY;
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

    pub fn set_particle_view(&mut self, v: u32) {
        self.render_mode = RenderMode::from_u32(v);
    }

    /// Mirror the Water-tab settings into the hero uniform + environment params.
    /// Called whenever a `render.hero.*` slider changes (all Live).
    pub fn set_hero_params(&mut self, hero: settings::HeroParams) {
        self.hero = hero;
        self.composite.set_hero_params(&self.queue, &hero);
        self.environment.set_params(&self.queue, &hero);
        self.skybox.set_params(&self.queue, &hero);
        self.caustics.set_hero_params(&self.queue, &hero);
        self.wetwall.set_params(&self.queue, &hero);
        self.smoothing
            .update_radius(&self.queue, hero.smooth_radius);
        self.thickness_smoothing
            .update_radius(&self.queue, hero.smooth_radius);
        self.whitewater_smoothing
            .update_radius(&self.queue, hero.smooth_radius);
        self.particles.splat_scale = hero.smooth_thickness_splat_scale;
    }

    /// Mirror the Water-tab diffuse (foam/spray/bubble) settings into the diffuse
    /// system. Called whenever a `render.diffuse.*` slider changes (all Live).
    pub fn set_diffuse_params(&mut self, params: settings::DiffuseParams) {
        self.diffuse.set_params(params);
    }

    /// Advance the diffuse-water particles by `dt` seconds (emit + update). Runs in
    /// its own command encoder, outside the timestamped sim passes. No-op while the
    /// feature is disabled.
    pub fn update_diffuse(&mut self, dt: f32) {
        self.diffuse_frame = self.diffuse_frame.wrapping_add(1);
        self.diffuse
            .record_step(&self.device, &self.queue, dt, self.diffuse_frame);
    }

    /// Advance the wet-wall wetness field by `dt` seconds. Reads the current
    /// `cell_type` buffer (post-substep, latest classification) and decays/updates
    /// the persistent wetness target. Runs in its own encoder, after `update_diffuse`
    /// and before the render prepass.
    pub fn update_wetwall(&mut self, dt: f32) {
        let (tank_lo, tank_hi) = self.fluid.tank_bounds();
        let [nx, ny, nz] = self.fluid.grid_dims();
        self.wetwall.record_step(
            &self.device,
            &self.queue,
            dt,
            &self.hero,
            nx,
            ny,
            nz,
            tank_lo,
            tank_hi,
        );
    }

    /// Update the wall occupancy buffer (v1.21 gap-fill). Reads current `cell_type`
    /// and writes dense per-wall-cell occupancy. Runs after `update_wetwall`,
    /// before the render prepass. No-op when fill is disabled.
    pub fn update_wallocc(&mut self) {
        self.wallocc
            .record_step(&self.device, &self.queue, &self.hero);
    }

    pub fn set_particle_slow_color(&mut self, rgb: [f32; 3]) {
        self.particle_slow_rgb = rgb;
        self.particles
            .set_particle_colors(self.particle_slow_rgb, self.particle_fast_rgb);
        self.composite.set_tint(&self.queue, rgb);
    }

    pub fn set_particle_fast_color(&mut self, rgb: [f32; 3]) {
        self.particle_fast_rgb = rgb;
        self.particles
            .set_particle_colors(self.particle_slow_rgb, self.particle_fast_rgb);
    }

    pub fn set_water_optical_density(&mut self, density: f32) {
        self.water_optical_density = density;
        self.particles.set_water_optical_density(density);
        self.composite.set_optical_density(&self.queue, density);
    }

    pub fn set_particle_edge(&mut self, v: f32) {
        self.particle_edge = v;
        self.particles.set_edge_inner(v);
    }

    pub fn set_particle_shading(&mut self, v: f32) {
        self.particle_shading = v;
        self.particles.set_shading(v);
        self.composite.set_shading(&self.queue, v);
    }

    pub fn set_whitewater_strength(&mut self, v: f32) {
        self.whitewater_strength = v;
        self.composite.set_whitewater_strength(&self.queue, v);
    }

    pub fn set_whitewater_threshold(&mut self, v: f32) {
        self.whitewater_threshold = v;
        self.composite.set_whitewater_threshold(&self.queue, v);
    }

    pub fn set_whitewater_softness(&mut self, v: f32) {
        self.whitewater_softness = v;
        self.composite.set_whitewater_softness(&self.queue, v);
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

    /// Opaque pass for the optical/simple particle modes: clear the swapchain +
    /// depth and draw the wireframe tank directly. (The Water mode draws into the
    /// offscreen scene prepass instead.)
    fn record_opaque_into_view(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
    ) {
        const CLEAR: wgpu::Color = wgpu::Color {
            r: 0.04,
            g: 0.05,
            b: 0.08,
            a: 1.0,
        };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("opaque pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
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
            timestamp_writes: self.timers.as_ref().map(|t| t.render_begin_writes()),
            occlusion_query_set: None,
            multiview_mask: None,
        });
        self.wireframe.draw(&mut pass);
    }

    pub fn render(
        &mut self,
        view_proj: &Mat4,
        cam_right: Vec3,
        cam_up: Vec3,
        eye_to_world: &Mat4,
        eye_world: Vec3,
        box_pos: Vec3,
        box_orient: glam::Quat,
    ) -> Result<(), String> {
        use wgpu::CurrentSurfaceTexture as Cur;
        let frame = match self.surface.get_current_texture() {
            Cur::Success(t) | Cur::Suboptimal(t) => t,
            Cur::Outdated | Cur::Lost => {
                self.surface.configure(&self.device, &self.config);
                self.depth_view = create_depth(&self.device, self.config.width, self.config.height);
                self.thickness_view = create_r16_target(
                    &self.device,
                    self.config.width,
                    self.config.height,
                    "water thickness",
                );
                self.whitewater_view = create_r16_target(
                    &self.device,
                    self.config.width,
                    self.config.height,
                    "water whitewater",
                );
                self.wallfill_mask_view = create_r16_target(
                    &self.device,
                    self.config.width,
                    self.config.height,
                    "wallfill mask",
                );
                self.nearest_z_view = create_r16_target(
                    &self.device,
                    self.config.width,
                    self.config.height,
                    "water nearest z",
                );
                self.smooth_z_ping_view = create_r16_target(
                    &self.device,
                    self.config.width,
                    self.config.height,
                    "water smooth z ping",
                );
                self.smooth_z_view = create_r16_target(
                    &self.device,
                    self.config.width,
                    self.config.height,
                    "water smooth z",
                );
                self.scene_color_view =
                    create_scene_color_target(&self.device, self.config.width, self.config.height);
                self.scene_depth_view = create_r16_target(
                    &self.device,
                    self.config.width,
                    self.config.height,
                    "hero scene depth",
                );
                // Rebuild temporal first; stable views feed composite + caustics.
                self.temporal.set_views(
                    &self.device,
                    &self.thickness_view,
                    &self.smooth_z_view,
                    &self.whitewater_view,
                    &self.hero,
                    self.config.width,
                    self.config.height,
                );
                self.composite.set_views(
                    &self.device,
                    self.temporal.stable_thickness(),
                    self.temporal.stable_whitewater(),
                    self.temporal.stable_smooth_z(),
                    &self.wallfill_mask_view,
                    &self.scene_color_view,
                    &self.scene_depth_view,
                );
                self.composite
                    .set_size(&self.queue, self.config.width, self.config.height);
                self.smoothing.set_views(
                    &self.device,
                    &self.nearest_z_view,
                    &self.smooth_z_ping_view,
                    &self.smooth_z_view,
                );
                self.thickness_smoothing.set_views(
                    &self.device,
                    &self.thickness_view,
                    &self.smooth_z_ping_view,
                );
                self.whitewater_smoothing.set_views(
                    &self.device,
                    &self.whitewater_view,
                    &self.smooth_z_ping_view,
                );
                self.caustics.set_views(
                    &self.device,
                    self.temporal.stable_smooth_z(),
                    self.temporal.stable_thickness(),
                    &self.scene_depth_view,
                    self.config.width,
                    self.config.height,
                );
                self.caustics.set_hero_params(&self.queue, &self.hero);
                self.prev_eye_to_world = Mat4::IDENTITY;
                return Ok(());
            }
            Cur::Timeout | Cur::Occluded => return Ok(()),
            Cur::Validation => return Err("surface validation error".to_string()),
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        self.wireframe.update_camera(&self.queue, view_proj);
        self.environment.update_camera(&self.queue, view_proj);
        // Push box-local camera eye position + box rotation to the environment
        // shader.  The environment mesh vertices are in box-local space, so we
        // need the eye in the same frame for a geometrically correct view_dir.
        // The box rotation mat is passed so the box-local reflection direction
        // can be rotated into world space before env_sample (which expects world).
        let eye_world_local = box_orient.inverse() * (eye_world - box_pos);
        let box_rot = glam::Mat3::from_quat(box_orient);
        self.environment
            .set_eye_world(&self.queue, eye_world_local, box_rot);
        // Camera-only eye->world rotation for the world-fixed environment: the
        // reflected env + skybox follow the camera but NOT the tank's rotation.
        // Also pass box-local eye + box_rot + tank bounds for the flat-water snap.
        let (tank_lo_arr, tank_hi_arr) = self.fluid.tank_bounds();
        self.composite.set_camera(
            &self.queue,
            eye_to_world,
            eye_world_local,
            box_rot,
            tank_lo_arr,
            tank_hi_arr,
            &self.hero,
        );
        // Push wallfill camera uniform (v1.21).
        self.wallfill.set_camera(
            &self.queue,
            eye_to_world,
            eye_world_local,
            box_rot,
            self.config.width,
            self.config.height,
            &self.hero,
        );
        self.skybox
            .set_camera(&self.queue, eye_to_world, self.aspect());
        self.caustics.cache_eye_to_world(eye_to_world);
        self.caustics.set_camera(&self.queue, eye_to_world);
        self.particles
            .update_camera(&self.queue, view_proj, cam_right, cam_up);
        self.diffuse
            .update_camera(&self.queue, view_proj, cam_right, cam_up);
        self.slice.update_camera(&self.queue, view_proj);

        // v1.18 Camera motion detection (CPU-side, no GPU readback).
        // eye_to_world is model-free (camera only), so we can compute a clean delta.
        let cam_reset = compute_camera_reset(
            &self.prev_eye_to_world,
            eye_to_world,
            self.hero.temporal_camera_motion_reset_threshold,
        );
        self.prev_eye_to_world = *eye_to_world;

        // Update temporal uniforms before issuing passes.
        self.temporal.update_params(
            &self.queue,
            &self.hero,
            cam_reset,
            self.config.width,
            self.config.height,
        );

        const CLEAR: wgpu::Color = wgpu::Color {
            r: 0.04,
            g: 0.05,
            b: 0.08,
            a: 1.0,
        };
        // Eye-distance "no geometry" sentinel for the scene_depth target (matches
        // the R16Float convention used by nearest_z / smooth_z).
        const SCENE_DEPTH_FAR: wgpu::Color = wgpu::Color {
            r: 65504.0,
            g: 0.0,
            b: 0.0,
            a: 0.0,
        };

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame"),
            });

        match self.render_mode {
            RenderMode::Water => {
                // Scene prepass: environment + wireframe into scene_color +
                // scene_depth, ahead of the water passes.
                {
                    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("hero scene prepass"),
                        color_attachments: &[
                            Some(wgpu::RenderPassColorAttachment {
                                view: &self.scene_color_view,
                                resolve_target: None,
                                depth_slice: None,
                                ops: wgpu::Operations {
                                    load: wgpu::LoadOp::Clear(CLEAR),
                                    store: wgpu::StoreOp::Store,
                                },
                            }),
                            Some(wgpu::RenderPassColorAttachment {
                                view: &self.scene_depth_view,
                                resolve_target: None,
                                depth_slice: None,
                                ops: wgpu::Operations {
                                    load: wgpu::LoadOp::Clear(SCENE_DEPTH_FAR),
                                    store: wgpu::StoreOp::Store,
                                },
                            }),
                        ],
                        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                            view: &self.depth_view,
                            depth_ops: Some(wgpu::Operations {
                                load: wgpu::LoadOp::Clear(1.0),
                                store: wgpu::StoreOp::Store,
                            }),
                            stencil_ops: None,
                        }),
                        timestamp_writes: self.timers.as_ref().map(|t| t.render_begin_writes()),
                        occlusion_query_set: None,
                        multiview_mask: None,
                    });
                    // World background first (fills scene_color behind geometry,
                    // far depth), then the tank floor/walls + wireframe over it.
                    if self.skybox.enabled() {
                        self.skybox.draw(&mut pass);
                    }
                    self.environment.draw(&mut pass);
                    self.wireframe.draw_scene(&mut pass);
                }
                {
                    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("water thickness pass"),
                        color_attachments: &[
                            Some(wgpu::RenderPassColorAttachment {
                                view: &self.thickness_view,
                                resolve_target: None,
                                depth_slice: None,
                                ops: wgpu::Operations {
                                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                                    store: wgpu::StoreOp::Store,
                                },
                            }),
                            Some(wgpu::RenderPassColorAttachment {
                                view: &self.nearest_z_view,
                                resolve_target: None,
                                depth_slice: None,
                                ops: wgpu::Operations {
                                    load: wgpu::LoadOp::Clear(wgpu::Color {
                                        r: 65504.0,
                                        g: 0.0,
                                        b: 0.0,
                                        a: 0.0,
                                    }),
                                    store: wgpu::StoreOp::Store,
                                },
                            }),
                            Some(wgpu::RenderPassColorAttachment {
                                view: &self.whitewater_view,
                                resolve_target: None,
                                depth_slice: None,
                                ops: wgpu::Operations {
                                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                                    store: wgpu::StoreOp::Store,
                                },
                            }),
                        ],
                        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                            view: &self.depth_view,
                            depth_ops: Some(wgpu::Operations {
                                load: wgpu::LoadOp::Load,
                                store: wgpu::StoreOp::Store,
                            }),
                            stencil_ops: None,
                        }),
                        timestamp_writes: None,
                        occlusion_query_set: None,
                        multiview_mask: None,
                    });
                    self.particles
                        .draw_thickness(&mut pass, self.fluid.particle_count());
                }
                // v1.21 Wall-fill injection pass: runs AFTER particle thickness, BEFORE
                // bilateral smoothing. Injects a flat glass-plane surface into the same
                // three MRT targets (thickness Add, nearest_z Min, whitewater Add) wherever
                // the occupancy buffer reports liquid against the wall, and writes a
                // separate wall-fill mask for composite-time color/reflection controls.
                // It still runs when disabled so the mask target is cleared every frame.
                {
                    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("wallfill injection pass"),
                        color_attachments: &[
                            Some(wgpu::RenderPassColorAttachment {
                                view: &self.thickness_view,
                                resolve_target: None,
                                depth_slice: None,
                                ops: wgpu::Operations {
                                    load: wgpu::LoadOp::Load,
                                    store: wgpu::StoreOp::Store,
                                },
                            }),
                            Some(wgpu::RenderPassColorAttachment {
                                view: &self.nearest_z_view,
                                resolve_target: None,
                                depth_slice: None,
                                ops: wgpu::Operations {
                                    load: wgpu::LoadOp::Load,
                                    store: wgpu::StoreOp::Store,
                                },
                            }),
                            Some(wgpu::RenderPassColorAttachment {
                                view: &self.whitewater_view,
                                resolve_target: None,
                                depth_slice: None,
                                ops: wgpu::Operations {
                                    load: wgpu::LoadOp::Load,
                                    store: wgpu::StoreOp::Store,
                                },
                            }),
                            Some(wgpu::RenderPassColorAttachment {
                                view: &self.wallfill_mask_view,
                                resolve_target: None,
                                depth_slice: None,
                                ops: wgpu::Operations {
                                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                                    store: wgpu::StoreOp::Store,
                                },
                            }),
                        ],
                        depth_stencil_attachment: None,
                        timestamp_writes: None,
                        occlusion_query_set: None,
                        multiview_mask: None,
                    });
                    self.wallfill.draw(&mut pass);
                }
                // v1.22 Thickness smoothing: a plain separable Gaussian over the
                // thickness target, run AFTER particle + wall-fill thickness writes
                // and BEFORE depth smoothing (so it can reuse the depth pass's
                // smooth_z_ping scratch). Raw thickness drives Beer-Lambert opacity
                // in the composite, so its per-particle splat noise was the source of
                // the speckled "sandy" body and the see-through gap where dark wall
                // showed between splats near the glass. Each iteration blurs in place:
                // X reads thickness -> writes ping; Y reads ping -> writes thickness.
                let smooth_iters = self.hero.smooth_iterations.max(1);
                for _ in 0..smooth_iters {
                    {
                        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                            label: Some("thickness smooth x pass"),
                            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                view: &self.smooth_z_ping_view,
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
                        self.thickness_smoothing.draw_x(&mut pass);
                    }
                    {
                        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                            label: Some("thickness smooth y pass"),
                            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                view: &self.thickness_view,
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
                        self.thickness_smoothing.draw_y(&mut pass);
                    }
                }
                // Whitewater/foam smoothing: same in-place plain Gaussian, reusing the
                // shared ping scratch. The whitewater target is a per-particle
                // speed-weighted accumulation; raw, it reads as a field of white foam
                // speckle dots on moving water. Blurring it makes foam coherent regions.
                for _ in 0..smooth_iters {
                    {
                        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                            label: Some("whitewater smooth x pass"),
                            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                view: &self.smooth_z_ping_view,
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
                        self.whitewater_smoothing.draw_x(&mut pass);
                    }
                    {
                        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                            label: Some("whitewater smooth y pass"),
                            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                view: &self.whitewater_view,
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
                        self.whitewater_smoothing.draw_y(&mut pass);
                    }
                }
                // Iterated bilateral depth smoothing. Iteration 0 reads nearest_z;
                // iterations 1+ compound the result in smooth_z via a second X bind group.
                for iter in 0..smooth_iters {
                    {
                        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                            label: Some("water smooth x pass"),
                            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                view: &self.smooth_z_ping_view,
                                resolve_target: None,
                                depth_slice: None,
                                ops: wgpu::Operations {
                                    load: wgpu::LoadOp::Clear(wgpu::Color {
                                        r: 65504.0,
                                        g: 0.0,
                                        b: 0.0,
                                        a: 0.0,
                                    }),
                                    store: wgpu::StoreOp::Store,
                                },
                            })],
                            depth_stencil_attachment: None,
                            timestamp_writes: None,
                            occlusion_query_set: None,
                            multiview_mask: None,
                        });
                        if iter == 0 {
                            self.smoothing.draw_x_first(&mut pass);
                        } else {
                            self.smoothing.draw_x_iter(&mut pass);
                        }
                    }
                    {
                        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                            label: Some("water smooth y pass"),
                            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                view: &self.smooth_z_view,
                                resolve_target: None,
                                depth_slice: None,
                                ops: wgpu::Operations {
                                    load: wgpu::LoadOp::Clear(wgpu::Color {
                                        r: 65504.0,
                                        g: 0.0,
                                        b: 0.0,
                                        a: 0.0,
                                    }),
                                    store: wgpu::StoreOp::Store,
                                },
                            })],
                            depth_stencil_attachment: None,
                            timestamp_writes: None,
                            occlusion_query_set: None,
                            multiview_mask: None,
                        });
                        self.smoothing.draw_y(&mut pass);
                    }
                }
                // v1.18 Temporal stabilization passes: blend thickness, smooth_z, whitewater
                // with their histories. Runs AFTER smooth-y writes smooth_z_view and AFTER
                // the thickness pass writes thickness/whitewater. Stable views feed caustics
                // and composite downstream.
                self.temporal.draw(&mut encoder);

                // Per-frame rebind of composite + caustics to the freshly-written stable
                // textures. draw() flips ping-pong parity so stable_view() now returns the
                // texture that was WRITTEN this frame; downstream bind groups built at
                // construction/resize are permanently wired to whichever side was "stable"
                // at that moment and become stale every other frame without this rebind.
                {
                    let (st, sz, sw) = self.temporal.stable_views();
                    self.composite.rebind_temporal_views(
                        &self.device,
                        st,
                        sw,
                        sz,
                        &self.wallfill_mask_view,
                        &self.scene_color_view,
                        &self.scene_depth_view,
                    );
                    self.caustics.rebind_input_views(&self.device, sz, st);
                }

                // Caustics pass (A): half-res generation into caustic ping/pong target.
                // Caustics pass (B): additive composite into scene_color so the water
                // composite (pass 5 below) picks up the caustic lighting via refract_uv.
                if self.hero.caustics_enabled {
                    // Determine the generation target BEFORE draw_generate flips parity,
                    // then record the pass (draw_generate borrows &mut self.caustics and
                    // flips parity internally).
                    let gen_target_is_ping = !self.caustics.frame_parity();
                    {
                        let gen_view = if gen_target_is_ping {
                            self.caustics.ping_view()
                        } else {
                            self.caustics.pong_view()
                        };
                        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                            label: Some("caustics generate pass"),
                            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                view: gen_view,
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
                        self.caustics
                            .draw_generate(&self.queue, &mut pass, cam_reset);
                    }
                    {
                        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                            label: Some("caustics composite pass"),
                            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                view: &self.scene_color_view,
                                resolve_target: None,
                                depth_slice: None,
                                ops: wgpu::Operations {
                                    load: wgpu::LoadOp::Load,
                                    store: wgpu::StoreOp::Store,
                                },
                            })],
                            depth_stencil_attachment: None,
                            timestamp_writes: None,
                            occlusion_query_set: None,
                            multiview_mask: None,
                        });
                        self.caustics.draw_composite(&mut pass);
                    }
                }
                let diffuse_active = self.diffuse.enabled();
                {
                    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("water composite pass"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: &view,
                            resolve_target: None,
                            depth_slice: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Load,
                                store: wgpu::StoreOp::Store,
                            },
                        })],
                        depth_stencil_attachment: None,
                        // render_end goes on the LAST render pass: diffuse if active,
                        // else slice if enabled, else this composite.
                        timestamp_writes: if self.slice_enabled || diffuse_active {
                            None
                        } else {
                            self.timers.as_ref().map(|t| t.render_end_writes())
                        },
                        occlusion_query_set: None,
                        multiview_mask: None,
                    });
                    self.composite.draw(&mut pass);
                }
                // Persistent foam/spray/bubbles over the composite, depth-tested
                // against the shared scene depth (environment + wireframe).
                if diffuse_active {
                    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("diffuse particle pass"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: &view,
                            resolve_target: None,
                            depth_slice: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Load,
                                store: wgpu::StoreOp::Store,
                            },
                        })],
                        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                            view: &self.depth_view,
                            depth_ops: Some(wgpu::Operations {
                                load: wgpu::LoadOp::Load,
                                store: wgpu::StoreOp::Store,
                            }),
                            stencil_ops: None,
                        }),
                        timestamp_writes: if self.slice_enabled {
                            None
                        } else {
                            self.timers.as_ref().map(|t| t.render_end_writes())
                        },
                        occlusion_query_set: None,
                        multiview_mask: None,
                    });
                    self.diffuse.draw(&mut pass);
                }
            }
            RenderMode::OpticalParticles => {
                self.record_opaque_into_view(&mut encoder, &view);
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("optical particle pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        depth_slice: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: &self.depth_view,
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        }),
                        stencil_ops: None,
                    }),
                    timestamp_writes: if self.slice_enabled {
                        None
                    } else {
                        self.timers.as_ref().map(|t| t.render_end_writes())
                    },
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                self.particles.draw(&mut pass, self.fluid.particle_count());
            }
            RenderMode::SimpleParticles => {
                self.record_opaque_into_view(&mut encoder, &view);
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("simple particle pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        depth_slice: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: &self.depth_view,
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        }),
                        stencil_ops: None,
                    }),
                    timestamp_writes: if self.slice_enabled {
                        None
                    } else {
                        self.timers.as_ref().map(|t| t.render_end_writes())
                    },
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                self.particles
                    .draw_simple(&mut pass, self.fluid.particle_count());
            }
        }

        if self.slice_enabled {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("slice overlay pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: self.timers.as_ref().map(|t| t.render_end_writes()),
                occlusion_query_set: None,
                multiview_mask: None,
            });
            self.slice.draw(&mut pass);
        }

        // Throttled GPU timing + liveness readback (the only allowed readback).
        let initiated = self
            .timers
            .as_ref()
            .map(|t| {
                t.record_resolve_and_maybe_copy(
                    &mut encoder,
                    self.fluid.stats_buffer(),
                    self.diffuse.counters_buffer(),
                )
            })
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

fn create_scene_color_target(device: &wgpu::Device, width: u32, height: u32) -> wgpu::TextureView {
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("hero scene color"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: SCENE_COLOR_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    tex.create_view(&wgpu::TextureViewDescriptor::default())
}

fn create_r16_target(
    device: &wgpu::Device,
    width: u32,
    height: u32,
    label: &'static str,
) -> wgpu::TextureView {
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::R16Float,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    tex.create_view(&wgpu::TextureViewDescriptor::default())
}

fn represented_liquid_volume(scene: &SceneConfig) -> f32 {
    let h = crate::sim::H;
    let extent = Vec3::new(
        scene.grid_resolution.x as f32 * h,
        scene.grid_resolution.y as f32 * h,
        scene.grid_resolution.z as f32 * h,
    );
    scene
        .initial_liquid
        .blocks
        .iter()
        .map(|b| {
            let e = (b.max - b.min).max(Vec3::ZERO) * extent;
            (e.x * e.y * e.z).max(0.0)
        })
        .sum::<f32>()
        .max(1e-6)
}

fn max_particle_storage_count_for(caps: &GpuCaps) -> u32 {
    let bytes = caps
        .max_storage_buffer_binding_size
        .min(caps.max_buffer_size);
    (bytes / 32).min(u32::MAX as u64) as u32
}

fn validate_particle_scale(
    requested_particles: u32,
    estimated_particles: u32,
    max_compute_workgroups_per_dimension: u32,
    storage_limit: u32,
) -> Result<(), String> {
    let dispatch_limit =
        fluid::max_tiled_particle_dispatch_count(max_compute_workgroups_per_dimension);
    if estimated_particles > dispatch_limit
        || fluid::particle_dispatch_shape(estimated_particles, max_compute_workgroups_per_dimension)
            .is_none()
    {
        return Err(format!(
            "requested {} particles seeds {}, exceeding the tiled particle dispatch capacity {} (max {} workgroups per dimension, {} threads/workgroup, u32 shader index)",
            requested_particles,
            estimated_particles,
            dispatch_limit,
            max_compute_workgroups_per_dimension,
            fluid::PARTICLE_WG,
        ));
    }
    if estimated_particles > storage_limit {
        return Err(format!(
            "requested {} particles seeds {}, exceeding the single particle storage binding limit {}",
            requested_particles, estimated_particles, storage_limit,
        ));
    }
    Ok(())
}

/// Compute whether the camera has moved enough to require a temporal history reset.
///
/// Uses the model-free `eye_to_world` (camera rotation + position, no box orientation)
/// so box tilting does not spuriously trigger resets. The metric combines:
///   rot_angle  = acos(clamp((trace(R_prev^T * R_cur) - 1) / 2, -1, 1))
///   pos_delta  = |cur.w_axis.xyz - prev.w_axis.xyz|
/// and returns `true` if either `rot_angle > threshold` OR `pos_delta > threshold`.
/// A scale factor of 0.5 is applied to pos_delta relative to threshold so that a
/// small dolly does not trigger a spurious reset while large pans/zooms still do.
fn compute_camera_reset(prev: &Mat4, cur: &Mat4, threshold: f32) -> bool {
    if threshold <= 0.0 {
        return false;
    }
    // Extract 3×3 rotation columns from the mat4 (eye→world columns 0,1,2).
    let pr = glam::Mat3::from_cols(
        prev.x_axis.truncate(),
        prev.y_axis.truncate(),
        prev.z_axis.truncate(),
    );
    let cr = glam::Mat3::from_cols(
        cur.x_axis.truncate(),
        cur.y_axis.truncate(),
        cur.z_axis.truncate(),
    );
    // R_prev^T * R_cur
    let rel = pr.transpose() * cr;
    // trace
    let tr = rel.x_axis.x + rel.y_axis.y + rel.z_axis.z;
    let cos_angle = ((tr - 1.0) / 2.0).clamp(-1.0, 1.0);
    let rot_angle = cos_angle.acos();

    // Eye-position delta (dolly / pan / zoom translate).
    // Scale matches rotation: threshold in radians ≈ threshold in world units
    // (a 1-radian orbit and a 1-unit dolly are treated as equally significant).
    let pos_delta = (cur.w_axis.truncate() - prev.w_axis.truncate()).length();

    rot_angle > threshold || pos_delta > threshold
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
        "            max_compute_workgroups_per_dimension={} tiled_particle_dispatch_capacity={}",
        limits.max_compute_workgroups_per_dimension,
        fluid::max_tiled_particle_dispatch_count(limits.max_compute_workgroups_per_dimension),
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
