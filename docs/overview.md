# System overview

**fluid-lab** is a browser-native, observability-first 3D fluid lab: a bounded-tank
hybrid particle-grid (FLIP/PIC) liquid simulation that runs on the GPU via WebGPU and
exposes its internal pipeline — particles, MAC grid, divergence, pressure, velocity,
and liquid-cell slices — as inspectable render modes with a live config panel and a
real-GPU-timestamp profiler. It is one Rust crate compiled to WASM plus a thin
TypeScript shell.

## Shape

```
 Browser tab
 ┌─────────────────────────────────────────────────────────────┐
 │  web shell (thin TS/HTML, no framework)                      │
 │    index.html + main.js + panels.js   ← canonical path       │
 │    config panel · profiler panel   (rendered from the bridge)│
 │                    │  config_json / set_setting / stats_json │
 │                    ▼                                          │
 │  WASM module (fluid-lab crate)                               │
 │    lib.rs  FluidApp — rAF frame loop, pointer modes (orbit/rotate/roll/slosh), bridge  │
 │     │  timestep accumulator (fixed dt=1/120, ≤4 substeps)    │
 │     ▼                                                        │
 │    per substep:  clear → mark/classify → P2G (i32 atomics)  │
 │       → normalize → forces → boundaries → divergence        │
 │       → CG pressure solve → subtract ∇p → boundaries        │
 │       → G2P (PIC/FLIP blend) → advect → recover             │
 │     │                                                        │
 │     ├─ settings  typed config registry (apply classes)      │
 │     ├─ profiler  hierarchical, config-tagged, timing-honest │
 │     └─ gpu       wgpu device · SoA buffers · WGSL pipelines │
 │                    │                                         │
 │                    ▼  render (GPU-native, no frame readback)│
 │            tank · particles · grid slice                      │
 └─────────────────────────────────────────────────────────────┘
        ▲ verified out-of-band by tools/capture.mjs (real Chrome GPU)
```

## What's in the stack

The whole stack lives under `app/` (the `code_root`); every code path in the docs is
relative to it. (The Rust crate manifest is `app/Cargo.toml`; its source is `app/crates/fluid-lab/src/`.)

- **Rust crate `fluid-lab`** (`app/crates/fluid-lab/src/`): one crate, modules `sim` (MAC grid + host
  reference math), `gpu` (wgpu resources, the GPU sim loop, renderers, timing),
  `scene` (typed scene config), `settings` (config registry), `profiler`, plus
  `lib.rs` (the WASM app + frame loop) and `camera.rs` / `timestep.rs`. WGSL compute &
  render shaders live in `app/crates/fluid-lab/src/gpu/shaders/`.
- **Web shell** (`app/web/`): thin TypeScript/HTML. The verified path is the no-bundler
  static path (`index.html` + `main.js` + `panels.js`); an orphaned Vite/TS stub
  (`src/main.ts`) exists but nothing loads it.
- **Capture harness** (`app/tools/capture.mjs`): headless real-GPU Chrome that writes a
  screenshot + page console — the one acceptance signal that can't be faked.

## The representations

| Representation | Is | Doc |
|---|---|---|
| Particles | 3D points carrying liquid mass + free-surface motion | [simulation](architecture/simulation.md) |
| MAC grid | staggered face velocities + cell-centered pressure/divergence/type | [simulation](architecture/simulation.md) · [pressure-solver](architecture/pressure-solver.md) |
| Liquid-cell slice | GPU-native cross-section through cell type, pressure, or speed | [rendering](architecture/rendering.md) |

## Hard-to-grep facts

- **Builds must run inside WSL** (`wsl.exe -d Ubuntu-24.04 -- bash -lc '…'`); the
  agent's shell is Windows over the `\\wsl.localhost\` share. See
  [agent-context/build-run.md](agent-context/build-run.md).
- Toolchain: **wgpu 29 · wasm-pack 0.15 · rustc/cargo ~1.95 · node 20 (WSL) / 24
  (Windows)**.
- Tank is a **rectangular box** of independent per-axis cell counts (`grid.res_x/y/z`,
  default **64** each → the `[-1,1]³` cube) at a **uniform** cell size `h = 2/64`;
  all-equal counts give a cube, unequal counts give a rectangular box (~254k particles
  at the default). Default pressure solve: **CG, 30 iters**. Default G2P blend is
  high-FLIP (~0.9) for lively splash.
- **No float atomics in WebGPU** → P2G is fixed-point `i32` atomics at `FIXED_SCALE =
  2^16`; the accumulate→normalize path must stay integer or determinism breaks.
- **naga drops unused bindings** → every compute shader references `params` (binding 0)
  or uses an explicit bind-group layout.
- `maxStorageBuffersPerShaderStage` is commonly ~8–10 → the MAC loop is split into many
  small passes.

## Where to go next

- The simulation loop: [architecture/simulation.md](architecture/simulation.md).
- The pressure solve: [architecture/pressure-solver.md](architecture/pressure-solver.md).
- GPU resources & buffer layout: [architecture/gpu-resources.md](architecture/gpu-resources.md).
- Subsystem facts: [architecture/index.md](architecture/index.md).
- Design rationale: [decisions/index.md](decisions/index.md).

## See also

- [index.md](index.md) — global router.
- [repository-layout.md](repository-layout.md) — file inventory.
