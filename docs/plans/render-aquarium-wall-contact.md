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

# Render — Aquarium wall contact (flush, see-through water at the wall)

> **Shipped (Phase 1).** Built on the post-surface-fidelity pipeline. Extended the
> surviving cheap `flat_water.*` snap with a contact-band coverage lift
> (`render.hero.flat_water.contact_fill`, Live, default 0.6, routed via composite
> `Cam.flat.w`): near a wall the band's effective thickness is lifted (ramped by the same
> wall-proximity weight as the normal/depth snap) so water reads as a continuous sheet
> flush to the wall instead of a faded fringe that let the dark matte wall show through —
> kept moderate so it stays see-through/refractive. No new buffers or passes; Phase 2
> (dense wall-fill atlas, which would reverse the "weak add-ons removed" decision)
> **not** entered — it remains gated behind a fresh decision entry + measured captures.
> Verified on real-GPU captures of the default settled pool (contact_fill off vs 0.6): on
> reads as a fuller, flush, still-translucent contact. Mid-tank surface fidelity is
> untouched (fill only affects near-wall pixels). Context migrated to
> `architecture/rendering.md` (Screen-space water wall-contact paragraph),
> `architecture/settings.md` (render controls), and `decisions/rendering.md` (extended the
> "weak add-ons removed" entry's cheap-snap note). Safe to delete.

> **Workstream 2 of 2.** Pairs with
> [`render-surface-fidelity.md`](render-surface-fidelity.md), which **must land first** —
> it rewrites the normal/depth reconstruction this work then has to defer to near walls.
> The two share `composite.wgsl`, `smoothing.rs`, and `HeroParams` and pull the surface
> in opposite directions (surface fidelity *preserves* curvature; this *flattens* it at
> the contact line). Author against the post-fidelity pipeline.

## Outcome

Water touching a tank wall **sits flat against it and reads as see-through, like real
water at an aquarium** — a clean, continuous, refractive sheet flush to the wall with no
dark wall/background bleeding through gaps between splats at the contact line. This is the
"bring back wall fill, but better" request, scoped to the *aquarium contact* look, not to
re-allocating the removed dense wall-fill atlas.

## Problem

Wall fill was removed as a weak add-on; only the **cheap near-wall snap** survives —
`render.hero.wall_contact_enabled` gating `flat_water.strength/epsilon/depth_strength`,
which flattens near-wall normal/depth in `gpu/shaders/composite.wgsl`
([`architecture/rendering.md`](../architecture/rendering.md):84-88). It evidently falls
short of reading as a flush, see-through aquarium pane. "Glass" here means the existing
**matte wall surface** (back/left); the goal is water reading flush and refractive against
it — **not** turning walls into a new transparent glass material (that option was
explicitly *not* chosen).

## Scope

In scope:

- Make the near-wall contact read as a **flat, continuous, refractive** sheet flush to the
  wall: coverage/thickness fills to the contact line, the surface normal snaps wall-flat,
  and refraction still samples `scene_color` so the background reads *through* the water.
- Reconcile with the surface-fidelity filter: near the wall, **defer to the wall plane**
  (flatten) instead of preserving curvature — the two effects must not fight at the
  contact band.
- Live controls under the Water view, mirrored into `HeroParams` (extend the existing
  `flat_water.*` surface rather than add a new mode/Reset class).

Out of scope (cut line):

- **No transparent-glass wall material / meniscus** (the "make walls real glass" option,
  not chosen).
- **No turning the open (+x/+z) viewing faces into walls** — the open-corner decision
  stands ([`decisions/rendering.md`](../decisions/rendering.md), "The tank has an open
  viewing corner").
- The dense wall-fill buffer is **Phase 2 only**, gated on a spike (below).

## Approach

**Phase 1 — spike the cheap screen-space enhancement first.**
Extend the existing `flat_water.*` snap: widen/clean the contact band so thickness and
coverage read flush to the wall, snap the normal wall-parallel at contact, and confirm
refraction still shows the background through the water. No new buffers, no new passes —
stays inside the surviving cheap-snap half of the removal decision. Capture against a wall
the water is pressed on.

**Phase 2 — dense wall-fill pass, only if the spike can't read flush.**
If captures from Phase 1 still show gaps / a non-flush contact line that the screen-space
snap can't fix, reintroduce a dense wall-fill pass (the removed `gpu/wallfill.rs` concept).
**This formally reverses the "weak add-ons removed" decision**
([`decisions/rendering.md`](../decisions/rendering.md):96-116), so it requires its own
decision entry, owned resource allocation, settings, and measured-capture justification —
do not slip it in silently. Record the profiler cost.

## Acceptance / verification

- `cd /home/adamg/fluid-simulation/app && cargo build --target wasm32-unknown-unknown` — WASM compile.
- `cd /home/adamg/fluid-simulation/app && cargo test --lib` — host reference + settings schema.
- **Visual acceptance:** real-GPU `tools/capture.mjs` of water pressed against a wall,
  showing a flush, see-through contact (background visible through the water, no dark
  inter-splat gaps at the wall). Before/after vs the current `flat_water` snap.
- Must not regress the surface-fidelity result mid-tank (the curvature-preserving filter
  still works away from walls).
- Profiler `gpu` totals recorded; mandatory judgement gate before shipping Phase 2.

## Open assumptions

- The flush, see-through aquarium look is reachable by enhancing the screen-space snap;
  the dense pass is a fallback, not the expected path. (If false → Phase 2 + its decision
  entry.)
- "Glass" = the existing matte wall surface; no new transparent wall material is wanted.
  (Confirm with the requester if Phase 1 captures read as "still not aquarium-like" for a
  reason that's actually about wall *material*, not water flushness.)

## Handoff notes

- **Sequencing:** starts after `render-surface-fidelity.md` lands. Build against its new
  normal/depth pipeline; at the contact band, this work overrides the curvature
  preservation with wall-flat snapping.
- **Shared files / collision risk:** same set as the paired workstream —
  `gpu/shaders/composite.wgsl`, `gpu/smoothing.rs`, `gpu/mod.rs` (`HeroParams`),
  `settings/mod.rs`. Phase 2 additionally re-adds `gpu/wallfill.rs` +
  `gpu/shaders/wallfill.wgsl` and a render target.
- **Review focus:** flush contact verified on a capture (not by reading the shader);
  background genuinely visible *through* the water at the wall; no fight with the surface
  filter mid-tank.
- Per the change→doc table: update `architecture/rendering.md` (the wall-contact behavior
  + remove it from the "removed features" list if Phase 2 ships), `architecture/settings.md`
  (the contact controls), and `decisions/rendering.md` (Phase 2 reverses a recorded
  removal decision → new entry required).

## See also

- [`render-surface-fidelity.md`](render-surface-fidelity.md) — paired workstream (land first).
- [`../architecture/rendering.md`](../architecture/rendering.md) · [`../decisions/rendering.md`](../decisions/rendering.md)
- [`~/agent-docs/v1/plan-lifecycle.md`](~/agent-docs/v1/plan-lifecycle.md) — status + ship-time migration.
