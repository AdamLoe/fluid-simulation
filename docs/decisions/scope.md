---
status:        active
owner:         adamg
last_updated:  2026-06-07
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

**Code anchors** — `app/crates/fluid-lab/src/settings/mod.rs → grid_res_x / grid_res_y / grid_res_z`;
`app/crates/fluid-lab/src/sim/mod.rs → H` (uniform cell size).

**Applies to** — `architecture/simulation.md`, `architecture/settings.md`.

## Particle count is derived from a per-seeded-cell density, not a raw absolute

**Decision** — The seeded particle population is controlled by a particles-per-cell
density (`particles.density`, default `8`), and the spawn count is **derived** as
`round(density * seeded_volume_fraction * total_grid_cells)`. "Per cell" means per
*seeded fluid cell* (the liquid-block volume in cell units), not per total grid cell.
The old raw `particles.count` becomes an advanced manual override where `0` = Auto
(derive from density) and a nonzero value pins an absolute count.

**Why** — A raw absolute count silently became wrong density whenever grid resolution
changed (e.g. ~11/cell at 128×64×128). Per-seeded-cell keeps both the default
`80×40×80` scene (~410k at the default fill) and larger grids sane, and tracks how much
of the tank a scenario fills, so denser scenes get proportionally more particles. Density `8` matches
the standard FLIP/PIC ~8/cell target and the prior default's effective ~7.7/seeded-cell.

**Tradeoffs** — The exact count now depends on the active scenario's fill fraction, so
it varies between presets at the same density; the advanced override exists for callers
that need an exact number.

**Code anchors** — `app/crates/fluid-lab/src/scene/mod.rs → resolved_particle_count`;
`app/crates/fluid-lab/src/settings/mod.rs → particle_density / particle_count_override`.

**Applies to** — `architecture/settings.md`.

## Volume (tank fill) and density are orthogonal; low-density volume is fixed by splat-radius scaling, not an SDF surface

**Decision** — Split the conflated density concept into two orthogonal knobs.
`scene.fill_level` (the **tank-fill percentage**, Reset, stored 0–100, default `20`)
controls *how much* water there is — it is a literal waterline (0 = empty tank,
100 = full, 50 = halfway up by height). The default scene seeds a full-footprint
floor slab from y=0 up to `fill` of the tank height, so the particle count follows
automatically. The named dynamic scenarios keep their shape but scale with `fill`
(dam-break wall height = `fill`; double-splash drop size = `fill`). `particles.density` becomes a pure
fidelity/cost knob and is made **volume-neutral**: the visible body stays the same
size as density drops, just blobbier. This is achieved cheaply by (a) scaling the
render splat radius with the seeded inter-particle spacing
(`radius = H · effective_density^(-1/3) · SPLAT_RADIUS_PER_SPACING`, constant `0.7`
to reproduce today at density 8), (b) auto-enabling the existing one-ring
`classify.wgsl` surface dilation below the reference density (8/cell) so the physics
liquid region stays hole-free, and (c) coupling the divergence anti-clump rest target
to the actual particle density (`effective_rest_density`) so the *dynamics* are
density-invariant too — without it, the occupancy-driven outward push scaled with
density and the water moved like a different volume (see `decisions/simulation.md`).
The **SDF / marching-cubes surface rewrite — the "proper" coverage fix — is
deliberately deferred** to a future plan.

**Why** — The visible water is built from particle splats, not liquid cells, so a
fixed-radius splat made lowering density *look like less water* even though the
seeded region was identical — wrong, because density should be cost-only. The
splat-radius + dilation approach decouples the two knobs at a fraction of the cost of
an SDF surface, which would be a large graphics-surface investment ahead of the
product need (see "Optional features are deferred").

**Tradeoffs** — The splat approach is a coverage approximation: at very low density
the body looks blobby (accepted), and the physics liquid-cell count is only
~density-invariant within ~15% (a density-dependent dilation rind) rather than exact.
The fast `filled_volume` proxy (`liquid_cells × H³`) and `app/tools/density_motion_sweep.mjs`
back the invariant; the screenshots are the real acceptance. Tuning
`SPLAT_RADIUS_PER_SPACING` and the dilation trigger is Phase-2 calibration-sweep work.

**Code anchors** — `app/crates/fluid-lab/src/scene/mod.rs → preset_blocks /
effective_particle_density / effective_surface_dilation / seeded_spacing`;
`app/crates/fluid-lab/src/gpu/mod.rs → SPLAT_RADIUS_PER_SPACING`;
`app/crates/fluid-lab/src/gpu/fluid.rs → effective_surface_dilation`.

**Applies to** — `architecture/simulation.md`, `architecture/rendering.md`,
`architecture/settings.md`.

## The fluid lab is the direction, but not the first-version scope

**Decision** — The long-term product is an inspectable fluid lab (particle/grid/
divergence/pressure/velocity views, cutaways, focused rendering, and split-view).
The first version proves the core loop first; the full lab is staged afterward.

**Why** — The lab concept is what makes the project distinctive, but shipping all of
it in the first version would over-scope a build with a hard technical core.

**Applies to** — `architecture/rendering.md`.

## Make the simulation pipeline a first-class, visible product concept

**Decision** — The app exposes the pipeline `Particles → MAC grid → divergence →
pressure → velocity` through particle and liquid-cell inspection views, with concise
honest explanations.

**Why** — Viewers seeing the internal machinery is what differentiates this from an
ordinary shader demo.

**Applies to** — `architecture/rendering.md`.

## Typed scene config; static scenes before moving solids

**Decision** — Scenes are built from a small typed `SceneConfig` (not hardcoded into
the solver), with a scene-selector across the shipped static presets. Static scenes
and obstacles come before moving paddles / dynamic solids, which stay deferred.

**Why** — A typed scene object keeps scenario selection clean rather than hardcoding
scenes into the solver. Dynamic solids destabilize boundaries, projection, recovery,
and reset determinism, so they are a deliberate follow-up, not accidental scope creep.

**Code anchors** — `app/crates/fluid-lab/src/scene/mod.rs → SceneConfig`.

**Applies to** — `architecture/app-shell.md`, `architecture/settings.md`.

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

**Applies to** — `architecture/simulation.md`.

## Source/drain is future mass-mutation work

**Decision** — Source/drain stays out of the shipped interaction controls. When it
returns, it must create and destroy particles or water volume through an explicit
mass-accounting plan, not fake the effect with impulses or rendering.

**Why** — Source/drain touches particle allocation, recycling/deletion policy,
classification, reset/live semantics, and maybe boundary behavior. Those are different
risks from low-risk tank pose and particle-velocity impulse tools.

**Code anchors** — `crates/fluid-lab/src/settings/mod.rs → Registry` (no
source/drain setting ids); `crates/fluid-lab/src/lib.rs → InteractionState`
(interaction tools are scheduling/impulse only).

**Applies to** — `architecture/settings.md`, `architecture/simulation.md`,
`architecture/app-shell.md`.

## Optional features are deferred instead of exposed prematurely

**Decision** — Split view, guided tour, pressure-comparison UI, pouring spout,
source/drain, moving paddle, spray/bubbles, replay/scrub, and 128³ are optional.
None blocks a phase. Unstable or unfinished optional work is deferred or put behind
an explicitly experimental plan, not shipped as a product-visible kill switch.

**Why** — These features are valuable only after the stable inspectable core exists.
Treating them as required turns the project into a large graphics surface before the
fluid loop is trustworthy. A complete, measured, honest demo beats an unfinished
maximal one.

**Applies to** — `architecture/rendering.md`, `architecture/web-shell.md`,
`architecture/settings.md`.

## Deterministic replay/scrub is future work

**Decision** — Replay/scrub of the last few seconds across all render modes is a
high-value future idea, deferred until frame-state ownership, buffer layout, and render
modes are stable.

**Why** — Scrubbing the same moment across particles / pressure / velocity / liquid
cells would make the lab unusually inspectable, but it adds memory,
state-capture, and UI complexity that should not be paid before the core is stable.

**Applies to** — `architecture/rendering.md`.

## Config shareability stays portable before preset management

**Decision** — Shareable configuration is limited to portable, registry-backed
settings: repeatable `?set=id:value` params and JSON import/export over visible
non-default settings. Named preset management, cloud sync, account-level saved states,
and stable enum slugs remain deferred.

**Why** — The important correctness boundary is that all imported values use the same
typed validation, clamping, apply-class, and legacy-id policy as the live settings
panel. Named preset management would add another product surface before there is
evidence that users need saved preset libraries.

**Applies to** — `architecture/settings.md`, `architecture/web-shell.md`.

## Portfolio honesty

**Decision** — This is a portfolio-grade real-time fluid lab, not a validated
scientific CFD solver. Copy and framing state the method (FLIP/PIC, rectangular
grid, CG pressure) and known limits; simulation problems are not hidden behind fake polish. An
unsupported-WebGPU fallback is required before public presentation.

**Why** — Overstated accuracy or hidden problems undermine a portfolio piece more than
a modest, honest one.

**Applies to** — `architecture/web-shell.md`.

## See also

- [`platform.md`](platform.md) · [`rendering.md`](rendering.md) · [`performance.md`](performance.md)
- [`../plans/future-roadmap.md`](../plans/future-roadmap.md)
- [`../agent-context/maintaining-docs.md`](../agent-context/maintaining-docs.md)
