---
status:        active
owner:         adamg
last_updated:  2026-06-17
okay_to_delete: false
long_lived:    true
---

# Simulation — Hybrid FLIP/PIC MAC-Grid Fluid

The fluid sim is a hybrid particle-grid solver inside a rectangular closed tank.
Particles carry moving liquid mass and velocity detail; a staggered MAC grid owns
cell typing, face velocities, divergence, and pressure projection. The frame loop
and tank pose are owned by `app-shell.md`; the pressure solve is owned by
`pressure-solver.md`; renderer choices are owned by `rendering.md`.

The normal simulation path is GPU-native. `app/crates/fluid-lab/src/gpu/mod.rs →
GpuContext::step` records each substep around `app/crates/fluid-lab/src/gpu/fluid.rs →
GpuFluid`: clear/mark, optional spatial sort, classify, particle-to-grid transfer,
body forces, boundary enforcement, pressure projection, gradient subtraction, a
second boundary enforce, then grid-to-particle transfer and advection.

The tank is rectangular, not a fixed cube. `app/crates/fluid-lab/src/sim/mod.rs →
GridDims` carries independent `nx/ny/nz` cell counts from the Reset-class
`grid.res_x/y/z` settings, while `sim::H` remains the single uniform cell size
(`2.0/64.0`). The default counts are `80×40×80`, which make the tank wider/deeper
and shorter than the original all-64 cube without changing the pressure operator.
`app/crates/fluid-lab/src/gpu/fluid.rs → Params` appends `gdim = [nx, ny, nz, 0]`
so shaders that decompose indices can use per-axis dimensions without changing the
shorter prefix mirrors used by scalar kernels. Because `h` is scalar, the pressure
operator remains isotropic; any per-axis `hx/hy/hz` change belongs in
`pressure-solver.md` and `../decisions/pressure.md` too.

---

## What It Owns

- **Grid/indexing contract** — `app/crates/fluid-lab/src/sim/mod.rs → GridDims`,
  `cell_idx`, `u_idx`, `v_idx`, `w_idx`, `world_to_cell`, `cell_center_world`,
  `classify_cells`, `mark_occupancy_from_particles`, `is_boundary_cell`
- **GPU simulation state and pass helpers** —
  `app/crates/fluid-lab/src/gpu/fluid.rs → GpuFluid`,
  `record_prep_pre_sort`, `record_sort`, `record_prep_post_sort`,
  `record_pressure`, `record_finish`, and the `dispatch_*` helpers
- **Simulation params** — `app/crates/fluid-lab/src/gpu/fluid.rs → Params`,
  `FIXED_SCALE`, and the live setters that rewrite the uniform buffer
- **Particle seeding and scene-derived density** —
  `app/crates/fluid-lab/src/gpu/fluid.rs → generate_particles`;
  `app/crates/fluid-lab/src/scene/mod.rs → preset_blocks`,
  `effective_particle_density`, `effective_particle_density_for_count`,
  `effective_surface_dilation`, `effective_rest_density`
- **Simulation WGSL kernels** — `app/crates/fluid-lab/src/gpu/shaders/{clear,mark,classify,scatter,scatter_local,normalize,save_vel,forces,boundaries,divergence,gradient,g2p,impulse}.wgsl`;
  CG/pressure kernels are owned by `pressure-solver.md`
- **Interaction impulses** — `app/crates/fluid-lab/src/gpu/fluid.rs →
  apply_impulse`, used by manual slosh and scheduled wave-maker controls

---

## Invariants And Gotchas

**P2G determinism is load-bearing.** `scatter.wgsl` and the sorted-path
`scatter_local.wgsl` accumulate velocity numerators and weights into integer
buffers with fixed-point `i32 atomicAdd`; `normalize.wgsl` is the first float
conversion. `FIXED_SCALE` is declared in `GpuFluid` code and passed through
`Params.geom.w`. Switching the accumulate path to a float reduction would make
particle-to-grid transfer order-dependent and is a simulation contract change.

**The sorted path preserves the same transfer contract.** `dev.particle_sort`
enables a deterministic GPU spatial sort before P2G; `record_sort` owns the
pass-boundary barriers needed by the prefix-sum stages. `dispatch_scatter` selects
`scatter_local.wgsl` when sorting is enabled and the plain `scatter.wgsl` otherwise.
Both paths stay integer/fixed-point through accumulation and flush.

**Particle-linear kernels share a tiled dispatch contract.** Mark, scatter,
scatter-local, G2P, and impulse use the shape from
`app/crates/fluid-lab/src/gpu/fluid.rs → particle_dispatch_shape` rather than a
single `global_invocation_id.x` ceiling. Shaders keep the same workgroup size,
row-major workgroup flattening, and particle-count guard.

**Index decomposition is per-axis.** Host indexing is `GridDims`; WGSL kernels that
need cell coordinates read `params.gdim` and decompose linear cell indices with
`nx/ny/nz`. Scalar kernels such as normalize and save-velocity do not need `gdim` and
keep the shorter `Params` prefix. `rg "params.gdim" app/crates/fluid-lab/src/gpu/shaders`
is the fastest way to find the shader-side mirrors.

**Boundary cells are always Solid.** `GridDims::is_boundary_cell` and
`classify_cells` stamp every outer cell Solid each substep. Liquid cells are
therefore interior, and divergence / CG SpMV can use the six-neighbor stencil without
range guards.

**The pressure operator stays isotropic.** `app/crates/fluid-lab/src/gpu/shaders/cg_spmv.wgsl`
uses the graph Laplacian over liquid neighbors, while divergence and gradient use the
single inverse cell size from `Params.geom`. Rectangular tanks are created by changing
cell counts, not by introducing non-uniform cell sizes.

**FLIP deltas start from the post-P2G, pre-force grid.** `save_vel.wgsl` snapshots
face velocities immediately after normalize. The delta sampled in `g2p.wgsl` includes
projection and boundary effects but not a direct copy of the body-force step.

**G2P is wall-aware but the tank stays closed.** `g2p.wgsl` excludes static
domain-edge / Solid-boundary face stencils from final and saved MAC gathers and
renormalizes the remaining weights. Boundary passes still zero closed-wall faces, and
escaped-particle recovery clamps particles inside and zeroes the crossed normal
velocity component.

**Gravity is a vector.** `Params.grav` carries local-frame `gx/gy/gz`; tank rotation
changes the gravity direction through `app/crates/fluid-lab/src/gpu/fluid.rs →
set_gravity_vec`.

**naga unused-binding behavior matters.** Compute shaders that declare the shared
`Params` binding must reference it, or Rust must provide an explicit bind group
layout. The RBGS red/black pressure pair uses
`app/crates/fluid-lab/src/gpu/fluid.rs → compute_with_layout` so one bind group is
valid for both pipelines.

---

## Volume And Density

`scene.fill_level` is the Reset-class scenario amount percentage. Runtime maps the
stored `0..100` value to a `[0,1]` amount and each preset applies that amount through
its authored geometry rather than a universal whole-tank volume. Falling Blob grows
as a suspended blob around the tuned 20% default, Dam Break raises the waterline
inside its historical wall footprint, and Double Splash stretches two suspended
drops. This keeps the visible compositions close to the tuned presets while making
the amount monotone and density-invariant. A 0% fill keeps a tiny compatibility seed
rather than an entirely empty particle dispatch.

`particles.density` is a fidelity/cost knob. It derives the requested seed target as
particles per represented seeded cell and does not change the target water volume.
`particles.count` remains a hidden absolute compatibility override where `0` means
Auto. The deterministic lattice can generate slightly fewer particles than requested;
runtime rest density, auto surface dilation, and render splat spacing use the
requested effective density so density `8` remains the tuned visual baseline.
Diagnostics still expose requested/generated drift. The current scene block geometry is in
`app/crates/fluid-lab/src/scene/mod.rs → preset_blocks`; host tests in the same file
cover preset-authored fill scale, density-invariant geometry, and generated-count
measurement.

Low-density cells can leave holes in the pressure active set. The effective surface
dilation is resolved by `app/crates/fluid-lab/src/scene/mod.rs →
effective_surface_dilation` and wired into `Params.cls` by
`app/crates/fluid-lab/src/gpu/fluid.rs → effective_surface_dilation`; the user
setting can force dilation, and the auto path turns it on below the reference
density.

The occupancy-driven volume correction is the trio
`physics.rest_density`, `physics.volume_stiffness`, and `physics.drift_clamp`.
`app/crates/fluid-lab/src/scene/mod.rs → effective_rest_density` makes the Auto rest
target track requested effective particle density; `app/crates/fluid-lab/src/gpu/fluid.rs →
effective_rest_density` covers reset/live writes to `Params.spc`. `divergence.wgsl` applies the
clamped occupancy bias before projection. This is a liveness/compactness correction,
not physical mass conservation.

`stats_json` exposes `filled_volume` and `liquid_fraction` from the throttled liquid
cell counter (`app/crates/fluid-lab/src/profiler/mod.rs → stats_json`).
Those values are useful capture proxies for gross drift and density/fill calibration,
but they are not exact fluid mass.

`apply_impulse` submits its own command encoder and writes particle velocity before
the next substep. Wave-maker impulses do not allocate particles, delete particles,
open boundaries, or act as a physical paddle.

---

## Update When

- Grid indexing or face sizing changes (`app/crates/fluid-lab/src/sim/mod.rs →
  GridDims`)
- `Params` layout changes, especially the appended `gdim` field
- Cell size stops being one uniform scalar `h`
- `GpuFluid` buffers, pass order, or record/dispatch helper boundaries change
- P2G changes representation, scale, or scatter/gather strategy
- FLIP blend, saved-velocity timing, wall-aware G2P, wall friction, or recovery
  semantics change
- Fill-level, particle-density, surface-dilation, or volume-correction semantics
  change
- Impulses become mass/source/drain behavior or moving-wall paddle behavior
- Particle-linear dispatch stops using the shared tiled contract

---

## See Also

- `pressure-solver.md` — pressure projection and CG details
- `gpu-resources.md` — buffer layout and sizing details
- `app-shell.md` — frame loop, accumulator, fixed-dt substep dispatch, timestep policy
- `rendering.md` — water, particle, tank, and slice rendering
- `../decisions/simulation.md` — simulation rationale
- `../agent-context/maintaining-docs.md` — doc maintenance rules
