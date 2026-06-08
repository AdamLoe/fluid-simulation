//! Typed config-registry.
//!
//! Per `decisions.md` (configuration flows through a schema-driven registry) and
//! the observability split, this file is the authoritative source for each setting's
//! id, label, semantic category, panel group, type, default, validation, optional
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
    Dev,
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
            Category::Dev => "Dev",
        }
    }
}

/// Which top-level panel tier a setting renders in. `category` remains the
/// semantic section inside the tier.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PanelGroup {
    Default,
    Advanced,
    Dev,
}

impl PanelGroup {
    pub fn as_str(self) -> &'static str {
        match self {
            PanelGroup::Default => "default",
            PanelGroup::Advanced => "advanced",
            PanelGroup::Dev => "dev",
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
    pub panel_group: PanelGroup,
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
    pub mode_enabled: bool,
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
                panel_group: PanelGroup::Default,
                default: Value::U32(0),
                value: Value::U32(0),
                validation: Validation::U32Range { min: 0, max: 2 },
                tooltip: Some("Chooses the starting setup and resets the sim into that scenario."),
                technical_tooltip: Some("Reset-class enum. The stored value is the option index; the web panel calls reset after changing it so scene buffers are rebuilt immediately."),
                apply: ApplyClass::Reset,
            },
            Setting {
                id: "grid.res_x",
                label: "Grid resolution X",
                category: Category::Grid,
                panel_group: PanelGroup::Default,
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
                panel_group: PanelGroup::Default,
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
                panel_group: PanelGroup::Default,
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
                panel_group: PanelGroup::Default,
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
                panel_group: PanelGroup::Default,
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
                panel_group: PanelGroup::Advanced,
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
                panel_group: PanelGroup::Default,
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
                panel_group: PanelGroup::Dev,
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
                panel_group: PanelGroup::Default,
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
                panel_group: PanelGroup::Advanced,
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
                panel_group: PanelGroup::Advanced,
                default: Value::F32(1.0),
                value: Value::F32(1.0),
                validation: Validation::F32Range { min: 0.0, max: 4.0 },
                tooltip: Some("Controls how strongly crowded regions are pushed apart."),
                technical_tooltip: Some("Live anti-clump stiffness. 0 disables the occupancy-driven divergence bias; higher values push over-dense cells toward Rest particles/cell."),
                apply: ApplyClass::Live,
            },
            Setting {
                id: "physics.drift_clamp",
                label: "Drift clamp",
                category: Category::Physics,
                panel_group: PanelGroup::Advanced,
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
                panel_group: PanelGroup::Default,
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
                panel_group: PanelGroup::Default,
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
                panel_group: PanelGroup::Default,
                default: Value::F32(0.45),
                value: Value::F32(0.45),
                validation: Validation::F32Range { min: 0.0, max: 1.2 },
                tooltip: Some("Sets the maximum random tank tilt; lower values make a gentler rocking motion."),
                technical_tooltip: Some("Live maximum target-pose tilt in radians. The scheduler uses deterministic PRNG targets and smooth interpolation rather than unbounded spin."),
                apply: ApplyClass::Live,
            },
            Setting {
                id: "interaction.auto_roll_cadence",
                label: "Auto-roll cadence",
                category: Category::Interaction,
                panel_group: PanelGroup::Default,
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
                panel_group: PanelGroup::Default,
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
                panel_group: PanelGroup::Default,
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
                panel_group: PanelGroup::Default,
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
                panel_group: PanelGroup::Advanced,
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
                panel_group: PanelGroup::Advanced,
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
                panel_group: PanelGroup::Default,
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
                panel_group: PanelGroup::Default,
                default: Value::F32(1.0),
                value: Value::F32(1.0),
                validation: Validation::F32Range { min: 0.2, max: 5.0 },
                tooltip: Some("Changes only how large particles are drawn; it does not add fluid."),
                technical_tooltip: Some("Live render-only point-size multiplier. Physics mass, seeding, pressure, and liquid-cell classification are unchanged."),
                apply: ApplyClass::Live,
            },
            Setting {
                id: "render.speed_scale",
                label: "Speed\u{2192}white",
                category: Category::Render,
                panel_group: PanelGroup::Default,
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
                panel_group: PanelGroup::Default,
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
                panel_group: PanelGroup::Default,
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
                panel_group: PanelGroup::Default,
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
                panel_group: PanelGroup::Default,
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
                panel_group: PanelGroup::Default,
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
                panel_group: PanelGroup::Default,
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
                panel_group: PanelGroup::Default,
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
                panel_group: PanelGroup::Default,
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
                panel_group: PanelGroup::Default,
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
                panel_group: PanelGroup::Default,
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
                panel_group: PanelGroup::Default,
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
                panel_group: PanelGroup::Default,
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
                panel_group: PanelGroup::Default,
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
                panel_group: PanelGroup::Default,
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
                id: "render.hero.mode_enabled",
                label: "Hero water",
                category: Category::Water,
                panel_group: PanelGroup::Default,
                default: Value::U32(1),
                value: Value::U32(1),
                validation: Validation::U32Range { min: 0, max: 1 },
                tooltip: Some("Master toggle for the hero-water look (scene-color refraction). Off renders the plain non-refractive composite over the environment."),
                technical_tooltip: Some("Live enum. Gates the hero sub-features in the screen-space water composite; when off, the refraction offset is forced to zero."),
                apply: ApplyClass::Live,
            },
            Setting {
                id: "render.hero.debug_view",
                label: "Debug view",
                category: Category::Water,
                panel_group: PanelGroup::Default,
                default: Value::U32(0),
                value: Value::U32(0),
                validation: Validation::U32Range { min: 0, max: 7 },
                tooltip: Some("Routes an intermediate hero-water buffer to the screen for debugging."),
                technical_tooltip: Some("Live enum. 0 = normal composite; other values blit a single stage (scene color/depth, thickness, refraction offset, Fresnel, absorption, water-only) to the swapchain."),
                apply: ApplyClass::Live,
            },
            Setting {
                id: "render.hero.ior",
                label: "Index of refraction",
                category: Category::Water,
                panel_group: PanelGroup::Default,
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
                panel_group: PanelGroup::Default,
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
                panel_group: PanelGroup::Default,
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
                panel_group: PanelGroup::Default,
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
                panel_group: PanelGroup::Default,
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
                panel_group: PanelGroup::Default,
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
                panel_group: PanelGroup::Default,
                default: Value::F32(1.5),
                value: Value::F32(1.5),
                validation: Validation::F32Range { min: 0.0, max: 8.0 },
                tooltip: Some("Overall strength of how much the water dims the background it passes through."),
                technical_tooltip: Some("Live Beer-Lambert k multiplier over normalized screen-space thickness."),
                apply: ApplyClass::Live,
            },
            Setting {
                id: "render.hero.base_tint",
                label: "Water tint",
                category: Category::Water,
                panel_group: PanelGroup::Default,
                default: Value::U32(0x194C_CC),
                value: Value::U32(0x194C_CC),
                validation: Validation::U32Range { min: 0, max: 0x00FF_FFFF },
                tooltip: Some("The water's own body color, mixed in as the water gets deeper."),
                technical_tooltip: Some("Live. Blended toward by the thickness-driven body factor."),
                apply: ApplyClass::Live,
            },
            Setting {
                id: "render.hero.transparency",
                label: "Transparency",
                category: Category::Water,
                panel_group: PanelGroup::Default,
                default: Value::F32(0.4),
                value: Value::F32(0.4),
                validation: Validation::F32Range { min: 0.0, max: 1.0 },
                tooltip: Some("How much of the refracted background remains visible through deep water."),
                technical_tooltip: Some("Live. Scales down the body-color opacity so 1.0 keeps the background fully visible."),
                apply: ApplyClass::Live,
            },
            Setting {
                id: "render.hero.deep_water_darkening",
                label: "Deep darkening",
                category: Category::Water,
                panel_group: PanelGroup::Default,
                default: Value::F32(1.5),
                value: Value::F32(1.5),
                validation: Validation::F32Range { min: 0.0, max: 6.0 },
                tooltip: Some("How quickly the water turns to its body color with depth."),
                technical_tooltip: Some("Live. Controls the 1-exp(-k*thickness) body factor used for tint and opacity."),
                apply: ApplyClass::Live,
            },
            Setting {
                id: "render.hero.floor_pattern_scale",
                label: "Floor pattern scale",
                category: Category::Water,
                panel_group: PanelGroup::Default,
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
                panel_group: PanelGroup::Default,
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
                panel_group: PanelGroup::Default,
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
                panel_group: PanelGroup::Default,
                default: Value::F32(0.3),
                value: Value::F32(0.3),
                validation: Validation::F32Range { min: 0.0, max: 1.0 },
                tooltip: Some("How visible the matte tank walls are."),
                technical_tooltip: Some("Live. Opacity/brightness of the matte side walls in the environment prepass."),
                apply: ApplyClass::Live,
            },
            Setting {
                id: "dev.detailed_gpu_profiling",
                label: "Detailed GPU profiling (dev)",
                category: Category::Dev,
                panel_group: PanelGroup::Dev,
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
            mode_enabled: self.u32_or("render.hero.mode_enabled", 1) != 0,
            debug_view: self.u32_or("render.hero.debug_view", 0),
            ior: self.f32_or("render.hero.ior", 1.33),
            refraction_strength: self.f32_or("render.hero.refraction_strength", 0.6),
            refraction_thickness_scale: self.f32_or("render.hero.refraction_thickness_scale", 1.0),
            refraction_max_offset_px: self.u32_or("render.hero.refraction_max_offset_px", 48) as f32,
            invalid_refraction_fallback: self.u32_or("render.hero.invalid_refraction_fallback", 0),
            absorption_color: unpack_rgb(self.u32_or("render.hero.absorption_color", 0x3366_80)),
            absorption_strength: self.f32_or("render.hero.absorption_strength", 1.5),
            base_tint: unpack_rgb(self.u32_or("render.hero.base_tint", 0x194C_CC)),
            transparency: self.f32_or("render.hero.transparency", 0.4),
            deep_water_darkening: self.f32_or("render.hero.deep_water_darkening", 1.5),
            floor_pattern_scale: self.f32_or("render.hero.floor_pattern_scale", 8.0),
            floor_pattern_strength: self.f32_or("render.hero.floor_pattern_strength", 0.5),
            backdrop_strength: self.f32_or("render.hero.backdrop_strength", 0.6),
            wall_visibility: self.f32_or("render.hero.wall_visibility", 0.3),
        }
    }

    /// Set a setting value by id, clamping to its validation range.
    /// Preserves the Value variant (U32 → round, F32 → as f32).
    /// Returns true if the id was found (regardless of clamping).
    pub fn set_value_f64(&mut self, id: &str, v: f64) -> bool {
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
    /// Shape: [{"id":...,"label":...,"category":...,"panel_group":...,
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
            out.push_str(&format!(
                r#","panel_group":{}"#,
                json_quote(s.panel_group.as_str())
            ));
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
        "render.hero.mode_enabled" => Some(&["Disabled", "Enabled"]),
        "render.hero.invalid_refraction_fallback" => Some(&["Unrefracted", "Base tint"]),
        "render.hero.debug_view" => Some(&[
            "Off",
            "Scene color",
            "Scene depth",
            "Thickness",
            "Refraction UV offset",
            "Fresnel",
            "Absorption",
            "Final water only",
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
        panel_group: PanelGroup,
        tooltip: Option<&'static str>,
        technical_tooltip: Option<&'static str>,
    ) -> Setting {
        Setting {
            id,
            label: "Test setting",
            category: Category::Dev,
            panel_group,
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
        let json = json_for(test_setting(
            "test.no_help",
            PanelGroup::Default,
            None,
            None,
        ));

        assert!(json.contains(r#""panel_group":"default""#));
        assert!(!json.contains(r#""tooltip""#));
        assert!(!json.contains(r#""technical_tooltip""#));
    }

    #[test]
    fn config_json_emits_functional_help_without_technical_help() {
        let json = json_for(test_setting(
            "test.functional_help",
            PanelGroup::Advanced,
            Some("Functional help"),
            None,
        ));

        assert!(json.contains(r#""panel_group":"advanced""#));
        assert!(json.contains(r#""tooltip":"Functional help""#));
        assert!(!json.contains(r#""technical_tooltip""#));
    }

    #[test]
    fn config_json_emits_two_help_fields() {
        let json = json_for(test_setting(
            "test.two_help_fields",
            PanelGroup::Dev,
            Some("Functional help"),
            Some("Technical help"),
        ));

        assert!(json.contains(r#""panel_group":"dev""#));
        assert!(json.contains(r#""tooltip":"Functional help""#));
        assert!(json.contains(r#""technical_tooltip":"Technical help""#));
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
            assert_eq!(setting.panel_group, PanelGroup::Default);
            assert_eq!(setting.apply, ApplyClass::Live);
            assert!(json.contains(&format!(r#""id":"{id}""#)));
        }

        assert!(!registry.auto_roll_enabled());
        assert!(!registry.wave_enabled());
        assert!((registry.auto_roll_strength() - 0.45).abs() < f32::EPSILON);
        assert!((registry.wave_strength() - 0.5).abs() < f32::EPSILON);
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
        assert_eq!(setting.panel_group, PanelGroup::Default);
        assert_eq!(setting.apply, ApplyClass::Live);
        assert_eq!(view.category, Category::Render);
        assert_eq!(view.panel_group, PanelGroup::Default);
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
            "render.hero.mode_enabled",
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
        ];
        for id in ids {
            let s = registry.get(id).unwrap_or_else(|| panic!("missing {id}"));
            assert_eq!(s.category, Category::Water, "{id} must be in the Water tab");
            assert_eq!(s.apply, ApplyClass::Live, "{id} must be Live");
            assert!(json.contains(&format!(r#""id":"{id}""#)));
        }
        // Color + enum metadata is emitted for the right ids.
        assert!(json.contains(r#""options":["Disabled","Enabled"]"#));
        assert!(json.contains(
            r#""options":["Off","Scene color","Scene depth","Thickness","Refraction UV offset","Fresnel","Absorption","Final water only"]"#
        ));

        // hero_params() reads the registry defaults and derives nothing nonsensical.
        let hero = registry.hero_params();
        assert!(hero.mode_enabled);
        assert_eq!(hero.debug_view, 0);
        assert!((hero.ior - 1.33).abs() < 1e-6);
        assert!((hero.refraction_max_offset_px - 48.0).abs() < 1e-6);
        assert_eq!(hero.invalid_refraction_fallback, 0);
    }
}
