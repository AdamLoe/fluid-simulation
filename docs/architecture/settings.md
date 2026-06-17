---
status:        active
owner:         adamg
last_updated:  2026-06-17
okay_to_delete: false
long_lived:    true
---

# Settings registry

The settings registry is the single typed, schema-driven config source for
fluid-lab. Each parameter is declared once in
`crates/fluid-lab/src/settings/mod.rs -> Registry`; the JS panel, localStorage
replay, and runtime setters read from or write through that registry.

## What it owns

`Setting` owns each row's stable id, label, category, numeric type/default/bounds,
optional help copy, and `ApplyClass`. `settings_tab` maps each visible row to a
registry-owned tab (`tab`, `tab_label`, `tab_order`, `tab_group`, `tab_variant`) in
`config_json`.

`config_json()` emits registry rows with visibility and tab metadata. The web panel
renders only visible durable rows and skips hidden scheduler/compatibility controls.
Optional fields include `tooltip`, `technical_tooltip`, `options`, and
`slider_scale`.

`set_setting_result_json(id, value)` is the honest mutation bridge. It rejects
non-finite values, validates finite values through the registry, stores the clamped
value when accepted, and returns a `SettingMutationResult` payload:

```json
{"ok":true,"status":"applied","id":"physics.cfl","requested_id":"physics.cfl","requested_value":99,"stored_value":6,"clamped":true,"apply":"live","applied_live":true,"needs_reset":false,"needs_reload":false}
```

The status/apply vocabulary is owned by
`crates/fluid-lab/src/settings/mod.rs -> MutationStatus` and
`crates/fluid-lab/src/settings/mod.rs -> ApplyClass`. Live settings push directly
into app/GPU state and set `applied_live`; Reset/Reload settings only store the
value and report `needs_reset` / `needs_reload`. The old
`set_setting(id, value) -> bool` remains a compatibility wrapper: `true` means the
id was accepted as Live-class, not that every possible outcome was distinguishable.

`physics.max_substeps` is the Reset-class frame catch-up cap. Its registry default is
2, which lets a 60 Hz frame execute two 1/120 s physics substeps when the frame budget
allows; the timestep controller still drops excess accumulated time when the natural
substep count exceeds the cap.

### Particle density and derived count

`scene.fill_level` is a Reset-class scenario amount percentage. `Registry::fill_level()`
maps the stored 0-100 value to a `[0,1]` amount, and each preset maps that amount
through its authored geometry rather than a universal whole-tank volume. Falling Blob
scales as a suspended body, Dam Break gets taller within its wall footprint, and
Double Splash stretches its two suspended drops. The 0% setting keeps a tiny
compatibility seed so the existing particle/GPU paths remain valid.

The seeded particle count is **derived**, not a fixed absolute number. The primary
control is `particles.density` (Reset-class `f32`, default `8`, range `1..32`): the
target particles-per-seeded-cell crowding of the represented liquid at reset.

"Per cell" means **per seeded fluid cell**, not per total grid cell. The seeded region
is the liquid-block volume measured in cells, i.e. `seeded_volume_fraction *
res_x*res_y*res_z`, where `seeded_volume_fraction` is the fraction of the normalized
[0,1]^3 tank the scenario's liquid blocks occupy. The requested count is
`round(density * seeded_volume_fraction * total_cells)`, floored at 1024 for
very-small compatibility seeds. This keeps the default `80×40×80` falling-blob scene
near the historical particle budget and scales correctly when grid resolution,
preset-authored scenario amount, or water amount changes. The
derivation lives in `crates/fluid-lab/src/scene/mod.rs -> resolved_particle_count`;
`gpu` reads the resolved `SceneConfig::particle_count`, so validation and the reported
"requested" count agree.

The deterministic lattice can generate slightly fewer particles than requested
because per-axis lattice counts are floored. Runtime rest density, auto
`classify.surface_dilation`, and render splat spacing intentionally use the requested
effective density so the reference value `8` remains the tuned visual baseline;
`stats_json` still exposes requested, estimated/generated, and actual particle counts
so captures can show the drift.

`particles.count` is a **hidden compatibility override** (Reset-class `u32`, default
`0` = Auto, range `0..134_217_728`). Rust still accepts it from old localStorage,
URLs, imports, and capture harness setup; `0` means derive from `particles.density`,
while a nonzero value pins an exact requested absolute count and ignores density. The
generated lattice can still trail that requested count. The registry accessors are
`particle_density()` and `particle_count_override()`; resolution happens in
`SceneConfig::from_settings`. The web shell does not render, export, persist, or
share this id.

Grid resolution (`grid.res_x/y/z`) and `particles.density` live in the Scenario tab.
The resolved effective particle count remains in `stats_json` and the Profiler, but
the settings shell no longer renders a Scenario summary row.

`solver.pressure_residual_tolerance` is a Live `f32` pressure setting. Default `0`
means disabled; finite inputs clamp to the registry's conservative relative-residual
range before the GPU scalar tolerance slot is updated.

`solver.pressure_warm_start` is a Live `u32` boolean pressure setting. Default `1`
lets the GPU pressure init reuse the previous pressure field as the initial CG guess;
`0` restores the zero-start pressure solve. Reset and rebuild paths clear the pressure
field before reuse.

Portable config payloads use the same persistence version as localStorage:

```json
{"schema":"fluidlab.config.v1","settings":{"physics.cfl":6}}
```

`settings` is a registry-id to numeric-value map over visible non-default rows. Import
callers pass each entry through `set_setting_result_json`; the registry still owns
clamping, unknown-id rejection, reset/reload classification, and legacy-id handling.
Enum settings currently serialize as their numeric registry values, not stable slugs.

## Functional tabs

The visible config-tab routing lives in
`crates/fluid-lab/src/settings/mod.rs -> settings_tab`. Registry-owned tabs are:
Scenario, Simulation, Camera, Surface, Smoothing, Whitewater, Color, Environment,
Refraction, and Reflection.
The web shell appends Profiler as a non-config tab and appends a shell-owned Theme tab
only when `?dev=true`. Environment is also hidden outside dev mode. Removed
render-feature tabs are not visible; `rendering.md` owns the removed-feature set,
while this doc owns registry and legacy-id behavior.

Scenario contains the scene, grid, particle-density, Auto Roll, and Wave sections.
The product-mode launcher owns the hidden scheduler enable toggles; the panel exposes
only the strength/cadence/frequency rows users can tune.

The Camera tab default pitch is `camera.rot_x = -0.3` so the initial view sees more of
the floor and water surface. The old tab ids `modes`, `camera-view`, `water-surface`,
`water-color`, and `sun-reflection` are shell aliases only; they are not registry tab
ids.

## Render controls

Most `render.hero.*` controls share one Live snapshot path:
`crates/fluid-lab/src/settings/mod.rs -> Registry::hero_params`, then
`crates/fluid-lab/src/gpu/mod.rs -> GpuContext::set_hero_params`. Core water toggles
are declared with the rest of the hero controls in
`crates/fluid-lab/src/settings/mod.rs -> Registry`.

Screen-space surface quality lives in the Smoothing tab and is tuned by the
`render.hero.smooth_*`, `render.hero.normal_*`, and
`render.hero.feature_preservation` Live controls. `render.hero.smooth_iterations = 0`
is the explicit off state; nonzero values run the smoothing passes. Feature
preservation still drives the curvature-adaptive filter (smooth faces + sharp crests;
0 = legacy isotropic behaviour) — see [`rendering.md`](rendering.md).

Persistent surface foam, flat-water wall-contact correction, and micronormal controls
are not settings anymore. `render.diffuse.*`,
`render.hero.wall_contact_enabled`, `render.hero.micro_normal_*`, and
`render.hero.flat_water.*` are removed ids and current imports reject them as unknown.
The Whitewater tab owns `render.whitewater_strength`,
`render.whitewater_threshold`, and `render.whitewater_softness`, which continue to use
the speed-weighted whitewater accumulation target in the Water composite.

## Legacy replay

Old persisted settings must not break startup. JavaScript no longer mirrors a legacy
allow-list; restore/import submits ids to the bridge and Rust decides whether to
apply, map, ignore, or reject them. `rendering.md` owns which removed render feature
families are absent; `crates/fluid-lab/src/settings/mod.rs ->
legacy_hidden_setting_id` owns the accepted legacy-id set. `render.hero.mode_enabled`
maps only the core optical toggles; removed caustic, temporal, wet-wall, dense
wall-fill, and obsolete diffuse spray/bubble/wall-impact ids replay as hidden
compatibility no-ops. The later persistent-foam ids (`render.diffuse.*`) and removed
flat-water/micronormal ids are no longer accepted.

Future saves walk visible non-default rows only, so removed ids drop out naturally.

`render.particle_alpha` is also accepted as a legacy no-op redirect to
`render.water_optical_density`. It reports `legacy_ignored`, fixing old localStorage
payloads that used to be skipped before Rust saw them.

## Gotchas

- Registry lookups and mutations are by id, not row index.
- `set_value_f64_result` rejects non-finite values, then clamps finite values to the
  row's declared bounds and reports whether clamping happened. `set_value_f64` is the
  legacy bool wrapper.
- U32 settings round incoming f64s before storage; F32 settings cast to f32.
- Hidden scheduler booleans (`interaction.auto_roll_enabled`,
  `interaction.wave_enabled`) and `particles.count` are real settings but not visible
  durable preferences.
- There is no `solver.density` setting.
- `technical_tooltip` metadata may still be emitted by Rust for docs/debug consumers,
  but the web shell renders only functional help affordances.

## Update when

- A tunable is added, removed, regrouped, or changes apply class.
- `config_json` shape, tab metadata, help fields, enum options, or slider hints change.
- Legacy compatibility behavior changes.

## See also

- `web-shell.md`
- `rendering.md`
- `profiler.md`
- `../decisions/observability.md`
- `../agent-context/maintaining-docs.md`
