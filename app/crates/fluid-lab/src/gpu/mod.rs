//! WebGPU context: adapter/device init, surface configuration, boot diagnostics,
//! the compute/integer-atomic smoke test, the GPU fluid sim, and rendering.

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
mod timing;

pub use timing::Readout as GpuReadout;
pub use timing::FINE_SECTIONS;

pub(crate) use fluid::effective_rest_density;

use crate::log;
use crate::scene::SceneConfig;
use crate::settings::{self, Registry};
use glam::{Mat4, Vec3};
use std::cell::Cell;
use std::sync::{
    atomic::{AtomicU8, Ordering},
    Arc,
};

const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
/// Offscreen scene-color format for the hero-water prepass: linear HDR so the
/// environment + wireframe can be sampled (and refraction-tapped) before the
/// water composites over it. See [`composite`].
const SCENE_COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// Splat radius as a fraction of the seeded inter-particle lattice spacing.
///
/// The visible water body is built from screen-space-smoothed particle splats, so
/// coverage stays roughly constant only when the splat radius tracks the spacing
/// between particles. The base splat radius is `seeded_spacing * SPLAT_RADIUS_PER_SPACING`
/// (the user's `render.particle_size` is a Live multiplier on top). This makes
/// density a volume-neutral fidelity knob: lowering `particles.density` coarsens
/// the lattice (larger spacing) and the splats grow to keep the body the same size,
/// just blobbier.
///
/// Calibration: at the reference density (8/cell) the spacing is `H * 8^(-1/3) =
/// H * 0.5`, so `0.5 * SPLAT_RADIUS_PER_SPACING` must equal the historical
/// `H * 0.35` → `SPLAT_RADIUS_PER_SPACING = 0.7`. Tune this single constant if the
/// coverage sweep shows low density under- or over-covering. (Equivalent to the
/// plan's `k_radius` expressed as a ratio to spacing rather than to `H`.)
const SPLAT_RADIUS_PER_SPACING: f32 = 0.7;

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

#[derive(Clone, Copy, Default)]
pub struct GpuMemoryStats {
    pub sim_buffers_bytes: u64,
    pub render_targets_bytes: u64,
    pub diffuse_bytes: u64,
    pub timing_bytes: u64,
    pub total_tracked_bytes: u64,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum GpuDeviceStatus {
    Ok = 0,
    SurfaceLost = 1,
    DeviceLost = 2,
    ValidationError = 3,
}

impl GpuDeviceStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            GpuDeviceStatus::Ok => "ok",
            GpuDeviceStatus::SurfaceLost => "surface-lost",
            GpuDeviceStatus::DeviceLost => "device-lost",
            GpuDeviceStatus::ValidationError => "validation-error",
        }
    }

    fn from_u8(value: u8) -> Self {
        match value {
            1 => GpuDeviceStatus::SurfaceLost,
            2 => GpuDeviceStatus::DeviceLost,
            3 => GpuDeviceStatus::ValidationError,
            _ => GpuDeviceStatus::Ok,
        }
    }

    fn fatal(self) -> bool {
        matches!(
            self,
            GpuDeviceStatus::DeviceLost | GpuDeviceStatus::ValidationError
        )
    }
}

thread_local! {
    static LATEST_MEMORY_STATS: Cell<GpuMemoryStats> = Cell::new(GpuMemoryStats {
        sim_buffers_bytes: 0,
        render_targets_bytes: 0,
        diffuse_bytes: 0,
        timing_bytes: 0,
        total_tracked_bytes: 0,
    });
}

pub fn latest_memory_stats() -> GpuMemoryStats {
    LATEST_MEMORY_STATS.with(Cell::get)
}

pub struct GpuContext {
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    depth_view: wgpu::TextureView,
    thickness_view: wgpu::TextureView,
    whitewater_view: wgpu::TextureView,
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
    /// Persistent surface-foam particles — render-only.
    diffuse: diffuse::DiffuseSystem,
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
    device_status: Arc<AtomicU8>,
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
        let device_status = Arc::new(AtomicU8::new(GpuDeviceStatus::Ok as u8));
        let lost_status = device_status.clone();
        device.set_device_lost_callback(move |_reason, _message| {
            lost_status.store(GpuDeviceStatus::DeviceLost as u8, Ordering::Relaxed);
        });

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
        let nearest_z_view = create_r16_target(&device, width, height, "water nearest z");
        let smooth_z_ping_view = create_r16_target(&device, width, height, "water smooth z ping");
        let smooth_z_view = create_r16_target(&device, width, height, "water smooth z");
        let scene_color_view = create_scene_color_target(&device, width, height);
        let scene_depth_view = create_r16_target(&device, width, height, "hero scene depth");

        // Resolved spawn target (density-derived or advanced override). `scene`
        // owns the derivation; this is the count we report as "requested".
        let requested_particles = scene.particle_count;
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
            caps.max_storage_buffers_per_stage,
            caps.max_buffer_size,
            settings.particle_sort(),
            settings.particle_sort_period(),
        );
        // Splat radius tracks the seeded inter-particle spacing so density is
        // volume-neutral (see SPLAT_RADIUS_PER_SPACING). At the reference density
        // this equals the historical H*0.35; the user's render.particle_size is a
        // Live multiplier applied on top via set_radius_scale below.
        let particle_radius = scene.seeded_spacing(settings) * SPLAT_RADIUS_PER_SPACING;

        let (tank_lo, tank_hi) = fluid.tank_bounds();
        let [grid_nx_init, grid_ny_init, grid_nz_init] = fluid.grid_dims();
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
            hero.feature_preservation,
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

        let composite = composite::CompositeRenderer::new(
            &device,
            format,
            &thickness_view,
            &whitewater_view,
            &smooth_z_view,
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
            device_status,
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
        // Resolved spawn target (density-derived or advanced override) from `scene`.
        let requested_particles = scene.particle_count;
        let dispatch_limit = self.max_particle_dispatch_count();
        let storage_limit = self.max_particle_storage_count();
        if let Err(e) = validate_particle_scale(
            requested_particles,
            estimated,
            self.caps.max_compute_workgroups_per_dimension,
            storage_limit,
        ) {
            let status = if estimated > dispatch_limit {
                "rejected-dispatch-capacity"
            } else {
                "rejected-storage-binding-limit"
            };
            log(&format!(
                "[fluid-lab][scale] rejected reset preflight: {status}"
            ));
            return Err(e);
        }
        let fluid = fluid::GpuFluid::new(
            &self.device,
            &self.queue,
            settings,
            scene,
            self.caps.max_compute_workgroups_per_dimension,
            self.caps.max_storage_buffers_per_stage,
            self.caps.max_buffer_size,
            settings.particle_sort(),
            settings.particle_sort_period(),
        );
        // Volume-neutral splat radius (see SPLAT_RADIUS_PER_SPACING); recomputed on
        // every reset so a density/count/fill_level change updates it.
        let particle_radius = scene.seeded_spacing(settings) * SPLAT_RADIUS_PER_SPACING;
        let (tank_lo, tank_hi) = fluid.tank_bounds();
        let [grid_nx, grid_ny, grid_nz] = fluid.grid_dims();
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
        self.fluid = fluid;
        self.particles = particles;
        self.slice = slice;
        self.diffuse = diffuse;
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
        self.requested_particles = requested_particles;
        self.estimated_particles = estimated;
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
        let memory = self.memory_stats();
        LATEST_MEMORY_STATS.with(|stats| stats.set(memory));
        memory.sim_buffers_bytes
    }

    fn memory_stats(&self) -> GpuMemoryStats {
        let sim_buffers_bytes = self.fluid.buffer_memory_bytes();
        let render_targets_bytes = self.render_target_memory_bytes();
        let diffuse_bytes = self.diffuse.memory_bytes();
        let timing_bytes = self
            .timers
            .as_ref()
            .map(timing::GpuTimers::buffer_memory_bytes)
            .unwrap_or(0);
        GpuMemoryStats {
            sim_buffers_bytes,
            render_targets_bytes,
            diffuse_bytes,
            timing_bytes,
            total_tracked_bytes: sim_buffers_bytes
                + render_targets_bytes
                + diffuse_bytes
                + timing_bytes,
        }
    }

    fn render_target_memory_bytes(&self) -> u64 {
        let pixels = (self.config.width as u64) * (self.config.height as u64);
        let depth32 = 4_u64;
        let r16_targets = 6_u64 * 2;
        let scene_color = 8_u64;
        pixels * (depth32 + r16_targets + scene_color)
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

    pub fn device_status(&self) -> GpuDeviceStatus {
        GpuDeviceStatus::from_u8(self.device_status.load(Ordering::Relaxed))
    }

    pub fn device_status_str(&self) -> &'static str {
        self.device_status().as_str()
    }

    pub fn device_status_is_fatal(&self) -> bool {
        self.device_status().fatal()
    }

    fn set_device_status(&self, status: GpuDeviceStatus) {
        if self.device_status().fatal() && !status.fatal() {
            return;
        }
        self.device_status.store(status as u8, Ordering::Relaxed);
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
        self.nearest_z_view = create_r16_target(&self.device, width, height, "water nearest z");
        self.smooth_z_ping_view =
            create_r16_target(&self.device, width, height, "water smooth z ping");
        self.smooth_z_view = create_r16_target(&self.device, width, height, "water smooth z");
        self.scene_color_view = create_scene_color_target(&self.device, width, height);
        self.scene_depth_view = create_r16_target(&self.device, width, height, "hero scene depth");
        self.composite.set_views(
            &self.device,
            &self.thickness_view,
            &self.whitewater_view,
            &self.smooth_z_view,
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

    pub fn set_pressure_warm_start(&mut self, enabled: bool) {
        self.fluid.set_pressure_warm_start(&self.queue, enabled);
    }

    pub fn set_pressure_residual_tolerance(&mut self, tol: f32) {
        self.fluid.set_pressure_residual_tolerance(&self.queue, tol);
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
        self.smoothing
            .update_radius(&self.queue, hero.smooth_radius, hero.feature_preservation);
        self.thickness_smoothing
            .update_radius(&self.queue, hero.smooth_radius);
        self.whitewater_smoothing
            .update_radius(&self.queue, hero.smooth_radius);
        self.particles.splat_scale = hero.smooth_thickness_splat_scale;
    }

    /// Mirror the Water-tab foam settings into the diffuse
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

    pub fn set_frame_substeps(&self, substeps: u32) {
        if let Some(t) = &self.timers {
            t.set_frame_substeps(substeps);
        }
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
        // If the spatial sort flipped the live ping-pong side this frame, rebind the
        // particle renderer to the new current buffer so it draws the live state.
        if self.fluid.take_sort_swapped() {
            self.particles
                .set_particle_buffer(&self.device, self.fluid.particle_buffer());
        }
    }

    /// COARSE substep: three monolithic timestamped compute passes
    /// (prep / pressure / finish), one begin/end pair each, owned by substep `i`.
    fn record_substep_coarse(&self, encoder: &mut wgpu::CommandEncoder, i: u32) {
        // Prep is split around the spatial sort so the sort's prefix-sum sub-passes
        // get real pass-boundary memory barriers (a single compute pass does not
        // guarantee a barrier between every dispatch — running the sort in one pass
        // produced a nondeterministic permutation). clear+mark run first (untimed in
        // coarse mode), then the sort in its own passes (cadence-gated), then the
        // bulk of prep (classify..boundary) carries the coarse `prep` timestamp.
        {
            let mut p = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("sim.prep_pre_sort"),
                timestamp_writes: None,
            });
            self.fluid.record_prep_pre_sort(&mut p);
        }
        if self.fluid.advance_sort_tick() {
            self.fluid.record_sort(encoder, None);
        }
        {
            let mut p = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("sim.prep"),
                timestamp_writes: self.timers.as_ref().map(|t| t.prep_writes(i)),
            });
            self.fluid.record_prep_post_sort(&mut p);
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

        // 0..16 = prep sections (clear..bound_pre_w).
        sec!(0, "d.clear", |p: &mut wgpu::ComputePass| f
            .dispatch_clear(p));
        sec!(1, "d.mark", |p: &mut wgpu::ComputePass| f.dispatch_mark(p));
        // Spatial sort (cadence-gated): the cadence tick advances every substep so
        // coarse/detailed stay in lockstep. On a sort substep the prefix-sum scan
        // runs as three separate (untimed, per-cell, negligible) compute passes for
        // correct memory barriers, then the per-particle `sort_scatter` pass carries
        // the `d.sort` timestamp (section 2). On non-sort substeps the section is an
        // empty timed pass (zero span) — the timestamp slot must still be written
        // every substep so the detailed readback stays aligned.
        if f.advance_sort_tick() {
            // scan_block/spine/add run untimed inside record_sort; the d.sort
            // timestamp wraps the dominant per-particle sort_scatter pass.
            f.record_sort(encoder, Some(timers.fine_section_writes(i, 2)));
        } else {
            let _empty = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("d.sort"),
                timestamp_writes: Some(timers.fine_section_writes(i, 2)),
            });
        }
        sec!(3, "d.classify", |p: &mut wgpu::ComputePass| f
            .dispatch_classify(p));
        // Fused P2G scatter (all three MAC components) is a single section.
        sec!(4, "d.scatter", |p: &mut wgpu::ComputePass| f
            .dispatch_scatter(p));
        sec!(5, "d.normalize_u", |p: &mut wgpu::ComputePass| f
            .dispatch_normalize(p, 0));
        sec!(6, "d.normalize_v", |p: &mut wgpu::ComputePass| f
            .dispatch_normalize(p, 1));
        sec!(7, "d.normalize_w", |p: &mut wgpu::ComputePass| f
            .dispatch_normalize(p, 2));
        sec!(8, "d.savevel_u", |p: &mut wgpu::ComputePass| f
            .dispatch_savevel(p, 0));
        sec!(9, "d.savevel_v", |p: &mut wgpu::ComputePass| f
            .dispatch_savevel(p, 1));
        sec!(10, "d.savevel_w", |p: &mut wgpu::ComputePass| f
            .dispatch_savevel(p, 2));
        sec!(11, "d.forces_u", |p: &mut wgpu::ComputePass| f
            .dispatch_forces(p, 0));
        sec!(12, "d.forces_v", |p: &mut wgpu::ComputePass| f
            .dispatch_forces(p, 1));
        sec!(13, "d.forces_w", |p: &mut wgpu::ComputePass| f
            .dispatch_forces(p, 2));
        sec!(14, "d.bound_pre_u", |p: &mut wgpu::ComputePass| f
            .dispatch_enforce(p, 0));
        sec!(15, "d.bound_pre_v", |p: &mut wgpu::ComputePass| f
            .dispatch_enforce(p, 1));
        sec!(16, "d.bound_pre_w", |p: &mut wgpu::ComputePass| f
            .dispatch_enforce(p, 2));

        // 17,18 = pressure-prelude sections; the CG iterations follow.
        if self.pressure_enabled {
            sec!(17, "d.divergence", |p: &mut wgpu::ComputePass| f
                .dispatch_divergence(p));
            sec!(18, "d.cg_init", |p: &mut wgpu::ComputePass| f
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

        // 19..26 = finish sections (gradient_*, bound_post_*, g2p).
        if self.pressure_enabled {
            sec!(19, "d.gradient_u", |p: &mut wgpu::ComputePass| f
                .dispatch_gradient(p, 0));
            sec!(20, "d.gradient_v", |p: &mut wgpu::ComputePass| f
                .dispatch_gradient(p, 1));
            sec!(21, "d.gradient_w", |p: &mut wgpu::ComputePass| f
                .dispatch_gradient(p, 2));
            sec!(22, "d.bound_post_u", |p: &mut wgpu::ComputePass| f
                .dispatch_enforce(p, 0));
            sec!(23, "d.bound_post_v", |p: &mut wgpu::ComputePass| f
                .dispatch_enforce(p, 1));
            sec!(24, "d.bound_post_w", |p: &mut wgpu::ComputePass| f
                .dispatch_enforce(p, 2));
        }
        sec!(25, "d.g2p", |p: &mut wgpu::ComputePass| f.dispatch_g2p(p));
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
                self.set_device_status(GpuDeviceStatus::SurfaceLost);
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
                self.composite.set_views(
                    &self.device,
                    &self.thickness_view,
                    &self.whitewater_view,
                    &self.smooth_z_view,
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
                return Ok(());
            }
            Cur::Timeout | Cur::Occluded => return Ok(()),
            Cur::Validation => {
                self.set_device_status(GpuDeviceStatus::ValidationError);
                return Err("surface validation error".to_string());
            }
        };
        if self.device_status() == GpuDeviceStatus::SurfaceLost {
            self.set_device_status(GpuDeviceStatus::Ok);
        }
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
        self.skybox
            .set_camera(&self.queue, eye_to_world, self.aspect());
        self.particles
            .update_camera(&self.queue, view_proj, cam_right, cam_up);
        self.diffuse
            .update_camera(&self.queue, view_proj, cam_right, cam_up);
        self.slice.update_camera(&self.queue, view_proj);

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
                // v1.22 Thickness smoothing: a plain separable Gaussian over the
                // thickness target, run AFTER particle thickness writes
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
                // Persistent surface foam over the composite, depth-tested
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
