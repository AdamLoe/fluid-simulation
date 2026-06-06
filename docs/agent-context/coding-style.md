# Coding style — fluid-lab

The universal principles **and** the generic per-language idioms live in
`~/.claude/agent-docs/v1/rules/coding-style.md` — read that first. This file lists
only this app's specifics, named patterns, and the invariants a casual edit can break.

## When does this apply

You're editing Rust, WGSL, or TypeScript under `app/`.

## App specifics

**Rust**
- One crate (`fluid-lab`), edition 2021, modules `sim` / `gpu` / `scene` / `settings`
  / `profiler` (`app/Cargo.toml`). Do not split into a workspace — see
  [`../decisions/platform.md`](../decisions/platform.md).
- Rust↔WGSL struct mirroring goes through `bytemuck`; check alignment/padding before
  trusting a mirrored struct. Hot data is structure-of-arrays.

**WGSL**
- **naga drops unused bindings.** A compute shader that never references `params`
  (binding 0) gets a different auto-generated bind-group layout than the Rust side
  expects, and pipeline creation desyncs/fails. Reference `params` in every compute
  shader, or use an explicit bind-group layout (the renderers do).
- **No float atomics exist in WebGPU.** The P2G accumulate→normalize path must stay
  integer/fixed-point — a float reduction silently breaks determinism and is a
  contract change. See [`../decisions/simulation.md`](../decisions/simulation.md).
- A single GPU pass must stay within `maxStorageBuffersPerShaderStage` (~8–10);
  split passes rather than binding everything at once.

**TypeScript / web**
- The shell is intentionally thin — no framework (React is optional wrapper only).
- The canonical, verified front-end is the no-bundler static path (`web/index.html` +
  `web/main.js` + `web/panels.js`); the orphaned `web/src/main.ts` Vite stub is dead. See
  [`../architecture/web-shell.md`](../architecture/web-shell.md).
- Panels are rendered *from* the WASM config registry (`config_json` / `set_setting` /
  `stats_json`) — never hand-wire a control against ad-hoc state.

## Invariants a casual edit can break

- Integer-only P2G accumulate→normalize (determinism).
- No CPU/GPU readback on the normal render frame (only throttled diagnostics/captures).
- The simulation never consumes raw browser frame `dt` — only the clamped accumulator.

## See also

- [`../architecture/index.md`](../architecture/index.md) — facts your edits must align with.
- [`build-run.md`](build-run.md) — how to build/verify.
- [`maintaining-docs.md`](maintaining-docs.md) — when a code edit needs a doc update.
- [`../decisions/simulation.md`](../decisions/simulation.md) · [`../decisions/platform.md`](../decisions/platform.md)
