---
status:        active
owner:         adamg
last_updated:  2026-06-15
okay_to_delete: false
long_lived:    true
---

# Pressure solver

The pressure solver enforces incompressibility at every fluid step by computing a
pressure field that, when its gradient is subtracted from the velocity grid, drives
divergence to zero. It is isolated behind a replaceable kernel interface so the
algorithm can be swapped without touching the surrounding sim loop.

## What it owns

- The MAC-Poisson operator and its RHS (`b_c = −scale·div_c`).
- The CG iteration state buffers (r, d, q, p, and the scalar slots for rs_old,
  dot scratch, alpha, beta, initial residual, active flag, and tolerance).
- The `cg_*.wgsl` kernel set and its dispatch sequencing in `record_pressure`.
- The host reference implementation and tests (`app/crates/fluid-lab/src/sim/pressure.rs`).

Divergence computation (`app/crates/fluid-lab/src/gpu/shaders/divergence.wgsl`) and the gradient-subtraction
pass (`app/crates/fluid-lab/src/gpu/shaders/gradient.wgsl`, `record_finish`) are called immediately before
and after this subsystem but are **owned by the sim step** — see `simulation.md`.

## The solver: unpreconditioned Conjugate Gradient

**Operator.** The SPD MAC-Poisson operator restricted to Liquid cells:
`(A x)_c = n_c · x_c − Σ_{liquid nb} x_nb`, where `n_c` counts non-solid
(liquid + air) neighbours. Air neighbours raise `n_c` but contribute `x = 0`
(Dirichlet free surface). Solid neighbours are excluded from both sum and count
(Neumann). Host reference: `app/crates/fluid-lab/src/sim/pressure.rs → apply_poisson`.

**RHS.** `b_c = −scale · div_c` at Liquid cells, zero elsewhere.
`scale = ρ h² / dt`. `app/crates/fluid-lab/src/sim/pressure.rs → ProjectionParams::rhs_scale`.

**Host reference.** `app/crates/fluid-lab/src/sim/pressure.rs → cg_solve` — plain Rust, no GPU, used by
tests. `cg_solve` remains the compatibility wrapper: zero initial pressure, fixed
iteration budget, and no residual tolerance. The internal
`app/crates/fluid-lab/src/sim/pressure.rs → cg_solve_with_options` helper accepts an
optional initial pressure field and an optional relative residual tolerance. When an
initial field is provided it is copied only for Liquid cells, the residual starts at
`r = b - A·p_initial`, and tolerance exit uses `rs_new <= tol² · rs_initial`.

**GPU kernel set.** The `cg_*.wgsl` kernels in `app/crates/fluid-lab/src/gpu/shaders/`
each own one algebraic step; `app/crates/fluid-lab/src/gpu/fluid.rs → record_pressure`
owns their fixed dispatch order. The sequence mirrors `cg_solve`: initialize `p/r/d`,
capture the initial residual, then run SpMV, dot products, scalar updates, pressure
update, residual update, and direction update for the configured iteration cap.

**Dot products.** Each `dot` is a two-level tree reduction: `cg_reduce` produces one
partial sum per workgroup into a scratch buffer; `cg_reduce_final` sums those into a
single scalar slot. Fixed dispatch order → fixed-order floating-point summation →
run-to-run deterministic on a given GPU. The tail of `cg_reduce` must branch before
loading vector buffers; do not rely on WGSL `select` to mask out-of-range lanes.

**Residual active gating.** Runtime exposes
`solver.pressure_residual_tolerance` (Live, default `0`) as an optional relative
residual tolerance. `0` disables gating and preserves the fixed-iteration behavior.
When nonzero, `cg_set_rsold` stores the initial residual and marks the solve active;
`cg_beta` clears the active flag after `rs_new <= tolerance² · rs_initial`. The
fixed dispatch loop is still recorded every substep. Inactive solves no-op inside the
heavy SpMV, reduction, update, and direction kernels, so this reduces shader
math/memory work after convergence without reducing dispatch count or adding CPU
readback.

**Pressure warm-start.** Runtime exposes `solver.pressure_warm_start` (Live,
default `1`). Default-on skips the prep clear so `cg_init` can reuse the previous
`pressure_a` field for Liquid cells, compute `r = b - A*p_old` on GPU, and seed
`d = r`. Setting it to `0` restores the zero-start path and clears `pressure_a` in
prep before `cg_init`. `cg_init` writes `0` into non-Liquid pressure entries before
the gradient pass can read them. Reset clears `pressure_a`, and rebuilds allocate a
fresh pressure buffer, so scene/rebuild changes do not carry stale pressure.

## Non-obvious invariants and gotchas

**GPU warm-start changes only initialization.** The default path reuses the previous
pressure field as the initial CG guess; the zero-start path is still available by
setting `solver.pressure_warm_start = 0`. This does not add CPU readback, indirect
dispatch, preconditioning, or a different iteration loop.

**Participation is cell-type gated.** Only Liquid cells hold a meaningful pressure.
Air cells are Dirichlet `p = 0` (they count as neighbours, pushing `n_c` up but
contributing nothing to the sum). Solid cells are Neumann (excluded entirely). This
is the stencil used by both `apply_poisson` and `cg_spmv`.

**Iteration budget.** Registry `solver.pressure_iterations` is Live and defaults in
`app/crates/fluid-lab/src/settings/mod.rs → Registry::default`. `record_pressure`
uses the setting as a fixed cap; residual active gating is not a reduced
dispatch-count claim. The CG-vs-Jacobi rationale lives in `../decisions/pressure.md`,
and `app/crates/fluid-lab/src/sim/pressure.rs → cg_beats_jacobi_16cubed` keeps the
host reference comparison covered by `cargo test --lib`.

**Scale consistency.** Relative divergence reduction is independent of ρ. Host tests
use `ρ = dt = h = 1` (`ProjectionParams::unit`). Runtime uses a hardcoded
`ρ = 1000`; there is no user-facing `solver.density` setting.
`app/crates/fluid-lab/src/sim/pressure.rs → cg_beats_jacobi_16cubed` asserts CG cuts
L2 divergence sharply and beats Jacobi on the same reference case.

**CG float ≠ integer P2G determinism.** The pressure solve is f32; CG dot-product
reductions are fixed-order but still floating-point. This is a separate, compatible
guarantee from the integer-atomic P2G determinism invariant (which must never
introduce a float reduction — see `simulation.md` and `../decisions/simulation.md`).

**Division guards are branches, not `select`.** `cg_alpha` and `cg_beta` explicitly
branch around near-zero denominators before dividing. WGSL `select` is not a safe
guard for arithmetic that would be invalid if both operands are evaluated.

**Gradient subtraction and solid re-enforcement are not owned here.** After
`record_pressure`, `record_finish` runs the gradient pass and `enforce` to zero solid
faces. Documented in `simulation.md`.

**Every Liquid cell is interior.** Boundary cells are always Solid (one-cell walls),
so Liquid cells always have exactly 6 in-range neighbours. No bounds checks are
needed inside the pressure or SpMV kernels.

## Update when

- The operator stencil changes (e.g. variable-density, ghost-fluid free surface).
- Preconditioning is added (changes the kernel set and the CG loop structure).
- `solver.pressure_iterations` default changes (verify `cg_beats_jacobi_16cubed` still holds).
- The default value or reset semantics of `solver.pressure_warm_start` changes.
- The two-level dot-product reduction workgroup size changes (determinism anchor).
- Air/Solid boundary conventions change (must stay consistent between `apply_poisson`
  and `cg_spmv`).

## See also

- `simulation.md` — produces divergence, owns gradient subtraction and solid
  re-enforcement after the solve.
- `gpu-resources.md` — CG scalar and vector buffer layout.
- `profiler.md` — `pressure` pass timing scope.
- `../decisions/pressure.md` — why CG (not Jacobi), replaceability rationale.
- `../agent-context/maintaining-docs.md`
