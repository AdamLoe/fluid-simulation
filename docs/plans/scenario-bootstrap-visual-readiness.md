---
status:        active
owner:         codex
last_updated:  2026-06-17
okay_to_delete: false
long_lived:    false
owning_docs:
  - architecture/app-shell.md
  - architecture/settings.md
  - architecture/simulation.md
  - architecture/web-shell.md
---

# Scenario bootstrap visual readiness

## Outcome

Scenario startup and reset should never show an intermediate default/still-water state
while the app is resolving fill level, particle density, grid size, and scenario
derived physics. The app may still do that work internally, but it must do it silently:
initialize or reinitialize the simulation in the background, then reveal/render the
selected scenario only after the particle layout and physics parameters match the
current settings.

Done means Falling Blob starts as the Falling Blob scenario again, with the expected
blob/amount of water, and the other scenarios also start with water amounts that match
their scene geometry, fill level, and particle density.

## Scope

In scope:

- Audit the boot, reset, URL/localStorage restore, scenario change, water-level change,
  particle-density change, and grid-resolution change paths that currently trigger
  particle allocation or physics recalculation.
- Make derived scenario state resolve before visuals become visible after boot/reset.
  Acceptable shapes include a readiness gate, hidden first reset, or a two-phase
  bootstrap as long as the user does not see the wrong scene flash or persist.
- Ensure `SceneConfig::from_settings`, resolved particle count, seeded liquid blocks,
  and GPU particle buffers agree after each relevant reset-class change.
- Fix Falling Blob specifically, then verify at least one other scenario so the fix is
  not hard-coded to that preset.
- Update docs for any new startup/reset/readiness contract.

Out of scope:

- Settings panel reorganization, splitter UI, and visible control removals; those live
  in `settings-panel-resize-and-tabs.md`.
- New scenario art direction or new presets.
- Changing the pressure solver, FLIP/PIC algorithm, or render modes except where a
  readiness gate temporarily suppresses rendering during initialization.
- Faking correctness by changing only camera angle or render visibility while the
  underlying particle/scene state remains wrong.

## Context routes

- `docs/architecture/app-shell.md` for WASM app construction, frame loop, reset
  behavior, scene config, pointer modes, and JS bridge ownership.
- `docs/architecture/simulation.md` for particle seeding, fill level, density-derived
  particle count, seeded blocks, and GPU simulation state.
- `docs/architecture/settings.md` for Reset-class settings, `set_setting_result_json`,
  density/count semantics, and URL/localStorage restore implications.
- `docs/architecture/web-shell.md` for boot ordering, shell helpers, reset helper,
  URL `set` application, settings persistence, and visual capture expectations.
- Code routes: `app/crates/fluid-lab/src/lib.rs`,
  `app/crates/fluid-lab/src/scene/mod.rs`,
  `app/crates/fluid-lab/src/settings/mod.rs`,
  `app/crates/fluid-lab/src/gpu/mod.rs`,
  `app/crates/fluid-lab/src/gpu/fluid.rs`, `app/web/main.js`,
  `app/web/panels.js`, and `app/tools/capture.mjs`.

## Open assumptions

- It is acceptable for the app to perform a silent initialization/reset before the
  first visible frame if that is the cleanest way to get the right particle count and
  physics state.
- During readiness work, showing a neutral loading/blank canvas state is acceptable;
  showing a wrong water layout is not.
- If `settings-panel-resize-and-tabs.md` removes the visible `particles.count`
  override, this stream should treat density/fill/scene-derived count as the canonical
  path and keep any count override behavior hidden compatibility at most.

## Acceptance / verification

- Clean boot into Falling Blob shows the Falling Blob particle layout on the first
  meaningful rendered frame, not a still-water/default-pool layout.
- Reset after Falling Blob preserves the selected scenario and reseeds with the correct
  blob/volume.
- Changing fill level, particle density, or grid resolution and then resetting produces
  a particle count and seeded water volume consistent with the scenario-derived
  formula documented in `simulation.md` and `settings.md`.
- At least one non-Falling-Blob scenario is checked for correct water amount after the
  same boot/reset path.
- Capture evidence includes final `stats_json` or trace data that supports the
  particle-count/liquid-volume claim; screenshots alone are not enough for this bug.
- The shell does not keep animating a stale frame during the hidden initialization
  window.
- `cargo build --target wasm32-unknown-unknown` and `cargo test --lib` pass from
  `app/`.
- A browser capture via `tools/capture.mjs` succeeds without WebGPU validation or
  device-loss errors.
- `architecture/app-shell.md`, `architecture/settings.md`,
  `architecture/simulation.md`, and `architecture/web-shell.md` reflect the shipped
  startup/reset contract before this plan is marked shipped.

## Handoff notes

- This bug is likely an ordering problem, not just a scene preset problem. Start by
  tracing who applies persisted/URL settings, when `SceneConfig` is resolved, when GPU
  buffers are allocated, and when the first visible frame is allowed.
- Avoid a fix that only special-cases Falling Blob. The user's note points to a general
  regression from making water level and particle density auto-update physics.
- Coordinate with `settings-panel-resize-and-tabs.md` before editing particle-count
  semantics. Both streams should converge on density/fill/scene-derived counts.

## See also

- `docs/plans/index.md`
- `docs/plans/settings-panel-resize-and-tabs.md`
- `docs/architecture/app-shell.md`
- `docs/architecture/settings.md`
- `docs/architecture/simulation.md`
- `docs/architecture/web-shell.md`
