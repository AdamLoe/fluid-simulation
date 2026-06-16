---
status:        active
owner:         adamg
last_updated:  2026-06-15
okay_to_delete: false
long_lived:    true
---

# GPU resources

`app/crates/fluid-lab/src/gpu/mod.rs` owns the WebGPU device/surface lifecycle and is
the entry point for GPU subsystems. `GpuContext` holds the device, queue, surface,
render targets, sub-renderers, timers, caps, and `GpuFluid` simulation state.

## What it owns

- `GpuCaps` - probed-once adapter limits and timestamp-query availability.
- Device/surface/render targets - shared `Depth32Float`, water `R16Float` targets,
  hero-water `scene_color` (`Rgba16Float`) and `scene_depth` (`R16Float`).
- `GpuFluid` - simulation buffers and compute pipelines.
- Sub-renderers - `WireframeRenderer`, `EnvironmentRenderer`, `SkyboxRenderer`,
  `ParticleRenderer`, `CompositeRenderer`, `WaterSmoothRenderer`,
  `ThicknessSmoothRenderer`, `SliceRenderer`, and `DiffuseSystem`.
- `HeroParams` - one Live snapshot pushed to composite, environment, skybox, and
  smoothing state.
- `GpuTimers` - optional timestamp-query readback, rebuilt when reset-time timer
  layout changes.
- Particle-scale preflight/status - requested, estimated, actual, dispatch shape,
  storage limits, and `scale_status`.
- Device/surface status - `gpu_device_status` values reported as `ok`,
  `surface-lost`, `device-lost`, or `validation-error`.

Removed render feature ownership lives in `rendering.md`; `GpuContext` only owns
targets and passes for the current renderer set.

## Render targets

Water mode owns swapchain-sized depth, thickness/whitewater, nearest/smoothed depth,
and hero-water scene prepass targets. The authoritative target set and formats live
in `app/crates/fluid-lab/src/gpu/mod.rs → GpuContext::new`, `GpuContext::resize`,
`create_r16_target`, `create_scene_color_target`, and `render_target_memory_bytes`.

Removed render-feature targets are intentionally absent; see `rendering.md` for the
canonical removed-feature list and legacy setting ids.

`DiffuseSystem` owns a fixed-capacity particle storage buffer
(`app/crates/fluid-lab/src/gpu/diffuse.rs → DIFFUSE_CAPACITY`), a counter buffer, and
uniform buffers. `DiffuseSystem::memory_bytes` owns the byte accounting.
`render.diffuse.max_particles` is an active cap inside that fixed capacity and is
Live.

## Buffer layout and storage-buffer budget

`GpuFluid` uses structure-of-arrays for grid/MAC data and one interleaved particle
buffer. The per-stage storage-buffer limit is still a hard WebGPU constraint, so the
MAC loop remains split into small passes rather than a mega-pass.

The authoritative buffer inventory and byte accounting live in
`app/crates/fluid-lab/src/gpu/fluid.rs → GpuFluid`, `GpuFluid::new`, and
`GpuFluid::buffer_memory_bytes`. The layout groups hot MAC velocities, fixed-point
P2G accumulators, FLIP snapshots, pressure/CG workspace, grid scalars, and optional
sort scratch into separate storage buffers so each pass binds only the buffers it
needs.

### Workgroup shared memory (scatter_local.wgsl)

The sorted-path P2G scatter (`scatter_local.wgsl`, used when `dev.particle_sort` is
on by default) is the only P2G kernel using `var<workgroup>` shared memory. Each
workgroup pre-accumulates sorted particles' P2G taps in an open-addressed integer
hash table before flushing global atomics. The table constants and fit proof live in
`app/crates/fluid-lab/src/gpu/shaders/scatter_local.wgsl → CAP`, `SLOT_STRIDE`, and
`local_add`; the shared explicit layout lives in
`app/crates/fluid-lab/src/gpu/fluid.rs → scatter_bgl` / `scatter_pll`.
Determinism is preserved because the accumulate and flush stay pure integer atomics.
See `decisions/performance.md` and `architecture/simulation.md`.

`cg_scalars` is the small scalar workspace shared by the CG scalar kernels; its size
and tolerance slot live in `app/crates/fluid-lab/src/gpu/fluid.rs → CG_SCALAR_COUNT`
and `CG_TOL_SQ_SLOT`. The tolerance slot is updated through a Live queue write; the
active flag is GPU-owned during each pressure solve.

`pressure_a` is the pressure field read by gradient. With the default
`solver.pressure_warm_start = 1`, prep preserves it so `cg_init` can form the initial
residual from the previous pressure. Setting warm-start to `0` clears `pressure_a`
before each solve; reset clears it explicitly before the next solve.

The tank is rectangular with uniform cell size and per-axis counts. Reset-class grid
or particle changes rebuild `GpuFluid` and renderers that bind sim buffers.

## Reset and resize

`GpuContext::recreate_fluid` preflights the requested particle scale before mutating
the active scale facts, then rebuilds the simulation, wireframe/environment geometry,
particle renderer, slice renderer, and diffuse system against fresh sim buffers.
Rejected recreates leave the active fluid and reported scale facts intact except for a
log line describing the rejected request. Rebuilding `DiffuseSystem` clears foam
particles. `GpuTimers` is also rebuilt when timestamp queries are available.

`resize` recreates the surface-sized targets and rebinds smoothing/composite views.

## Surface loss and device loss

`GpuContext::render` treats `CurrentSurfaceTexture::Lost` and
`CurrentSurfaceTexture::Outdated` as recoverable surface events: it sets
`gpu_device_status` to `surface-lost`, reconfigures the surface, recreates the
swapchain-sized target views, rebinds composite/smoothing views, skips that frame,
and lets the next frame continue. The next successful surface acquisition returns
the status to `ok`. `Timeout` and `Occluded` skip the frame without changing status.
`Validation` sets `validation-error` and returns an error to the caller.

wgpu's device-lost callback sets `gpu_device_status` to `device-lost`. This is not
full WebGPU device-loss recovery: the app does not recreate the adapter/device/queue
or rebuild every GPU owner after true device loss. The shell treats `device-lost` and
`validation-error` as fatal statuses and asks the user to reload.

## Readbacks and counters

Normal sim/render frames do not map GPU buffers. Allowed readbacks are:

- boot smoke test,
- throttled `GpuTimers` timing/liveness/diffuse-counter readback.

Diffuse counters are copied as cursor/emitted/clamped/alive-foam plus legacy zero
slots for spray/bubble. The profiler reports foam only while preserving the JSON shape.

## Gotchas

- Naga drops unused bindings from reflected layouts; shaders with params uniforms must
  read a field or use explicit layouts.
- Particle-linear work uses one shared tiled dispatch shape.
- Particle-scale preflight rejects impossible create/Reset attempts before allocation.
- `gpu_buffer_mb` reports simulation buffers for legacy consumers. `stats_json`
  also reports tracked categories (`sim_buffers_mb`, `render_targets_mb`,
  `diffuse_mb`, `timing_mb`, `total_tracked_mb`), but those are allocation math
  from known owners, not total driver-resident VRAM. When timers exist, `timing_mb`
  counts the timestamp resolve buffer plus the mapped readback buffer; it does not
  include hidden `QuerySet` driver memory because `wgpu` does not expose that byte
  size.
- Timestamp-query is optional; timing paths guard on `Option<GpuTimers>`.

## Update when

- A new GPU pass or render subsystem is added.
- Render-target formats/recreation rules change.
- Reset-class settings or timer construction parameters change.
- Diffuse counter/readback shape changes.

## See also

- `rendering.md`
- `profiler.md`
- `simulation.md`
- `../decisions/performance.md`
- `../agent-context/maintaining-docs.md`
