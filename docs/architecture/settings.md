---
status:        active
owner:         adamg
last_updated:  2026-06-12
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
value when accepted, and returns:

```json
{"ok":true,"status":"applied","id":"physics.cfl","requested_id":"physics.cfl","requested_value":99,"stored_value":6,"clamped":true,"apply":"live","applied_live":true,"needs_reset":false,"needs_reload":false}
```

`status` is one of `applied`, `stored`, `unknown_id`, `non_finite_rejected`,
`legacy_mapped`, or `legacy_ignored`. `apply` is `live`, `reset`, `reload`, or
`null` when the id is unknown/non-finite. Live settings push directly into app/GPU
state and set `applied_live`; Reset/Reload settings only store the value and report
`needs_reset` / `needs_reload`. The old `set_setting(id, value) -> bool` remains a
compatibility wrapper: `true` means the id was accepted as Live-class, not that every
possible outcome was distinguishable.

`physics.max_substeps` is the Reset-class frame catch-up cap. Its registry default is
2, which lets a 60 Hz frame execute two 1/120 s physics substeps when the frame budget
allows; the timestep controller still drops excess accumulated time when the natural
substep count exceeds the cap.

### Particle density and derived count

The seeded particle count is **derived**, not a fixed absolute number. The primary
control is `particles.density` (Reset-class `f32`, default `8`, range `1..32`): the
particles-per-cell crowding of the seeded liquid at reset.

"Per cell" means **per seeded fluid cell**, not per total grid cell. The seeded region
is the liquid-block volume measured in cells, i.e. `seeded_volume_fraction *
res_x*res_y*res_z`, where `seeded_volume_fraction` is the fraction of the normalized
[0,1]^3 tank the scenario's liquid blocks occupy. The resolved count is
`round(density * seeded_volume_fraction * total_cells)`, floored at 1024. This keeps
the default 64³ falling-blob scene near the historical ~254k particles (~264k at
density 8) and scales correctly when grid resolution or scenario changes. The
derivation lives in `scene/mod.rs::resolved_particle_count`; `gpu` reads the resolved
`SceneConfig::particle_count`, so seeding, validation, and the reported "requested"
count all agree.

`particles.count` is now an **advanced manual override** (Reset-class `u32`, default
`0` = Auto, range `0..134_217_728`). `0` means derive from `particles.density`; a
nonzero value pins an exact absolute count and ignores density. The registry accessors
are `particle_density()` and `particle_count_override()`; resolution happens in
`SceneConfig::from_settings`. The override no longer uses a log2 slider.

Grid resolution (`grid.res_x/y/z`) and `particles.density` are the prominent
Scenario-tab controls; the web panel shows the resolved effective particle count in a
read-only summary at the top of that tab (sourced from `stats_json`), so users can see
what a density yields before reset.

`solver.pressure_residual_tolerance` is a Live `f32` pressure setting. Default `0`
means disabled; finite inputs clamp to the registry's conservative relative-residual
range before the GPU scalar tolerance slot is updated.

`solver.pressure_warm_start` is a Live `u32` boolean pressure setting. Default `0`
keeps the zero-start pressure solve and comparable default captures. `1` lets the
GPU pressure init reuse the previous pressure field as the initial CG guess; reset
and rebuild paths clear the pressure field before reuse.

Portable config payloads use the same persistence version as localStorage:

```json
{"schema":"fluidlab.config.v1","settings":{"physics.cfl":6}}
```

`settings` is a registry-id to numeric-value map over visible non-default rows. Import
callers pass each entry through `set_setting_result_json`; the registry still owns
clamping, unknown-id rejection, reset/reload classification, and legacy-id handling.
Enum settings currently serialize as their numeric registry values, not stable slugs.

## Functional tabs

The visible tabs are:

- Scenario
- Simulation
- Modes
- Camera & View
- Water Surface
- Water Color
- Environment
- Sun & Reflection
- Foam

The web shell appends Profiler as a non-config tab. Removed render-feature tabs are
not visible; `rendering.md` owns the removed-feature set, while this doc owns
registry and legacy-id behavior.

## Render controls

Most `render.hero.*` controls share one Live snapshot path:
`Registry::hero_params() -> HeroParams`, then `GpuContext::set_hero_params`.
Visible core water toggles are:

- `render.hero.refraction_enabled`
- `render.hero.reflection_enabled`
- `render.hero.body_color_enabled`
- `render.hero.wall_contact_enabled`

The cheap wall-contact snap remains visible through
`render.hero.flat_water.strength`, `render.hero.flat_water.epsilon`, and
`render.hero.flat_water.depth_strength`.

Screen-space surface quality is tuned by the `render.hero.smooth_*`,
`render.hero.normal_*`, and `render.hero.feature_preservation` Live controls. The last
drives the curvature-adaptive feature-preserving filter (smooth faces + sharp crests; 0
= legacy isotropic behaviour) — see [`rendering.md`](rendering.md).

`render.diffuse.*` now means surface foam. It shares one Live snapshot path:
`Registry::diffuse_params() -> DiffuseParams`, then
`GpuContext::set_diffuse_params`. Visible foam controls cover enable, active particle
cap, emission rate/budget, surface-speed onset/gain, lifetime, radius, opacity, and
random seed. There are no visible spray, bubble, wall-impact, or diffuse debug
settings.

## Legacy replay

Old persisted settings must not break startup. JavaScript no longer mirrors a legacy
allow-list; restore/import submits ids to the bridge and Rust decides whether to
apply, map, ignore, or reject them. `rendering.md` owns which removed render feature
families are absent; `legacy_hidden_setting_id` accepts and ignores their persisted
ids:

- `render.hero.mode_enabled` maps only the core optical toggles.
- `render.hero.caustics.*`
- `render.hero.temporal.*`
- `render.hero.wet_wall.*`
- dense wall-fill ids under `render.hero.flat_water.fill_*` plus
  `render.hero.flat_water.waterline_softness`
- obsolete diffuse ids:
  `render.diffuse.wall_impact_threshold`, `render.diffuse.wall_impact_gain`,
  `render.diffuse.spray_lifetime`, `render.diffuse.bubble_lifetime`,
  `render.diffuse.bubble_buoyancy`, `render.diffuse.spray_drag`,
  `render.diffuse.debug_view`

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
  `interaction.wave_enabled`) are real settings but not visible durable preferences.
- There is no `solver.density` setting.

## Update when

- A tunable is added, removed, regrouped, or changes apply class.
- `config_json` shape, tab metadata, help fields, enum options, or slider hints change.
- Legacy compatibility behavior changes.

## See also

- `web-shell.md`
- `rendering.md`
- `profiler.md`
- `../decisions/observability.md`
