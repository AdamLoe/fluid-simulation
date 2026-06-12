---
status:         active
owner:          adamg
last_updated:   2026-06-07
okay_to_delete: false
long_lived:     true
owning_docs:    [architecture/rendering.md, architecture/simulation.md, architecture/settings.md, architecture/profiler.md, architecture/app-shell.md, decisions/scope.md, decisions/rendering.md, decisions/performance.md, decisions/simulation.md]
---

# Roadmap - remaining direction

The core is shipped: a working, inspectable, interactive **64³ FLIP/PIC** fluid lab
with live config + real-GPU-timestamp profiler panels, GPU-native grid-slice
inspection, CG pressure solve, tank auto-roll / impulse wave controls, and a
particle/liquid-cell visual direction. This plan holds future direction that does not
belong in current-state architecture. It is `long_lived`; active implementation
coordination lives in disposable versioned plans.

The active implementation hub for this archived period was the deleted
`v1.x-particle-scale-orchestrator.md` plan. That hub recorded MC removal,
scale/profiler work, wall/volume quality, settings help/compactness, and low-risk
interactive forces.

## Current direction: particle and voxel scale

Particles and liquid-cell/voxel inspection are the product surface. Marching cubes is
not a deferred polish target anymore; the deleted `v1.2.0-marching-cubes-removal.md`
plan removed it completely. Any future surface idea must be a ground-up design that
earns its place after scale work, not a return to the old MC path.

Scale is the headline: use the profiler and real-GPU capture output to find how much
water the user's high-end PC can run at about 30 FPS. Performance claims stay
hardware-dependent until broader measurements exist.

## Future source and drain

Source/drain remains desired future work, but it is not part of the shipped
interaction-control surface. When it returns, it should create and destroy particles or
water volume rather than fake the effect visually. It needs its own plan because it
touches particle allocation, mass accounting, liquid classification, reset/live
semantics, and maybe boundary behavior.

Likely future questions:

- Is source/drain a creative live tool, scenario preset, or both?
- Does drain delete particles permanently, recycle them to a source, or maintain a
  target particle budget?
- How does source seeding avoid clumps, walls, and invalid pressure cells?
- Which profiler counters prove it scales?

## Future surface/view ideas

No marching-cubes polish. If particle/voxel scale still needs a denser read after the
active map ships, consider particle-native or screen-space approaches that do not
rebuild the old MC stack:

- denser particle shading from fewer simulated particles,
- screen-space splats/metaballs,
- voxel/liquid-cell render modes that show volume directly,
- lower-cost density views for inspection rather than photoreal surface extraction.

## Portfolio polish

- Reconcile the two web entry paths (`architecture/web-shell.md`): make Vite import the
  same modules, or drop Vite and ship the static path (the safe bet).
- Verify the `#unsupported` WebGPU overlay renders cleanly; add a poster/GIF + honest
  caveat copy.
- Honest framing: "browser-native Rust/WASM/WebGPU 3D fluid lab" — FLIP/PIC, 64³, CG
  pressure, with the volume-compaction caveat. NOT "scientific CFD", NOT
  "photorealistic". Camera presets, a title/explainer. (`decisions/scope.md` portfolio
  honesty.)

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

- [`../../index.md`](../../index.md) — plans landing + lifecycle.
- `v1.x-particle-scale-orchestrator.md` — deleted implementation map for this archived period.
- [`../../../decisions/scope.md`](../../../decisions/scope.md) — product scope.
- [`../../../architecture/rendering.md`](../../../architecture/rendering.md) · [`../../../architecture/simulation.md`](../../../architecture/simulation.md)
