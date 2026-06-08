---
status:        active
owner:         adamg
last_updated:  2026-06-07
---

# Decisions — Platform

## Rust + WASM + WebGPU

**Decision** — The project is browser-native: Rust compiled to WASM, GPU work through
WebGPU / wgpu / WGSL.

**Why** — It matches the goal of a visual, systems-heavy Rust/WASM portfolio project,
and WebGPU is the right browser technology for compute-heavy GPU simulation and
rendering. The result is easy to share from a portfolio site.

**Tradeoffs** — Browser GPU constraints (limits, feature availability, timestamp-query
gaps) and awkward WASM/browser debugging, in exchange for shareability.

**Applies to** — `architecture/gpu-resources.md`, `architecture/web-shell.md`.

## One Rust crate with modules, not a multi-crate workspace

**Decision** — The project is a single crate (`fluid-lab`) with internal modules
(`sim`, `gpu`, `scene`, `settings`, `profiler`), not a multi-crate workspace.

**Why** — A multi-crate split is premature modularization for a solo build: it adds
workspace wiring and cross-crate visibility friction before any code exists, and the
boundaries are guesses until the code is real. Modules promote to crates trivially
later if compile times or reuse actually force it.

**Tradeoffs** — Slightly less enforced separation early, far less scaffolding; the
split can happen later from evidence instead of up-front speculation.

**Applies to** — `repository-layout.md`, every `architecture/*` doc.

## Keep the CPU reference tiny and disposable

**Decision** — A CPU implementation exists only as a small correctness/reference tool
(indexing math, MAC layout, divergence/CG sanity tests). It is not a parallel
production simulator and may be frozen or dropped if it slows the GPU path.

**Why** — The product path is GPU; maintaining two full simulators would double
complexity. The CPU reference earns its keep purely as host-testable algorithm sanity.

**Code anchors** — host reference + tests in `app/crates/fluid-lab/src/sim/mod.rs` and
`app/crates/fluid-lab/src/sim/pressure.rs`.

**Applies to** — `architecture/simulation.md`, `architecture/pressure-solver.md`,
`agent-context/testing.md`.

## React is optional scaffolding, not a core dependency

**Decision** — The simulator does not require React. The web shell is minimal
TypeScript/HTML; React may wrap the shell only if the surrounding portfolio site
already uses it.

**Why** — The hard part is Rust/WASM/WebGPU; a frontend framework must not become a
second project.

**Applies to** — `architecture/web-shell.md`.

## The verified web entry path is the no-bundler static path

**Decision** — The canonical, verified front-end is the no-bundler static path
(`web/index.html` + `web/main.js` + `web/panels.js`, served by a plain static
server). The shell is named `index.html` so the bare `/` serves it under any server,
with no `/`-remap. The old Vite/TS entry (`web/src/main.ts`) is an orphaned stub that
nothing loads; do not verify against it.

**Why** — npm/Vite has been unreliable on the build machine, and the static path is
the one that actually carries the rendered panels and is exercised by the capture
harness. Reconciling the two paths is deferred polish.

**Applies to** — `architecture/web-shell.md`, `agent-context/build-run.md`.

## See also

- [`../architecture/web-shell.md`](../architecture/web-shell.md) · [`../architecture/gpu-resources.md`](../architecture/gpu-resources.md)
- [`scope.md`](scope.md)
- [`../agent-context/maintaining-docs.md`](../agent-context/maintaining-docs.md)
