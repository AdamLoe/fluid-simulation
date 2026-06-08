---
status:        active
owner:         adamg
last_updated:  2026-06-07
---

# Decisions — Performance

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

## Run exactly one preset until the human-facing pass; tiers are 1.5 work

**Decision** — Until the 1.5 scale/quality pass there is exactly one preset (whatever
runs). The low/default/high tier system is a 1.5 *output* set from measurements, not a
running concern.

**Why** — Tiers are only meaningful once measurements exist and a human is choosing.
Building a tier system earlier serves no consumer.

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

## Fixed-dt substep cap: default 1, drop excess, catch up by rendering the next frame

**Decision** — `physics.max_substeps` defaults to 1. Each rendered frame advances at most `max_substeps` fixed-dt substeps; if the natural substep count would exceed the cap, the remaining accumulated sim time is dropped entirely that frame (not carried forward) and recorded in cumulative `dropped_time`. The browser catches up by rendering the next frame, not by making one frame longer.

**Why** — A slow frame should stay cheap. A high cap allows unbounded catch-up: one hitch produces N× the normal per-frame work, compounding the overload and stalling interactivity. With the default of 1 the worst case per frame is exactly one fixed-dt substep, so frame time is predictable. The physics falls slightly behind under load but recovers immediately on the next frame.

**Tradeoffs** — At max\_substeps=1 the sim runs at most one substep per rendered frame, so on a 30 fps machine with a 120 Hz fixed\_dt the sim effectively runs at 30 Hz physics. For dev/stress testing, raising to 4 allows catch-up at the cost of occasional longer frames.

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

## Reject impossible one-dimensional particle dispatches before allocation

**Decision** — Until particle-linear shaders support tiled indexing, Reset preflights
the exact seeded particle count against the adapter's one-dimensional workgroup and
single-storage-binding limits. Impossible requests preserve the running simulation and
report a rejected scale status instead of submitting invalid commands.

**Why** — On the measured BrowserWebGpu adapter,
`maxComputeWorkgroupsPerDimension=65,535` and workgroup size 64 impose a 4,194,240
particle dispatch ceiling. Requested 8M seeds 7,939,819 and previously generated an
invalid command every frame. Requested 4.20M seeds 4,171,888 and runs; requested
4.25M seeds 4,209,137 and is rejected.

**Tradeoffs** — This makes the current ceiling truthful rather than raising it.
Dispatch tiling requires coordinated particle indexing across mark, scatter, G2P, and
impulse paths and is deferred until it can be verified as one change.

**Evidence** — `../plans/v1.3.0-scale-measurements.md`.

**Applies to** — `architecture/gpu-resources.md`, `architecture/profiler.md`.

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
