---
status:        active
owner:         adamg
last_updated:  2026-06-05
okay_to_delete: false
long_lived:    true
---

# App Shell

The app shell owns the WASM/JS boundary, the per-frame dispatch loop, the fixed-timestep accumulator, the orbit camera, all interactive tank-transform modes, and the typed scene-config object that seeds the default simulation.

## What it owns

- **WASM entry struct** — `app/crates/fluid-lab/src/lib.rs → FluidApp` (wasm-bindgen exported). One instance per canvas; TypeScript constructs it async and drives it via `rAF`.
- **Frame entry point** — `app/crates/fluid-lab/src/lib.rs → FluidApp::frame`. Receives raw browser rAF delta (ms), hands it to the timestep controller, calls `gpu.step(n)`, then renders.
- **Fixed-timestep accumulator** — `app/crates/fluid-lab/src/timestep.rs → TimestepController`. Pure Rust, unit-tested natively.
- **Orbit camera** — `app/crates/fluid-lab/src/camera.rs → OrbitCamera`. Quaternion-based yaw/pitch; no up-vector clamp.
- **Tank transform** — `box_orient: glam::Quat` + `box_pos: glam::Vec3` fields on `FluidApp`; mutated by the pointer-mode methods.
- **JS↔WASM bridge** — `app/crates/fluid-lab/src/lib.rs → FluidApp::config_json`, `FluidApp::set_setting`, `FluidApp::stats_json`.
- **Scene config** — `app/crates/fluid-lab/src/scene/mod.rs → SceneConfig`, `InitialLiquidConfig`, `LiquidBlock`, `ScenePreset`.
- **Run state** — `app/crates/fluid-lab/src/lib.rs → RunState` (Running / Paused); `pending_steps` counter for single-step-while-paused.

## Frame loop

```
rAF delta_ms (TS)
  └─ FluidApp::frame
       ├─ timestep.steps_for_frame(clamped_s) → n
       ├─ gpu.step(n)            [sim substeps]
       ├─ gpu.render(view_proj, billboard_basis, eye_local)
       └─ profiler.end_frame_and_maybe_log
```

TypeScript owns `requestAnimationFrame`; Rust owns all scheduling. TS never drives sim frames independently.

## Timestep accumulator

`app/crates/fluid-lab/src/timestep.rs → TimestepController::steps_for_frame`:

1. Clamps incoming render dt to `MAX_RENDER_DT_S` = 1/30 s before accumulation — a single browser hitch cannot produce unbounded sim work.
2. Drains the accumulator in `fixed_dt` (default 1/120 s) chunks.
3. Runs `n = min(n_natural, max_substeps)` substeps. `max_substeps` defaults to 1 (see `decisions/performance.md`). If `n_natural > max_substeps` (behind), the entire remaining accumulator is zeroed this frame and the dropped seconds are added to cumulative `dropped_time`. The browser catches up by rendering the next frame, not by making one frame longer.

Each call records a `TimestepFrameStats` snapshot (`substeps`, `accumulated_before`, `accumulated_after`, `dropped_this_frame`). Accessors: `last_stats()` returns the per-frame snapshot; `total_dropped()` (and legacy `dropped_time()`) returns the cumulative total. `FluidApp::frame` pushes both into the profiler via `set_timestep_stats(...)`.

When the sim is paused, `FluidApp::frame` calls `timestep.reset()` each frame so no stale time bursts on resume. `reset()` zeroes both the accumulator and `last` (so paused frames report 0 substeps / 0 dropped; cumulative `dropped_time` is preserved). On hard reset (`FluidApp::reset`) the controller is fully reconstructed from the registry.

## Camera and pointer modes

`app/crates/fluid-lab/src/camera.rs → OrbitCamera` uses a quaternion orientation (yaw about world-Y, then pitch about local-X, then roll about local-Z) so pitch is unclamped — the camera can orbit fully over the top without a `look_at` pole degeneracy. `OrbitCamera::billboard_basis` returns the camera-facing right/up pair used by both particle billboards and all box-relative pointer operations. `OrbitCamera::set_distance` clamps to `[2, 40]`; `create()` and `reset()` restore pitch/yaw/roll/distance from the `camera.rot_x/rot_y/rot_z` and `camera.distance` registry settings (all Live-class), so the camera sliders define the default view.

The five wasm-exported pointer modes on `FluidApp` (`move_box` is exported but the web UI binds only four interaction modes — camera/rotate/rotateRoll/slosh; see `web-shell.md`):

| Method | Effect |
|---|---|
| `camera_orbit(dx,dy)` | Orbit camera around tank |
| `rotate_box(dx,dy)` | Spin tank about camera up + tip about camera right |
| `rotate_box_roll(dx,dy)` | Roll tank about camera view-fwd + tip about camera right |
| `move_box(dx,dy)` | Translate tank in camera screen plane |
| `slosh_box(dx,dy)` | Translate tank + apply opposite local-frame impulse to fluid |

## Scene config

`app/crates/fluid-lab/src/scene/mod.rs → SceneConfig::from_settings` builds the scene from the registry at construction and again on every `reset()` call. Liquid geometry is described as one or more `LiquidBlock` AABBs in normalized tank space [0,1]^3; the blocks stay normalized and are mapped to world space via the per-axis tank origin+extent (see "Rectangular tank" below) — no hardcoded 2.0/-1.0. The named presets are the variants of `app/crates/fluid-lab/src/scene/mod.rs → ScenePreset` (`FallingBlob`, `DamBreak`, `DoubleSplash`); the preset integer is the wire value of the `scene.preset` registry setting. `SceneConfig::default_tank` is the historical alias that always returns `FallingBlob`, used by callers that want the canonical look independent of the dropdown.

`SceneConfig.grid_resolution` is a `UVec3` built from the `grid.res_x/res_y/res_z` registry settings (all Reset-class), feeding the per-axis cell counts.

## Rectangular tank

The tank is a rectangular box, not a fixed cube. The cell size is a single **uniform** scalar `h = sim::H = 2.0/64.0` (`app/crates/fluid-lab/src/sim/mod.rs → H`); the box becomes rectangular only by varying the per-axis cell counts `nx/ny/nz` (the `grid.res_x/y/z` settings) independently. The domain is centered: per-axis `extent = n_axis * h`, `origin = [-nx*h/2, -ny*h/2, -nz*h/2]`. An all-64 grid reproduces the exact original `[-1,1]^3` cube. The GPU side owns the world placement (`app/crates/fluid-lab/src/gpu/fluid.rs → GpuFluid::new`, `tank_bounds`); `simulation.md` owns the grid-indexing contract.

Particle placement within each block uses a deterministic seeded jitter (`app/crates/fluid-lab/src/gpu/` — see the GPU init path), so `reset()` is bit-reproducible.

## Non-obvious invariants and gotchas

**Gravity follows tank orientation.** `push_gravity` (`app/crates/fluid-lab/src/lib.rs → FluidApp::push_gravity`) converts the world-down vector into the tank's local frame via `box_orient.inverse() * world_g` and sends it to the GPU. Every rotation and reset calls `push_gravity`. If you mutate `box_orient` without calling `push_gravity`, the GPU gravity vector is stale.

**Reset restores the full pose.** `FluidApp::reset` sets `box_orient = Quat::IDENTITY`, `box_pos = Vec3::ZERO`, reconstructs `OrbitCamera::new()` then restores its pitch/yaw/roll **and distance** from the `camera.*` settings, and finally calls `push_gravity`. A Reset always returns the tank to upright, centered, untilted, with the settings-defined camera pose — gravity points straight down again.

**Accumulator must be zeroed on pause.** The frame loop calls `timestep.reset()` every paused frame. Forgetting this would let the accumulator silently fill during a pause and burst multiple substeps on resume.

**`set_setting` has two return semantics.** Returns `true` only for `Live`-class settings (change applied to GPU immediately). Returns `false` for `Reset`- and `Reload`-class settings, meaning the registry was updated but the caller must prompt the user to reset/reload. The web panel uses this return value to show the hint badge.

**`box_pos` is clamped.** `move_box` and `slosh_box` both clamp `box_pos` to `[-3, 3]^3` so the tank cannot escape the camera frustum entirely.

**The mesh shader needs the camera eye in tank-local space.** `FluidApp::frame` computes `eye_local = box_orient⁻¹·(camera.eye − box_pos)` and passes it to `gpu.render`; the MC vertices live in tank-local space, so a world-space eye would make view-dependent shading flash as the tank moves. `OrbitCamera::eye` is the world eye. (`simulation.md`/`rendering.md` own the downstream use.)

**`dropped_time` is cumulative since last hard reset**, not a per-frame value. The per-frame drop is in `TimestepFrameStats.dropped_this_frame` (seconds). Both are surfaced through the profiler / `stats_json` and are useful for detecting sustained frame-rate overload.

**`#![allow(dead_code)]`** is intentional on the crate — many registry and scene fields belong to the forward-looking data model and are not yet read. Do not remove it.

## Update when

- The JS↔WASM bridge surface changes (new exported methods, new `set_setting` ids, changed JSON schemas for `config_json`/`stats_json`).
- The timestep constants (`MAX_RENDER_DT_S`, `fixed_dt`, `max_substeps`) are made configurable, their defaults change, or the drop-excess policy changes (currently: zero accumulator when capped).
- A new pointer mode is added or an existing mode's semantics change.
- A new `ScenePreset` variant is added or the normalized-space block definitions change.
- The tank stops being a uniform-`h` rectangular box (e.g. per-axis cell sizes), or the centered-origin placement changes.
- `OrbitCamera` gains/loses a settings-restored field, or `reset()` stops restoring camera distance.
- `push_gravity` logic changes (e.g., if gravity magnitude becomes a vector setting rather than a scalar).
- `reset()` no longer restores camera/box to defaults.

## See also

- `simulation.md` — GPU solver and substep internals that `gpu.step(n)` drives
- `gpu-resources.md` — WebGPU surface, pipeline, and `GpuContext` ownership
- `../decisions/platform.md` — why TypeScript owns rAF and Rust owns scheduling
- `../decisions/scope.md` — the typed scene config / scenarios-are-later rationale
- `../decisions/performance.md` — fixed-dt and substep-cap rationale
- `../agent-context/maintaining-docs.md`
