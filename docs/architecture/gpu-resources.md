---
status:        active
owner:         adamg
last_updated:  2026-06-12
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

Water mode owns these swapchain-sized targets:

- depth: `Depth32Float`
- `thickness`: `R16Float`
- `whitewater`: `R16Float`
- `nearest_z`: `R16Float`
- `smooth_z_ping`: `R16Float` smoothing scratch
- `smooth_z`: `R16Float`
- `scene_color`: `Rgba16Float`
- `scene_depth`: `R16Float`

Removed render-feature targets are intentionally absent; see `rendering.md` for the
canonical removed-feature list and legacy setting ids.

`DiffuseSystem` owns a fixed-capacity particle storage buffer
(`DIFFUSE_CAPACITY`, 48 bytes/particle, about 12.6 MB), a small counter buffer, and
uniform buffers. `render.diffuse.max_particles` is an active cap inside that fixed
capacity and is Live.

## Buffer layout and storage-buffer budget

`GpuFluid` uses structure-of-arrays for grid/MAC data and one interleaved particle
buffer. The per-stage storage-buffer limit is still a hard WebGPU constraint, so the
MAC loop remains split into small passes rather than a mega-pass.

```
Particles      particles + particles_b (pos+vel, 32 B/particle; B = spatial-sort
               ping-pong second side, a 32 B placeholder when the sort is off or
               its buffer can't allocate)
MAC vels       u_vel / v_vel / w_vel
P2G accum      u_num / u_den / v_num / v_den / w_num / w_den
FLIP snapshot  u_saved / v_saved / w_saved
Pressure       pressure_a / pressure_b
CG workspace   cg_d / cg_q / cg_partials / cg_scalars
Grid scalar    divergence / occupancy / cell_type / stats
Sort scan      cell_offset (per-cell bucket starts / cursor) / scan_spine (per-block)
```

### Workgroup shared memory (scatter_local.wgsl)

The sorted-path P2G scatter (`scatter_local.wgsl`, used when `dev.particle_sort` is
on — the default) is the only kernel using `var<workgroup>` shared memory. Each
workgroup holds an open-addressed hash table that pre-accumulates its 64 sorted
particles' P2G taps before flushing one global atomic per touched face slot:

- `sh_key: array<atomic<i32>, 1024>` — packed global face slot (`buffer_id·2^22 +
  face_idx`, stored +1 so 0 = empty). 4 KB.
- `sh_val: array<atomic<i32>, 1024>` — accumulated i32 fixed-point value. 4 KB.

Total **8 KB** of workgroup storage, well under the WebGPU floor
(`maxComputeWorkgroupStorageSize` ≥ 16 KB). The 2^22 (4,194,304) slot stride covers
the largest face buffer at the 128-capped grid (`(nx+1)·ny·nz = 129·128·128 ≈
2.11M`); 6 buffers · 2^22 ≈ 25.2M < 2^31 so `key+1` fits i32. Determinism is
preserved because the accumulate AND the flush stay pure integer atomics (add is
associative/commutative). See `decisions/performance.md` and
`architecture/simulation.md`.

`cg_scalars` is a tiny seven-`f32` storage buffer shared by the CG scalar kernels:
`rs_old`, dot scratch, alpha, beta, initial residual, active flag, and tolerance
squared. The tolerance slot is updated through a Live queue write; the active flag is
GPU-owned during each pressure solve.

`pressure_a` is the pressure field read by gradient. With the default
`solver.pressure_warm_start = 0`, prep clears it before each solve. With warm-start
enabled, prep preserves it so `cg_init` can form the initial residual from the
previous pressure; reset clears it explicitly before the next solve.

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
- `../decisions/performance.md`
