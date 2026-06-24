# Repository layout

One sentence per non-trivial directory. Use this to find where a thing lives without
grepping. Architecture docs explain what the things *are*; this doc just maps paths.

## Repo root

The repository root contains:

```
<repo>/
  app/       The whole stack (Rust workspace + WGSL shaders + web shell + capture harness).
  docs/      This documentation tree (agent-docs v1). See index.md.
  media/     Durable README/demo media tracked with the repository.
  captures/  Capture-harness screenshot and console output (generated).
```

The workflow machinery (doc-authoring rules, the maintenance/chat skills) is global,
not in the repo — see [`agent-context/maintaining-docs.md`](agent-context/maintaining-docs.md).

## `app/` at a glance

The whole stack lives under `app/` (the `code_root`); every code path in the docs is
relative to it. `app/Cargo.toml` is the Cargo workspace manifest. The crate manifest
lives at `app/crates/fluid-lab/Cargo.toml` (package `fluid-lab`, edition 2021) and its
source is under `app/crates/fluid-lab/src/`.

```
app/
  Cargo.toml            Cargo workspace manifest.
  Cargo.lock            Locked deps. Edit only via cargo.

  crates/
    fluid-lab/          The fluid-lab crate.
      Cargo.toml        Crate manifest (package fluid-lab, edition 2021).
      src/
        lib.rs          WASM app: FluidApp, rAF frame loop, JS bridge, pointer modes.
        timestep.rs     Fixed/clamped timestep accumulator.
        camera.rs       Orbit camera.
        sim/            CPU reference + MAC types: indexing, classification,
                        pressure (CG host ref), wall-aware gather tests. Host-tested.
        gpu/            wgpu resources + the GPU simulation loop + renderers + timing.
          mod.rs        Device/surface, adapter limits, buffer layout, recreate path.
          fluid.rs      GpuFluid: buffers + the per-substep pass recorders.
          renderer.rs   Scene/debug render orchestration.
          particles.rs  Particle render mode.
          slice.rs      GPU-native grid-slice inspection.
          timing.rs     GPU timestamp-query handling.
          smoke.rs      Boot compute/atomic/timestamp smoke test.
          shaders/      WGSL compute + render shaders (scatter, normalize, cg_*, particles, slice, …).
        scene/          Typed SceneConfig + default scene initialization.
        settings/       Typed config registry + apply classes (the data model).
        profiler/       Hierarchical, config-tagged, timing-honest profiler.

  web/                  Thin TS/HTML web shell.
    index.html          The canonical no-bundler page (served at the bare /).
    main.js             Canonical bootstrap: mounts the wasm, wires panels.
    panels.js           Rendered config + profiler side panels (from the WASM bridge).
    pkg/                wasm-pack output (generated; fluid_lab_bg.wasm + glue).
    src/main.ts, vite.config.ts   Orphaned Vite/TS stub (nothing loads it; do not verify against).

  tools/
    capture.mjs         Headless real-GPU Chrome screenshot + console capture.
```

## See also

- [`index.md`](index.md) — global router.
- [`architecture/index.md`](architecture/index.md) — what these dirs *do*.
- [`agent-context/build-run.md`](agent-context/build-run.md) — how to build/run/verify.
