---
status:        shipped
owner:         adamg
last_updated:  2026-06-13
okay_to_delete: true
long_lived:    false
owning_docs:
  - architecture/rendering.md
  - decisions/rendering.md
  - architecture/settings.md
---

# Render — Surface fidelity (smoother sheets, sharper crests)

> **Shipped (Phase 1).** Curvature-adaptive feature-preserving depth filter + normal
> reconstruction, one Live knob `render.hero.feature_preservation` (default 0.6, 0 =
> legacy isotropic). Verified on real-GPU captures of the default scene (settled pool +
> mid-splash): off reads as a glassy rounded sheet, on preserves surface chop/crests with
> no speckle breakdown. Phase 2 (anisotropic splats) **not** entered — Phase 1 judged
> sufficient. Context migrated to `architecture/rendering.md` (Screen-space water),
> `architecture/settings.md` (render controls), and `decisions/rendering.md` ("Surface
> fidelity uses a curvature-adaptive screen-space filter"). Safe to delete.

> **Workstream 1 of 2.** Pairs with
> [`render-aquarium-wall-contact.md`](render-aquarium-wall-contact.md). **Do this one
> first** — it rewrites the normal/depth reconstruction that the wall-contact work then
> builds on. The two share `composite.wgsl`, `smoothing.rs`/`water_smooth.wgsl`, and the
> `HeroParams` uniform, and they pull the surface in opposite directions (this work
> *preserves* curvature; wall contact *flattens* it at the wall). Land this, then hand
> off.

## Outcome

The default Water surface reads as **real water, not smoothed spheres**: large faces are
glassy-smooth while crests, ridges, and thin droplet tips stay *pointy* instead of being
rounded off by the smoothing pass. Stays entirely in the screen-space composite — no SDF
/ level-set surface, which remains deliberately deferred
([`decisions/rendering.md`](../decisions/rendering.md), scope.md). User-perceived win is
confirmed by before/after real-GPU captures of the default scene.

## Problem

Today the front surface is screen-space splats → **isotropic bilateral** depth smoothing
on `nearest_z` (`gpu/shaders/water_smooth.wgsl`, iterated in `gpu/smoothing.rs`), with the
normal reconstructed from smoothed depth derivatives in `gpu/shaders/composite.wgsl`. An
isotropic kernel rounds *everything* equally, so there is no setting that yields
smooth-sheets-and-sharp-features at once: more iterations = blobbier, fewer = speckled
spheres. The thickness target is already Gaussian-smoothed separately
([`decisions/rendering.md`](../decisions/rendering.md), "thickness and whitewater are
spatially smoothed"); the *shape* (depth/normal) is the remaining rounding source.

## Scope

In scope:

- Replace or augment the isotropic bilateral depth filter with a **feature-preserving**
  screen-space surface filter — e.g. a narrow-range filter or screen-space curvature
  flow — that smooths low-curvature regions but preserves depth discontinuities and
  high-curvature ridges.
- Improve normal reconstruction in `composite.wgsl` so sharp features survive into
  shading/refraction (the normal is what the eye reads as "pointy").
- New **Live** controls under the Water view, mirrored into the single `HeroParams`
  uniform per the existing hero-features decision (no new render mode, no Reset class).
- Keep the existing `render.hero.flat_water.smooth_iters`-style knobs working or migrate
  them cleanly.

Out of scope (cut line):

- **No SDF / level-set / marching-cubes surface.** Stays deferred.
- **No new per-particle work in Phase 1.** Anisotropic splats are a *gated stretch*
  (Phase 2 below), not part of the first ship.
- Wall-contact / aquarium flush behavior — owned by the paired workstream.
- Thickness/whitewater absorption model — unchanged.

## Approach

**Phase 1 — pure screen-space, ship this first (no per-particle cost).**
Swap the isotropic bilateral for a feature-preserving filter and rebuild the normal from
it. This touches only screen-space passes (`smoothing.rs`, `water_smooth.wgsl`,
`composite.wgsl`) and the `HeroParams` plumbing — it does **not** touch the P2G/g2p hot
path that is the known per-particle bottleneck
([[fluid-perf-bottleneck-is-per-particle]]). Capture before/after at the default scene.

**Phase 2 — anisotropic splats, only if Phase 1 still rounds the thinnest tips.**
If captures show droplet tips / thin sheets still read too round after Phase 1, add a
per-particle neighbor pass that orients each splat to the local particle distribution
(ellipsoid / Yu–Turk-style splatting). **Gate explicitly:** this adds work to the
per-particle path that is already the bottleneck, so it ships only if (a) Phase 1 is
visibly insufficient on captures and (b) the profiler `gpu` totals after it are recorded
and judged acceptable. Perf is quality-first / measure-later for this plan, but Phase 2
is the one step where the cost lands in the hot path, so its evidence is mandatory before
shipping.

## Acceptance / verification

- `cd /home/adamg/fluid-simulation/app && cargo build --target wasm32-unknown-unknown` — WASM compile.
- `cd /home/adamg/fluid-simulation/app && cargo test --lib` — host reference + settings schema (any new settings ids must register cleanly).
- **Visual acceptance (the signal that can't be faked):** real-GPU `tools/capture.mjs`
  before/after of the default scene, showing smooth faces *and* preserved crests/tips.
  Include a violent-slosh frame so thin features are present.
- Direct particle/grid inspection views must remain reachable
  ([`decisions/rendering.md`](../decisions/rendering.md), "Visual styling must preserve
  observability").
- Profiler `gpu` render totals recorded as evidence (not a hard gate in Phase 1; required
  judgement gate before shipping Phase 2).

## Open assumptions

- Feature-preserving filtering in screen space is enough for "smoother *and* pointier" on
  the common scenes; anisotropic splats are only needed for the thinnest airborne
  features. (If false → Phase 2 triggers.)
- The single `HeroParams` uniform has room for the new controls without splitting the
  composite into multiple pipelines (consistent with the current hero-features decision).

## Handoff notes

- **Sequencing:** land before `render-aquarium-wall-contact.md`. That workstream consumes
  the normal/depth pipeline this one rewrites.
- **Shared files / collision risk:** `gpu/shaders/composite.wgsl`,
  `gpu/smoothing.rs` + `gpu/shaders/water_smooth.wgsl`, `gpu/mod.rs` (`HeroParams`),
  `settings/mod.rs` (`HeroParams` struct + registry ids). Leave the near-wall snap
  (`wall_contact_enabled` / `flat_water.*`) functionally intact so the paired workstream
  has a clean base to extend.
- **Review focus:** does the new normal actually preserve high curvature (verify on a
  capture, not by reading the shader), and did any new settings id register + replay
  safely?
- Per the change→doc table: update `architecture/rendering.md` (smoothing/normal pass +
  view modes), `architecture/settings.md` (new `render.hero.*` controls), and
  `decisions/rendering.md` (the filter-choice decision and why screen-space over SDF).

## See also

- [`render-aquarium-wall-contact.md`](render-aquarium-wall-contact.md) — paired workstream.
- [`../architecture/rendering.md`](../architecture/rendering.md) · [`../decisions/rendering.md`](../decisions/rendering.md)
- [`~/agent-docs/v1/plan-lifecycle.md`](~/agent-docs/v1/plan-lifecycle.md) — status + ship-time migration.
