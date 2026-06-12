---
status:        active
owner:         adamg
last_updated:  2026-06-05
---

# Decisions — Pressure

## Pressure projection is a core feature, not later polish

**Decision** — Incompressibility / pressure solving is part of the core simulation
from the first GPU loop.

**Why** — Without projection the water behaves like loose particles or compressible
smoke, not liquid. Pressure is what makes it read as water.

**Tradeoffs** — The pressure solve is the hardest, most performance-sensitive part,
so early versions accept low resolution; correctness and debuggability outrank visual
scale.

**Applies to** — `architecture/pressure-solver.md`, `architecture/simulation.md`.

## Keep the pressure solver replaceable

**Decision** — The pressure solve is isolated behind a clear solver kernel/interface
rather than hard-coded throughout the loop.

**Why** — Solver quality and cost dominate the sim. Isolating it lets a different
method be tried without rewriting the project — which is exactly what happened when
the first solver was replaced (below).

**Code anchors** — host reference `app/crates/fluid-lab/src/sim/pressure.rs → cg_solve`; GPU kernels
`app/crates/fluid-lab/src/gpu/shaders/cg_*.wgsl` looped from `app/crates/fluid-lab/src/gpu/fluid.rs → record_pressure`.

**Applies to** — `architecture/pressure-solver.md`.

## Unpreconditioned Conjugate Gradient is the default solver

**Decision** — The default pressure solver is unpreconditioned CG on the SPD
MAC-Poisson operator, default `solver.pressure_iterations = 30` (Live). It replaced
the original Jacobi solver.

**Why** — Jacobi's iteration count grows O(N²) and cannot converge the low-frequency
hydrostatic mode across a deep (64-cell) column, so a settled pool over-compacted
(~11.5k liquid cells at 64³, ~2.7× over-dense). CG converges that mode in ~15
iterations; at 30 iters a settled pool holds ~19.2k cells (+67%, visibly deeper) at
the *same* ~1.4 ms GPU/step cost as the old under-converged Jacobi-120.

**Tradeoffs / known limit** — The residual gap from ~19.2k to the ideal ~31.8k seed
density is **inherent FLIP volume loss at 64³, not solver under-convergence**: a
fully-converged CG and a brute-force 400-iteration Jacobi/RBGS both plateau at
~19.2k. Closing it is a transfer-quality problem (APIC/affine transfer, density
correction) or higher resolution — *not* more pressure iterations.

**Alternatives considered** — Jacobi (too slow to converge the hydrostatic mode);
Red-Black Gauss-Seidel measured **cost-neutral vs Jacobi on GPU** (its 2× per-sweep
convergence is cancelled by 2× dispatch cost from half-idle color threads), so RBGS
alone is not a fix. Multigrid/PCG remain future options.

**Revisit when** — FLIP volume fidelity becomes the focus, or a larger grid needs a
preconditioner.

**Applies to** — `architecture/pressure-solver.md`.

## Boundary conventions: air Dirichlet p=0, solid Neumann

**Decision** — Pressure is solved only at Liquid cells. Air neighbours are Dirichlet
`p = 0` (they count in the stencil contributing zero); Solid neighbours are Neumann
(excluded from both the neighbour sum and the neighbour count). Runtime GPU pressure
is zeroed each step (no warm-start). Determinism of the solve comes from fixed-order
tree reductions in the CG dot products.

**Why** — This is the standard free-surface/closed-tank treatment and keeps the
operator SPD so CG applies. One-cell-thick solid walls make every Liquid cell
interior, so the divergence/pressure kernels need no bounds checks.

**Note** — The CG solve is float; this is orthogonal to and does not weaken the
integer-P2G determinism invariant (see `decisions/simulation.md`).

**Applies to** — `architecture/pressure-solver.md`, `architecture/simulation.md`.

## Host warm-start and tolerance support stays internal until the GPU path is measured

**Decision** — The host CG reference may expose an internal optional initial-pressure
and relative-residual-tolerance helper, but public/default runtime behavior remains
fixed-iteration zero-start CG until GPU warm-start or early-exit work is implemented
and measured.

**Why** — The host helper proves the math and boundary convention without changing
the shipped GPU loop, pressure-buffer clearing semantics, reset behavior, or the
no-normal-frame-readback rule.

**Code anchors** — `app/crates/fluid-lab/src/sim/pressure.rs → cg_solve`;
`app/crates/fluid-lab/src/sim/pressure.rs → cg_solve_with_options`.

**Revisit when** — GPU active gating, indirect dispatch, or pressure warm-start is
wired behind settings and backed by profiler/capture evidence.

**Applies to** — `architecture/pressure-solver.md`, `architecture/simulation.md`.

## See also

- [`../architecture/pressure-solver.md`](../architecture/pressure-solver.md)
- [`simulation.md`](simulation.md) · [`performance.md`](performance.md)
- [`../agent-context/maintaining-docs.md`](../agent-context/maintaining-docs.md)
