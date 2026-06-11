---
status:        active
owner:         adamg
last_updated:  2026-06-09
okay_to_delete: false
long_lived:    true
---

# GPU resources

`app/crates/fluid-lab/src/gpu/mod.rs` owns the WebGPU device/surface lifecycle and is the single entry point all other GPU subsystems pass through. `GpuContext` holds the device, queue, surface, depth texture, all sub-renderers, and the `GpuFluid` simulation state. Everything GPU-related is created here or handed a reference here.

At boot, `GpuContext::new` requests a `HighPerformance` adapter, clones the full adapter limits into `required_limits` (no artificial down-capping), probes `TIMESTAMP_QUERY` feature availability, then runs the `smoke::run_atomic_smoke_test` before any sim state is built. Adapter name, backend, key limits, and timestamp-query availability are written to the console by `log_boot_diagnostics` — first place to look when a user reports an unexpected behavior on a specific GPU.

## What it owns

- **`GpuCaps`** — probed-once struct: adapter name, backend, storage-stage/workgroup
  limits, `max_compute_workgroups_per_dimension`, `max_buffer_size`,
  `max_storage_buffer_binding_size`, and `timestamp_query`. It is authoritative for
  particle-scale preflight and boot diagnostics.
- **Device / surface / render targets** — created and owned here; resize re-configures
  the surface and recreates the shared `Depth32Float` texture, the screen-space water
  `R16Float` targets (`create_depth`, `create_r16_target`), and the hero-water scene
  prepass targets `scene_color` (`Rgba16Float`, `create_scene_color_target`) +
  `scene_depth` (`R16Float`, eye distance).
- **`GpuFluid`** (`app/crates/fluid-lab/src/gpu/fluid.rs`) — simulation buffer set and all compute pipelines; `GpuContext` drives it via `record_prep` / `record_pressure` / `record_finish`.
- **Sub-renderers** — `WireframeRenderer`, `EnvironmentRenderer`, `SkyboxRenderer`
  (`gpu/skybox.rs`, the world-background procedural sky — owns one small uniform, no
  fluid/tank dependency, so it is NOT rebuilt on recreate), `ParticleRenderer`,
  `CompositeRenderer`, `WaterSmoothRenderer`, `SliceRenderer`, `DiffuseSystem`
  (`gpu/diffuse.rs`, the render-only foam/spray/bubble particles — owns its own
  persistent particle + counter + uniform buffers), `CausticsSystem`
  (`gpu/caustics.rs`, half-res caustic ping-pong targets), `WetWallSystem`
  (`gpu/wetwall.rs`, the persistent supersampled per-wall-texel wetness buffer; rebuilt
  on recreate), `WallOccupancySystem` / `WallFillRenderer` (`gpu/wallfill.rs`, the dense
  current-frame wall-fill occupancy buffer + MRT injection renderer), and
  `TemporalSystem` (`gpu/temporal.rs`, full-res history ping-pong); renderers that
  need simulation data receive buffer handles from `GpuFluid` at construction, not raw
  device buffers. `GpuFluid::grid_dims()` and `GpuFluid::tank_bounds()` thread the
  per-axis cell counts and the world-space tank AABB into the renderers (the wireframe
  + environment geometry are sized from the tank AABB) on both construct and recreate.
- **`HeroParams` snapshot** — `RenderMode` and the flat `HeroParams` (mirrored from the
  `render.hero.*` registry settings) live on `GpuContext`. `set_hero_params` pushes the
  snapshot into the composite + environment + skybox uniforms; it is also re-applied after
  `recreate_fluid` rebuilds the environment. Per frame, `render` also pushes the
  camera-only eye→world rotation into the composite (`Cam` uniform, binding 8) and skybox
  so the reflected environment + skybox stay world-fixed under box rotation
  (`rendering.md`). No new swapchain-sized targets: the skybox writes into the existing
  `scene_color`/`scene_depth` prepass targets.
- **`GpuTimers`** (`app/crates/fluid-lab/src/gpu/timing.rs`) — wraps timestamp-query sets; `None` when the feature is absent. When present, constructed with `(max_substeps, detailed, pressure_iters)` from the registry so the `QuerySet` is sized at construction time.
- **Particle-scale preflight/status** — `GpuContext::new` and
  `GpuContext::recreate_fluid` validate the requested seeded particle count against the
  shared tiled particle-dispatch contract and the storage-binding limit before
  allocation/submission, then surface the result through `requested_particles`,
  `estimated_particles`, `scale_status`, `particle_dispatch_groups`, and
  `particle_dispatch_capacity`.

## Buffer layout and the per-stage storage-buffer budget

`GpuFluid` uses **structure-of-arrays** (SoA): each MAC face axis has its own separate `u32`/`f32` storage buffer rather than interleaved structs. Particle data is the one exception — particles are interleaved `{pos: vec4, vel: vec4}` (32 B each) in a single buffer. All grid buffers are `f32` or `i32`, element-count × 4 bytes, allocated once at construction and cleared each step on the GPU.

The tank is **rectangular**: a uniform cell size `crate::sim::H = 2/64` with independent per-axis cell counts `nx, ny, nz` (all-64 reproduces the original `[-1,1]³` cube). Buffer element counts derive from those counts, fixed at `GpuFluid::new` from `grid.res_x/res_y/res_z`: cells = `nx·ny·nz`; the staggered MAC face counts are `(nx+1)·ny·nz`, `nx·(ny+1)·nz`, `nx·ny·(nz+1)`. The `Params` uniform (`gpu/fluid.rs → Params`) is eight `vec4` = 128 B; the per-axis grid dims travel through an **appended** field `gdim: vec4<u32> = [nx, ny, nz, 0]` — appended at the end so prefix-mirroring shaders that don't decompose a cell index stay untouched, and only the decomposing shaders mirror it.

```
Particles         particles (interleaved pos+vel, 32 B/particle)

MAC face vels     u_vel / v_vel / w_vel          (face counts: (nx+1)·ny·nz etc.)
P2G accum         u_num / u_den / v_num / v_den / w_num / w_den
FLIP snapshot     u_saved / v_saved / w_saved
Pressure          pressure_a / pressure_b  (ping-pong; result always in pressure_a)
CG workspace      cg_d / cg_q / cg_partials / cg_scalars
Grid scalar       divergence / occupancy / cell_type / stats
```

The **hard constraint** is `maxStorageBuffersPerShaderStage`, which is commonly 8–10 on real WebGPU adapters. The MAC loop needs u/v/w face buffers, pressure ping-pong, divergence, cell-type, particles, P2G num/den accumulation — far more than 10 in aggregate. This is why the sim is decomposed into many small passes (clear, mark, classify, scatter×3, normalize×3, save×3, gravity×3, enforce×3, divergence, CG-init/spmv/reduce/alpha/update/beta/dir, gradient×3, g2p) each binding at most 6 storage buffers. This is a **layout constraint, not a performance optimization** — a single mega-pass would fail pipeline creation on most adapters. The `GpuFluid` doc comment states the ≤6 ceiling explicitly.

Bind groups are built once in `GpuFluid::new`; buffers never move after creation so the bind groups remain valid for the lifetime of the `GpuFluid` instance.

## Non-obvious invariants and gotchas

**naga drops unused bindings.** When a WGSL shader does not reference a binding, naga's reflection omits it from the auto-generated `BindGroupLayout`. If the Rust side builds a BGL from the pipeline's reflected layout and that BGL is then used to create a bind group that _does_ include the unused binding, the counts mismatch and pipeline creation fails silently. The fix is either to ensure every shader references `params` (binding 0) or to pass an explicit `BindGroupLayoutDescriptor` to `create_compute_pipeline`. Any new shader that adds a params uniform must actually read a field from it.

**Reset-class settings require buffer reallocation.** The per-axis grid resolutions `grid.res_x/res_y/res_z`, particle count, `fixed_dt`, `max_substeps`, `render.hero.wet_wall.supersample`, `render.hero.flat_water.fill_supersample`, and `dev.detailed_gpu_profiling` are baked into buffer sizes, uniforms, or timer layout at construction. Changing them requires calling `GpuContext::recreate_fluid`, which calls `GpuFluid::new` and rebuilds the `WireframeRenderer`, `EnvironmentRenderer`, `ParticleRenderer`, `SliceRenderer`, `DiffuseSystem`, `WetWallSystem`, and `WallOccupancySystem` from the new buffer handles (and re-applies the current `HeroParams` to the rebuilt environment; rebuilding `DiffuseSystem` clears its particles, rebuilding `WetWallSystem` zeroes the wetness field and rebinds it into the environment, and both wet-wall/wall-fill systems rebind the fresh sim buffers; temporal + caustics history is also dropped for a clean first frame). The swapchain-sized temporal/caustics ping-pong targets plus `wallfill_mask` are rebuilt on `resize`/`Outdated`, not here. `GpuTimers` is also rebuilt from the new `max_substeps` / mode / `pressure_iters`. The device, surface, and format are untouched. Live/tweak-class settings are written to uniforms or renderer state without a rebuild.

**Particle-linear work uses one shared tiled dispatch shape.**
`gpu/fluid.rs -> particle_dispatch_shape` is the contract for every particle-linear
pass: mark, scatter U/V/W, G2P, and the standalone impulse path all dispatch with
`@workgroup_size(64, 1, 1)` and the same `(groups_x, groups_y)` shape. The shader-side
particle index is `((workgroup_id.y * num_workgroups.x + workgroup_id.x) * 64) +
local_invocation_index`; partial tiles still guard `p >= particle_count`.

**Particle-scale preflight happens before allocation/submission.** `GpuContext::new`
and `GpuContext::recreate_fluid` compute the exact deterministic seeded count before
allocation and reject create/Reset when that count exceeds either the tiled dispatch
capacity or the single particle storage-binding limit. The tiled ceiling is
`min(max_compute_workgroups_per_dimension^2, floor(u32::MAX / 64)) * 64`, still
subject to the particle storage-binding limit. A rejected Reset preserves the running
fluid and exposes the requested, estimated, actual, and limiting values through
`stats_json`; `scale_status` distinguishes dispatch-capacity rejection from
storage-binding rejection.

The measured v1.8 scale matrix confirmed that this preflight/model split matters:
2,000,000 requested particles ran as `30,586 x 1 x 1`, 4,000,000 as `61,396 x 1 x 1`,
and 8,000,000 as `65,535 x 2 x 1` with `scale_status=ok`. The 8M row is evidence for
the legal tiled-dispatch ceiling only; it proves the app no longer fails on the old
common one-dimensional `65,535 x 64` workgroup limit, not that 8M is a practical
frame-time target.

**Memory accounting exposes active simulation buffers; water targets are render
memory.** `GpuContext::buffer_memory_bytes()` forwards `GpuFluid::buffer_memory_bytes()`,
so `stats_json.gpu_buffer_mb` is the simulation-buffer budget. Rendering also owns the
shared depth texture and four persistent swapchain-sized `R16Float` water targets:
thickness, speed-weighted whitewater, nearest-Z, and smoothed-Z, plus a transient ping
target shared by **all** the separable smoothing passes — the bilateral depth blur
(`WaterSmoothRenderer`) plus the plain-Gaussian thickness and whitewater blurs (two
`ThicknessSmoothRenderer` instances, each blurring its target in place in sequence, so
they allocate no extra target). The hero-water Water mode adds two more
swapchain-sized prepass targets: `scene_color` (`Rgba16Float`, ~8 bytes/px) and
`scene_depth` (`R16Float`). At 1280×800 the five R16 water targets are ~10 MB and the
two scene targets ~12 MB. There is still no extracted-surface vertex allocation.
The diffuse-water system (`gpu/diffuse.rs -> DiffuseSystem`) owns one persistent
particle storage buffer at a **fixed capacity** (`DIFFUSE_CAPACITY`, 48 B/particle
≈ 12.6 MB) plus a small counters buffer; `render.diffuse.max_particles` is an active
cap within that capacity (Live, no realloc).

The hero-water finishing systems own three more render-memory allocations, all GPU-only
(no readback) and none counted in `gpu_buffer_mb`:

- **Caustics** (`gpu/caustics.rs -> CausticsSystem`) — a **half-res** `R16Float`
  ping-pong pair (the working caustic map + its temporal history); ≈ 1 MB total at
  1280×800. The composite/composite-bind groups read the freshly-written half each frame.
- **Wetness** (`gpu/wetwall.rs -> WetWallSystem`) — one flat `f32` `STORAGE | COPY_DST`
  buffer, supersampled by `render.hero.wet_wall.supersample` per wall axis:
  back wall `(nx*ss)·(ny*ss)` + left wall `(nz*ss)·(ny*ss)` + floor
  `(nx*ss)·(nz*ss)` concatenated. At default 64³ and `ss=8`, that is ~786k texels × 4 B
  ≈ 3.0 MB; at selectable max `ss=32`, ~12.6M texels ≈ 48 MB. Shares a small
  `WetWallUniform` (allocation-time supersampled dims + tank bounds + strengths) with
  `environment.wgsl` so the wall FS and update pass map world position → texel
  identically. Rebuilt (zeroed) on `recreate_fluid`.
- **Wall fill occupancy** (`gpu/wallfill.rs -> WallOccupancySystem`) — one flat `f32`
  storage buffer with dense current-frame wall occupancy, supersampled by
  `render.hero.flat_water.fill_supersample`: back/front `(nx*ss)·(ny*ss)` each,
  left/right `(nz*ss)·(ny*ss)` each, plus floor `(nx*ss)·(nz*ss)`. At default 64³ and
  `ss=16` this is ~7.34M entries × 4 B ≈ 28 MB; at selectable max `ss=32`,
  ~29.4M entries ≈ 112 MB. It is recomputed from near-wall particle splats every frame
  while wall fill is enabled, using a 2D workgroup split when the flattened pass would exceed WebGPU's
  per-dimension dispatch ceiling, and is rebound when `recreate_fluid` creates fresh sim
  buffers. The wall-fill render pass samples the back and left atlas faces only, matching
  the rendered glass faces in `rendering.md`, and also clears/writes one
  swapchain-sized `R16Float` `wallfill_mask` target each frame; the composite samples it
  for fill-only optical controls.
- **Temporal** (`gpu/temporal.rs -> TemporalSystem`) — true **full-res** `R16Float`
  ping-pong (two stabilized textures) per enabled target: `thickness` + `smooth_z`
  (default-on) ≈ **8 MB**, plus `whitewater` ≈ **4 MB** when `foam_history` is on, at
  1280×800 — the series' largest memory add. Caustics history is the existing half-res
  pair (no extra full-res caustic ping-pong). The camera-reset metric needs no GPU buffer:
  `prev_eye_to_world` is a CPU `Mat4` on `GpuContext`, diffed each frame into a scalar
  pushed through the temporal uniform.

**No per-frame readbacks.** The only allowed readback is the smoke test (one-shot at boot, in `smoke::run_atomic_smoke_test`) and the throttled timing + liveness readback driven by `GpuTimers::record_resolve_and_maybe_copy`. Normal sim steps submit compute encoders and return; no `map_async` on hot paths.

**P2G uses integer atomics, not float.** WebGPU has no `atomicAdd` for floats. P2G scatter accumulates into `i32` buffers via fixed-point scaling (`FIXED_SCALE = 65536.0`); the normalize pass converts back to `f32`. The smoke test in `smoke::run_atomic_smoke_test` validates that `u32` `atomicAdd` works correctly before any sim state is built.

**Timestamp-query is optional.** `GpuTimers` is `Option<timing::GpuTimers>`; all timing paths guard on `self.timers.as_ref()`. If the adapter does not expose `TIMESTAMP_QUERY`, GPU timing readouts return `None` and the profiler falls back to a minimum-honest estimate.

**Surface format selection.** The surface format is the first sRGB format the adapter supports, falling back to the adapter's first supported format. `DEPTH_FORMAT` is `Depth32Float`, a module-level constant in `app/crates/fluid-lab/src/gpu/mod.rs`.

## Update when

- A new compute pass is added to `GpuFluid` — update the buffer layout diagram and verify the ≤6 storage-buffer ceiling is maintained.
- `GpuCaps` fields change — update "What it owns."
- A new Reset-class setting is added — note it in the reallocation invariant. (The per-axis `grid.res_x/res_y/res_z` and `dev.detailed_gpu_profiling` are already Reset-class.)
- The adapter limit floor changes (e.g., if a WebGPU spec update guarantees ≥10) — update the constraint description.
- `GpuTimers` construction parameters change — update "What it owns" above; `profiler.md` owns the timing readout shape.
- The shared particle dispatch contract changes (workgroup size, tiling formula, or
  preflight status names).
- Screen-space water target formats or recreation rules change — update render-target
  ownership and memory notes here, and the pass order in `rendering.md`.

## See also

- `simulation.md` — MAC loop pass sequence and FLIP/PIC blend semantics.
- `pressure-solver.md` — CG solver pass breakdown (`cg_init` → `cg_spmv` → reduce → update cycle).
- `rendering.md` — `WireframeRenderer`, `ParticleRenderer`, and `SliceRenderer` internals.
- `profiler.md` — `GpuTimers` throttled readback and timing readout structure.
- `../decisions/performance.md` — rationale for SoA layout and pass-splitting strategy.
- `../agent-context/maintaining-docs.md` — doc maintenance rules.
