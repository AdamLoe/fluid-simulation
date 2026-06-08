---
status:        abandoned
owner:         adamg
last_updated:  2026-06-08
okay_to_delete: true
long_lived:    false
owning_docs:
  - architecture/gpu-resources.md
  - architecture/simulation.md
  - architecture/profiler.md
  - architecture/rendering.md
  - decisions/performance.md
---

# Particle scale dispatch and performance

## Superseded

This source draft has been split into
[`particle-dispatch-audit.md`](particle-dispatch-audit.md),
[`v1.8.0-particle-dispatch-tiling.md`](v1.8.0-particle-dispatch-tiling.md), and
[`v1.9.0-particle-performance-followup.md`](v1.9.0-particle-performance-followup.md).
Use those docs for research and implementation. This file remains only as source
context until the next plan cleanup.

## Mission

Run the next real performance pass with a sharper goal: raise the particle-count
ceiling and then optimize the measured bottlenecks. The current 8M request is not just
slow; it is rejected because particle-linear compute passes use one-dimensional
dispatches that hit the adapter's `maxComputeWorkgroupsPerDimension` ceiling around
4.2M particles. Done means the team knows whether tiled particle dispatch can safely
raise the limit, what the new bottleneck becomes, and which measured optimizations are
worth implementing.

## Scope

In scope:

- Audit every particle-linear GPU path that assumes one-dimensional dispatch or a
  simple global particle index.
- Design a coordinated tiled/2D dispatch indexing strategy for particle paths such as
  mark/classify inputs, scatter, G2P, advect/recover, and impulse/wave forces.
- Verify that all particle paths agree on indexing, bounds checks, and partial tiles.
- Re-run the real-GPU scale matrix after the ceiling change, including at least 2M,
  4M, and 8M requested particles.
- Use profiler output to choose the next optimization class: transfer/scatter/G2P,
  render, pressure/grid, memory/allocation, or timing policy.
- Keep performance claims hardware-dependent and tied to raw captures/profiler output.

Out of scope:

- Water-look rendering experiments. Those belong to
  `water-rendering-optical-depth.md`, though this plan should measure render cost if
  render becomes the bottleneck.
- Source/drain or new physics features.
- Blind shader rewrites without profiler evidence.
- Universal promises like "8M at 30 FPS" before measurement.

## Approach

Treat this as two separate questions:

1. Can the app legally and correctly run more particles than the current 1D dispatch
   ceiling?
2. Once it can, what actually limits frame rate?

The first question is a correctness/architecture pass. Tiled indexing must be applied
coherently across every particle-linear compute path; a local fix to one shader is not
enough. It should preserve the current preflight behavior by changing the legal limit,
not by removing safety checks.

The second question is a measurement pass. After 8M can at least reset and submit
valid work, profile it fresh. It may reveal transfer/scatter/G2P cost, render cost,
memory pressure, dropped-time policy, or a browser watchdog/adapter limit. Optimize
only the measured top class.

## High-level questions

- Is the target "8M can run at all" or "8M should be interactive near 30 FPS"?
- Should the first implementation preserve the exact visual particle count, or is
  render decimation/LOD acceptable after the sim runs more particles?
- Should the ceiling work target the measured high-end adapter first, or enforce a
  broader adapter compatibility floor?
- Is it acceptable for the app to expose a high-count preset that runs slower but
  remains honest and controllable?
- Which evidence should decide success: raw `stats_json`, captures, profiler panel,
  or a saved measurement table?

## Exit gate

- The plan identifies every shader/runtime path that must change for tiled particle
  dispatch.
- Requested 8M no longer fails because of a one-dimensional dispatch limit, or the
  plan records the exact remaining architectural blocker.
- A fresh matrix records at least requested 2M, 4M, and 8M with particles actual,
  scale status, frame percentiles, dropped time, GPU memory, timing source, and sorted
  GPU costs.
- The next optimization class is selected from evidence, not intuition.
- Any shipped optimization has before/after captures or profiler output at the same
  scene/scale.

## Discipline rules

- Do not remove preflight safety; update it to match the new legal dispatch model.
- Do not change wall-aware G2P behavior accidentally while touching G2P indexing.
- Do not claim a higher limit unless reset, dispatch, render, and profiling all agree.
- Separate "higher ceiling" from "faster frame time" in reports.

## Migration notes (filled in at ship time)

- Update `architecture/gpu-resources.md` for the new dispatch/indexing limits and
  preflight behavior.
- Update `architecture/simulation.md` for particle-linear shader indexing changes.
- Update `architecture/profiler.md` if scale facts or stats fields change.
- Update `architecture/rendering.md` if render decimation/LOD or renderer limits
  change.
- Update `decisions/performance.md` with measured claims and rejected optimizations.

## See also

- `v1.3.0-scale-measurements.md`
- `v1.3.0-scale-performance-profiler.md`
- `architecture/gpu-resources.md`
- `decisions/performance.md`
