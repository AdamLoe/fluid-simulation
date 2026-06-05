---
status:        active
owner:         adamg
last_updated:  2026-06-05
---

# Decisions — Scope

## Bounded 3D tank before any infinite-ocean work

**Decision** — The project is a bounded 3D water simulation in a tank, not an infinite
ocean simulator. Infinite ocean / heightfield / FFT-wave work stays future work or a
separate companion demo.

**Why** — A bounded tank can show true volumetric behaviour (volume, pressure,
splashes, obstacles, falling/dam-break motion, debug cutaways). An ocean scene pulls
toward visual-only water tricks.

**Tradeoffs** — The demo initially looks smaller than an ocean scene, but is deeper
and more physically meaningful.

**Applies to** — `architecture/simulation.md`, `overview.md`.

## The tank may be a rectangular box, not only a cube

**Decision** — The cubic-tank assumption is lifted: the tank is a rectangular box set
by independent per-axis cell counts (`grid.res_x/y/z`, 16..128 each) at a single
uniform cell size. Setting all three axes equal reproduces the original cube. The
pressure operator stays isotropic (uniform `h`); only the cell counts differ per axis.

**Why** — Per-axis counts at a fixed cell size give shallow/tall/wide tanks without
introducing anisotropic spacing, which would complicate the pressure operator and the
"every Liquid cell is interior" CG invariant. Uniform `h` keeps the solver unchanged.

**Tradeoffs** — Extreme aspect ratios are an untuned follow-up (see
`decisions/performance.md`); CFL / `FIXED_SCALE` were not re-tuned for them.

**Code anchors** — `app/crates/fluid-lab/src/settings/mod.rs → grid.res_x/y/z`;
`app/crates/fluid-lab/src/sim/mod.rs → H` (uniform cell size).

**Applies to** — `architecture/simulation.md`, `architecture/settings.md`.

## The fluid lab is the direction, but not the first-version scope

**Decision** — The long-term product is an inspectable fluid lab (particle/grid/
divergence/pressure/velocity/scalar views, cutaways, mesh, final render, split-view).
The first version proves the core loop first; the full lab is staged afterward.

**Why** — The lab concept is what makes the project distinctive, but shipping all of
it in the first version would over-scope a build with a hard technical core.

**Applies to** — `plans/roadmap.md`, `architecture/rendering.md`.

## Make the simulation pipeline a first-class, visible product concept

**Decision** — The app exposes the pipeline `Particles → MAC grid → divergence →
pressure → velocity → scalar field → mesh → final render` as render modes / a pipeline
strip, with concise honest explanations.

**Why** — Viewers seeing the internal machinery is what differentiates this from an
ordinary shader demo.

**Applies to** — `architecture/rendering.md`, `plans/roadmap.md`.

## Typed scene config; static scenes before moving solids

**Decision** — Scenes are built from a small typed `SceneConfig` (not hardcoded into
the solver), with a scene-selector across the shipped static presets. Static scenes
and obstacles come before moving paddles / dynamic solids, which stay deferred.

**Why** — A typed scene object keeps scenario selection clean rather than hardcoding
scenes into the solver. Dynamic solids destabilize boundaries, projection, recovery,
and reset determinism, so they are a deliberate follow-up, not accidental scope creep.

**Code anchors** — `app/crates/fluid-lab/src/scene/mod.rs → SceneConfig`.

**Applies to** — `architecture/app-shell.md`, `plans/roadmap.md`.

## Floating / bouncing rigid objects stay deferred (two-tier audit)

**Decision** — Floating/bouncing rigid objects (cube/sphere with size + weight) are
scoped out. A cycle audit assessed two tiers; if pursued, start with Tier A:
- **Tier A** — a CPU-side rigid body with geometric buoyancy + drag + wall-bounce,
  rendered as a cube/sphere mesh, with optional weak fluid push via the existing
  impulse pass. Low risk: no pressure-solver, readback, or determinism changes.
- **Tier B** — the object as moving solid cells inside the pressure projection. This
  breaks the load-bearing "every Liquid cell is interior / no bounds checks" CG
  invariant, is multi-week, and has uncertain solver stability.

**Why** — Tier A buys the visible feature without touching the solver contract; Tier B
pays a large, risky solver rewrite for two-way coupling that the demo does not need.

**Applies to** — `plans/roadmap.md`, `architecture/simulation.md`.

## Optional features are not phase blockers; use kill switches

**Decision** — Split view, guided tour, pressure-comparison UI, pouring spout, moving
paddle, foam/spray, transparent/refractive material, replay/scrub, 128³, and 1M
particles are optional. None blocks a phase; unstable optional work is hidden,
deferred, or labeled experimental. When a target clearly blocks progress, take the
documented kill switch (drop to 48³/32³; particles/voxels instead of mesh; hide an
unstable scenario; defer 128³ / 1M particles).

**Why** — These features are valuable only after the stable inspectable core exists.
Treating them as required turns the project into a large graphics surface before the
fluid loop is trustworthy. A complete, measured, honest demo beats an unfinished
maximal one.

**Applies to** — `plans/roadmap.md`.

## Deterministic replay/scrub is future work

**Decision** — Replay/scrub of the last few seconds across all render modes is a
high-value future idea, deferred until frame-state ownership, buffer layout, and render
modes are stable.

**Why** — Scrubbing the same moment across final render / particles / pressure /
velocity / scalar / mesh would make the lab unusually inspectable, but it adds memory,
state-capture, and UI complexity that should not be paid before the core is stable.

**Applies to** — `plans/roadmap.md`.

## Portfolio honesty

**Decision** — This is a portfolio-grade real-time fluid lab, not a validated
scientific CFD solver. Copy and framing state the method (FLIP/PIC, 64³, CG pressure)
and known limits; simulation problems are not hidden behind fake polish. An
unsupported-WebGPU fallback is required before public presentation.

**Why** — Overstated accuracy or hidden problems undermine a portfolio piece more than
a modest, honest one.

**Applies to** — `architecture/web-shell.md`, `plans/roadmap.md`.

## See also

- [`platform.md`](platform.md) · [`rendering.md`](rendering.md) · [`performance.md`](performance.md)
- [`../plans/roadmap.md`](../plans/roadmap.md)
- [`../agent-context/maintaining-docs.md`](../agent-context/maintaining-docs.md)
