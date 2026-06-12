---
status:        shipped
owner:         codex
last_updated:  2026-06-12
okay_to_delete: true
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

## Worker 2 update

- Added `solver.pressure_residual_tolerance` as a Live `f32` setting. Default `0`
  disables residual gating; finite values clamp to a conservative relative-residual
  range before the GPU tolerance-squared scalar slot is updated.
- Expanded GPU CG scalar state to seven `f32` slots:
  `0 rs_old`, `1 dot_scratch`, `2 alpha`, `3 beta`, `4 rs_initial`, `5 active`,
  `6 tol_sq`.
- `cg_init` resets scalar scratch/active for each solve, `cg_set_rsold` stores
  `rs_old`, `rs_initial`, and `active`, and `cg_beta` clears `active` when
  `tol_sq > 0 && rs_new <= tol_sq * rs_initial`.
- Fixed dispatch loops remain unchanged. `cg_spmv`, `cg_reduce`,
  `cg_reduce_final`, `cg_update`, and `cg_dir` no-op when inactive, reducing
  shader math/memory after convergence without CPU readback or reduced dispatch
  count.
- Shader contract tests cover the scalar layout and default-off active-gating shape.
- Real Chrome/WebGPU captures passed after fixing two WGSL-only issues that Rust/WASM
  compile did not catch: storage-dependent reduction returns before barriers, and
  `active` as a reserved WGSL local name. The capture harness now fails on WebGPU/WGSL
  validation warnings so rejected pipelines cannot look green.
- Default-off capture: `captures/llm-overhaul-pressure-gating-off-3.png`, with
  `pressure_ms:9.623`, `sim_ms:11.593`, `liquid_cells:35814`, and no validation
  warnings.
- Tolerance-on capture through `?set=solver.pressure_residual_tolerance:0.05`:
  `captures/llm-overhaul-pressure-gating-on.png`, with `pressure_ms:7.756`,
  `sim_ms:10.116`, `liquid_cells:35983`, and no validation warnings. This is useful
  smoke evidence, not a controlled benchmark.
- Warm-start, indirect dispatch, controlled before/after benchmarking, and any reduced
  dispatch-count claim remain deferred.

## Worker 3 update

- Added `solver.pressure_warm_start` as a Live `u32` boolean setting. Default `0`
  preserves the zero-start pressure solve and existing default capture comparability.
- Mirrored the flag into `Params.dims[3]`. When default-off, prep still clears
  `pressure_a` before `cg_init`, and `cg_init` starts from `p = 0`.
- When enabled, prep preserves `pressure_a`; `cg_init` reuses Liquid-cell pressure as
  the initial guess, computes `r = b - A*p_old` on GPU, sets `d = r`, and zeros
  non-Liquid pressure entries so gradient reads stay clean.
- Reset now clears `pressure_a`, while rebuild/scene changes allocate fresh pressure
  buffers through the existing `GpuFluid` recreate path.
- Tests cover the setting default/apply/metadata shape and the shader warm-start
  contract. No CPU readback, indirect dispatch, scalar-layout change, or reserved
  WGSL `active` local was added.
- Real Chrome/WebGPU captures passed with no WebGPU/WGSL validation warnings:
  `captures/llm-overhaul-final-default-detailed.png` kept the default zero-start
  path (`dispatches_per_substep:309`, `pressure_ms:8.463`, `sim_ms:9.832`);
  `captures/llm-overhaul-final-warm-start.png` applied
  `?set=solver.pressure_warm_start:1` (`dispatches_per_substep:308`,
  `gpuDeviceStatus:"ok"`); and
  `captures/llm-overhaul-final-warm-residual.png` applied warm-start plus
  `solver.pressure_residual_tolerance:0.05` (`pressure_ms:7.856`,
  `gpuDeviceStatus:"ok"`).
- Warm-start and residual gating remain default-off. The captures are runtime
  validation and smoke evidence, not a controlled benchmark or a default-change
  justification. True reduced dispatch count via indirect dispatch remains a future
  plan.

## Exit Gate

- `cd app && cargo test --lib` passed, 54 tests.
- `cd app && cargo build --target wasm32-unknown-unknown` passed.
- `node --check tools/capture.mjs`, `node --check web/main.js`, and
  `node --check web/panels.js` passed.
- `git diff --check` passed.
- Real-GPU captures listed above passed and named pressure iterations, timing source,
  GPU status, dispatch count, and top CG costs.

## Migration Notes

- Solver sequence and buffers -> `architecture/pressure-solver.md`.
- Simulation step interaction -> `architecture/simulation.md`.
- Solver choice/trade-off -> `decisions/pressure.md`.
- Performance evidence -> `decisions/performance.md`.
- Future indirect dispatch / default-on decisions need a new plan with controlled
  before/after benchmarks.
