---
status:        active
owner:         adamg
last_updated:  2026-06-05
okay_to_delete: false
long_lived:    true
---

# Settings registry

The settings registry is the single typed, schema-driven config source for every
tunable parameter in fluid-lab. Each parameter is declared exactly once with an id,
label, category, type, default, validation bounds, tooltip, and apply class. There is
no secondary config location. The JS control panel and any persistence layer are
validated override layers on top of this registry — they read from it, they write
through it, and they never define settings independently.

The full registry is `app/crates/fluid-lab/src/settings/mod.rs → Registry` (the `Default` impl is the
authoritative table). The `Setting` struct is the per-entry type.

## What it owns

Every tunable has: a stable string `id` (dot-namespaced, e.g. `physics.gravity`), a
`Category` (the variants are the `Category` enum in `app/crates/fluid-lab/src/settings/mod.rs`), a `Value` (either
`Value::U32` or `Value::F32`), inclusive `Validation` bounds, a tooltip string, and
an `ApplyClass`. The tooltip is a single string following a two-part convention —
plain-language effect, an em-dash separator, then the technical detail
(`"<effect> — <detail>"`); this is a content convention only, not a schema field
split. Values are stored and mutated in the registry; typed accessors on
`Registry` (e.g. `grid_res_x/y/z()`, `flip_blend()`, `camera_distance()`, `mesh_enabled()`, `detailed_gpu_profiling()`) read them
back for the sim.

The full `Category` list (serialized as the `category` string in `config_json`) is: `Scene / Grid / Particles / Physics / Solver / Camera / Render / Dev`. The `Dev` category groups dev/debug toggles that are off by default and carry a Reset apply class.

Persistence (localStorage) is a panel / web-shell concern, not the registry's. The
registry holds the canonical runtime state; the panel may layer saved values on top
at load time by calling `set_setting` for each persisted id.

## Apply classes

`app/crates/fluid-lab/src/settings/mod.rs → ApplyClass` is a three-variant enum:

- `Live` — the change takes effect immediately during the current run. `set_setting`
  pushes it to the GPU right away and returns `true`.
- `Reset` — requires a simulation reset: buffer reallocation, scene rebuild, and
  re-seeding from the new values. `set_setting` stores the new value in the registry
  and returns `false`; the panel is expected to show a "needs reset" cue and trigger
  the reset.
- `Reload` — requires a full page/device reload (device/adapter feature selection,
  threading mode changes). `set_setting` stores the value and returns `false`.

**Assignment rule:** mark `Reset` when changing a setting requires buffer realloc or
baked-at-init constants (grid resolution, particle count, fixed timestep,
max substeps, `dev.mesh_enabled`, `dev.detailed_gpu_profiling`). Mark `Reload` for device/feature/threading settings. Mark `Live` for
everything else. Never force a structurally unsafe setting to apply live.

**Current wiring state (important gotcha):** Reset-class settings are stored in the
registry when `set_setting` is called, but the actual recreate-fluid-from-settings
path (buffer realloc + scene rebuild) is NOT triggered automatically by `set_setting`.
It is triggered by the caller (the JS panel) invoking a separate reset on the app.
Until that reset happens, the GPU is running with the old buffers. This is by design —
the panel batches multiple Reset changes before a single reset — but it means a
Reset-class setting read from the registry after `set_setting` reflects the new
intended value, while the running sim still uses the pre-reset allocation.

## The JS bridge

Three `#[wasm_bindgen]` methods on `FluidApp` are the entire settings surface exposed
to TypeScript. All three are in `app/crates/fluid-lab/src/lib.rs → set_setting / config_json / stats_json`.

`config_json()` serializes the full registry to a JSON array. Each entry carries:
`id`, `label`, `category`, `type` (`"u32"` or `"f32"`), `value`, `default`, `min`,
`max`, `apply`, `tooltip`, and two optional fields: `options` (for enum-valued
settings that render as dropdowns — currently only `scene.preset`) and `slider_scale`
(a non-linear scale hint for the slider — currently only `"log2"` on
`particles.count`). The panel reads this once at init to construct all controls; it
never hard-codes control metadata.

`set_setting(id, value: f64)` writes a validated value into the registry. For Live
settings it additionally dispatches the change to the GPU immediately via the
appropriate `gpu.*` setter and returns `true`. For Reset/Reload settings it returns
`false`. Unknown ids return `false` without modifying anything.

`stats_json()` returns profiler and GPU timing data; it reads `grid_res_x()` and
`gpu.particle_count()` from the registry and GPU state respectively. It is a read-only
observer — it does not write settings.

## Non-obvious invariants and gotchas

- The registry is append-safe: new settings can be added to the `Default` vec without
  breaking existing ids. Lookups (`get`, `set_value_f64`, typed accessors) are by id
  string, not by index.
- `set_value_f64` clamps the incoming f64 to the setting's declared bounds before
  storing. There is no path to store an out-of-range value. The clamp is the only
  validation; there is no reject/error path — a clamped value silently takes effect.
- The `Value` variant (U32 vs F32) is fixed at declaration time. `set_value_f64`
  preserves it: a U32 setting rounds the incoming f64 and stores a `Value::U32`; an
  F32 setting casts and stores a `Value::F32`. Cross-variant mismatches in validation
  are handled conservatively but this situation should not arise for correctly declared
  settings.
- The `options` field in `config_json` output exists only for settings with an entry
  in `app/crates/fluid-lab/src/settings/mod.rs → enum_options`. Currently only `scene.preset` has options.
  Adding a new dropdown-rendered setting requires adding an arm there.
- The `slider_scale` field is emitted only for settings with an entry in
  `app/crates/fluid-lab/src/settings/mod.rs → slider_scale`. Currently only `particles.count` returns
  `"log2"`. It makes the panel render that setting's slider in exponent space (each
  notch doubles the value) so one slider spans a huge range; the number input still
  accepts any exact value in `[min, max]`. `particles.count` ranges `1_024` (2^10) to
  `134_217_728` (2^27, the smallest power of two over 100M) — both endpoints are
  powers of two so the log2 stepping is clean.
- `physics.max_substeps` defaults to 1 (range 1–16). The default of 1 prefers interactivity — excess accumulated sim time is dropped and the browser catches up by rendering the next frame (see `decisions/performance.md`). Raise to 4 for dev/stress catch-up testing.
- `dev.mesh_enabled` (U32 0/1, default 0, Reset-class) — controls lazy allocation of the ~73 MB MC GPU resources. Accessor: `mesh_enabled() → bool`.
- The marching-cubes water look is tuned by Live, mesh-only settings: `render.mesh_iso`, `render.mesh_smooth` (u32 blur iterations), `render.mesh_opacity`, `render.mesh_fresnel`, `render.mesh_foam`, plus the volume-shading pair `render.water_absorb` (Beer-Lambert depth-tint strength, default 2.5) and `render.water_refract` (screen-space refraction strength, default 0.6). They no-op until `dev.mesh_enabled` is on (the `gpu.set_mesh_*` / `gpu.set_water_*` setters guard on `Option<MeshExtractor>`, and `MeshLook` — which now also carries `absorb`/`refract` — preserves them across (re)allocation). See `architecture/rendering.md`.
- `physics.cfl` (F32, default 2.0, Live) — the CFL number = max grid cells a particle may cross per substep; raises the velocity ceiling so splash height does not shrink as the grid is refined. Writes `Params.cls[2]`; see `architecture/simulation.md`.
- `dev.detailed_gpu_profiling` (U32 0/1, default 0, Reset-class) — switches `GpuTimers` from coarse (3 passes/substep) to detailed (one pass per fine section + per-CG-iter timing). Accessor: `detailed_gpu_profiling() → bool`.
- The colored apply-class dots (green/yellow/red) are rendered by the web panel from
  the `apply` string in `config_json`. They are not stored in the registry data model.
- There is no `solver.density` in the current registry; the CG solver uses a hardcoded
  `rho = 1000` internally. (The pressure solve is scale-consistent, so ρ has no visible
  effect — see `../decisions/pressure.md`.)

## Update when

- A new tunable is added: add a `Setting` entry to the `Registry::Default` vec, pick
  an apply class, add a typed accessor, and wire the Live path in
  `app/crates/fluid-lab/src/lib.rs → set_setting` if it is Live.
- A setting's apply class changes: update the `apply` field and audit whether
  `set_setting`'s Live dispatch branch needs adding or removing.
- A new enum-valued setting is added: add an arm to
  `app/crates/fluid-lab/src/settings/mod.rs → enum_options`.
- The JS bridge shape changes: update `app/crates/fluid-lab/src/settings/mod.rs → Registry::config_json`
  and the corresponding TypeScript consumer in the web shell.

## See also

- `architecture/app-shell.md` — consumes registry accessors to build the scene and
  drive the simulation loop; owns the reset path that re-reads Reset-class settings.
- `web-shell.md` — renders controls from `config_json`; owns localStorage persistence
  and the "needs reset" / "needs reload" UI cues.
- `profiler.md` — `stats_json` is the read path; a config snapshot may be embedded.
- `../decisions/observability.md` — the split between registry data model (here) and
  rendered panel (web shell, phase 1.2).
- `../agent-context/maintaining-docs.md` — always consult before editing any arch doc.
