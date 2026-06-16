---
status:        shipped
owner:         codex
last_updated:  2026-06-16
okay_to_delete: true
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

Foam here means the persistent surface-foam feature exposed by the Foam tab and its
enable/disable control. Keep the Water composite's core speed-weighted whitewater/foam
tint; this plan must not remove that whitewater signal.

## Scope

In scope:

- Remove surface foam as a runtime feature, including its settings, GPU resources,
  shaders/passes, render dispatch, visible profiler text, and docs references.
- Keep core Water-mode whitewater tinting that is not backed by the Foam tab's
  persistent `DiffuseSystem` resources/settings.
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

## Sequencing and ownership

This plan runs first. `ui-shell-settings-simplification.md` and
`dev-theme-system.md` should wait until these render settings and tabs are gone.

Owned surfaces: Rust GPU/render/settings/profiler removal work plus
`architecture/rendering.md`, `architecture/settings.md`,
`architecture/gpu-resources.md`, `architecture/profiler.md`, and
`decisions/rendering.md`.

Do not own: cosmetic settings-tab restructuring, web-shell theme state, or the compact
control layout except where deleting Foam/micronormal/flat-water controls leaves dead UI
references.

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
- The Water-mode capture is reviewed near tank walls after flat-water/contact-fill
  removal; any visible wall-contact regression is either fixed or explicitly accepted in
  `decisions/rendering.md`.
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

- Current render-pass and removed-feature facts went to `architecture/rendering.md`:
  Water still owns thickness/whitewater/depth smoothing and composite, while
  persistent `DiffuseSystem`, micronormals, and flat-water/wall-contact correction are
  absent.
- Removed setting ids and legacy behavior went to `architecture/settings.md`: current
  `render.diffuse.*`, `render.hero.micro_normal_*`, `render.hero.flat_water.*`, and
  `render.hero.wall_contact_enabled` imports are rejected as unknown, while older
  caustic/temporal/wet-wall compatibility ids remain hidden no-ops.
- Removed buffers/pipelines went to `architecture/gpu-resources.md`; `DiffuseSystem`
  buffers/counters are gone and tracked memory is sim/render-target/timing only.
- Stats/profiler shape changes went to `architecture/profiler.md`; `gpu.diffuse`,
  foam counters, and visible foam profiler rows are gone.
- The rationale for keeping only the Water composite speed-weighted whitewater tint
  went to `decisions/rendering.md`.

## See also

- `docs/plans/index.md`
- `docs/architecture/rendering.md`
- `docs/architecture/settings.md`
- `docs/architecture/gpu-resources.md`
- `docs/architecture/profiler.md`
- `docs/decisions/rendering.md`
