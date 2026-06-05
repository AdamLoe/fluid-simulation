---
status:         active
owner:          adamg
last_updated:   2026-06-05
okay_to_delete: false
long_lived:     true
owning_docs:    [architecture/rendering.md, architecture/simulation.md, architecture/settings.md, decisions/scope.md, decisions/rendering.md, decisions/performance.md]
---

# Roadmap — remaining direction

The core is shipped: a working, inspectable, interactive **64³ FLIP/PIC** fluid lab
with live config + real-GPU-timestamp profiler panels, GPU-native grid-slice
inspection, CG pressure solve, and a (default-off) marching-cubes surface. This plan
holds the remaining optional/forward direction. It is `long_lived` because it is
genuine future product direction that doesn't belong in current-state architecture.

Remaining direction, in rough execution order — **surface extraction is demoted**:

## Surface extraction polish (demoted / optional)

Marching cubes is built and wired (`architecture/rendering.md`) but off by default and
the project's single biggest trap (`decisions/rendering.md`). Only invest further if
the particle/voxel view is judged insufficient. A scalar field can be built from the
existing P2G weight (density) buffers. Keep particles the default render; any mesh work
needs a fallback contract first (lower scalar res / update every N frames / cap
triangle output / fall back to particles).

## Portfolio polish

- Reconcile the two web entry paths (`architecture/web-shell.md`): make Vite import the
  same modules, or drop Vite and ship the static path (the safe bet).
- Verify the `#unsupported` WebGPU overlay renders cleanly; add a poster/GIF + honest
  caveat copy.
- Honest framing: "browser-native Rust/WASM/WebGPU 3D fluid lab" — FLIP/PIC, 64³, CG
  pressure, with the volume-compaction caveat. NOT "scientific CFD", NOT
  "photorealistic". Camera presets, a title/explainer. (`decisions/scope.md` portfolio
  honesty.)

## Scale / quality pass

- **FLIP volume fidelity** is the headline open quality problem: a settled 64³ pool
  plateaus at ~19.2k liquid cells vs an ideal ~31.8k — inherent FLIP volume loss, not
  solver convergence (`decisions/pressure.md`). Closing it is transfer-quality work
  (APIC/affine transfer, density correction) or higher resolution.
- Reset-class settings now rebuild via `recreate_fluid` — the panel's reset-class path
  triggers reset → `recreate_fluid`, so live grid-resolution changes work
  (`grid.res_x/y/z` are per-axis Reset-class; `app/crates/fluid-lab/src/lib.rs → reset`).
  Remaining: confirm the same path covers particle-count and the scenario selector.
- Define low/default/high tiers from measurements (`decisions/performance.md`).
  Optional 128³ exploration (2M+ cells) only if 64³ headroom + memory allow.
- 1M-particle stretch test.

## Floating / bouncing objects (deferred)

A cube/sphere object with size + weight controls that floats and bounces in the tank.
Deferred this cycle (`decisions/scope.md`). Two tiers were assessed; **start with
Tier A**:
- **Tier A (suggested start)** — CPU-side rigid body: geometric buoyancy + drag +
  wall-bounce, rendered as a cube/sphere mesh, optional weak fluid push via the existing
  impulse pass. Low risk — no pressure-solver, readback, or determinism changes.
- **Tier B** — object as moving solid cells in the pressure projection. Breaks the
  load-bearing "every Liquid cell is interior / no bounds checks" CG invariant;
  multi-week with uncertain solver stability.

## See also

- [`index.md`](index.md) — plans landing + lifecycle.
- [`../decisions/scope.md`](../decisions/scope.md) — what's optional and the kill switches.
- [`../architecture/rendering.md`](../architecture/rendering.md) · [`../architecture/simulation.md`](../architecture/simulation.md)
