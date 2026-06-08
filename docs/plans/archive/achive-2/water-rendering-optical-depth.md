---
status:        abandoned
owner:         adamg
last_updated:  2026-06-08
okay_to_delete: true
long_lived:    false
owning_docs:
  - architecture/rendering.md
  - architecture/gpu-resources.md
  - architecture/settings.md
  - decisions/rendering.md
  - decisions/performance.md
---

# Water rendering optical depth

## Superseded

This source draft has been split into
[`water-rendering-research.md`](water-rendering-research.md) and
[`v1.10.0-water-rendering-optical-depth.md`](v1.10.0-water-rendering-optical-depth.md).
Use those docs for research and implementation. This file remains only as source
context until the next plan cleanup.

## Mission

Rethink how the fluid reads visually as water. The current particle/liquid-cell look is
better with color and shading controls, but alpha opacity is the wrong mental model:
one small droplet should be mostly see-through, while many overlapping droplets should
accumulate tint, depth, and density. Done means the team has chosen a rendering
direction that makes water more transparent at the surface and denser through depth,
with measured cost and clear trade-offs against observability.

## Scope

In scope:

- Define the target visual behavior in product terms: transparent thin water,
  accumulated color/absorption through many droplets, and readable motion.
- Audit why the current particle shader still looks shaded/heavy when opacity is low
  or sphere shading is off.
- Compare candidate directions:
  - optical-depth or transmittance accumulation over particles,
  - weighted blended transparency / order-independent transparency approximations,
  - screen-space or density-splat water from the current particles,
  - voxel/liquid-cell volume rendering,
  - a new extracted-surface pass, possibly a fresh marching-cubes replacement.
- If marching cubes returns, treat it as a new product/rendering design, not a
  resurrection of the old removed path.
- Capture before/after evidence at the same scene, color, opacity, particle count, and
  camera angle.

Out of scope:

- Changing simulation physics, compactness math, wall behavior, or particle count
  ceilings.
- Restoring old marching-cubes code unchanged.
- Hiding solver artifacts so completely that the app stops being inspectable as a
  fluid lab.
- Shipping a multi-pass renderer without measuring its cost.

## Approach

Start with design and measurement, not code. The important split is between a
particle-native transparency model and a surface/volume renderer.

A particle-native pass preserves the current product identity and likely has the
smallest simulation impact, but it may require multi-pass composition to accumulate
optical depth instead of blending each droplet as independent flat alpha.

A surface or volume pass may read more like water, but it changes the rendering
architecture and could bring back the cost/complexity that caused marching cubes to be
removed. If this path looks promising, write a focused follow-up plan before asking an
implementer to build it.

## High-level questions

- Is the desired first win "more water-like particles" or "a coherent water surface"?
- How much observability can the default render sacrifice for beauty if slice/profiler
  views remain available?
- Is a multi-pass render acceptable if it costs measurable frame time but gives the
  right optical-depth behavior?
- Should water rendering target high particle counts first, or make lower particle
  counts read denser?
- If marching cubes is reconsidered, what must be different from the removed version:
  lighting, smoothing, extraction resolution, cost model, or product role?

## Exit gate

- A short visual target statement exists for water transparency and depth buildup.
- Current particle opacity/shading failure modes are documented with screenshots or
  captures.
- At least two rendering directions are compared for visual quality, performance cost,
  implementation risk, and observability.
- The chosen next step is one of:
  - a small particle-shader/composition implementation plan,
  - a larger surface/volume-rendering design plan,
  - or a decision to defer water-look work until performance ceilings are raised.
- Any shipped visible change has a real-GPU browser capture and an honest profiler
  note.

## Discipline rules

- Do not restore the old marching-cubes stack by default.
- Do not make opacity controls lie. If opacity becomes optical density, name and
  document it that way.
- Do not claim "realistic water" without side-by-side visual evidence.
- Preserve an inspectable mode even if the default render becomes more cinematic.

## Migration notes (filled in at ship time)

- Update `architecture/rendering.md` for any new render pass, texture, order, or view.
- Update `architecture/gpu-resources.md` if the renderer adds offscreen targets,
  depth/accumulation textures, or new persistent buffers.
- Update `architecture/settings.md` if opacity/color/shading controls are renamed or
  change semantics.
- Update `decisions/rendering.md` for the particle-native vs surface/volume decision.
- Update `decisions/performance.md` if the accepted renderer changes measured render
  cost policy.

## See also

- `architecture/rendering.md`
- `decisions/rendering.md`
- `v1.2.0-marching-cubes-removal.md`
- `v1.3.0-scale-measurements.md`
