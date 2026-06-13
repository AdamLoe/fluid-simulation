---
status:        active
owner:         unassigned
last_updated:  2026-06-13
okay_to_delete: false
long_lived:    false
owning_docs:
  - architecture/simulation.md
  - decisions/performance.md
---

# Perf: per-particle transfer passes (P2G scatter, G2P)

## Status (2026-06-12)

- **Phase 1 (fuse the 3 P2G scatter passes) — DONE.** `scatter.wgsl` is now a
  single fused `scatter_all` pass (params uniform + 7 storage buffers), reading
  each particle once and scattering all three MAC components; `fluid.rs` drops
  the `[_;3]` pipeline/bind-group arrays for one each; `FINE_SECTIONS` collapsed
  27→25 (one `scatter` section), `gpu/mod.rs` detailed-section indices renumbered
  (scatter=3, g2p=24), `dispatches_per_substep` 39→37. Runtime asserts
  `max_storage_buffers_per_stage >= 8`. Determinism preserved (integer i32
  atomicAdds at `FIXED_SCALE = 2^16`, bit-identical); real-GPU capture (dev
  adapter, `max_storage_buffers_per_shader_stage=16`) boots clean, smoke-test
  PASS, water renders, `gpuDeviceStatus:"ok"`. Docs updated:
  `architecture/simulation.md`, `architecture/profiler.md`. The perf measurement
  (scatter-section ms drop at the 128³ stress config) still needs a 3080 Ti
  capture — see the Exit gate.
- **Follow-up fix (2026-06-12):** the 27→25 renumber left a stale hardcoded
  index rollup in `gpu/timing.rs` (`sec[20..27]` on a `[f32; 25]`) that panicked
  inside the `map_async` detailed-readback callback, so `gpu` stats came back
  null and all timing/liquid_cells read 0 in detailed-profiling mode. Fixed:
  the coarse rollup now derives its boundaries by section name
  (`PREP_END`/`FINISH_START`) next to `FINE_SECTIONS` so a future renumber can't
  silently desync. Verified with a real-GPU detailed capture (non-null `gpu`,
  prep/pressure/finish all > 0, single `scatter` section, no panic).
- **Phase D (particle density) — DONE, shipped as a derived-density control
  rather than a silent default change.** Added `particles.density` (Reset-class
  f32, default 8/seeded-cell, range 1..32); `particles.count` became an advanced
  absolute override where `0` = Auto. The spawn count is derived in
  `scene/mod.rs::resolved_particle_count` as
  `round(density * seeded_volume_fraction * total_cells)`, so it tracks grid
  resolution and scenario fill instead of being a fixed absolute that silently
  drifts off-density when the grid changes. The web Scenario tab shows the
  resolved effective count. Docs updated: `architecture/settings.md`,
  `architecture/simulation.md`, `decisions/scope.md`. NOTE: this gives the user
  the *control* the plan asked them to decide on; the quality-vs-fidelity
  re-tuning of `classify.liquid_threshold` and the volume-correction trio at
  non-default densities is still untested and remains the user's call.
- **Phases 2, 3, and the open measurement question (#1) remain.** This plan
  stays `active`, not shipped.

## Mission

At a 128×64×128 grid with ~4.13M particles the sim is badly GPU-starved
(~12 fps, `real_time_factor` 0.285, substep cap of 2 hit on 95% of frames;
~3.8 substeps/frame are needed for real time). A real RTX 3080 Ti profiler
capture proves the cost is overwhelmingly **per-particle**: P2G scatter (~25
ms), G2P (~12 ms), and advect/recover (already folded into G2P) dominate the
~84 ms frame, while the entire grid/CG path is single-digit ms and is **not**
the target. This plan sequences particle-pass optimizations so the biggest
*safe* win ships first (Phase 1), with larger/riskier rewrites and a
quality-tradeoff decision (particle density) clearly separated as optional
follow-ons. Done = Phase 1 landed, host tests green, and a real-GPU capture
shows the P2G `scatter_*` section total dropping materially with unchanged
visual behavior.

## Ground-truth profiler data (the levers must tie to these)

Config: grid 128×64×128, ~4.13M particles, default settings, `pressure_iterations=30`,
2 substeps executed/frame. `liquid_cells` ≈ 373k (~36% of cells liquid).

Coarse section totals (frame, 2 substeps): prep=36.9 ms, finish=27.8 ms,
pressure=13.4 ms, render=5.9 ms (≈84 ms/frame).

Detailed per-pass (ms/frame), the decisive data:

- `g2p` = 11.87
- `scatter_w` = 8.66, `scatter_u` = 8.38, `scatter_v` = 7.88 (**P2G ≈ 24.9**)
- `mark` = 0.52, `clear` = 0.49; everything else (normalize, forces, savevel,
  boundary, gradient, divergence, classify, cg_init) < 0.13 each.
- CG internals: spmv=2.87, reduce=4.88, update=3.07 — pressure is 16% of the
  frame and is explicitly out of scope.

**Conclusion:** scatter + g2p (+ the advect/recover it contains) ≈ 65 ms of 84
ms is per-particle. Per-cell work is effectively free at this scale.

## Untimed-finish reconciliation (resolved — read before lever #2)

The brief hypothesized that `advect` + `recover` are ~16 ms of *untimed*
per-particle passes hiding inside the coarse `finish` total (27.8 ms vs the
detailed sections summing to ~12 ms). **This is not the case.** Confirmed by
reading the code:

- `record_finish` (`app/crates/fluid-lab/src/gpu/fluid.rs`) dispatches only
  `gradient×3` → `enforce×3` → `dispatch_g2p`. There is **no** separate
  `advect.wgsl` or `recover.wgsl`; there is no advect/recover pass recorder.
- `g2p.wgsl` (`app/crates/fluid-lab/src/gpu/shaders/g2p.wgsl`) already does
  G2P (3 wall-aware trilinear gathers) **plus** the PIC/FLIP blend, CFL clamp,
  wall friction, **RK1 advect**, and **escaped-particle recovery** in one
  `main`, reading and writing each particle exactly once. The doc line "G2P →
  advect → recover" is the logical content of that single pass, not three
  passes.

So the ~16 ms gap between coarse `finish` (27.8 ms) and the detailed sections
(~12 ms) is a **measurement-mode artifact, not a hidden pass.** Coarse mode
records `record_finish` as one `begin_compute_pass` span; detailed mode
(`gpu/mod.rs → record_substep_detailed`) wraps every section in its own
`begin_compute_pass` (each a pipeline/barrier boundary), which changes how GPU
work overlaps and bills, and the two captures were not the same run. The same
reasoning explains the ~10 ms prep gap (prep coarse=36.9 ms vs detailed prep
sections summing to ≈ scatter 24.9 + mark 0.52 + clear 0.49 + small ≈ 26 ms).

**Implication for the plan:** lever #2 ("fuse g2p/advect/recover so a particle
is read once") is **already done** — there is nothing to fuse there. The one
remaining real per-particle structural win is fusing the **three scatter
passes** (lever #1), which currently read each particle 3× and recompute
trilinear weights 3×. That is Phase 1. An **open question** (below) is whether
the coarse-vs-detailed gap is real avoidable overhead worth chasing; it needs a
controlled capture before any work is justified.

## Scope

**In scope:** the per-particle transfer kernels and their pass structure —
`scatter.wgsl` (P2G), its bind-group wiring in `fluid.rs`, optionally `mark`
fusion, and `g2p.wgsl` memory-access tuning. The particle-density *decision*
(perf math + risk) is in scope to surface; choosing it is the user's call.

**Out of scope (hard line):** the pressure solver and all CG kernels
(`cg_*.wgsl`, `pressure.rs`); divergence/gradient/forces/boundaries/normalize/
classify grid passes (all < 0.13 ms — touching them is wasted budget); the
renderer; the timestep/substep-cap policy; the fixed-point P2G representation
(must stay `i32` atomics at `FIXED_SCALE = 2^16`); the staggered-grid math; the
shared tiled particle-dispatch contract.

## Approach

Leverage-ranked, risk-tagged. Phase 1 is the single biggest **safe** win and is
independently shippable. Phases 2–3 are optional follow-ons; Phase D is a user
decision, not an implementer task.

---

### Phase 1 — Fuse the 3 per-axis P2G scatter passes into one (PRIMARY, SAFE)

**Idea.** Today `record_prep` dispatches `scatter_u`, `scatter_v`, `scatter_w`
as three separate particle-linear passes (`fluid.rs → dispatch_scatter(a)`,
pipelines `scatter_pl[0..3]` built from `scatter.wgsl` with an `AXIS` override).
Each pass re-reads `particles[p].pos`/`.vel`, recomputes the particle→grid
transform, and re-walks a 2×2×2 trilinear stencil. The three axes share the
*same* particle position; only the staggering offset, the face dims, and which
velocity component differ. Fuse into **one** `scatter_all` pass that reads each
particle once, computes the cell-centered position once, and scatters all three
components into their respective `num`/`den` buffers — each axis still using its
own correct half-cell offset and `+1` face dimension.

**Target pass(es) / expected ms saved.** `scatter_u`+`scatter_v`+`scatter_w` =
**24.9 ms/frame** today. Fusing removes 2 of 3 redundant particle reads and 2 of
3 transform setups, and lets the three stencil walks share index/weight scaffolding.
Realistic expectation: **~6–10 ms/frame saved** (roughly a 25–40% cut of the
scatter cluster). The atomic-add traffic itself is unchanged (same number of
`atomicAdd`s land in the same buffers), so the floor is set by atomic
contention, not arithmetic — do not promise the full 2/3.

**Files / shaders touched.**
- `app/crates/fluid-lab/src/gpu/shaders/scatter.wgsl` — add a fused entry (or a
  new `scatter_all.wgsl`) that drops the `AXIS` override and inlines all three
  axes. Keep the `AXIS`-parameterized version only if a fallback is wanted;
  otherwise replace.
- `app/crates/fluid-lab/src/gpu/fluid.rs` — replace `scatter_pl: [_;3]` /
  `scatter_bg: [_;3]` with one pipeline + one bind group; replace the
  `for a in 0..3 { dispatch_scatter }` loop in `record_prep` with one dispatch;
  update `dispatch_scatter` and the detailed-mode section wiring.
- `app/crates/fluid-lab/src/gpu/timing.rs` — `FINE_SECTIONS` currently lists
  `scatter_u/v/w` (3 sections); collapse to one `scatter` section (or keep three
  labels mapping to one pass). Adjust `N_FINE`, `record_substep_detailed`
  (`gpu/mod.rs`) section indices, and `dispatches_per_substep` (drops by 2).
  This is a `Params`/timing-shape change → update `architecture/simulation.md`
  and `architecture/profiler.md` per the change→doc table.

**Storage-buffer binding budget (the gating constraint).** Per-axis scatter
binds: `params`(0, uniform) + `particles`(1, read) + `num`(2) + `den`(3) → 3
storage. Fused needs: `params`(0, uniform) + `particles`(1, read) + `u_num`,
`u_den`, `v_num`, `v_den`, `w_num`, `w_den` (bindings 2–7, all
`read_write` `atomic<i32>`) = **7 storage buffers + 1 uniform**. That fits the
common `maxStorageBuffersPerShaderStage` floor of 8 with one slot to spare.
**Implementer must assert `caps.max_storage_buffers_per_stage >= 8`** (already
probed at `gpu/mod.rs → GpuCaps`) — if a target adapter reports < 8, keep the
3-pass path. Naga note: the fused shader references `params` (binding 0)
directly, so no explicit BGL gymnastics are needed.

**Risk.** *Low.* Determinism is preserved — still fixed-point `i32`
`atomicAdd`s into the same six buffers in the same quantities; integer add stays
associative/commutative, so results are bit-identical. No visual change
intended. The only correctness trap is the MAC staggering: each axis must keep
its own `off` (0 on its axis, −0.5 on the others) and its own face `dim`
(`nx+1`/`ny+1`/`nz+1` on its axis). Get the per-axis `base`/`t`/index right or
velocities land on the wrong faces.

**Verification.**
- `cargo test --lib` (host reference: sim math, determinism, P2G) stays green —
  the fused scatter must produce identical grid velocities; if any determinism/
  P2G test exists it should pass byte-for-byte. If no host test covers GPU
  scatter directly, the determinism guarantee rests on the integer-add argument
  plus the capture below.
- Real-GPU capture via `tools/capture.mjs` at the repro config
  (`?set=particles.count:4130000` + `grid.res_x/y/z` 128/64/128, detailed GPU
  profiling on): the `scatter` section total drops from ~24.9 ms; `prep` and
  total frame time drop; `real_time_factor` rises; `gpuDeviceStatus:"ok"`. Diff
  the screenshot against a pre-change capture at the same seed/frame to confirm
  no visual regression (the determinism check that can't be faked).

---

### Phase 2 — Optionally fold `mark` into the fused scatter (SMALL, MEDIUM RISK)

**Idea.** `mark.wgsl` is a fourth per-particle scatter (`atomicAdd(occ[c], 1)`
into the occupancy buffer) costing 0.52 ms. Since the fused scatter already
reads each particle's position and computes its containing cell, it could also
do the occupancy `atomicAdd` in the same pass, removing a whole particle-linear
dispatch.

**Expected ms saved.** ~0.5 ms/frame (small). Real value is removing one
particle-buffer sweep, not the arithmetic.

**Files.** `scatter.wgsl` (fused) gains the `occupancy` binding; `mark.wgsl` and
its pipeline/bind-group/dispatch removed from `record_prep`. **Binding budget:**
adds `occupancy`(binding 8, `read_write atomic<u32>`) → **8 storage buffers**.
This is exactly at the common limit; on an 8-buffer adapter there is zero
headroom and this fusion may not build. **Gate it on
`max_storage_buffers_per_stage >= 9`**, else keep `mark` separate.

**Risk.** *Medium* — purely the binding-budget ceiling (8 vs 9). No determinism
or visual risk (integer `atomicAdd`, same counts). Sequencing: do Phase 2 only
after Phase 1 lands, as a small bolt-on; if it pushes past the binding limit,
drop it without affecting Phase 1.

**Verification.** Same as Phase 1; additionally confirm `classify` still sees
the same occupancy counts (liquid-cell count `gpu.liquid_cells` unchanged in the
throttled readout).

---

### Phase 3 — Atomic-contention / memory-access tuning in scatter & g2p (RISKIER, MEASURE FIRST)

**Idea.** After fusion, the scatter floor is **atomic contention** (4M particles
× 8 stencil corners × 2 `atomicAdd`s, at ~11 particles/cell → heavy
same-address contention). Candidate mitigations, each needs its own capture
before committing:

- **Workgroup-local pre-accumulation.** Particles in a 64-lane workgroup that hit
  the same face could combine contributions in workgroup-shared memory before one
  `atomicAdd` to global — but particle order is not spatially sorted, so hit rate
  is unknown without a capture. Risky and possibly net-negative.
- **Particle spatial reordering (Z-order/cell-bucketed).** Sorting particles by
  cell would make scatter atomics and g2p gathers cache/contention-friendly, but
  adds a sort pass and a particle-buffer permutation each substep — a large piece
  of work with its own cost. Out of scope for this plan beyond naming it; would
  need its own plan and a measured cost case.
- **g2p memory access.** `g2p.wgsl` does 3 separate trilinear gathers, each
  reading a `_vel` and a `_saved` buffer with per-corner `*_touches_static_solid`
  branch divergence. There may be a modest win from hoisting the shared
  `base`/`t` per axis, but the three axes have different staggering so sharing is
  limited. Low expected payoff; measure before touching the wall-aware sampling
  (it encodes a free-slip invariant — see `simulation.md` "G2P samples skip
  static wall-zeroed face stencils").

**Expected ms saved.** Unknown; **do not promise a number** without a capture.
This phase is explicitly "measure, then maybe."

**Risk.** *High* for the sort path (new architecture, new buffers, perf claim
needs evidence per `decisions/performance.md`). *Medium* for g2p tweaks (the
wall-aware sampling invariant is load-bearing for visual quality). Gate all of
Phase 3 behind a fresh capture that isolates contention as the remaining cost.

**Verification.** `cargo test --lib` green; capture must show a real `scatter`/
`g2p` reduction *and* an unchanged screenshot diff. Any sort path is a separate
plan.

---

### Phase D — Particle density (USER DECISION, not an implementer task)

**This is a quality tradeoff. Do not silently pick it.**

**The math.** 4.13M particles / 373k liquid cells ≈ **11.1 particles/cell**. The
standard FLIP/PIC target is ~8/cell. Right-sizing to 8/cell means ~2.98M
particles — a **~28% cut**. Because *every* dominant cost (scatter, g2p,
advect/recover, mark) is strictly linear in particle count, a 28% particle cut
is ~**18 ms off the ~65 ms particle budget** — the single largest *raw* lever
available, larger than Phase 1. Cutting toward a leaner ~6/cell (~2.24M, −46%)
would roughly halve particle cost.

**Why it's a decision, not a default.** Particle count is the seeded
mass/distribution (`particles.count`, default `254_144`, Reset-class;
`settings/mod.rs`). The 4.13M figure is a stress config, not the shipped
default. Fewer particles/cell means: thinner splash sheets, more visible
graininess at the free surface, and a higher chance of empty liquid cells
(velocity holes) — a **visual-quality regression** that only the user can weigh
against the frame-rate win. It also interacts with `classify.liquid_threshold`
and the volume-correction trio (`rest_density`/`volume_stiffness`/`drift_clamp`),
which were tuned around the current density.

**What to put in front of the user.** "At the 4.13M stress config you are at ~11
particles/cell vs the conventional ~8. Dropping to 8/cell (~2.98M) is ~18 ms/
frame — bigger than the scatter fusion — but thins the splash and risks surface
graininess. Pick a target particles/cell (e.g. 8 or 6), or keep 11 for fidelity
and rely on the kernel fusions." If chosen, it's a one-line settings/preset
change plus re-tuning the classify/volume knobs and a fresh capture — trivial to
*apply*, consequential to *decide*.

## Exit gate

Phase 1 (the shippable unit):

- `wsl.exe -d Ubuntu-24.04 -- bash -lc 'cd /home/adamg/fluid-simulation/app && cargo test --lib'`
  is green (sim math / determinism / settings).
- `wsl.exe -d Ubuntu-24.04 -- bash -lc 'cd /home/adamg/fluid-simulation/app && cargo build --target wasm32-unknown-unknown'`
  compiles (the fused-binding pipeline actually builds on the dev adapter).
- A real-GPU `tools/capture.mjs` run at the documented repro config reports
  `gpuDeviceStatus:"ok"`, a `scatter` section total materially below 24.9 ms, a
  lower frame total / higher `real_time_factor`, and a screenshot that matches a
  pre-change capture at the same seed/frame (no visual regression). No
  performance claim ships without this capture (`decisions/performance.md`).

## Discipline rules

- **Determinism is non-negotiable.** Any scatter change stays integer/fixed-point
  `i32` atomics at `FIXED_SCALE = 2^16`. No float accumulation "for convenience."
- **Respect the binding budget per fused pass** and assert the probed
  `max_storage_buffers_per_stage` at runtime; keep the split path as the fallback
  when a target adapter is under the needed limit (Phase 1 needs ≥ 8, Phase 2
  needs ≥ 9).
- **No work on the pressure solver or grid passes** — they are single-digit (or
  sub-0.13) ms and explicitly out of scope.
- **Measure before Phase 3.** No contention/sort work without a capture proving
  contention is the remaining cost; no perf claim without profiler output.

## Open questions (need a capture/decision before the relevant phase)

1. **Is the coarse-vs-detailed gap real overhead?** Coarse `prep`/`finish`
   (36.9/27.8 ms) exceed the summed detailed sections (~26/~12 ms) by ~10/~16 ms.
   This plan attributes it to per-pass measurement-mode differences, not hidden
   passes (confirmed: no advect/recover pass exists). Before chasing it, take one
   *controlled* capture — same seed, same frame, coarse vs detailed back-to-back —
   to decide whether the gap is avoidable pass/barrier overhead or just a
   billing artifact. This gates any "reduce dispatch/barrier count" follow-on.
2. **Particle density target (Phase D)** — user must choose particles/cell (keep
   ~11, or 8, or 6) before that lever is applied.
3. **Phase 3 contention** — needs a post-Phase-1 capture isolating atomic
   contention before any workgroup-local-accumulation or particle-sort work is
   justified.

## Migration notes (filled in at ship time)

On ship, route facts/decisions into:

- `architecture/simulation.md` — the fused scatter pass replacing the 3-axis
  `scatter_u/v/w` in the step sequence; the corrected note that g2p already
  contains advect+recover (one particle read/write); the `Params`/dispatch-count
  change (`dispatches_per_substep` drops by 2, or 3 with Phase 2).
- `architecture/profiler.md` — `FINE_SECTIONS` collapse (`scatter_u/v/w` → one
  `scatter`), `N_FINE`, and detailed-mode section indices.
- `decisions/performance.md` — the scatter-fusion decision and its binding-budget
  rationale (extends "Respect the per-stage storage-buffer limit — split passes":
  fuse *only* when the budget and a capture both allow it); if Phase D is taken,
  a particles/cell density decision with the perf-vs-fidelity tradeoff.
- `architecture/gpu-resources.md` — if buffer bindings/ownership shift.

List exactly what landed where so a reviewer can confirm `okay_to_delete: true`.

## See also

- `../architecture/simulation.md` — owns the step sequence, the per-particle
  passes, P2G determinism, and the g2p (G2P+advect+recover) pass.
- `../architecture/gpu-resources.md` — buffer layout and the storage-buffer
  budget.
- `../architecture/profiler.md` — `FINE_SECTIONS`, coarse vs detailed timing
  modes.
- `../decisions/performance.md` — pass-split rationale, "profile before
  optimize," and the tiled particle-dispatch contract.
- `index.md` — where live plans land.
