# Decisions index

Rationale for current design choices, indexed by **architecture domain** (not by
date — the git log owns dates). Each entry states a decision and the reason that
still holds; superseded deliberation stays in git, not here.

## How to use this folder

Load only the domain doc that matches the area you're working in. For the **what**
(current behaviour) follow the architecture doc the decision constrains — each entry
names it under `Applies to`.

## Routing

| Need | Read |
|---|---|
| True 3D MAC grid, hybrid FLIP/PIC, fixed-point integer-atomic P2G & determinism, fixed/clamped timestep | [`simulation.md`](simulation.md) |
| Pressure projection as core, replaceable solver, why CG over Jacobi, boundary conventions | [`pressure.md`](pressure.md) |
| GPU-native no-readback views, particle/liquid-cell product surface, single-pass rendering, separate particle/grid representations | [`rendering.md`](rendering.md) |
| 32³/64³/128³ targets, one preset until 1.5, SoA buffers, per-stage storage-buffer limit, 1M-particle stretch | [`performance.md`](performance.md) |
| Observability-first product, schema-driven config registry, apply classes, hierarchical/timing-honest profiler | [`observability.md`](observability.md) |
| Rust+WASM+WebGPU, single crate not workspace, tiny disposable CPU reference, React optional, the no-bundler web path | [`platform.md`](platform.md) |
| Bounded tank vs ocean, fluid-lab direction, scenarios are 1.x, source/drain deferred, optional features are not blockers | [`scope.md`](scope.md) |

## See also

- [`../index.md`](../index.md) — global router.
- [`../architecture/index.md`](../architecture/index.md) — what these constrain.
- [`../agent-context/maintaining-docs.md`](../agent-context/maintaining-docs.md) — the authoring rules.
