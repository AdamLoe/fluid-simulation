# Agent-docs manifest — fluid-lab

App-specific bindings for the global agent-docs kit. The generic skills and rules in
`~/.claude/agent-docs/<agent_docs_version>/` read the slots below; everything
app-specific lives here, nothing generic does.

```yaml
agent_docs_version: v1
repo_name: fluid-lab — browser-native Rust/WASM/WebGPU 3D fluid simulation lab
code_root: app/
```

> **Roots.** Agent-docs v1 fixes the docs root at `docs/` (repo top level).
> `code_root` is `app/`: every code path in the docs resolves under it. The repo root
> contains exactly `docs/` and `app/`. `app/` is a Cargo workspace; the Rust crate
> lives at `app/crates/fluid-lab/` (manifest) with source under `app/crates/fluid-lab/src/`,
> so a Rust anchor like `crates/fluid-lab/src/gpu/fluid.rs → record_pressure` resolves
> to `<repo>/app/crates/fluid-lab/src/gpu/fluid.rs`; web anchors (`web/panels.js`) and
> tool anchors (`tools/capture.mjs`) resolve under `code_root` directly.

## Slot: decisions-domains

`simulation`, `pressure`, `rendering`, `performance`, `observability`, `platform`,
`scope`. (Authoritative list: `ls docs/decisions/`.)

## Slot: drift-gates

Per-commit gates — all run inside WSL (see `agent-context/build-run.md`):

- `wsl.exe -d Ubuntu-24.04 -- bash -lc 'cd /home/adamg/fluid-simulation/app && cargo build --target wasm32-unknown-unknown'` — WASM compile check.
- `wsl.exe -d Ubuntu-24.04 -- bash -lc 'cd /home/adamg/fluid-simulation/app && cargo test --lib'` — host reference tests (sim math, CG, settings schema, wall-aware gather).
- For any change that alters visible/GPU behaviour: a browser capture via
  `tools/capture.mjs` against the static path (the acceptance signal that can't be
  faked). Never make a performance claim without profiler output.

## Slot: change-to-doc

The table to consult before declaring a change "done." Maps "changed file X → update
doc Y." Code paths are relative to `code_root` (`app/`).

| If you changed… | Update… |
|---|---|
| `app/crates/fluid-lab/src/lib.rs` (frame loop, pointer modes, JS bridge dispatch) | `architecture/app-shell.md`; `architecture/settings.md` if the `config_json`/`set_setting`/`stats_json` surface changes; `architecture/rendering.md` if render dispatch changes |
| `app/crates/fluid-lab/src/timestep.rs`, `app/crates/fluid-lab/src/camera.rs` | `architecture/app-shell.md` |
| `app/crates/fluid-lab/src/scene/mod.rs` (SceneConfig, default scene) | `architecture/app-shell.md`; `decisions/scope.md` if scene/scenario policy changes |
| `app/crates/fluid-lab/src/sim/mod.rs` (indexing, classification, host reference) | `architecture/simulation.md` |
| `app/crates/fluid-lab/src/gpu/fluid.rs` (buffers, pass recorders, step sequence) | `architecture/simulation.md`; `architecture/pressure-solver.md` if `record_pressure` changes; `architecture/gpu-resources.md` if buffer layout changes |
| `app/crates/fluid-lab/src/gpu/shaders/{scatter,normalize,mark,classify,clear,forces,boundaries,g2p,gradient,divergence,save_vel,impulse}.wgsl` | `architecture/simulation.md` |
| `app/crates/fluid-lab/src/gpu/shaders/cg_*.wgsl`, `app/crates/fluid-lab/src/gpu/shaders/pressure.wgsl`, `app/crates/fluid-lab/src/sim/pressure.rs` | `architecture/pressure-solver.md`; `decisions/pressure.md` if the solver choice/convention changes |
| `app/crates/fluid-lab/src/gpu/mod.rs` (device, GpuCaps, buffer layout, recreate path), `app/crates/fluid-lab/src/gpu/smoke.rs` | `architecture/gpu-resources.md`; `decisions/performance.md` if the pass-split/SoA strategy changes |
| `app/crates/fluid-lab/src/gpu/{renderer,particles,slice,environment,composite,skybox}.rs`, `app/crates/fluid-lab/src/gpu/shaders/{particles,slice,environment,composite,skybox,env}.wgsl` | `architecture/rendering.md`; `architecture/settings.md` if `render.hero.*` controls change; `decisions/rendering.md` if a render-policy decision changes |
| `app/crates/fluid-lab/src/gpu/diffuse.rs`, `app/crates/fluid-lab/src/gpu/shaders/diffuse_{emit,update,render}.wgsl` (foam/spray/bubble diffuse-water) | `architecture/rendering.md`; `architecture/gpu-resources.md` (buffer/memory); `architecture/settings.md` (the `render.diffuse.*` block); `decisions/rendering.md` if the whitewater policy changes |
| `app/crates/fluid-lab/src/gpu/wetwall.rs`, `app/crates/fluid-lab/src/gpu/wallfill.rs`, `app/crates/fluid-lab/src/gpu/shaders/{wetwall_update,wallfill}.wgsl` (wet-wall field and dense wall-fill sheet) | `architecture/rendering.md`; `architecture/gpu-resources.md` (buffer/memory/recreate); `architecture/settings.md` (the `render.hero.wet_wall.*` / `render.hero.flat_water.*` blocks); `decisions/rendering.md` if the render-only wall cue policy changes |
| `app/crates/fluid-lab/src/profiler/mod.rs`, `app/crates/fluid-lab/src/gpu/timing.rs` | `architecture/profiler.md`; `decisions/observability.md` if the timing-honesty/threshold policy changes |
| `app/crates/fluid-lab/src/settings/mod.rs` (registry, ApplyClass, defaults, validation) | `architecture/settings.md`; `decisions/observability.md` if the apply-class policy changes |
| `web/*` (main.js, panels.js, index.html, the orphaned Vite/TS stub), `tools/capture.mjs` | `architecture/web-shell.md`; `agent-context/build-run.md` if the build/serve/verify flow changes |
| `app/Cargo.toml` deps or toolchain pins | `overview.md` (toolchain facts), `agent-context/build-run.md` |
| Repository directory layout (new/renamed dir) | `repository-layout.md` |
| A new/removed/re-routed architecture doc | `architecture/index.md`, and `_meta/ownership.json` |
| A new/removed/re-routed decisions domain | `decisions/index.md`, and `_meta/ownership.json` |
| A new procedural workflow doc, or a changed "when it applies" | `agent-context/index.md` and `docs/index.md` |
| A concept gets a new canonical owner, or a cross-doc ownership conflict appears | `_meta/ownership.json` (+ `ownership.md` if the prose pointer changes) |
| Plan lifecycle / status-metadata shape | `~/.claude/agent-docs/v1/plan-lifecycle.md` + `plan-template.md` (generic); `plans/index.md` only if app routing changes |

## Slot: drift-verification (high-risk surfaces for fix-docs-drift-all)

The doc-fix sweep verifies `path → symbol` pointers still resolve and spot-checks these
high-risk invariants against code:

- The P2G accumulate→normalize path is still **integer/fixed-point** (`scatter.wgsl` /
  `normalize.wgsl`, `FIXED_SCALE`) — a float reduction is a determinism-breaking
  contract change.
- No CPU/GPU readback on the normal render frame (only throttled diagnostics/captures).
- Every compute shader references `params` (binding 0) or uses an explicit BGL (naga
  unused-binding gotcha).
- The `ApplyClass` set (Live/Reset/Reload) in `app/crates/fluid-lab/src/settings/mod.rs`.
- The default pressure solver is CG (`app/crates/fluid-lab/src/gpu/fluid.rs → record_pressure`,
  `app/crates/fluid-lab/src/sim/pressure.rs → cg_solve`).
- The tank is a **rectangular box**: three independent per-axis cell counts
  `nx,ny,nz` (Reset-class settings `grid.res_x/y/z`, default 64 each → the exact
  original `[-1,1]³` cube) at a single **uniform** scalar cell size `h = 2/64`. The cell
  counts ride in `Params.gdim: vec4<u32> = [nx,ny,nz,0]`; the pressure operator stays
  isotropic. Not a fixed cube and not a single resolution scalar.
- Which render modes are actually wired: tank wireframe, particles, and optional grid slice.

## Notes

- The generic agent-docs kit (authoring rules, coding-style, repo-rules, orchestrating
  rules) lives at `~/.claude/agent-docs/v1/rules/`. The workflow commands are global
  skills in `~/.claude/skills/`. This manifest is the only app-specific binding the kit
  reads.
- `agent-context/maintaining-docs.md` and `ownership.md` are thin in-repo stubs kept so
  `See also` links resolve; the rules are global and the ownership data is
  `_meta/ownership.json`.
