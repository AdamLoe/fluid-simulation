---
status:        shipped
owner:         codex
last_updated:  2026-06-12
okay_to_delete: true
long_lived:    false
owning_docs:
  - architecture/rendering.md
  - architecture/settings.md
  - architecture/web-shell.md
  - architecture/gpu-resources.md
  - architecture/profiler.md
  - decisions/rendering.md
  - decisions/performance.md
---

# Phase 2 Water Foam And Dead-Feature Removal

## Mission

Implement the user's Phase 2 product decision after Phase 1 commit `54a1ecd`: keep
surface-level foam and make it credible, remove caustics, temporal stabilization, dense
wall fill, and wet walls from the runtime and visible settings for now, and simplify the
settings bar so it is just the navigator plus settings content with no navigator-collapse
chrome or extra surrounding decoration.

Done means the Water view no longer allocates, schedules, draws, or exposes the removed
feature groups; old persisted settings for those groups replay safely and disappear from
future visible saves; foam is a conservative surface-only effect with no spray, bubbles,
airborne particles, or wall decals; and the settings panel has no top collapse control or
redundant outer header/padding.

## Source Facts Used

- `code_root` is `app/`; visible/GPU changes require real browser capture through
  `app/tools/capture.mjs`.
- The worktree was already dirty before this plan, including
  `app/crates/fluid-lab/src/gpu/shaders/diffuse_{emit,update,render}.wgsl` and several
  docs. Implementers must read current files before editing and must not revert unrelated
  dirty work. If any file scheduled for deletion is dirty, inspect the diff and either
  migrate relevant user work into the surviving design or pause before deleting it.
- Phase 1 shipped with weak add-ons default-off but still present. This Phase 2 product
  decision supersedes Phase 1's temporary opt-in policy for caustics, temporal, wet
  walls, and dense wall fill.
- Current Water render scheduling in `gpu/mod.rs` owns first-class fields for
  `CausticsSystem`, `TemporalSystem`, `WetWallSystem`, `WallOccupancySystem`, and
  `WallFillRenderer`. Temporal passes run every Water frame even when disabled by
  alpha-zero uniforms, and downstream composite/caustics views are rebound around
  temporal stable views.
- `environment.rs` and `shaders/environment.wgsl` currently bind a wet-wall uniform and
  wetness storage buffer as group 1 and contain wall wetness, meniscus, gloss, streak,
  and contact-shadow shader code.
- Dense wall fill is separate from the cheap core wall-contact snap. Keep
  `render.hero.wall_contact_enabled` plus `render.hero.flat_water.strength`,
  `render.hero.flat_water.epsilon`, and `render.hero.flat_water.depth_strength`; remove
  only `render.hero.flat_water.fill_*`, the wall occupancy/fill renderer, the
  `wallfill_mask` target, and composite fill-only tuning/debug paths.
- Caustics ids live under `render.hero.caustics.*`; temporal ids under
  `render.hero.temporal.*`; wet-wall ids under `render.hero.wet_wall.*`; dense wall-fill
  ids under `render.hero.flat_water.fill_*` plus
  `render.hero.flat_water.waterline_softness`.
- Current diffuse water is a persistent GPU particle system in `gpu/diffuse.rs` and
  `shaders/diffuse_{emit,update,render}.wgsl` with foam, spray, and bubbles, plus
  profiler stats for foam/spray/bubble. The conservative Phase 2 path is to gut this
  system to foam-only and reuse its buffer/pass plumbing, not to replace it with another
  screen-space speed mask. The core composite already has a speed-weighted whitewater
  target; the missing effect is persistent surface foam.
- `web/index.html` currently contains `#settings-nav-toggle`, `.settings-header`,
  `.settings-panel.nav-collapsed` CSS, and outer settings-panel margin/header styling.
  `web/panels.js` wires the nav-collapse button and has a short replay allowlist for
  hidden legacy ids.

## Scope

In scope:

- Delete the caustics runtime and shaders:
  `gpu/caustics.rs`,
  `gpu/shaders/caustics_generate.wgsl`, and
  `gpu/shaders/caustics_composite.wgsl`.
- Delete the temporal runtime and shader:
  `gpu/temporal.rs` and `gpu/shaders/temporal_blend.wgsl`.
- Delete the wet-wall runtime and shader:
  `gpu/wetwall.rs` and `gpu/shaders/wetwall_update.wgsl`.
- Delete the dense wall-fill runtime and shader:
  `gpu/wallfill.rs` and `gpu/shaders/wallfill.wgsl`.
- Remove all allocation, fields, resize/recreate/reset hooks, per-frame update calls,
  pass scheduling, rebinds, shader bindings, debug views, profiler/capture claims, and
  settings rows for those deleted systems.
- Preserve safe replay for removed ids in Rust and the web shell so localStorage payloads
  from Phase 1 do not break startup.
- Convert the current diffuse system to surface foam only, using existing
  `gpu/diffuse.rs` and `diffuse_*.wgsl` plumbing unless current dirty shader work makes a
  smaller equivalent approach obvious. Visible behavior must be foam only.
- Remove settings nav collapse UI and simplify the settings panel chrome.
- Update owning docs at ship time so plans are not needed to understand the new runtime.

Out of scope:

- Reintroducing caustics, temporal reprojection/history, dense wall fill, wet-wall
  materials, spray, bubbles, airborne particles, wall decals, or particle-to-wall
  wetness.
- Replacing screen-space water with a mesh or marching-cubes surface.
- Changing FLIP/PIC simulation, pressure solve, P2G fixed-point atomics, tank topology,
  scene presets, pointer controls, or normal-frame readback policy.
- Renaming the entire internal diffuse module if that would expand churn. It may remain
  `diffuse.rs` internally while docs/UI call the surviving effect surface foam.

## Approach

Use one serial implementation. The same files own settings schema, render scheduling,
shader bindings, web settings persistence, profiler stats, and docs, so parallel agents
would collide.

### 1. Removed Feature Purge

Owned files:

- `crates/fluid-lab/src/gpu/mod.rs`
- `crates/fluid-lab/src/gpu/environment.rs`
- `crates/fluid-lab/src/gpu/composite.rs`
- `crates/fluid-lab/src/gpu/shaders/environment.wgsl`
- `crates/fluid-lab/src/gpu/shaders/composite.wgsl`
- deleted: `crates/fluid-lab/src/gpu/{caustics,temporal,wetwall,wallfill}.rs`
- deleted: `crates/fluid-lab/src/gpu/shaders/{caustics_generate,caustics_composite,temporal_blend,wetwall_update,wallfill}.wgsl`

Work:

- Remove `mod caustics`, `mod temporal`, `mod wetwall`, and `mod wallfill`.
- Remove corresponding `GpuContext` fields, construction, resize, recreate, reset,
  `set_hero_params`, camera update, per-frame update, render pass, and rebind code.
- Wire `CompositeRenderer` directly to raw smoothed targets: thickness, whitewater, and
  smooth-Z. Remove temporal stable target allocation and every per-frame temporal rebind.
- Remove caustics generation/composite passes and remove the caustics debug view.
- Remove `wallfill_mask_view`, wall-fill injection/clear passes, fill-only composite
  uniform fields, fill mask texture binding, and wallfill debug view.
- Simplify `EnvironmentRenderer` to one bind group: camera/material uniform only. Remove
  wet-wall bind group, wetness buffer/uniform arguments, wetness shader helpers, wet wall
  material response, meniscus, streak/gloss/darken, and wetness floor logic. Keep the
  floor/back/left wall environment geometry and ordinary material/reflection needed for
  core water.
- Keep the cheap wall-contact snap path in composite: `wall_contact_enabled`,
  `flat_water.strength`, `flat_water.epsilon`, and `flat_water.depth_strength`.

Acceptance:

- `rg` finds no runtime references to `CausticsSystem`, `TemporalSystem`,
  `WetWallSystem`, `WallOccupancySystem`, `WallFillRenderer`, `wallfill_mask`,
  `wetness_buf`, or deleted shader include files.
- Deleted modules and WGSL files are actually removed from disk, not left inert.
- Water mode still renders scene prepass, thickness/whitewater/smooth-Z smoothing,
  composite, optional foam, and optional grid slice.
- Composite debug options no longer name caustics or wallfill mask, and enum max values
  match the remaining options.

### 2. Settings Contract And Legacy Replay

Owned files:

- `crates/fluid-lab/src/settings/mod.rs`
- `crates/fluid-lab/src/lib.rs`
- `web/panels.js`
- tests in `settings/mod.rs`

Work:

- Remove visible settings and `HeroParams` fields for:
  - every `render.hero.caustics.*` id;
  - every `render.hero.temporal.*` id;
  - every `render.hero.wet_wall.*` id;
  - `render.hero.flat_water.fill_enabled`;
  - `render.hero.flat_water.fill_strength`;
  - `render.hero.flat_water.fill_slab`;
  - `render.hero.flat_water.fill_supersample`;
  - `render.hero.flat_water.fill_color_strength`;
  - `render.hero.flat_water.fill_reflection_strength`;
  - `render.hero.flat_water.fill_roughness`;
  - `render.hero.flat_water.fill_absorption_strength`;
  - `render.hero.flat_water.waterline_softness`.
- Remove `SettingsTab::WallFill`, `WetWall`, `Caustics`, and `Temporal`. Keep a Foam
  settings tab using the existing diffuse settings or a renamed tab label, but do not
  expose spray/bubble settings.
- Add a Rust-side removed-id compatibility function that accepts the removed ids as
  no-ops in `Registry::set_value_f64`. Keep existing compatibility for
  `render.hero.mode_enabled` mapping to the three core optical toggles.
- In `web/panels.js`, broaden replay compatibility so persisted removed ids are passed
  to Rust even though they are absent from `config_json`, then disappear on the next
  save because saving still walks visible settings only. A prefix/predicate helper is
  preferred over a long JS-only exact list, but Rust remains authoritative.
- Update tests so removed ids are accepted but absent from `config_json`, removed tabs
  are absent, `hero_params()` contains only surviving core water fields, and legacy
  `render.hero.mode_enabled` still does not touch wall contact or foam.

Acceptance:

- Startup with localStorage entries for every removed id does not throw, does not call
  reset/reload unnecessarily, and produces a subsequent saved config without those ids.
- `config_json` contains no caustics, temporal, wet-wall, or dense wall-fill rows/tabs.
- Core water rows, cheap wall-contact rows, environment rows, and foam rows still render.
- `set_setting` for removed ids returns `true` or otherwise replays safely; unknown ids
  outside the compatibility set still fail normally.

### 3. Foam-Only Surface Effect

Owned files:

- `crates/fluid-lab/src/gpu/diffuse.rs`
- `crates/fluid-lab/src/gpu/shaders/diffuse_emit.wgsl`
- `crates/fluid-lab/src/gpu/shaders/diffuse_update.wgsl`
- `crates/fluid-lab/src/gpu/shaders/diffuse_render.wgsl`
- `crates/fluid-lab/src/settings/mod.rs`
- `crates/fluid-lab/src/gpu/timing.rs`
- `crates/fluid-lab/src/profiler/mod.rs`
- `web/panels.js`

Decision:

- Gut the current diffuse particle system to foam-only rather than replacing it with a
  simpler screen-space mask. The current screen-space whitewater target is already the
  simple mask/fallback; the work needed now is a persistent, surface-constrained foam
  layer that can linger briefly without becoming spray, bubbles, or airborne confetti.

Work:

- Keep `DiffuseSystem` plumbing if that is the smallest path, but make behavior and docs
  foam-only:
  - emitter spawns only from liquid cells touching air;
  - no wall-impact spawn branch;
  - no bubble type;
  - no spray type;
  - particle type is either implicit foam or type 0 only;
  - update couples foam to local MAC flow only while it remains on/near liquid surface;
  - kill foam when it is no longer surface-adjacent, rises into air, hugs vertical glass
    above the floor, exceeds lifetime, or leaves the tank band;
  - render white/off-white soft billboards only, depth-tested over the composite.
- Make foam conservative by default:
  - default enabled if capture shows it improves the normal Water path without speckle;
  - low alpha/radius/cap/emission defaults;
  - no debug color-by-type as a product control unless useful for implementation smoke.
- Remove or hide/legacy-accept obsolete diffuse settings:
  `render.diffuse.wall_impact_threshold`, `render.diffuse.wall_impact_gain`,
  `render.diffuse.spray_lifetime`, `render.diffuse.bubble_lifetime`,
  `render.diffuse.bubble_buoyancy`, and `render.diffuse.spray_drag`.
  These ids must be accepted as Rust no-op compatibility ids, included in web replay
  compatibility, covered by tests, included in the removed-legacy replay capture, and
  absent from `config_json`.
- Keep a small visible foam control set:
  enabled, max/cap, emit rate, surface speed threshold/gain, foam lifetime, radius,
  alpha, and optionally random seed/debug if still needed for deterministic capture.
- Update stats/profiler JSON and panel wording to this contract: the UI shows foam count
  only; `stats_json.gpu.diffuse` may keep legacy `spray` and `bubble` fields only as
  zero-valued compatibility fields if removing them is higher risk. If kept, docs must
  state they are legacy-zero fields. The UI must not present spray or bubble as active
  features.

Acceptance:

- With foam enabled, captures after a moving-water scenario show soft surface foam
  patches on high-speed or breaking liquid surfaces.
- No foam appears as airborne particles, falling spray, rising bubbles, vertical wall
  decals, or random confetti.
- With foam disabled, the system skips emission/update/render work as before and the
  base screen-space whitewater fallback still works.
- Profiler/settings UI uses foam language and does not show spray/bubble controls or
  nonzero spray/bubble counts.

### 4. Settings Bar Cleanup

Owned files:

- `web/index.html`
- `web/panels.js`
- `web/main.js` only if helper state references need cleanup
- `tools/capture.mjs` only if capture helper assumptions change

Work:

- Remove `#settings-nav-toggle` from the DOM.
- Remove the `navToggle` lookup/listener from `web/panels.js`.
- Remove `.settings-panel.nav-collapsed` CSS and all collapsed-rail behavior.
- Remove the redundant settings header/top bar. The right panel should contain only
  `.settings-content`, with the navigator and settings body as the visible structure.
- Remove extra outer padding/chrome around the settings panel:
  - the panel may keep one border/background/shadow as the container;
  - navigator and settings body own their own internal padding;
  - no separate styled header band, no empty gutter, no collapse icon column, no extra
    top padding outside the navigator/body;
  - desktop panel should sit flush to the right edge/top-bottom layout chosen by the
    existing shell rather than floating with decorative margin unless needed for mobile
    overlap clearance.
- Keep panel open/close through the toolbar settings icon, tab grouping, active-tab
  state, Profiler polling only when visible, reset-to-default behavior, localStorage
  replay, and `window.__fluidShell` helpers.

Acceptance:

- There is no visible or accessible settings navigation collapse control.
- The class `nav-collapsed` does not appear in CSS or JS.
- Desktop settings capture shows only the navigator plus settings content inside the
  panel, with no separate top header bar or redundant padding around them.
- Narrow settings capture shows tabs/body/launcher do not overlap and tab text does not
  overflow incoherently.
- Bottom Mode/Control launcher behavior from Phase 1 remains intact.

## Sequencing

1. Start by inventorying dirty files and reading current diffs for touched files,
   especially the dirty diffuse shaders. Do not revert unrelated changes.
2. Remove caustics/temporal/wet-wall/wall-fill settings and legacy replay handling first,
   so code paths can compile against the final control contract.
3. Remove deleted render systems from `gpu/mod.rs`, `environment.rs`,
   `composite.rs`, and shaders. Keep the app compiling after each deletion batch if
   practical.
4. Convert diffuse to foam-only and update settings/profiler/UI language.
5. Remove settings nav collapse and outer settings-panel chrome.
6. Run code gates, browser captures, and docs migration before marking this plan shipped.
7. At closeout, set this plan to `status: shipped` and `okay_to_delete: true` only after
   durable facts are migrated; update `_meta/ownership.json` if ownership changes.

## Exit Gate

Required commands:

- `cd /home/adamg/fluid-simulation/app && cargo test --lib`
- `cd /home/adamg/fluid-simulation/app && cargo build --target wasm32-unknown-unknown`
- `cd /home/adamg/fluid-simulation/app && node --check web/panels.js`
- `cd /home/adamg/fluid-simulation/app && node --check web/main.js`
- `cd /home/adamg/fluid-simulation/app && node --check tools/capture.mjs`

Source assertions:

- `rg -n "CausticsSystem|TemporalSystem|WetWallSystem|WallOccupancySystem|WallFillRenderer|wallfill_mask|wetness_buf|render\\.hero\\.caustics|render\\.hero\\.temporal|render\\.hero\\.wet_wall|render\\.hero\\.flat_water\\.fill" app/crates/fluid-lab/src app/web` returns only deliberate compatibility/tests/docs references, not runtime or visible UI wiring.
- `rg -n "render\\.diffuse\\.(spray|bubble|wall_impact)" app/crates/fluid-lab/src app/web`
  returns only deliberate compatibility/tests/docs references, not runtime or visible UI
  wiring.
- Deleted feature modules/shaders are absent from disk.
- `config_json` has no removed feature tabs or ids.

Browser captures through `app/tools/capture.mjs`:

- `captures/phase2_default_water.png` - default Water view, desktop around 1440x900,
  after 4-6 seconds.
- `captures/phase2_foam_forced_on.png` - moving-water scenario with foam enabled if not
  already default-on.
- `captures/phase2_foam_forced_off.png` - same scene with foam disabled.
- `captures/phase2_settings_desktop.png` - settings panel open on Water Surface or Foam,
  desktop around 1440x900.
- `captures/phase2_settings_narrow.png` - settings panel open at around 390x844 with the
  launcher visible.
- `captures/phase2_removed_legacy_replay.png` - page loaded after injecting representative
  localStorage entries for removed ids; console must show no restore crash.

Capture health:

- `navigator.gpu` present, smoke PASS, no WGSL validation/device errors.
- EVAL-driven captures echo expected labels in console logs.
- No performance claim without profiler output. It is acceptable to report "removed
  passes/resources by code inspection" without claiming a measured speedup.

## Deferrals

- Caustics can return later only after core water reaches the desired appearance and a
  new plan defines a credible receiver/light model.
- Temporal can return later as real motion-vector reprojection or a clearly valuable
  stabilization pass, not alpha-zero always-on history plumbing.
- Dense wall fill and wet walls can return later only with evidence that they improve
  core water rather than masking core surface issues.
- Spray, bubbles, airborne particles, and particle-to-wall wetness are deferred. Phase 2
  foam is surface-only.
- Internal `diffuse` naming may be cleaned up in a later mechanical rename if desired;
  Phase 2 should not spend risk on that unless it is required for clarity in the touched
  settings/UI.

## Shipped Evidence

- Required Rust/wasm/node gates passed on 2026-06-12.
- Browser captures were taken through `app/tools/capture.mjs` for default water,
  foam-on, foam-off, desktop settings, narrow settings, and removed legacy replay.
- Source assertions found no removed runtime systems or visible settings wiring; remaining
  removed ids are deliberate hidden legacy replay compatibility and tests.

## Migration Notes

- `architecture/rendering.md` describes the removed runtime systems, the surviving core
  Water pass order, and foam-only behavior.
- `architecture/settings.md` documents removed-id compatibility, visible Foam controls,
  and the absence of caustics, temporal, wet-wall, and wall-fill tabs.
- `architecture/web-shell.md` documents the settings panel without navigator collapse or
  header chrome.
- `architecture/gpu-resources.md` removes caustics, temporal, wet-wall, and wall-fill
  allocations and describes the remaining foam buffer/counter allocation.
- `architecture/profiler.md` describes foam stats only and the intentionally
  legacy-compatible diffuse JSON fields left at zero.
- `decisions/rendering.md` records the Phase 2 decision: caustics, temporal, wet walls,
  and dense wall fill are removed for now; surface foam remains.
- `decisions/performance.md` records removal of always-allocated/history/fill render
  resources as a scope and cost-control decision without unsupported speedup claims.

## See Also

- `docs/plans/orchestrator/hero-water-ui-remediation.md`
- `docs/architecture/rendering.md`
- `docs/architecture/settings.md`
- `docs/architecture/web-shell.md`
- `docs/architecture/gpu-resources.md`
- `docs/decisions/rendering.md`
- `docs/decisions/performance.md`
