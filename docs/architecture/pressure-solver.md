---
status:        active
owner:         adamg
last_updated:  2026-06-05
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
- The CG iteration state buffers (r, d, q, p, and the scalar slots for rs_old, alpha,
  beta).
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
tests. The same math is mirrored verbatim in the WGSL kernel set.

**GPU kernel set.** A set of small `cg_*.wgsl` kernels in `app/crates/fluid-lab/src/gpu/shaders/`, each doing
one algebraic step. Dispatch sequence in `app/crates/fluid-lab/src/gpu/fluid.rs → record_pressure`:

```
init (p=0, r=b, d=b)
 ↓ reduce+reduce_final+set_rsold  [rs_old = dot(r,r)]
 ↓─────────── repeat pressure_iters times ────────────
 │  spmv    q = A·d
 │  reduce+reduce_final           [dq = dot(d,q)]
 │  alpha   α = rs_old / dq
 │  update  p += α·d ; r -= α·q
 │  reduce+reduce_final           [rs_new = dot(r,r)]
 │  beta    β = rs_new/rs_old ; rs_old = rs_new
 └  dir     d = r + β·d
```

**Dot products.** Each `dot` is a two-level tree reduction: `cg_reduce` produces one
partial sum per workgroup into a scratch buffer; `cg_reduce_final` sums those into a
single scalar slot. Fixed dispatch order → fixed-order floating-point summation →
run-to-run deterministic on a given GPU. The tail of `cg_reduce` must branch before
loading vector buffers; do not rely on WGSL `select` to mask out-of-range lanes.

## Non-obvious invariants and gotchas

**Pressure is zeroed each step — no warm-start.** `cg_init` sets `p = 0` before the
first iteration every step. The result is deterministic; the previous frame's pressure
field is discarded.

**Participation is cell-type gated.** Only Liquid cells hold a meaningful pressure.
Air cells are Dirichlet `p = 0` (they count as neighbours, pushing `n_c` up but
contributing nothing to the sum). Solid cells are Neumann (excluded entirely). This
is the stencil used by both `apply_poisson` and `cg_spmv`.

**Default 30 iters; CG knee ~15.** Registry `solver.pressure_iterations` (Live).
The settled-pool residual plateau at ~19.2 k liquid cells (64³) is FLIP volume loss,
not solver under-convergence — brute-force Jacobi at 400 iters reaches the same
ceiling. Raising iterations past ~30 has no visible effect on fluid volume.

**Scale consistency.** Relative divergence reduction is independent of ρ. Host tests
use `ρ = dt = h = 1` (`ProjectionParams::unit`). Runtime uses a hardcoded
`ρ = 1000`; there is no user-facing `solver.density` setting.
`app/crates/fluid-lab/src/sim/pressure.rs → cg_beats_jacobi_16cubed` asserts CG cuts
L2 divergence sharply and beats Jacobi on the same reference case.

**CG float ≠ integer P2G determinism.** The pressure solve has always been f32; CG
dot-product reductions are fixed-order but still floating-point. This is a separate,
compatible guarantee from the integer-atomic P2G determinism invariant (which must
never introduce a float reduction — see `simulation.md` and `../decisions/simulation.md`).

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
- `pressure_iters` default changes (verify `cg_beats_jacobi_16cubed` still holds).
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
