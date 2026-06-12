---
status:        active
owner:         codex
last_updated:  2026-06-12
okay_to_delete: false
long_lived:    false
owning_docs:
  - architecture/app-shell.md
  - architecture/pressure-solver.md
  - architecture/profiler.md
  - architecture/settings.md
  - architecture/web-shell.md
  - architecture/gpu-resources.md
  - architecture/rendering.md
  - architecture/simulation.md
  - decisions/performance.md
  - decisions/observability.md
  - decisions/pressure.md
  - decisions/platform.md
---

# LLM Review Overhaul Hub

## Mission

Turn the pasted external LLM review into a staged overhaul of fluid-lab. The work
is intentionally split into independently reviewable plans so each stage has a
measurable acceptance gate and the highest-risk GPU/solver work happens after the
observability needed to judge it.

## Source Critique

The review identifies these themes:

- Sim speed depended on display refresh when `fixed_dt = 1/120`, `max_substeps = 1`,
  and capped frames dropped accumulated time.
- Pressure solve does fixed work and starts cold each substep despite temporal
  coherence.
- Volume loss is visible and should be measured before deeper correction work.
- The profiler/capture harness should become a gate, not just a dashboard.
- Settings and URL parameters should route through the registry and expose honest
  apply/clamp outcomes.
- Docs should stop repeating removed-feature absences across subsystem docs.
- Device-loss and full VRAM accounting need an owner and recovery/measurement story.

## Plan Set

| Plan | Stage | Status | Owned Area | Purpose |
|---|---:|---|---|---|
| `llm-review-01-realtime-step.md` | 1 | Shipped | app shell, profiler, web panel | Make sim-time/wall-time ratio explicit and fix refresh-rate slow motion. |
| `llm-review-02-observability-gates.md` | 1 | Active | profiler, capture, testing docs | Add volume drift and perf-budget acceptance before optimization work. |
| `llm-review-03-config-shareability.md` | 2 | Active | settings registry, JS shell | Honest setting outcomes plus registry-backed shareable config URLs. |
| `llm-review-04-pressure-performance.md` | 3 | Active | pressure solver, GPU passes | Warm-start and residual-gated pressure work after gates exist. |
| `llm-review-05-volume-fidelity.md` | 4 | Draft | simulation fidelity | Use the new drift metric to select and verify a volume-loss correction. |
| `llm-review-06-docs-platform-cleanup.md` | 5 | Active | docs, device lifecycle, VRAM | Consolidate removed-feature notes and document platform recovery gaps. |

## Streams

| Stream | Area | Status | Last observed fact | Next action | Blockers |
|---|---|---|---|---|---|
| A | Done | Worker added RTF/policy stats, panel display, tests, docs, and changed the measured default cap to `2` while preserving drop-excess hitches. Final capture reported `max_substeps:2`, `natural_substeps:2`, and `substep_cap_hit:false`. | None | Lower refresh/throttled frames can still run below real time by design. |
| B | Stage 1 done | Worker added opt-in capture assertions, stats/trace sidecars, occupied-cell drift proxy, and tracked memory categories; final capture emitted `stats.json`/trace and passed assertions. | Revisit Rust drift promotion/timing bytes later. | None for stage 1. |
| C | Settings/shareability | Stage 1 done | Worker added `set_setting_result_json`, registry mutation results, registry-backed `?set=id:value`, bridge-owned legacy restore, and docs; narrow gates passed. | Browser smoke at consolidated gate; preset UI remains deferred. | None |
| D | Pressure performance | Stage 1 done | Worker added host `cg_solve_with_options`, warm initial guess/tolerance tests, and docs; runtime GPU loop remains unchanged. | Defer GPU active gating/warm-start until browser metrics and review. | GPU stages still need browser metrics. |
| E | Volume fidelity | Queued | External review names volume loss as the biggest physical defect. | Wait for volume metric from plan 02, then plan correction. | Depends on plan 02. |
| F | Docs/platform cleanup | Stage 1 done | Worker made rendering the canonical removed-feature owner, replaced duplicated absence prose with pointers, and documented surface/device-loss current state. | Source-level device-loss status remains future work. | None |

## Sequencing Rules

- Read-only audit agents may run in parallel.
- Code-touching workers run sequentially in this repo. The app-specific
  orchestration notes say not to use worktrees and not to run multiple code agents
  against the live tree.
- Run the cheapest narrow gate per worker, then a consolidated end gate with
  `cargo test --lib`, `cargo build --target wasm32-unknown-unknown`, and one real-GPU
  capture if visible/GPU behavior changed.
- Do not make a performance claim unless the capture/profiler evidence names the
  browser/GPU, grid resolution, particle count, pressure iterations, render mode, and
  measured frame or pass time.

## Decisions

- Treat stage 1 as enabling work, not just UI polish: later solver and volume work
  needs explicit real-time, drift, and budget signals to be judged honestly.
- Keep active-cell compaction out of this first overhaul. It is strategic but large
  enough to need a separate future plan after lower-risk pressure and observability
  changes land.
- Keep device-loss recovery as a platform follow-up unless audits show an obvious
  small recovery hook. A documented owner and failure mode is better than pretending
  full recovery exists.

## Open Questions

- Plan 01 used the measured default-cap fix: `physics.max_substeps = 2`, not adaptive
  or unbounded catch-up. Final capture confirmed the ordinary cap hit is gone
  (`natural_substeps:2`, `substep_cap_hit:false`).
- Should capture perf budgets start as advisory JSON output or fail the harness by
  default? The audit agent should inspect existing testing policy.
- Should shareable URLs encode only non-default settings, or every registry value?
  Current recommendation: versioned config payloads and URL/import patches should use
  stable registry IDs or explicit URL keys plus enum slugs, not positional enum labels.

## Consolidated Exit Gate

- All active plan docs either shipped with migration notes or left clearly queued with
  blockers.
- `cd app && cargo test --lib`
- `cd app && cargo build --target wasm32-unknown-unknown`
- If visible/GPU behavior changes, run `app/tools/capture.mjs` through Windows node as
  documented in `docs/agent-context/build-run.md`.

## Gate Evidence

- `cargo test --lib` — passed, 47 tests.
- `cargo build --target wasm32-unknown-unknown` — passed.
- `node --check tools/capture.mjs`, `node --check web/main.js`, and
  `node --check web/panels.js` — passed.
- Real Chrome/WebGPU capture — passed at `captures/llm-overhaul-cap2-smoke.png`.
  Final stats included `timing:"gpu-timestamp"`, `scale_status:"ok"`,
  `max_substeps:2`, `natural_substeps:2`, `substep_cap_hit:false`,
  `sim_advanced_ms:16.667`, `wall_raf_ms:20.8`, `real_time_factor:0.8013`,
  `gpu.sim_ms:8.405`, `gpu.render_ms:0.993`, and occupied-cell drift proxy `0`.

## See also

- `docs/plans/llm-review-01-realtime-step.md`
- `docs/plans/llm-review-02-observability-gates.md`
- `docs/plans/llm-review-03-config-shareability.md`
- `docs/plans/llm-review-04-pressure-performance.md`
- `docs/plans/llm-review-05-volume-fidelity.md`
- `docs/plans/llm-review-06-docs-platform-cleanup.md`
