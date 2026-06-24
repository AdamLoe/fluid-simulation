---
status:        active
owner:         adamg
last_updated:  2026-06-24
---

# Decisions — Performance

## Dense wall fill is removed until profiler data justifies a new implementation

**Decision** — Dense wall fill no longer ships as a runtime feature. The cheaper
render-side wall-contact normal/depth snap is also removed; the active product path is
screen-space depth smoothing plus ordinary tank-boundary simulation contact.

**Why** — Wall fill owns a large supersampled occupancy atlas and per-frame splat +
injection work. Without capture/profiler evidence that it improves the startup view more
than it costs, it should not be in the normal frame path.

**Tradeoffs** — Reintroducing dense wall fill requires a fresh measured cost case,
new settings, new resource ownership, and captures. There is no occupancy buffer,
wall-fill injection pass, `wallfill_mask` target, or render-side wall-contact snap in
the current runtime.

**Code anchors** — `crates/fluid-lab/src/settings/mod.rs → Registry`;
`crates/fluid-lab/src/gpu/mod.rs → GpuContext::render`.

**Revisit when** — repeated real-GPU captures show wall fill materially improves default
quality and profiler samples show acceptable median `gpu.render_ms`.

**Applies to** — `architecture/rendering.md`, `architecture/gpu-resources.md`,
`architecture/settings.md`.

## Resolution targets: 32³ baseline, 80×40×80 default, 128³ aspirational

**Decision** — A stable/interactable 32³ is the required floor; the current default is
the rectangular 80×40×80 tank with density-derived particles; 128³ is aspirational and
must not drive the architecture. A smaller stable preset with measured justification is
an acceptable ship.

**Why** — 128³ is 2M+ cells before particles and solver workspace. Starting there
builds an impressive target instead of a working app. Correct 32³ beats broken or
unmeasured larger scales; the default 80×40×80 rectangular tank is the current product
baseline.

**Tradeoffs** — The default rectangular tank is larger than 32³ while still below the
128³ aspirational cube. Its default falling-blob scene derives roughly 512k requested
particles from density 10 over about 20% seeded volume, so performance claims need the
active particle count and profiler output rather than a cube-resolution shorthand.

**Applies to** — `architecture/simulation.md`, `architecture/gpu-resources.md`.

## Quality presets are startup/runtime configuration, not performance claims

**Decision** — The web shell exposes Performance, Balanced, Quality, and Ultra as
runtime quality presets over existing registry settings, and may auto-select a
non-Ultra preset before the first frame with a conservative device/viewport heuristic.

**Why** — Demo startup needs a bounded weak-laptop default and manual capture needs a
fast way to raise fidelity. Presets are useful as configuration batches, but they are
not benchmarks and must not claim 30 FPS/TPS without profiler or capture evidence.

**Tradeoffs** — Startup auto-selection is deliberately heuristic: saved localStorage
config skips it, URL/imported settings override it through normal registry replay, and
Ultra remains manual until profiler/capture evidence justifies using it automatically.
Preset buttons persist only when the user explicitly chooses them.

**Code anchors** — `web/panels.js → QUALITY_PRESETS`;
`web/panels.js → chooseStartupQualityPreset`;
`web/main.js → main`.

**Applies to** — `architecture/settings.md`, `architecture/web-shell.md`.

## Respect the per-stage storage-buffer limit — split passes, don't combine

**Decision** — The MAC loop is split into many small GPU passes, each binding only a
few storage buffers, rather than one mega-pass binding everything.

**Why** — `maxStorageBuffersPerShaderStage` is commonly only 8–10 on real adapters,
and the loop has many buffers (ping-ponged `u/v/w`, pressure ×2, divergence, cell
type, particle pos/vel, P2G sum/weight). A single combined pass would exceed the limit
and fail at pipeline creation — a classic "first GPU loop won't even build" failure.
This is a layout constraint, not an optimization.

**Exception — fuse a pass only when the budget *and* a capture both allow it.** The
three per-axis P2G scatter passes (`scatter_u/v/w`) were fused into one `scatter_all`
pass that reads each particle once and atomicAdds all three MAC components. This is the
opposite of the split rule, and it is allowed only because (a) the fused pass needs 7
storage buffers + 1 uniform, which fits the common 8-buffer floor with one slot to
spare, and the runtime asserts `max_storage_buffers_per_stage >= 8`; and (b) a real-GPU
capture confirmed the scatter section dropped (~14%) with bit-identical results
(integer atomics stay associative/commutative). The rule still holds for everything
else — do not combine passes that would exceed the probed limit, and do not claim a
fusion win without a capture.

**Code anchors** — `crates/fluid-lab/src/gpu/mod.rs → GpuCaps` (limit probe at boot);
`crates/fluid-lab/src/gpu/fluid.rs → dispatch_scatter`;
`crates/fluid-lab/src/gpu/shaders/scatter.wgsl → main`.

**Applies to** — `architecture/gpu-resources.md`, `architecture/simulation.md`.

## Particle spatial sort + workgroup-local scatter (default ON) — the high-count win

**Decision** — At high particle counts the per-particle P2G/G2P transfers are the
frame. Reorder particles into linear-cell-index order with a GPU counting sort before
P2G (default ON, `dev.particle_sort`, cadence `dev.particle_sort_period`=4), AND on
that sorted path run a workgroup-local pre-accumulation scatter
(`scatter_local.wgsl`) instead of the plain global-atomic scatter.

**Why** — The sort makes the G2P gather ~4× faster (coherent memory). But feeding
sorted particles into the *plain* scatter makes it SLOWER: cell-sorted particles in a
workgroup hammer the **same** grid-face atomic addresses simultaneously, serializing
the global atomics (the unsorted layout accidentally spreads them out). That
regression killed the plain sort at 13M/22M (the earlier "NOT shipped" result).
`scatter_local.wgsl` fixes it: each workgroup pre-accumulates its 64 sorted
particles' taps into an 8 KB shared-memory hash table, then flushes ONE global atomic
per touched face slot — collapsing the per-tap global-atomic count ~20-30× and cutting
the contention. Net result, drift-robust **interleaved A/B** captures (dev GPU, grid
128, N=4; sequential sweeps were too thermally noisy to trust — pair OFF/ON
back-to-back and take the paired median): **13.4M ON 16.1% faster (median 72.5→60.8
ms, 6/6 rounds win); 21.6M ON 17.0% faster (median 109.9→91.2 ms, 5/6 win, 1 tie);
6.6M also a win.**

**Determinism is preserved and is the gate.** The sort is a deterministic permutation,
and both the shared-memory accumulate and the global flush stay pure i32 fixed-point
(`FIXED_SCALE = 2^16`) — integer add is associative/commutative, so any
grouping/flush order yields identical `num`/`den`. Verified **0-pixel-diff** sorted vs
unsorted on the order-independent `render.hero.debug_view=10` ("Nearest Z") depth
stage after K fixed substeps. (Full-render and `liquid_cells` comparisons are too
noisy — particle draw-order alpha; use the depth stage.)

**Shared-memory budget** — `scatter_local.wgsl` uses 8 KB workgroup storage (two
`array<atomic<i32>, 1024>`: keys + vals), under the 16 KB WebGPU floor. Both scatter
pipelines share ONE explicit bind-group layout so a single `scatter_bg` drives either
(auto-layout would derive incompatible layouts — the same pitfall as RBGS red/black).

**Measurement note** — This GPU's run-to-run variance was 2-3×, so absolute numbers
from a single sequential sweep are unreliable; the verdict rests on interleaved paired
A/B medians (`app/tools/perf_determinism.mjs duel`), which cancel slow thermal drift.

**Code anchors** — `crates/fluid-lab/src/gpu/fluid.rs → record_sort`,
`dispatch_scatter`, `scatter_local_pl`, `scatter_bgl`, `scatter_pll`;
`crates/fluid-lab/src/gpu/shaders/scatter_local.wgsl → CAP`, `local_add`, `main`;
`crates/fluid-lab/src/gpu/shaders/sort_scan_block.wgsl → main`;
`crates/fluid-lab/src/gpu/shaders/sort_scan_spine.wgsl → main`;
`crates/fluid-lab/src/gpu/shaders/sort_scan_add.wgsl → main`;
`crates/fluid-lab/src/gpu/shaders/sort_scatter.wgsl → main`;
`crates/fluid-lab/src/settings/mod.rs → Registry` (`dev.particle_sort` default 1,
`dev.particle_sort_period` default 4).

**Applies to** — `architecture/simulation.md`, `architecture/gpu-resources.md`.

## Hot data is structure-of-arrays with fixed per-scene buffers and ping-pong

**Decision** — Hot simulation data uses structure-of-arrays layout, fixed-size
buffers per scene, and ping-pong buffers for iterative passes (pressure, scalar
fields). Avoid CPU/GPU readback during normal frames; keep debug readbacks throttled.

**Why** — SoA suits the GPU access pattern and avoids alignment traps from mirroring
arbitrary Rust structs into WGSL; fixed buffers avoid per-frame allocation; ping-pong
is the standard double-buffer for in-place iterative solves.

**Applies to** — `architecture/gpu-resources.md`, `architecture/simulation.md`.

## Rectangular-tank cost scales with nx·ny·nz; extreme aspect ratios are unaddressed

**Decision** — Cell counts are per-axis (`grid.res_x/y/z`, 16..128 each) at a fixed
uniform cell size, so per-frame work and buffer size scale with `nx·ny·nz`. The
SoA / pass-split layout above is unchanged — per-axis counts only change the buffer
extents, not the strategy. Extreme aspect ratios (very thin or very elongated tanks)
are a follow-up perf/quality concern: CFL and `FIXED_SCALE` were not re-tuned for them.

**Why** — Uniform `h` keeps the solver and buffer layout identical to the cube case;
only the dimensions differ, so the existing fixed-buffer / ping-pong rationale carries
over directly. Per-axis tuning of the timestep is deferred until extreme ratios matter.

**Code anchors** — `crates/fluid-lab/src/settings/mod.rs → Registry`
(`grid.res_x/y/z`); `crates/fluid-lab/src/sim/mod.rs → H`;
`crates/fluid-lab/src/gpu/fluid.rs → FIXED_SCALE`.

**Revisit when** — extreme tank aspect ratios produce instability or visible quality loss.

**Applies to** — `architecture/gpu-resources.md`, `architecture/simulation.md`.

## Fixed-dt substep cap: default 2, drop excess, catch up by rendering the next frame

**Decision** — `physics.max_substeps` defaults to 2. Each rendered frame advances at most `max_substeps` fixed-dt substeps; if the natural substep count would exceed the cap, the remaining accumulated sim time is dropped entirely that frame (not carried forward) and recorded in cumulative `dropped_time`. The browser catches up by rendering the next frame, not by making one frame longer.

**Why** — A 60 Hz frame naturally wants two 1/120 s physics substeps. Real-GPU timing with the previous cap showed `natural_substeps=2`, `substep_cap_hit=true`, `real_time_factor=0.4026`, and roughly 4.9 ms combined sim/render GPU time, so the default cap of 1 caused ordinary refresh-rate slow motion despite available frame budget. A default of 2 fixes that ordinary case while preserving a bounded cap for hitches.

**Tradeoffs** — The default now allows up to two fixed-dt substeps per rendered frame, so ordinary 60 Hz cadence can advance at real time. It is still not an adaptive or unbounded catch-up policy: a 30 Hz frame naturally wants four substeps, runs two by default, and drops the rest to avoid compounding overload. The profiler surfaces this as `real_time_factor`, plus executed/natural substeps and cap-hit state. For dev/stress testing, raising the setting higher allows more catch-up at the cost of occasional longer frames.

**Current policy note** — The measured policy change is only the default cap. `fixed_dt` remains 1/120 s, and `TimestepController` still zeroes the remaining accumulator when capped.

**Code anchors** — `crates/fluid-lab/src/timestep.rs → TimestepController::steps_for_frame`;
`crates/fluid-lab/src/settings/mod.rs → Registry` (`physics.max_substeps`).

**Applies to** — `architecture/app-shell.md`, `architecture/settings.md`.

## PNG export uses explicit fixed substeps, not rAF cadence

**Decision** — The first export workflow advances simulation through
`FluidApp::export_frame(substeps, sim_dt_s)`, with the tool requiring
`sim_seconds_per_frame` to be an integer multiple of the active `physics.fixed_dt`.
It bypasses the normal rAF accumulator while export mode is active.

**Why** — A headless sequence generator needs frame count and simulation duration to be
owned by the export job, not by wall-clock browser cadence or `render.fps_target`.
Keeping export stepping explicit makes the output deterministic within one run and
environment, and keeps dropped-time/catch-up policy out of generated frames.

**Tradeoffs** — The first slice does not support fractional-frame accumulation,
motion blur, subframe sampling, audio timing, or timeline/camera paths. Users choose
output FPS and simulation seconds per frame; the tool fails if that duration cannot be
represented by whole fixed substeps.

**Code anchors** — `crates/fluid-lab/src/lib.rs → FluidApp::export_frame`;
`crates/fluid-lab/src/timestep.rs → TimestepController::record_export_steps`;
`tools/export_sequence.mjs`.

**Applies to** — `architecture/app-shell.md`, `architecture/web-shell.md`,
`architecture/profiler.md`.

## Optimize only after profiling; 1M particles is a stretch benchmark

**Decision** — Do not rewrite the solver, buffers, or renderer on intuition — use
profiler data. The exception is obvious architecture risk (no normal-frame readback,
safe buffer layout, safe P2G). 1M particles at 30 FPS is a stretch benchmark to test
once the GPU path is mature, not an MVP design target.

**Why** — This sim has many plausible bottlenecks; blind optimization wastes budget
and can distort the architecture toward a number nobody needs yet.

**Applies to** — `architecture/gpu-resources.md`, `architecture/profiler.md`.

## Pressure optimizations need GPU evidence before performance claims

**Decision** — GPU residual active gating and pressure warm-start are Live controls
and do not justify pressure performance claims until measured with profiler/capture
evidence.

**Why** — Active gating avoids normal-frame readback and preserves the fixed
`solver.pressure_iterations` dispatch loop, so it can only reduce shader math/memory
after convergence. Warm-start preserves `pressure_a` and computes the initial
residual on GPU, but it also keeps the same fixed CG dispatch count.

**Code anchors** — `crates/fluid-lab/src/sim/pressure.rs → cg_solve_with_options`;
`crates/fluid-lab/src/gpu/fluid.rs → record_pressure`;
`crates/fluid-lab/src/gpu/shaders/cg_init.wgsl → main`;
`crates/fluid-lab/src/gpu/shaders/cg_spmv.wgsl → main`;
`crates/fluid-lab/src/gpu/shaders/cg_reduce.wgsl → main`;
`crates/fluid-lab/src/gpu/shaders/cg_reduce_final.wgsl → main`;
`crates/fluid-lab/src/gpu/shaders/cg_alpha.wgsl → main`;
`crates/fluid-lab/src/gpu/shaders/cg_update.wgsl → main`;
`crates/fluid-lab/src/gpu/shaders/cg_beta.wgsl → main`;
`crates/fluid-lab/src/gpu/shaders/cg_dir.wgsl → main`;
`crates/fluid-lab/src/gpu/shaders/cg_set_rsold.wgsl → main`.

**Revisit when** — Indirect dispatch lands behind conservative settings, or captures
name pressure iteration costs before/after residual gating or warm-start changes.

**Current evidence** — Real Chrome/WebGPU smoke captures on 2026-06-12 validated the
runtime paths for zero-start, warm-start, and warm-start plus residual gating. The raw
PNGs are ignored capture artifacts, so durable documentation keeps only the summary:
the runs reported `gpuDeviceStatus:"ok"` and real GPU timestamps. Treat this as
correctness/smoke evidence, not a controlled performance benchmark.

**Applies to** — `architecture/pressure-solver.md`, `architecture/profiler.md`.

## Keep one shared tiled particle-dispatch contract and preflight impossible scales

**Decision** — Particle-linear work uses one shared tiled dispatch contract across
mark, scatter U/V/W, G2P, and impulse, and create/Reset still preflight the exact
seeded particle count against tiled dispatch capacity plus the particle
storage-binding limit before allocation/submission.

**Why** — The old one-dimensional dispatch assumption made high seeded counts illegal
even when the shaders were otherwise correct. Raising the legal ceiling safely requires
one coordinated indexing model across every particle-linear path; loosening preflight
before those paths agree would trade a truthful rejection for invalid GPU work.

**Tradeoffs** — This raises the legal submission ceiling, not the measured frame-rate
ceiling. The scale matrix shows the old one-dimensional dispatch ceiling is no longer
the blocker, but that does not make the largest seeded counts a practical
frame-time target by itself.

**Code anchors** — `crates/fluid-lab/src/gpu/fluid.rs → particle_dispatch_shape`;
`crates/fluid-lab/src/gpu/mod.rs → validate_particle_scale`;
`crates/fluid-lab/src/gpu/shaders/mark.wgsl → particle_index`;
`crates/fluid-lab/src/gpu/shaders/scatter.wgsl → particle_index`;
`crates/fluid-lab/src/gpu/shaders/g2p.wgsl → particle_index`;
`crates/fluid-lab/src/gpu/shaders/impulse.wgsl → particle_index`.

**Revisit when** — the current real-GPU scale matrix identifies whether the next
bottleneck is dispatch legality no longer, transfer/G2P/render cost, or storage.

**Applies to** — `architecture/gpu-resources.md`, `architecture/simulation.md`, `architecture/profiler.md`.

## Prefer narrow arithmetic wins in particle-linear transfer paths before broader rewrites

**Decision** — The narrow `inv_h` transfer-path change is the right first arithmetic
win: use the precomputed inverse cell size in scatter and G2P
particle-to-grid/grid-to-particle coordinate conversion instead of dividing by `h`
per particle.

**Why** — It removes redundant per-particle divide work without changing dispatch
shape, fixed-point scatter semantics, wall-aware G2P invariants, or advection logic.

**Tradeoffs** — This does not eliminate transfer cost, and it does not solve the
remaining high-scale render cost. Render remains a separate decision surface rather
than something to bundle into a transfer-kernel patch.

**Code anchors** — `crates/fluid-lab/src/gpu/shaders/scatter.wgsl → main`;
`crates/fluid-lab/src/gpu/shaders/g2p.wgsl → main`;
`crates/fluid-lab/src/gpu/fluid.rs → Params`.

**Applies to** — `architecture/simulation.md`, `architecture/profiler.md`.

## Accept screen-space water while render cost and memory stay explicit

**Decision** — The water renderer may use screen-space R16 targets and multiple render
passes for thickness, speed-weighted whitewater, nearest depth, smoothing, and
composite, as long as captures keep reporting one honest `gpu.render_ms` total and
docs name the extra render memory.

**Why** — Per-billboard particle shading could not make deep volumes read as a coherent
lit body. Screen-space thickness and smoothing give the water a readable body without
forcing a second surface representation.

**Tradeoffs** — The path now owns persistent screen-sized render targets in addition
to simulation buffers, and render timing is a coarse multi-pass total rather than one
draw-pass cost. Refraction samples the offscreen scene-color prepass; the scene-color
target and visible scene detail make that cost legible.

**Applies to** — `architecture/rendering.md`, `architecture/gpu-resources.md`,
`architecture/profiler.md`.

## Do not carry an unused extracted-surface cost

**Decision** — GPU memory accounting and render timing cover the particle/grid
product path only; no dormant extracted-surface buffers, pipelines, or offscreen
targets are allocated.

**Why** — Unused surface infrastructure distorted memory and rendering measurements
for the scale work the product actually prioritizes.

**Tradeoffs** — Reintroducing any extracted-surface renderer requires a fresh measured
cost case and new runtime ownership.

**Applies to** — `architecture/gpu-resources.md`, `architecture/rendering.md`.

## See also

- [`../architecture/gpu-resources.md`](../architecture/gpu-resources.md)
- [`simulation.md`](simulation.md) · [`pressure.md`](pressure.md) · [`observability.md`](observability.md)
- [`../agent-context/maintaining-docs.md`](../agent-context/maintaining-docs.md)
