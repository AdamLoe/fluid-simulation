---
status:        shipped
owner:         unassigned
last_updated:  2026-06-11
okay_to_delete: true
long_lived:    false
owning_docs:
  - architecture/rendering.md
  - architecture/gpu-resources.md
  - architecture/settings.md
  - decisions/rendering.md
---

# Wall Wetness And Fill Polish

## Mission

Polish the wall-contact rendering without reopening the whole wall-fill design. Done
means wet-wall detail follows the newer dense pixel strategy used by wall fill, still
water fills tank corners cleanly instead of rounding away from them, and wall-fill lines
that change with camera/tank rotation are gone.

## Scope

In scope:

- Update wet-wall wetness generation/sampling toward the dense/supersampled pixel
  strategy already used by wall fill, so wet walls read as pixel-accurate material
  response instead of coarse grid blocks.
- Fix only the wall-fill corner behavior. The rest of wall fill is considered good and
  should be preserved.
- Diagnose and remove rotation-dependent wall-fill lines/stripes/seams.
- Keep the work render-only unless diagnosis proves a small classification read is
  incorrect.
- Use debug views and controlled captures before tuning values.

Out of scope:

- Rewriting wall fill globally.
- Changing the main screen-space water smoothing pipeline unless it is directly proven to
  create the rotation-dependent wall-fill lines.
- Adding real thin-film drainage physics.
- Changing diffuse-water behavior.
- Tuning unrelated hero-water defaults.

## Known Target Symptoms

Corner issue:

- A completely still water mass is the best test.
- Corners look too rounded; the desired result is a clean box-aligned fill into the tank
  corner.
- The fix should affect corner/edge reconstruction only, not make the whole wall-fill
  sheet thicker or more opaque.

Rotation-dependent lines:

- Wall fill shows strange lines that change depending on camera or tank rotation.
- Treat this as a source-buffer or projection bug, not as a cosmetic opacity problem.
- The lines must be removed, not merely made less visible by lowering fill strength.

Wet-wall detail:

- Wetness should use the same kind of dense wall-local pixel strategy as fill wall where
  practical.
- If wet wall and wall fill have separate buffers, document why they remain separate and
  how their resolution/sampling strategies now align.

## Approach

Owned streams:

| Stream | Area | Owned files |
|---|---|---|
| Diagnosis | Add/use wall-fill mask, wetness, nearest-Z, and final composite debug captures | existing debug views, possibly `settings/mod.rs` if a temporary view becomes durable |
| Wet-wall density | Improve wetness field update/read resolution and filtering | `crates/fluid-lab/src/gpu/wetwall.rs`, `crates/fluid-lab/src/gpu/shaders/wetwall_update.wgsl`, `crates/fluid-lab/src/gpu/shaders/environment.wgsl` |
| Corner fill | Adjust only corner/edge reconstruction in wall fill | `crates/fluid-lab/src/gpu/wallfill.rs`, `crates/fluid-lab/src/gpu/shaders/wallfill.wgsl` |
| Rotation lines | Fix the proven producer: atlas sampling, per-face seam handling, depth/nearest-Z interaction, or projection math | likely `wallfill.wgsl`; expand only if diagnosis requires |
| Docs | Migrate final current-state facts | `architecture/rendering.md`, `architecture/gpu-resources.md`, `architecture/settings.md`, `decisions/rendering.md` |

Diagnostic requirements:

- Capture a paused, still-water scene at a fixed frame.
- Capture a short rotation sweep or several fixed rotations that reproduce the changing
  wall-fill lines.
- Compare `wallfill_mask`, nearest-Z, wetness, and final composite for the same frames.
- If the mask is clean but final composite has lines, inspect composite/depth/normal
  interaction.
- If the mask has lines, inspect wall-plane intersection, face atlas coordinates, bilinear
  sampling at cell/face seams, and corner/edge blending.

Likely corner direction:

- Preserve the current wall-fill atlas and ordinary per-face sampling.
- Add explicit edge/corner handling where two wall faces or wall+floor meet, so a still
  water mass does not lose coverage at the corner because each face is reconstructed in
  isolation.
- Avoid globally inflating coverage. Corner repair should be local to edge/corner
  neighborhoods.

Likely line direction:

- Check for face-boundary discontinuities in atlas coordinates.
- Check for snapped rows/columns or integer truncation that becomes visible under
  bilinear sampling.
- Check for depth fighting or nearest-Z min writes that vary with viewing angle.
- Check that per-frame clear and fill-disabled paths write a stable zero mask.

## Exit Gate

- `cd /home/adamg/fluid-simulation/app && cargo test --lib`
- `cd /home/adamg/fluid-simulation/app && cargo build --target wasm32-unknown-unknown`
- Browser captures against `http://localhost:5184/`:
  - Still-water corner stress case, paused/fixed camera.
  - Same case with at least three camera/tank rotations that previously showed changing
    lines.
  - Wet-wall close-up with fill on and wet wall on.
- Console logs must show healthy WebGPU boot, atomic smoke PASS, and no WGSL/device
  errors.
- Visual acceptance:
  - Corners are box-aligned and no longer visibly rounded away.
  - Wall-fill lines do not appear or change across the rotation captures.
  - Non-corner wall-fill look remains substantially the same.
  - Wet-wall detail reads finer/more pixel-accurate than the prior coarse grid.

## Discipline Rules

- Diagnose before tuning.
- Do not lower wall-fill strength, color, reflection, or opacity as the primary fix for
  lines.
- Do not rewrite wall fill outside the corner/line producers.
- Do not add CPU/GPU readback to normal frames.
- Do not change P2G, pressure, or simulation boundary invariants.

## Migration Notes

Migrated at ship time:

- `architecture/rendering.md` describes the dense wall-fill atlas, particle-splat
  occupancy, rendered back/left wall injection, local shared-corner repair, and the
  removal of hidden front/right projected sheets that caused rotation-dependent mask
  seams.
- `architecture/gpu-resources.md` describes the wall-fill atlas allocation and clarifies
  that the render pass samples the back/left atlas faces while the mask remains a
  swapchain-sized render target.
- `architecture/settings.md` documents that `waterline_softness` affects the sampled
  wall-atlas coverage and rendered back-left corner repair.
- `decisions/rendering.md` keeps the wet-wall and wall-fill policy render-only, records
  particle-splat wall-fill as a visual sheet rather than simulated mass, and ties the
  wall-fill projection to the open viewing-corner decision.

Verification evidence: `captures/wall_polish_corner_final.png`,
`captures/wall_polish_corner_mask.png`, `captures/wall_polish_corner_drag_wide.png`, and
`captures/wall_polish_corner_drag_opposite.png` booted with WebGPU, atomic smoke PASS, and
no page/shader errors in their console logs.

## See Also

- `docs/architecture/rendering.md`
- `docs/architecture/gpu-resources.md`
- `docs/architecture/settings.md`
- `docs/decisions/rendering.md`
- `docs/plans/wall-detail-atlas-polish.md`
- `docs/plans/wall-contact-render-diagnosis.md`
