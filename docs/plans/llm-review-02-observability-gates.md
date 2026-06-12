---
status:        active
owner:         codex
last_updated:  2026-06-12
okay_to_delete: false
long_lived:    false
owning_docs:
  - architecture/profiler.md
  - architecture/simulation.md
  - architecture/gpu-resources.md
  - agent-context/testing.md
  - agent-context/build-run.md
  - decisions/observability.md
---

# LLM Review 02 - Observability Gates

## Mission

Turn existing stats and capture output into acceptance signals for the overhaul:
volume drift is measured, performance budgets can be asserted, and memory accounting
is explicit enough that later solver/render changes cannot claim wins from partial
data.

## Scope

In scope:

- Volume drift metric based on the post-reset liquid baseline and throttled stats.
- Capture harness support for perf or stats assertions.
- Fuller VRAM accounting if render target sizes are already known in one owner.
- Testing/build-run docs for the new gate behavior.

Out of scope:

- Actual volume correction.
- Perfetto/Chrome trace export unless audit shows it is a very small serializer.
- Device-loss recovery, except for noting platform gaps in plan 06.

## Approach

1. Add capture assertion options first: minimum timing source, max frame average/p95,
   max GPU sim/render cost, required scale status, and required GPU timestamps when
   explicitly requested.
2. Add harness-side trace/stat export by polling `window.__fluid.stats_json()` during
   `MEASURE_WAIT` and writing `<out>.stats.json` plus a compact NDJSON trace.
3. Use existing throttled `gpu.liquid_cells` readback to compute an occupied-cell drift
   proxy in capture, then optionally promote baseline/drift fields into Rust stats if
   the worker finds that cleaner.
4. Supplement `gpu_buffer_mb` with categorized tracked memory fields for sim buffers,
   render targets, diffuse storage, timing buffers, and total tracked memory.
5. Update observability/testing docs with the exact command shape and honesty rules.

## Subagents

- Read-only audit: observability/docs explorer.
- Worker: observability implementation. This worker may touch
  `app/crates/fluid-lab/src/profiler/`, relevant stats producers under
  `app/crates/fluid-lab/src/`, `app/tools/capture.mjs`, and owning docs only.

## Audit Notes

- Capture currently records screenshot, console, and final `stats_json`; it fails on
  missing WebGPU/stats, page/request errors, rejected setup, or smoke failure, but not
  on perf budgets.
- GPU timing/liveness readback is throttled every 20 frames and already includes
  `liquid_cells`.
- `gpu_buffer_mb` is sim-buffer-only today. New memory fields should be labeled as
  tracked/categorized memory, not driver-resident VRAM.
- Timing-source assertions must respect profiler honesty: timestamp-specific budgets
  only apply when `stats.timing === "gpu-timestamp"`.
- Occupied-cell drift is a useful proxy for volume drift but not physical volume.

## Exit Gate

- `cd app && cargo test --lib`
- `cd app && cargo build --target wasm32-unknown-unknown`
- Capture harness run proving the new stats fields are emitted and any new assertion
  mode behaves as documented.

## Migration Notes

Stage 1 implementation context has been migrated into the owner docs. The
consolidated browser capture ran at `captures/llm-overhaul-cap2-smoke.png` and wrote
the stats/trace sidecars, but the plan stays active because occupied-cell drift
remains a harness-side proxy rather than a promoted Rust stat and timing-buffer byte
accounting is still deferred.

- Stats/profiler shape and tracked-memory honesty -> `architecture/profiler.md`.
- GPU resource memory caveats -> `architecture/gpu-resources.md`.
- Occupied-cell drift proxy semantics -> `architecture/simulation.md`.
- Capture gate workflow and exact assertion env vars -> `agent-context/testing.md` and
  `agent-context/build-run.md`.
- Source-honest capture gate policy -> `decisions/observability.md`.

Deferred before shipping:

- Decide whether a dedicated assertion-mode browser smoke with intentionally clamped
  settings is needed before closing this plan, beyond the consolidated capture that
  proved sidecar output.
- Decide whether occupied-cell baseline/drift should stay harness-side or be promoted
  into Rust `stats_json`.
- Fill exact timing-buffer memory accounting if the timing owner becomes in scope;
  stage 1 exposes `timing_mb` as `null`.
