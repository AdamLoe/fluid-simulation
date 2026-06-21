---
status:        active
owner:         adamg
last_updated:  2026-06-20
okay_to_delete: false
long_lived:    true
---

# App Shell

The app shell owns the WASM/JS boundary, the per-frame dispatch loop, the fixed-timestep accumulator, the orbit camera, all interactive tank-transform modes, and the typed scene-config object that seeds the default simulation.

## What it owns

- **WASM entry struct** — `app/crates/fluid-lab/src/lib.rs → FluidApp` (wasm-bindgen exported). One instance per canvas; the web shell constructs it async and drives it via `rAF`.
- **Frame entry point** — `app/crates/fluid-lab/src/lib.rs → FluidApp::frame`. Receives browser rAF delta (ms), sanitizes non-finite/negative input to a non-negative finite value, hands it to the timestep controller, calls `gpu.step(n)`, then renders.
- **Fixed-timestep accumulator** — `app/crates/fluid-lab/src/timestep.rs → TimestepController`. Pure Rust, unit-tested natively.
- **Orbit camera** — `app/crates/fluid-lab/src/camera.rs → OrbitCamera`. Quaternion-based yaw/pitch; no up-vector clamp.
- **Tank transform** — `box_orient: glam::Quat` + `box_pos: glam::Vec3` fields on `FluidApp`; mutated by cube pointer methods.
- **Interaction scheduler** — `app/crates/fluid-lab/src/lib.rs → InteractionState`; deterministic app-side auto-roll and wave-maker timing owned by `FluidApp`, not by JavaScript.
- **JS↔WASM bridge** — `app/crates/fluid-lab/src/lib.rs → FluidApp::config_json`, `FluidApp::set_setting`, `FluidApp::set_setting_result_json`, `FluidApp::stats_json`, `FluidApp::gpu_device_status`.
- **Scene config** — `app/crates/fluid-lab/src/scene/mod.rs → SceneConfig`, `InitialLiquidConfig`, `LiquidBlock`, `ScenePreset`.
- **Run state** — `app/crates/fluid-lab/src/lib.rs → RunState` (Running / Paused); `pending_steps` counter for single-step-while-paused.

## Frame loop

The web shell owns `requestAnimationFrame`; Rust owns the fixed sequence inside
`app/crates/fluid-lab/src/lib.rs → FluidApp::frame`. Each frame sanitizes the browser
delta, updates scheduled interactions while Running, asks
`app/crates/fluid-lab/src/timestep.rs → TimestepController::steps_for_frame` for the
substep count, runs
`gpu.step(n)`, renders, and then closes the profiler/timing frame. JavaScript never
drives sim steps independently.

## Timestep accumulator

`app/crates/fluid-lab/src/timestep.rs → TimestepController::steps_for_frame` receives only finite, non-negative seconds from `FluidApp::frame`:

1. Clamps incoming render dt to `MAX_RENDER_DT_S` = 1/30 s before accumulation — a single browser hitch cannot produce unbounded sim work.
2. Drains the accumulator in `fixed_dt` (default 1/120 s) chunks.
3. Runs `n = min(n_natural, max_substeps)` substeps. `max_substeps` defaults to 2 (see `decisions/performance.md`), so an ordinary 60 Hz frame can execute the two 1/120 s physics steps it naturally wants when frame budget allows. If `n_natural > max_substeps` (behind), the entire remaining accumulator is zeroed this frame and the dropped seconds are added to cumulative `dropped_time`. The browser catches up by rendering the next frame, not by making one frame longer.

Each call records a `TimestepFrameStats` snapshot: executed `substeps`, `fixed_dt`, `max_substeps`, `natural_substeps`, whether the cap hit, accumulator before/after, per-frame dropped time, actually advanced sim time, raw sanitized rAF wall time, real-time factor, and the policy label. The real-time factor is `sim_advanced / raw_rAF_wall_dt`; dropped time is not counted as advanced simulation time. Accessors: `last_stats()` returns the per-frame snapshot; `total_dropped()` (and legacy `dropped_time()`) returns the cumulative total. `FluidApp::frame` pushes both into the profiler via `set_timestep_stats(...)`.

When the sim is paused, `FluidApp::frame` calls `timestep.reset()` each frame so no stale time bursts on resume. Scheduled interaction time does not advance while paused; single-step while paused advances the sim tick but does not run auto-roll or wave-maker scheduling. Paused idle frames record zero executed substeps with the raw rAF wall time; paused single-step records one manual substep so profiler stats match the actual `gpu.step(1)` call. `reset()` zeroes both the accumulator and `last` (so paused frames report 0 substeps / 0 dropped until the frame records idle/manual stats; cumulative `dropped_time` is preserved). On hard reset (`FluidApp::reset`) the controller is fully reconstructed from the registry.

## Camera and pointer methods

`app/crates/fluid-lab/src/camera.rs → OrbitCamera` uses a quaternion orientation (yaw about world-Y, then pitch about local-X, then roll about local-Z) so pitch is unclamped — the camera can orbit fully over the top without a `look_at` pole degeneracy. `OrbitCamera::billboard_basis` returns the camera-facing right/up pair used by both particle billboards and all box-relative pointer operations. `OrbitCamera::set_distance` clamps to `[2, 40]`; `create()` and `reset()` restore pitch/yaw/roll/distance from the `camera.rot_x/rot_y/rot_z` and `camera.distance` registry settings (all Live-class), so the camera sliders define the default view.

The web shell chooses Camera or Cube control, then dispatches pointer and keyboard
viewport input (see `web-shell.md`). `app/crates/fluid-lab/src/lib.rs` exports separate
camera and cube operations: Camera mode maps primary/middle/secondary drag, and the
matching keyboard chords, to `camera_orbit` / `camera_pan` / `camera_twist`; Cube mode
maps them to `rotate_box` / `move_box` / `rotate_box_roll`. `slosh_box` remains
exported for scripts, but the current bottom Control UI does not bind it.

## Scene config

`app/crates/fluid-lab/src/scene/mod.rs → SceneConfig::from_settings` builds the scene from the registry at construction and again on every `reset()` call. Liquid geometry is described as one or more `LiquidBlock` AABBs in normalized tank space [0,1]^3; the blocks stay normalized and are mapped to world space via the per-axis tank origin+extent (see "Grid handoff" below) — no hardcoded 2.0/-1.0. The named presets are the variants of `app/crates/fluid-lab/src/scene/mod.rs → ScenePreset` (`FallingBlob`, `DamBreak`, `DoubleSplash`); the preset integer is the wire value of the `scene.preset` registry setting. `SceneConfig::default_tank` is a helper that always returns `FallingBlob`, kept for any caller that wants the canonical look independent of the dropdown.

`scene.drop_height` is a Reset-class scene parameter consumed by `SceneConfig::from_settings`.
For suspended presets, the authored liquid blocks are shifted vertically by the height
delta and clamped inside `[0,1]` while preserving block size. Falling Blob is a
suspended central blob whose default normalized volume tracks the 20% fill default;
Dam Break remains floor-anchored, so the setting has limited effect there.

`SceneConfig.grid_resolution` is a `UVec3` built from the `grid.res_x/res_y/res_z` registry settings (all Reset-class), feeding the per-axis cell counts.

## Scheduled interactions

`FluidApp::update_interactions` owns automatic tank and wave motion. JavaScript only writes registry values through `set_setting`; it does not drive schedules.

`InteractionState` contains a lightweight deterministic PRNG and two schedules:

- Auto-roll chooses bounded random target orientations, smoothly interpolates `box_orient`, and calls `push_gravity` after each pose change. The camera is untouched.
- Wave maker emits periodic local horizontal velocity impulses through the existing GPU impulse path. The tank transform is unchanged by wave impulses.

The `interaction.auto_roll_*` and `interaction.wave_*` settings are Live-class. Defaults keep both enable toggles off; changing strength/cadence/frequency updates the next scheduled behavior without a rebuild. Reset restores the tank to upright/centered, rebuilds the fluid, and resets the deterministic schedules, but it does not change the Live setting values.

## Grid handoff

`SceneConfig.grid_resolution` is a `UVec3` built from the `grid.res_x/res_y/res_z`
registry settings (all Reset-class), and `FluidApp::create` / `FluidApp::reset` hand
that scene config into GPU creation/rebuild. The tank-grid geometry contract itself
is owned by `simulation.md`; the app shell owns only the handoff between
`SceneConfig::from_settings` and
`app/crates/fluid-lab/src/gpu/mod.rs → GpuContext::recreate_fluid`.

Particle placement within each block uses deterministic seeded jitter
(`app/crates/fluid-lab/src/gpu/fluid.rs → generate_particles`), so `reset()` is
bit-reproducible.

The web shell applies localStorage and URL `set` batches before starting the rAF
frame loop. Reset-class changes in those batches call `FluidApp::reset` synchronously,
so the first meaningful rendered frame uses the selected preset, fill level, density,
grid resolution, and derived particle count rather than a default-scene intermediate.

## Non-obvious invariants and gotchas

**Gravity follows tank orientation.** `push_gravity` (`app/crates/fluid-lab/src/lib.rs → FluidApp::push_gravity`) converts the world-down vector into the tank's local frame via `box_orient.inverse() * world_g` and sends it to the GPU. Every rotation and reset calls `push_gravity`. If you mutate `box_orient` without calling `push_gravity`, the GPU gravity vector is stale.

**Reset is staged around GPU rebuild success.** `FluidApp::reset` builds the new `SceneConfig` and calls `GpuContext::recreate_fluid` before mutating app-visible reset state. If GPU preflight rejects the requested scale, the current fluid, tick, schedules, pose, profiler window, and reset counter remain intact.
The method returns `false` for rejected resets and `true` after the rebuild and
app-state commit succeed; JS callers must not rerender/reset UI state as though a
failed reset applied.

**A successful reset restores the full pose.** After `recreate_fluid` succeeds, `FluidApp::reset` sets `box_orient = Quat::IDENTITY`, `box_pos = Vec3::ZERO`, reconstructs `OrbitCamera::new()` then restores its pitch/yaw/roll **and distance** from the `camera.*` settings, and finally calls `push_gravity`. A successful Reset always returns the tank to upright, centered, untilted, with the settings-defined camera pose — gravity points straight down again.

**Auto-roll is bounded tank motion, not camera spin.** The app generates target tank poses in Rust and clamps them by `interaction.auto_roll_strength`. It never changes `OrbitCamera`.

**Wave maker is an impulse tool.** It applies local X/Z particle velocity kicks via the existing impulse dispatch. It does not translate the tank, move a paddle, or create/delete particles.

**Accumulator must be zeroed on pause.** The frame loop calls `timestep.reset()` every paused frame. Forgetting this would let the accumulator silently fill during a pause and burst multiple substeps on resume.

**`set_setting` is the legacy bool bridge.** `app/crates/fluid-lab/src/lib.rs → FluidApp::set_setting` returns `true` only for accepted `Live`-class settings whose change applied immediately. Accepted `Reset`- and `Reload`-class settings still return `false`, meaning the registry stored the value but the caller must reset/reload. Non-finite input is rejected before the registry changes and also returns `false`. The shell prefers `FluidApp::set_setting_result_json` for honest status/clamping/reset details and falls back to the bool wrapper only for compatibility.

**Runtime status crosses the bridge as data, not console text.** `FluidApp::stats_json`
includes the current `gpu_device_status` alongside profiler facts, and
`FluidApp::gpu_device_status` remains the direct status method for the shell loop.
Status meanings are owned by `gpu-resources.md` and displayed by `web-shell.md`.

**`box_pos` is clamped.** `move_box` and `slosh_box` both clamp `box_pos` to `[-3, 3]^3` so the tank cannot escape the camera frustum entirely.

**`dropped_time` is cumulative since last hard reset**, not a per-frame value. The per-frame drop is in `TimestepFrameStats.dropped_this_frame` (seconds). Both are surfaced through the profiler / `stats_json` and are useful for detecting sustained frame-rate overload. `real_time_factor` uses raw rAF wall time as its denominator, so an ordinary 60 Hz frame with default `max_substeps=2` reports about `1.0x` when both 1/120 s substeps execute; capped hitches still report the lower factor instead of hiding dropped accumulator time.

**`#![allow(dead_code)]`** is intentional on the crate — many registry and scene fields belong to the forward-looking data model and are not yet read. Do not remove it.

## Update when

- The JS↔WASM bridge surface changes (new exported methods, new `set_setting` ids, changed JSON schemas for `config_json`/`stats_json`).
- The timestep constants (`MAX_RENDER_DT_S`, `fixed_dt`, `max_substeps`) are made configurable, their defaults change, or the drop-excess policy changes (currently: zero accumulator when capped).
- A pointer method or web-shell pointer mapping changes.
- Scheduled interaction behavior or `interaction.*` settings change.
- A new `ScenePreset` variant is added or the normalized-space block definitions change.
- The tank stops being a uniform-`h` rectangular box (e.g. per-axis cell sizes), or the centered-origin placement changes.
- `OrbitCamera` gains/loses a settings-restored field, or `reset()` stops restoring camera distance.
- `push_gravity` logic changes (e.g., if gravity magnitude becomes a vector setting rather than a scalar).
- `reset()` no longer restores camera/box to defaults.

## See also

- [`simulation.md`](simulation.md) — GPU solver and substep internals that `gpu.step(n)` drives
- [`gpu-resources.md`](gpu-resources.md) — WebGPU surface, pipeline, and `GpuContext` ownership
- [`../decisions/platform.md`](../decisions/platform.md) — why the web shell owns rAF and Rust owns scheduling
- [`../decisions/scope.md`](../decisions/scope.md) — the typed scene config / scenarios-are-later rationale
- [`../decisions/performance.md`](../decisions/performance.md) — fixed-dt and substep-cap rationale
- [`../agent-context/maintaining-docs.md`](../agent-context/maintaining-docs.md)
- [`~/.agentdocs/rules/authoring-rules.md`](~/.agentdocs/rules/authoring-rules.md)
