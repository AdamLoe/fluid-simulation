---
status:        active
owner:         adamg
last_updated:  2026-06-12
---

# Decisions — Performance

## Dense wall fill is removed until profiler data justifies a new implementation

**Decision** — Dense wall fill no longer ships as a runtime feature. The cheaper
wall-contact normal/depth snap remains the default near-wall aid.

**Why** — Wall fill owns a large supersampled occupancy atlas and per-frame splat +
injection work. Without capture/profiler evidence that it improves the startup view more
than it costs, it should not be in the normal frame path.

**Tradeoffs** — Reintroducing dense wall fill requires a fresh measured cost case,
new settings, new resource ownership, and captures. There is no occupancy buffer,
wall-fill injection pass, or `wallfill_mask` target in the current runtime.

**Code anchors** — `crates/fluid-lab/src/settings/mod.rs → Registry`;
`crates/fluid-lab/src/gpu/mod.rs → GpuContext::render`.

**Revisit when** — repeated real-GPU captures show wall fill materially improves default
quality and profiler samples show acceptable median `gpu.render_ms`.

**Applies to** — `architecture/rendering.md`, `architecture/gpu-resources.md`,
`architecture/settings.md`.

## Resolution targets: 32³ baseline, 64³ first serious target, 128³ aspirational

**Decision** — A stable/interactable 32³ is the required floor; 64³ is the first
serious measured target and the current default; 128³ is aspirational and must not
drive the architecture. A smaller stable preset with measured justification is an
acceptable ship.

**Why** — 128³ is 2M+ cells before particles and solver workspace.
Starting there builds an impressive target instead of a working app. Correct 32³
beats broken or unmeasured 64³.

**Tradeoffs** — 64³ looks modest until surface rendering and scenarios arrive, in
exchange for developing against real GPU constraints sooner.

**Applies to** — `architecture/simulation.md`, `architecture/gpu-resources.md`.

## Keep tiers as measurement output, not a runtime system

**Decision** — Scene presets may exist as authored Reset-class scenarios, but
low/default/high tier labels remain measurement outputs, not a runtime system.

**Why** — Tiers are only meaningful once measurements exist and a human is choosing.
Building a tier system earlier serves no consumer; authored scene presets are a
separate product affordance, not a performance-tier architecture.

**Applies to** — `architecture/settings.md`, `architecture/gpu-resources.md`.

## Respect the per-stage storage-buffer limit — split passes, don't combine

**Decision** — The MAC loop is split into many small GPU passes, each binding only a
few storage buffers, rather than one mega-pass binding everything.

**Why** — `maxStorageBuffersPerShaderStage` is commonly only 8–10 on real adapters,
and the loop has many buffers (ping-ponged `u/v/w`, pressure ×2, divergence, cell
type, particle pos/vel, P2G sum/weight). A single combined pass would exceed the limit
and fail at pipeline creation — a classic "first GPU loop won't even build" failure.
This is a layout constraint, not an optimization.

**Code anchors** — `app/crates/fluid-lab/src/gpu/mod.rs → GpuCaps` (limit probe at boot);
`app/crates/fluid-lab/src/gpu/fluid.rs` (pass split).

**Applies to** — `architecture/gpu-resources.md`.

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

**Code anchors** — `app/crates/fluid-lab/src/settings/mod.rs → grid.res_x/y/z`;
`app/crates/fluid-lab/src/sim/mod.rs → H`; `app/crates/fluid-lab/src/gpu/fluid.rs → FIXED_SCALE`.

**Revisit when** — extreme tank aspect ratios produce instability or visible quality loss.

**Applies to** — `architecture/gpu-resources.md`, `architecture/simulation.md`.

## Fixed-dt substep cap: default 2, drop excess, catch up by rendering the next frame

**Decision** — `physics.max_substeps` defaults to 2. Each rendered frame advances at most `max_substeps` fixed-dt substeps; if the natural substep count would exceed the cap, the remaining accumulated sim time is dropped entirely that frame (not carried forward) and recorded in cumulative `dropped_time`. The browser catches up by rendering the next frame, not by making one frame longer.

**Why** — A 60 Hz frame naturally wants two 1/120 s physics substeps. Real-GPU timing with the previous cap showed `natural_substeps=2`, `substep_cap_hit=true`, `real_time_factor=0.4026`, and roughly 4.9 ms combined sim/render GPU time, so the default cap of 1 caused ordinary refresh-rate slow motion despite available frame budget. A default of 2 fixes that ordinary case while preserving a bounded cap for hitches.

**Tradeoffs** — The default now allows up to two fixed-dt substeps per rendered frame, so ordinary 60 Hz cadence can advance at real time. It is still not an adaptive or unbounded catch-up policy: a 30 Hz frame naturally wants four substeps, runs two by default, and drops the rest to avoid compounding overload. The profiler surfaces this as `real_time_factor`, plus executed/natural substeps and cap-hit state. For dev/stress testing, raising the setting higher allows more catch-up at the cost of occasional longer frames.

**Current policy note** — The measured policy change is only the default cap. `fixed_dt` remains 1/120 s, and `TimestepController` still zeroes the remaining accumulator when capped.

**Code anchors** — `app/crates/fluid-lab/src/timestep.rs → TimestepController::steps_for_frame`; `app/crates/fluid-lab/src/settings/mod.rs → physics.max_substeps`.

**Applies to** — `architecture/app-shell.md`, `architecture/settings.md`.

## Optimize only after profiling; 1M particles is a stretch benchmark

**Decision** — Do not rewrite the solver, buffers, or renderer on intuition — use
profiler data. The exception is obvious architecture risk (no normal-frame readback,
safe buffer layout, safe P2G). 1M particles at 30 FPS is a stretch benchmark to test
once the GPU path is mature, not an MVP design target.

**Why** — This sim has many plausible bottlenecks; blind optimization wastes budget
and can distort the architecture toward a number nobody needs yet.

**Applies to** — `architecture/gpu-resources.md`, `architecture/profiler.md`.

## Pressure optimizations need GPU evidence before performance claims

**Decision** — GPU residual active gating and pressure warm-start are default-off
Live controls and do not justify pressure performance claims until measured with
profiler/capture evidence.

**Why** — Active gating avoids normal-frame readback and preserves the fixed
`solver.pressure_iterations` dispatch loop, so it can only reduce shader math/memory
after convergence. Warm-start preserves `pressure_a` and computes the initial
residual on GPU, but it also keeps the same fixed CG dispatch count.

**Code anchors** — `app/crates/fluid-lab/src/sim/pressure.rs → cg_solve_with_options`;
`app/crates/fluid-lab/src/gpu/fluid.rs → record_pressure`;
`app/crates/fluid-lab/src/gpu/shaders/cg_*.wgsl`.

**Revisit when** — Indirect dispatch lands behind conservative settings, or captures
name pressure iteration costs before/after the default-off pressure controls.

**Current evidence** — Real Chrome/WebGPU smoke captures on 2026-06-12 validated the
runtime paths: default zero-start (`captures/llm-overhaul-final-default-detailed.png`),
warm-start via `?set=solver.pressure_warm_start:1`
(`captures/llm-overhaul-final-warm-start.png`), and warm-start plus residual gating
(`captures/llm-overhaul-final-warm-residual.png`). All reported
`gpuDeviceStatus:"ok"` and real GPU timestamps. Treat these as correctness/smoke
evidence, not a controlled performance benchmark.

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

**Code anchors** — `app/crates/fluid-lab/src/gpu/shaders/scatter.wgsl`;
`app/crates/fluid-lab/src/gpu/shaders/g2p.wgsl`;
`app/crates/fluid-lab/src/gpu/fluid.rs -> Params`.

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
