//! fluid-lab — browser-native Rust/WASM/WebGPU 3D fluid lab.
//!
//! Phase 0.1 scope: app shell, WebGPU boot, compute/atomic smoke test, wireframe
//! tank + orbit camera, pause/reset/step, hierarchical profiler skeleton, typed
//! config-registry skeleton. No fluid simulation yet.
//!
//! Frame-loop ownership: TypeScript owns `requestAnimationFrame` and calls into
//! the single Rust entry point [`FluidApp::frame`]. Rust owns all app state and
//! scheduling. TS never drives simulation frames independently.
//!
//! NOTE: `#![allow(dead_code)]` is intentional for the 0.1–0.2 skeleton. Many
//! config-registry / scene / profiler fields belong to the forward-looking data
//! model and are not read until later phases. (It also sidesteps a rustc 1.95 ICE
//! in the dead-code-lint diagnostic renderer on multi-line struct field spans.)
#![allow(dead_code)]

// Pure modules — always compiled, unit-tested natively via `cargo test`.
mod camera;
mod scene;
mod settings;
mod sim;
mod timestep;

// wasm-only modules (use wgpu / web-sys); excluded from native test builds.
#[cfg(target_arch = "wasm32")]
mod gpu;
#[cfg(target_arch = "wasm32")]
mod profiler;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

#[cfg(target_arch = "wasm32")]
pub(crate) fn log(s: &str) {
    web_sys::console::log_1(&JsValue::from_str(s));
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn warn(s: &str) {
    web_sys::console::warn_1(&JsValue::from_str(s));
}

/// App-level run state driven by the pause/reset/step controls.
#[cfg(target_arch = "wasm32")]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum RunState {
    Running,
    Paused,
}

/// The single WASM-exported application object. TypeScript constructs one of
/// these per canvas and calls [`FluidApp::frame`] from its rAF loop.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub struct FluidApp {
    gpu: gpu::GpuContext,
    camera: camera::OrbitCamera,
    profiler: profiler::Profiler,
    settings: settings::Registry,
    scene: scene::SceneConfig,
    run_state: RunState,
    pressure_enabled: bool,
    /// Number of single-step requests pending (step button while paused).
    pending_steps: u32,
    /// Monotonic count of logical sim ticks; reset returns it to 0.
    tick: u64,
    reset_count: u32,
    timestep: timestep::TimestepController,
    /// Box orientation quaternion (world-space). Default = identity (upright).
    box_orient: glam::Quat,
    /// Box translation in world space. Default = zero (centered).
    box_pos: glam::Vec3,
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
impl FluidApp {
    /// Async constructor: initializes WebGPU against the given canvas, logs boot
    /// diagnostics, runs the compute/atomic smoke test, and builds the renderer.
    #[wasm_bindgen]
    pub async fn create(canvas: web_sys::HtmlCanvasElement) -> Result<FluidApp, JsValue> {
        console_error_panic_hook::set_once();

        let settings = settings::Registry::default();
        let scene = scene::SceneConfig::from_settings(&settings);

        let gpu = gpu::GpuContext::new(canvas, &settings, &scene)
            .await
            .map_err(|e| JsValue::from_str(&format!("WebGPU init failed: {e}")))?;

        let camera = camera::OrbitCamera::new();

        log("[fluid-lab] FluidApp created — Phase 0.1 shell ready.");

        let timestep = timestep::TimestepController::new(
            settings.fixed_dt(),
            settings.max_substeps(),
        );

        let mut app = FluidApp {
            gpu,
            camera,
            profiler: profiler::Profiler::new(),
            settings,
            scene,
            run_state: RunState::Running,
            pressure_enabled: true,
            pending_steps: 0,
            tick: 0,
            reset_count: 0,
            timestep,
            box_orient: glam::Quat::IDENTITY,
            box_pos: glam::Vec3::ZERO,
        };
        // Apply settings-defined initial camera orientation and sun direction.
        app.camera.set_pitch(app.settings.camera_rot_x());
        app.camera.set_yaw(app.settings.camera_rot_y());
        app.camera.set_roll(app.settings.camera_rot_z());
        app.camera.set_distance(app.settings.camera_distance());
        app.gpu.set_sun_dir(app.settings.sun_x(), app.settings.sun_y(), app.settings.sun_z());
        app.push_gravity();
        Ok(app)
    }

    /// Single frame entry point. `render_dt_ms` is the browser rAF delta in
    /// milliseconds. Simulation stepping (added in later phases) must clamp this;
    /// 0.1 only advances a logical tick counter when not paused.
    #[wasm_bindgen]
    pub fn frame(&mut self, render_dt_ms: f64) {
        self.profiler.begin_frame(render_dt_ms);

        // --- update (logical) ---
        self.profiler.scope_begin("update");
        let n_substeps: u32 = match self.run_state {
            RunState::Running => {
                let n = self.timestep.steps_for_frame((render_dt_ms as f32) / 1000.0);
                if n > 0 {
                    self.profiler.scope_begin("simulation");
                    self.gpu.step(n);
                    self.profiler.scope_end("simulation");
                    self.tick += n as u64;
                }
                n
            }
            RunState::Paused => {
                // Keep the accumulator from building up while paused.
                self.timestep.reset();
                if self.pending_steps > 0 {
                    self.pending_steps -= 1;
                    self.profiler.scope_begin("simulation");
                    self.gpu.step(1);
                    self.profiler.scope_end("simulation");
                    self.tick += 1;
                    1
                } else {
                    0
                }
            }
        };
        self.profiler.scope_end("update");

        // --- render ---
        self.profiler.scope_begin("render");
        let aspect = self.gpu.aspect();
        let model = glam::Mat4::from_translation(self.box_pos)
            * glam::Mat4::from_quat(self.box_orient);
        let view_proj = self.camera.view_proj(aspect) * model;
        // Billboard basis is camera-facing in WORLD space, but the particle quads are
        // expanded in MODEL space (their positions go through `model` in the shader).
        // Rotate the basis into the box-local frame so the box rotation baked into
        // `model` cancels out and the quads stay camera-facing — otherwise they tilt
        // with the box and go edge-on (thin dark lines) at certain orientations.
        let (right, up) = self.camera.billboard_basis();
        let inv = self.box_orient.inverse();
        let (right, up) = (inv * right, inv * up);
        // Camera eye in the tank's local frame (vertices/MC live in local space; the
        // model matrix is baked into view_proj only). The mesh shader needs this for
        // correct view-dependent shading while the tank is moved/rotated.
        let cam_pos_local = inv * (self.camera.eye() - self.box_pos);
        if let Err(e) = self.gpu.render(&view_proj, right, up, cam_pos_local) {
            // Recoverable surface errors (resize/lost); log and continue.
            warn(&format!("[fluid-lab] render error: {e}"));
        }
        self.profiler.scope_end("render");

        // Feed real GPU-timestamp results (throttled readback) to the profiler.
        if let Some(r) = self.gpu.gpu_timing() {
            if r.valid {
                self.profiler.set_gpu_sample(r, n_substeps);
            }
        }

        self.profiler.set_substeps(n_substeps);
        // Timestep accounting (this-frame stats + cumulative dropped sim time).
        self.profiler
            .set_timestep_stats(self.timestep.last_stats(), self.timestep.total_dropped());
        // Structural per-frame facts sourced from the GPU context.
        self.profiler.set_frame_facts(
            self.gpu.total_cells(),
            self.gpu.particle_count(),
            self.gpu.grid_res(),
            self.gpu.buffer_memory_bytes(),
            self.gpu.dispatches_per_substep(),
        );
        self.profiler.end_frame_and_maybe_log(&self.config_snapshot());
    }

    /// Build the active config snapshot string used to tag profiler output.
    fn config_snapshot(&self) -> String {
        format!(
            "grid={gx}x{gy}x{gz} particles={p} dt={dt} press_iters={pi} mode={mode} tick={tick} run={run:?}",
            gx = self.settings.grid_res_x(),
            gy = self.settings.grid_res_y(),
            gz = self.settings.grid_res_z(),
            p = self.gpu.particle_count(),
            dt = self.settings.fixed_dt(),
            pi = self.settings.pressure_iterations(),
            mode = if self.pressure_enabled { "particles+pressure" } else { "particles+NOpressure" },
            tick = self.tick,
            run = self.run_state,
        )
    }

    // --- controls (called from TypeScript) ---

    #[wasm_bindgen]
    pub fn set_paused(&mut self, paused: bool) {
        self.run_state = if paused {
            RunState::Paused
        } else {
            RunState::Running
        };
        log(&format!("[fluid-lab] run_state = {:?}", self.run_state));
    }

    #[wasm_bindgen]
    pub fn is_paused(&self) -> bool {
        self.run_state == RunState::Paused
    }

    #[wasm_bindgen]
    pub fn step(&mut self) {
        // A step only has meaning while paused; queue one logical tick.
        self.pending_steps += 1;
        log("[fluid-lab] step queued");
    }

    #[wasm_bindgen]
    pub fn reset(&mut self) {
        self.tick = 0;
        self.pending_steps = 0;
        self.reset_count += 1;
        // Rebuild scene + fluid + timestep from the current registry so Reset-class
        // settings (grid resolution, particle count, fixed dt, max substeps, density)
        // take effect on Reset — the recreate-fluid-from-settings path.
        self.scene = scene::SceneConfig::from_settings(&self.settings);
        self.timestep = timestep::TimestepController::new(
            self.settings.fixed_dt(),
            self.settings.max_substeps(),
        );
        self.gpu.recreate_fluid(&self.settings, &self.scene);
        // dev.mesh_enabled is Reset-class: materialize it now so the MC resources
        // are (de)allocated to match the registry on Reset.
        self.gpu.set_mesh_enabled(self.settings.mesh_enabled());
        // Restore the camera orientation from settings (so the camera sliders define the
        // default view), and the box transform to identity.
        self.camera = camera::OrbitCamera::new();
        self.camera.set_pitch(self.settings.camera_rot_x());
        self.camera.set_yaw(self.settings.camera_rot_y());
        self.camera.set_roll(self.settings.camera_rot_z());
        self.camera.set_distance(self.settings.camera_distance());
        self.box_orient = glam::Quat::IDENTITY;
        self.box_pos = glam::Vec3::ZERO;
        self.push_gravity();
        log(&format!(
            "[fluid-lab] reset (count={}) — rebuilt from settings (grid={}x{}x{}, particles={})",
            self.reset_count,
            self.settings.grid_res_x(),
            self.settings.grid_res_y(),
            self.settings.grid_res_z(),
            self.gpu.particle_count(),
        ));
    }

    #[wasm_bindgen]
    pub fn reset_count(&self) -> u32 {
        self.reset_count
    }

    /// Toggle pressure projection (for the pressure on/off comparison capture).
    #[wasm_bindgen]
    pub fn set_pressure_enabled(&mut self, enabled: bool) {
        self.pressure_enabled = enabled;
        self.gpu.set_pressure_enabled(enabled);
        log(&format!("[fluid-lab] pressure_enabled = {enabled}"));
    }

    /// Toggle the grid-slice debug view (XY cross-section at k=n/2).
    #[wasm_bindgen]
    pub fn set_slice_enabled(&mut self, on: bool) {
        self.gpu.set_slice_enabled(on);
        log(&format!("[fluid-lab] slice_enabled = {on}"));
    }

    /// Toggle the marching-cubes mesh surface view (replaces particles when on).
    #[wasm_bindgen]
    pub fn set_mesh_enabled(&mut self, on: bool) {
        self.gpu.set_mesh_enabled(on);
        log(&format!("[fluid-lab] mesh_enabled = {on}"));
    }

    /// Live update of the MC isosurface level (particles/cell, default 2.0).
    #[wasm_bindgen]
    pub fn set_mesh_iso(&mut self, v: f32) {
        self.gpu.set_mesh_iso(v);
        log(&format!("[fluid-lab] mesh_iso = {v}"));
    }

    /// Set the slice inspection mode: 0=cell-type, 1=pressure, 2=speed.
    #[wasm_bindgen]
    pub fn set_slice_mode(&mut self, m: u32) {
        self.gpu.set_slice_mode(m);
        log(&format!("[fluid-lab] slice_mode = {m}"));
    }

    /// Live PIC↔FLIP blend (0 = damped PIC, 1 = lively FLIP).
    #[wasm_bindgen]
    pub fn set_flip_blend(&mut self, blend: f32) {
        self.gpu.set_flip_blend(blend);
        log(&format!("[fluid-lab] flip_blend = {blend}"));
    }

    // --- camera (called from TypeScript pointer handlers) ---

    #[wasm_bindgen]
    pub fn camera_orbit(&mut self, dx: f32, dy: f32) {
        self.camera.orbit(dx, dy);
    }

    #[wasm_bindgen]
    pub fn camera_zoom(&mut self, delta: f32) {
        self.camera.zoom(delta);
    }

    // --- JSON bridge (config panel + profiler panel) ---

    /// Return all settings serialized as a JSON array.
    #[wasm_bindgen]
    pub fn config_json(&self) -> String {
        self.settings.config_json()
    }

    /// Set a setting by id from a JS number (f64).
    /// For Live settings pushes the change to the GPU immediately and returns true.
    /// For Reset/Reload settings only the registry value is updated and returns false
    /// (caller should show a "needs reset / needs reload" hint).
    #[wasm_bindgen]
    pub fn set_setting(&mut self, id: &str, value: f64) -> bool {
        if !self.settings.set_value_f64(id, value) {
            return false; // unknown id
        }
        match self.settings.apply_class_str(id) {
            Some("live") => {
                match id {
                    "physics.gravity" => {
                        self.push_gravity();
                        log(&format!("[fluid-lab] live gravity = {value}"));
                    }
                    "physics.flip_blend" => {
                        self.gpu.set_flip_blend(value as f32);
                        log(&format!("[fluid-lab] live flip_blend = {value}"));
                    }
                    "physics.wall_friction" => {
                        self.gpu.set_wall_friction(value as f32);
                        log(&format!("[fluid-lab] live wall_friction = {value}"));
                    }
                    "physics.rest_density" => {
                        self.gpu.set_rest_density(value as f32);
                        log(&format!("[fluid-lab] live rest_density = {value}"));
                    }
                    "physics.volume_stiffness" => {
                        self.gpu.set_volume_stiffness(value as f32);
                        log(&format!("[fluid-lab] live volume_stiffness = {value}"));
                    }
                    "physics.drift_clamp" => {
                        self.gpu.set_drift_clamp(value as f32);
                        log(&format!("[fluid-lab] live drift_clamp = {value}"));
                    }
                    "physics.cfl" => {
                        self.gpu.set_cfl(value as f32);
                        log(&format!("[fluid-lab] live cfl = {value}"));
                    }
                    "render.mesh_iso" => {
                        self.gpu.set_mesh_iso(value as f32);
                        log(&format!("[fluid-lab] live mesh_iso = {value}"));
                    }
                    "render.mesh_smooth" => {
                        self.gpu.set_mesh_smooth(value as u32);
                        log(&format!("[fluid-lab] live mesh_smooth = {value}"));
                    }
                    "render.mesh_opacity" => {
                        self.gpu.set_mesh_opacity(value as f32);
                        log(&format!("[fluid-lab] live mesh_opacity = {value}"));
                    }
                    "render.mesh_fresnel" => {
                        self.gpu.set_mesh_fresnel(value as f32);
                        log(&format!("[fluid-lab] live mesh_fresnel = {value}"));
                    }
                    "render.mesh_foam" => {
                        self.gpu.set_mesh_foam(value as f32);
                        log(&format!("[fluid-lab] live mesh_foam = {value}"));
                    }
                    "classify.liquid_threshold" => {
                        self.gpu.set_liquid_threshold(value as u32);
                        log(&format!("[fluid-lab] live liquid_threshold = {value}"));
                    }
                    "classify.surface_dilation" => {
                        self.gpu.set_surface_dilation(value as u32);
                        log(&format!("[fluid-lab] live surface_dilation = {value}"));
                    }
                    "solver.pressure_iterations" => {
                        self.gpu.set_pressure_iters(value as u32);
                        log(&format!("[fluid-lab] live pressure_iterations = {value}"));
                    }
                    "render.particle_size" => {
                        self.gpu.set_particle_size(value as f32);
                        log(&format!("[fluid-lab] live particle_size = {value}"));
                    }
                    "render.speed_scale" => {
                        self.gpu.set_speed_scale(value as f32);
                        log(&format!("[fluid-lab] live speed_scale = {value}"));
                    }
                    "render.fps_target" => {
                        // FPS target is consumed by the JS rAF loop; no GPU dispatch needed.
                        log(&format!("[fluid-lab] live fps_target = {value}"));
                    }
                    "camera.rot_x" => {
                        self.camera.set_pitch(value as f32);
                        log(&format!("[fluid-lab] live camera.rot_x = {value}"));
                    }
                    "camera.rot_y" => {
                        self.camera.set_yaw(value as f32);
                        log(&format!("[fluid-lab] live camera.rot_y = {value}"));
                    }
                    "camera.rot_z" => {
                        self.camera.set_roll(value as f32);
                        log(&format!("[fluid-lab] live camera.rot_z = {value}"));
                    }
                    "camera.distance" => {
                        self.camera.set_distance(value as f32);
                        log(&format!("[fluid-lab] live camera.distance = {value}"));
                    }
                    "render.sun_x" | "render.sun_y" | "render.sun_z" => {
                        let sx = self.settings.sun_x();
                        let sy = self.settings.sun_y();
                        let sz = self.settings.sun_z();
                        self.gpu.set_sun_dir(sx, sy, sz);
                        log(&format!("[fluid-lab] live sun_dir = ({sx},{sy},{sz})"));
                    }
                    _ => {}
                }
                true
            }
            _ => false,
        }
    }

    /// Return the current FPS target so the JS rAF loop can throttle itself.
    /// 0 = uncapped.
    #[wasm_bindgen]
    pub fn fps_target(&self) -> u32 {
        self.settings.fps_target()
    }

    /// Return live profiler and GPU timing stats as a JSON object.
    #[wasm_bindgen]
    pub fn stats_json(&self) -> String {
        self.profiler.stats_json(
            self.settings.grid_res_x(),
            self.gpu.particle_count(),
        )
    }

    // --- canvas sizing ---

    #[wasm_bindgen]
    pub fn resize(&mut self, width: u32, height: u32) {
        self.gpu.resize(width, height);
    }

    // --- box transform ---

    /// Compute and push the current gravity vector (in the box's local frame) to the GPU.
    fn push_gravity(&mut self) {
        let mag = self.settings.gravity(); // signed scalar (e.g. -9.81)
        let world = glam::Vec3::new(0.0, mag, 0.0);
        let g = self.box_orient.inverse() * world; // local-frame gravity
        self.gpu.set_gravity_vec(g.x, g.y, g.z);
    }

    /// Rotate the tank (and its gravity) by drag deltas (pixels). dx spins about the
    /// camera's up axis, dy tips about the camera's right axis.
    #[wasm_bindgen]
    pub fn rotate_box(&mut self, dx: f32, dy: f32) {
        const SENS: f32 = 0.01;
        let (right, up) = self.camera.billboard_basis();
        let dq = glam::Quat::from_axis_angle(up, dx * SENS)
            * glam::Quat::from_axis_angle(right, dy * SENS);
        self.box_orient = (dq * self.box_orient).normalize();
        self.push_gravity();
    }

    /// Second rotate control: dx ROLLS the tank about the camera's view axis, dy tips
    /// it about the camera's right axis — so together the two rotate modes reach all
    /// three rotation axes.
    #[wasm_bindgen]
    pub fn rotate_box_roll(&mut self, dx: f32, dy: f32) {
        const SENS: f32 = 0.01;
        let (right, _up) = self.camera.billboard_basis();
        let fwd = self.camera.view_dir();
        let dq = glam::Quat::from_axis_angle(fwd, dx * SENS)
            * glam::Quat::from_axis_angle(right, dy * SENS);
        self.box_orient = (dq * self.box_orient).normalize();
        self.push_gravity();
    }

    /// Translate the tank (water follows) in the camera screen plane.
    #[wasm_bindgen]
    pub fn move_box(&mut self, dx: f32, dy: f32) {
        let (right, up) = self.camera.billboard_basis();
        let scale = 0.004;
        self.box_pos += right * (dx * scale) - up * (dy * scale);
        self.box_pos = self.box_pos.clamp(glam::Vec3::splat(-3.0), glam::Vec3::splat(3.0));
    }

    /// Slosh the tank: moves the tank in the screen plane but gives the water an
    /// opposite impulse so it lags behind (inertia / sloshing effect).
    #[wasm_bindgen]
    pub fn slosh_box(&mut self, dx: f32, dy: f32) {
        let (right, up) = self.camera.billboard_basis();
        let scale = 0.004;
        let d_world = right * (dx * scale) - up * (dy * scale);
        self.box_pos = (self.box_pos + d_world).clamp(glam::Vec3::splat(-3.0), glam::Vec3::splat(3.0));
        // Water lags: local-frame impulse opposite to the box's motion.
        let gain = 35.0;
        let imp = self.box_orient.inverse() * (-d_world * gain);
        self.gpu.apply_impulse([imp.x, imp.y, imp.z]);
    }
}

