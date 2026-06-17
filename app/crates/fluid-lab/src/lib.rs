//! fluid-lab — browser-native Rust/WASM/WebGPU 3D fluid lab.
//!
//! Runtime scope: app shell, WebGPU boot/smoke test, GPU FLIP/PIC simulation,
//! rendering, profiler, and the typed config registry.
//!
//! Frame-loop ownership: the web shell owns `requestAnimationFrame` and calls
//! into the single Rust entry point [`FluidApp::frame`]. Rust owns all app state
//! and scheduling. JavaScript never drives simulation frames independently.
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

/// Decode a packed 0x00RRGGBB u32 to linear [0,1] RGB floats.
#[cfg(target_arch = "wasm32")]
fn unpack_rgb(c: u32) -> [f32; 3] {
    [
        ((c >> 16) & 0xFF) as f32 / 255.0,
        ((c >> 8) & 0xFF) as f32 / 255.0,
        (c & 0xFF) as f32 / 255.0,
    ]
}

const INTERACTION_SEED: u64 = 0x464c_5549_445f_5631;
const INTERACTION_MAX_DT_S: f32 = 1.0 / 30.0;

#[derive(Clone, Copy, Debug)]
struct InteractionRng {
    state: u64,
}

impl InteractionRng {
    fn new(seed: u64) -> Self {
        let state = if seed == 0 {
            0x9e37_79b9_7f4a_7c15
        } else {
            seed
        };
        Self { state }
    }

    fn next_u32(&mut self) -> u32 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.state >> 32) as u32
    }

    fn next_f32(&mut self) -> f32 {
        const SCALE: f32 = 1.0 / ((u32::MAX as f32) + 1.0);
        (self.next_u32() as f32) * SCALE
    }

    fn signed_f32(&mut self) -> f32 {
        self.next_f32() * 2.0 - 1.0
    }
}

#[derive(Clone, Copy, Debug)]
struct AutoRollSchedule {
    enabled_last: bool,
    elapsed_s: f32,
    duration_s: f32,
    from: glam::Quat,
    target: glam::Quat,
}

impl Default for AutoRollSchedule {
    fn default() -> Self {
        Self {
            enabled_last: false,
            elapsed_s: 0.0,
            duration_s: 0.0,
            from: glam::Quat::IDENTITY,
            target: glam::Quat::IDENTITY,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct WaveSchedule {
    enabled_last: bool,
    time_until_next_s: f32,
    phase: u32,
}

impl Default for WaveSchedule {
    fn default() -> Self {
        Self {
            enabled_last: false,
            time_until_next_s: 0.0,
            phase: 0,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct InteractionState {
    rng: InteractionRng,
    auto_roll: AutoRollSchedule,
    wave: WaveSchedule,
}

impl InteractionState {
    fn new(seed: u64) -> Self {
        Self {
            rng: InteractionRng::new(seed),
            auto_roll: AutoRollSchedule::default(),
            wave: WaveSchedule::default(),
        }
    }

    fn reset(&mut self, seed: u64) {
        *self = Self::new(seed);
    }

    fn update_auto_roll(
        &mut self,
        dt_s: f32,
        current: glam::Quat,
        enabled: bool,
        strength: f32,
        cadence_s: f32,
    ) -> Option<glam::Quat> {
        let strength = strength.max(0.0);
        if !enabled || strength <= f32::EPSILON {
            self.auto_roll.enabled_last = false;
            return None;
        }

        let dt_s = finite_nonnegative(dt_s);
        if !self.auto_roll.enabled_last || self.auto_roll.duration_s <= f32::EPSILON {
            self.schedule_auto_roll(current, strength, cadence_s);
            self.auto_roll.enabled_last = true;
        }

        self.auto_roll.elapsed_s += dt_s;
        while self.auto_roll.duration_s > f32::EPSILON
            && self.auto_roll.elapsed_s >= self.auto_roll.duration_s
        {
            let overflow = self.auto_roll.elapsed_s - self.auto_roll.duration_s;
            let start = self.auto_roll.target;
            self.schedule_auto_roll(start, strength, cadence_s);
            self.auto_roll.enabled_last = true;
            self.auto_roll.elapsed_s = overflow.min(self.auto_roll.duration_s);
        }

        let t = (self.auto_roll.elapsed_s / self.auto_roll.duration_s).clamp(0.0, 1.0);
        let eased = t * t * (3.0 - 2.0 * t);
        Some(clamp_rotation(
            self.auto_roll
                .from
                .slerp(self.auto_roll.target, eased)
                .normalize(),
            strength,
        ))
    }

    fn schedule_auto_roll(&mut self, from: glam::Quat, strength: f32, cadence_s: f32) {
        let cadence_s = cadence_s.max(0.1);
        let jitter = 0.75 + self.rng.next_f32() * 0.5;
        let horizontal = glam::Vec3::new(self.rng.signed_f32(), 0.0, self.rng.signed_f32());
        let axis = if horizontal.length_squared() > 1.0e-6 {
            horizontal.normalize()
        } else {
            glam::Vec3::X
        };
        let angle = strength * (0.35 + self.rng.next_f32() * 0.65);

        self.auto_roll.from = clamp_rotation(from.normalize(), strength);
        self.auto_roll.target = glam::Quat::from_axis_angle(axis, angle).normalize();
        self.auto_roll.duration_s = cadence_s * jitter;
        self.auto_roll.elapsed_s = 0.0;
    }

    fn update_wave(
        &mut self,
        dt_s: f32,
        enabled: bool,
        strength: f32,
        frequency_hz: f32,
    ) -> Option<[f32; 3]> {
        let strength = strength.max(0.0);
        let frequency_hz = frequency_hz.max(0.0);
        if !enabled || strength <= f32::EPSILON || frequency_hz <= f32::EPSILON {
            self.wave.enabled_last = false;
            return None;
        }

        if !self.wave.enabled_last {
            self.wave.enabled_last = true;
            self.wave.time_until_next_s = 0.0;
        }

        self.wave.time_until_next_s -= finite_nonnegative(dt_s);
        if self.wave.time_until_next_s > 0.0 {
            return None;
        }

        let dir = wave_direction(self.wave.phase);
        self.wave.phase = self.wave.phase.wrapping_add(1);
        self.wave.time_until_next_s += 1.0 / frequency_hz;
        if self.wave.time_until_next_s < 0.0 {
            self.wave.time_until_next_s = 1.0 / frequency_hz;
        }

        Some([dir.x * strength, 0.0, dir.z * strength])
    }
}

fn finite_nonnegative(v: f32) -> f32 {
    if v.is_finite() {
        v.max(0.0)
    } else {
        0.0
    }
}

fn clamp_rotation(q: glam::Quat, max_angle: f32) -> glam::Quat {
    let max_angle = max_angle.max(0.0);
    let (axis, angle) = q.to_axis_angle();
    if angle > max_angle && axis.length_squared() > 0.0 {
        glam::Quat::from_axis_angle(axis, max_angle).normalize()
    } else {
        q.normalize()
    }
}

fn wave_direction(phase: u32) -> glam::Vec3 {
    match phase % 4 {
        0 => glam::Vec3::X,
        1 => -glam::Vec3::X,
        2 => glam::Vec3::Z,
        _ => -glam::Vec3::Z,
    }
}

/// App-level run state driven by the pause/reset/step controls.
#[cfg(target_arch = "wasm32")]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum RunState {
    Running,
    Paused,
}

/// The single WASM-exported application object. The web shell constructs one of
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
    interactions: InteractionState,
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

        let timestep =
            timestep::TimestepController::new(settings.fixed_dt(), settings.max_substeps());

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
            interactions: InteractionState::new(INTERACTION_SEED),
        };
        // Apply settings-defined initial camera orientation.
        app.camera.set_pitch(app.settings.camera_rot_x());
        app.camera.set_yaw(app.settings.camera_rot_y());
        app.camera.set_roll(app.settings.camera_rot_z());
        app.camera.set_distance(app.settings.camera_distance());
        app.push_gravity();
        Ok(app)
    }

    /// Single frame entry point. `render_dt_ms` is the browser rAF delta in
    /// milliseconds. The browser value is sanitized at the bridge before the fixed
    /// timestep controller consumes it.
    #[wasm_bindgen]
    pub fn frame(&mut self, render_dt_ms: f64) {
        if self.gpu.device_status_is_fatal() {
            return;
        }

        let render_dt_ms = finite_or_zero_f64(render_dt_ms).max(0.0);
        let render_dt_s = (render_dt_ms as f32) / 1000.0;
        self.profiler.begin_frame(render_dt_ms);

        // --- update (logical) ---
        self.profiler.scope_begin("update");
        let n_substeps: u32 = match self.run_state {
            RunState::Running => {
                let interaction_dt_s = render_dt_s.clamp(0.0, INTERACTION_MAX_DT_S);
                self.update_interactions(interaction_dt_s);
                let n = self.timestep.steps_for_frame(render_dt_s);
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
                    self.timestep.record_manual_steps(render_dt_s, 1);
                    1
                } else {
                    self.timestep.record_idle_frame(render_dt_s);
                    0
                }
            }
        };
        self.profiler.scope_end("update");
        self.gpu.set_frame_substeps(n_substeps);

        // --- render ---
        self.profiler.scope_begin("render");
        let aspect = self.gpu.aspect();
        let model =
            glam::Mat4::from_translation(self.box_pos) * glam::Mat4::from_quat(self.box_orient);
        let view_proj = self.camera.view_proj(aspect) * model;
        // Billboard basis is camera-facing in WORLD space, but the particle quads are
        // expanded in MODEL space (their positions go through `model` in the shader).
        // Rotate the basis into the box-local frame so the box rotation baked into
        // `model` cancels out and the quads stay camera-facing — otherwise they tilt
        // with the box and go edge-on (thin dark lines) at certain orientations.
        let (right, up) = self.camera.billboard_basis();
        // Camera-only eye->world rotation (world-space basis, NO box orientation):
        // columns are the world directions of the eye-space X/Y/Z axes
        // (right, up, -forward). The hero-water environment reflection + skybox use
        // this so the world background follows the camera but stays fixed when the
        // box rotates (box rotation only changes the gravity source).
        let fwd = self.camera.view_dir();
        let eye_to_world = glam::Mat4::from_cols(
            right.extend(0.0),
            up.extend(0.0),
            (-fwd).extend(0.0),
            glam::Vec4::new(0.0, 0.0, 0.0, 1.0),
        );
        let inv = self.box_orient.inverse();
        let (right, up) = (inv * right, inv * up);
        let eye_world = self.camera.eye();
        if let Err(e) = self.gpu.render(
            &view_proj,
            right,
            up,
            &eye_to_world,
            eye_world,
            self.box_pos,
            self.box_orient,
        ) {
            // Recoverable surface errors (resize/lost); log and continue.
            warn(&format!("[fluid-lab] render error: {e}"));
        }
        self.profiler.scope_end("render");

        // Feed real GPU-timestamp results (throttled readback) to the profiler.
        if let Some(r) = self.gpu.gpu_timing() {
            if r.valid {
                self.profiler.set_gpu_sample(r);
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
            self.gpu.requested_particles(),
            self.gpu.estimated_particles(),
            self.gpu.max_compute_workgroups_per_dimension(),
            self.gpu.max_particle_dispatch_count(),
            self.gpu.particle_dispatch_groups(),
            self.gpu.particle_dispatch_capacity(),
            self.gpu.max_particle_storage_count(),
            self.gpu.scale_status(),
        );
        self.profiler
            .end_frame_and_maybe_log(&self.config_snapshot());
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
    pub fn reset(&mut self) -> bool {
        let scene = scene::SceneConfig::from_settings(&self.settings);
        if let Err(e) = self.gpu.recreate_fluid(&self.settings, &scene) {
            warn(&format!("[fluid-lab][scale] reset rejected: {e}"));
            return false;
        }

        self.tick = 0;
        self.pending_steps = 0;
        self.reset_count += 1;
        // Rebuild scene + fluid + timestep from the current registry so Reset-class
        // settings (grid resolution, particle count, fixed dt, max substeps, density)
        // take effect on Reset — the recreate-fluid-from-settings path.
        self.profiler.reset_measurement();
        self.timestep = timestep::TimestepController::new(
            self.settings.fixed_dt(),
            self.settings.max_substeps(),
        );
        self.scene = scene;
        // Restore the camera orientation from settings (so the camera sliders define the
        // default view), and the box transform to identity.
        self.camera = camera::OrbitCamera::new();
        self.camera.set_pitch(self.settings.camera_rot_x());
        self.camera.set_yaw(self.settings.camera_rot_y());
        self.camera.set_roll(self.settings.camera_rot_z());
        self.camera.set_distance(self.settings.camera_distance());
        self.box_orient = glam::Quat::IDENTITY;
        self.box_pos = glam::Vec3::ZERO;
        self.interactions
            .reset(interaction_seed_for_reset(self.reset_count));
        self.push_gravity();
        log(&format!(
            "[fluid-lab] reset (count={}) — rebuilt from settings (grid={}x{}x{}, particles={})",
            self.reset_count,
            self.settings.grid_res_x(),
            self.settings.grid_res_y(),
            self.settings.grid_res_z(),
            self.gpu.particle_count(),
        ));
        true
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

    /// Set the slice inspection mode: 0=cell-type, 1=pressure, 2=speed.
    #[wasm_bindgen]
    pub fn set_slice_mode(&mut self, m: u32) {
        self.gpu.set_slice_mode(m);
        log(&format!("[fluid-lab] slice_mode = {m}"));
    }

    /// Live PIC↔FLIP blend (0 = damped PIC, 1 = lively FLIP).
    #[wasm_bindgen]
    pub fn set_flip_blend(&mut self, blend: f32) {
        if !blend.is_finite() {
            warn("[fluid-lab] ignored non-finite flip_blend");
            return;
        }
        self.gpu.set_flip_blend(blend);
        log(&format!("[fluid-lab] flip_blend = {blend}"));
    }

    // --- camera (called from TypeScript pointer handlers) ---

    #[wasm_bindgen]
    pub fn camera_orbit(&mut self, dx: f32, dy: f32) {
        self.camera.orbit(dx, dy);
    }

    #[wasm_bindgen]
    pub fn camera_twist(&mut self, dx: f32, dy: f32) {
        self.camera.twist(dx, dy);
    }

    #[wasm_bindgen]
    pub fn camera_pan(&mut self, dx: f32, dy: f32) {
        self.camera.pan(dx, dy);
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

    /// Legacy boolean setting bridge. True still means "accepted and live-class";
    /// callers that need honest mutation details should use set_setting_result_json.
    #[wasm_bindgen]
    pub fn set_setting(&mut self, id: &str, value: f64) -> bool {
        let result = self.mutate_setting(id, value);
        result.accepted() && result.apply == Some(settings::ApplyClass::Live)
    }

    /// Set a setting by id from a JS number and return an honest JSON result.
    #[wasm_bindgen]
    pub fn set_setting_result_json(&mut self, id: &str, value: f64) -> String {
        self.mutate_setting(id, value).to_json()
    }

    fn mutate_setting(&mut self, id: &str, value: f64) -> settings::SettingMutationResult {
        let mut result = self.settings.set_value_f64_result(id, value);
        match result.status {
            settings::MutationStatus::UnknownId => return result,
            settings::MutationStatus::NonFiniteRejected => {
                warn(&format!("[fluid-lab] ignored non-finite setting {id}"));
                return result;
            }
            settings::MutationStatus::LegacyIgnored => {
                if id == "render.particle_alpha" {
                    log("[fluid-lab] ignored legacy render.particle_alpha; use render.water_optical_density");
                } else {
                    log(&format!("[fluid-lab] ignored legacy setting {id}"));
                }
                return result;
            }
            settings::MutationStatus::Applied | settings::MutationStatus::LegacyMapped => {}
            settings::MutationStatus::Stored => return result,
        }

        let canonical_id = result.id;
        let stored_value = result.stored_value.unwrap_or(value);
        result.applied_live = self.apply_live_setting(canonical_id, stored_value);
        result
    }

    fn apply_live_setting(&mut self, id: &str, value: f64) -> bool {
        // Hero-water (Water tab) settings are all Live: rebuild the single
        // HeroParams snapshot and push it to the composite + environment, instead
        // of plumbing each id individually.
        if id.starts_with("render.hero.") {
            self.gpu.set_hero_params(self.settings.hero_params());
            log(&format!("[fluid-lab] live {id} = {value}"));
            return true;
        }
        if self.settings.apply_class_str(id) != Some("live") {
            return false;
        }
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
                // Anti-clump target tracks actual particle density: 0 (Auto) resolves
                // to the scene's effective particles-per-cell so density is
                // motion-neutral; a nonzero value pins a manual target.
                let eff = gpu::effective_rest_density_for_count(
                    &self.settings,
                    &self.scene,
                    self.gpu.particle_count(),
                );
                self.gpu.set_rest_density(eff);
                log(&format!(
                    "[fluid-lab] live rest_density = {value} (effective {eff})"
                ));
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
            "classify.liquid_threshold" => {
                self.gpu.set_liquid_threshold(value as u32);
                log(&format!("[fluid-lab] live liquid_threshold = {value}"));
            }
            "classify.surface_dilation" => {
                let eff = gpu::effective_surface_dilation_for_count(
                    &self.settings,
                    &self.scene,
                    self.gpu.particle_count(),
                );
                self.gpu.set_surface_dilation(eff);
                log(&format!(
                    "[fluid-lab] live surface_dilation = {value} (effective {eff})"
                ));
            }
            "solver.pressure_iterations" => {
                self.gpu.set_pressure_iters(value as u32);
                log(&format!("[fluid-lab] live pressure_iterations = {value}"));
            }
            "solver.pressure_warm_start" => {
                let enabled = value >= 0.5;
                self.gpu.set_pressure_warm_start(enabled);
                log(&format!("[fluid-lab] live pressure_warm_start = {enabled}"));
            }
            "solver.pressure_residual_tolerance" => {
                self.gpu.set_pressure_residual_tolerance(value as f32);
                log(&format!(
                    "[fluid-lab] live pressure_residual_tolerance = {value}"
                ));
            }
            "render.particle_size" => {
                self.gpu.set_particle_size(value as f32);
                log(&format!("[fluid-lab] live particle_size = {value}"));
            }
            "render.speed_scale" => {
                self.gpu.set_speed_scale(value as f32);
                log(&format!("[fluid-lab] live speed_scale = {value}"));
            }
            "render.particle_view" => {
                let view = self.settings.particle_view();
                self.gpu.set_particle_view(view);
                log(&format!("[fluid-lab] live particle_view = {view}"));
            }
            "render.particle_slow_color" => {
                self.gpu.set_particle_slow_color(unpack_rgb(value as u32));
            }
            "render.particle_fast_color" => {
                self.gpu.set_particle_fast_color(unpack_rgb(value as u32));
            }
            "render.water_optical_density" => {
                self.gpu.set_water_optical_density(value as f32);
                log(&format!("[fluid-lab] live water_optical_density = {value}"));
            }
            "render.particle_edge" => {
                self.gpu.set_particle_edge(value as f32);
            }
            "render.particle_shading" => {
                self.gpu.set_particle_shading(value as f32);
            }
            "render.whitewater_strength" => {
                self.gpu.set_whitewater_strength(value as f32);
            }
            "render.whitewater_threshold" => {
                self.gpu.set_whitewater_threshold(value as f32);
            }
            "render.whitewater_softness" => {
                self.gpu.set_whitewater_softness(value as f32);
            }
            "render.fps_target" => {
                // FPS target is consumed by the JS rAF loop; no GPU dispatch needed.
                log(&format!("[fluid-lab] live fps_target = {value}"));
            }
            "interaction.auto_roll_enabled" => {
                log(&format!(
                    "[fluid-lab] live auto_roll_enabled = {}",
                    value as u32 != 0
                ));
            }
            "interaction.auto_roll_strength" => {
                log(&format!("[fluid-lab] live auto_roll_strength = {value}"));
            }
            "interaction.auto_roll_cadence" => {
                log(&format!("[fluid-lab] live auto_roll_cadence = {value}"));
            }
            "interaction.wave_enabled" => {
                log(&format!(
                    "[fluid-lab] live wave_enabled = {}",
                    value as u32 != 0
                ));
            }
            "interaction.wave_strength" => {
                log(&format!("[fluid-lab] live wave_strength = {value}"));
            }
            "interaction.wave_frequency" => {
                log(&format!("[fluid-lab] live wave_frequency = {value}"));
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
            _ => {}
        }
        true
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
            self.settings.pressure_iterations(),
            if self.pressure_enabled {
                "particles+pressure"
            } else {
                "particles+NOpressure"
            },
        )
    }

    #[wasm_bindgen]
    pub fn gpu_device_status(&self) -> String {
        self.gpu.device_status_str().to_string()
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

    fn update_interactions(&mut self, dt_s: f32) {
        let next_orient = self.interactions.update_auto_roll(
            dt_s,
            self.box_orient,
            self.settings.auto_roll_enabled(),
            self.settings.auto_roll_strength(),
            self.settings.auto_roll_cadence(),
        );
        if let Some(next_orient) = next_orient {
            if self.box_orient.dot(next_orient).abs() < 0.999_999 {
                self.box_orient = next_orient;
                self.push_gravity();
            }
        }

        if let Some(impulse) = self.interactions.update_wave(
            dt_s,
            self.settings.wave_enabled(),
            self.settings.wave_strength(),
            self.settings.wave_frequency(),
        ) {
            self.gpu.apply_impulse(impulse);
        }
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
        self.box_pos = self
            .box_pos
            .clamp(glam::Vec3::splat(-3.0), glam::Vec3::splat(3.0));
    }

    /// Slosh the tank: moves the tank in the screen plane but gives the water an
    /// opposite impulse so it lags behind (inertia / sloshing effect).
    #[wasm_bindgen]
    pub fn slosh_box(&mut self, dx: f32, dy: f32) {
        let (right, up) = self.camera.billboard_basis();
        let scale = 0.004;
        let d_world = right * (dx * scale) - up * (dy * scale);
        self.box_pos =
            (self.box_pos + d_world).clamp(glam::Vec3::splat(-3.0), glam::Vec3::splat(3.0));
        // Water lags: local-frame impulse opposite to the box's motion.
        let gain = 35.0;
        let imp = self.box_orient.inverse() * (-d_world * gain);
        self.gpu.apply_impulse([imp.x, imp.y, imp.z]);
    }
}

fn interaction_seed_for_reset(reset_count: u32) -> u64 {
    INTERACTION_SEED ^ ((reset_count as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15))
}

fn finite_or_zero_f64(v: f64) -> f64 {
    if v.is_finite() {
        v
    } else {
        0.0
    }
}

#[cfg(test)]
mod interaction_tests {
    use super::*;

    #[test]
    fn interaction_rng_is_repeatable() {
        let mut a = InteractionRng::new(123);
        let mut b = InteractionRng::new(123);

        for _ in 0..16 {
            assert_eq!(a.next_u32(), b.next_u32());
        }
    }

    #[test]
    fn auto_roll_stays_within_strength_bound() {
        let mut state = InteractionState::new(42);
        let mut q = glam::Quat::IDENTITY;
        let strength = 0.4;

        for _ in 0..64 {
            q = state
                .update_auto_roll(0.1, q, true, strength, 1.0)
                .expect("enabled auto-roll should produce a pose");
            assert!(glam::Quat::IDENTITY.angle_between(q) <= strength + 1.0e-5);
        }
    }

    #[test]
    fn wave_scheduler_fires_immediate_then_by_frequency() {
        let mut state = InteractionState::new(7);

        assert_eq!(
            state.update_wave(0.0, true, 0.5, 2.0),
            Some([0.5, 0.0, 0.0])
        );
        assert_eq!(state.update_wave(0.1, true, 0.5, 2.0), None);
        assert_eq!(
            state.update_wave(0.4, true, 0.5, 2.0),
            Some([-0.5, 0.0, -0.0])
        );

        state.reset(7);
        assert_eq!(state.update_wave(0.0, false, 0.5, 2.0), None);
    }

    #[test]
    fn bridge_dt_sanitizer_replaces_non_finite_values() {
        assert_eq!(finite_or_zero_f64(f64::NAN), 0.0);
        assert_eq!(finite_or_zero_f64(f64::INFINITY), 0.0);
        assert_eq!(finite_or_zero_f64(-12.5), -12.5);
    }
}

#[cfg(test)]
mod shader_contract_tests {
    #[test]
    fn cg_reduce_uses_branch_guard_before_tail_loads() {
        let src = include_str!("gpu/shaders/cg_reduce.wgsl");

        assert!(src.contains("if (is_active && idx < cells)"));
        assert!(!src.contains("select(0.0, vecA[idx] * vecB[idx]"));
    }

    #[test]
    fn wgsl_shaders_do_not_use_reserved_active_local() {
        let shaders = [
            include_str!("gpu/shaders/cg_reduce.wgsl"),
            include_str!("gpu/shaders/cg_reduce_final.wgsl"),
        ];

        for src in shaders {
            assert!(!src.contains("let active"));
        }
    }

    #[test]
    fn cg_scalar_division_guards_do_not_use_select() {
        let alpha = include_str!("gpu/shaders/cg_alpha.wgsl");
        let beta = include_str!("gpu/shaders/cg_beta.wgsl");

        assert!(alpha.contains("if (abs(dq) > 1e-30)"));
        assert!(beta.contains("if (rs_old > 1e-30)"));
        assert!(!alpha.contains("select("));
        assert!(!beta.contains("select("));
    }

    #[test]
    fn cg_scalar_layout_includes_residual_gating_slots() {
        let init = include_str!("gpu/shaders/cg_init.wgsl");
        let set_rsold = include_str!("gpu/shaders/cg_set_rsold.wgsl");

        let layout = "0 rs_old, 1 dot_scratch, 2 alpha, 3 beta, 4 rs_initial, 5 active, 6 tol_sq";
        assert!(init.contains(layout));
        assert!(set_rsold.contains(layout));
        assert!(set_rsold.contains("cg_scalars[4] = cg_scalars[1]"));
        assert!(set_rsold.contains("cg_scalars[5] = 1.0"));
    }

    #[test]
    fn cg_init_supports_default_off_pressure_warm_start() {
        let init = include_str!("gpu/shaders/cg_init.wgsl");

        assert!(init.contains("let warm_start = params.dims.w != 0u"));
        assert!(init.contains("p_initial = pressure_a[c]"));
        assert!(init.contains("ap = apply_pressure(c, p_initial)"));
        assert!(init.contains("let rc = rhs - ap"));
        assert!(init.contains("pressure_a[c] = 0.0"));
        assert!(!init.contains("pressure_a[c] = p_initial"));
    }

    #[test]
    fn cg_reductions_do_not_return_before_workgroup_barriers() {
        let reduce = include_str!("gpu/shaders/cg_reduce.wgsl");
        let reduce_final = include_str!("gpu/shaders/cg_reduce_final.wgsl");

        for src in [reduce, reduce_final] {
            let before_first_barrier = src
                .split("workgroupBarrier()")
                .next()
                .expect("reduction shader should contain a workgroup barrier");
            assert!(!before_first_barrier.contains("return;"));
            assert!(src.contains("let is_active = cg_scalars[5] != 0.0"));
        }
    }

    #[test]
    fn cg_residual_gating_is_default_off_and_shader_gated() {
        let beta = include_str!("gpu/shaders/cg_beta.wgsl");
        let spmv = include_str!("gpu/shaders/cg_spmv.wgsl");
        let reduce = include_str!("gpu/shaders/cg_reduce.wgsl");
        let reduce_final = include_str!("gpu/shaders/cg_reduce_final.wgsl");
        let update = include_str!("gpu/shaders/cg_update.wgsl");
        let dir = include_str!("gpu/shaders/cg_dir.wgsl");

        assert!(beta.contains("if (tol_sq > 0.0 && rs_new <= tol_sq * rs_initial)"));
        assert!(beta.contains("cg_scalars[5] = 0.0"));
        for src in [spmv, update, dir] {
            assert!(src.contains("if (cg_scalars[5] == 0.0)"));
        }
        for src in [reduce, reduce_final] {
            assert!(src.contains("let is_active = cg_scalars[5] != 0.0"));
        }
    }
}

#[cfg(test)]
mod particle_sort_tests {
    //! Host mirror of the GPU counting-sort scan + cursor scatter (the spatial sort
    //! in `gpu/shaders/sort_scan_block|spine|add.wgsl` + `sort_scatter.wgsl`).
    //! Pure-logic, no GPU — runs under `cargo test --lib` on the native target
    //! (the `gpu` module itself is wasm-only). Asserts the three invariants the
    //! determinism argument relies on:
    //!   (a) the permutation is a bijection (every particle written exactly once),
    //!   (b) every particle in a bucket shares the same linear cell key,
    //!   (c) bucket starts equal the EXCLUSIVE prefix sum of the occupancy histogram.

    /// Linear cell key — IDENTICAL math to mark.wgsl / sort_scatter.wgsl:
    /// key = i + nx*(j + ny*k), (i,j,k) = clamp(floor((pos-origin)/h), 0, n-1).
    fn cell_key(pos: [f32; 3], origin: [f32; 3], h: f32, dims: [u32; 3]) -> u32 {
        let g = [
            (pos[0] - origin[0]) / h,
            (pos[1] - origin[1]) / h,
            (pos[2] - origin[2]) / h,
        ];
        let i = (g[0].floor().clamp(0.0, (dims[0] - 1) as f32)) as u32;
        let j = (g[1].floor().clamp(0.0, (dims[1] - 1) as f32)) as u32;
        let k = (g[2].floor().clamp(0.0, (dims[2] - 1) as f32)) as u32;
        i + dims[0] * (j + dims[1] * k)
    }

    /// Exclusive prefix sum (net effect of sort_scan_block → scan_spine → scan_add).
    fn exclusive_scan(hist: &[u32]) -> Vec<u32> {
        let mut out = vec![0u32; hist.len()];
        let mut acc = 0u32;
        for (c, &h) in hist.iter().enumerate() {
            out[c] = acc;
            acc += h;
        }
        out
    }

    /// CPU mirror of the full sort: histogram → exclusive scan → cursor scatter.
    /// Returns (`perm` where sorted[d] == particles[perm[d]], the per-particle keys).
    fn counting_sort(
        positions: &[[f32; 3]],
        origin: [f32; 3],
        h: f32,
        dims: [u32; 3],
    ) -> (Vec<u32>, Vec<u32>) {
        let cells = (dims[0] * dims[1] * dims[2]) as usize;
        let keys: Vec<u32> = positions
            .iter()
            .map(|p| cell_key(*p, origin, h, dims))
            .collect();
        let mut hist = vec![0u32; cells];
        for &k in &keys {
            hist[k as usize] += 1;
        }
        let mut cursor = exclusive_scan(&hist);
        let mut perm = vec![u32::MAX; positions.len()];
        for (p, &k) in keys.iter().enumerate() {
            let dst = cursor[k as usize];
            cursor[k as usize] += 1;
            perm[dst as usize] = p as u32;
        }
        (perm, keys)
    }

    /// Deterministic spread of particles across the domain (LCG, no GPU).
    fn sample_positions(n: usize, origin: [f32; 3], h: f32, dims: [u32; 3]) -> Vec<[f32; 3]> {
        let extent = [dims[0] as f32 * h, dims[1] as f32 * h, dims[2] as f32 * h];
        let mut st = 0x9E37_79B9u32;
        let mut rng = move || {
            st = st.wrapping_mul(1664525).wrapping_add(1013904223);
            (st >> 8) as f32 / (1u32 << 24) as f32
        };
        (0..n)
            .map(|_| {
                [
                    origin[0] + rng() * extent[0],
                    origin[1] + rng() * extent[1],
                    origin[2] + rng() * extent[2],
                ]
            })
            .collect()
    }

    #[test]
    fn exclusive_scan_matches_definition() {
        let hist = [3u32, 0, 5, 1, 0, 4];
        let off = exclusive_scan(&hist);
        assert_eq!(off, [0, 3, 3, 8, 9, 9]);
        assert_eq!(off[5] + hist[5], hist.iter().sum::<u32>());
    }

    #[test]
    fn permutation_is_a_bijection() {
        let dims = [16u32, 8, 16];
        let (h, origin) = (0.1, [-0.8, -0.4, -0.8]);
        let pos = sample_positions(5000, origin, h, dims);
        let (perm, _) = counting_sort(&pos, origin, h, dims);
        assert_eq!(perm.len(), pos.len());
        let mut seen = vec![false; pos.len()];
        for &src in &perm {
            assert!((src as usize) < pos.len(), "dst slot unfilled");
            assert!(!seen[src as usize], "particle {src} placed twice");
            seen[src as usize] = true;
        }
        assert!(seen.iter().all(|&b| b), "some particle never placed");
    }

    #[test]
    fn each_bucket_shares_one_key_and_is_monotone() {
        let dims = [12u32, 6, 10];
        let (h, origin) = (0.1, [-0.6, -0.3, -0.5]);
        let pos = sample_positions(4000, origin, h, dims);
        let (perm, keys) = counting_sort(&pos, origin, h, dims);
        let mut prev = 0u32;
        for &src in &perm {
            let k = keys[src as usize];
            assert!(k >= prev, "sorted keys not monotone: {k} after {prev}");
            prev = k;
        }
        let cells = (dims[0] * dims[1] * dims[2]) as usize;
        let mut hist = vec![0u32; cells];
        for &k in &keys {
            hist[k as usize] += 1;
        }
        let starts = exclusive_scan(&hist);
        for c in 0..cells {
            let s = starts[c] as usize;
            let e = s + hist[c] as usize;
            for d in s..e {
                assert_eq!(
                    keys[perm[d] as usize], c as u32,
                    "bucket {c} slot {d} wrong key"
                );
            }
        }
    }

    #[test]
    fn clamps_out_of_bounds_positions_into_edge_cells() {
        let dims = [4u32, 4, 4];
        let (h, origin) = (1.0, [0.0, 0.0, 0.0]);
        let pos = vec![[-100.0, -100.0, -100.0], [100.0, 100.0, 100.0]];
        let (perm, keys) = counting_sort(&pos, origin, h, dims);
        let cells = dims[0] * dims[1] * dims[2];
        for &k in &keys {
            assert!(k < cells, "key {k} out of range {cells}");
        }
        assert_eq!(keys.iter().copied().min().unwrap(), 0);
        assert_eq!(keys.iter().copied().max().unwrap(), 63);
        assert_eq!(perm.len(), 2);
        assert!(perm.contains(&0) && perm.contains(&1));
    }
}
