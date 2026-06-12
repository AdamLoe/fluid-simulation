---
status:        active
owner:         codex
last_updated:  2026-06-12
okay_to_delete: false
long_lived:    false
owning_docs:
  - architecture/pressure-solver.md
  - architecture/simulation.md
  - architecture/gpu-resources.md
  - decisions/pressure.md
  - decisions/performance.md
---

# LLM Review 04 - Pressure Performance

## Mission

Reduce pressure-solver work without weakening determinism or pressure correctness.
The intended first wins are warm-starting from the previous pressure field and
skipping converged iterations without CPU readback.

## Scope

In scope:

- Pressure warm-start if current pressure state can be preserved safely across
  substeps/resets.
- Residual-gated early exit only if WebGPU indirect dispatch or an equivalent
  GPU-native mechanism is practical in this codebase.
- Host reference tests and GPU capture evidence for correctness/performance.

Out of scope:

- Active-cell compaction.
- Changing the default solver away from CG.
- Large shader fusion unless profiling proves dispatch overhead is the bottleneck.

## Approach

1. Add host-reference tolerance and optional initial-pressure path first. Prove the
   zero-initial-guess path matches current fixed-iteration behavior.
2. Add GPU residual state and fixed-dispatch active gating before true dispatch-count
   reduction. This can reduce shader math after convergence without adding CPU
   readback.
3. Add warm-start behind a setting/default-off flag: retain `pressure_a`, compute
   `r = b - A*p_old`, initialize `d = r`, zero non-liquid pressure, and handle
   reset/first-step zeroing.
4. Consider true indirect-dispatch reduction only after measurement. Avoid CPU readback
   in normal frames.
5. Use capture/profiler evidence before claiming performance gains.

## Subagents

- Read-only audit: pressure-solver explorer.
- Worker 1: host-reference tolerance and settings/schema prep.
- Worker 2: GPU active-gating implementation.
- Worker 3: warm-start implementation, launched only after the first two stages and
  metric capture are in place.

## Audit Notes

- Current GPU sequence always runs divergence, `cg_init`, then a fixed
  `solver.pressure_iterations` loop. Default is CG-30, max 200.
- `pressure_a` and `pressure_b` are cleared every substep and `cg_init.wgsl` also
  zeroes `pressure_a`, so there is explicitly no warm-start today.
- Warm-start is feasible but medium risk because it changes clear semantics,
  `cg_init`, reset/first-frame behavior, and non-liquid pressure handling.
- True dynamic dispatch reduction would require indirect-dispatch plumbing or
  CPU/GPU synchronization. Fixed-dispatch shader no-op gating is lower risk and keeps
  the no-normal-frame-readback rule.
- Use relative residual: `rs_new <= tol^2 * rs_initial`.

## Worker 1 update

- Host reference now has an internal `cg_solve_with_options` path with optional
  initial pressure and optional relative residual tolerance. `cg_solve` remains the
  public/default compatibility wrapper: zero initial pressure, fixed iteration count,
  and no tolerance.
- Initial pressure is accepted only into Liquid cells; Air/Solid pressure remains zero
  and the existing operator/boundary convention is unchanged.
- Tests cover zero-initial equivalence with the fixed-iteration output and show a
  warm initial pressure can reach an equal/better residual with fewer current-solve
  iterations on the existing 16³ divergent tank case.
- No setting was added in this stage. GPU warm-start, GPU residual state,
  shader/pass early-exit, and browser capture/profiler evidence are deferred to later
  workers.

## Exit Gate

- `cd app && cargo test --lib`
- `cd app && cargo build --target wasm32-unknown-unknown`
- Real-GPU capture with detailed profiler evidence before and after the pressure
  change, naming pressure iterations and top GPU costs.

## Migration Notes

Fill at ship time:

- Solver sequence and buffers -> `architecture/pressure-solver.md`.
- Simulation step interaction -> `architecture/simulation.md`.
- Solver choice/trade-off -> `decisions/pressure.md`.
- Performance evidence -> `decisions/performance.md`.
