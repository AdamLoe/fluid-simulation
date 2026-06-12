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
  - decisions/performance.md
  - decisions/platform.md
---

# Review Fixes Hub

## Goal

Fix the confirmed review findings from the coding and architecture audit:
solver reduction safety, public numeric sanitization, reset atomicity, profiler
honesty, capture/tooling reliability, and docs drift.

## Streams

| Stream | Area | Status | Last observed fact | Next action | Blockers |
|---|---|---|---|---|---|
| A | Solver/WGSL correctness | Done | `cg_reduce` now branches before tail loads; scalar division guards branch before divide. | None | None |
| B | WASM bridge/settings/reset | Done | Non-finite bridge values are rejected; finite out-of-range live values are clamped before GPU update; reset returns false until recreate succeeds. | None | None |
| C | Profiler/timing honesty | Done | GPU readout owns sampled substeps including zero-substep frames; diffuse metrics expose `compute_timed:false`. | None | None |
| D | Web/capture/tooling | Done | Capture defaults to 5184, fails bad evidence/rejected setup, and `run.sh` frees only the target port. | None | None |
| E | Docs drift | Done | Owner docs, manifest routes, and stale decision text updated; stale-phrase grep passed. | None | None |
| F | Review/verification | Done | Rust, WASM, web build, real-GPU capture, and three read-only review agents completed. | None | None |

## Decisions

- Treat capture harness as a pass/fail gate by default. Evidence-only behavior can be
  added later behind an explicit flag, but the default should protect agents.
- Do not introduce a larger refactor while fixing the reset path; make reset atomic by
  moving state commits after `recreate_fluid` succeeds.
- Keep diffuse timing honest. If full timestamp integration is too broad, expose it as
  intentionally untimed and avoid implying GPU totals are complete.

## Verification Plan

- `cargo test --lib` — passed, 38 tests.
- `cargo build --target wasm32-unknown-unknown` — passed.
- `npm run build` from `app/web` — passed.
- `tools/capture.mjs` real-GPU Chrome capture — passed at
  `captures/review-fixes-boot-2.png`, with WebGPU present, smoke PASS, stats JSON,
  and `gpu.diffuse.compute_timed:false`.
