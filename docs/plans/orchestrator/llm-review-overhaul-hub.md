---
status:        shipped
owner:         codex
last_updated:  2026-06-12
okay_to_delete: true
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
| `llm-review-02-observability-gates.md` | 1 | Shipped | profiler, capture, testing docs | Add volume drift and perf-budget acceptance before optimization work. |
| `llm-review-03-config-shareability.md` | 2 | Shipped | settings registry, JS shell | Honest setting outcomes plus registry-backed shareable config URLs. |
| `llm-review-04-pressure-performance.md` | 3 | Shipped | pressure solver, GPU passes | Default-off residual gating and warm-start implemented and real-GPU smoke validated. |
| `llm-review-05-volume-fidelity.md` | 4 | Shipped | simulation fidelity | Two measurement sweeps found no safe default/code change; current occupancy bias remains. |
| `llm-review-06-docs-platform-cleanup.md` | 5 | Shipped | docs, device lifecycle, VRAM | Consolidated removed-feature notes and added honest GPU status/reload reporting. |

## Streams

| Stream | Area | Status | Last observed fact | Next action | Blockers |
|---|---|---|---|---|---|
| A | Done | Worker added RTF/policy stats, panel display, tests, docs, and changed the measured default cap to `2` while preserving drop-excess hitches. Final capture reported `max_substeps:2`, `natural_substeps:2`, and `substep_cap_hit:false`. | None | Lower refresh/throttled frames can still run below real time by design. |
| B | Shipped | Worker added opt-in capture assertions, stats/trace sidecars, ordinary `MEASURE_WAIT` polling, assertion-only stats mode, occupied-cell drift proxy, and tracked timing-buffer memory. | Consolidated browser capture remains orchestrator-owned. | None for code work. |
| C | Settings/shareability | Shipped | Worker added `set_setting_result_json`, registry mutation results, registry-backed `?set=id:value`, bridge-owned legacy restore, compact share/export/import controls, shell smoke hooks, and docs; narrow gates passed. | Browser smoke can verify via `window.__fluidShell.state().urlApplyResult`, `setting(id)`, `shareUrl()`, and `importConfigPayload(...)`. | None |
| D | Pressure performance | Shipped | Worker added host `cg_solve_with_options`, GPU residual active gating behind default-off `solver.pressure_residual_tolerance`, and GPU warm-start behind default-off `solver.pressure_warm_start`; real-GPU captures passed default, warm-start, and warm-start+residual paths. | Future indirect dispatch/default-on decisions need controlled benchmarks. | None for this overhaul. |
| E | Volume fidelity | Shipped | 2026-06-12 sweeps showed the current pressure-coupled occupancy bias is better than `volume_stiffness=0`; stronger candidates inflated occupied cells; nearby softer/clamped candidates were worse than the current default. | Future formula work needs a stronger physical-volume/visual-pulsing gate. | None for this overhaul. |
| F | Docs/platform cleanup | Shipped | Worker made rendering the canonical removed-feature owner, replaced duplicated absence prose with pointers, documented surface/device-loss current state, added `gpuDeviceStatus` shell reporting, and made capture fail on fatal GPU status/console failures. | Full WebGPU device recovery remains a future reload/reinit project. | None |

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

- All plan docs shipped with migration notes.
- `cd app && cargo test --lib`
- `cd app && cargo build --target wasm32-unknown-unknown`
- If visible/GPU behavior changes, run `app/tools/capture.mjs` through Windows node as
  documented in `docs/agent-context/build-run.md`.

## Gate Evidence

- Final consolidated `cargo test --lib` — passed, 54 tests.
- `cargo build --target wasm32-unknown-unknown` — passed.
- `node --check tools/capture.mjs`, `node --check web/main.js`, and
  `node --check web/panels.js` — passed.
- Real Chrome/WebGPU capture — passed at `captures/llm-overhaul-cap2-smoke.png`.
  Final stats included `timing:"gpu-timestamp"`, `scale_status:"ok"`,
  `max_substeps:2`, `natural_substeps:2`, `substep_cap_hit:false`,
  `sim_advanced_ms:16.667`, `wall_raf_ms:20.8`, `real_time_factor:0.8013`,
  `gpu.sim_ms:8.405`, `gpu.render_ms:0.993`, and occupied-cell drift proxy `0`.
- Plan 02 final worker gates — `node --check tools/capture.mjs`, assertion-only
  passing/failing checks, `cargo test --lib`, and
  `cargo build --target wasm32-unknown-unknown` passed. Browser capture was not run
  by that worker; the orchestrator owns the consolidated real-GPU rerun.
- Plan 03 final worker gates — `node --check web/main.js`,
  `node --check web/panels.js`, `cargo test --lib`, and
  `cargo build --target wasm32-unknown-unknown` passed.
- Pressure active-gating real-GPU captures — default-off passed at
  `captures/llm-overhaul-pressure-gating-off-3.png`; tolerance-on via
  `?set=solver.pressure_residual_tolerance:0.05` passed at
  `captures/llm-overhaul-pressure-gating-on.png`. Both had `timing:"gpu-timestamp"`
  and no WebGPU/WGSL validation warnings after the reduction shader fixes.
- Volume fidelity measurement captures — `captures/llm-review-05-default.png`
  ended with occupied-cell drift proxy `+0.1383` (34005 -> 38708 cells);
  `captures/llm-review-05-stiffness0.png` ended at `-0.2978` (13544 -> 9510);
  `captures/llm-review-05-rd6-vs075-dc075.png` ended at `+0.0299` (49491 -> 50969);
  `captures/llm-review-05-rd4-vs12-dc10.png` ended at `-0.0058` (62780 -> 62413).
  The stronger candidates were rejected as default changes because their final
  occupied-cell counts were inflated relative to the default capture.
- Pressure warm-start real-GPU captures — default detailed capture passed at
  `captures/llm-overhaul-final-default-detailed.png`; warm-start URL capture passed
  at `captures/llm-overhaul-final-warm-start.png`; warm-start plus residual gating
  passed at `captures/llm-overhaul-final-warm-residual.png`. Final shell states
  reported `gpuDeviceStatus:"ok"` and URL setting results were applied without
  rejection.
- Volume narrow sweep captures — current default with `classify.surface_dilation=0`
  ended near-flat at `-0.0021` (34423 -> 34350). Nearby candidates ended worse:
  `volume_stiffness=0.30` at `-0.0849`, `volume_stiffness=0.35` at `-0.0499`,
  `drift_clamp=0.35` at `-0.0224`, and combined `0.35/0.35` at `-0.0622`. No
  default/code change was selected.

## See also

- `docs/plans/llm-review-01-realtime-step.md`
- `docs/plans/llm-review-02-observability-gates.md`
- `docs/plans/llm-review-03-config-shareability.md`
- `docs/plans/llm-review-04-pressure-performance.md`
- `docs/plans/llm-review-05-volume-fidelity.md`
- `docs/plans/llm-review-06-docs-platform-cleanup.md`
