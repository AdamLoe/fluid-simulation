---
status:        active
owner:         unassigned
last_updated:  2026-06-13
okay_to_delete: false
phase_3_designed: true
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

## Status update (2026-06-13) — high-count refocus, Phase 3 designed

The user wants to push perf at **HIGH particle counts** (well past the original
4.13M repro). Fresh measured baseline (RTX 3080 Ti, grid 128×64×128, default
settings, throttled detailed profiling):

| density | particles | fps |
|--------:|----------:|----:|
| 8       | ~1.5M     | 40  |
| 32      | ~6M       | 26  |
| 32      | ~22M      | 6   |

The sim is **per-PARTICLE bound** at these counts: P2G `scatter` + `g2p`
dominate; per-cell passes and the CG solve are cheap because only ~36% of cells
are liquid (~373k of ~1.05M cells at 128×64×128). Phase 1 (scatter fusion)
already shipped — scatter is **one** fused pass reading each particle once
(`scatter.wgsl`, 7 storage + 1 uniform). The prior benchmark concluded fusion
did **not** move the atomic-traffic floor, so the remaining scatter cost is
**atomic contention + incoherent memory access**, not redundant reads. That is
exactly what a **particle spatial sort** attacks, and it is the designed Phase 3
below (replacing the old "measure, then maybe" placeholder). The mark-fold and
workgroup-local-accumulation levers are re-sequenced as smaller follow-ons.

**Ranked levers (gain / effort / risk), high-count regime:**

| # | Lever | Mechanism | Expected gain | Effort | Risk | Determinism |
|---|-------|-----------|---------------|--------|------|-------------|
| 1 | **Particle spatial sort (cell-bucketed counting sort)** | reorder particles by linear cell index every N steps so scatter atomics and g2p gathers hit coherent grid memory → less same-address atomic contention + far better cache/bandwidth | **largest** — targets the actual scatter/g2p floor; literature & memory-bound reasoning suggest 1.3–2× on the ~per-particle budget at 6–22M, net of the sort's own cost | High | Med | preserved (see argument) |
| 2 | Workgroup-local scatter pre-accumulation | combine same-face contributions in `var<workgroup>` before one global atomic | small/uncertain **unless** particles are already sorted; near-useless on unsorted input (low intra-WG cell coherence). Best framed as a *rider on lever 1* | Med | Med | preserved if integer accum |
| 3 | Mark-fold into scatter | drop the 4th particle sweep (`mark`) by doing the occupancy `atomicAdd` inside fused scatter | ~one particle-sweep (~2% at high count); gated on binding budget ≥ 9 | Low | Low | preserved |
| 4 | Particle SoA split (pos / vel buffers) | scatter & mark read only `pos`; g2p reads+writes both. Splitting AoS{pos,vel} into two buffers halves bytes touched by the read-only passes | modest bandwidth win at 22M (particle buffer is 32B×22M ≈ 700MB); compounds with lever 1 | Med | Low | preserved |
| 5 | g2p gather micro-tuning | hoist shared base/t; the 3 axes stagger differently so sharing is limited | low | Low | Med (wall-aware invariant) |
| 6 | Workgroup-size / occupancy sweep | try WG 128/256 for scatter & g2p | low, free to try | Low | Low | preserved |

**Recommended top lever: the particle spatial sort (lever 1), designed as
Phase 3 below.** A periodic counting sort by linear cell index makes both the
scatter atomics and the g2p gathers spatially coherent, which is the only thing
that moves the contention/bandwidth floor those two passes now sit on. At 6–22M
particles the per-particle budget is essentially all of the frame, so even a
1.3–1.5× cut there is the single biggest available win. The sort is a GPU
counting sort (clear-histogram → count → prefix-sum → scatter-into-sorted-order)
that runs at most once per step (optionally every N steps), and its cost is
small relative to the scatter+g2p it accelerates.

**Determinism:** preserved and argued in Phase 3. The sort key is the integer
linear cell index `i + nx*(j + ny*k)` computed from `floor((pos-origin)/h)`
(identical to `mark.wgsl`), so it is a pure deterministic function of state. P2G
stays i32 fixed-point atomics, which are order-independent, so any permutation
of particles yields **bit-identical** grid `num`/`den`. g2p reads/writes each
particle independently, so reordered storage gives identical per-particle
results. The only requirement is a **stable, deterministic** sort (ties broken
by original index) so a given input always produces one permutation — satisfied
by a counting sort with a deterministic intra-bucket order.

**User decision needed before implementation:** re-sort cadence — **every step**
(max coherence, ~full sort cost each frame) vs **every N steps** (amortized sort
cost; coherence degrades between sorts as particles advect across cells, ~≤1
cell/step under the CFL clamp so N=4–8 is cheap-and-safe). This is a perf/perf
tradeoff (no quality change). Recommended default: re-sort every step at high
counts and measure N=2/4/8 in the same capture sweep. The implementer should
also decide whether the sort is gated on a particle-count threshold (skip it at
low counts where it is net-negative).

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

### Phase 3 — Particle spatial sort (PRIMARY high-count lever) — IMPLEMENTER-READY

**Status:** designed, not started. This is the top remaining lever at 6–22M
particles. It is the concrete replacement for the old "measure, then maybe"
Phase 3 placeholder.

#### Why this is the lever

After Phase 1, the scatter floor is set by **same-address atomic contention**
(N particles × 8 stencil corners × 2 `atomicAdd`s into 6 face buffers) and by
**incoherent memory** — particles in storage order are spread across the grid,
so consecutive lanes in a 64-wide workgroup touch unrelated cache lines (both on
the scatter `atomicAdd` targets and on the g2p `u/v/w_vel`/`_saved` gathers).
The particle layout is seeded as a lattice (`generate_particles` in `fluid.rs`)
and then advects freely; after a few seconds it is effectively random w.r.t.
cell index. Reordering particles so that particles in the same cell (and nearby
cells) are **contiguous in the buffer** makes:

- scatter `atomicAdd`s from a workgroup land on a small, shared set of face
  addresses → fewer cross-warp atomic collisions and better L2 residency;
- g2p gathers read a small working set of grid faces per workgroup → cache hits
  instead of misses.

Both dominant passes are per-particle and memory-bound at high count, so this is
the only structural lever that moves their floor.

#### Algorithm: GPU counting sort by linear cell index

Sort key per particle: `key = i + nx*(j + ny*k)` where
`(i,j,k) = clamp(floor((pos - origin)/h), 0, n-1)` — **identical** to the cell
index `mark.wgsl` already computes. Range is `[0, cell_count)` (≤ ~1.05M at
128×64×128), small enough for a single-pass counting sort (no multi-digit radix
needed). Five GPU passes, all already-supported dispatch shapes:

1. **clear_hist** (cell-linear): zero `cell_hist: array<atomic<u32>>` of length
   `cell_count`. (Reuse the existing `clear.wgsl` + a clear bind group, or fold
   into the existing clear list.)
2. **count** (particle-linear): each particle computes `key`,
   `atomicAdd(&cell_hist[key], 1u)`. This is *exactly* the work `mark.wgsl`
   already does — see "mark merge" below; the histogram **is** the occupancy
   buffer.
3. **prefix-sum** (exclusive scan of `cell_hist` → `cell_offset`): produces the
   start offset of each cell's bucket in the sorted array. `cell_count` ≤ ~1M, so
   a standard work-efficient block scan + block-offset fixup (two cell-linear
   dispatches + a small spine scan) suffices; the existing `cg_reduce`/
   `cg_reduce_final` two-level reduction pattern is the template to copy. The
   scan is on **cells**, not particles, so it is cheap (per-cell work is already
   "free" at this scale).
4. **scatter_into_order** (particle-linear): each particle recomputes `key`, does
   `dst = atomicAdd(&cell_offset[key], 1u)` (a running cursor), and writes its
   record to `particles_sorted[dst]`. To keep the sort **deterministic** (stable)
   the running-cursor order is *not* guaranteed stable by itself (atomic return
   order is nondeterministic), so either (a) accept "deterministic grid, possibly
   non-stable particle permutation" — which is still bit-identical for the sim
   because P2G is order-independent and g2p is per-particle (see determinism
   argument) — or (b) make it strictly stable with a per-particle rank. **Take
   option (a):** it is correct and bit-identical at the simulation level; do NOT
   pay for strict stability. (Documented and argued below.)
5. **(no separate reindex pass)** — passes 4 writes the fully reordered particle
   records directly; no permutation indirection is needed downstream because every
   particle-linear pass already addresses `particles[p]` by dense index.

Then **ping-pong**: subsequent passes (mark/scatter/.../g2p) read from
`particles_sorted`; next step's sort writes back into `particles`. Maintain two
particle buffers (`particles_a`, `particles_b`) and swap which is "current" each
sorted step, exactly like the pressure ping-pong already in this file.

#### Buffer changes

- **Second particle buffer** `particles_b` (same size as `particles`,
  `particle_count * 32` B). At 22M this is ~700MB extra — assess against device
  budget; the plan must surface this VRAM cost as a gate (lever 4, SoA split,
  reduces it). The existing `particles` becomes `particles_a`.
- **`cell_offset: array<atomic<u32>>`** length `cell_count` (the scan output +
  running cursor). The histogram input is the **existing `occupancy` buffer**
  (see mark merge) so no new histogram buffer is needed; if mark stays separate,
  add `cell_hist` of length `cell_count`.
- All bind groups that bind `particles` (mark, scatter, g2p, impulse, the
  renderer's particle vertex buffer, `reset`) must be built **for both**
  `particles_a` and `particles_b` and selected by the current-buffer flag, OR
  rebuilt on swap. Building both up front (as the code already does for CG
  ping-pong reduce groups) is cleaner and avoids per-frame bind-group creation.
- `reset()` rewrites `initial` into the current buffer and resets the
  current-buffer flag to a known side so the first step is deterministic.

#### Storage-binding budget (per new pass)

- **clear_hist**: 1 storage (reuse clear). OK.
- **count**: `params`(uniform) + `particles`(read) + `cell_hist`(rw atomic) = 2
  storage. OK. (Identical to `mark` → merge.)
- **prefix-sum** block scan: `params`(uniform) + `cell_hist`(read) +
  `cell_offset`(write) + `spine`(rw) = 3 storage. OK.
- **scatter_into_order**: `params`(uniform) + `particles_src`(read) +
  `particles_dst`(write) + `cell_offset`(rw atomic cursor) = 3 storage. **Well
  under the 8 floor.** No budget pressure anywhere in the sort — the tight
  pass remains the existing fused `scatter` (7) and `g2p` (8), which the sort
  does not touch. Keep the runtime `assert max_storage_buffers_per_stage >= 8`.

#### Mark merge (free win, fold lever 3 in here)

`mark.wgsl` already does pass 2's `atomicAdd(&occ[key], 1u)` with the **same key
math**. So the counting-sort histogram **is** the occupancy buffer: run `mark`
(renamed conceptually "count") to fill `occupancy`, run the prefix-sum to derive
`cell_offset` from `occupancy`, then `scatter_into_order`. This removes the
separate `cell_hist` buffer and means the sort adds only **prefix-sum +
scatter_into_order** as genuinely new particle/cell work on top of the existing
mark. (classify still reads `occupancy` unchanged — read it *before* any
in-place scan mutates it, so the scan must write `cell_offset` as a separate
buffer, not overwrite `occupancy`.)

#### Dispatch wiring

New helpers in `fluid.rs` mirroring the existing ones:
`dispatch_clear_hist`, `dispatch_prefix_sum`, `dispatch_sort_scatter`, plus a
`current particle buffer` selector used by `dispatch_mark/scatter/g2p/impulse`
and `g2p`'s in-place write target. Sequencing inside the step (cadence-gated):

```
record_prep:
  clear (incl. occupancy)         // existing
  mark/count -> occupancy         // existing
  [if sort_this_step]:
     prefix_sum(occupancy -> cell_offset)
     sort_scatter(particles_src -> particles_dst); swap current buffer
  classify                        // reads occupancy (unchanged)
  scatter (fused P2G)             // reads current particle buffer
  ... normalize/savevel/forces/enforce
```

Note the sort must happen **after** mark/count (needs the histogram) and
**before** scatter/g2p (so they read sorted order), and classify must read
`occupancy` regardless. The detailed profiler (`timing.rs FINE_SECTIONS`,
`gpu/mod.rs record_substep_detailed`) gains up to **2 new sections**
(`prefix_sum`, `sort_scatter`); update `N_FINE`, the `sec!` indices, the
coarse-rollup is unaffected (both land in `prep`), and `dispatches_per_substep`
grows by the sort's dispatch count when sorting. Follow the
`PREP_END`/`FINISH_START` by-name boundary convention already in `timing.rs`.

#### Cadence (the user decision)

Re-sort **every step** (max coherence) vs **every N steps** (amortize sort cost;
coherence decays as particles advect, but the CFL clamp limits motion to ~≤1
cell/step so N=4–8 keeps buckets nearly-coherent). Implement a
`sort_period` (Reset-class, or a dev knob) and a particle-count threshold below
which sorting is skipped (it is net-negative at ~1.5M). **Default for the first
capture: sort every step; sweep N∈{1,2,4,8} at 6M and 22M to find the knee.**

#### Determinism argument (load-bearing — must hold bit-identically)

The sim must stay deterministic (host tests + capture screenshot diff). The sort
preserves this:

1. **Key is a pure function of state.** `key(pos)` uses the same integer cell
   math as `mark.wgsl`; no float reduction, no time/order dependence.
2. **P2G is order-independent.** Scatter accumulates i32 fixed-point `atomicAdd`s
   into `num`/`den`. Integer addition is associative and commutative, so **any**
   permutation of the particle array produces **bit-identical** `num`/`den`, hence
   bit-identical normalized grid velocity. The sort only permutes which lane
   processes which particle — the multiset of `atomicAdd`s is unchanged.
3. **g2p is per-particle independent.** Each invocation reads grid velocity
   (identical, from 2) and reads+writes **only its own** `particles[p]`. The
   result for a given particle depends only on its own pos/vel and the (identical)
   grid — not on neighbors or buffer position. So reordered storage gives every
   particle the identical updated pos/vel; the *set* of particles is identical.
4. **No pass depends on absolute buffer index meaning.** Particles are an
   unordered set; nothing keys off `p` as an identity (the renderer draws all
   instances; impulse hits all uniformly). So a non-stable permutation in pass 4
   (option a) is fine — the simulation state (grid + particle multiset) is
   bit-identical regardless of which slot a particle lands in.

Therefore the sort changes performance only, not results. **The one thing that
would break determinism** is introducing any float accumulation into the key or
the histogram, or letting the scan read a partially-cleared histogram — both
avoided by the integer key and the clear→count→scan→scatter ordering.

Host tests: the existing host reference (`cargo test --lib`) does not run the GPU
sort, so it stays green by construction. Add a **host unit test for the
prefix-sum / counting-sort logic** (a CPU mirror of the scan + cursor scatter)
asserting: (a) every particle lands in exactly one slot (permutation is a
bijection), (b) all particles in a bucket share the same cell key, (c) bucket
order matches the exclusive prefix sum. Put it next to the existing
`particle_dispatch_shape` tests in `fluid.rs` (pure functions, no GPU).

#### Expected gain (do NOT claim without a capture)

Memory-bound reasoning: at 6–22M the per-particle budget is ~all of the frame;
sorted access typically buys 1.3–2× on memory-bound scatter/gather kernels. Net
of the sort's own cost (one counting sort ≈ a few particle-sweeps + a cell scan,
i.e. small vs the scatter+g2p it accelerates), expect a **material** scatter+g2p
drop at high count. **No number ships without the capture below.**

#### Capture to prove it (the acceptance signal)

Real-GPU `app/tools/capture.mjs` (it drives `window.__fluid.set_setting`, and
honors `PARTICLES=` + `DETAILED=1` env). Run a **before/after sweep at the high
counts**, detailed profiling on, same seed/scene/grid (128×64×128):

- Set `grid.res_x/y/z` = 128/64/128 and `particles.density` = 32 (the 6M and 22M
  configs) via `EVAL`/`set_setting`; capture detailed timing.
- Record `scatter` + `g2p` section ms/frame **before** (current `main`) and
  **after** (sort landed), at 6M and 22M, plus fps / `real_time_factor`.
- Acceptance: `scatter`+`g2p` total drops materially at 6M **and** 22M, frame
  total drops / fps rises, `gpuDeviceStatus:"ok"`, and a screenshot at a fixed
  seed/frame **matches** the pre-change capture (the determinism check that can't
  be faked). If 22M can't allocate the second particle buffer, that gates the
  VRAM cost (do lever 4 SoA split first, or sort in place — but in-place sort is
  not a counting sort; flag as a separate design).

#### Risk

*Medium.* New buffers + ping-pong + 2 new passes, but every piece reuses an
existing pattern (atomic histogram = mark; scan = cg_reduce two-level; ping-pong
= pressure buffers; particle dispatch = the tiled contract). The determinism
argument is airtight given integer P2G. The real risks are (1) the +700MB second
particle buffer at 22M (gate on device budget), and (2) getting the prefix-sum
exclusive-scan boundary fixup right — covered by the host unit test.

---

### Phase 4 — Mark-fold + workgroup-local scatter accumulation (SMALL, ride on Phase 3)

These two were the old Phase 2/3 placeholders; at high count they are smaller
than the sort and **partly subsumed by it**.

**Mark-fold** is now folded into the Phase 3 sort design (the histogram *is*
occupancy; mark = the counting-sort count pass), so there is no separate
mark-fold task once Phase 3 lands. If Phase 3 is deferred, mark-fold remains a
standalone ~one-sweep win, gated on `max_storage_buffers_per_stage >= 9` (it
would add `occupancy` as binding 8 to the already-7-binding fused scatter).

**Workgroup-local scatter pre-accumulation** becomes worthwhile **only after the
sort**: with particles sorted by cell, a 64-lane workgroup mostly touches a small
set of faces, so accumulating contributions in `var<workgroup>` shared memory and
emitting one global `atomicAdd` per (face,workgroup) cuts global-atomic traffic
sharply. On *unsorted* input the intra-workgroup cell coherence is low and this
is near-useless or net-negative — which is why it is sequenced after Phase 3, not
before. Keep accumulation **integer** (i32 fixed-point) so determinism holds.
Measure as a rider on the Phase 3 capture.

**Risk.** Low/Medium. Pure perf riders; do them only with a capture showing they
add on top of the sort.

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

Phase 3 (the particle spatial sort, shippable unit):

- `cargo test --lib` green, including the **new host counting-sort/prefix-sum
  unit test** (bijection, per-bucket key equality, exclusive-scan bucket order).
- `cargo build --target wasm32-unknown-unknown` compiles (new pipelines build on
  the dev adapter; second particle buffer allocates).
- Real-GPU before/after detailed sweep at **6M and 22M** (grid 128×64×128,
  `particles.density`=32): `scatter`+`g2p` section total drops materially at both
  counts, frame total down / fps up, `gpuDeviceStatus:"ok"`, and a fixed
  seed/frame screenshot **matches** the pre-sort capture (bit-identical sim, per
  the determinism argument). If the 22M second particle buffer can't allocate,
  that result gates the VRAM cost (lever 4 SoA split first). No perf claim ships
  without this capture.

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
3. **Phase 3 sort cadence (USER DECISION)** — re-sort every step (max coherence)
   vs every N steps (amortized; pick N at the captured knee, likely 4–8 under the
   CFL clamp). Perf/perf tradeoff, no quality change. Plus: gate the sort on a
   particle-count threshold so it is skipped at low counts where it is
   net-negative.
4. **Phase 3 VRAM (gate, not a free decision)** — the counting sort needs a
   second particle buffer (~700MB at 22M). If a target device can't allocate it,
   either do lever 4 (SoA pos/vel split) first to shrink it, or treat in-place
   reordering as a separate design. Confirm against the device budget in the
   capture.

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
  a particles/cell density decision with the perf-vs-fidelity tradeoff. **Phase 3:**
  the particle-spatial-sort decision — a periodic GPU counting sort by linear
  cell index, the determinism argument (integer-P2G order-independence makes any
  permutation bit-identical), the sort-cadence tradeoff, and the second-particle-
  buffer VRAM cost as a high-count gate.
- `architecture/gpu-resources.md` — if buffer bindings/ownership shift. **Phase 3:**
  the second particle buffer (ping-pong), `cell_offset` (scan output), the
  occupancy-as-histogram reuse, and the per-pass binding budgets for clear_hist /
  prefix_sum / sort_scatter (all ≤ 3 storage, well under the floor).
- `architecture/simulation.md` (Phase 3) — the sort passes inserted between
  mark/count and the fused scatter in the step sequence; the current-particle-
  buffer ping-pong; the determinism note that the sort is perf-only.
- `architecture/profiler.md` (Phase 3) — the new `prefix_sum` / `sort_scatter`
  `FINE_SECTIONS`, the bumped `N_FINE`, and the `dispatches_per_substep` growth
  when sorting.

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
