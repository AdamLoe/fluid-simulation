---
status:        active
owner:         adamg
last_updated:  2026-06-08
okay_to_delete: false
long_lived:    true
---

# Simulation — Hybrid FLIP/PIC MAC-Grid Fluid

The fluid sim is a hybrid particle-grid (FLIP/PIC) solver on a staggered MAC grid enclosed in a rectangular world box. Particles carry velocity; the grid enforces incompressibility each substep via pressure projection. Everything below (except the pressure solve itself) runs as a sequence of GPU compute passes recorded in `app/crates/fluid-lab/src/gpu/fluid.rs → GpuFluid`.

The tank is rectangular: a single **uniform** scalar cell size `h = sim::H = 2.0/64.0` with independent per-axis cell counts `nx/ny/nz` (the `grid.res_x/y/z` Reset-class settings). The domain is centered (`origin = -n_axis*h/2`, `extent = n_axis*h` per axis); an all-64 grid reproduces the historical `[-1,1]³` cube. Because `h` stays uniform, the pressure operator is isotropic — there is no per-axis `hx/hy/hz` (see "invariants" below). World placement is owned by `app-shell.md`.

The host reference for grid math and indexing lives in `app/crates/fluid-lab/src/sim/mod.rs`; the WGSL port mirrors it. The pressure solve is owned by `pressure-solver.md`.

---

## What it owns

- **Grid/indexing contract** — `app/crates/fluid-lab/src/sim/mod.rs → GridDims`, `cell_idx`, `u_idx`, `v_idx`, `w_idx`, `world_to_cell`, `classify_cells`, `mark_occupancy_from_particles`
- **GPU state (buffers + pipelines + bind groups)** — `app/crates/fluid-lab/src/gpu/fluid.rs → GpuFluid`; particle buffer (`{pos:vec4, vel:vec4}`, 32 B/particle), per-face integer P2G buffers (`u/v/w_num`, `u/v/w_den`), float velocity buffers (`u/v/w_vel`), pre-force snapshot (`u/v/w_saved`), occupancy, cell-type, divergence, pressure double-buffer — all in `app/crates/fluid-lab/src/gpu/fluid.rs → GpuFluid`
- **Uniform params** — `app/crates/fluid-lab/src/gpu/fluid.rs → Params` (dims, geom, phys, origin, grav, spc, cls, gdim — 8 `vec4` = 128 B); written to `params_buf`; updated live via `set_flip_blend`, `set_gravity_vec`, `set_wall_friction`, etc. The per-axis cell counts `[nx,ny,nz,0]` live in the **appended** `gdim` field (see "per-axis indexing" below).
- **Per-substep GPU passes** — `record_prep`, `record_pressure`, `record_finish`
- **WGSL shaders** (in `app/crates/fluid-lab/src/gpu/shaders/`): `clear.wgsl`, `mark.wgsl`, `classify.wgsl`, `scatter.wgsl`, `normalize.wgsl`, `save_vel.wgsl`, `forces.wgsl`, `boundaries.wgsl`, `divergence.wgsl`, `gradient.wgsl`, `g2p.wgsl`, `impulse.wgsl`; pressure and CG kernels owned by `pressure-solver.md`
- **Deterministic particle init** — `app/crates/fluid-lab/src/gpu/fluid.rs → generate_particles` (lattice + seeded LCG jitter; volume-proportional split across scene blocks)
- **Escaped-particle recovery and wall contact** — wall-aware MAC sampling, clamp + zero-normal recovery, and optional tangential damping in `app/crates/fluid-lab/src/gpu/shaders/g2p.wgsl`
- **Volume / compactness knobs** — `particles.count` and scene seeding define initial mass/distribution; `physics.rest_density`, `physics.volume_stiffness`, and `physics.drift_clamp` add occupancy-driven divergence bias for anti-clump volume correction; `classify.liquid_threshold` and `classify.surface_dilation` choose the liquid-cell active set; `render.particle_size` is visual only
- **Interaction impulses** — manual slosh and scheduled wave-maker impulses (`app/crates/fluid-lab/src/gpu/shaders/impulse.wgsl`, triggered by `app/crates/fluid-lab/src/gpu/fluid.rs → apply_impulse`)

---

## The physics step

One substep = `record_prep` → `record_pressure` → `record_finish`, in that order, all on a single `wgpu::ComputePass`:

```
CLEAR (u/v/w_num, u/v/w_den, occupancy, pressure, stats)
  └─ clear.wgsl

MARK / CLASSIFY
  ├─ mark.wgsl          → atomicAdd occupancy counts for classification and volume correction
  └─ classify.wgsl      → boundary→Solid, occupied-interior→Liquid, else→Air

P2G SCATTER  [×3 axes]
  └─ scatter.wgsl       → i32 atomicAdd into u/v/w_num + u/v/w_den (FIXED_SCALE 2^16)

P2G NORMALIZE  [×3 axes]
  └─ normalize.wgsl     → float u/v/w_vel = num/den (den==0 → vel=0, face invalid)

SAVE PRE-FORCE VELOCITY  [×3 axes]
  └─ save_vel.wgsl      → snapshot u/v/w_vel → u/v/w_saved  (FLIP delta baseline)

BODY FORCES  [×3 axes]
  └─ forces.wgsl        → u/v/w_vel += gravity_component·dt on liquid-adjacent faces

ENFORCE BOUNDARIES  [×3 axes]  ← first enforce
  └─ boundaries.wgsl   → zero solid-adjacent / domain-edge faces

─────── record_pressure (see pressure-solver.md) ────────────────────────────
  divergence.wgsl  →  [CG solve]  →  final pressure in pressure_a
──────────────────────────────────────────────────────────────────────────────

SUBTRACT GRADIENT  [×3 axes]
  └─ gradient.wgsl      → u/v/w_vel -= (dt/rho)·(p_hi - p_lo)/h  (non-solid faces)

ENFORCE BOUNDARIES  [×3 axes]  ← second enforce (post-projection)
  └─ boundaries.wgsl

G2P + ADVECT + RECOVER
  └─ g2p.wgsl           → wall-aware trilinear interpolation, PIC/FLIP blend, RK1 advect,
                           escaped-particle clamp + zero-normal-velocity recovery,
                           wall-friction tangential damping
```

---

## Non-obvious invariants and gotchas

**Per-axis indexing drives every kernel.** The host contract (`app/crates/fluid-lab/src/sim/mod.rs → GridDims`, `cell_idx`/`u_idx`/`v_idx`/`w_idx`, `world_to_cell`, `cell_center_world`) is fully per-axis: cell index `i + nx*(j + ny*k)`, staggered face counts `(nx+1)·ny·nz` / `nx·(ny+1)·nz` / `nx·ny·(nz+1)`, scalar `h`. The WGSL port mirrors it: any shader that decomposes a linear cell index uses `i = c%nx; j = (c/nx)%ny; k = c/(nx*ny)` and the per-axis staggered face dims, reading `nx/ny/nz` from `params.gdim` (total cells `nx*ny*nz`). The sim/CG kernels that decompose indices all carry the `gdim` mirror; the scalar kernels (e.g. `clear`, `normalize`, `save_vel`) keep a shorter prefix `Params`. To find the set, `grep params.gdim` over `app/crates/fluid-lab/src/gpu/shaders/`.

**Particle-linear passes use one tiled particle index contract.** Mark, scatter U/V/W,
G2P, and the standalone impulse pass all compute particle index from 2D workgroup
coordinates plus `local_invocation_index`, not from `global_invocation_id.x` alone.
The contract is shared with `gpu/fluid.rs → particle_dispatch_shape`: same
`@workgroup_size(64, 1, 1)`, same row-major workgroup flattening, same
`if (p >= params.dims.y) { return; }` guard before touching the particle buffer. This
raises the legal particle ceiling without changing P2G/G2P physics.

The transfer kernels also use the precomputed inverse cell size already stored in
`params.geom.y`. `scatter.wgsl` converts particle world coordinates to grid space with
`* inv_h`, and `g2p.wgsl` uses the same `inv_h` term in all three MAC-face samplers,
rather than dividing by `h` per particle. The v1.9 transfer-path optimization changed
that arithmetic only; it did not change fixed-point scatter atomics, wall-aware G2P
sampling, CFL clamp, RK1 advection, or dispatch shape.

**`gdim` is appended last in `Params`.** It sits at the END of the struct so shaders that don't decompose cell indices can keep their existing (shorter prefix) `Params` mirror without re-layout. `Params` is now 8 `vec4` = 128 B. A shader that needs per-axis dims must include all eight `vec4` fields up to and including `gdim`.

**The pressure operator stays ISOTROPIC.** Because `h` is a single uniform scalar, there is NO per-axis `hx/hy/hz`. The CG SpMV (`app/crates/fluid-lab/src/gpu/shaders/cg_spmv.wgsl`) is the symmetric graph-Laplacian `(A d)_c = n_c·d_c − Σ_{liquid nb} d_nb` (the `1/h²` factor folds out uniformly); divergence/gradient use the single `params.geom` `inv_h`. Introducing a non-uniform cell size would make the operator anisotropic and is a contract change.

**P2G determinism is the load-bearing invariant.** The entire accumulate→normalize path (`scatter.wgsl` → `normalize.wgsl`) must stay in integer/fixed-point. WebGPU has no float atomics; `atomicAdd` on `i32`/`u32` is forced. Integer addition is associative and commutative: P2G results are bit-identical regardless of GPU thread scheduling, making reset and recovery deterministic. Any switch to float accumulation (e.g. "for convenience") breaks run-to-run determinism and invalidates every determinism claim. This is a contract change — record it in `../decisions/simulation.md`.

**CFL clamp is `cfl·h/dt`, not `h/dt`.** `g2p.wgsl` caps particle speed at `params.cls.z · h / dt`, where `cls.z` is the `physics.cfl` setting (the max grid cells a particle may cross per substep, default 2). The `h/dt` ceiling alone scales **down** as the grid is refined (finer `h` → lower max speed → shallower splash), so a fixed CFL > 1 decouples the achievable splash height from grid resolution; `cfl=2` at the 64³ default reproduces the old 32³ `h/dt` ceiling (~7.5 u/s). The wall-contact clamp in `g2p.wgsl` still prevents particles escaping the tank, so a few cells/step is safe. Live via `app/crates/fluid-lab/src/gpu/fluid.rs → set_cfl` (writes `params.cls[2]`). Rationale in `../decisions/simulation.md`.

**FIXED_SCALE = 2^16.** Declared as `app/crates/fluid-lab/src/gpu/fluid.rs → FIXED_SCALE` (`65536.0`), passed into shaders via `params.geom.w`. At the default CFL the velocity cap gives comfortable i32 headroom (~3× safety on `num` sums at 8 particles/cell). If a future preset saturates i32 (e.g. a high CFL × fine grid), lower FIXED_SCALE to 2^12 (the overflow headroom rationale lives in `../decisions/simulation.md`).

**One-cell solid walls → no bounds checks in divergence/pressure.** Every boundary cell is unconditionally Solid — boundary means any axis index at `0` or `n_axis-1` (`app/crates/fluid-lab/src/sim/mod.rs → is_boundary_cell`, `classify_cells`). This holds unchanged on the rectangular grid. Every Liquid cell is therefore interior, so its 6 stencil neighbors are always in-range. `divergence.wgsl` and the CG SpMV (`app/crates/fluid-lab/src/gpu/shaders/cg_spmv.wgsl`) exploit this: they iterate over all 6 neighbors without range guards.

**Cell typing is reset every substep.** Solid walls are re-stamped, interior cells are re-classified from the fresh occupancy bitmap. There is no persistent cell-type state between substeps — stale type from the previous frame must never be assumed.

**FLIP blend default is high-FLIP (~0.9) for lively motion.** The `physics.flip_blend` registry value drives `params.phys[2]`. Pure PIC (blend=0) is maximally dissipative; high FLIP (≈0.9) preserves velocity variance and produces visible splash/wave. Updated live via `app/crates/fluid-lab/src/gpu/fluid.rs → set_flip_blend`.

**FLIP delta is taken from post-P2G / pre-force velocity.** `save_vel.wgsl` snapshots `u/v/w_vel` immediately after normalize, before `forces.wgsl` applies gravity. The FLIP delta in `g2p.wgsl` is `v_grid_now − v_saved`, so it includes the effect of pressure projection and boundary enforcement but not the gravity step directly (gravity was applied to the grid, projected out, and shows up implicitly in the post-projection velocity).

**G2P samples skip static wall-zeroed face stencils.** Boundary enforcement zeros every domain-edge face and every face touching a Solid boundary cell, preserving the closed-tank no-normal-flow model. A naive G2P gather would interpolate those zeros into near-wall particles, creating implicit wall drag even when `physics.wall_friction = 0` (about 55% retained tangential velocity one epsilon from a wall, and about 5% retained away-from-ceiling normal velocity in the dam-break repro). `g2p.wgsl` therefore excludes static boundary/domain-edge face stencils from both final and saved MAC gathers and renormalizes the remaining weights. This is free-slip sampling, not open-boundary behavior: inward wall-normal motion is still clamped and zeroed by recovery, and boundary faces are still zeroed before divergence/projection and after gradient subtraction.

**Gravity is a 3-axis vector, not a scalar.** `params.grav = [gx, gy, gz, _]`.
Gravity updates via `app/crates/fluid-lab/src/gpu/fluid.rs → set_gravity_vec` preserve
the unused `.w` slot. Rotating the tank changes the gravity direction in the sim's
local frame.

**naga auto-layout drops unused bindings.** Each WGSL compute shader must either reference `params` (binding 0) in executed code, or the pipeline must be compiled with an explicit `BindGroupLayout`. The RBGS red/black pair (`app/crates/fluid-lab/src/gpu/shaders/pressure.wgsl`, compiled with `app/crates/fluid-lab/src/gpu/fluid.rs → compute_with_layout`) shares an explicit BGL so a single `rbgs_bg` bind group is accepted by both pipelines — using auto-layout produces distinct layout objects and the bind group is rejected at dispatch.

**Escaped-particle recovery is deterministic and non-bouncing.** `g2p.wgsl` clamps position to one epsilon inside the walls and zeroes the velocity component normal to the crossed wall. No random perturbation, no restitution.

**Compactness is split across physics and rendering.** `render.particle_size` only changes how large particles look. `particles.count` plus the selected scene preset control the initial seeded mass/distribution and require Reset. `physics.rest_density`, `physics.volume_stiffness`, and `physics.drift_clamp` are the volume-correction trio: sufficiently occupied liquid cells above the rest particles/cell target receive a clamped negative divergence bias so projection pushes crowded regions outward. `classify.liquid_threshold` and `classify.surface_dilation` decide which occupied cells participate as liquid; the default dilation is 0 so thin cells are not automatically expanded. `solver.pressure_iterations` controls incompressibility quality/perf, not visual particle overlap.

**Pressure solve ceiling (~19.2k liquid cells at 64³) is FLIP volume loss, not solver deficit.** Both CG-30 and brute-force Jacobi-400 plateau at the same occupied-cell count. See `pressure-solver.md` and `../decisions/pressure.md`.

**`apply_impulse` submits its own command encoder.** The slosh and wave-maker impulses (`app/crates/fluid-lab/src/gpu/fluid.rs → apply_impulse`) are one-shot dispatches that run outside the main substep command buffer, writing directly to the particle buffer before the next `record_prep` clear.

**Wave maker is a velocity impulse, not a source/drain.** Scheduled waves add local horizontal particle velocity only. They do not allocate particles, delete particles, mutate fluid volume, open boundaries, or add a physical paddle. Source/drain needs a separate particle/mass-accounting plan.

---

## Update when

- Grid indexing formula or face-count formula changes (`app/crates/fluid-lab/src/sim/mod.rs → GridDims`)
- The `Params` layout changes (a field is appended/reordered, or its `vec4` count / 128 B size changes) — every decomposing shader's mirror must match
- The cell size stops being a single uniform scalar `h` (per-axis `hx/hy/hz`), which would make the pressure operator anisotropic
- A new buffer is added to `GpuFluid` or a buffer is repurposed
- The P2G kernel changes representation (float accumulation, different scale, gather instead of scatter)
- The FLIP blend formula or the pre-force snapshot point moves
- Wall-friction, wall-aware G2P sampling, or volume-correction semantics change
- Advection order upgrades from RK1 to RK2
- Particle init layout changes (block config, jitter seed, spacing rule)
- The step sequence order changes (e.g. forces before mark, or second enforce removed)
- The impulse path changes from a particle velocity kick into a mass/source/drain or wall-paddle model
- Particle-linear indexing stops matching the shared tiled dispatch contract

---

## See also

- `pressure-solver.md` — owns the pressure solve (divergence RHS → CG → pressure_a)
- `gpu-resources.md` — buffer layout and sizing details
- `app-shell.md` — frame loop, accumulator, fixed-dt substep dispatch, timestep policy
- `../decisions/simulation.md` — durable rationale for FLIP/PIC, fixed-point P2G, CG solver
- `../agent-context/maintaining-docs.md`
