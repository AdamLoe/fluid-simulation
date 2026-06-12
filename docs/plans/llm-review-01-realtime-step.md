---
status:        shipped
owner:         codex
last_updated:  2026-06-12
okay_to_delete: true
long_lived:    false
owning_docs:
  - architecture/app-shell.md
  - architecture/profiler.md
  - decisions/performance.md
---

# LLM Review 01 - Real-Time Step

## Mission

Make the simulation's refresh-rate dependence explicit, then use measured GPU evidence
to choose the scheduling policy change. Stage 1 is done when the profiler reports the
sim-time/wall-time ratio and cap/drop policy honestly; the full plan is done when a
60 Hz display no longer advances the liquid at roughly half speed solely because only
one 1/120 s substep is allowed per animation frame, or when a documented measurement
decision explains why that default remains.

## Scope

In scope:

- App frame accumulator and substep cap policy.
- Stats/profiler fields needed to show real-time factor and dropped-time behavior.
- Web panel display for the new timing signal.
- Host tests for timestep accounting where practical.

Out of scope:

- Pressure-solver optimizations.
- Changing the fixed simulation step size unless the audit finds that is the least
  risky fix.
- Any claim that the GPU has more headroom without profiler evidence.

## Approach

1. Audit current `FluidApp` frame accounting and `Timestep` behavior.
2. Add explicit policy observability first: natural substeps, cap hit, max cap,
   simulated ms, wall ms, and real-time factor derived from executed physics over raw
   sanitized rAF wall time.
3. Add stats fields for substeps, simulated seconds, wall seconds, dropped seconds, and
   real-time factor in the existing stats surface.
4. Render the signal in the profiler/web panel without adding a new control surface.
5. Fix paused single-step accounting if it currently reports contradictory substep
   counts.
6. Make the smallest scheduling-policy change justified by the evidence. If changing
   the default cap is too risky without real-GPU profiling, leave behavior unchanged
   but make the slow-motion condition visible and document the remaining policy
   decision in this plan.
7. Update app-shell/profiler/performance docs with the new current state.

## Subagents

- Read-only audit: time-step/profiler explorer.
- Worker: app-shell timestep implementation. This worker may touch
  `app/crates/fluid-lab/src/lib.rs`, `app/crates/fluid-lab/src/timestep.rs`,
  `app/crates/fluid-lab/src/profiler/`, `app/web/`, and owning docs only.

## Audit Notes

- Current code clamps render dt to `1/30s`, computes natural substeps, executes
  `min(natural, max_substeps)`, then drops leftover accumulator when capped.
- The pre-policy capture with `max_substeps=1` reported `timing:"gpu-timestamp"`,
  `gpu.sim_ms:3.854`, `gpu.render_ms:1.023`, `natural_substeps:2`,
  `substep_cap_hit:true`, and `real_time_factor:0.4026`, showing ordinary 60 Hz
  slow motion while measured GPU work remained below a frame budget.
- Default `fixed_dt=1/120s` and `max_substeps=2` lets a 60 Hz frame execute the two
  substeps it naturally wants when budget allows, while larger hitches still cap and
  drop excess accumulated time.
- Profiler JSON already exposes substeps and dropped time; it should add
  `fixed_dt_ms`, `max_substeps`, `natural_substeps`, `substep_cap_hit`,
  `sim_advanced_ms`, `real_time_factor`, and a policy label without removing existing
  keys.
- Use raw sanitized rAF wall time as the real-time-factor denominator so hitches and
  throttling remain visible.

## Exit Gate

- `cd app && cargo test --lib`
- `cd app && cargo build --target wasm32-unknown-unknown`
- If the web panel changes, one capture run that records `stats_json` with the new
  timing fields.

## Migration Notes

Stage 1 implementation note:

- Current timestep policy and expanded `TimestepFrameStats` fields were migrated to
  `architecture/app-shell.md`.
- Profiler/stat fields were migrated to `architecture/profiler.md`.
- Performance rationale was migrated to `decisions/performance.md`.

Measured scheduling-policy note:

- The registry default for `physics.max_substeps` is now 2. The fixed `1/120s`
  timestep and drop-excess hitch policy remain unchanged.
- Final browser capture passed at `captures/llm-overhaul-cap2-smoke.png`; it reported
  `max_substeps:2`, `natural_substeps:2`, `substep_cap_hit:false`,
  `gpu.sim_ms:8.405`, `gpu.render_ms:0.993`, and `real_time_factor:0.8013` on a
  ~20.8 ms headless frame. The ordinary 60 Hz cap-hit defect is fixed; lower
  refresh/throttled frames can still run below real time by design.
