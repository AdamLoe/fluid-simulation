---
status:        shipped
owner:         unassigned
last_updated:  2026-06-17
okay_to_delete: true
long_lived:    false
owning_docs:
  - architecture/simulation.md
  - architecture/settings.md
  - architecture/rendering.md
  - decisions/simulation.md
  - decisions/scope.md
---

# Water Level And Particle Density Calibration

## Mission

Make `scene.fill_level` and `particles.density` predictable across slider values,
presets, and rectangular tank sizes, without jumping straight to a tuning patch. The
first implementation stream must measure the current formulas and visible/GPU behavior,
test competing semantics, and only then change the seeding or correction model.

Done means an implementer can show, for representative tank sizes, fill levels, and
density values, that the initialized amount of water, particle count, effective
particles-per-cell, and early simulation behavior match the chosen semantics within
documented tolerances.

## Scope

In scope:

- Define and validate exact semantics for water level and particle density.
- Trace and test scene block geometry, derived particle count, actual generated
  particle count, particle spacing, splat radius, classify dilation, and occupancy
  volume correction.
- Experiment across presets, rectangular tank sizes, water levels, and density values
  before selecting an implementation.
- Add or revise host tests, WASM build verification, and browser captures once the
  implementation phase starts.
- Update owning docs after implementation: at minimum `architecture/simulation.md` and
  `architecture/settings.md`; update `decisions/simulation.md` and likely
  `decisions/scope.md` if the slider semantics change; update `architecture/rendering.md`
  if density/splat calibration changes.

Out of scope:

- Replacing the renderer with an SDF, marching-cubes, or level-set surface.
- Adding source/drain behavior, particle spawning/deletion during normal simulation, or
  open boundaries.
- Changing the pressure solver topology or introducing non-uniform cell sizes.
- Making performance claims without profiler evidence.
- Treating `particles.count` as a public control again; it remains hidden compatibility
  unless a separate product decision changes that.

## Current Findings

The current code already tries to separate water amount from particle density, but the
contract is not yet tight enough for the reported behavior.

- `scene.fill_level` is Reset-class and stored as `0..100`; `Registry::fill_level()`
  maps it to `[0,1]`.
- `preset_blocks` maps that fraction differently per preset:
  - Falling Blob scales a suspended central block about its center.
  - Dam Break creates a floor-anchored wall slab whose top is `fill * 0.98`.
  - Double Splash scales two suspended drops vertically.
- Existing docs disagree in important places. `settings.md` and `scene/mod.rs` say
  Falling Blob is a suspended central blob, while the `scene.fill_level` tooltip and
  `decisions/scope.md` describe a literal full-footprint floor slab / waterline.
- `particles.density` is intended to mean particles per seeded liquid cell, not total
  grid cell. Auto count is `round(density * seeded_volume_fraction * total_cells)`,
  floored at 1024.
- `generate_particles` does not guarantee it produces exactly the requested count. It
  distributes each block's target by volume, computes spacing as `(vol / target)^(1/3)`,
  floors per-axis lattice counts, and returns `floor(ext.x/spacing) *
  floor(ext.y/spacing) * floor(ext.z/spacing)` per block. This can make actual
  particles lower than requested, especially for thin or oddly shaped blocks.
- `effective_particle_density` is based on `resolved_particle_count`, not the actual
  generated count. That means rest-density, surface dilation, and splat spacing can be
  calibrated to requested density while the particle buffer contains a different
  effective density.
- High water amounts can plausibly compress or behave oddly because more fill creates
  high occupancy near walls and ceilings, while `divergence.wgsl` applies a one-sided
  anti-clump bias from raw occupancy over `rest_density`. Overfilled or tightly packed
  cells can hit `physics.drift_clamp`, and the pressure projection/boundary constraints
  must then push liquid outward inside a closed tank with limited free space.
- `stats_json` exposes `requested_particles`, `estimated_particles`, actual
  `particles`, `filled_volume`, and `liquid_fraction`; `filled_volume` is
  `liquid_cells * H^3`, useful as a proxy but not exact fluid mass.

## Working Semantics To Validate

Use these as the starting semantics for experiments. Change them only if measurements
show they cannot produce intuitive, stable behavior.

### Water Level

Default hypothesis: `scene.fill_level` should mean represented volume fraction of the
whole tank at reset. In normalized tank coordinates, the target is:

`target_normalized_volume = clamp(fill_level / 100, 0, 1)`

For a grid `nx * ny * nz` with uniform cell size `H`, this means:

`target_volume_world = target_normalized_volume * (nx * H) * (ny * H) * (nz * H)`

`target_seeded_cells = target_normalized_volume * nx * ny * nz`

Every preset may choose a different shape, but under this default hypothesis the sum of
its normalized liquid block volumes should approximate `target_normalized_volume`.
For Dam Break's partial X/Z footprint, that means the needed slab height is
`target_normalized_volume / footprint_area`, capped only by an explicit high-fill
policy. It is not the same as "fill percent of the footprint height." If the footprint
area is `0.333`, a 20% whole-tank fill needs about 60% height over that footprint; a
20% footprint-relative waterline would represent only about 6.7% of the whole tank.

For suspended splash presets, the same formula means equivalent represented volume,
not a literal settled y-height before gravity acts.

The plan should compare this default hypothesis to these alternatives:

- Full-footprint literal waterline: all scenarios seed from the floor across the full
  X/Z footprint up to normalized height `fill`, so represented whole-tank volume is
  exactly `fill`.
- Scenario-footprint waterline: floor-anchored scenarios seed from the floor over their
  authored X/Z footprint up to normalized height `fill`, so represented whole-tank
  volume is `fill * footprint_area`. This is cheaper to preserve for Dam Break, but it
  makes the slider mean different total water amounts across presets.
- Preset-authored amount: each preset maps the slider to a shaped block whose largest
  amount may be less than a full tank. This may be visually intentional, but it must be
  documented as "scenario amount," not "water level."

The default recommendation is target volume fraction because it best matches the user's
complaint across slider values and cube/tank sizes while preserving splash presets.

### Particle Density

`particles.density` should mean target particles per seeded cell at reset. It is a
fidelity/cost knob, not a water-volume knob.

For a fixed fill, preset, and tank size:

- Target seeded cells come from the selected water-level formula. Under the default
  whole-tank-volume hypothesis this is `target_normalized_volume * total_grid_cells`;
  under a footprint-relative or preset-authored alternative it is the measured block
  volume fraction times `total_grid_cells`.
- Target particle count = target particles-per-cell * target seeded cells.
- Actual generated particle count must be close enough to target that effective density
  and mass/correction parameters can use it honestly.
- Inter-particle spacing should be `(cell_volume / particles_per_cell)^(1/3)` for the
  chosen effective density.
- Particle mass should remain a derived constant for the chosen represented volume:
  represented volume / actual particles, or an equivalent normalized mass proxy. Total
  represented mass should track water amount, not particle density.

If the implementation keeps occupancy-based correction instead of explicit particle
mass, it must still use the actual generated effective density for rest target,
surface-dilation decisions, and render splat spacing.

## Hypotheses

Measure these before picking a fix:

1. **Preset semantic mismatch.** The UI says waterline/full tank, but Falling Blob and
   Double Splash seed suspended shaped bodies. Users read 50% as half-full while code
   creates a large falling blob.
2. **Volume fraction drift by preset/tank.** Normalized block volume is monotone but may
   not equal the slider value after clamping, especially at high fill or unusual
   aspect ratios.
3. **Requested vs actual count drift.** Lattice flooring makes actual particle count
   lower than `resolved_particle_count`, which makes the real density lower than the
   density used for rest-density, surface dilation, and splat radius.
4. **Overfilled closed-tank compression.** High fills leave too little air/free space
   for falling or sloshing water. Boundary cells are Solid, pressure uses a closed
   tank, and the occupancy anti-clump term can introduce strong divergence bias in
   overpacked cells.
5. **Density/correction mismatch.** `physics.rest_density` Auto tracks requested
   effective density, but `occ[c]` is raw actual per-cell occupancy. If actual density
   differs from requested density, the correction is biased.
6. **Grid occupancy assumptions.** Very dense or high-fill cases may classify too many
   cells liquid, reducing air interface quality and making pressure projection appear
   compressed or rubbery.
7. **Render coverage masks physics issues.** Splat radius scaling can make visible
   volume look acceptable while liquid-cell volume, occupancy, or pressure behavior is
   wrong.

## Measurement Plan

Start with host-side deterministic measurements so the experiment matrix is cheap, then
use WASM/capture only for the cases that distinguish hypotheses.

For every case, record:

- preset, grid resolution, `H`, tank extents, slider fill fraction, density value, and
  hidden `particles.count` override state.
- normalized block min/max, block volume, target filled fraction, seeded cells, target
  particle count, estimated lattice count, actual generated count, and actual
  effective particles-per-seeded-cell.
- initial free-air margin: top of the seeded block/AABB to the tank ceiling, empty cell
  layers above the initial liquid where measurable, and total non-liquid interior-cell
  fraction.
- inter-particle spacing, splat radius input, effective surface dilation, effective
  rest density, volume stiffness, and drift clamp.
- after reset and after fixed warm-up windows: actual particles, liquid cells,
  `filled_volume`, `liquid_fraction`, pressure iterations, dropped timestep, scale
  status, and GPU timing source.
- stability/compression signals where feasible: fluid AABB min/max and height drift,
  boundary-adjacent liquid cells, ceiling-adjacent liquid cells, occupancy histogram
  or at least max/p95 occupancy, cells where `occ > rest`, and a clamp-hit proxy count
  for cells where `min(volume_stiffness * max(0, occ - rest) / rest, drift_clamp)`
  reaches `drift_clamp`.
- visual evidence only from browser captures; do not infer visible behavior from host
  formulas alone.

Add a small measurement helper if useful, but keep it deterministic and reusable. A host
unit helper can expose the pure seeding math without allocating massive GPU buffers. A
browser sweep can extend `tools/capture.mjs` or follow `tools/density_motion_sweep.mjs`.

## Experiment Matrix

### Tank sizes

Use a bounded set that covers cubes and the current rectangular default:

- `32x32x32` for cheap host and browser sanity.
- `64x64x64` as the historical cube.
- `80x40x80` as the current default rectangular tank.
- `128x64x128` as the large/stretch case when preflight accepts it.
- One thin/high-aspect case such as `96x24x96` or `48x96x48` to expose clamping and
  floor/ceiling issues.

### Presets

- Falling Blob: default and most likely user-facing mismatch.
- Dam Break: floor-anchored waterline-like control.
- Double Splash: multiple suspended blocks and volume splitting.

### Water levels

Use `0`, `5`, `20`, `50`, `80`, and `100`. Treat `0` as a semantics test: either it
means empty/no particles, or it remains a minimum viable seed. The chosen behavior must
be explicit in settings help and tests.

### Density values

Use `1`, `2`, `8`, `16`, and `32`. Density `8` is the reference. Low density tests
surface dilation and visual pinholes; high density tests occupancy and closed-tank
compression.

### Physics correction variants

For a reduced subset of visual captures, compare:

- current Auto rest density, current `volume_stiffness`, current `drift_clamp`.
- Auto rest density computed from actual generated density.
- `volume_stiffness = 0` to isolate projection/advection from occupancy bias.
- lower `drift_clamp` for high-fill cases, only as an experiment.

Do not ship a correction change until the visual captures and stats explain the
improvement.

## Post-Measurement Checkpoint

No behavior-changing implementation may start immediately after instrumentation. The
measurement pass must first write a compact results table back into this plan or the
orchestration hub, including:

- the measured current formula results for the experiment matrix subset that was run;
- the selected water-level semantics, with the exact formula;
- the selected particle-density semantics, including whether effective density uses
  requested or actual generated count;
- which hypothesis or hypotheses the evidence supports;
- any revised numeric tolerances, with the observed data that justifies the change.

Only after that checkpoint may an implementer change scene geometry, particle
generation, density calibration, correction constants, or user-facing setting help.

## Measurement Checkpoint 2026-06-17

Measurement-only instrumentation landed in `app/crates/fluid-lab/src/scene/mod.rs`
under `#[cfg(test)]`; it mirrors the current host-side seeding formulas without changing
runtime behavior. The helper records normalized block volume, requested count, exact
lattice-generated count, requested vs actual effective density, top-air margin, free
fraction, and a uniform-density clamp proxy. Measurements below use the current registry
defaults unless noted, including `scene.drop_height = 1.0`, which clamps suspended
Falling Blob / Double Splash blocks against the ceiling.

| Preset | Grid | Fill | Density | Seeded frac | Requested | Generated | Actual/requested density | Top margin | Free frac | Clamp proxy |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| Falling Blob | 32x32x32 | 0.20 | 8 | 0.199888 | 52,399 | 50,193 | 7.663 / 8.000 | 0.000 | 0.800 | 0.000 |
| Falling Blob | 64x64x64 | 0.50 | 8 | 0.433959 | 910,077 | 893,022 | 7.850 / 8.000 | 0.000 | 0.566 | 0.000 |
| Falling Blob | 80x40x80 | 0.50 | 1 | 0.433959 | 111,093 | 107,736 | 0.970 / 1.000 | 0.000 | 0.566 | 0.000 |
| Falling Blob | 80x40x80 | 0.50 | 8 | 0.433959 | 888,747 | 879,844 | 7.920 / 8.000 | 0.000 | 0.566 | 0.000 |
| Falling Blob | 80x40x80 | 0.50 | 32 | 0.433959 | 3,554,988 | 3,493,413 | 31.446 / 32.000 | 0.000 | 0.566 | 0.000 |
| Falling Blob | 80x40x80 | 0.80 | 8 | 0.651619 | 1,334,516 | 1,306,397 | 7.831 / 8.000 | 0.000 | 0.348 | 0.000 |
| Dam Break | 80x40x80 | 0.50 | 8 | 0.163170 | 334,172 | 329,043 | 7.877 / 8.000 | 0.510 | 0.837 | 0.000 |
| Dam Break | 80x40x80 | 1.00 | 8 | 0.326340 | 668,344 | 658,086 | 7.877 / 8.000 | 0.020 | 0.674 | 0.000 |
| Double Splash | 80x40x80 | 0.50 | 8 | 0.157920 | 323,420 | 311,220 | 7.698 / 8.000 | 0.000 | 0.842 | 0.000 |
| Falling Blob | 96x24x96 | 0.80 | 8 | 0.651619 | 1,153,022 | 1,131,008 | 7.847 / 8.000 | 0.000 | 0.348 | 0.000 |

Observed current formulas:

- Falling Blob uses a suspended authored block scaled about its center. With registry
  default `drop_height = 1.0`, it is ceiling-clamped even at 20% fill; at 50% fill it
  represents `0.433959` of the tank, not the requested `0.5`.
- Dam Break uses footprint-relative waterline semantics: normalized represented volume
  is approximately `0.37 * 0.90 * 0.98 * fill`, so 50% fill represents only `0.163170`
  of the whole tank and 100% represents `0.326340`.
- Double Splash uses two suspended authored drops; at 50% fill it represents `0.157920`
  of the whole tank and is also ceiling-clamped at the registry-default drop height.
- Lattice flooring makes generated particles consistently lower than requested. The
  measured subset ranges from roughly `-1.0%` to `-4.2%`, with actual effective density
  lower than the density/rest target that runtime formulas currently derive from the
  requested count.
- At fixed fill/preset/grid, changing density does not change seeded geometry or seeded
  cells. The density control currently changes requested/generated count and spacing,
  but requested effective density still uses the requested count, not the generated
  count.
- The host-only uniform-density clamp proxy is `0` for the measured subset because the
  actual lattice density is below requested/rest density. This does not rule out GPU
  compression from local occupancy hot spots, ceiling/wall contact, pressure projection,
  or warm-up motion; it only says the average actual-vs-rest mismatch does not itself
  predict clamp hits.

Selected next-pass hypotheses:

- Water-level behavior should be tested by implementing the **canonical target volume
  fraction** direction first: `target_normalized_volume = clamp(fill_level / 100, 0, 1)`.
  Presets may keep distinct shapes, but their total represented volume should match that
  target unless a documented high-fill guardrail clamps it.
- Particle-density behavior should be tested with **actual-count-calibrated density**:
  derive effective density, auto rest target, surface-dilation decisions, splat spacing,
  and diagnostics from the generated lattice count divided by selected seeded cells, not
  from requested count alone.
- Supported hypotheses from this pass: preset semantic mismatch, volume fraction drift
  by preset, requested-vs-actual count drift, density/correction mismatch, and high-fill
  closed-tank compression risk for suspended presets because the default drop height
  leaves no top-air margin. The pass did not provide GPU evidence for or against render
  coverage masking or warm-up compression.

Revised tolerance notes for the next behavior pass:

- Keep the existing `+/-5%` ordinary generated-count tolerance for now; current flooring
  stays inside it on the measured subset. If the next pass changes the generator, target
  a tighter ordinary tolerance only after measuring the new lattice behavior.
- Add an explicit top-air-margin observation to high-fill acceptance: host formulas must
  report whether seeded blocks touch the ceiling before browser warm-up. A numeric
  ceiling-compression threshold still needs GPU/capture evidence.
- Browser capture was deferred in this measurement pass because the canonical capture
  workflow rebuilds `app/web/pkg`, while `app/web/**` is outside this pass's allowed file
  set. Serving an existing package would risk measuring stale WASM.

## Implementation Shipped 2026-06-17

The first behavior pass implemented the selected checkpoint direction, but follow-up
visual review rejected that outcome because it made the presets occupy far more space
and degraded the tuned look. The whole-tank volume semantics below are superseded by
the regression correction that follows.

- `scene.fill_level` now targets whole-tank represented volume:
  `target_normalized_volume = clamp(fill_level / 100, 0, 1)`.
- Falling Blob and Double Splash remain suspended presets; Dam Break remains
  floor-anchored. Their block volumes target the whole-tank fraction rather than a
  preset-footprint fraction.
- Suspended and near-full cases use an explicit `0.02` normalized top-air guardrail.
  A 100% fill therefore represents `0.98` of the tank instead of silently ceiling
  clamping a suspended overfill. A 0% fill produces no represented liquid blocks.
- `particles.density` remains a requested particles-per-seeded-cell fidelity/cost
  target. The deterministic lattice can trail the request, so reset-time effective
  density, Auto rest density, auto surface dilation, splat spacing, and diagnostics
  use generated lattice count divided by seeded cells where the generated count is
  available.
- Host tests now enforce whole-tank target volume for presets, zero-fill behavior,
  density-invariant represented volume, generated-count effective density, requested
  vs generated-count tolerances, and the high-fill guardrail.

Verification:

| Gate | Result |
|---|---|
| `cd app && cargo test --lib` | PASS, 79 tests |
| `cd app && cargo build --target wasm32-unknown-unknown` | PASS |
| `cd app && wasm-pack build crates/fluid-lab --target web --out-dir ../../web/pkg --dev` | PASS; regenerated tracked `app/web/pkg` outputs |
| `cd app && ./local_dev.sh` | PASS; served canonical static path at `http://localhost:5184/` |

Browser capture commands used `tools/capture.mjs` through Windows Chrome against the
canonical static URL with `?set=id:value` reset settings. Every listed capture passed
with `scale_status: "ok"`, `timing: "gpu-timestamp"`, WebGPU smoke PASS, and
`gpuDeviceStatus: "ok"`.

| Capture | Case | Requested | Generated | Notes |
|---|---|---:|---:|---|
| `wlc-fb64-f20-d8-url.png` | 64x64x64 Falling Blob, fill 20, density 8 | 419,430 | 411,906 | Whole-tank 20% target; generated trails request by ~1.8%. |
| `wlc-fb64-f80-d8.png` | 64x64x64 Falling Blob, fill 80, density 8 | 1,677,721 | 1,640,625 | High-fill suspended case stayed within scale limits. |
| `wlc-fb80x40-f50-d8.png` | 80x40x80 Falling Blob, fill 50, density 8 | 1,024,000 | 1,005,536 | Rectangular default tank, whole-tank 50% target. |
| `wlc-dam80x40-f20-d8.png` | 80x40x80 Dam Break, fill 20, density 8 | 409,600 | 407,808 | Dam Break no longer uses the old footprint-relative small volume. |
| `wlc-dam80x40-f80-d8.png` | 80x40x80 Dam Break, fill 80, density 8 | 1,638,400 | 1,628,640 | High-fill floor-anchored case stayed within scale limits. |
| `wlc-fb32-f50-d1/d8/d32.png` | 32x32x32 Falling Blob, fill 50, densities 1/8/32 | 16,384 / 131,072 / 524,288 | 14,872 / 126,405 / 512,975 | Fixed represented volume, cost scales with density. |
| `wlc-dam32-f50-d1/d8/d32.png` | 32x32x32 Dam Break, fill 50, densities 1/8/32 | 16,384 / 131,072 / 524,288 | 15,624 / 127,224 / 513,513 | Floor-anchored density comparison. |

Capture caveat: `liquid_fraction`/`filled_volume` are occupied-cell proxies after
warm-up, not exact mass. The density-32 captures show lower occupied-cell fraction
than density 1/8 because low/reference-density cases get the auto surface-dilation
rind while high density does not. The source-of-truth represented volume and particle
mass target are the seeded block volume and generated particle count, which the host
tests cover directly.

## Regression Correction 2026-06-17

User visual review rejected the whole-tank behavior: the fluid took up far more space
than before and looked materially worse. The correction preserves the density fix but
restores the water amount semantics to preset-authored scale:

- `scene.fill_level` is a scenario amount, not a universal whole-tank volume target.
- Falling Blob scales as a suspended blob around the tuned 20% default.
- Dam Break grows taller inside its authored wall footprint instead of expanding to
  occupy a whole-tank volume fraction.
- Double Splash stretches its two authored suspended drops.
- `particles.density` remains volume-neutral: it changes requested/generated count and
  spacing, not seeded geometry.
- Generated lattice count remains the calibration source for effective density, Auto
  rest density, auto surface dilation, splat spacing, and diagnostics where available.

Corrective tests pin preset-authored scale (`Dam Break` 50% at roughly `0.163` seeded
fraction, live-default `Falling Blob` 50% at roughly `0.434`) while keeping the
generated-count density tests.

## Candidate Implementation Directions

Pick only after the measurements identify the failing contract.

1. **Doc/help correction only.** If the math is stable and the problem is mostly user
   expectation, update settings help and decisions to say fill controls preset volume,
   not literal y-height. This is the smallest fix but only acceptable if captures show
   behavior is stable.
2. **Canonical target volume fraction.** Make each preset's seeded block volume match
   the slider more exactly across tank sizes, with explicit clamping near walls. This
   preserves splash shapes while making the slider physically consistent.
3. **Literal waterline mode for floor-fill semantics.** Rework default fill to seed a
   floor slab or add a separate preset/mode for literal tank waterline. This is a
   product semantics change and must update `decisions/scope.md`.
4. **Actual-count-calibrated density.** Use actual generated lattice count, not just
   requested count, to derive effective density, rest target, splat radius, and
   diagnostics.
5. **Better lattice generation.** Replace per-axis floor-only counts with a generator
   that hits target count/density more closely while keeping deterministic jitter and
   spacing bounds.
6. **High-fill guardrails.** Cap unstable high-fill/preset combinations, change 100%
   semantics to leave interior margin, or adjust occupancy correction for near-full
   closed-tank states. This needs clear UI/docs language.
7. **Correction tuning.** Retune `volume_stiffness`, `drift_clamp`, or auto rest-density
   only if experiments show compression comes primarily from the occupancy bias.

## Acceptance / Verification

Implementation is not done until these checks are satisfied for the selected direction:

- Host tests cover `scene.fill_level` semantics for all presets, including `0`, `20`,
  `50`, and `100`, and at least one rectangular tank.
- Host tests verify requested count, estimated/generated count, and effective density
  stay within documented tolerance for representative densities and block shapes.
- Host tests verify density changes do not change target water volume at fixed fill.
- Host tests cover any changed `particles.count` override interaction.
- Provisional numeric tolerances, to revise only through the post-measurement
  checkpoint:
  - target normalized block volume is within `+/-0.02` absolute whole-tank fraction of
    the selected water-level formula for ordinary `5..80%` cases, and within `+/-0.03`
    for explicitly clamped `100%` or high-aspect cases;
  - actual generated particle count is within `+/-5%` of target for ordinary cases and
    within `+/-10%` for thin/high-aspect or very-low-count cases unless the generator is
    deliberately changed to a tighter guarantee;
  - actual effective particles-per-seeded-cell is within the same percentage as the
    generated-count tolerance;
  - at fixed fill/preset/tank, changing density does not change target represented
    water volume at all, and measured initial block volume remains within `+/-2%`
    relative across densities;
  - for capture proxies at fixed fill, `liquid_fraction` / `filled_volume` after the
    agreed warm-up stay within `+/-15%` relative of the density-8 baseline unless the
    screenshots and occupancy data justify a different proxy tolerance;
  - high-fill warm-up AABB height drift, ceiling-adjacent liquid count, boundary-touch
    count, and clamp-hit proxy do not indicate persistent ceiling/wall compression; if
    a numeric threshold is chosen during measurement, record it in the checkpoint
    before behavior changes.
- `cargo test --lib` passes from `app/`.
- `cargo build --target wasm32-unknown-unknown` passes from `app/`.
- Browser captures via `tools/capture.mjs` succeed for at least:
  - `64x64x64`, Falling Blob, fill `20/50/80`, density `8`.
  - `80x40x80`, Falling Blob, fill `20/50/80`, density `8`.
  - `80x40x80`, Dam Break, fill `20/50/80`, density `8`.
  - one fixed fill across densities `1/8/32`.
- Capture outputs include screenshots plus stats JSON or console lines with particles,
  requested/estimated particles, `liquid_fraction`, `filled_volume`, scale status, and
  timing source.
- No unsupported performance claim is made. If timing is discussed, cite profiler or
  capture stats and distinguish GPU timestamp from CPU/fence timing.
- High-fill cases should not show obvious wall/ceiling compression artifacts after the
  agreed warm-up unless the UI explicitly marks them as intentionally extreme.

## Handoff Notes

- Start with `docs/architecture/simulation.md`, `docs/architecture/settings.md`,
  `docs/architecture/rendering.md`, `docs/decisions/simulation.md`, and
  `docs/decisions/scope.md`.
- Code routes: `app/crates/fluid-lab/src/scene/mod.rs`,
  `app/crates/fluid-lab/src/settings/mod.rs`,
  `app/crates/fluid-lab/src/gpu/fluid.rs`,
  `app/crates/fluid-lab/src/gpu/mod.rs`,
  `app/crates/fluid-lab/src/gpu/shaders/divergence.wgsl`,
  `app/crates/fluid-lab/src/profiler/mod.rs`, and `app/tools/capture.mjs`.
- Coordinate with
  `docs/plans/orchestrator/2026-06-17-water-level-density-calibration-hub.md`.
- Treat the existing shipped
  `docs/plans/scenario-bootstrap-visual-readiness.md` as background only; do not reopen
  startup readiness unless experiments show resets are applying stale settings.
- The first implementation step should be measurement/instrumentation, not a behavior
  change.

## Migration Notes

At ship time, migrate:

- final water-level and density semantics to `architecture/simulation.md` and
  `architecture/settings.md`;
- any density/splat-radius or visible-volume calibration change to
  `architecture/rendering.md`;
- any semantic tradeoff to `decisions/simulation.md` and `decisions/scope.md`;
- any new verification command or capture workflow to `agent-context/build-run.md` only
  if the standard workflow changes.

Migrated:

- Final water-level and density semantics to `architecture/simulation.md` and
  `architecture/settings.md`.
- Generated-count splat spacing semantics to `architecture/rendering.md`.
- Whole-tank preset policy and density/rest-density rationale to
  `decisions/scope.md` and `decisions/simulation.md`.
- No workflow change was made; the existing `agent-context/build-run.md` capture path
  remains current.

## See Also

- `docs/plans/orchestrator/2026-06-17-water-level-density-calibration-hub.md`
- `docs/architecture/simulation.md`
- `docs/architecture/settings.md`
- `docs/architecture/rendering.md`
- `docs/decisions/simulation.md`
- `docs/decisions/scope.md`
