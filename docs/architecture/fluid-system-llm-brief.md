---
status:        active
owner:         adamg
last_updated:  2026-06-08
---

# Fluid system LLM brief

This is a shareable, current-state brief for asking external LLMs or rendering
experts for ideas. The canonical detailed owners are `simulation.md` for the
particle-grid solver, `pressure-solver.md` for incompressibility, and
`rendering.md` for visual output. This page is intentionally a bridge, not a
replacement for those docs.

## Current product shape

`fluid-lab` is a browser-native Rust/WASM/WebGPU bounded-tank liquid lab. The
simulation is a hybrid particle-grid solver: particles carry moving liquid mass and
velocity detail, while a staggered MAC grid enforces incompressibility each substep.
The renderer can show the raw particles directly, a screen-space water composite
derived from those same particles, a wireframe tank, and optional grid slices.

The most important design split is:

- **Particles are the visible moving mass.** They are the data source for both the
  basic particle look and the default screen-space water view.
- **The grid is the solver scaffold.** It stores cell types, staggered velocities,
  divergence, and pressure for projection; it is not the visible water surface.
- **Rendering is replaceable.** The simulation does not require the current
  screen-space water material. A future renderer can consume the particle buffer,
  grid buffers, or both.

## Simulation representation

The tank is a rectangular box with independent per-axis cell counts and one uniform
cell size `h = 2.0 / 64.0`. An all-equal grid reproduces the original centered cube.
The uniform cell size keeps the pressure operator isotropic even when the tank is
rectangular. Host indexing and classification live in
`crates/fluid-lab/src/sim/mod.rs -> GridDims`; the GPU implementation mirrors that
contract in `crates/fluid-lab/src/gpu/fluid.rs -> GpuFluid` and WGSL shaders under
`crates/fluid-lab/src/gpu/shaders/`.

Each particle is stored as `{pos: vec4, vel: vec4}` in a GPU storage buffer. The
particle initializer seeds deterministic jittered lattice positions from the selected
scene. The current particle count and scene layout define how much liquid exists at
reset; changing `render.particle_size` only changes visual coverage.

The grid uses MAC staggering: cell-centered scalar fields for type, pressure, and
divergence, and separate face-centered velocity fields for x/y/z flow. Boundary cells
are Solid, occupied interior cells are Liquid, and empty interior cells are Air.
Classification is rebuilt from particles every substep, so stale cell type should not
be treated as persistent state.

## Substep pipeline

One fixed simulation substep follows this shape:

1. Clear grid accumulators, occupancy, pressure, and diagnostics.
2. Mark occupied cells from particles and classify Solid/Liquid/Air cells.
3. Scatter particle velocities to staggered grid faces.
4. Normalize face accumulators into grid velocities.
5. Save the post-P2G, pre-force velocity field for FLIP deltas.
6. Add body forces such as gravity to liquid-adjacent faces.
7. Enforce closed-tank boundary velocities.
8. Compute divergence and solve pressure with Conjugate Gradient.
9. Subtract the pressure gradient and enforce boundaries again.
10. Gather grid velocities back to particles, blend PIC/FLIP, advect, and recover
    any wall-crossing particles.

The load-bearing invariant is fixed-point integer P2G scatter. WebGPU has no float
atomics, so `scatter.wgsl` accumulates into integer numerator/denominator buffers and
`normalize.wgsl` converts to floats after the atomics finish. This keeps P2G
deterministic across GPU thread scheduling.

The solver uses a high-FLIP blend by default for lively motion. Pure PIC damps motion;
high FLIP preserves splashes and wave energy but relies on projection, boundary
handling, and recovery to keep the tank stable.

## Current render modes

The renderer samples live GPU buffers; normal frames do not read simulation state back
to the CPU.

The selectable particle views are:

- **Screen-space water**: the default. Particle billboards write normalized
  world-space thickness, speed-weighted whitewater, and nearest front depth into
  screen-sized `R16Float` targets. Depth is smoothed, a screen-space normal is derived,
  and a composite pass blends Beer-Lambert tint, diffuse/specular/fresnel lighting,
  and a speed-based whitewater mask over the swapchain.
- **Optical particles**: the v1.10 optical-depth billboard view. It exposes individual
  particle motion and overlap better than the screen-space composite.
- **Simple particles**: the pre-v1.10 alpha billboard view. It is the basic particle
  look that still often reads better than the newer water materials because it does
  not pretend to be a continuous surface.

The default screen-space path has useful infrastructure: thickness accumulation,
front-depth smoothing, and a composite pass. It does not sample scene color, refract a
background, reflect an environment, simulate foam lifetime, or reconstruct a true
surface mesh.

## Why the current realistic-water attempts fall short

The v1.10 optical-particle direction was limited because each billboard only knows its
own local chord thickness. It cannot know whether it is a lone spray particle or the
front particle of a deep water volume.

The v1.11 screen-space direction fixed the missing accumulated-thickness signal and
added smoothed front-depth lighting, but it still lacks several cues humans expect from
clear water:

- no refracted scene detail behind the water,
- no real environment reflection or sky/room context,
- no coherent foam/bubble state with birth, advection, and decay,
- no surface mesh or world-space surface continuity outside the current camera view,
- no caustic or shadowing cue on the tank floor/walls.

The whitewater in the current composite is speed-weighted thickness. That can mark
fast regions, but realistic foam usually needs persistent air/bubble state generated
near breaking waves, impacts, high curvature, strong compression, or surface
separation. A fast-water mask alone tends to look like white tint rather than foam.

## Good external-LLM question framing

Ask for a renderer that consumes this existing simulation instead of replacing the
solver. Useful constraints to include:

- WebGPU in the browser; no normal-frame CPU/GPU readback.
- Source data: particle positions/velocities plus MAC grid cell types, face
  velocities, pressure, and divergence.
- Bounded tank, not ocean.
- Must keep basic particle and grid-slice inspection modes available.
- Current screen-space path already has thickness, nearest-depth smoothing, and a
  composite pass, but no scene-color refraction target.
- Basic particle view is a strong visual fallback; new work must beat it in captures.

High-value ideas to request:

- Whether to continue screen-space fluid rendering with scene-color refraction,
  environment reflection, and better foam, or pivot to particle-to-surface
  reconstruction.
- A practical WebGPU surface reconstruction path from particles or grid occupancy:
  splatted signed-distance field, marching cubes, screen-space surface only, or hybrid.
- A foam/whitewater model that fits this particle-grid solver without changing mass
  conservation: emission criteria, lifetime, advection, decay, and render material.
- A material model for clear tank water: absorption, Fresnel, reflection/refraction,
  roughness, floor/backdrop detail, and temporal stability.
- Capture-driven acceptance tests that compare against the current simple particle
  view at identical camera, scene, and particle count.

## See also

- `simulation.md` - canonical particle-grid solver facts
- `pressure-solver.md` - pressure projection and boundary conventions
- `rendering.md` - current render passes and view modes
- `settings.md` - render and simulation controls
- `../decisions/rendering.md` - rendering direction and constraints
- `../decisions/simulation.md` - simulation representation rationale
- `../agent-context/maintaining-docs.md`
