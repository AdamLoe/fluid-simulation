---
status:        draft
owner:         unassigned
last_updated:  2026-06-16
okay_to_delete: false
long_lived:    false
owning_docs:
  - architecture/rendering.md
  - architecture/settings.md
  - architecture/gpu-resources.md
  - architecture/profiler.md
  - decisions/rendering.md
---

# Render feature removals

## Mission

Remove foam, micronormals, and flat-water behavior from the app rather than merely
hiding their controls. Done means those features no longer allocate resources, schedule
passes, expose settings, report visible profiler state, or appear in current
architecture docs, and the water renderer still builds and runs through the supported
render modes.

## Scope

In scope:

- Remove surface foam as a runtime feature, including its settings, GPU resources,
  shaders/passes, render dispatch, visible profiler text, and docs references.
- Remove micronormal controls and shader/runtime behavior from the water surface.
- Remove flat-water / wall-contact controls and shader/runtime behavior.
- Remove or simplify legacy compatibility for these ids. The user explicitly does not
  care about preserving old saved URLs/localStorage because the app is not in use yet.
- Clean docs so foam, micronormals, and flat-water are not described as live features.

Out of scope:

- Cosmetic settings-tab restructuring; that belongs to
  `ui-shell-settings-simplification.md`.
- Theme variables or color presets; that belongs to `dev-theme-system.md`.
- Replacing removed features with a new SDF, level-set, wall-fill, or whitewater system.

## Approach

This should be implemented as a render/GPU/settings stream, separate from the web UI
cleanup. Treat foam as the high-risk removal because docs show it spans
`crates/fluid-lab/src/gpu/diffuse.rs`, `diffuse_*` shaders, render pass dispatch,
settings snapshots, profiler/stats shape, and architecture docs.

The implementer should remove feature code in dependency order: settings and UI exposure
must not reference fields after GPU structs/shaders stop owning them, and profiler/capture
expectations must be updated with the new stats shape. Since compatibility is not a
product requirement for these removed ids, prefer deleting legacy mapping/no-op handling
for these families unless a small bridge shim is needed to avoid boot failures while
localStorage is cleared.

Keep the surviving water renderer focused on the supported modes documented today:
Water, OpticalParticles, SimpleParticles, tank wireframe, skybox/environment, and optional
grid slice overlay.

## Exit gate

- `rg` finds no live foam, micronormal, or flat-water settings/controls outside deleted
  history, migration notes, or explicitly retained "removed feature" compatibility notes.
- Water, OpticalParticles, and SimpleParticles render paths still compile and run.
- `cargo build --target wasm32-unknown-unknown` and `cargo test --lib` pass from `app/`.
- A browser capture of Water mode succeeds without WebGPU validation/device-loss errors.
- `architecture/rendering.md`, `architecture/settings.md`,
  `architecture/gpu-resources.md`, `architecture/profiler.md`, and
  `decisions/rendering.md` reflect the smaller render surface.

## Handoff notes

- The repo may already contain unrelated modified/deleted files. Do not revert them.
- The current docs describe foam as conservative surface foam and flat-water as a cheap
  wall-contact snap; after implementation those should move to removed/deleted history
  or disappear from current-state docs.
- If profiler JSON retains zero-valued legacy keys for capture compatibility, document
  that as compatibility shape only, not as a live feature.

## Migration notes (filled in at ship time)

- Current render-pass and removed-feature facts go to `architecture/rendering.md`.
- Removed setting ids and any remaining legacy behavior go to `architecture/settings.md`.
- Removed buffers/pipelines go to `architecture/gpu-resources.md`.
- Stats/profiler shape changes go to `architecture/profiler.md`.
- The rationale for not replacing these systems goes to `decisions/rendering.md` if it
  is not already covered.

## See also

- `docs/plans/index.md`
- `docs/architecture/rendering.md`
- `docs/architecture/settings.md`
- `docs/architecture/gpu-resources.md`
- `docs/architecture/profiler.md`
- `docs/decisions/rendering.md`
