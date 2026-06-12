//! Typed config-registry.
//!
//! Per `decisions.md` (configuration flows through a schema-driven registry) and
//! the observability split, this file is the authoritative source for each setting's
//! id, label, semantic category, functional tab, type, default, validation, optional
//! help copy, and apply class.

/// When a changed setting can take effect. The colored dot (🟢/🟡/🔴) is a 1.2
/// panel concern; the class itself is part of the data model from the start.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ApplyClass {
    /// Applies immediately during the current run.
    Live,
    /// Requires simulation reset/reinitialization (buffer realloc, scene rebuild).
    Reset,
    /// Requires page/app reload (device/adapter features, threading mode).
    Reload,
}

impl ApplyClass {
    pub fn as_str(self) -> &'static str {
        match self {
            ApplyClass::Live => "live",
            ApplyClass::Reset => "reset",
            ApplyClass::Reload => "reload",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Category {
    Scene,
    Grid,
    Particles,
    Physics,
    Interaction,
    Solver,
    Camera,
    Render,
    Water,
    Diagnostics,
}

impl Category {
    pub fn as_str(self) -> &'static str {
        match self {
            Category::Scene => "Scene",
            Category::Grid => "Grid",
            Category::Particles => "Particles",
            Category::Physics => "Physics",
            Category::Interaction => "Interaction",
            Category::Solver => "Solver",
            Category::Camera => "Camera",
            Category::Render => "Render",
            Category::Water => "Water",
            Category::Diagnostics => "Diagnostics",
        }
    }
}

/// Product-facing settings tab. The web shell groups rows by this registry-owned
/// metadata instead of rebuilding a taxonomy in JavaScript.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SettingsTab {
    Scenario,
    Simulation,
    Modes,
    CameraView,
    WaterSurface,
    WaterColor,
    Environment,
    SunReflection,
    Foam,
}

impl SettingsTab {
    pub fn as_str(self) -> &'static str {
        match self {
            SettingsTab::Scenario => "scenario",
            SettingsTab::Simulation => "simulation",
            SettingsTab::Modes => "modes",
            SettingsTab::CameraView => "camera-view",
            SettingsTab::WaterSurface => "water-surface",
            SettingsTab::WaterColor => "water-color",
            SettingsTab::Environment => "environment",
            SettingsTab::SunReflection => "sun-reflection",
            SettingsTab::Foam => "foam",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            SettingsTab::Scenario => "Scenario",
            SettingsTab::Simulation => "Simulation",
            SettingsTab::Modes => "Modes",
            SettingsTab::CameraView => "Camera & View",
            SettingsTab::WaterSurface => "Water Surface",
            SettingsTab::WaterColor => "Water Color",
            SettingsTab::Environment => "Environment",
            SettingsTab::SunReflection => "Sun & Reflection",
            SettingsTab::Foam => "Foam",
        }
    }

    pub fn order(self) -> u32 {
        match self {
            SettingsTab::Scenario => 10,
            SettingsTab::Simulation => 20,
            SettingsTab::Modes => 30,
            SettingsTab::CameraView => 40,
            SettingsTab::WaterSurface => 50,
            SettingsTab::WaterColor => 60,
            SettingsTab::Environment => 70,
            SettingsTab::SunReflection => 80,
            SettingsTab::Foam => 90,
        }
    }

    pub fn group(self) -> &'static str {
        match self {
            SettingsTab::Scenario | SettingsTab::Simulation | SettingsTab::Modes => "Setup",
            SettingsTab::CameraView
            | SettingsTab::WaterSurface
            | SettingsTab::WaterColor
            | SettingsTab::Environment
            | SettingsTab::SunReflection
            | SettingsTab::Foam => "Core",
        }
    }

    pub fn variant(self) -> &'static str {
        match self {
            _ => "normal",
        }
    }
}

/// A typed setting value. The registry currently exposes numeric sliders and
/// enum/color hints through JSON metadata.
#[derive(Clone, Copy, Debug)]
pub enum Value {
    U32(u32),
    F32(f32),
}

/// Inclusive numeric validation bounds.
#[derive(Clone, Copy, Debug)]
pub enum Validation {
    U32Range { min: u32, max: u32 },
    F32Range { min: f32, max: f32 },
    None,
}

#[derive(Clone)]
pub struct Setting {
    pub id: &'static str,
    pub label: &'static str,
    pub category: Category,
    pub default: Value,
    pub value: Value,
    pub validation: Validation,
    pub tooltip: Option<&'static str>,
    pub technical_tooltip: Option<&'static str>,
    pub apply: ApplyClass,
}

impl Setting {
    fn as_u32(&self) -> u32 {
        match self.value {
            Value::U32(v) => v,
            Value::F32(v) => v as u32,
        }
    }
    fn as_f32(&self) -> f32 {
        match self.value {
            Value::F32(v) => v,
            Value::U32(v) => v as f32,
        }
    }

    /// Current value as f64 (for JSON bridge).
    pub fn value_as_f64(&self) -> f64 {
        match self.value {
            Value::U32(v) => v as f64,
            Value::F32(v) => v as f64,
        }
    }

    /// Default value as f64 (for JSON bridge).
    pub fn default_as_f64(&self) -> f64 {
        match self.default {
            Value::U32(v) => v as f64,
            Value::F32(v) => v as f64,
        }
    }

    /// Type name string: "u32" or "f32".
    pub fn type_str(&self) -> &'static str {
        match self.value {
            Value::U32(_) => "u32",
            Value::F32(_) => "f32",
        }
    }

    /// Validation min as f64 (sensible wide defaults if Validation::None).
    pub fn min_as_f64(&self) -> f64 {
        match self.validation {
            Validation::U32Range { min, .. } => min as f64,
            Validation::F32Range { min, .. } => min as f64,
            Validation::None => match self.value {
                Value::U32(_) => 0.0,
                Value::F32(_) => -1.0e38,
            },
        }
    }

    /// Validation max as f64 (sensible wide defaults if Validation::None).
    pub fn max_as_f64(&self) -> f64 {
        match self.validation {
            Validation::U32Range { max, .. } => max as f64,
            Validation::F32Range { max, .. } => max as f64,
            Validation::None => match self.value {
                Value::U32(_) => u32::MAX as f64,
                Value::F32(_) => 1.0e38,
            },
        }
    }
}

/// A flat snapshot of the Water-tab (hero-water) settings. The renderer mirrors
/// this into the composite's std140 uniform whenever a hero slider changes, so
/// there is no per-setting GPU plumbing. Plain data — no GPU types — keeps the
/// registry renderer-agnostic.
#[derive(Clone, Copy, Debug)]
pub struct HeroParams {
    pub refraction_enabled: bool,
    pub reflection_enabled: bool,
    pub body_color_enabled: bool,
    pub wall_contact_enabled: bool,
    pub debug_view: u32,
    pub ior: f32,
    pub refraction_strength: f32,
    pub refraction_thickness_scale: f32,
    pub refraction_max_offset_px: f32,
    pub invalid_refraction_fallback: u32,
    pub absorption_color: [f32; 3],
    pub absorption_strength: f32,
    pub base_tint: [f32; 3],
    pub transparency: f32,
    pub deep_water_darkening: f32,
    pub floor_pattern_scale: f32,
    pub floor_pattern_strength: f32,
    pub backdrop_strength: f32,
    pub wall_visibility: f32,
    // --- Environment reflection (v1.15) ---
    pub reflection_strength: f32,
    pub environment_strength: f32,
    pub environment_mode: u32,
    pub environment_rotation: f32,
    pub environment_brightness: f32,
    pub skybox_enabled: bool,
    pub roughness_base: f32,
    pub roughness_velocity_scale: f32,
    pub roughness_normal_variance_scale: f32,
    pub roughness_foam_scale: f32,
    pub specular_strength: f32,
    pub sun_direction: [f32; 3],
    pub sun_intensity: f32,
    pub micro_normal_enabled: bool,
    pub micro_normal_strength: f32,
    pub micro_normal_scale: f32,
    pub micro_normal_velocity_scale: f32,
    // --- Screen-space surface quality (v1.19 polish) ---
    /// Number of bilateral smooth X+Y iterations. Default 2.
    pub smooth_iterations: u32,
    /// Bilateral kernel half-width in pixels. Default 5.
    pub smooth_radius: u32,
    /// Scale applied to particle world radius when writing nearest_z splat.
    /// Larger values fatten overlapping splats so they fuse more before smoothing.
    pub smooth_thickness_splat_scale: f32,
    /// Central-difference half-width in pixels for the surface normal reconstruction.
    /// 1 = original 1px stencil; 2-3px low-passes per-splat ripple into smoother normals.
    pub normal_stencil: u32,
    /// Optional normal blur strength (0–1). Blends the raw reconstructed normal toward
    /// an averaged normal sampled over a small offset cross, further smoothing.
    pub normal_smooth_strength: f32,
    // --- Flat-water-against-walls (v1.20) ---
    /// Blend strength for snapping water surface normals flat against tank walls/floor.
    /// At 1.0, water pressed against glass renders as a flat sheet. Live. Default 0.8.
    pub flat_water_strength: f32,
    /// Distance (box-local units) within which the flat-water normal snap engages.
    /// Roughly one cell ≈ 0.03 units in a [-1,1]^3 tank at 64-cell resolution. Live. Default 0.04.
    pub flat_water_epsilon: f32,
    /// Blend strength for snapping the water FRONT-SURFACE DEPTH (silhouette) to the wall plane.
    /// At 1.0 the depth is fully coplanar with the glass for near-wall pixels, eliminating
    /// bumpy sphere silhouettes at the glass. Live. Default 0.8.
    pub flat_water_depth_strength: f32,
}

/// A flat snapshot of the surface-foam settings. Like [`HeroParams`] these are
/// Live: the renderer mirrors this into the foam emit/update/render uniform
/// whenever a `render.diffuse.*` slider changes.
/// `max_particles` is an active cap within a fixed buffer capacity, not a realloc.
#[derive(Clone, Copy, Debug)]
pub struct DiffuseParams {
    pub enabled: bool,
    pub max_particles: u32,
    pub emit_rate: f32,
    pub emit_budget_per_frame: u32,
    pub surface_speed_threshold: f32,
    pub surface_speed_gain: f32,
    pub foam_lifetime: f32,
    pub radius: f32,
    pub alpha: f32,
    pub random_seed: u32,
}

/// The authoritative settings table. Order is stable; lookups are by id.
pub struct Registry {
    settings: Vec<Setting>,
}

impl Default for Registry {
    fn default() -> Self {
        let settings = vec![
            Setting {
                id: "scene.preset",
                label: "Scenario",
                category: Category::Scene,
                default: Value::U32(0),
                value: Value::U32(0),
                validation: Validation::U32Range { min: 0, max: 2 },
                tooltip: Some("Chooses the starting setup and resets the sim into that scenario."),
                technical_tooltip: Some("Reset-class enum. The stored value is the option index; the web panel calls reset after changing it so scene buffers are rebuilt immediately."),
                apply: ApplyClass::Reset,
            },
            Setting {
                id: "scene.drop_height",
                label: "Drop height",
                category: Category::Scene,
                default: Value::F32(0.72),
                value: Value::F32(0.72),
                validation: Validation::F32Range { min: 0.0, max: 1.0 },
                tooltip: Some("Sets where suspended scenarios start vertically; Falling Blob and Double Splash shift on reset. Dam Break stays floor-anchored, so this has limited effect there."),
                technical_tooltip: Some("Reset-class normalized height. Suspended blocks are shifted by height delta and clamped inside [0,1] while preserving size; Dam Break keeps its floor-anchored column."),
                apply: ApplyClass::Reset,
            },
            Setting {
                id: "grid.res_x",
                label: "Grid resolution X",
                category: Category::Grid,
                default: Value::U32(64),
                value: Value::U32(64),
                validation: Validation::U32Range { min: 16, max: 128 },
                tooltip: Some("Sets the tank width in grid cells; higher gives a wider, finer X axis."),
                technical_tooltip: Some("Reset-class. The sim keeps a uniform cell size and reallocates grid and solver buffers when any axis resolution changes."),
                apply: ApplyClass::Reset,
            },
            Setting {
                id: "grid.res_y",
                label: "Grid resolution Y",
                category: Category::Grid,
                default: Value::U32(64),
                value: Value::U32(64),
                validation: Validation::U32Range { min: 16, max: 128 },
                tooltip: Some("Sets the tank height in grid cells; higher gives a taller, finer Y axis."),
                technical_tooltip: Some("Reset-class. The sim keeps a uniform cell size and reallocates grid and solver buffers when any axis resolution changes."),
                apply: ApplyClass::Reset,
            },
            Setting {
                id: "grid.res_z",
                label: "Grid resolution Z",
                category: Category::Grid,
                default: Value::U32(64),
                value: Value::U32(64),
                validation: Validation::U32Range { min: 16, max: 128 },
                tooltip: Some("Sets the tank depth in grid cells; higher gives a deeper, finer Z axis."),
                technical_tooltip: Some("Reset-class. The sim keeps a uniform cell size and reallocates grid and solver buffers when any axis resolution changes."),
                apply: ApplyClass::Reset,
            },
            Setting {
                id: "particles.count",
                label: "Particle count",
                category: Category::Particles,
                default: Value::U32(254_144),
                value: Value::U32(254_144),
                validation: Validation::U32Range {
                    min: 1_024,
                    max: 134_217_728,
                },
                tooltip: Some("Sets how much liquid is seeded at reset; more particles look smoother but cost more."),
                technical_tooltip: Some("Reset-class initial mass/distribution control. The slider steps by powers of two, while the number box accepts any value in range; large requests may hit device limits."),
                apply: ApplyClass::Reset,
            },
            Setting {
                id: "physics.gravity",
                label: "Gravity",
                category: Category::Physics,
                default: Value::F32(-9.81),
                value: Value::F32(-9.81),
                validation: Validation::F32Range {
                    min: -40.0,
                    max: 40.0,
                },
                tooltip: Some("Changes how hard the liquid falls; positive values pull it upward."),
                technical_tooltip: Some("Live Y-axis gravitational acceleration in m/s^2 before tank interaction modes rotate the gravity direction."),
                apply: ApplyClass::Live,
            },
            Setting {
                id: "physics.fixed_dt",
                label: "Fixed timestep",
                category: Category::Physics,
                default: Value::F32(1.0 / 120.0),
                value: Value::F32(1.0 / 120.0),
                validation: Validation::F32Range {
                    min: 1.0 / 480.0,
                    max: 1.0 / 30.0,
                },
                tooltip: Some("Changes the physics step size; smaller is steadier, larger is cheaper but riskier."),
                technical_tooltip: Some("Reset-class fixed timestep in seconds. Browser frame time is accumulated separately and never feeds advection directly."),
                apply: ApplyClass::Reset,
            },
            Setting {
                id: "physics.flip_blend",
                label: "FLIP blend",
                category: Category::Physics,
                default: Value::F32(0.9),
                value: Value::F32(0.9),
                validation: Validation::F32Range { min: 0.0, max: 1.0 },
                tooltip: Some("Lower is calmer and more damped; higher is livelier and splashier."),
                technical_tooltip: Some("Live PIC/FLIP velocity-transfer blend. 0 is pure PIC; 1 is pure FLIP."),
                apply: ApplyClass::Live,
            },
            Setting {
                id: "physics.max_substeps",
                label: "Max substeps",
                category: Category::Physics,
                default: Value::U32(1),
                value: Value::U32(1),
                validation: Validation::U32Range { min: 1, max: 16 },
                tooltip: Some("Limits physics catch-up work per rendered frame; higher helps stress tests but costs interactivity."),
                technical_tooltip: Some("Reset-class cap on fixed-dt substeps per rAF callback. Default 1 drops excess accumulated sim time instead of extending a slow frame."),
                apply: ApplyClass::Reset,
            },
            Setting {
                id: "physics.wall_friction",
                label: "Wall friction",
                category: Category::Physics,
                default: Value::F32(0.0),
                value: Value::F32(0.0),
                validation: Validation::F32Range { min: 0.0, max: 1.0 },
                tooltip: Some("Adds optional drag along tank walls; 0 keeps wall contact free-sliding."),
                technical_tooltip: Some("Live tangential damping near walls. Closed-wall normal blocking remains enforced even when free-slip sampling removes wall-adjacent cling."),
                apply: ApplyClass::Live,
            },
            Setting {
                id: "physics.rest_density",
                label: "Rest particles/cell",
                category: Category::Physics,
                default: Value::F32(8.0),
                value: Value::F32(8.0),
                validation: Validation::F32Range { min: 1.0, max: 32.0 },
                tooltip: Some("Sets the crowding target for anti-clump volume correction."),
                technical_tooltip: Some("Live target particles per liquid cell. Cells above this target receive volume-correction bias through the pressure projection."),
                apply: ApplyClass::Live,
            },
            Setting {
                id: "physics.volume_stiffness",
                label: "Volume stiffness",
                category: Category::Physics,
                default: Value::F32(0.45),
                value: Value::F32(0.45),
                validation: Validation::F32Range { min: 0.0, max: 4.0 },
                tooltip: Some("Controls how strongly crowded regions are pushed apart."),
                technical_tooltip: Some("Live anti-clump stiffness. 0 disables the occupancy-driven divergence bias; higher values push over-dense cells toward Rest particles/cell."),
                apply: ApplyClass::Live,
            },
            Setting {
                id: "physics.drift_clamp",
                label: "Drift clamp",
                category: Category::Physics,
                default: Value::F32(0.5),
                value: Value::F32(0.5),
                validation: Validation::F32Range { min: 0.05, max: 2.0 },
                tooltip: Some("Caps how fast volume correction can act; lower is gentler, higher corrects faster."),
                technical_tooltip: Some("Live stability clamp on the per-step volume-correction divergence bias. It is inert when Volume stiffness is 0."),
                apply: ApplyClass::Live,
            },
            Setting {
                id: "physics.cfl",
                label: "Splash speed (CFL)",
                category: Category::Physics,
                default: Value::F32(2.0),
                value: Value::F32(2.0),
                validation: Validation::F32Range { min: 1.0, max: 6.0 },
                tooltip: Some("Raises or lowers the speed ceiling for thrown and sloshing water."),
                technical_tooltip: Some("Live CFL number: max grid cells a particle may cross per substep. The velocity ceiling is CFL * h / dt."),
                apply: ApplyClass::Live,
            },
            Setting {
                id: "interaction.auto_roll_enabled",
                label: "Auto-roll enabled",
                category: Category::Interaction,
                default: Value::U32(0),
                value: Value::U32(0),
                validation: Validation::U32Range { min: 0, max: 1 },
                tooltip: Some("Automatically rocks the tank while the simulation is running."),
                technical_tooltip: Some("Live app-side scheduler. It changes the tank orientation, not the camera, and pushes the rotated gravity vector after each pose update."),
                apply: ApplyClass::Live,
            },
            Setting {
                id: "interaction.auto_roll_strength",
                label: "Auto-roll strength",
                category: Category::Interaction,
                default: Value::F32(0.22),
                value: Value::F32(0.22),
                validation: Validation::F32Range { min: 0.0, max: 1.2 },
                tooltip: Some("Sets the maximum random tank tilt; lower values make a gentler rocking motion."),
                technical_tooltip: Some("Live maximum target-pose tilt in radians. The scheduler uses deterministic PRNG targets and smooth interpolation rather than unbounded spin."),
                apply: ApplyClass::Live,
            },
            Setting {
                id: "interaction.auto_roll_cadence",
                label: "Auto-roll cadence",
                category: Category::Interaction,
                default: Value::F32(2.5),
                value: Value::F32(2.5),
                validation: Validation::F32Range { min: 0.5, max: 8.0 },
                tooltip: Some("Controls how often the automatic tank roll picks a new target pose."),
                technical_tooltip: Some("Live nominal seconds between auto-roll targets. Each segment gets deterministic jitter so repeated targets do not feel metronomic."),
                apply: ApplyClass::Live,
            },
            Setting {
                id: "interaction.wave_enabled",
                label: "Wave maker enabled",
                category: Category::Interaction,
                default: Value::U32(0),
                value: Value::U32(0),
                validation: Validation::U32Range { min: 0, max: 1 },
                tooltip: Some("Adds periodic horizontal impulses so the liquid keeps making waves."),
                technical_tooltip: Some("Live app-side scheduler. It uses the existing particle impulse dispatch and never creates or destroys particles."),
                apply: ApplyClass::Live,
            },
            Setting {
                id: "interaction.wave_strength",
                label: "Wave strength",
                category: Category::Interaction,
                default: Value::F32(0.5),
                value: Value::F32(0.5),
                validation: Validation::F32Range { min: 0.0, max: 3.0 },
                tooltip: Some("Sets the size of each wave-maker kick."),
                technical_tooltip: Some("Live local-frame velocity impulse magnitude in units/s. Impulses are horizontal in tank space and alternate direction."),
                apply: ApplyClass::Live,
            },
            Setting {
                id: "interaction.wave_frequency",
                label: "Wave frequency",
                category: Category::Interaction,
                default: Value::F32(0.75),
                value: Value::F32(0.75),
                validation: Validation::F32Range { min: 0.05, max: 4.0 },
                tooltip: Some("Controls how many wave-maker kicks happen per second."),
                technical_tooltip: Some("Live frequency in Hz. Pause freezes the countdown; resume continues from the same scheduler state."),
                apply: ApplyClass::Live,
            },
            Setting {
                id: "classify.liquid_threshold",
                label: "Liquid threshold",
                category: Category::Solver,
                default: Value::U32(1),
                value: Value::U32(1),
                validation: Validation::U32Range { min: 1, max: 8 },
                tooltip: Some("Changes which occupied cells count as liquid for pressure solving."),
                technical_tooltip: Some("Live liquid-cell inclusion threshold. A cell needs at least this many particles before it joins the active pressure region."),
                apply: ApplyClass::Live,
            },
            Setting {
                id: "classify.surface_dilation",
                label: "Surface dilation",
                category: Category::Solver,
                default: Value::U32(0),
                value: Value::U32(0),
                validation: Validation::U32Range { min: 0, max: 1 },
                tooltip: Some("Optionally expands the liquid region by one cell to seal tiny gaps."),
                technical_tooltip: Some("Live morphology toggle. 1 makes cells adjacent to liquid participate as liquid too, increasing pressure work slightly."),
                apply: ApplyClass::Live,
            },
            Setting {
                id: "solver.pressure_iterations",
                label: "Pressure iterations",
                category: Category::Solver,
                default: Value::U32(30),
                value: Value::U32(30),
                validation: Validation::U32Range { min: 1, max: 200 },
                tooltip: Some("More iterations make water resist compression better but cost FPS."),
                technical_tooltip: Some("Live Conjugate-Gradient pressure-solve iteration cap per substep. Default 30 gives headroom beyond the usual 64^3 convergence knee."),
                apply: ApplyClass::Live,
            },
            Setting {
                id: "render.particle_size",
                label: "Particle size",
                category: Category::Render,
                default: Value::F32(0.45),
                value: Value::F32(0.45),
                validation: Validation::F32Range { min: 0.2, max: 5.0 },
                tooltip: Some("Changes only how large particles are drawn; it does not add fluid."),
                technical_tooltip: Some("Live render-only point-size multiplier. Physics mass, seeding, pressure, and liquid-cell classification are unchanged."),
                apply: ApplyClass::Live,
            },
            Setting {
                id: "render.speed_scale",
                label: "Speed\u{2192}white",
                category: Category::Render,
                default: Value::F32(4.0),
                value: Value::F32(4.0),
                validation: Validation::F32Range { min: 0.5, max: 20.0 },
                tooltip: Some("Sets how fast particles must move before the speed color reaches white."),
                technical_tooltip: Some("Live render-only color scale in units/s. Particle color ramps from slow to fast color over [0, this value]."),
                apply: ApplyClass::Live,
            },
            Setting {
                id: "render.particle_view",
                label: "Water view",
                category: Category::Render,
                default: Value::U32(0),
                value: Value::U32(0),
                validation: Validation::U32Range { min: 0, max: 2 },
                tooltip: Some("Switches between screen-space water and particle renderers."),
                technical_tooltip: Some("Live enum. 0 selects screen-space water; 1 selects the v1.10 optical-depth particle path; 2 selects the pre-v1.10 simple alpha billboard path."),
                apply: ApplyClass::Live,
            },
            Setting {
                id: "render.fps_target",
                label: "FPS target",
                category: Category::Render,
                default: Value::U32(60),
                value: Value::U32(60),
                validation: Validation::U32Range { min: 0, max: 240 },
                tooltip: Some("Caps rendering smoothness and GPU load; 0 runs as fast as the browser allows."),
                technical_tooltip: None,
                apply: ApplyClass::Live,
            },
            Setting {
                id: "camera.rot_x",
                label: "Camera pitch",
                category: Category::Camera,
                default: Value::F32(-0.2),
                value: Value::F32(-0.2),
                validation: Validation::F32Range { min: -3.14159, max: 3.14159 },
                tooltip: None,
                technical_tooltip: None,
                apply: ApplyClass::Live,
            },
            Setting {
                id: "camera.rot_y",
                label: "Camera yaw",
                category: Category::Camera,
                default: Value::F32(0.6),
                value: Value::F32(0.6),
                validation: Validation::F32Range { min: -3.14159, max: 3.14159 },
                tooltip: None,
                technical_tooltip: None,
                apply: ApplyClass::Live,
            },
            Setting {
                id: "camera.rot_z",
                label: "Camera roll",
                category: Category::Camera,
                default: Value::F32(0.0),
                value: Value::F32(0.0),
                validation: Validation::F32Range { min: -3.14159, max: 3.14159 },
                tooltip: None,
                technical_tooltip: None,
                apply: ApplyClass::Live,
            },
            Setting {
                id: "camera.distance",
                label: "Camera distance",
                category: Category::Camera,
                default: Value::F32(6.0),
                value: Value::F32(6.0),
                validation: Validation::F32Range { min: 2.0, max: 40.0 },
                tooltip: Some("Sets the starting camera zoom distance from the tank."),
                technical_tooltip: None,
                apply: ApplyClass::Live,
            },
            Setting {
                id: "render.particle_slow_color",
                label: "Slow particle color",
                category: Category::Render,
                default: Value::U32(0x194C_CC),
                value: Value::U32(0x194C_CC),
                validation: Validation::U32Range { min: 0, max: 0x00FF_FFFF },
                tooltip: None,
                technical_tooltip: None,
                apply: ApplyClass::Live,
            },
            Setting {
                id: "render.particle_fast_color",
                label: "Fast particle color",
                category: Category::Render,
                default: Value::U32(0xB2EB_FF),
                value: Value::U32(0xB2EB_FF),
                validation: Validation::U32Range { min: 0, max: 0x00FF_FFFF },
                tooltip: None,
                technical_tooltip: None,
                apply: ApplyClass::Live,
            },
            Setting {
                id: "render.water_optical_density",
                label: "Water absorption",
                category: Category::Render,
                default: Value::F32(1.25),
                value: Value::F32(1.25),
                validation: Validation::F32Range { min: 0.0, max: 8.0 },
                tooltip: Some("Controls how strongly accumulated water thickness absorbs light; higher makes deep water read denser."),
                technical_tooltip: Some("Live Beer-Lambert absorption k for normalized screen-space thickness. This is not direct alpha/opacity, and particle size must not change represented volume."),
                apply: ApplyClass::Live,
            },
            Setting {
                id: "render.particle_edge",
                label: "Particle edge",
                category: Category::Render,
                default: Value::F32(0.6),
                value: Value::F32(0.6),
                validation: Validation::F32Range { min: 0.0, max: 0.99 },
                tooltip: Some("Controls whether particle circles read as soft blobs or crisp discs."),
                technical_tooltip: None,
                apply: ApplyClass::Live,
            },
            Setting {
                id: "render.particle_shading",
                label: "Surface lighting",
                category: Category::Render,
                default: Value::F32(0.25),
                value: Value::F32(0.25),
                validation: Validation::F32Range { min: 0.0, max: 1.0 },
                tooltip: Some("Controls the strength of screen-space water surface lighting."),
                technical_tooltip: Some("Live surface-lighting gain for the screen-space water composite."),
                apply: ApplyClass::Live,
            },
            Setting {
                id: "render.whitewater_strength",
                label: "Whitewater",
                category: Category::Render,
                default: Value::F32(0.65),
                value: Value::F32(0.65),
                validation: Validation::F32Range { min: 0.0, max: 1.0 },
                tooltip: Some("Adds white highlights where fast water makes the surface look rough."),
                technical_tooltip: Some("Live screen-space mix toward white/ice-blue from the speed-weighted thickness target."),
                apply: ApplyClass::Live,
            },
            Setting {
                id: "render.whitewater_threshold",
                label: "Whitewater speed",
                category: Category::Render,
                default: Value::F32(0.38),
                value: Value::F32(0.38),
                validation: Validation::F32Range { min: 0.0, max: 1.0 },
                tooltip: Some("Sets how much normalized speed is needed before whitewater appears."),
                technical_tooltip: Some("Live threshold over speed-weighted thickness divided by total thickness. Lower values show more rough water."),
                apply: ApplyClass::Live,
            },
            Setting {
                id: "render.whitewater_softness",
                label: "Whitewater softness",
                category: Category::Render,
                default: Value::F32(0.22),
                value: Value::F32(0.22),
                validation: Validation::F32Range { min: 0.01, max: 1.0 },
                tooltip: Some("Controls how gradually whitewater fades in around the speed threshold."),
                technical_tooltip: Some("Live smoothing width for the whitewater threshold in normalized speed units."),
                apply: ApplyClass::Live,
            },
            // --- Hero water (Water tab). All Live: sliders auto-apply by
            // rebuilding the HeroParams uniform; no pipeline rebuilds, no reset. ---
            Setting {
                id: "render.hero.refraction_enabled",
                label: "Refraction",
                category: Category::Water,
                default: Value::U32(1),
                value: Value::U32(1),
                validation: Validation::U32Range { min: 0, max: 1 },
                tooltip: Some("Bend the scene color through the water surface normal. Off samples the same background without the UV offset."),
                technical_tooltip: Some("Live enum. Gates only the normal-driven scene-color refraction offset; body color and reflection remain independently controlled."),
                apply: ApplyClass::Live,
            },
            Setting {
                id: "render.hero.reflection_enabled",
                label: "Reflection",
                category: Category::Water,
                default: Value::U32(1),
                value: Value::U32(1),
                validation: Validation::U32Range { min: 0, max: 1 },
                tooltip: Some("Reflect the procedural sky/room and sun highlight on the water surface."),
                technical_tooltip: Some("Live enum. Gates Fresnel environment reflection and sun specular in the water composite only; the skybox and environment prepass stay active."),
                apply: ApplyClass::Live,
            },
            Setting {
                id: "render.hero.body_color_enabled",
                label: "Body color",
                category: Category::Water,
                default: Value::U32(1),
                value: Value::U32(1),
                validation: Validation::U32Range { min: 0, max: 1 },
                tooltip: Some("Show the water body tint, absorption, transparency, and deep-water darkening."),
                technical_tooltip: Some("Live enum. Gates Beer-Lambert absorption, base tint, transparency, and deep-water darkening without changing refraction or reflection."),
                apply: ApplyClass::Live,
            },
            Setting {
                id: "render.hero.wall_contact_enabled",
                label: "Wall contact snap",
                category: Category::Water,
                default: Value::U32(1),
                value: Value::U32(1),
                validation: Validation::U32Range { min: 0, max: 1 },
                tooltip: Some("Flatten near-wall water normals and depth so water pressed against glass reads as a sheet."),
                technical_tooltip: Some("Live enum. Gates the cheap flat_water normal/depth correction."),
                apply: ApplyClass::Live,
            },
            Setting {
                id: "render.hero.debug_view",
                label: "Debug view",
                category: Category::Water,
                default: Value::U32(0),
                value: Value::U32(0),
                validation: Validation::U32Range { min: 0, max: 11 },
                tooltip: Some("Routes an intermediate hero-water buffer to the screen for debugging."),
                technical_tooltip: Some("Live enum. 0 = normal composite; other values blit a single stage (scene color/depth, thickness, refraction offset, Fresnel, absorption, water-only, reflection/env, nearest_z, whitewater) to the swapchain."),
                apply: ApplyClass::Live,
            },
            Setting {
                id: "render.hero.ior",
                label: "Index of refraction",
                category: Category::Water,
                default: Value::F32(1.33),
                value: Value::F32(1.33),
                validation: Validation::F32Range { min: 1.0, max: 2.0 },
                tooltip: Some("Optical density of the water; drives both the Fresnel rim and the bend strength."),
                technical_tooltip: Some("Live. Schlick f0 = ((ior-1)/(ior+1))^2 is derived from this, so f0 is never an independent knob."),
                apply: ApplyClass::Live,
            },
            Setting {
                id: "render.hero.refraction_strength",
                label: "Refraction strength",
                category: Category::Water,
                default: Value::F32(0.6),
                value: Value::F32(0.6),
                validation: Validation::F32Range { min: 0.0, max: 2.0 },
                tooltip: Some("How strongly the floor and backdrop bend as they pass through the water."),
                technical_tooltip: Some("Live. Scales the screen-space UV offset taken along the water surface normal's xy."),
                apply: ApplyClass::Live,
            },
            Setting {
                id: "render.hero.refraction_thickness_scale",
                label: "Refraction thickness",
                category: Category::Water,
                default: Value::F32(1.0),
                value: Value::F32(1.0),
                validation: Validation::F32Range { min: 0.0, max: 4.0 },
                tooltip: Some("Makes thicker water bend the background more than thin water."),
                technical_tooltip: Some("Live. Multiplies normalized thickness into the refraction offset so deep water refracts harder than rim water."),
                apply: ApplyClass::Live,
            },
            Setting {
                id: "render.hero.refraction_max_offset_px",
                label: "Max bend (px)",
                category: Category::Water,
                default: Value::U32(48),
                value: Value::U32(48),
                validation: Validation::U32Range { min: 0, max: 256 },
                tooltip: Some("Caps how far the refracted sample can travel; keeps grazing angles from smearing."),
                technical_tooltip: Some("Live. Clamps the refraction UV offset to this many pixels before sampling scene color."),
                apply: ApplyClass::Live,
            },
            Setting {
                id: "render.hero.invalid_refraction_fallback",
                label: "Invalid sample",
                category: Category::Water,
                default: Value::U32(0),
                value: Value::U32(0),
                validation: Validation::U32Range { min: 0, max: 1 },
                tooltip: Some("What to show when the refracted sample would grab geometry in front of the water."),
                technical_tooltip: Some("Live enum. 0 = fall back to the unrefracted scene-color tap; 1 = fall back to the flat base tint."),
                apply: ApplyClass::Live,
            },
            Setting {
                id: "render.hero.absorption_color",
                label: "Absorption color",
                category: Category::Water,
                default: Value::U32(0x3366_80),
                value: Value::U32(0x3366_80),
                validation: Validation::U32Range { min: 0, max: 0x00FF_FFFF },
                tooltip: Some("Per-channel Beer-Lambert extinction color; higher channels are absorbed faster with depth."),
                technical_tooltip: Some("Live. Used as extinction coefficients exp(-absorption_color*strength*thickness) applied to the refracted background."),
                apply: ApplyClass::Live,
            },
            Setting {
                id: "render.hero.absorption_strength",
                label: "Absorption strength",
                category: Category::Water,
                default: Value::F32(2.4),
                value: Value::F32(2.4),
                validation: Validation::F32Range { min: 0.0, max: 8.0 },
                tooltip: Some("Overall strength of how much the water dims the background it passes through."),
                technical_tooltip: Some("Live Beer-Lambert k multiplier over normalized screen-space thickness."),
                apply: ApplyClass::Live,
            },
            Setting {
                id: "render.hero.base_tint",
                label: "Water tint",
                category: Category::Water,
                default: Value::U32(0x1B6F_A6),
                value: Value::U32(0x1B6F_A6),
                validation: Validation::U32Range { min: 0, max: 0x00FF_FFFF },
                tooltip: Some("The water's own body color, mixed in as the water gets deeper."),
                technical_tooltip: Some("Live. Blended toward by the thickness-driven body factor."),
                apply: ApplyClass::Live,
            },
            Setting {
                id: "render.hero.transparency",
                label: "Transparency",
                category: Category::Water,
                default: Value::F32(0.18),
                value: Value::F32(0.18),
                validation: Validation::F32Range { min: 0.0, max: 1.0 },
                tooltip: Some("How much of the refracted background remains visible through deep water."),
                technical_tooltip: Some("Live. Scales down the body-color opacity so 1.0 keeps the background fully visible."),
                apply: ApplyClass::Live,
            },
            Setting {
                id: "render.hero.deep_water_darkening",
                label: "Deep darkening",
                category: Category::Water,
                default: Value::F32(2.4),
                value: Value::F32(2.4),
                validation: Validation::F32Range { min: 0.0, max: 6.0 },
                tooltip: Some("How quickly the water turns to its body color with depth."),
                technical_tooltip: Some("Live. Controls the 1-exp(-k*thickness) body factor used for tint and opacity."),
                apply: ApplyClass::Live,
            },
            Setting {
                id: "render.hero.floor_pattern_scale",
                label: "Floor pattern scale",
                category: Category::Water,
                default: Value::F32(8.0),
                value: Value::F32(8.0),
                validation: Validation::F32Range { min: 1.0, max: 32.0 },
                tooltip: Some("Size of the checker/grid pattern on the tank floor; gives refraction something to bend."),
                technical_tooltip: Some("Live. Tiling frequency of the procedural floor pattern in tank-UV space."),
                apply: ApplyClass::Live,
            },
            Setting {
                id: "render.hero.floor_pattern_strength",
                label: "Floor pattern strength",
                category: Category::Water,
                default: Value::F32(0.5),
                value: Value::F32(0.5),
                validation: Validation::F32Range { min: 0.0, max: 1.0 },
                tooltip: Some("Contrast of the floor pattern."),
                technical_tooltip: Some("Live. Mix amount between the floor base color and the pattern color."),
                apply: ApplyClass::Live,
            },
            Setting {
                id: "render.hero.backdrop_strength",
                label: "Backdrop strength",
                category: Category::Water,
                default: Value::F32(0.6),
                value: Value::F32(0.6),
                validation: Validation::F32Range { min: 0.0, max: 1.0 },
                tooltip: Some("Brightness of the gradient backdrop behind the tank."),
                technical_tooltip: Some("Live. Scales the procedural backdrop gradient written into scene color."),
                apply: ApplyClass::Live,
            },
            Setting {
                id: "render.hero.wall_visibility",
                label: "Wall visibility",
                category: Category::Water,
                default: Value::F32(0.3),
                value: Value::F32(0.3),
                validation: Validation::F32Range { min: 0.0, max: 1.0 },
                tooltip: Some("How visible the matte tank walls are."),
                technical_tooltip: Some("Live. Opacity/brightness of the matte side walls in the environment prepass."),
                apply: ApplyClass::Live,
            },
            // --- Environment reflection (v1.15). The water reflects a procedural
            // sky/room that is shared with the world-background skybox. All Live. ---
            Setting {
                id: "render.hero.reflection_strength",
                label: "Reflection strength",
                category: Category::Water,
                default: Value::F32(0.8),
                value: Value::F32(0.8),
                validation: Validation::F32Range { min: 0.0, max: 2.0 },
                tooltip: Some("How strongly the surface reflects the sky/room environment at glancing angles."),
                technical_tooltip: Some("Live. Scales the Fresnel-weighted environment reflection mixed into the composite. Forced to 0 when hero water is off."),
                apply: ApplyClass::Live,
            },
            Setting {
                id: "render.hero.environment_strength",
                label: "Environment strength",
                category: Category::Water,
                default: Value::F32(1.0),
                value: Value::F32(1.0),
                validation: Validation::F32Range { min: 0.0, max: 2.0 },
                tooltip: Some("Brightness of the reflected environment color."),
                technical_tooltip: Some("Live. Multiplies the env_sample reflection color before the Fresnel mix."),
                apply: ApplyClass::Live,
            },
            Setting {
                id: "render.hero.environment_mode",
                label: "Environment mode",
                category: Category::Water,
                default: Value::U32(0),
                value: Value::U32(0),
                validation: Validation::U32Range { min: 0, max: 2 },
                tooltip: Some("Which procedural environment the water reflects and the background shows."),
                technical_tooltip: Some("Live enum. 0 = Sky, 1 = Room (azimuth wall panels), 2 = Studio (bright neutral)."),
                apply: ApplyClass::Live,
            },
            Setting {
                id: "render.hero.environment_rotation",
                label: "Environment rotation",
                category: Category::Water,
                default: Value::F32(0.0),
                value: Value::F32(0.0),
                validation: Validation::F32Range { min: -3.1416, max: 3.1416 },
                tooltip: Some("Spins the reflected sky/room around vertical."),
                technical_tooltip: Some("Live. Yaw (radians) applied to the env sample direction; the env stays world-fixed otherwise."),
                apply: ApplyClass::Live,
            },
            Setting {
                id: "render.hero.environment_brightness",
                label: "Environment brightness",
                category: Category::Water,
                default: Value::F32(1.0),
                value: Value::F32(1.0),
                validation: Validation::F32Range { min: 0.0, max: 3.0 },
                tooltip: Some("Overall brightness of the procedural sky/room (background and reflection)."),
                technical_tooltip: Some("Live. Final multiplier inside env_sample; shared by the skybox and the reflection."),
                apply: ApplyClass::Live,
            },
            Setting {
                id: "render.hero.skybox_enabled",
                label: "World skybox",
                category: Category::Water,
                default: Value::U32(1),
                value: Value::U32(1),
                validation: Validation::U32Range { min: 0, max: 1 },
                tooltip: Some("Draw the procedural sky/room as the world background behind the tank."),
                technical_tooltip: Some("Live enum. When on, a fullscreen procedural skybox fills the scene-color prepass behind the geometry; it is camera-driven and does NOT rotate with the box."),
                apply: ApplyClass::Live,
            },
            Setting {
                id: "render.hero.roughness_base",
                label: "Roughness base",
                category: Category::Water,
                default: Value::F32(0.12),
                value: Value::F32(0.12),
                validation: Validation::F32Range { min: 0.0, max: 1.0 },
                tooltip: Some("Baseline surface roughness; higher values blur the reflection and widen the highlight."),
                technical_tooltip: Some("Live. Blends the reflected env toward an averaged sky and softens the specular exponent."),
                apply: ApplyClass::Live,
            },
            Setting {
                id: "render.hero.roughness_velocity_scale",
                label: "Roughness from speed",
                category: Category::Water,
                default: Value::F32(0.6),
                value: Value::F32(0.6),
                validation: Validation::F32Range { min: 0.0, max: 2.0 },
                tooltip: Some("How much fast/breaking water roughens the reflection."),
                technical_tooltip: Some("Live. Adds the speed-weighted whitewater fraction (a velocity proxy) into roughness."),
                apply: ApplyClass::Live,
            },
            Setting {
                id: "render.hero.roughness_normal_variance_scale",
                label: "Roughness from chop",
                category: Category::Water,
                default: Value::F32(0.5),
                value: Value::F32(0.5),
                validation: Validation::F32Range { min: 0.0, max: 2.0 },
                tooltip: Some("How much choppy/curved surface detail roughens the reflection."),
                technical_tooltip: Some("Live. Adds the local surface curvature (depth Laplacian) into roughness."),
                apply: ApplyClass::Live,
            },
            Setting {
                id: "render.hero.roughness_foam_scale",
                label: "Roughness from foam",
                category: Category::Water,
                default: Value::F32(0.8),
                value: Value::F32(0.8),
                validation: Validation::F32Range { min: 0.0, max: 2.0 },
                tooltip: Some("How much foam roughens the reflection."),
                technical_tooltip: Some("Live. Adds the composite foam factor into roughness."),
                apply: ApplyClass::Live,
            },
            Setting {
                id: "render.hero.specular_strength",
                label: "Sun specular",
                category: Category::Water,
                default: Value::F32(1.0),
                value: Value::F32(1.0),
                validation: Validation::F32Range { min: 0.0, max: 4.0 },
                tooltip: Some("Brightness of the sun's specular highlight on the water."),
                technical_tooltip: Some("Live. Scales the sharp sun highlight along the reflection vector; its width follows roughness."),
                apply: ApplyClass::Live,
            },
            Setting {
                id: "render.hero.sun_dir_x",
                label: "Sun direction X",
                category: Category::Water,
                default: Value::F32(0.4),
                value: Value::F32(0.4),
                validation: Validation::F32Range { min: -1.0, max: 1.0 },
                tooltip: Some("World-space sun direction (X)."),
                technical_tooltip: Some("Live. Component of the normalized world-space sun direction shared by the sky glow and the specular highlight."),
                apply: ApplyClass::Live,
            },
            Setting {
                id: "render.hero.sun_dir_y",
                label: "Sun direction Y",
                category: Category::Water,
                default: Value::F32(0.7),
                value: Value::F32(0.7),
                validation: Validation::F32Range { min: -1.0, max: 1.0 },
                tooltip: Some("World-space sun direction (Y, up)."),
                technical_tooltip: Some("Live. Vertical component of the world-space sun direction."),
                apply: ApplyClass::Live,
            },
            Setting {
                id: "render.hero.sun_dir_z",
                label: "Sun direction Z",
                category: Category::Water,
                default: Value::F32(0.5),
                value: Value::F32(0.5),
                validation: Validation::F32Range { min: -1.0, max: 1.0 },
                tooltip: Some("World-space sun direction (Z)."),
                technical_tooltip: Some("Live. Depth component of the world-space sun direction."),
                apply: ApplyClass::Live,
            },
            Setting {
                id: "render.hero.sun_intensity",
                label: "Sun intensity",
                category: Category::Water,
                default: Value::F32(1.2),
                value: Value::F32(1.2),
                validation: Validation::F32Range { min: 0.0, max: 4.0 },
                tooltip: Some("Brightness of the sun in the sky and on the water."),
                technical_tooltip: Some("Live. Scales the sun glow in env_sample and the specular highlight in the composite."),
                apply: ApplyClass::Live,
            },
            Setting {
                id: "render.hero.micro_normal_enabled",
                label: "Micro-normals",
                category: Category::Water,
                default: Value::U32(0),
                value: Value::U32(0),
                validation: Validation::U32Range { min: 0, max: 1 },
                tooltip: Some("Add fine surface 'tooth' that perturbs the reflection (off by default; can shimmer)."),
                technical_tooltip: Some("Live enum. Adds procedural high-frequency normal perturbation before the reflection/Fresnel taps. Keep conservative to avoid shimmer."),
                apply: ApplyClass::Live,
            },
            Setting {
                id: "render.hero.micro_normal_strength",
                label: "Micro-normal strength",
                category: Category::Water,
                default: Value::F32(0.15),
                value: Value::F32(0.15),
                validation: Validation::F32Range { min: 0.0, max: 1.0 },
                tooltip: Some("How strongly the micro-normals perturb the surface."),
                technical_tooltip: Some("Live. Amplitude of the screen-space micro-normal perturbation."),
                apply: ApplyClass::Live,
            },
            Setting {
                id: "render.hero.micro_normal_scale",
                label: "Micro-normal scale",
                category: Category::Water,
                default: Value::F32(60.0),
                value: Value::F32(60.0),
                validation: Validation::F32Range { min: 1.0, max: 256.0 },
                tooltip: Some("Frequency of the micro-normal detail."),
                technical_tooltip: Some("Live. Spatial frequency of the procedural micro-normal pattern in screen-UV space."),
                apply: ApplyClass::Live,
            },
            Setting {
                id: "render.hero.micro_normal_velocity_scale",
                label: "Micro-normal from speed",
                category: Category::Water,
                default: Value::F32(1.0),
                value: Value::F32(1.0),
                validation: Validation::F32Range { min: 0.0, max: 4.0 },
                tooltip: Some("How much fast water amplifies the micro-normals."),
                technical_tooltip: Some("Live. Scales the micro-normal amplitude by the speed-weighted whitewater fraction."),
                apply: ApplyClass::Live,
            },
            // --- Flat-water-against-walls (v1.20). Live: batched into composite Hero uniform. ---
            Setting {
                id: "render.hero.flat_water.strength",
                label: "Flat water strength",
                category: Category::Water,
                default: Value::F32(0.8),
                value: Value::F32(0.8),
                validation: Validation::F32Range { min: 0.0, max: 1.0 },
                tooltip: Some("Blends the water surface normal toward the adjacent wall/floor plane when the water front is within epsilon of the tank glass. At 1.0, wall-pressed water renders as a flat sheet."),
                technical_tooltip: Some("Live. In composite.wgsl: reconstructs front-surface box-local position from smooth_z + eye ray, tests min signed-distance to the five tank planes, and blends n toward the plane normal by strength*smoothstep. Requires box-local eye + box_rot in composite CamUniform."),
                apply: ApplyClass::Live,
            },
            Setting {
                id: "render.hero.flat_water.epsilon",
                label: "Flat water epsilon",
                category: Category::Water,
                default: Value::F32(0.04),
                value: Value::F32(0.04),
                validation: Validation::F32Range { min: 0.0, max: 0.2 },
                tooltip: Some("Distance from a tank wall (box-local units, [-1,1]^3 tank) within which the flat-water normal snap engages. ~0.04 ≈ one cell at 64-cell resolution."),
                technical_tooltip: Some("Live. Controls the smoothstep ramp width for the normal blend toward the nearest tank plane normal. Box-local units; the tank occupies [-1,1]^3 so walls are at +-1 on x and z, floor at y=-1."),
                apply: ApplyClass::Live,
            },
            Setting {
                id: "render.hero.flat_water.depth_strength",
                label: "Flat water depth strength",
                category: Category::Water,
                default: Value::F32(1.0),
                value: Value::F32(1.0),
                validation: Validation::F32Range { min: 0.0, max: 1.0 },
                tooltip: Some("Snaps the water front-surface DEPTH (silhouette), not just the normal, to the wall plane for near-wall pixels so wall-pressed water reads as a flat sheet rather than bumpy spheres."),
                technical_tooltip: Some("Live. In composite.wgsl, intersects the box-local eye ray with the nearest in-epsilon tank plane and mixes front_z toward the hit by depth_strength*ramp before the refraction depth guard. Routed via composite CamUniform flat.z."),
                apply: ApplyClass::Live,
            },
            // --- Screen-space surface quality (v1.19 polish). All Live: sliders
            // rebuild the HeroParams uniform via the existing render.hero.* batch
            // route. smooth_iterations/smooth_radius wire through smoothing.rs;
            // smooth_thickness_splat_scale routes through the particle camera
            // uniform so the particle system scales the nearest_z splat footprint. ---
            Setting {
                id: "render.hero.smooth_iterations",
                label: "Smooth iterations",
                category: Category::Water,
                default: Value::U32(2),
                value: Value::U32(2),
                validation: Validation::U32Range { min: 1, max: 4 },
                tooltip: Some("How many bilateral smoothing passes to run on the water depth each frame. More passes reduce the sphere look by fusing adjacent particle splats."),
                technical_tooltip: Some("Live. Each iteration runs one X+Y bilateral pass (ping-pong). sigma_spatial scales with the radius setting; the depth-range Gaussian keeps silhouettes intact."),
                apply: ApplyClass::Live,
            },
            Setting {
                id: "render.hero.smooth_radius",
                label: "Smooth radius",
                category: Category::Water,
                default: Value::U32(5),
                value: Value::U32(5),
                validation: Validation::U32Range { min: 3, max: 8 },
                tooltip: Some("Bilateral filter kernel half-width in pixels. Wider kernels merge more adjacent particle splats at the cost of a bit of performance."),
                technical_tooltip: Some("Live. sigma_spatial = radius / 2 so the Gaussian is never hard-truncated. The range Gaussian (sigma_range proportional to depth) stays tight to preserve silhouettes."),
                apply: ApplyClass::Live,
            },
            Setting {
                id: "render.hero.smooth_thickness_splat_scale",
                label: "Splat scale",
                category: Category::Water,
                default: Value::F32(1.8),
                value: Value::F32(1.8),
                validation: Validation::F32Range { min: 0.5, max: 4.0 },
                tooltip: Some("Enlarges the depth-capture footprint of each particle splat so neighbours overlap more before smoothing. Larger values (1.5-2.5x) help fuse adjacent splats into a continuous surface."),
                technical_tooltip: Some("Live. Multiplies cam.right.w (particle world radius) in the nearest_z write inside fs_thickness. Does NOT change the visible particle size or thickness accumulation."),
                apply: ApplyClass::Live,
            },
            Setting {
                id: "render.hero.normal_stencil",
                label: "Normal stencil width",
                category: Category::Water,
                default: Value::U32(2),
                value: Value::U32(2),
                validation: Validation::U32Range { min: 1, max: 3 },
                tooltip: Some("Central-difference half-width (pixels) for surface normal reconstruction. 1=original, 2-3 low-passes per-splat ripple and reduces the sphere look."),
                technical_tooltip: Some("Live. The normal central difference samples depth at pixel ± stencil in each axis, dividing by 2*stencil*world_per_px. Wider stencils average over more pixels, suppressing residual per-particle lobes."),
                apply: ApplyClass::Live,
            },
            Setting {
                id: "render.hero.normal_smooth_strength",
                label: "Normal smooth blend",
                category: Category::Water,
                default: Value::F32(0.5),
                value: Value::F32(0.5),
                validation: Validation::F32Range { min: 0.0, max: 1.0 },
                tooltip: Some("Blends the reconstructed normal toward an averaged neighbourhood normal. Reduces residual spiky lobes without blurring silhouettes."),
                technical_tooltip: Some("Live. Inline normal blur: weight to mix(pixel_normal, avg_cross_normal, strength). The avg samples the normal at 4 diagonal offsets of size stencil+1 pixels."),
                apply: ApplyClass::Live,
            },
            // --- Surface foam. All
            // Live: sliders rebuild the DiffuseParams uniform; no realloc, no reset.
            // `max_particles` is an active cap within a fixed GPU buffer capacity. ---
            Setting {
                id: "render.diffuse.enabled",
                label: "Foam",
                category: Category::Water,
                default: Value::U32(1),
                value: Value::U32(1),
                validation: Validation::U32Range { min: 0, max: 1 },
                tooltip: Some("Master toggle for conservative surface foam. The screen-space whitewater tint remains as the fallback when off."),
                technical_tooltip: Some("Live enum. Off skips foam emission/update/render; foam particles are surface-constrained and do not create spray, bubbles, or wall decals."),
                apply: ApplyClass::Live,
            },
            Setting {
                id: "render.diffuse.max_particles",
                label: "Max foam particles",
                category: Category::Water,
                default: Value::U32(64_000),
                value: Value::U32(64_000),
                validation: Validation::U32Range { min: 1_024, max: 262_144 },
                tooltip: Some("Caps how many surface foam particles can be alive at once; higher looks denser but costs more."),
                technical_tooltip: Some("Live active cap within a fixed GPU buffer capacity (262144). The emitter recycles the oldest slots via a ring cursor when this fills."),
                apply: ApplyClass::Live,
            },
            Setting {
                id: "render.diffuse.emit_rate",
                label: "Emission rate",
                category: Category::Water,
                default: Value::F32(0.85),
                value: Value::F32(0.85),
                validation: Validation::F32Range { min: 0.0, max: 8.0 },
                tooltip: Some("Overall multiplier on how readily surface foam is born."),
                technical_tooltip: Some("Live global gain on the per-cell stochastic spawn probability."),
                apply: ApplyClass::Live,
            },
            Setting {
                id: "render.diffuse.emit_budget_per_frame",
                label: "Emission budget/frame",
                category: Category::Water,
                default: Value::U32(1_500),
                value: Value::U32(1_500),
                validation: Validation::U32Range { min: 0, max: 40_000 },
                tooltip: Some("Caps new particles spawned per frame to bound GPU cost; clamping is reported in the profiler."),
                technical_tooltip: Some("Live per-frame spawn budget enforced by an integer atomic; rejected spawns are counted and surfaced as diffuse_clamped."),
                apply: ApplyClass::Live,
            },
            Setting {
                id: "render.diffuse.surface_speed_threshold",
                label: "Surface speed onset",
                category: Category::Water,
                default: Value::F32(1.6),
                value: Value::F32(1.6),
                validation: Validation::F32Range { min: 0.0, max: 10.0 },
                tooltip: Some("Minimum water speed at the liquid-air surface before foam starts to form."),
                technical_tooltip: Some("Live threshold (units/s) on cell-centered grid speed at liquid/air interface cells."),
                apply: ApplyClass::Live,
            },
            Setting {
                id: "render.diffuse.surface_speed_gain",
                label: "Surface speed gain",
                category: Category::Water,
                default: Value::F32(0.7),
                value: Value::F32(0.7),
                validation: Validation::F32Range { min: 0.0, max: 4.0 },
                tooltip: Some("How quickly faster surface water makes more foam."),
                technical_tooltip: Some("Live gain converting (speed - onset) into spawn probability for surface foam."),
                apply: ApplyClass::Live,
            },
            Setting {
                id: "render.diffuse.foam_lifetime",
                label: "Foam lifetime",
                category: Category::Water,
                default: Value::F32(1.8),
                value: Value::F32(1.8),
                validation: Validation::F32Range { min: 0.1, max: 8.0 },
                tooltip: Some("How long surface foam flecks persist before fading."),
                technical_tooltip: Some("Live max lifetime (s) for foam; spawned lifetimes are randomized in [0.4x, 1.0x]."),
                apply: ApplyClass::Live,
            },
            Setting {
                id: "render.diffuse.radius",
                label: "Foam radius",
                category: Category::Water,
                default: Value::F32(0.0045),
                value: Value::F32(0.0045),
                validation: Validation::F32Range { min: 0.002, max: 0.03 },
                tooltip: Some("World-space size of each soft foam fleck."),
                technical_tooltip: Some("Live billboard radius (world units) for foam particles."),
                apply: ApplyClass::Live,
            },
            Setting {
                id: "render.diffuse.alpha",
                label: "Foam opacity",
                category: Category::Water,
                default: Value::F32(0.22),
                value: Value::F32(0.22),
                validation: Validation::F32Range { min: 0.0, max: 1.0 },
                tooltip: Some("How opaque the foam reads at full lifetime."),
                technical_tooltip: Some("Live peak opacity; per-particle alpha also fades with normalized age."),
                apply: ApplyClass::Live,
            },
            Setting {
                id: "render.diffuse.random_seed",
                label: "Foam random seed",
                category: Category::Water,
                default: Value::U32(1337),
                value: Value::U32(1337),
                validation: Validation::U32Range { min: 0, max: 65_535 },
                tooltip: Some("Changes the random pattern of where foam is born."),
                technical_tooltip: Some("Live seed mixed into the per-cell, per-frame spawn hash (deterministic; no wall-clock randomness)."),
                apply: ApplyClass::Live,
            },
            Setting {
                id: "dev.detailed_gpu_profiling",
                label: "Detailed GPU profiling",
                category: Category::Diagnostics,
                default: Value::U32(0),
                value: Value::U32(0),
                validation: Validation::U32Range { min: 0, max: 1 },
                tooltip: Some("Splits coarse GPU timings into detailed per-pass profiler sections."),
                technical_tooltip: Some("Reset-class dev toggle. Detailed mode allocates a larger timestamp query set and adds timestamp writes around fine compute passes and CG iterations."),
                apply: ApplyClass::Reset,
            },
        ];
        Self { settings }
    }
}

impl Registry {
    pub fn get(&self, id: &str) -> Option<&Setting> {
        self.settings.iter().find(|s| s.id == id)
    }

    #[allow(dead_code)]
    pub fn iter(&self) -> impl Iterator<Item = &Setting> {
        self.settings.iter()
    }

    /// Validate a candidate value against a setting's bounds. Returns the clamped
    /// value if out of range. (Used by the 1.2 panel and any live edits.)
    #[allow(dead_code)]
    pub fn validate(&self, id: &str, candidate: Value) -> Option<Value> {
        let s = self.get(id)?;
        Some(match (s.validation, candidate) {
            (Validation::U32Range { min, max }, Value::U32(v)) => Value::U32(v.clamp(min, max)),
            (Validation::F32Range { min, max }, Value::F32(v)) => Value::F32(v.clamp(min, max)),
            _ => candidate,
        })
    }

    // --- typed accessors used by the app/sim ---

    pub fn scene_preset(&self) -> u32 {
        self.get("scene.preset").map(|s| s.as_u32()).unwrap_or(0)
    }
    pub fn drop_height(&self) -> f32 {
        self.get("scene.drop_height")
            .map(|s| s.as_f32())
            .unwrap_or(0.72)
    }
    pub fn grid_res_x(&self) -> u32 {
        self.get("grid.res_x").map(|s| s.as_u32()).unwrap_or(64)
    }
    pub fn grid_res_y(&self) -> u32 {
        self.get("grid.res_y").map(|s| s.as_u32()).unwrap_or(64)
    }
    pub fn grid_res_z(&self) -> u32 {
        self.get("grid.res_z").map(|s| s.as_u32()).unwrap_or(64)
    }
    pub fn particle_count(&self) -> u32 {
        self.get("particles.count").map(|s| s.as_u32()).unwrap_or(0)
    }
    pub fn fixed_dt(&self) -> f32 {
        self.get("physics.fixed_dt")
            .map(|s| s.as_f32())
            .unwrap_or(1.0 / 120.0)
    }
    #[allow(dead_code)]
    pub fn gravity(&self) -> f32 {
        self.get("physics.gravity")
            .map(|s| s.as_f32())
            .unwrap_or(-9.81)
    }
    pub fn flip_blend(&self) -> f32 {
        self.get("physics.flip_blend")
            .map(|s| s.as_f32())
            .unwrap_or(0.9)
    }
    pub fn wall_friction(&self) -> f32 {
        self.get("physics.wall_friction")
            .map(|s| s.as_f32())
            .unwrap_or(0.0)
    }
    pub fn rest_density(&self) -> f32 {
        self.get("physics.rest_density")
            .map(|s| s.as_f32())
            .unwrap_or(8.0)
    }
    pub fn volume_stiffness(&self) -> f32 {
        self.get("physics.volume_stiffness")
            .map(|s| s.as_f32())
            .unwrap_or(1.0)
    }
    pub fn drift_clamp(&self) -> f32 {
        self.get("physics.drift_clamp")
            .map(|s| s.as_f32())
            .unwrap_or(0.5)
    }
    pub fn liquid_threshold(&self) -> u32 {
        self.get("classify.liquid_threshold")
            .map(|s| s.as_u32())
            .unwrap_or(1)
    }
    pub fn surface_dilation(&self) -> u32 {
        self.get("classify.surface_dilation")
            .map(|s| s.as_u32())
            .unwrap_or(0)
    }
    pub fn max_substeps(&self) -> u32 {
        self.get("physics.max_substeps")
            .map(|s| s.as_u32())
            .unwrap_or(4)
    }
    pub fn pressure_iterations(&self) -> u32 {
        self.get("solver.pressure_iterations")
            .map(|s| s.as_u32())
            .unwrap_or(40)
    }
    pub fn particle_size(&self) -> f32 {
        self.get("render.particle_size")
            .map(|s| s.as_f32())
            .unwrap_or(1.0)
    }
    pub fn speed_scale(&self) -> f32 {
        self.get("render.speed_scale")
            .map(|s| s.as_f32())
            .unwrap_or(4.0)
    }
    pub fn particle_view(&self) -> u32 {
        self.get("render.particle_view")
            .map(|s| s.as_u32())
            .unwrap_or(0)
    }
    pub fn whitewater_strength(&self) -> f32 {
        self.get("render.whitewater_strength")
            .map(|s| s.as_f32())
            .unwrap_or(0.65)
    }
    pub fn whitewater_threshold(&self) -> f32 {
        self.get("render.whitewater_threshold")
            .map(|s| s.as_f32())
            .unwrap_or(0.38)
    }
    pub fn whitewater_softness(&self) -> f32 {
        self.get("render.whitewater_softness")
            .map(|s| s.as_f32())
            .unwrap_or(0.22)
    }
    pub fn fps_target(&self) -> u32 {
        self.get("render.fps_target")
            .map(|s| s.as_u32())
            .unwrap_or(60)
    }
    pub fn camera_rot_x(&self) -> f32 {
        self.get("camera.rot_x").map(|s| s.as_f32()).unwrap_or(-0.2)
    }
    pub fn camera_rot_y(&self) -> f32 {
        self.get("camera.rot_y").map(|s| s.as_f32()).unwrap_or(0.6)
    }
    pub fn camera_rot_z(&self) -> f32 {
        self.get("camera.rot_z").map(|s| s.as_f32()).unwrap_or(0.0)
    }
    pub fn camera_distance(&self) -> f32 {
        self.get("camera.distance")
            .map(|s| s.as_f32())
            .unwrap_or(6.0)
    }
    pub fn cfl(&self) -> f32 {
        self.get("physics.cfl").map(|s| s.as_f32()).unwrap_or(2.0)
    }
    pub fn auto_roll_enabled(&self) -> bool {
        self.get("interaction.auto_roll_enabled")
            .map(|s| s.as_u32() != 0)
            .unwrap_or(false)
    }
    pub fn auto_roll_strength(&self) -> f32 {
        self.get("interaction.auto_roll_strength")
            .map(|s| s.as_f32())
            .unwrap_or(0.45)
    }
    pub fn auto_roll_cadence(&self) -> f32 {
        self.get("interaction.auto_roll_cadence")
            .map(|s| s.as_f32())
            .unwrap_or(2.5)
    }
    pub fn wave_enabled(&self) -> bool {
        self.get("interaction.wave_enabled")
            .map(|s| s.as_u32() != 0)
            .unwrap_or(false)
    }
    pub fn wave_strength(&self) -> f32 {
        self.get("interaction.wave_strength")
            .map(|s| s.as_f32())
            .unwrap_or(0.5)
    }
    pub fn wave_frequency(&self) -> f32 {
        self.get("interaction.wave_frequency")
            .map(|s| s.as_f32())
            .unwrap_or(0.75)
    }
    pub fn particle_slow_color(&self) -> [f32; 3] {
        unpack_rgb(
            self.get("render.particle_slow_color")
                .map(|s| s.as_u32())
                .unwrap_or(0x194CCC),
        )
    }
    pub fn particle_fast_color(&self) -> [f32; 3] {
        unpack_rgb(
            self.get("render.particle_fast_color")
                .map(|s| s.as_u32())
                .unwrap_or(0xB2EBFF),
        )
    }
    pub fn water_optical_density(&self) -> f32 {
        self.get("render.water_optical_density")
            .map(|s| s.as_f32())
            .unwrap_or(1.25)
    }
    pub fn particle_shading(&self) -> f32 {
        self.get("render.particle_shading")
            .map(|s| s.as_f32())
            .unwrap_or(0.25)
    }
    pub fn particle_edge(&self) -> f32 {
        self.get("render.particle_edge")
            .map(|s| s.as_f32())
            .unwrap_or(0.6)
    }
    pub fn detailed_gpu_profiling(&self) -> bool {
        self.get("dev.detailed_gpu_profiling")
            .map(|s| s.as_u32() != 0)
            .unwrap_or(false)
    }

    fn f32_or(&self, id: &str, default: f32) -> f32 {
        self.get(id).map(|s| s.as_f32()).unwrap_or(default)
    }
    fn u32_or(&self, id: &str, default: u32) -> u32 {
        self.get(id).map(|s| s.as_u32()).unwrap_or(default)
    }

    /// Build a flat snapshot of all Water-tab (hero-water) settings. The renderer
    /// mirrors this into the composite uniform whenever a `render.hero.*` slider
    /// changes.
    pub fn hero_params(&self) -> HeroParams {
        HeroParams {
            refraction_enabled: self.u32_or("render.hero.refraction_enabled", 1) != 0,
            reflection_enabled: self.u32_or("render.hero.reflection_enabled", 1) != 0,
            body_color_enabled: self.u32_or("render.hero.body_color_enabled", 1) != 0,
            wall_contact_enabled: self.u32_or("render.hero.wall_contact_enabled", 1) != 0,
            debug_view: self.u32_or("render.hero.debug_view", 0),
            ior: self.f32_or("render.hero.ior", 1.33),
            refraction_strength: self.f32_or("render.hero.refraction_strength", 0.6),
            refraction_thickness_scale: self.f32_or("render.hero.refraction_thickness_scale", 1.0),
            refraction_max_offset_px: self.u32_or("render.hero.refraction_max_offset_px", 48)
                as f32,
            invalid_refraction_fallback: self.u32_or("render.hero.invalid_refraction_fallback", 0),
            absorption_color: unpack_rgb(self.u32_or("render.hero.absorption_color", 0x3366_80)),
            absorption_strength: self.f32_or("render.hero.absorption_strength", 2.4),
            base_tint: unpack_rgb(self.u32_or("render.hero.base_tint", 0x1B6F_A6)),
            transparency: self.f32_or("render.hero.transparency", 0.18),
            deep_water_darkening: self.f32_or("render.hero.deep_water_darkening", 2.4),
            floor_pattern_scale: self.f32_or("render.hero.floor_pattern_scale", 8.0),
            floor_pattern_strength: self.f32_or("render.hero.floor_pattern_strength", 0.5),
            backdrop_strength: self.f32_or("render.hero.backdrop_strength", 0.6),
            wall_visibility: self.f32_or("render.hero.wall_visibility", 0.3),
            reflection_strength: self.f32_or("render.hero.reflection_strength", 0.8),
            environment_strength: self.f32_or("render.hero.environment_strength", 1.0),
            environment_mode: self.u32_or("render.hero.environment_mode", 0),
            environment_rotation: self.f32_or("render.hero.environment_rotation", 0.0),
            environment_brightness: self.f32_or("render.hero.environment_brightness", 1.0),
            skybox_enabled: self.u32_or("render.hero.skybox_enabled", 1) != 0,
            roughness_base: self.f32_or("render.hero.roughness_base", 0.12),
            roughness_velocity_scale: self.f32_or("render.hero.roughness_velocity_scale", 0.6),
            roughness_normal_variance_scale: self
                .f32_or("render.hero.roughness_normal_variance_scale", 0.5),
            roughness_foam_scale: self.f32_or("render.hero.roughness_foam_scale", 0.8),
            specular_strength: self.f32_or("render.hero.specular_strength", 1.0),
            sun_direction: [
                self.f32_or("render.hero.sun_dir_x", 0.4),
                self.f32_or("render.hero.sun_dir_y", 0.7),
                self.f32_or("render.hero.sun_dir_z", 0.5),
            ],
            sun_intensity: self.f32_or("render.hero.sun_intensity", 1.2),
            micro_normal_enabled: self.u32_or("render.hero.micro_normal_enabled", 0) != 0,
            micro_normal_strength: self.f32_or("render.hero.micro_normal_strength", 0.15),
            micro_normal_scale: self.f32_or("render.hero.micro_normal_scale", 60.0),
            micro_normal_velocity_scale: self
                .f32_or("render.hero.micro_normal_velocity_scale", 1.0),
            // --- Screen-space surface quality (v1.19 polish) ---
            smooth_iterations: self.u32_or("render.hero.smooth_iterations", 2),
            smooth_radius: self.u32_or("render.hero.smooth_radius", 5),
            smooth_thickness_splat_scale: self
                .f32_or("render.hero.smooth_thickness_splat_scale", 1.8),
            normal_stencil: self.u32_or("render.hero.normal_stencil", 2),
            normal_smooth_strength: self.f32_or("render.hero.normal_smooth_strength", 0.5),
            // --- Flat-water wall-contact snap ---
            flat_water_strength: self.f32_or("render.hero.flat_water.strength", 0.8),
            flat_water_epsilon: self.f32_or("render.hero.flat_water.epsilon", 0.04),
            flat_water_depth_strength: self.f32_or("render.hero.flat_water.depth_strength", 0.8),
        }
    }

    /// Build a flat snapshot of all surface-foam settings. The renderer mirrors
    /// this into the diffuse uniform whenever a `render.diffuse.*` slider changes.
    pub fn diffuse_params(&self) -> DiffuseParams {
        DiffuseParams {
            enabled: self.u32_or("render.diffuse.enabled", 1) != 0,
            max_particles: self.u32_or("render.diffuse.max_particles", 64_000),
            emit_rate: self.f32_or("render.diffuse.emit_rate", 0.85),
            emit_budget_per_frame: self.u32_or("render.diffuse.emit_budget_per_frame", 1_500),
            surface_speed_threshold: self.f32_or("render.diffuse.surface_speed_threshold", 1.6),
            surface_speed_gain: self.f32_or("render.diffuse.surface_speed_gain", 0.7),
            foam_lifetime: self.f32_or("render.diffuse.foam_lifetime", 1.8),
            radius: self.f32_or("render.diffuse.radius", 0.0045),
            alpha: self.f32_or("render.diffuse.alpha", 0.22),
            random_seed: self.u32_or("render.diffuse.random_seed", 1337),
        }
    }

    /// Set a setting value by id, clamping to its validation range.
    /// Preserves the Value variant (U32 → round, F32 → as f32).
    /// Returns true if the id was found (regardless of clamping).
    pub fn set_value_f64(&mut self, id: &str, v: f64) -> bool {
        if !v.is_finite() {
            return false;
        }
        if id == "render.hero.mode_enabled" {
            let mapped = if v == 0.0 { 0.0 } else { 1.0 };
            let mut accepted = false;
            for mapped_id in [
                "render.hero.refraction_enabled",
                "render.hero.reflection_enabled",
                "render.hero.body_color_enabled",
            ] {
                accepted |= self.set_value_f64(mapped_id, mapped);
            }
            return accepted;
        }
        if legacy_hidden_setting_id(id) {
            return true;
        }

        let idx = match self.settings.iter().position(|s| s.id == id) {
            Some(i) => i,
            None => return false,
        };
        let s = &self.settings[idx];
        let new_value = match (s.validation, s.value) {
            (Validation::U32Range { min, max }, Value::U32(_)) => {
                Value::U32((v.round() as u32).clamp(min, max))
            }
            (Validation::F32Range { min, max }, Value::F32(_)) => {
                Value::F32((v as f32).clamp(min, max))
            }
            (Validation::None, Value::U32(_)) => Value::U32(v.round() as u32),
            (Validation::None, Value::F32(_)) => Value::F32(v as f32),
            // Mismatched variant: clamp as the existing variant type.
            (Validation::U32Range { min, max }, Value::F32(_)) => {
                Value::F32((v as f32).clamp(min as f32, max as f32))
            }
            (Validation::F32Range { min, max }, Value::U32(_)) => {
                Value::U32((v.round() as u32).clamp(min as u32, max as u32))
            }
        };
        self.settings[idx].value = new_value;
        true
    }

    /// Return the apply class string for a setting id, or None if not found.
    pub fn apply_class_str(&self, id: &str) -> Option<&'static str> {
        self.get(id).map(|s| s.apply.as_str())
    }

    /// Serialize all settings to a JSON array string.
    /// Shape: [{"id":...,"label":...,"category":...,"tab":...,"tab_label":...,
    ///          "type":...,"value":<num>,"default":<num>,"min":<num>,
    ///          "max":<num>,"apply":...}, ...]
    /// Optional help fields are emitted only when present.
    pub fn config_json(&self) -> String {
        let mut out = String::from("[");
        for (i, s) in self.settings.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push('{');
            out.push_str(&format!(r#""id":{}"#, json_quote(s.id)));
            out.push_str(&format!(r#","label":{}"#, json_quote(s.label)));
            out.push_str(&format!(
                r#","category":{}"#,
                json_quote(s.category.as_str())
            ));
            let tab = settings_tab(s);
            out.push_str(&format!(r#","tab":{}"#, json_quote(tab.as_str())));
            out.push_str(&format!(r#","tab_label":{}"#, json_quote(tab.label())));
            out.push_str(&format!(r#","tab_order":{}"#, tab.order()));
            out.push_str(&format!(r#","tab_group":{}"#, json_quote(tab.group())));
            out.push_str(&format!(r#","tab_variant":{}"#, json_quote(tab.variant())));
            out.push_str(&format!(r#","type":{}"#, json_quote(s.type_str())));
            out.push_str(&format!(r#","value":{}"#, fmt_f64(s.value_as_f64())));
            out.push_str(&format!(r#","default":{}"#, fmt_f64(s.default_as_f64())));
            out.push_str(&format!(r#","min":{}"#, fmt_f64(s.min_as_f64())));
            out.push_str(&format!(r#","max":{}"#, fmt_f64(s.max_as_f64())));
            out.push_str(&format!(r#","apply":{}"#, json_quote(s.apply.as_str())));
            if let Some(tooltip) = s.tooltip {
                out.push_str(&format!(r#","tooltip":{}"#, json_quote(tooltip)));
            }
            if let Some(tooltip) = s.technical_tooltip {
                out.push_str(&format!(r#","technical_tooltip":{}"#, json_quote(tooltip)));
            }
            // Optional non-linear slider scale (e.g. "log2": each notch doubles
            // the value). The number input still spans the full [min, max].
            if let Some(scale) = slider_scale(s.id) {
                out.push_str(&format!(r#","slider_scale":{}"#, json_quote(scale)));
            }
            // Enum-valued settings carry a list of option labels so the panel can
            // render a dropdown instead of a slider. The value is the option index.
            if let Some(opts) = enum_options(s.id) {
                out.push_str(r#","options":["#);
                for (j, label) in opts.iter().enumerate() {
                    if j > 0 {
                        out.push(',');
                    }
                    out.push_str(&json_quote(label));
                }
                out.push(']');
            }
            out.push('}');
        }
        out.push(']');
        out
    }
}

fn legacy_hidden_setting_id(id: &str) -> bool {
    id.starts_with("render.hero.caustics.")
        || id.starts_with("render.hero.temporal.")
        || id.starts_with("render.hero.wet_wall.")
        || matches!(
            id,
            "render.hero.flat_water.fill_enabled"
                | "render.hero.flat_water.fill_strength"
                | "render.hero.flat_water.fill_slab"
                | "render.hero.flat_water.fill_supersample"
                | "render.hero.flat_water.fill_color_strength"
                | "render.hero.flat_water.fill_reflection_strength"
                | "render.hero.flat_water.fill_roughness"
                | "render.hero.flat_water.fill_absorption_strength"
                | "render.hero.flat_water.waterline_softness"
                | "render.diffuse.wall_impact_threshold"
                | "render.diffuse.wall_impact_gain"
                | "render.diffuse.spray_lifetime"
                | "render.diffuse.bubble_lifetime"
                | "render.diffuse.bubble_buoyancy"
                | "render.diffuse.spray_drag"
                | "render.diffuse.debug_view"
        )
}

fn settings_tab(setting: &Setting) -> SettingsTab {
    let id = setting.id;
    if id.starts_with("scene.") || id.starts_with("grid.") || id == "particles.count" {
        return SettingsTab::Scenario;
    }
    if id.starts_with("interaction.") {
        return SettingsTab::Modes;
    }
    if id.starts_with("camera.") || id == "render.particle_view" || id == "render.fps_target" {
        return SettingsTab::CameraView;
    }
    if id.starts_with("render.diffuse.") {
        return SettingsTab::Foam;
    }
    if matches!(
        id,
        "render.hero.floor_pattern_scale"
            | "render.hero.floor_pattern_strength"
            | "render.hero.backdrop_strength"
            | "render.hero.wall_visibility"
            | "render.hero.environment_strength"
            | "render.hero.environment_mode"
            | "render.hero.environment_rotation"
            | "render.hero.environment_brightness"
            | "render.hero.skybox_enabled"
    ) {
        return SettingsTab::Environment;
    }
    if matches!(
        id,
        "render.hero.reflection_enabled"
            | "render.hero.reflection_strength"
            | "render.hero.roughness_base"
            | "render.hero.roughness_velocity_scale"
            | "render.hero.roughness_normal_variance_scale"
            | "render.hero.roughness_foam_scale"
            | "render.hero.specular_strength"
            | "render.hero.sun_dir_x"
            | "render.hero.sun_dir_y"
            | "render.hero.sun_dir_z"
            | "render.hero.sun_intensity"
            | "render.hero.micro_normal_enabled"
            | "render.hero.micro_normal_strength"
            | "render.hero.micro_normal_scale"
            | "render.hero.micro_normal_velocity_scale"
    ) {
        return SettingsTab::SunReflection;
    }
    if matches!(
        id,
        "render.speed_scale"
            | "render.particle_slow_color"
            | "render.particle_fast_color"
            | "render.water_optical_density"
            | "render.whitewater_strength"
            | "render.whitewater_threshold"
            | "render.whitewater_softness"
            | "render.hero.body_color_enabled"
            | "render.hero.absorption_color"
            | "render.hero.absorption_strength"
            | "render.hero.base_tint"
            | "render.hero.transparency"
            | "render.hero.deep_water_darkening"
    ) {
        return SettingsTab::WaterColor;
    }
    if id.starts_with("render.hero.")
        || matches!(
            id,
            "render.particle_size" | "render.particle_edge" | "render.particle_shading"
        )
    {
        return SettingsTab::WaterSurface;
    }
    if id.starts_with("physics.")
        || id.starts_with("classify.")
        || id.starts_with("solver.")
        || id == "dev.detailed_gpu_profiling"
    {
        return SettingsTab::Simulation;
    }
    SettingsTab::Simulation
}

/// Option labels for enum-valued settings (index = stored u32 value). Returns
/// `None` for ordinary numeric settings. Add an arm here to make a setting render
/// as a dropdown in the config panel.
fn enum_options(id: &str) -> Option<&'static [&'static str]> {
    match id {
        "scene.preset" => Some(&["Falling blob", "Dam break", "Double splash"]),
        "render.particle_view" => Some(&[
            "Screen-space water",
            "Optical particles",
            "Simple particles",
        ]),
        "render.hero.refraction_enabled"
        | "render.hero.reflection_enabled"
        | "render.hero.body_color_enabled"
        | "render.hero.wall_contact_enabled" => Some(&["Disabled", "Enabled"]),
        "render.hero.invalid_refraction_fallback" => Some(&["Unrefracted", "Base tint"]),
        "render.hero.skybox_enabled" => Some(&["Disabled", "Enabled"]),
        "render.hero.micro_normal_enabled" => Some(&["Disabled", "Enabled"]),
        "render.hero.environment_mode" => Some(&["Sky", "Room", "Studio"]),
        "render.diffuse.enabled" => Some(&["Disabled", "Enabled"]),
        "render.hero.debug_view" => Some(&[
            "Off",
            "Scene color",
            "Scene depth",
            "Thickness",
            "Refraction UV offset",
            "Fresnel",
            "Absorption",
            "Final water only",
            "Reflection",
            "Env only",
            "Nearest Z",
            "Whitewater",
        ]),
        _ => None,
    }
}

/// Optional non-linear scale for a setting's *slider*. `"log2"` makes each slider
/// notch double the value (powers of two from `min` to `max`), keeping a slider
/// usable across a huge range; the number input still accepts any exact value in
/// `[min, max]`. Both `min` and `max` should be powers of two for clean stepping
/// (`particles.count` runs 2^10 .. 2^27). Returns `None` for linear sliders.
fn slider_scale(id: &str) -> Option<&'static str> {
    match id {
        "particles.count" => Some("log2"),
        "render.particle_slow_color"
        | "render.particle_fast_color"
        | "render.hero.absorption_color"
        | "render.hero.base_tint" => Some("color"),
        _ => None,
    }
}

/// Decode a packed 0x00RRGGBB u32 to linear [0,1] RGB floats.
fn unpack_rgb(c: u32) -> [f32; 3] {
    [
        ((c >> 16) & 0xFF) as f32 / 255.0,
        ((c >> 8) & 0xFF) as f32 / 255.0,
        (c & 0xFF) as f32 / 255.0,
    ]
}

/// Escape a string for JSON: wraps in double quotes, escapes `"` and `\`.
fn json_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Format a f64 for JSON: use integer form if it is a whole number, otherwise
/// 6 significant decimal places with trailing zeros stripped.
fn fmt_f64(v: f64) -> String {
    debug_assert!(
        v.is_finite(),
        "settings JSON cannot encode non-finite numbers"
    );
    if v.fract() == 0.0 && v.abs() < 1.0e15 {
        format!("{}", v as i64)
    } else {
        // Up to 6 decimal places, trailing zeros stripped.
        let s = format!("{:.6}", v);
        let s = s.trim_end_matches('0');
        let s = s.trim_end_matches('.');
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_setting(
        id: &'static str,
        tooltip: Option<&'static str>,
        technical_tooltip: Option<&'static str>,
    ) -> Setting {
        Setting {
            id,
            label: "Test setting",
            category: Category::Diagnostics,
            default: Value::U32(1),
            value: Value::U32(1),
            validation: Validation::U32Range { min: 0, max: 2 },
            tooltip,
            technical_tooltip,
            apply: ApplyClass::Live,
        }
    }

    fn json_for(setting: Setting) -> String {
        Registry {
            settings: vec![setting],
        }
        .config_json()
    }

    #[test]
    fn config_json_omits_help_fields_when_absent() {
        let json = json_for(test_setting("test.no_help", None, None));

        assert!(json.contains(r#""tab":"simulation""#));
        assert!(json.contains(r#""tab_label":"Simulation""#));
        assert!(!json.contains(r#""tooltip""#));
        assert!(!json.contains(r#""technical_tooltip""#));
    }

    #[test]
    fn config_json_emits_functional_help_without_technical_help() {
        let json = json_for(test_setting(
            "test.functional_help",
            Some("Functional help"),
            None,
        ));

        assert!(json.contains(r#""tab_order":20"#));
        assert!(json.contains(r#""tooltip":"Functional help""#));
        assert!(!json.contains(r#""technical_tooltip""#));
    }

    #[test]
    fn config_json_emits_two_help_fields() {
        let json = json_for(test_setting(
            "test.two_help_fields",
            Some("Functional help"),
            Some("Technical help"),
        ));

        assert!(json.contains(r#""category":"Diagnostics""#));
        assert!(json.contains(r#""tooltip":"Functional help""#));
        assert!(json.contains(r#""technical_tooltip":"Technical help""#));
    }

    #[test]
    fn non_finite_setting_values_are_rejected() {
        let mut registry = Registry::default();
        let before = registry.flip_blend();

        assert!(!registry.set_value_f64("physics.flip_blend", f64::NAN));
        assert!(!registry.set_value_f64("physics.flip_blend", f64::INFINITY));
        assert_eq!(registry.flip_blend(), before);

        let json = registry.config_json();
        assert!(!json.contains("NaN"));
        assert!(!json.contains("inf"));
    }

    #[test]
    fn finite_out_of_range_setting_values_are_clamped() {
        let mut registry = Registry::default();

        assert!(registry.set_value_f64("physics.cfl", 1.0e100));
        assert_eq!(registry.f32_or("physics.cfl", 0.0), 6.0);

        assert!(registry.set_value_f64("solver.pressure_iterations", 1.0e100));
        assert_eq!(registry.u32_or("solver.pressure_iterations", 0), 200);
    }

    #[test]
    fn interaction_settings_are_live_default_controls() {
        let registry = Registry::default();
        let json = registry.config_json();
        let ids = [
            "interaction.auto_roll_enabled",
            "interaction.auto_roll_strength",
            "interaction.auto_roll_cadence",
            "interaction.wave_enabled",
            "interaction.wave_strength",
            "interaction.wave_frequency",
        ];

        for id in ids {
            let setting = registry.get(id).expect("missing interaction setting");
            assert_eq!(setting.category, Category::Interaction);
            assert_eq!(setting.apply, ApplyClass::Live);
            assert!(json.contains(&format!(r#""id":"{id}""#)));
        }

        assert!(json.contains(r#""tab":"modes""#));
        assert!(!registry.auto_roll_enabled());
        assert!(!registry.wave_enabled());
        assert!((registry.auto_roll_strength() - 0.22).abs() < f32::EPSILON);
        assert!((registry.wave_strength() - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn reset_class_settings_stay_reset_class() {
        let registry = Registry::default();
        for id in [
            "scene.preset",
            "scene.drop_height",
            "grid.res_x",
            "grid.res_y",
            "grid.res_z",
            "particles.count",
            "physics.fixed_dt",
            "physics.max_substeps",
            "dev.detailed_gpu_profiling",
        ] {
            let setting = registry.get(id).unwrap_or_else(|| panic!("missing {id}"));
            assert_eq!(
                setting.apply,
                ApplyClass::Reset,
                "{id} must remain Reset-class"
            );
        }
    }

    #[test]
    fn water_optical_density_replaces_particle_opacity_control() {
        let registry = Registry::default();
        let setting = registry
            .get("render.water_optical_density")
            .expect("missing water optical density setting");
        let view = registry
            .get("render.particle_view")
            .expect("missing particle view setting");

        assert_eq!(setting.category, Category::Render);
        assert_eq!(setting.apply, ApplyClass::Live);
        assert_eq!(view.category, Category::Render);
        assert_eq!(view.apply, ApplyClass::Live);
        assert!(matches!(
            setting.validation,
            Validation::F32Range { min: 0.0, max: 8.0 }
        ));
        assert!((registry.water_optical_density() - 1.25).abs() < f32::EPSILON);
        assert_eq!(registry.particle_view(), 0);
        assert!((registry.particle_shading() - 0.25).abs() < f32::EPSILON);
        assert!((registry.whitewater_strength() - 0.65).abs() < f32::EPSILON);
        assert!((registry.whitewater_threshold() - 0.38).abs() < f32::EPSILON);
        assert!((registry.whitewater_softness() - 0.22).abs() < f32::EPSILON);

        let json = registry.config_json();
        assert!(json.contains(r#""id":"render.water_optical_density""#));
        assert!(json.contains(r#""id":"render.particle_view""#));
        assert!(json.contains(r#""id":"render.whitewater_strength""#));
        assert!(json.contains(
            r#""options":["Screen-space water","Optical particles","Simple particles"]"#
        ));
        assert!(!json.contains(r#""id":"render.particle_alpha""#));
        assert!(!json.contains("Particle opacity"));
    }

    #[test]
    fn hero_water_settings_are_live_water_tab_controls() {
        let registry = Registry::default();
        let json = registry.config_json();
        let ids = [
            "render.hero.refraction_enabled",
            "render.hero.reflection_enabled",
            "render.hero.body_color_enabled",
            "render.hero.wall_contact_enabled",
            "render.hero.debug_view",
            "render.hero.ior",
            "render.hero.refraction_strength",
            "render.hero.refraction_thickness_scale",
            "render.hero.refraction_max_offset_px",
            "render.hero.invalid_refraction_fallback",
            "render.hero.absorption_color",
            "render.hero.absorption_strength",
            "render.hero.base_tint",
            "render.hero.transparency",
            "render.hero.deep_water_darkening",
            "render.hero.floor_pattern_scale",
            "render.hero.floor_pattern_strength",
            "render.hero.backdrop_strength",
            "render.hero.wall_visibility",
            "render.hero.reflection_strength",
            "render.hero.environment_strength",
            "render.hero.environment_mode",
            "render.hero.environment_rotation",
            "render.hero.environment_brightness",
            "render.hero.skybox_enabled",
            "render.hero.roughness_base",
            "render.hero.roughness_velocity_scale",
            "render.hero.roughness_normal_variance_scale",
            "render.hero.roughness_foam_scale",
            "render.hero.specular_strength",
            "render.hero.sun_dir_x",
            "render.hero.sun_dir_y",
            "render.hero.sun_dir_z",
            "render.hero.sun_intensity",
            "render.hero.micro_normal_enabled",
            "render.hero.micro_normal_strength",
            "render.hero.micro_normal_scale",
            "render.hero.micro_normal_velocity_scale",
            // --- v1.19 surface quality ---
            "render.hero.smooth_iterations",
            "render.hero.smooth_radius",
            "render.hero.smooth_thickness_splat_scale",
            "render.hero.normal_stencil",
            "render.hero.normal_smooth_strength",
            // --- Flat-water wall-contact snap ---
            "render.hero.flat_water.strength",
            "render.hero.flat_water.epsilon",
            "render.hero.flat_water.depth_strength",
        ];
        for id in ids {
            let s = registry.get(id).unwrap_or_else(|| panic!("missing {id}"));
            assert_eq!(s.category, Category::Water, "{id} must be in the Water tab");
            assert_eq!(s.apply, ApplyClass::Live, "{id} must be Live");
            assert!(json.contains(&format!(r#""id":"{id}""#)));
        }
        // Color + enum metadata is emitted for the right ids.
        assert!(json.contains(r#""options":["Disabled","Enabled"]"#));
        assert!(json.contains(r#""options":["Sky","Room","Studio"]"#));
        assert!(json.contains(
            r#""options":["Off","Scene color","Scene depth","Thickness","Refraction UV offset","Fresnel","Absorption","Final water only","Reflection","Env only","Nearest Z","Whitewater"]"#
        ));
        assert!(!json.contains(r#""id":"render.hero.caustics.enabled""#));
        assert!(!json.contains(r#""id":"render.hero.wet_wall.enabled""#));
        assert!(!json.contains(r#""id":"render.hero.temporal.enabled""#));
        assert!(!json.contains(r#""id":"render.hero.flat_water.fill_enabled""#));

        // hero_params() reads the registry defaults and derives nothing nonsensical.
        let hero = registry.hero_params();
        assert!(hero.refraction_enabled);
        assert!(hero.reflection_enabled);
        assert!(hero.body_color_enabled);
        assert!(hero.wall_contact_enabled);
        assert_eq!(hero.debug_view, 0);
        assert!((hero.ior - 1.33).abs() < 1e-6);
        assert!((hero.refraction_max_offset_px - 48.0).abs() < 1e-6);
        assert_eq!(hero.invalid_refraction_fallback, 0);
        assert!((hero.absorption_strength - 2.4).abs() < 1e-5);
        assert!((hero.transparency - 0.18).abs() < 1e-5);
        assert!((hero.deep_water_darkening - 2.4).abs() < 1e-5);
        // v1.19 surface quality defaults
        assert_eq!(hero.smooth_iterations, 2, "smooth_iterations default 2");
        assert_eq!(hero.smooth_radius, 5, "smooth_radius default 5");
        assert!(
            (hero.smooth_thickness_splat_scale - 1.8).abs() < 1e-5,
            "splat_scale default 1.8"
        );
        assert_eq!(hero.normal_stencil, 2, "normal_stencil default 2");
        assert!(
            (hero.normal_smooth_strength - 0.5).abs() < 1e-5,
            "normal_smooth_strength default 0.5"
        );
        assert!(
            (hero.flat_water_strength - 0.8).abs() < 1e-5,
            "flat_water_strength default 0.8"
        );
        assert!(
            (hero.flat_water_epsilon - 0.04).abs() < 1e-5,
            "flat_water_epsilon default 0.04"
        );
        assert!(
            (hero.flat_water_depth_strength - 1.0).abs() < 1e-5,
            "flat_water_depth_strength default 1.0"
        );
    }

    #[test]
    fn legacy_hero_mode_maps_only_core_optical_toggles() {
        let mut registry = Registry::default();
        assert!(registry.set_value_f64("render.hero.wall_contact_enabled", 0.0));

        assert!(registry.set_value_f64("render.hero.mode_enabled", 0.0));
        let hero = registry.hero_params();
        assert!(!hero.refraction_enabled);
        assert!(!hero.reflection_enabled);
        assert!(!hero.body_color_enabled);
        assert!(!hero.wall_contact_enabled);

        assert!(registry.set_value_f64("render.hero.mode_enabled", 7.0));
        let hero = registry.hero_params();
        assert!(hero.refraction_enabled);
        assert!(hero.reflection_enabled);
        assert!(hero.body_color_enabled);
        assert!(!hero.wall_contact_enabled);
    }

    #[test]
    fn legacy_hidden_hero_settings_are_accepted_but_not_visible() {
        let mut registry = Registry::default();
        let json = registry.config_json();
        for id in [
            "render.hero.mode_enabled",
            "render.hero.caustics.enabled",
            "render.hero.caustics.intensity",
            "render.hero.caustics.focus_strength",
            "render.hero.caustics.thickness_scale",
            "render.hero.caustics.floor_enabled",
            "render.hero.caustics.back_wall_enabled",
            "render.hero.caustics.side_walls_enabled",
            "render.hero.caustics.motion_scale",
            "render.hero.caustics.max_intensity",
            "render.hero.caustics.mode",
            "render.hero.caustics.resolution_scale",
            "render.hero.caustics.blur_radius",
            "render.hero.caustics.temporal_enabled",
            "render.hero.caustics.temporal_alpha",
            "render.hero.temporal.enabled",
            "render.hero.temporal.thickness_history",
            "render.hero.temporal.normal_history",
            "render.hero.temporal.caustic_history",
            "render.hero.temporal.foam_history",
            "render.hero.temporal.history_alpha",
            "render.hero.temporal.camera_motion_reset_threshold",
            "render.hero.temporal.depth_reject_threshold",
            "render.hero.temporal.normal_reject_threshold",
            "render.hero.temporal.jitter_enabled",
            "render.hero.wet_wall.enabled",
            "render.hero.wet_wall.wetness_decay",
            "render.hero.wet_wall.wetness_contact_gain",
            "render.hero.wet_wall.wetness_spray_gain",
            "render.hero.wet_wall.darkening_strength",
            "render.hero.wet_wall.gloss_strength",
            "render.hero.wet_wall.streak_strength",
            "render.hero.wet_wall.meniscus_enabled",
            "render.hero.wet_wall.meniscus_width",
            "render.hero.wet_wall.meniscus_strength",
            "render.hero.wet_wall.meniscus_fresnel_boost",
            "render.hero.wet_wall.contact_shadow_enabled",
            "render.hero.wet_wall.contact_shadow_strength",
            "render.hero.wet_wall.contact_shadow_radius",
            "render.hero.wet_wall.debug_view",
            "render.hero.wet_wall.reflectivity",
            "render.hero.wet_wall.specular",
            "render.hero.wet_wall.blur",
            "render.hero.flat_water.fill_enabled",
            "render.hero.flat_water.fill_strength",
            "render.hero.flat_water.fill_slab",
            "render.hero.flat_water.fill_supersample",
            "render.hero.flat_water.fill_color_strength",
            "render.hero.flat_water.fill_reflection_strength",
            "render.hero.flat_water.fill_roughness",
            "render.hero.flat_water.fill_absorption_strength",
            "render.hero.flat_water.waterline_softness",
            "render.diffuse.wall_impact_threshold",
            "render.diffuse.wall_impact_gain",
            "render.diffuse.spray_lifetime",
            "render.diffuse.bubble_lifetime",
            "render.diffuse.bubble_buoyancy",
            "render.diffuse.spray_drag",
            "render.diffuse.debug_view",
        ] {
            assert!(registry.set_value_f64(id, 1.0), "{id} should replay safely");
            assert!(
                !json.contains(&format!(r#""id":"{id}""#)),
                "{id} should be hidden"
            );
        }
    }
}
