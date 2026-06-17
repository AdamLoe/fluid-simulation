---
status:        active
owner:         adamg
last_updated:  2026-06-15
---

# Decisions — Simulation

## Use a true 3D grid, never a 2D heightmap

**Decision** — The simulation grid is fully 3D (`nx × ny × nz` cells). The project
does not flatten water to a 2D heightfield and reconstruct fake 3D.

**Why** — A heightmap cannot represent overturning waves, falling water, splashes,
droplets, stacked water, or flow around vertical obstacles. Volumetric credibility
is the whole point of the project.

**Tradeoffs** — A 3D grid is far more expensive than a heightfield, so practical
browser volumes remain bounded by the GPU budget.

**Code anchors** — `app/crates/fluid-lab/src/sim/mod.rs → GridDims`;
`app/crates/fluid-lab/src/gpu/fluid.rs → Params`.

**Applies to** — `architecture/simulation.md`.

## Hybrid particle-grid (FLIP/PIC) method

**Decision** — The solver is a hybrid particle-grid method: particles carry mass and
free-surface motion; a staggered MAC grid solves pressure/incompressibility.

**Why** — Particles track *where* the water is and keep splashy free-surface motion;
a grid is where pressure projection, incompressibility, and collisions are tractable.
The two representations also visualize separately, which makes the sim debuggable.

**Tradeoffs** — Particle↔grid transfers are the subtle part and must be implemented
carefully (see the P2G decision below).

**Applies to** — `architecture/simulation.md`.

## Staggered MAC grid as the grid model

**Decision** — Velocity components `u,v,w` live on cell **faces**; pressure,
divergence, and cell type (`solid`/`liquid`/`air`) live at cell **centers**.

**Why** — Face velocities make divergence and pressure-gradient operators clean and
avoid the checkerboard artifacts of a collocated grid. It is the standard practical
representation for incompressible grid fluids.

**Tradeoffs** — Slightly more complex staggered indexing and more buffers, in
exchange for a much better fit to pressure projection.

**Applies to** — `architecture/simulation.md`.

## Rectangular tanks use per-axis counts with one uniform cell size

**Decision** — Tank shape is controlled by independent grid counts
`grid.res_x/y/z`, while the simulation keeps one scalar cell size `h` and one
isotropic pressure operator.

**Why** — Per-axis counts let the tank become wider, taller, or deeper without
introducing anisotropic finite differences into pressure projection. The default
`80×40×80` profile uses that rectangular shape while keeping the pressure operator
isotropic.

**Tradeoffs** — Changing an axis changes world extent as well as work size. A future
non-uniform-cell tank would be a pressure-solver decision, not a settings-only
change.

**Code anchors** — `app/crates/fluid-lab/src/settings/mod.rs → grid_res_x`;
`app/crates/fluid-lab/src/settings/mod.rs → grid_res_y`;
`app/crates/fluid-lab/src/settings/mod.rs → grid_res_z`;
`app/crates/fluid-lab/src/sim/mod.rs → H`;
`app/crates/fluid-lab/src/gpu/shaders/cg_spmv.wgsl`.

**Applies to** — `architecture/simulation.md`, `architecture/pressure-solver.md`.

## GPU P2G is fixed-point integer-atomic accumulation (forced, and deterministic)

**Decision** — Particle-to-grid transfer accumulates weighted velocity and weight
into per-face **integer** buffers via `i32 atomicAdd` at a fixed-point scale
(`FIXED_SCALE = 2^16`), then normalizes in a separate float pass. The entire
accumulate→normalize path stays integer.

**Why** — Two hard WebGPU facts, not choices: (1) **there are no float atomics in
WGSL** (only `i32`/`u32`), so fixed-point integer accumulation is *forced* — do not
spend effort evaluating float-atomic feasibility; (2) **integer `atomicAdd` is
associative and commutative**, so the result is order-independent and
bit-deterministic regardless of GPU scheduling. That determinism is the *only*
reason deterministic reset/recovery (and any future replay) are achievable.

**Invariant** — Introducing any float reduction or float-atomic-style step into the
accumulate→normalize path silently breaks run-to-run determinism. That is a
**contract change** and must be recorded, not done casually.

**Alternatives considered** — Naive many-particles-write-one-cell scatter (races,
nondeterminism); normal-frame CPU transfer or hidden per-frame readback (rejected,
see `decisions/rendering.md` no-readback rule). Documented fallbacks if the default
proves unworkable: per-cell buckets, particle binning/sorting, gather-based transfer.

**Code anchors** — `app/crates/fluid-lab/src/gpu/shaders/scatter.wgsl`, `app/crates/fluid-lab/src/gpu/shaders/normalize.wgsl`,
`app/crates/fluid-lab/src/gpu/shaders/scatter_local.wgsl`,
`app/crates/fluid-lab/src/gpu/fluid.rs → FIXED_SCALE`.

**Applies to** — `architecture/simulation.md`.

## Fixed/clamped physics timestep with substeps

**Decision** — Physics runs on a fixed `dt = 1/120 s`; the browser render `dt` is
clamped to ≤ `1/30 s` and fed to an accumulator that emits fixed steps
(`physics.max_substeps` defaults to 2, dropped time tracked). Raw
`requestAnimationFrame` delta is never consumed by advection.

**Why** — Advection and projection go unstable when a tab hitch, GC pause, or shader
compile produces a huge `dt`. A fixed step also makes profiling and reproduction
honest.

**Tradeoffs** — A little frame-loop machinery in exchange for stability.

**Applies to** — `architecture/app-shell.md` (the accumulator), `architecture/simulation.md`.

## CFL velocity cap is a tunable cells-per-step number, not a hard `h/dt`

**Decision** — The advection velocity clamp is `cfl · h/dt` with `cfl` a Live setting,
rather than the bare `h/dt` ceiling.

**Why** — `h/dt` ties the max speed to grid resolution: refining the grid (smaller
`h`) silently lowers the speed ceiling. Decoupling it with a CFL number restores and
exposes the splash ceiling. A few cells per step is safe here because the wall-contact
clamp in `g2p.wgsl` already prevents particles leaving the tank, and RK1 advection
tolerates it visually.

**Tradeoffs** — A high `cfl` × fine grid raises peak speeds, eating into the i32 P2G
headroom (see the fixed-point decision above) and admitting more advection error;
the shipped default stays within both.

**Code anchors** — `app/crates/fluid-lab/src/gpu/shaders/g2p.wgsl` (the clamp);
`app/crates/fluid-lab/src/gpu/fluid.rs → set_cfl`;
`app/crates/fluid-lab/src/settings/mod.rs → cfl`.

**Applies to** — `architecture/simulation.md`.

## Wall-aware G2P sampling gives free slip without opening the tank

**Decision** — G2P interpolation excludes static domain-edge / Solid-boundary face
stencils and renormalizes the remaining MAC weights for both final and saved velocity
samples. Boundary enforcement still zeroes those faces before pressure and after the
gradient, and particle recovery still clamps escaped particles inside and zeroes the
crossed wall-normal velocity.

**Why** — Interpolating boundary-zeroed MAC faces into near-wall particles acts like
hidden wall drag even when `physics.wall_friction = 0`. Applying the same wall-aware
gather to saved and final velocities keeps FLIP deltas consistent while preserving
free-slip contact.

**Tradeoffs** — This assumes Solid cells are the static tank boundary, not arbitrary
moving obstacles. A future obstacle system must either bind cell type into G2P or
provide an equivalent obstacle-aware sampling mask. It is intentionally not bounce,
negative-pressure clamping, or a floor/ceiling special case.

**Code anchors** — `app/crates/fluid-lab/src/gpu/shaders/g2p.wgsl`;
`app/crates/fluid-lab/src/sim/mod.rs`.

**Applies to** — `architecture/simulation.md`, `architecture/settings.md`.

## Interaction tools stay app-side pose/impulse tools

**Decision** — Automatic tank roll is app-side tank-pose scheduling, and wave making
is periodic particle velocity impulses through the existing impulse pass. Neither tool
changes pressure-solver topology, cell classification, or particle allocation.

**Why** — Tank pose and one-shot impulses compose with the current closed-tank FLIP/PIC
contract and make the sim more lively without adding a new mass/source/drain model or
moving-solid boundary semantics.

**Tradeoffs** — The wave maker is not a physical paddle and auto-roll is bounded target
motion rather than a scripted "best physics" choreography. More realistic machinery can
be planned later if it earns the solver risk.

**Code anchors** — `app/crates/fluid-lab/src/lib.rs → InteractionState`;
`app/crates/fluid-lab/src/gpu/fluid.rs → apply_impulse`.

**Applies to** — `architecture/app-shell.md`, `architecture/simulation.md`,
`architecture/settings.md`.

## Anti-clump rest target tracks particle density (density is motion-neutral)

**Decision** — The anti-clump divergence source's rest target tracks the scene's
requested effective particle density by default, instead of a frozen constant.
`physics.rest_density` is an optional manual override (`0` = Auto; nonzero pins a
fixed target). The effective `rest` fed to `divergence.wgsl`'s `spc[0]` is
`manual > 0 ? manual : requested_particles / seeded_cells`.

**Why** — The divergence anti-clump source compares raw per-cell particle count with
a rest target, and raw occupancy scales with `particles.density`. With a frozen rest
target, changing density also changed the pressure bias and made the water look like
a different *volume in motion*. Coupling `rest` to requested effective density keeps
density changes closer to fidelity/cost changes while preserving the tuned reference
look at density `8`; generated-count drift is diagnostic rather than a render/runtime
calibration input.

**Tradeoffs** — Power users can still pin a target via the override. This remains a
pragmatic liveness/compactness correction, not physical mass conservation.

**Code anchors** — `app/crates/fluid-lab/src/scene/mod.rs → effective_particle_density`;
`app/crates/fluid-lab/src/scene/mod.rs → effective_rest_density`;
`app/crates/fluid-lab/src/gpu/fluid.rs → effective_rest_density`;
`app/crates/fluid-lab/src/gpu/shaders/divergence.wgsl`;
`app/crates/fluid-lab/src/settings/mod.rs → rest_density`.

**Applies to** — `architecture/simulation.md`, `architecture/settings.md`.

## Keep the current occupancy-bias defaults until a stronger volume metric exists

**Decision** — Keep the current `physics.volume_stiffness` and
`physics.drift_clamp` defaults as the volume-correction surface. Do not add PBF,
particle spawning/deletion, source/drain behavior, or a new divergence formula from
the current occupied-cell proxy alone.

**Why** — Capture sweeps show the existing one-sided, pressure-coupled occupancy
bias is useful, but the occupied-cell proxy is not strong enough to justify a new
formula by itself. Stronger candidates reduced drift mainly by inflating occupied
cell counts.

**Tradeoffs** — The default is a pragmatic liveness/compactness correction, not
physical mass conservation. Future volume work needs either a better physical-volume
metric, a visual pulsing gate, or a separately scoped formula change with captures.

**Code anchors** — `app/crates/fluid-lab/src/settings/mod.rs → volume_stiffness`;
`app/crates/fluid-lab/src/settings/mod.rs → drift_clamp`;
`app/crates/fluid-lab/src/gpu/shaders/divergence.wgsl`;
`app/tools/density_motion_sweep.mjs`.

**Applies to** — `architecture/simulation.md`, `architecture/pressure-solver.md`,
`decisions/performance.md`.

## See also

- [`../architecture/simulation.md`](../architecture/simulation.md)
- [`pressure.md`](pressure.md) · [`performance.md`](performance.md) · [`scope.md`](scope.md)
- [`../agent-context/maintaining-docs.md`](../agent-context/maintaining-docs.md)
