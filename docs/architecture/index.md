# Architecture index

Current-state subsystem docs. These describe what the system **is now**, not how it
changed over time.

## How to use this folder

Load only the subsystem doc that matches the task. For rationale, follow the doc's
`See also` into [`../decisions/`](../decisions/index.md).

## Subsystems

| Need | Read |
|---|---|
| WASM/JS boundary, the per-frame loop, the fixed/clamped timestep accumulator, orbit camera + interactive tank pointer modes, the typed scene config | [`app-shell.md`](app-shell.md) |
| The hybrid FLIP/PIC MAC-grid sim — indexing, cell typing, fixed-point integer-atomic P2G, G2P blend, forces, advection, recovery, the per-substep loop | [`simulation.md`](simulation.md) |
| External-LLM handoff for solver and rendering questions; start with the canonical owners | [`fluid-system-llm-brief.md`](fluid-system-llm-brief.md) |
| The Conjugate-Gradient pressure solve (SPD MAC-Poisson), the `cg_*.wgsl` kernels, boundary conventions, host reference | [`pressure-solver.md`](pressure-solver.md) |
| wgpu device/surface init, adapter-limit probe, SoA buffer layout, bind-group strategy, the per-stage storage-buffer constraint, the recreate path | [`gpu-resources.md`](gpu-resources.md) |
| GPU-native screen-space water, optical/simple particle rendering, tank-wireframe, and grid-slice views; water pass order and no-normal-frame-readback rules | [`rendering.md`](rendering.md) |
| The hierarchical, config-tagged, timing-source-honest profiler + GPU timestamp queries | [`profiler.md`](profiler.md) |
| The typed config registry, apply classes (live/reset/reload), the JS bridge | [`settings.md`](settings.md) |
| The thin JS/HTML web shell, the legacy TS stub, the rendered config/profiler panels, the capture harness | [`web-shell.md`](web-shell.md) |

## See also

- [`../decisions/index.md`](../decisions/index.md) — why these are shaped this way.
- [`../ownership.md`](../ownership.md) — canonical owner per concept.
- [`../agent-context/maintaining-docs.md`](../agent-context/maintaining-docs.md) — doc-authoring rules.
