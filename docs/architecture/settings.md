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

`config_json()` emits visible settings only. Optional fields include `tooltip`,
`technical_tooltip`, `options`, and `slider_scale`.

`set_setting(id, value)` validates and stores through the registry. Live settings
push directly into app/GPU state and return `true`; Reset/Reload settings store the
value and return `false`.

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

The web shell appends Profiler as a non-config tab. Caustics, Temporal, Wet Wall,
Wall Fill, and Diffuse Water are not visible tabs.

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

`render.diffuse.*` now means surface foam. It shares one Live snapshot path:
`Registry::diffuse_params() -> DiffuseParams`, then
`GpuContext::set_diffuse_params`. Visible foam controls cover enable, active particle
cap, emission rate/budget, surface-speed onset/gain, lifetime, radius, opacity, and
random seed. There are no visible spray, bubble, wall-impact, or diffuse debug
settings.

## Legacy replay

Old persisted settings must not break startup. `legacy_hidden_setting_id` accepts and
ignores removed render ids:

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

`web/panels.js` mirrors that compatibility during localStorage restore. Future saves
walk visible non-default rows only, so removed ids drop out naturally.

`render.particle_alpha` is also accepted as a legacy no-op redirect to
`render.water_optical_density`.

## Gotchas

- Registry lookups and mutations are by id, not row index.
- `set_value_f64` clamps values instead of rejecting them.
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
