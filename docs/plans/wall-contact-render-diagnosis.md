---
status:        shipped
owner:         adamg
last_updated:  2026-06-11
okay_to_delete: true
long_lived:    false
owning_docs:
  - architecture/rendering.md
  - architecture/gpu-resources.md
  - architecture/settings.md
  - decisions/rendering.md
---

# Wall Contact Render Diagnosis

## Resolution (2026-06-11)

**Both failures had one root cause: the screen-space `thickness` target was never
spatially smoothed.** The depth (`nearest_z`) target gets an edge-preserving bilateral
blur → a clean surface normal, but the composite drove Beer-Lambert opacity straight from
the *raw* thickness. At ~247k particles in a 64³ tank that thickness is a per-particle
splat speckle, which read directly as (a) the "rounded/blocky pixels" — a sandy, speckled
water body — and (b) the "space between water and glass": between splats near the wall the
opacity dropped to ~0, so the dark wall showed through, reading as a dry gap.

**Diagnosis method (source-buffer evidence, not tuning):** added three temporary-but-kept
`render.hero.debug_view` routes (nearest-Z, whitewater, wallfill-mask) and an A/B toggle
matrix (wet-wall on/off × wall-fill on/off) captured on a paused, fixed-camera settled
frame. The `Thickness` debug view was pure static while `Nearest Z` was already smooth —
isolating the producer to thickness, not the normal, the wall-fill (its mask was a clean
continuous band reaching the corners), or the wet-wall.

**Fix (smallest structural change):** `ThicknessSmoothRenderer`
(`gpu/smoothing.rs` + `shaders/thickness_smooth.wgsl`) — a plain separable Gaussian that
blurs the thickness target in place after wall-fill injection and before temporal, reusing
the depth pass's `smooth_z_ping` scratch and `render.hero.smooth_radius` /
`smooth_iterations`. No new render target; `gpu_buffer_mb` unchanged; `render_ms` ≈ 1.6.
Made the prior particle-thickness wall-suppress bandage redundant, so it was reverted
(particles back to a clean billboard path; no wall blobs return because thickness is now
smoothed).

**Second pass (the moving default).** First-round testing was done with auto-roll OFF on
a settled pool — the wrong scenario: the live default product mode is **autoRotate**
(`web/main.js`, auto-roll ON), so the tank rocks and water sloshes. In that state the
dominant "pixelly" was the **whitewater (foam) target**, also an unsmoothed per-particle
signal, rendering as a field of white speckle dots all over moving water (A/B: foam
strength 0 made the body clean). Fix: a second `ThicknessSmoothRenderer` instance blurs the
whitewater target the same way (reusing the same ping scratch). Foam now reads as coherent
soft regions. Verified live with auto-roll ON.

**Evidence:** `captures/wall_fix_before_after.png` (overview, speckle→coherent),
`captures/wall_fix_final_settled.png` (settled headline, water reaches the glass with a
wet-wall sheen, no dry gap). Boot console: smoke PASS, `navigator.gpu present: true`, no
shader/page errors. `cargo test --lib` 28/28, `cargo build --target wasm32` clean,
`git diff --check` clean.

**Auto-roll gentled (default look).** Because the spray is a screen-space-particle limit
that smoothing can't remove, the `interaction.auto_roll_strength` default was lowered
`0.45 -> 0.22` (max tilt ~26° -> ~13°) so the default Auto-Rotate rocks gently and water
stays a coherent sloshing pool instead of crashing into spray. Live/per-user adjustable;
this is the biggest lever for the default look. Verified live (`captures/gentle/`).

**Remaining limitations (stated plainly):** sparse **airborne spray** thrown by a hard
slosh (now rare at the gentled default, still reachable via manual slosh or higher
strength) is still individual particles, so it renders as discrete soft billboard blobs,
not a smooth sheet (a screen-space-particle limit). At the very thin water edge against the glass some residual texture remains
(thickness → 0 there gives the blur little to work with, and the refracted floor checker
adds pattern); the wet-wall sheen is a procedural cue, not simulated film; content-motion
shimmer is still only addressed by the (default-off) temporal blend. Durable detail migrated to `architecture/rendering.md`,
`architecture/gpu-resources.md`, `architecture/settings.md`, `decisions/rendering.md`.

The original diagnostic brief below is retained for context; it is safe to delete.

## Mission

Fix the two visible wall-contact failures by diagnosing the actual producer first:

- There is still visible space between water and the glass/walls.
- Wall-adjacent effects still read as rounded/blocky pixels instead of smooth realistic
  water/wet glass.

Do not start by tuning wet-wall blur, opacity, fill thickness, or foam defaults. The next
agent must isolate which render target/pass creates each artifact, then make the smallest
structural fix in that producer.

## Ground Rules

- Stop treating this as a settings problem until proven otherwise.
- Use fixed-frame, fixed-camera captures. Avoid judging from auto-rotating random frames.
- Do not claim fixed unless the final live browser capture visibly improves both issues.
- Prefer temporary debug views over guessing.
- Keep the fix render-only unless the diagnosis proves simulation data must change.

## Diagnostic Captures

Run the app with:

```bash
cd app
./run.sh
```

Use `app/tools/capture.mjs` for browser captures. Save named captures under `captures/`.

Add or use debug views for the same paused/fixed frame:

- raw particle `thickness`
- `nearest_z`
- `whitewater`
- `wallfill_mask`
- wall-fill injected thickness only
- wet-wall wetness only
- final composite with wet-wall disabled
- final composite with wall-fill disabled
- final composite with particle thickness suppressed near walls
- final composite with diffuse particles disabled

The diagnosis is not complete until the bad pixels are visible in exactly one source
buffer/pass, or until a controlled A/B proves which combination creates them.

## Diagnose The Gap

Determine whether the water/wall space is caused by:

- simulation particle centers being intentionally inset from solid boundary cells
- particle thickness splats not reaching the wall
- wall-fill sheet not writing where expected
- wall-fill writing but losing in depth/composite
- smoothing/nearest-Z pulling the visible surface away from the wall
- glass/wireframe/environment geometry visually covering the wall-contact water

Required proof:

- Compare `wallfill_mask`, raw `thickness`, `nearest_z`, and final composite for the same
  frame.
- If the mask reaches the wall but the final image has a gap, inspect composite/depth.
- If the mask does not reach the wall, inspect wall-fill occupancy/projection.
- If particle thickness reaches the wall but appears as blobs, wall-fill must own that
  band and particle splats must be suppressed or replaced there.

## Diagnose The Pixelly Wall Artifacts

Determine whether the rounded/blocky wall artifacts come from:

- main particle thickness billboards
- wall-fill occupancy atlas or mask
- wet-wall wetness material
- diffuse foam/spray particles
- composite whitewater/speed mask
- environment wall shading or glass/wireframe overlay

Required proof:

- Eliminate sources one at a time using debug toggles/captures.
- If artifacts remain with diffuse off and wet-wall off, inspect particle thickness and
  wall-fill.
- If artifacts remain with particle thickness suppressed near walls, inspect wall-fill
  mask/atlas and final composite.
- If artifacts only appear in final composite, inspect mask-driven color/reflection,
  whitewater, roughness, and depth interaction.

## Likely Fix Paths

If the gap is missing wall-contact surface:

- Implement a deliberate wall-contact render layer projected onto the inner glass plane.
- Composite it consistently in front of/inside the glass wall.
- Let this layer bridge the particle-center inset instead of relying on splats.

If artifacts are main particle splats:

- Suppress or feather particle thickness in a vertical-wall band.
- Let the smooth wall-contact layer own those pixels.
- Avoid global alpha/radius reductions that damage open-water appearance.

If artifacts are wall-fill atlas/mask:

- Improve wall-local occupancy filtering and waterline reconstruction.
- Use continuous coverage from multiple nearby cells.
- Reject isolated low-coverage texels before they write visible mask/thickness.
- Ensure dispatch shape remains legal at the chosen default supersample.

If artifacts are wet-wall material:

- Threshold tiny wetness islands before material response.
- Blur at read time and/or update time.
- Render wetness as broad material response, not visible spots/dots.

If artifacts are composite/depth:

- Inspect whether `nearest_z`, wall depth, and scene depth disagree.
- Fix ordering or depth selection so wall-contact water is not hidden behind glass or
  pulled away from the wall plane.

## Implementation Rules

- Make the smallest structural change after diagnosis identifies the producer.
- Do not change unrelated defaults unless they are part of the proven producer.
- Keep temporary debug views either behind explicit debug settings or remove them before
  handoff.
- Update docs if render pipeline behavior, resources, settings, or decisions change.

## Verification

Before handing back:

```bash
cd app
cargo test --lib
cargo build --target wasm32-unknown-unknown
cd ..
git diff --check
```

Then run `./run.sh`, capture the final browser image, and inspect it visually.

The final handoff must include:

- final capture path
- test/build results
- the root cause for the gap
- the root cause for the pixelly wall artifacts
- exactly which pass/buffer was changed
- any remaining visual limitations, stated plainly

## Acceptance Criteria

- Water visually reaches the glass/walls in the default view without a dry gap.
- Wall-contact water/wetness reads as a continuous sheet/material cue, not rounded pixels
  or blocky cell artifacts.
- The final browser capture has no WebGPU shader/page errors.
- The fix is explained by source-buffer evidence, not by subjective tuning alone.
