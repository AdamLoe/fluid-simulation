---
status:        shipped
owner:         unassigned
last_updated:  2026-06-13
okay_to_delete: true
long_lived:    false
owning_docs:
  - architecture/simulation.md
  - architecture/rendering.md
  - decisions/scope.md
---

# Volume / density decoupling: waterline-height + volume-neutral density

## Mission

Today a single conflated knob (`particles.density`, default 8 per seeded
cell) controls both *how much water there is* and *how finely it is
resolved*. Because the visible body of water is built from **fixed-radius
particle splats** (not from liquid cells), lowering density makes the
splats stop overlapping and the water *looks like there is less of it* —
even though the seeded region is identical. That is wrong: density should
be a pure fidelity/cost knob.

This plan splits the concept into **two orthogonal, user-legible sliders**:

- **Liquid volume = waterline height** — how deep the water sits in the
  tank. Raising it makes the body deeper; particle count follows
  automatically (it already does, via `resolved_particle_count`).
- **Particle density = particles per seeded cell** — fidelity/cost only.
  Lowering it must keep the *same* visible body of water (just blobbier /
  coarser, possibly sloshing differently — acceptable).

The decoupling is achieved cheaply (user-confirmed): **scale the render
splat radius with the inter-particle spacing** so coverage stays roughly
constant as density changes, plus **auto-enable the existing
`surface_dilation`** so the physics liquid region stays hole-free at low
density. The SDF surface rewrite is explicitly **out of scope** (deferred
to a future plan).

The defining deliverable is **empirical calibration**: a programmatic way
to measure how much water a given `(waterline, density)` actually produces,
and to assert the invariant *"visible volume is ~constant across density at
a fixed waterline."* This is what lets us tune the radius coefficient until
the sliders behave. **Done** = waterline changes water depth, low-vs-high
density at the same waterline shows ~the same volume (blobbier at low), the
fast liquid-cell invariant holds in a host/integration check, `cargo build`
(wasm) + `cargo test --lib` are green, and a `tools/capture.mjs` sweep
shows it visually.

## Background / ground truth (verified against code)

- **Visible water = fixed-radius splats.** `gpu/mod.rs:309` and `:526`:
  `let particle_radius = crate::sim::H * 0.35;` with `sim::H = 2.0/64.0`
  (`sim/mod.rs:27`). This radius (× `render.particle_size` via
  `particles.set_radius_scale`, mod.rs:337/600/849) is splatted by
  `particles.draw_thickness(...)` (mod.rs:~1412) and screen-space smoothed
  into the surface. The radius is **independent of density** → sparse
  splats pinhole. `particles.rs` already has `base_radius`/`radius_scale`/
  `set_radius_scale` (lines 27–28, 264–265, 278) and recomputes a
  `volume_scale` kernel normalization on change (`recompute_volume_scale`,
  ~362). **`set_radius_scale` is the lever for fix #2.**
- **Seeding & count.** `scene/mod.rs`:
  `resolved_particle_count` (147–159) =
  `round(total_cells * seeded_volume_fraction(blocks) * density)`, floored
  1024; `particles.count` override (`particle_count_override`) wins when
  nonzero. `seeded_volume_fraction` (126–135) = summed normalized block
  volume. `preset_blocks` (162–185) gives normalized `[0,1]³` AABBs;
  suspended presets are shifted by `scene.drop_height` via `shift_block_y`
  (187–192), dam-break is floor-anchored and ignores drop height.
- **Spacing.** `generate_particles` (`gpu/fluid.rs:1436–1509`) lays a
  deterministic jittered lattice with `spacing = (vol/target).cbrt()` per
  block. Lowering count **enlarges spacing within the same region** (does
  not shrink the region) — exactly why fixed-radius splats pinhole.
- **Classification & dilation.** `classify.wgsl`: a cell is Liquid iff
  `occ >= max(liquid_threshold,1)`, OR (only if `surface_dilation>=1`) a
  6-neighbour is filled. The dilation is **already implemented but
  defaults OFF** (`classify.surface_dilation`, settings ~698, default 0,
  Live). Wired through `cls: [...]` in `fluid.rs:264`. `liquid_cells` is
  counted into `stats[0]` and surfaced everywhere (timing.rs:128,
  profiler 142/272/389/496, `stats_json`).
- **Stats already exposed.** `stats_json` (lib.rs:834, profiler:432–496)
  emits `liquid_cells`, `requested_particles` (= `scene.particle_count`,
  gpu/mod.rs:292), `particles` (seeded), `total_cells`, `grid_res`.
  `set_setting(id, f64)` and `reset()` are wasm-exported (lib.rs:630, 517).
  `tools/capture.mjs` already drives `set_setting`+`reset`+`stats_json`
  and takes canvas screenshots (capture.mjs:142, 345–360, 408).
- **eb75ec0** ("…add particle-density control") is HEAD~1: it shipped
  `particles.density` (Reset, default 8), reworked `particles.count` into
  Auto(0)/override, added `resolved_particle_count`, and a panel
  "Effective scenario" readout (`appendScenarioSummary`, panels.js:357).
  **This plan builds on that — it does not re-add density.**

## Scope

**In scope**

1. A **waterline-height** volume setting that deepens/shallows each
   preset's fill from the floor up; count follows automatically.
2. **Volume-neutral density** via splat-radius scaling (`set_radius_scale`
   driven by seeded spacing), with a single tunable coefficient.
3. **Auto `surface_dilation`** at low density so the physics liquid region
   stays hole-free.
4. **Calibration & testability** (first-class): a fast liquid-cell volume
   proxy + density-invariance assertion (host/integration), and a
   capture-based visual-coverage sweep to tune the radius coefficient.
5. **Panel reconciliation** with eb75ec0's density/count: waterline +
   density + resolved-count readout + an effective-volume readout; the
   `particles.count` override still wins and is documented as such.

**Out of scope (deliberate cut line)**

- The **SDF / marching-cubes surface rewrite** (the "proper" fix). The
  splat-radius approach is a coverage approximation; it can look blobby at
  very low density and that is accepted. SDF is a separate future plan.
- Changing the **P2G mass / physics** per particle (mass stays as-is;
  determinism preserved). We only change *render radius* and *cell
  classification dilation*, never the deterministic seeding RNG or the
  i32 fixed-point scatter.
- Any **per-preset art redesign** beyond defining the waterline mapping.

## Design

### 1. Waterline-height volume knob

**Setting.** Add `scene.fill_level` (label "Water level"), Category
`Scene`, **`ApplyClass::Reset`** (matches `scene.drop_height` and grid
res — geometry/count changes require rebuilding scene buffers). Validation
`F32Range { min: 0.0, max: 1.0 }`, default chosen per-preset to preserve
today's look at the default value (see below). Add accessor
`fill_level(&self)` in settings/mod.rs and include it in the Scenario tab
ordering.

**Semantics — waterline rises from the floor.** `fill_level` ∈ [0,1] sets
the **top of the water (max.y)** for floor-resting presets and the
**fill fraction** for suspended presets, scaling about the floor (most
intuitive: 0 ≈ empty, 1 ≈ full tank). Concretely, in `preset_blocks`
(after the existing drop-height shift), apply a per-preset waterline map:

- **DamBreak** (floor slab `[0.05..0.42] × [0.05..0.95] × …`): the slab is
  the body of water. Set `max.y = lerp(min.y, 0.95, fill_level)` so the
  wall slab gets taller/shorter from the floor. Default `fill_level` is
  chosen so `max.y == 0.95` reproduces today (≈ `0.947` given
  `min.y=0.05`, or simply define the map as
  `max.y = 0.05 + fill_level*(0.95-0.05)` and set default ≈ `1.0`). Keep
  the existing X/Z extent.
- **FallingBlob** (suspended centered blob): keep the blob's *thickness*
  but **raise/lower the resting waterline it falls into**. Simplest
  coherent choice for a "suspended" preset: add a **floor pool** whose
  top is `fill_level`-controlled, OR (preferred, smaller change) scale the
  blob's vertical extent about its current center so a higher level = a
  bigger blob = deeper resulting pool. **Decision needed** (see Open
  decisions): for Phase 1, use the *vertical-extent-scaling* variant —
  `max.y` grows toward 1.0 and `min.y` toward 0 proportionally to
  `fill_level`, centered on the existing block center, clamped to the
  tank — because it reuses `seeded_volume_fraction` cleanly and needs no
  new pool geometry. Default = the `fill_level` that reproduces today's
  `[0.55..0.9]` block.
- **DoubleSplash** (two suspended columns): scale each column's vertical
  extent the same way FallingBlob does, symmetric about each column's
  center; default reproduces `[0.45..0.92]`.

Because the blocks shrink/grow in normalized space,
`seeded_volume_fraction` → `resolved_particle_count` **track the waterline
automatically** with no extra wiring. Add a helper
`fn apply_fill_level(block, fill_level, preset) -> LiquidBlock` next to
`shift_block_y`, ordered **after** the drop-height shift, with the same
`clamp` discipline (`[0,1]`, preserve validity).

**Interaction with `drop_height`.** `drop_height` positions suspended
blocks vertically; `fill_level` sizes the water body. They compose:
shift first, then resize about the (shifted) center, then clamp. Document
this in `scene/mod.rs` and `simulation.md`.

**Files:** `scene/mod.rs` (preset_blocks + helper + tests),
`settings/mod.rs` (definition + accessor + Scenario ordering + the
`set_value_f64` change-detection list near 2473), `web/panels.js`
(Scenario tab; show under/next to density).

### 2. Density → volume-neutral via splat-radius scaling

**Principle.** Splat coverage of the smoothed thickness surface is roughly
constant when the **splat radius tracks the inter-particle spacing**.
Spacing `s = (cell_volume / density)^(1/3) = H · density^(-1/3)` (since one
seeded cell has volume `H³` and holds `density` particles on the lattice).
At the reference density `d0 = 8` the current look uses
`particle_radius = H · 0.35`. So define:

```
radius(density) = H · k_radius · (d0 / density)^(1/3)
                = (H · k_radius · d0^(1/3)) · density^(-1/3)
```

with `k_radius` chosen so that at `density = d0` it equals today's value:
`k_radius · d0^(1/3) = 0.35` → `k_radius ≈ 0.35 / 8^(1/3) = 0.175`. This
makes radius ∝ `density^(-1/3)` ∝ spacing: halving particles-per-cell
(`density 8→1`, `8^(1/3)/1^(1/3) = 2×`) doubles the splat radius, keeping
the *summed splatted area* ≈ constant.

**Where it lives — single tunable coefficient.** Introduce one constant in
the renderer (e.g. `const SPLAT_RADIUS_PER_SPACING: f32 = 0.35;` i.e. the
radius-to-spacing ratio at the reference) so the formula reads
`radius = SPLAT_RADIUS_PER_SPACING · spacing`, where `spacing` is computed
from the *actual seeded layout* rather than re-derived. **Prefer plumbing
the real seeded spacing** out of `generate_particles`: it already computes
`spacing = (vol/target).cbrt()` per block — expose a representative
(volume-weighted mean or the dominant block's) `seeded_spacing` on
`SceneConfig`/`GpuFluid` (a single `f32`), so the radius matches reality
even when the override `particles.count` is set (the override changes
spacing too, and the splat should follow). Fall back to the closed-form
`H · density^(-1/3)` when no scene is available.

**Plumbing.** `gpu/mod.rs:309` and `:526`: replace the fixed
`crate::sim::H * 0.35` with `fluid.seeded_spacing() * SPLAT_RADIUS_PER_SPACING`
(or the closed form). The user's `render.particle_size` slider stays as the
**Live** multiplier on top, via `set_radius_scale(settings.particle_size())`
— so users can still fine-tune, but the *default* now tracks density.
`recompute_volume_scale` already keeps the kernel normalization consistent
when the radius changes.

**ApplyClass.** The base radius depends on density/count (Reset-class
inputs) and the *spacing*, so it is naturally recomputed on reset alongside
the renderer rebuild (mod.rs:526 path). `render.particle_size` stays Live.
No new Live wiring required for Phase 1; if we later want the radius to
update without a reset we can recompute `base_radius` in the density Live
path, but density is Reset so that is unnecessary.

**Files:** `gpu/mod.rs:309/526` (radius derivation), `gpu/fluid.rs`
(expose `seeded_spacing`), `gpu/particles.rs` (uses existing
`set_radius_scale`/`base_radius`; possibly add a setter for base radius if
cleaner), `architecture/rendering.md` (document the spacing-tracking splat
radius and the coefficient).

### 3. Auto `surface_dilation` at low density

The render fix keeps the *picture* full, but the *physics* liquid region
(`classify.wgsl`) still pinholes when `occ < threshold` in sparse cells.
Enable the **already-implemented** one-ring dilation automatically.

**Trigger (Phase 1, simplest robust choice): always-on one ring when the
seeded density is below the reference.** Compute an *effective* dilation =
`max(user_surface_dilation, auto)` where
`auto = 1 if effective_density < d0 else 0` (or, even simpler and
defensible: default the auto ring **on whenever density < 8**, off at/above
8 to preserve today's tight surface at full density). The user's
`classify.surface_dilation` setting remains an explicit override that can
force it on at any density.

**Where.** The cleanest seam is in `fluid.rs` where the `cls:` uniform is
populated (~264): set `cls[1] = effective_surface_dilation(settings,
scene)` instead of the raw setting. `effective_density` = density used by
`resolved_particle_count`, or derive from
`requested_particles / seeded_cells`. Keep it a pure host-side function so
it is unit-testable.

**Files:** `gpu/fluid.rs` (~264, the `cls` population + helper),
`settings/mod.rs` (tooltip update noting auto behavior; no new setting
strictly required, but consider exposing the threshold as a dev/advanced
constant), `classify.wgsl` (no shader change needed — dilation already
implemented), `simulation.md`.

### 4. Calibration & testability (first-class deliverable)

Two proxies — a **fast physical proxy** (host/integration, the Phase-1
gate) and a **visual proxy** (capture sweep, tunes the radius coefficient).

**(a) Fast physical proxy — liquid-cell volume invariance.**

`liquid_cells` already exists in `stats_json`. Define
**filled volume = liquid_cells × cell_volume** (`cell_volume = H³`,
constant). With **dilation on** (fix #3), the liquid region is the seeded
body regardless of density, so at a **fixed waterline** filled volume must
be ~constant across a density sweep. Deliverables:

- A **host unit test** (cargo test --lib, in `scene/mod.rs`) on the pure
  derivation: for each preset, `seeded_volume_fraction` (hence
  `resolved_particle_count`) is **independent of density** and **monotone
  increasing in `fill_level`**. This is deterministic and needs no GPU.
- A **density-invariance integration check** via `tools/capture.mjs`:
  sweep `particles.density ∈ {1,2,4,8}` at a fixed `fill_level` and fixed
  grid, with effective dilation on; read `liquid_cells` from `stats_json`
  after warm-up; assert
  `max(liquid_cells)/min(liquid_cells) ≤ 1 + TOL_VOL` (propose
  **TOL_VOL = 0.15** — 15%, generous because dilation adds a density-
  dependent rind; tighten empirically). Also assert a **waterline
  monotonicity** leg: at fixed density, `liquid_cells` strictly increases
  across `fill_level ∈ {0.25,0.5,0.75,1.0}`.

**(b) Visual proxy — thickness/coverage sweep (tunes `k_radius`).**

Add a coverage metric to the capture harness: after warm-up, count
**non-background water pixels** in the canvas screenshot (the smoothed
thickness surface), e.g. pixels whose color falls in the water band, OR —
cleaner if cheap — integrate the thickness buffer if we expose a scalar
`thickness_sum` in `stats_json` (optional stretch). Sweep
`density ∈ {1,2,4,8}` at fixed `fill_level`; assert the **water-pixel
count is ~constant**: `max/min ≤ 1 + TOL_COVER` with **TOL_COVER = 0.20**.
This is the loop used to *tune `k_radius`/`SPLAT_RADIUS_PER_SPACING`*: if
low density under-covers, raise the coefficient; if it bloats the body,
lower it.

**Sweep procedure (write to `tools/` or document in rendering.md):**

1. Fix `grid.res_* = 64`, `scene.preset = dam-break` (largest stable
   body, least suspended-fall noise), `fill_level = 0.75`.
2. For `density in [8,4,2,1]`: `set_setting("particles.density", d)`;
   `reset()`; warm up ~`waitMs`; read `stats_json.liquid_cells` and the
   screenshot water-pixel count.
3. Emit a small JSON report; assert (a)-invariance and (b)-coverage
   tolerances; screenshot each density for the visual gate.

**Files:** `scene/mod.rs` (unit tests), `app/tools/capture.mjs` (sweep mode
+ coverage metric + assertions; reuse existing `set_setting`/`reset`/
`stats_json` plumbing), optionally `profiler/mod.rs` + `lib.rs` if we add
`thickness_sum` to `stats_json`, `architecture/rendering.md` (document the
calibration loop).

### 5. Panel & settings reconciliation (with eb75ec0)

- **Do not duplicate density.** Keep eb75ec0's `particles.density` (Reset)
  and `particles.count` (Auto-0/override). Add `scene.fill_level` next to
  them on the **Scenario tab**, ordered: *Preset → Water level
  (`fill_level`) → Grid res → Particle density → (advanced) Particle
  count override*.
- **Readouts.** Extend `appendScenarioSummary` (panels.js:357) to show,
  alongside the resolved/seeded particle counts, an **effective-volume
  readout**: `liquid_cells × H³` (or `liquid_cells` plus a fraction of the
  tank `liquid_cells / total_cells`). This makes the waterline knob's
  effect legible and gives the user the same number the calibration check
  asserts on.
- **Override precedence unchanged & documented:** if `particles.count > 0`
  it still wins over density (count fixed) but the **splat radius now
  follows the resulting spacing**, so an override no longer silently
  changes apparent volume.

## Approach (sequencing & parallelism)

Sequenced by **file-disjointness** so streams can run mostly in parallel;
fix #4 depends on #1–#3 landing.

**Phase 1 — minimum coherent shippable slice. ✅ DONE (2026-06-12).** Shipped:
`scene.fill_level` waterline knob (Reset, default 0.75, reproduces today's look per
preset via `apply_fill_level`); volume-neutral splat radius
(`SPLAT_RADIUS_PER_SPACING = 0.7`, `radius = H · effective_density^(-1/3) · 0.7`);
auto one-ring `surface_dilation` below the reference density
(`effective_surface_dilation`); `filled_volume` / `liquid_fraction` in `stats_json`
+ a panel Scenario-summary readout; host unit tests (fill_level monotone in count,
count density-scaling at fixed waterline, density-invariant seeded fraction, auto-
dilation threshold). Real-GPU verification (`app/tools/vdd_sweep.mjs`, dam-break):
waterline 0.3→0.9 scales `liquid_cells` 33k→73k (≈2.2×, filled_volume 1.0→2.23);
density 8 vs 2 at fill 0.75 holds the visible body (screenshots) with `liquid_cells`
within ~15% early (1.15) drifting to ~1.3 as the dynamics settle — the visible water
is held by the splat scaling; the physics-cell ratio is the loose proxy.

**Follow-up (2026-06-12): `fill_level` redefined to a literal tank-fill percentage.**
The original per-preset waterline maps (calibrated so default 0.75 reproduced each
preset's historical geometry) were confusing. `scene.fill_level` is now a true "how
full is the tank" knob: stored 0–100 (%), default **20**, where the default scene
(Falling blob) is a full-footprint floor slab `(0,0,0)`–`(1, fill, 1)`, so 100% fills
the tank and 50% fills it halfway by height. Dam break = wall-slab height `fill * 0.98`;
double splash = suspended drops scaled by `fill`. `Registry::fill_level()` maps the
0–100 store to a `[0,1]` fraction. Real-GPU default-scene sweep
(`app/tools/fill_sweep.mjs`, near-seeded): fill 10/20/50/100% → `liquid_fraction`
0.097 / 0.191 / 0.442 / 0.909 (monotone, ~linear; 100% ≈ full tank). Geometry in
`preset_blocks` (the old `apply_fill_level` is gone).

**Anti-clump rest coupling — motion-neutral density. ✅ DONE (2026-06-12).** The
remaining "density changes the *motion/volume* of the water" divergence (distinct from
the seeded-body coverage handled by the splat scaling) was root-caused to the divergence
anti-clump source `min(stiff·(occ − rest)/rest, clamp)`: `occ` is the raw per-cell
particle count (scales with `particles.density`) but `rest` (`physics.rest_density`) was
frozen at 8, so `occ/rest` diverged badly across densities (puffy at 32, flat at 1). Fix:
the effective `rest` (divergence `spc[0]`) now tracks the scene's effective particles-per-
cell — `scene/mod.rs → effective_rest_density` (`manual > 0 ? manual : density`),
`gpu/fluid.rs → effective_rest_density` (build + Live). `physics.rest_density` is now an
optional manual override (`0` = Auto, new default). Host test
`auto_rest_density_tracks_particle_density`. Verification
(`app/tools/density_motion_sweep.mjs`, `particles.density ∈ {1,8,32}`, fixed falling-blob,
no rotation): `liquid_cells` held within ~12% (d1/d8/d32 vs d8 ratios at t1/4/8s all inside
0.93–1.12) vs ~38–44% baseline spread; settled screenshots show the same pool level at all
three densities. Rest coupling alone cleared the ~15% bar, so the secondary `flip_blend`
density trim was deliberately **not** added. See `decisions/simulation.md`.

**Phase 2 — remaining (optional polish, NOT tracked by this shipped plan).** The
pixel/thickness coverage metric + the `SPLAT_RADIUS_PER_SPACING` tuning loop (tighten
the ~12% residual `liquid_cells` invariance and the visual coverage tolerance), and the
SDF/level-set surface rewrite. The core decoupling shipped; this residual polish is
captured as future work in [`future-roadmap.md`](future-roadmap.md) (the
volume-neutral-density-residual and surface-rendering items). This plan is
`shipped + okay_to_delete: true`.

**Phase 1 streams (as shipped).** Three largely disjoint
streams:

- **Stream A (scene/settings):** waterline knob (#1) + panel
  reconciliation (#5) + the pure host unit tests (#4a-host).
  Files: `scene/mod.rs`, `settings/mod.rs`, `web/panels.js`.
- **Stream B (renderer):** spacing-tracking splat radius (#2) + expose
  `seeded_spacing`. Files: `gpu/mod.rs`, `gpu/fluid.rs` (seeded_spacing),
  `gpu/particles.rs`, `architecture/rendering.md`.
- **Stream C (classify):** auto-dilation (#3). Files: `gpu/fluid.rs` (cls
  population + helper), `settings/mod.rs` tooltip, `simulation.md`.
  *(B and C both touch `gpu/fluid.rs` — coordinate: B adds `seeded_spacing`
  near `generate_particles`, C edits the `cls` uniform near 264; non-
  overlapping regions, but serialize the two `fluid.rs` edits if done by
  separate agents.)*

Phase 1 also includes the **fast liquid-cell invariance integration leg**
(#4a-capture) since it is cheap once `set_setting`/`stats_json` are in
place.

**Phase 2 — visual calibration (heavier).** The capture coverage metric
(#4b) + the radius-coefficient tuning loop + the visual acceptance
screenshots. Split out because pixel-band/thickness-sum work and Chrome-on-
Windows runs are the heaviest part and gate on Phase 1 being correct.

## Exit gate

- `cargo build` for the wasm target succeeds.
- `cargo test --lib` green, including new tests:
  - `fill_level` raises/lowers `seeded_volume_fraction` /
    `resolved_particle_count` monotonically per preset;
  - count derivation is **density-independent** at fixed `fill_level`;
  - `effective_surface_dilation` host helper returns the expected
    on/off across the density threshold.
- **Density-invariance** (capture sweep, dilation on, fixed `fill_level`):
  `liquid_cells` varies ≤ `TOL_VOL` (start 0.15) across `density ∈
  {1,2,4,8}`.
- **Coverage invariance** (Phase 2): water-pixel count varies ≤
  `TOL_COVER` (start 0.20) across the same sweep.
- **Visual gate (the real acceptance signal):** `tools/capture.mjs`
  screenshots showing (i) the `fill_level` knob changing water depth, and
  (ii) low-vs-high density at the *same* `fill_level` showing ~the same
  body of water (blobbier at low). Attach to the ship commit.
- `architecture/simulation.md`, `architecture/rendering.md`,
  `decisions/scope.md` updated (see Migration notes).

## Discipline rules

- **Determinism preserved.** Do not touch the seeding RNG in
  `generate_particles` or the i32 fixed-point P2G scatter. The only
  physics-visible change is the auto one-ring dilation in classification
  (Live, already implemented, race-free single pass).
- **naga unused-binding rule:** no new shader bindings are added
  (classify dilation is already wired); if `thickness_sum` is added it
  must be fully consumed.
- **Settings completeness:** `scene.fill_level` needs label, tooltip,
  technical_tooltip, validation, default, and ApplyClass (Reset) — and
  must be added to any change-detection / category-ordering lists
  (settings/mod.rs ~2143, ~2473; panels ordering).
- **ApplyClass:** geometry/count/`fill_level` = **Reset**; splat radius is
  recomputed on the reset path (no new Live wiring); `render.particle_size`
  stays **Live**.

## Open decisions (flag for the user)

Tunable empirically (no call needed — calibrate via the sweep):

- **`k_radius` / `SPLAT_RADIUS_PER_SPACING` starting value** — start at the
  value that reproduces today at `density 8` (`≈0.35` ratio-to-spacing,
  i.e. `k_radius ≈ 0.175`); tune with the coverage sweep.
- **`TOL_VOL = 0.15`, `TOL_COVER = 0.20`** — starting tolerances; tighten
  as the coefficient settles.
- **Auto-dilation density threshold** (`< 8`) and whether it is one ring
  or proportional — start at one ring below reference density.
- **Min-density floor** — the existing 1024-particle floor in
  `resolved_particle_count` already prevents degenerate counts; confirm it
  is sufficient or pick an explicit `density` slider min (e.g. 0.5).

Needs a product call:

- **`fill_level` mapping for SUSPENDED presets (FallingBlob /
  DoubleSplash).** Phase 1 proposes *vertical-extent scaling about the
  block center* (reuses `seeded_volume_fraction`, no new geometry). The
  alternative — a **separate floor pool** whose depth = `fill_level`, with
  the blob always dropping into it — is more physically literal ("water
  level" = pool depth) but adds new block geometry and changes the look.
  **Which mapping do you want for suspended presets?** (Dam-break is
  unambiguous: taller floor slab.)
- **`fill_level` range/default** — confirm `[0,1]` with per-preset
  defaults that reproduce today's look, vs. a single shared default.

## Migration notes (filled in at ship time)

- `architecture/simulation.md` — `fill_level` waterline semantics per
  preset, its composition with `drop_height`, density-independent count
  derivation, and the auto `surface_dilation` policy.
- `architecture/rendering.md` — spacing-tracking splat radius formula,
  the `SPLAT_RADIUS_PER_SPACING`/`k_radius` coefficient and where it lives,
  and the calibration/coverage sweep.
- `decisions/scope.md` — the decision to fix low-density volume via
  splat-radius scaling + dilation (cheap) and **defer the SDF surface
  rewrite**; the density/count/waterline knob taxonomy.
- `_meta/ownership.json` — route `fill_level` / "water level" /
  "splat radius coverage" / "volume calibration" concepts to the docs
  above.

## See also

- `docs/plans/index.md` — where live plans land.
- `docs/plans/perf-particle-passes.md` — eb75ec0's density/count work this
  plan builds on.
- [`plan-lifecycle.md`](plan-lifecycle.md) — status metadata + ship-time
  migration.
- Owning docs in the frontmatter.
