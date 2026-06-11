---
status:        shipped
owner:         adamg
last_updated:  2026-06-09
okay_to_delete: true
long_lived:    false
owning_docs:
  - architecture/rendering.md
  - architecture/gpu-resources.md
  - architecture/settings.md
  - decisions/rendering.md
---

# Wall Detail Atlas Polish

## Mission

Make the wall wetness and wall-fill effects read as high-detail continuous render cues
instead of coarse cell/column artifacts. Done means wall wetness no longer exposes a
blocky 64-cell grid up close, wall fill responds gradually to strength/thickness controls,
and disconnected water patches in the same wall column do not get joined into one filled
sheet.

## Scope

In scope:

- Replace wall-fill's single-waterline-per-column data model with a dense wall-local mask
  that represents vertical occupancy.
- Improve wet-wall detail using GPU-side dense/supersampled wall data and shader-side
  filtering/material response.
- Retune fill defaults so color and thickness are subtle and controllable.
- Keep the effects render-only; no solver feedback and no normal-frame readback.
- Update the owning architecture/settings/decision docs after the code ships.

Out of scope:

- Real thin-film drainage physics.
- A marching-cubes or extracted surface path.
- A full projected photon/caustic system.

## Approach

Streams:

| Stream | Area | Status | Last observed fact | Next action | Blockers |
|---|---|---|---|---|---|
| Recon A | wall-fill data model | done | current `[occ, topmost_waterline]` per column destroys dry gaps before render | worker gets dense per-wall-cell occupancy prompt | — |
| Recon B | wet-wall resolution/material | done | supersample repeats identical per-cell blocks; Reset-class `supersample` can desync uniform/buffer; gloss setting is not wired | run after wall-fill worker | — |
| Implement 1 | wall-fill dense vertical mask | done | worker changed only `wallfill.rs` + `wallfill.wgsl`; compile gate green | include in final verification | — |
| Implement 2 | wet-wall detail/material tune | done | worker fixed stored allocation dims, fractional supersampled contact, gloss wiring; build + 28 host tests green | include in integration review | — |
| Review | adversarial GPU diff review | done | one medium finding: `waterline_softness` had become no-op; fixed and compile-gated | — | — |
| Verify | compile, tests, browser capture | done | WASM build green; host tests 28 passed; debug/default captures healthy | — | — |
| Docs | architecture/settings/decisions migration | done | rendering/resources/settings/decision docs plus manifest routing updated | — | — |

Implementation direction:

1. Wall fill should store dense wall occupancy over both horizontal and vertical wall
   axes, not a single topmost waterline per column.
2. The fill render pass should sample that dense mask at the wall hit point, feather it
   in wall UV space, and write subtle coverage/thickness into the existing MRT targets.
3. Strength and slab thickness must stay separate in behavior: strength controls how much
   the fill contributes, slab controls optical body only.
4. Wet-wall detail should derive from GPU-side dense wall fields and filtered reads, not
   CPU readback or sparse/hash lookup.

Observed recon facts:

- Current wall-fill storage is `2 f32` per wall column: occupied plus topmost liquid row.
  This cannot represent liquid-air-liquid vertical columns, so any water above another
  patch merges the whole interval below the top waterline.
- The smallest safe wall-fill fix is one `f32` occupancy value per wall texel/cell:
  back/front `nx*ny`, left/right `nz*ny`, floor `nx*nz` kept for layout continuity.
- Wet-wall supersampling currently maps `i_ss / ss` and `j_ss / ss` back to the same
  source cell, so it creates bigger buffers but identical `ss*ss` blocks.
- `render.hero.wet_wall.supersample` is Reset-class, but `WetWallSystem::record_step`
  rereads the live value into uniform dimensions before Reset. The system should store
  the resolved allocation dimensions and reuse them until rebuilt.
- `wet_wall_gloss_strength` is carried through uniforms but not used in the wall shader.

Observed implementation facts:

- Wall-fill buffer now stores one `f32` occupancy per wall texel/cell. Face bases are
  back `0`, left `nx*ny`, right `nx*ny + nz*ny`, front `nx*ny + 2*nz*ny`, floor
  `2*nx*ny + 2*nz*ny`.
- Fill rendering maps hit `y` to an exact occupancy row and samples only horizontally,
  preserving one-cell dry gaps between separated liquid patches.
- Worker gate: `cargo build --target wasm32-unknown-unknown` finished successfully.
- Wet-wall worker made `WetWallSystem` store allocation-time `dims`/`face_counts` and
  use those in `record_step`, so changing the Reset-class supersample setting no longer
  desynchronizes the uniform from the allocated buffer before Reset.
- Wet-wall supersampled texels now compute fractional contact from bilinear neighboring
  `cell_type` samples in the wall axes, instead of duplicating one source cell into every
  `ss*ss` texel. Default supersample is now 4.
- `wet_wall_gloss_strength` now scales wet-wall sun specular/broad sheen. Worker gate:
  WASM build and `cargo test --lib` passed with 28 tests.
- Review follow-up rewired `render.hero.flat_water.waterline_softness` so it feathers only
  inside occupied dense wall rows near dry neighboring rows; dry rows remain a hard gate.
- Wall-fill defaults were retuned to `fill_strength=0.45` and `fill_slab=0.018` so the
  default color/thickness contribution is subtle instead of overpowering.
- Follow-up after user feedback replaced the wall-fill per-cell occupancy with a
  supersampled fractional occupancy atlas controlled by
  `render.hero.flat_water.fill_supersample` (Reset-class, default 8, selectable to 32).
  The compute pass bilinearly samples neighboring `cell_type` values on the wall axes,
  and the render pass samples `nx*ss`/`ny*ss` or `nz*ss`/`ny*ss` wall-space texels.
- Anti-alias follow-up after user feedback changed wall-fill render sampling from a
  snapped-y / horizontal-only row sample to true bilinear coverage in both wall axes.
  `waterline_softness` now blends raw atlas coverage toward a smoothstep coverage curve,
  reducing visible stair steps without requiring higher supersample values.
- Wet-wall read blur now uses a real triangular weighted filter over the requested
  radius. The previous weighting gave taps outside the immediate bilinear pair nearly
  zero weight, so larger blur values did not meaningfully anti-alias staircases.
- The wall-fill pass now writes a fourth screen-space `R16Float` `wallfill_mask` target
  in addition to thickness/nearest-Z/whitewater. The pass runs every frame so the mask is
  always cleared; when fill is disabled the shader writes zero.
- `composite.wgsl` samples `wallfill_mask` to apply fill-only optical controls:
  `fill_color_strength`, `fill_reflection_strength`, `fill_roughness`, and
  `fill_absorption_strength`. These are Live `HeroParams` controls and do not change
  normal open-water pixels.
- Wet-wall detail defaults were raised again: `wet_wall.supersample` defaults to 8 and is
  selectable to 32; `wet_wall.blur` defaults to 2 with max 4. `WetWallSystem` clamps
  allocation to 32.
- Final gates:
  - `cargo build --target wasm32-unknown-unknown` passed.
  - `cargo test --lib` passed: 28 tests.
  - `bash app/cf-build.sh` passed and refreshed the tracked release WASM package.
  - Browser capture `captures/wall-detail-debug.png`: WebGPU present, atomic smoke PASS,
    EVAL `"wall-detail-debug"`, no WGSL/device errors.
  - Browser capture `captures/wall-detail-default.png`: WebGPU present, atomic smoke PASS,
    EVAL `"wall-detail-default"`, no WGSL/device errors; default fill values exercised.
  - Browser capture `captures/wall-detail-round2.png`: WebGPU present, atomic smoke PASS,
    EVAL `"wall-detail-round2"`, no WGSL/device errors; supersampled wall-fill atlas,
    wall-fill mask binding, and composite controls exercised.
  - Browser capture `captures/wall-aa-smoke.png`: WebGPU present, atomic smoke PASS,
    EVAL `"wall-aa-smoke"`, no WGSL/device errors; continuous wall-fill sampling and
    triangular wet-wall blur exercised.

## Exit gate

- `cd /home/adamg/fluid-simulation/app && cargo build --target wasm32-unknown-unknown`
- `cd /home/adamg/fluid-simulation/app && cargo test --lib`
- Browser capture against `http://localhost:5184/` with wall wetness/fill enabled; console
  must show healthy WebGPU boot and no WGSL/device errors.
- Visual acceptance from capture/manual inspection: no blocky wall grid up close, no
  vertical bridging between disconnected wall-water patches, and strength/thickness
  sliders produce gradual changes.

## Discipline Rules

- Code-touching subagents run sequentially; read-only agents may run in parallel.
- Do not change simulation invariants or P2G fixed-point paths.
- Do not add CPU/GPU readback to normal frames.
- Do not make performance claims without capture/profiler output.

## Migration Notes

Migrated at ship time:

- `architecture/rendering.md` now owns the current pass order, supersampled wet-wall
  update/material behavior, and dense wall-fill MRT injection behavior.
- `architecture/gpu-resources.md` now owns wetness buffer sizing at default `ss=8`, the
  supersampled wall-fill occupancy buffer plus screen-space mask target, and Reset-class
  `wet_wall.supersample` / `flat_water.fill_supersample` rebuild rules.
- `architecture/settings.md` now owns the Live-vs-Reset hero setting distinction, wet-wall
  supersample default, and flat wall-fill control semantics.
- `decisions/rendering.md` now records the render-only policy for supersampled wet walls
  and dense wall fill, including the tradeoff that the signal remains grid-derived rather
  than true droplet drainage.
- `_meta/manifest.md` now routes future `wetwall`/`wallfill` changes to these owner docs.

## See Also

- `docs/architecture/rendering.md`
- `docs/architecture/settings.md`
- `docs/decisions/rendering.md`
