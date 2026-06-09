---
status:        active
owner:         adamg
last_updated:  2026-06-09
okay_to_delete: false
long_lived:    true
---

# Settings registry

The settings registry is the single typed, schema-driven config source for every
tunable parameter in fluid-lab. Each parameter is declared once in
`crates/fluid-lab/src/settings/mod.rs -> Registry`; the JS panel, localStorage
restore path, and runtime setters read from or write through that registry instead of
defining settings independently.

## What It Owns

`crates/fluid-lab/src/settings/mod.rs -> Setting` owns each row's stable id,
label, semantic `Category`, top-level `PanelGroup`, numeric value type/default/bounds,
optional help copy, and `ApplyClass`. `Category` is the section name inside a panel
tier; the Interaction category groups tank auto-roll and wave-maker controls.
`PanelGroup` is the top-level UI tier serialized as `panel_group` with values
`default`, `advanced`, or `dev`.

Help is deliberately optional. `tooltip` is short functional help: what visibly changes
or how the control should be used. `technical_tooltip` is optional technical help:
where the sim uses the value, apply-class implications, and failure modes when useful.
Rows may have no help, functional help only, or both fields; absent fields are omitted
from `config_json` rather than emitted as empty strings. There is no one-string
functional-plus-technical separator convention.

Persistence is a web-shell concern. The registry holds canonical runtime state; the
panel layers saved values on load by calling `set_setting` for each known persisted id.
The shell may intentionally hide or suppress persistence for user-invisible internal
controls while still keeping them as real registry settings.

## Apply Classes

`crates/fluid-lab/src/settings/mod.rs -> ApplyClass` is a three-variant enum:

- `Live` changes are pushed to the running sim immediately and `set_setting` returns
  `true`.
- `Reset` changes are stored in the registry but require `app.reset()` to rebuild
  buffers, scene data, or baked-at-init constants.
- `Reload` changes are stored but require a page/device reload.

Assignment rule: use `Reset` for buffer allocation, scene rebuild, fixed timestep,
max-substep, and detailed-profiling changes; use `Reload` for device/feature/threading
settings; use `Live` only when the running GPU state can be updated safely.

Reset-class settings are not materialized by `set_setting` itself. The caller owns the
reset step, which lets the panel batch multiple reset-class edits before one rebuild.
Live settings may update GPU parameters or app-owned scheduling state. Interaction
settings are Live app controls: they change how future frames are scheduled but do not
recreate buffers.

## JS Bridge

The settings bridge lives in `crates/fluid-lab/src/lib.rs -> FluidApp::config_json`
and `FluidApp::set_setting`, with serialization in
`crates/fluid-lab/src/settings/mod.rs -> Registry::config_json`.

Each `config_json()` entry carries the stable numeric/control contract:

```json
{
  "id": "solver.pressure_iterations",
  "label": "Pressure iterations",
  "category": "Solver",
  "panel_group": "default",
  "type": "u32",
  "value": 30,
  "default": 30,
  "min": 1,
  "max": 200,
  "apply": "live"
}
```

Optional fields are emitted only when present:

- `tooltip` and `technical_tooltip` for functional and technical help.
- `options` for enum/dropdown settings from
  `crates/fluid-lab/src/settings/mod.rs -> enum_options`.
- `slider_scale` for panel rendering hints from
  `crates/fluid-lab/src/settings/mod.rs -> slider_scale`
  (`log2` particle-count slider, `color` packed RGB color picker).

`set_setting(id, value: f64)` validates and stores the value. For Live settings it
also dispatches the matching GPU/app setter and returns `true`; for Reset/Reload or
unknown ids it returns `false`.

## Compactness Taxonomy

Compactness is intentionally split across several settings:

- `render.particle_size` is visual only. It changes point size, not mass, pressure, or
  liquid-cell classification.
- `particles.count` plus `scene.preset` control initial seeded mass/distribution and
  require Reset.
- `physics.rest_density`, `physics.volume_stiffness`, and `physics.drift_clamp` are
  Advanced volume-correction controls for the occupancy-driven anti-clump divergence
  bias.
- `classify.liquid_threshold` and `classify.surface_dilation` are Advanced liquid-cell
  inclusion controls.
- `solver.pressure_iterations` stays in the default panel group because it is the main
  visible incompressibility/FPS trade-off.

`physics.max_substeps` and `dev.detailed_gpu_profiling` are Dev-tier controls. Particle
look controls that are self-evident, such as packed colors and opacity, can omit help
entirely.

## Interaction Controls

The Interaction settings are Live and default to conservative off states for the
feature toggles:

- `interaction.auto_roll_enabled`, `interaction.auto_roll_strength`, and
  `interaction.auto_roll_cadence` control bounded automatic tank rocking. They affect
  `FluidApp` scheduling and the tank pose/gravity path, not the camera.
- `interaction.wave_enabled`, `interaction.wave_strength`, and
  `interaction.wave_frequency` control periodic local horizontal impulses through the
  existing particle impulse pass. They never create or delete particles.

These controls stay in the default panel group because they are product-facing
interaction tools, not internal solver tuning. If future randomness/seed controls are
added, those belong in Advanced unless the product needs them visible by default.

The v1.7 shell treats the two `*_enabled` booleans as internal scheduler state rather
than direct user controls. `web/panels.js -> HIDDEN_SETTING_IDS` suppresses
`interaction.auto_roll_enabled` and `interaction.wave_enabled` from the config UI and
from the persisted `localStorage` payload. `web/main.js` owns product-mode selection
(Auto Rotate / Waves / Manual) and writes those hidden booleans through
`set_setting(...)` as shell state transitions. The visible Interaction rows are the
mode-specific strength/cadence/frequency settings, rendered under the workspace's
Modes tab.

## Water Render Controls

The public transparency control is `render.water_optical_density`, a Live render
setting exposed in `config_json` as "Water absorption". It controls Beer-Lambert
absorption over normalized screen-space thickness, not a flat alpha multiplier.
`render.particle_size` changes raster coverage and smoothing feel, but the represented
volume normalization prevents it from acting as an opacity control.

`render.particle_view` is a Live enum that switches between the default screen-space
water composite, the v1.10 optical-depth particle renderer, and the pre-v1.10 simple
alpha particle renderer. Both particle views preserve direct per-particle motion/speed
inspection while the default view presents the accumulated liquid body.
`render.particle_shading` is the Live lighting strength for screen-space water and
the shaded particle billboard modes.

Whitewater controls are Live screen-space composite controls. `render.whitewater_strength`
sets how far fast water mixes toward white/ice-blue, `render.whitewater_threshold`
sets the normalized speed needed before the effect appears, and
`render.whitewater_softness` controls the fade around that threshold. These controls
act on a speed-weighted thickness target; they do not change simulation velocity or
particle mass.

`render.particle_alpha` is not serialized into `config_json` and is not a visible or
persisted user setting anymore. `FluidApp::set_setting("render.particle_alpha", ...)`
is accepted only as a legacy compatibility no-op that logs a redirect to
`render.water_optical_density`. The remaining water/debug look controls —
`render.particle_size`, `render.particle_edge`, `render.particle_slow_color`,
`render.particle_fast_color`, and `render.speed_scale` — stay Live.

## Hero Water Controls

The `Water` category (v1.12) holds the hero-water controls, rendered under their own
**Water** workspace tab (`web/panels.js -> TAB_CATEGORY_ALLOWLIST.water = {"Water"}`).
Every `render.hero.*` setting is **Live** and in the `default` panel group, so sliders
auto-apply with no reset:

- `render.hero.mode_enabled` — enum master toggle. Off forces both the refraction offset
  and the reflection strength to zero (the non-hero comparison baseline).
- `render.hero.debug_view` — enum routing an intermediate composite stage to the
  swapchain (scene color/depth, thickness, refraction UV offset, Fresnel, absorption,
  water-only, reflection, env-only). Authoritative list:
  `settings/mod.rs -> enum_options`.
- Refraction: `ior` (drives Schlick `f0` — `f0` is never an independent setting),
  `refraction_strength`, `refraction_thickness_scale`, `refraction_max_offset_px`,
  `invalid_refraction_fallback` (enum).
- Material: `absorption_color` (+`absorption_strength`), `base_tint`, `transparency`,
  `deep_water_darkening`. The two colors use the `color` `slider_scale`.
- Environment prepass: `floor_pattern_scale`, `floor_pattern_strength`,
  `wall_visibility`. (`backdrop_strength` is an inert legacy id — the backdrop quad was
  replaced by the procedural skybox; `environment_brightness` controls the new world
  background.)
- Reflection (v1.15): `reflection_strength`, `environment_strength`, `environment_mode`
  (enum Sky/Room/Studio), `environment_rotation`, `environment_brightness`,
  `skybox_enabled` (enum). The same procedural environment feeds both the world skybox and
  the water's reflected environment (`rendering.md`).
- Roughness/specular (v1.15): `roughness_base` + the velocity/chop/foam scales
  (`roughness_velocity_scale`, `roughness_normal_variance_scale`, `roughness_foam_scale`,
  Advanced), `specular_strength`, `sun_dir_{x,y,z}` (Advanced) + `sun_intensity`.
- Micro-normals (v1.15, Advanced, off by default): `micro_normal_enabled` (enum),
  `micro_normal_strength`, `micro_normal_scale`, `micro_normal_velocity_scale`.
- Caustics (`render.hero.caustics.*`, Advanced, off by default): `enabled` plus
  `intensity`, `focus_strength`, `thickness_scale`, `floor_enabled`, `back_wall_enabled`,
  `side_walls_enabled`, `motion_scale`, `max_intensity`. `mode`, `resolution_scale`, and
  `blur_radius` are reserved/not-yet-wired (their tooltips say so). The
  `caustics.temporal_enabled`/`caustics.temporal_alpha` pair is now driven through the
  unified Temporal control below (kept for compatibility; see the temporal block).
- Wet walls (`render.hero.wet_wall.*`, Advanced, off by default): `enabled`,
  `wetness_decay`, `wetness_contact_gain`, `wetness_spray_gain` (stubbed at 0),
  `darkening_strength`, `gloss_strength`, `streak_strength`, `meniscus_enabled`,
  `meniscus_width`, `meniscus_strength`, `meniscus_fresnel_boost`,
  `contact_shadow_enabled`, `contact_shadow_strength`, `contact_shadow_radius`.
- Temporal stabilization (`render.hero.temporal.*`, Advanced, off by default): `enabled`,
  the per-target toggles `thickness_history` / `normal_history` (gates the `smooth_z`
  blend the normal derives from) / `caustic_history` / `foam_history`, `history_alpha`
  (`mix(current, history, α)`; 0 = all-current, 1 = frozen), `camera_motion_reset_threshold`,
  `depth_reject_threshold`, `normal_reject_threshold`, and `jitter_enabled` (kept off;
  reserved for a future TAA-jitter follow-up). This is the **unified** temporal control:
  it also governs the v1.16 caustics in-shader history blend (`rendering.md`).

Unlike other Live settings (one GPU setter per id in `set_setting`), the hero settings
share a **single uniform**: `set_setting` matches the `render.hero.` id prefix, rebuilds
the flat `Registry::hero_params() -> HeroParams` snapshot, and pushes it via
`GpuContext::set_hero_params`, which fans out to the composite, environment, skybox,
caustics, and wet-wall uniforms. There is no per-setting GPU plumbing; adding a hero knob
means a registry row plus a field in `HeroParams`/`hero_uniform`.
The authoritative row list is `settings/mod.rs -> Registry::default` (ids prefixed
`render.hero.`).

The `render.diffuse.*` block (v1.13) adds the foam/spray/bubble diffuse-water controls
to the same **Water** tab, all Live, with the same single-snapshot pattern:
`set_setting` matches the `render.diffuse.` prefix, rebuilds
`Registry::diffuse_params() -> DiffuseParams`, and pushes it via
`GpuContext::set_diffuse_params`. The set is the master `enabled` toggle, the active
`max_particles` cap + `emit_rate`/`emit_budget_per_frame`, the surface-speed and
wall-impact `*_threshold`/`*_gain` emission signals, per-type lifetimes, shared
`radius`/`alpha`, `bubble_buoyancy`, `spray_drag`, a `debug_view` enum, and
`random_seed` (Advanced). `max_particles` is an active cap within a fixed GPU buffer
capacity, so it applies Live without a reset (see `rendering.md`). The
authoritative row list is `settings/mod.rs -> Registry::default` (ids prefixed
`render.diffuse.`).

## Gotchas

- The registry is append-safe: lookups and mutations are by id, not row index.
- `set_value_f64` clamps instead of rejecting out-of-range values.
- The declared `Value` variant fixes the stored type; U32 settings round incoming f64s
  before storage, and F32 settings cast to f32.
- `GpuContext` preserves Live particle-look values across fluid recreation and reapplies
  them to the new particle renderer.
- Reset restores tank pose and interaction schedules but preserves the Live
  interaction setting values.
- Reload restores persisted visible settings, then the shell reapplies the default
  Auto Rotate product mode; hidden enable booleans are therefore not durable user
  preferences.
- There is no `solver.density` setting. The CG pressure solve uses a hardcoded density
  internally, and the current scale convention makes it visually inert; see
  `../decisions/pressure.md`.

## Update When

- A tunable is added, removed, regrouped, or recategorized.
- A setting's apply class changes, especially when a Live path must be added or removed
  from `FluidApp::set_setting`.
- The JSON bridge shape changes, including optional help or panel-rendering metadata.
- Interaction control semantics, defaults, grouping, or Live scheduling behavior change.
- The shell's hidden-setting/persistence rules change for internal interaction toggles.
- Water render control semantics, view modes, or legacy compatibility behavior change.
- Compactness, particle seeding, liquid-cell inclusion, or pressure-quality semantics
  change.

## See Also

- `app-shell.md` - consumes registry accessors and owns the reset path.
- `web-shell.md` - renders controls from `config_json` and owns localStorage.
- `simulation.md` - owns compactness and liquid-cell simulation semantics.
- `profiler.md` - owns `stats_json` timing/readback semantics.
- `../decisions/observability.md` - registry and panel rationale.
- `../agent-context/maintaining-docs.md` - doc maintenance rules.
