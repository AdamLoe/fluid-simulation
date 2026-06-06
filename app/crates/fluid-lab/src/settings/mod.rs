//! Typed config-registry — Phase 0.1 skeleton (data model only).
//!
//! Per `decisions.md` (configuration flows through a schema-driven registry) and
//! the observability split: the typed registry *data model* is early; the rendered
//! config panel, localStorage persistence, tooltips, search, and apply-class dots
//! are deferred to 1.2. This file is the authoritative source for each setting's
//! id, label, category, type, default, validation, tooltip, and apply class.
//!
//! 0.1 holds a handful of settings; 0.2 sketches the fuller first schema. Nothing
//! here is rendered.

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
    Solver,
    Camera,
    Render,
    Dev,
}

impl Category {
    pub fn as_str(self) -> &'static str {
        match self {
            Category::Scene => "Scene",
            Category::Grid => "Grid",
            Category::Particles => "Particles",
            Category::Physics => "Physics",
            Category::Solver => "Solver",
            Category::Camera => "Camera",
            Category::Render => "Render",
            Category::Dev => "Dev",
        }
    }
}

/// A typed setting value. Kept minimal for 0.1; extended as the schema grows.
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
    pub tooltip: &'static str,
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
                tooltip: "Picks the canned starting setup you'll see when the sim begins: Falling blob (a blob drops and splashes), Dam break (a tall column pinned to one wall is released and slams across), or Double splash (two columns fall and collide) — Reset-class: choosing a scenario only takes effect after Reset, which the panel triggers automatically to rebuild the sim from it.",
                apply: ApplyClass::Reset,
            },
            Setting {
                id: "grid.res_x",
                label: "Grid resolution X",
                category: Category::Grid,
                default: Value::U32(64),
                value: Value::U32(64),
                validation: Validation::U32Range { min: 16, max: 128 },
                tooltip: "Sets how many cells make up the tank left-to-right, which also sets how wide the tank is; higher means a wider tank with finer detail, lower means narrower and coarser, all three axes equal gives a cube and unequal gives a rectangular box. — Number of cells along the X axis at a uniform cell size; more cells mean sharper surfaces but far more compute and memory, and it reallocates all grid and solver buffers so it is Reset-class and only takes effect after Reset.",
                // Reallocates all grid/solver buffers → reset.
                apply: ApplyClass::Reset,
            },
            Setting {
                id: "grid.res_y",
                label: "Grid resolution Y",
                category: Category::Grid,
                default: Value::U32(64),
                value: Value::U32(64),
                validation: Validation::U32Range { min: 16, max: 128 },
                tooltip: "Sets how many cells make up the tank top-to-bottom, which also sets how tall the tank is; higher means a taller tank with finer detail, lower means shorter and coarser, all three axes equal gives a cube and unequal gives a rectangular box. — Number of cells along the Y axis at a uniform cell size; more cells mean sharper surfaces but far more compute and memory, and it reallocates all grid and solver buffers so it is Reset-class and only takes effect after Reset.",
                // Reallocates all grid/solver buffers → reset.
                apply: ApplyClass::Reset,
            },
            Setting {
                id: "grid.res_z",
                label: "Grid resolution Z",
                category: Category::Grid,
                default: Value::U32(64),
                value: Value::U32(64),
                validation: Validation::U32Range { min: 16, max: 128 },
                tooltip: "Sets how many cells make up the tank front-to-back, which also sets how deep the tank is; higher means a deeper tank with finer detail, lower means shallower and coarser, all three axes equal gives a cube and unequal gives a rectangular box. — Number of cells along the Z axis at a uniform cell size; more cells mean sharper surfaces but far more compute and memory, and it reallocates all grid and solver buffers so it is Reset-class and only takes effect after Reset.",
                // Reallocates all grid/solver buffers → reset.
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
                tooltip: "How much liquid the sim starts with: more particles make the fluid look smoother and more detailed, fewer make it chunkier and faster. — Number of particles seeded into the initial liquid block; higher counts slow down every step, and it is Reset-class so it takes effect after Reset rebuilds the sim with the new count. The slider runs 1,024 → 134,217,728 in ×2 steps (each notch doubles the count); type an exact number into the box for any value in between (heavy — large buffer allocation, may exceed device limits).",
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
                tooltip: "How hard the fluid is pulled down (or up): a stronger negative value makes it fall and splash harder, positive flips it so it rises, and you can drag on the tank to tilt the direction. — Gravitational acceleration along Y in m/s^2, negative is down (default -9.81) and positive is up; applied Live.",
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
                tooltip: "How big a slice of time each physics step advances: smaller slices give steadier, more accurate motion but need more steps to keep up, larger slices are cheaper but can look jumpy or go unstable. — Fixed physics timestep in seconds; the browser frame dt never feeds advection directly, and it is Reset-class so it takes effect after Reset.",
                apply: ApplyClass::Reset,
            },
            Setting {
                id: "physics.flip_blend",
                label: "FLIP blend",
                category: Category::Physics,
                default: Value::F32(0.9),
                value: Value::F32(0.9),
                validation: Validation::F32Range { min: 0.0, max: 1.0 },
                tooltip: "How lively the fluid moves: lower makes it calmer and more damped, higher makes it more energetic and splashy. — Blend between PIC and FLIP velocity transfer where 0 is pure PIC (damped, stable) and 1 is pure FLIP (lively, splashy); applied Live.",
                apply: ApplyClass::Live,
            },
            Setting {
                id: "physics.max_substeps",
                label: "Max substeps",
                category: Category::Physics,
                default: Value::U32(1),
                value: Value::U32(1),
                validation: Validation::U32Range { min: 1, max: 16 },
                tooltip: "Limits how many physics steps fire per rendered frame so a slow frame stays cheap and the browser catches up by rendering the next frame rather than by making one frame longer; excess accumulated sim time is dropped. — Maximum fixed-dt physics substeps fired per rAF callback; default 1 prefers interactivity (excess accumulated sim time is dropped). Raise to 4 for dev/stress catch-up. Reset-class.",
                apply: ApplyClass::Reset,
            },
            Setting {
                id: "physics.wall_friction",
                label: "Wall friction",
                category: Category::Physics,
                default: Value::F32(0.0),
                value: Value::F32(0.0),
                validation: Validation::F32Range { min: 0.0, max: 1.0 },
                tooltip: "How much the water grips the walls: 0 lets it slide freely off walls and ceiling, higher makes it drag and cling to them. — Wall friction applied on particle-wall contact where 0 is free slip and 1 is no-slip (tangential velocity killed), default 0; applied Live.",
                apply: ApplyClass::Live,
            },
            Setting {
                id: "physics.rest_density",
                label: "Rest particles/cell",
                category: Category::Physics,
                default: Value::F32(8.0),
                value: Value::F32(8.0),
                validation: Validation::F32Range { min: 1.0, max: 32.0 },
                tooltip: "How tightly the fluid likes to pack before it pushes back: lower gives looser, fluffier fluid, higher allows denser packing before the solver reacts. — Target particles-per-cell the pressure solver treats as neutral; combined with Volume stiffness, cells holding more than this get pushed apart, applied Live.",
                apply: ApplyClass::Live,
            },
            Setting {
                id: "physics.volume_stiffness",
                label: "Volume stiffness",
                category: Category::Physics,
                default: Value::F32(1.0),
                value: Value::F32(1.0),
                validation: Validation::F32Range { min: 0.0, max: 4.0 },
                tooltip: "How strongly the fluid fights clumping: 0 lets particles bunch up, higher actively spreads crowded spots out for more even, uniform-looking fluid. — Anti-clumping stiffness in the pressure solve where 0 is pure incompressibility and higher pushes over-dense cells back toward Rest particles/cell; the physical replacement for the old occupancy liquid-volume hack, applied Live.",
                apply: ApplyClass::Live,
            },
            Setting {
                id: "physics.drift_clamp",
                label: "Drift clamp",
                category: Category::Physics,
                default: Value::F32(0.5),
                value: Value::F32(0.5),
                validation: Validation::F32Range { min: 0.05, max: 2.0 },
                tooltip: "A safety limit on how fast anti-clumping is allowed to act: lower keeps things gentle and stable, higher lets it correct faster but can make the fluid jitter. — Stability cap on the per-step volume correction added to the divergence in cell-divergence units, inert when Volume stiffness is 0; applied Live.",
                apply: ApplyClass::Live,
            },
            Setting {
                id: "physics.cfl",
                label: "Splash speed (CFL)",
                category: Category::Physics,
                default: Value::F32(2.0),
                value: Value::F32(2.0),
                validation: Validation::F32Range { min: 1.0, max: 6.0 },
                tooltip: "How high water can be thrown when you slosh or shake the tank: 1 is the calm baseline, higher lets fast water launch farther up the walls. — CFL number = max grid cells a particle may cross per substep; the velocity ceiling is this x h/dt, so raising it decouples splash height from grid resolution (a finer grid otherwise lowers the ceiling). Applied Live.",
                apply: ApplyClass::Live,
            },
            Setting {
                id: "classify.liquid_threshold",
                label: "Liquid threshold",
                category: Category::Solver,
                default: Value::U32(1),
                value: Value::U32(1),
                validation: Validation::U32Range { min: 1, max: 8 },
                tooltip: "Controls how puffy versus tight the liquid surface looks: 1 counts even a stray droplet as liquid for a puffy surface, higher lets thin spray stay as air for a tighter, denser surface. — Minimum particles in a cell for it to be classified liquid and pressure-solved (default 1); applied Live.",
                apply: ApplyClass::Live,
            },
            Setting {
                id: "classify.surface_dilation",
                label: "Surface dilation",
                category: Category::Solver,
                default: Value::U32(0),
                value: Value::U32(0),
                validation: Validation::U32Range { min: 0, max: 1 },
                tooltip: "Helps fast-moving fluid hold together by filling tiny gaps: off can leave pinholes and torn sheets, on seals them so the surface stays continuous. — Morphological one-cell dilation of the liquid region where 0 is off and 1 makes cells next to liquid also count as liquid, costing a little extra pressure work; applied Live.",
                apply: ApplyClass::Live,
            },
            Setting {
                id: "solver.pressure_iterations",
                label: "Pressure iterations",
                category: Category::Solver,
                default: Value::U32(30),
                value: Value::U32(30),
                validation: Validation::U32Range { min: 1, max: 200 },
                tooltip: "How hard the sim works each step to keep the fluid from compressing: more iterations give more solid, believable water but cost more time, too few can let it look spongy. — Conjugate-Gradient pressure-solve iterations per substep; CG converges about 10x faster than Jacobi and roughly 15 fully resolve the 64^3 hydrostatic column (default 30 for headroom), applied Live.",
                apply: ApplyClass::Live,
            },
            Setting {
                id: "render.particle_size",
                label: "Particle size",
                category: Category::Render,
                default: Value::F32(1.0),
                value: Value::F32(1.0),
                validation: Validation::F32Range { min: 0.2, max: 5.0 },
                tooltip: "How big each particle looks: larger makes the same fluid appear to fill more volume and read as more solid, smaller makes it look sparser. — Render-only multiplier on particle point size with no effect on the physics; applied Live.",
                apply: ApplyClass::Live,
            },
            Setting {
                id: "render.speed_scale",
                label: "Speed\u{2192}white",
                category: Category::Render,
                default: Value::F32(4.0),
                value: Value::F32(4.0),
                validation: Validation::F32Range { min: 0.5, max: 20.0 },
                tooltip: "Sets how fast water has to move to glow white: lower makes even gentle motion turn white, higher keeps it blue until it really races. — Speed in units/s mapped to full white, with particle color ramping deep-blue to white over the range [0, this]; render-only, applied Live.",
                apply: ApplyClass::Live,
            },
            Setting {
                id: "render.mesh_iso",
                label: "Surface level",
                category: Category::Render,
                default: Value::F32(2.0),
                value: Value::F32(2.0),
                validation: Validation::F32Range { min: 0.5, max: 8.0 },
                tooltip: "Where the water surface is drawn in marching-cubes mode: lower wraps the surface loosely around sparse spray for a puffier blob, higher hugs only dense water for a tighter, smaller surface. — Isolevel (particles/cell) the MC mesh extracts at; only affects the mesh view. Applied Live.",
                apply: ApplyClass::Live,
            },
            Setting {
                id: "render.mesh_smooth",
                label: "Surface smoothing",
                category: Category::Render,
                default: Value::U32(2),
                value: Value::U32(2),
                validation: Validation::U32Range { min: 0, max: 8 },
                tooltip: "How smooth versus bumpy the marching-cubes surface looks: 0 is raw and lumpy (per-cell blobs), higher rounds the surface off into smooth water. — Number of extra Gaussian blur iterations applied to the density field before MC extraction; mesh view only. Applied Live.",
                apply: ApplyClass::Live,
            },
            Setting {
                id: "render.mesh_opacity",
                label: "Water opacity",
                category: Category::Render,
                default: Value::F32(0.55),
                value: Value::F32(0.55),
                validation: Validation::F32Range { min: 0.05, max: 1.0 },
                tooltip: "How see-through the water surface is: lower is glassy and transparent, higher is thick and opaque. — Base alpha of the MC water surface (Fresnel still adds opacity at grazing angles); mesh view only. Applied Live.",
                apply: ApplyClass::Live,
            },
            Setting {
                id: "render.mesh_fresnel",
                label: "Water reflectivity",
                category: Category::Render,
                default: Value::F32(1.0),
                value: Value::F32(1.0),
                validation: Validation::F32Range { min: 0.0, max: 2.0 },
                tooltip: "How mirror-like the water looks at glancing angles: 0 is flat matte water, higher gives bright sky-reflecting edges like real water. — Strength multiplier on the Schlick-Fresnel reflection/opacity term of the MC water shader; mesh view only. Applied Live.",
                apply: ApplyClass::Live,
            },
            Setting {
                id: "render.mesh_foam",
                label: "Foam amount",
                category: Category::Render,
                default: Value::F32(0.8),
                value: Value::F32(0.8),
                validation: Validation::F32Range { min: 0.0, max: 2.0 },
                tooltip: "How much white foam appears where the water is moving fast: 0 keeps the surface clear blue everywhere, higher whitens fast-moving splash and crests. — Strength of the velocity-driven foam term that tints fast surface regions white (matching the particle speed-to-white cue); mesh view only. Applied Live.",
                apply: ApplyClass::Live,
            },
            Setting {
                id: "render.water_absorb",
                label: "Water depth tint",
                category: Category::Render,
                default: Value::F32(2.5),
                value: Value::F32(2.5),
                validation: Validation::F32Range { min: 0.0, max: 8.0 },
                tooltip: "How strongly the water colours with depth: 0 is clear glass (a long path through water does nothing), higher makes thick water deepen toward blue-green while thin films stay clear. — Beer-Lambert absorption coefficient applied over the per-pixel water thickness (front-to-back surface distance); mesh view only. Applied Live.",
                apply: ApplyClass::Live,
            },
            Setting {
                id: "render.water_refract",
                label: "Water refraction",
                category: Category::Render,
                default: Value::F32(0.6),
                value: Value::F32(0.6),
                validation: Validation::F32Range { min: 0.0, max: 2.0 },
                tooltip: "How much the background bends as it shows through the water: 0 looks through cleanly, higher distorts the view behind the surface like real water. — Screen-space refraction offset strength along the surface normal; mesh view only. Applied Live.",
                apply: ApplyClass::Live,
            },
            Setting {
                id: "render.fps_target",
                label: "FPS target",
                category: Category::Render,
                default: Value::U32(60),
                value: Value::U32(60),
                validation: Validation::U32Range { min: 0, max: 240 },
                tooltip: "How smooth and how heavy the animation is: higher looks smoother but works the GPU harder, lower eases GPU load, and 0 runs as fast as the browser allows. — Target frames per second for the render loop where 0 is uncapped; render-only, applied Live.",
                apply: ApplyClass::Live,
            },
            Setting {
                id: "camera.rot_x",
                label: "Camera pitch",
                category: Category::Camera,
                default: Value::F32(-0.2),
                value: Value::F32(-0.2),
                validation: Validation::F32Range { min: -3.14159, max: 3.14159 },
                tooltip: "Tilts the starting view up or down: positive looks down onto the tank from above, negative looks up at it from below. — Initial camera pitch, rotation around the X axis in radians; applied Live and restored on Reset.",
                apply: ApplyClass::Live,
            },
            Setting {
                id: "camera.rot_y",
                label: "Camera yaw",
                category: Category::Camera,
                default: Value::F32(0.6),
                value: Value::F32(0.6),
                validation: Validation::F32Range { min: -3.14159, max: 3.14159 },
                tooltip: "Spins the starting view around the tank sideways, choosing which face you look at first. — Initial camera yaw, the horizontal orbit angle as rotation around the Y axis in radians; applied Live and restored on Reset.",
                apply: ApplyClass::Live,
            },
            Setting {
                id: "camera.rot_z",
                label: "Camera roll",
                category: Category::Camera,
                default: Value::F32(0.0),
                value: Value::F32(0.0),
                validation: Validation::F32Range { min: -3.14159, max: 3.14159 },
                tooltip: "Tilts the whole starting view sideways so the horizon line leans left or right. — Initial camera roll, rotation around the Z axis in radians; applied Live and restored on Reset.",
                apply: ApplyClass::Live,
            },
            Setting {
                id: "camera.distance",
                label: "Camera distance",
                category: Category::Camera,
                default: Value::F32(6.0),
                value: Value::F32(6.0),
                validation: Validation::F32Range { min: 2.0, max: 40.0 },
                tooltip: "How zoomed in the starting view is: lower moves the camera closer so the tank fills more of the screen, higher pulls back for a wider shot. — Initial camera zoom distance from the tank center; applied Live and restored on Reset.",
                apply: ApplyClass::Live,
            },
            Setting {
                id: "render.sun_x",
                label: "Sun dir X",
                category: Category::Render,
                default: Value::F32(0.4),
                value: Value::F32(0.4),
                validation: Validation::F32Range { min: -2.0, max: 2.0 },
                tooltip: "Shifts the lighting direction left or right, changing which side of the water surface catches the highlight. — Sun direction X component in world space; the vector is normalized so only the ratio between X, Y, and Z matters, and it affects the lit mesh surface only, applied Live.",
                apply: ApplyClass::Live,
            },
            Setting {
                id: "render.sun_y",
                label: "Sun dir Y",
                category: Category::Render,
                default: Value::F32(1.0),
                value: Value::F32(1.0),
                validation: Validation::F32Range { min: -2.0, max: 2.0 },
                tooltip: "Raises or lowers the light: positive lights the water from above, negative lights it from below. — Sun direction Y component in world space; the vector is normalized so only the ratio between X, Y, and Z matters, and it affects the lit mesh surface only, applied Live.",
                apply: ApplyClass::Live,
            },
            Setting {
                id: "render.sun_z",
                label: "Sun dir Z",
                category: Category::Render,
                default: Value::F32(0.3),
                value: Value::F32(0.3),
                validation: Validation::F32Range { min: -2.0, max: 2.0 },
                tooltip: "Shifts the lighting direction front to back, changing which side of the water surface catches the highlight. — Sun direction Z component in world space; the vector is normalized so only the ratio between X, Y, and Z matters, and it affects the lit mesh surface only, applied Live.",
                apply: ApplyClass::Live,
            },
            Setting {
                id: "dev.mesh_enabled",
                label: "Marching cubes (dev)",
                category: Category::Dev,
                default: Value::U32(0),
                value: Value::U32(0),
                validation: Validation::U32Range { min: 0, max: 1 },
                tooltip: "Renders the fluid surface as a marching-cubes mesh instead of particles; a heavyweight dev/debug view. — Allocates ~73 MB of MC GPU buffers + pipelines only when on; 0=particles (default), 1=mesh. Requires Reset.",
                apply: ApplyClass::Reset,
            },
            Setting {
                id: "dev.detailed_gpu_profiling",
                label: "Detailed GPU profiling (dev)",
                category: Category::Dev,
                default: Value::U32(0),
                value: Value::U32(0),
                validation: Validation::U32Range { min: 0, max: 1 },
                tooltip: "Breaks the GPU timeline into per-phase sections (clear, P2G, CG, gradient, G2P, render) instead of coarse totals; adds some GPU overhead. — Splits sim compute into many small timestamped passes and sizes an extended query set; 0=coarse (default), 1=detailed. Requires Reset.",
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
        self.get("physics.fixed_dt").map(|s| s.as_f32()).unwrap_or(1.0 / 120.0)
    }
    #[allow(dead_code)]
    pub fn gravity(&self) -> f32 {
        self.get("physics.gravity").map(|s| s.as_f32()).unwrap_or(-9.81)
    }
    pub fn flip_blend(&self) -> f32 {
        self.get("physics.flip_blend").map(|s| s.as_f32()).unwrap_or(0.9)
    }
    pub fn wall_friction(&self) -> f32 {
        self.get("physics.wall_friction").map(|s| s.as_f32()).unwrap_or(0.0)
    }
    pub fn rest_density(&self) -> f32 {
        self.get("physics.rest_density").map(|s| s.as_f32()).unwrap_or(8.0)
    }
    pub fn volume_stiffness(&self) -> f32 {
        self.get("physics.volume_stiffness").map(|s| s.as_f32()).unwrap_or(1.0)
    }
    pub fn drift_clamp(&self) -> f32 {
        self.get("physics.drift_clamp").map(|s| s.as_f32()).unwrap_or(0.5)
    }
    pub fn liquid_threshold(&self) -> u32 {
        self.get("classify.liquid_threshold").map(|s| s.as_u32()).unwrap_or(1)
    }
    pub fn surface_dilation(&self) -> u32 {
        self.get("classify.surface_dilation").map(|s| s.as_u32()).unwrap_or(0)
    }
    pub fn max_substeps(&self) -> u32 {
        self.get("physics.max_substeps").map(|s| s.as_u32()).unwrap_or(4)
    }
    pub fn pressure_iterations(&self) -> u32 {
        self.get("solver.pressure_iterations").map(|s| s.as_u32()).unwrap_or(40)
    }
    pub fn particle_size(&self) -> f32 {
        self.get("render.particle_size").map(|s| s.as_f32()).unwrap_or(1.0)
    }
    pub fn speed_scale(&self) -> f32 {
        self.get("render.speed_scale").map(|s| s.as_f32()).unwrap_or(4.0)
    }
    pub fn fps_target(&self) -> u32 {
        self.get("render.fps_target").map(|s| s.as_u32()).unwrap_or(60)
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
        self.get("camera.distance").map(|s| s.as_f32()).unwrap_or(6.0)
    }
    pub fn sun_x(&self) -> f32 {
        self.get("render.sun_x").map(|s| s.as_f32()).unwrap_or(0.4)
    }
    pub fn sun_y(&self) -> f32 {
        self.get("render.sun_y").map(|s| s.as_f32()).unwrap_or(1.0)
    }
    pub fn sun_z(&self) -> f32 {
        self.get("render.sun_z").map(|s| s.as_f32()).unwrap_or(0.3)
    }
    pub fn cfl(&self) -> f32 {
        self.get("physics.cfl").map(|s| s.as_f32()).unwrap_or(2.0)
    }
    pub fn mesh_iso(&self) -> f32 {
        self.get("render.mesh_iso").map(|s| s.as_f32()).unwrap_or(2.0)
    }
    pub fn mesh_smooth(&self) -> u32 {
        self.get("render.mesh_smooth").map(|s| s.as_u32()).unwrap_or(2)
    }
    pub fn mesh_opacity(&self) -> f32 {
        self.get("render.mesh_opacity").map(|s| s.as_f32()).unwrap_or(0.55)
    }
    pub fn mesh_fresnel(&self) -> f32 {
        self.get("render.mesh_fresnel").map(|s| s.as_f32()).unwrap_or(1.0)
    }
    pub fn water_absorb(&self) -> f32 {
        self.get("render.water_absorb").map(|s| s.as_f32()).unwrap_or(2.5)
    }
    pub fn water_refract(&self) -> f32 {
        self.get("render.water_refract").map(|s| s.as_f32()).unwrap_or(0.6)
    }
    pub fn mesh_foam(&self) -> f32 {
        self.get("render.mesh_foam").map(|s| s.as_f32()).unwrap_or(0.8)
    }
    pub fn mesh_enabled(&self) -> bool {
        self.get("dev.mesh_enabled").map(|s| s.as_u32() != 0).unwrap_or(false)
    }
    pub fn detailed_gpu_profiling(&self) -> bool {
        self.get("dev.detailed_gpu_profiling").map(|s| s.as_u32() != 0).unwrap_or(false)
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
    /// Shape: [{"id":...,"label":...,"category":...,"type":...,"value":<num>,
    ///          "default":<num>,"min":<num>,"max":<num>,"apply":...,"tooltip":...}, ...]
    pub fn config_json(&self) -> String {
        let mut out = String::from("[");
        for (i, s) in self.settings.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push('{');
            out.push_str(&format!(r#""id":{}"#, json_quote(s.id)));
            out.push_str(&format!(r#","label":{}"#, json_quote(s.label)));
            out.push_str(&format!(r#","category":{}"#, json_quote(s.category.as_str())));
            out.push_str(&format!(r#","type":{}"#, json_quote(s.type_str())));
            out.push_str(&format!(r#","value":{}"#, fmt_f64(s.value_as_f64())));
            out.push_str(&format!(r#","default":{}"#, fmt_f64(s.default_as_f64())));
            out.push_str(&format!(r#","min":{}"#, fmt_f64(s.min_as_f64())));
            out.push_str(&format!(r#","max":{}"#, fmt_f64(s.max_as_f64())));
            out.push_str(&format!(r#","apply":{}"#, json_quote(s.apply.as_str())));
            out.push_str(&format!(r#","tooltip":{}"#, json_quote(s.tooltip)));
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
        _ => None,
    }
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
