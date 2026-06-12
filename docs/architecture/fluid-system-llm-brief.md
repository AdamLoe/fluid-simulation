---
status:        active
owner:         adamg
last_updated:  2026-06-12
---

# Fluid system LLM brief

This page is a thin handoff for asking external LLMs or rendering experts for help.
The canonical owners are:

- `simulation.md` for the particle-grid solver
- `pressure-solver.md` for incompressibility
- `rendering.md` for current render modes and inspection views
- `settings.md` for user-facing controls and compatibility rules

Use this page to frame questions, not to restate the full subsystem docs.

## Current system shape

`fluid-lab` is a browser-native Rust/WASM/WebGPU bounded-tank liquid lab. It uses a
hybrid particle-grid solver: particles carry moving liquid mass and velocity detail,
while a staggered MAC grid handles classification, divergence, and pressure
projection. The renderer exposes a default screen-space water composite, selectable
particle views, a wireframe tank, and optional grid slices.

The durable split is:

- **Particles are the visible moving mass.** They feed both the basic particle view
  and the water composite.
- **The grid is the solver scaffold.** It owns cell type, staggered velocities,
  divergence, and pressure; it is not the visible surface.
- **Rendering is replaceable.** The simulation does not depend on one specific water
  material.

## What to ask an external model

Ask for ideas that fit the existing simulation instead of replacing it:

- WebGPU in the browser, with no normal-frame CPU/GPU readback.
- Source data: particle positions/velocities plus MAC grid cell types, face
  velocities, pressure, and divergence.
- Bounded tank, not ocean.
- Basic particle and grid-slice inspection modes must remain available.
- The current water path already has thickness accumulation, front-depth smoothing,
  and a composite pass.

Useful prompts usually ask about one of these:

- whether to keep improving the screen-space water path with refraction and
  environment cues, or switch to surface reconstruction
- a practical WebGPU surface-reconstruction path from particles or grid occupancy
- a foam model that fits the current particle-grid solver without changing mass
  conservation
- a material model for clear tank water that stays stable under capture
- acceptance tests that compare any new path against the current simple particle view

## See also

- `simulation.md`
- `pressure-solver.md`
- `rendering.md`
- `settings.md`
- `../decisions/rendering.md`
- `../decisions/simulation.md`
- `../agent-context/maintaining-docs.md`
