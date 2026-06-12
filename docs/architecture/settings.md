---
status:        active
owner:         adamg
last_updated:  2026-06-12
okay_to_delete: false
long_lived:    true
---

# Settings registry

The settings registry is the single typed, schema-driven config source for tunable
parameters in fluid-lab. Each parameter is declared once in
`crates/fluid-lab/src/settings/mod.rs -> Registry`; the JS panel, localStorage replay,
and runtime setters read from or write through that registry instead of defining
settings independently.

## What it owns

`crates/fluid-lab/src/settings/mod.rs -> Setting` owns each row's stable id, label,
semantic `Category`, numeric value type/default/bounds, optional help copy, and
`ApplyClass`. Functional settings-tab metadata is registry-owned through
`crates/fluid-lab/src/settings/mod.rs -> settings_tab`, which maps each row to a
`SettingsTab` (`tab`, `tab_label`, `tab_order`, `tab_group`, and `tab_variant` in
`config_json`). The web shell does not maintain an independent product taxonomy.

Help is optional. `tooltip` is short functional help; `technical_tooltip` is technical
help about wiring, apply-class implications, or caveats. Absent help fields are omitted
from `config_json`.

Persistence is a web-shell concern. The registry holds canonical runtime state; the
panel layers saved values on load by calling `set_setting` for each known persisted id.
The shell may hide or suppress persistence for user-invisible internal controls while
keeping them as real registry settings.

## Apply classes

`crates/fluid-lab/src/settings/mod.rs -> ApplyClass` is a three-variant enum:

- `Live` changes are pushed to the running sim immediately and `set_setting` returns
  `true`.
- `Reset` changes are stored in the registry but require `app.reset()` to rebuild
  buffers, scene data, or baked-at-init constants.
- `Reload` changes are stored but require a page/device reload.

Use `Reset` for buffer allocation, scene rebuild, fixed timestep, max-substep, and
detailed-profiling changes; use `Reload` for device/feature/threading settings; use
`Live` only when the running GPU/app state can be updated safely.

## JS bridge

The settings bridge lives in `crates/fluid-lab/src/lib.rs -> FluidApp::config_json` and
`FluidApp::set_setting`, with serialization in
`crates/fluid-lab/src/settings/mod.rs -> Registry::config_json`.

Each `config_json()` entry carries stable control fields (`id`, `label`, `category`,
`tab`, `tab_label`, `tab_order`, `tab_group`, `tab_variant`, `type`, `value`, `default`,
`min`, `max`, and `apply`). Optional fields are emitted only when present:

- `tooltip` and `technical_tooltip`.
- `options` for enum/dropdown settings from
  `crates/fluid-lab/src/settings/mod.rs -> enum_options`.
- `slider_scale` for panel hints from `slider_scale` (`log2` particle-count slider,
  `color` packed RGB color picker).

`set_setting(id, value: f64)` validates and stores the value. For Live settings it also
dispatches the matching GPU/app setter and returns `true`; for Reset/Reload settings it
returns `false` after storing the value, so the caller can show an honest reset/reload
hint.

## Functional tabs

The product-facing tabs are owned by `SettingsTab` in the registry and rendered by
`web/panels.js`. `tab_group` lets the shell cluster Setup, Core, and Optional Effects
without hardcoding feature groups; `tab_variant: "experimental"` marks optional render
effects for subdued navigation styling.

- Scenario — `scene.*`, `grid.*`, and `particles.count`.
- Simulation — physics, liquid classification, pressure solver, and diagnostics
  controls.
- Modes — auto-roll and wave-maker tuning, excluding hidden scheduler enables.
- Camera & View — camera pose/distance plus view/FPS controls.
- Water Surface, Water Color, Environment, Sun & Reflection — screen-space water and
  environment controls split by user-facing function.
- Wall Fill, Wet Wall, Caustics, Temporal, Diffuse Water — focused render feature
  groups for larger `render.hero.*` / `render.diffuse.*` blocks.

The Profiler tab is appended by the web shell and is not a config tab; it does not
participate in tab-level reset.

## Scenario settings

`scene.preset` is a Reset-class enum for the starting liquid layout. The web panel
auto-resets after changing it because changing the scenario without rebuilding would be
misleading.

`scene.drop_height` is a Reset-class normalized height control. It shifts suspended
presets during `crates/fluid-lab/src/scene/mod.rs -> SceneConfig::from_settings` while
clamping the full block inside `[0,1]` and preserving block size. Falling Blob and
Double Splash respond to it; Dam Break remains floor-anchored and the setting help says
its effect is limited for that preset. The default reproduces the current authored
blocks.

`grid.res_x/y/z` and `particles.count` remain Reset-class because they change fluid
allocation and initial seeding.

## Interaction controls

The Interaction settings are Live app controls:

- `interaction.auto_roll_*` controls deterministic automatic tank rocking in
  `FluidApp`.
- `interaction.wave_*` controls periodic local horizontal impulses through the
  existing particle impulse pass.

The `*_enabled` booleans are hidden shell scheduler state. `web/panels.js ->
HIDDEN_SETTING_IDS` suppresses them from UI rows and from persisted visible settings.
`web/main.js` owns Mode selection (Auto rotate / Waves / Manual) and writes those
booleans through `set_setting(...)`. The Modes tab exposes only strength/cadence and
strength/frequency tuning.

## Render controls

Water and environment controls are Live unless they resize GPU resources. Most
`render.hero.*` settings share one snapshot path: `FluidApp::set_setting` matches the
`render.hero.` prefix, rebuilds `Registry::hero_params() -> HeroParams`, and pushes it
through `GpuContext::set_hero_params`. The visible hero-water core is split into
`render.hero.refraction_enabled`, `render.hero.reflection_enabled`,
`render.hero.body_color_enabled`, and `render.hero.wall_contact_enabled`; the hidden
legacy `render.hero.mode_enabled` id still replays by setting only refraction,
reflection, and body-color. Reserved/no-op caustics controls and temporal jitter are
hidden from `config_json` but accepted as no-op legacy ids so old localStorage payloads
do not break startup.

`render.diffuse.*` uses the same pattern via `Registry::diffuse_params() ->
DiffuseParams` and `GpuContext::set_diffuse_params`. `render.diffuse.max_particles` is
an active cap inside a fixed buffer, so it applies Live.

Weak optional render effects are available but default off: caustics, temporal,
diffuse water, wet walls, and dense wall fill. Controls that resize render-side buffers
are Reset-class, such as wet-wall supersample and wall-fill supersample. Their registry
rows store the value immediately; Reset performs the allocation.

`render.particle_alpha` is not serialized into `config_json`. `FluidApp::set_setting`
accepts the legacy id as a no-op redirect to `render.water_optical_density`.

## Gotchas

- The registry is append-safe: lookups and mutations are by id, not row index.
- `set_value_f64` clamps instead of rejecting out-of-range values.
- The declared `Value` variant fixes the stored type; U32 settings round incoming f64s
  before storage, and F32 settings cast to f32.
- Reset restores tank pose and interaction schedules but preserves Live interaction
  setting values; the web shell reapplies current Mode after reset.
- Reload restores persisted visible settings, then the shell reapplies Auto rotate as
  the product default. Hidden enable booleans are not durable user preferences.
- The web shell accepts a short allowlist of hidden legacy render ids during restore;
  the ids are omitted on the next save because persistence is rebuilt from visible
  non-default rows.
- There is no `solver.density` setting. The CG pressure solve uses a hardcoded density
  internally; see `../decisions/pressure.md`.

## Update when

- A tunable is added, removed, regrouped, recategorized, or has its apply class changed.
- The JSON bridge shape changes, including tab metadata, help fields, enum options, or
  slider hints.
- Interaction control semantics, hidden-setting persistence rules, or scene reset
  settings change.
- Water render control semantics, view modes, or legacy compatibility behavior change.

## See also

- `app-shell.md` — consumes registry accessors and owns reset behavior.
- `web-shell.md` — renders controls from `config_json` and owns localStorage.
- `simulation.md` — compactness and liquid-cell simulation semantics.
- `profiler.md` — `stats_json` timing/readback semantics.
- `../decisions/observability.md` — registry and panel rationale.
- `../agent-context/maintaining-docs.md` — doc maintenance rules.
