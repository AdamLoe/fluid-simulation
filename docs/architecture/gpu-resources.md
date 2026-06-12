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

Phase 2 removed the GPU owners for caustics, temporal stabilization, wet walls, and
dense wall fill. `GpuContext` no longer has fields for those systems, no longer
allocates their targets/buffers, and no longer schedules their passes.

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

Removed targets are absent: no caustic ping/pong, no temporal history ping/pong, no
wetness storage buffer, no wall occupancy atlas, and no `wallfill_mask` target.

`DiffuseSystem` owns a fixed-capacity particle storage buffer
(`DIFFUSE_CAPACITY`, 48 bytes/particle, about 12.6 MB), a small counter buffer, and
uniform buffers. `render.diffuse.max_particles` is an active cap inside that fixed
capacity and is Live.

## Buffer layout and storage-buffer budget

`GpuFluid` uses structure-of-arrays for grid/MAC data and one interleaved particle
buffer. The per-stage storage-buffer limit is still a hard WebGPU constraint, so the
MAC loop remains split into small passes rather than a mega-pass.

```
Particles      particles (pos+vel, 32 B/particle)
MAC vels       u_vel / v_vel / w_vel
P2G accum      u_num / u_den / v_num / v_den / w_num / w_den
FLIP snapshot  u_saved / v_saved / w_saved
Pressure       pressure_a / pressure_b
CG workspace   cg_d / cg_q / cg_partials / cg_scalars
Grid scalar    divergence / occupancy / cell_type / stats
```

The tank is rectangular with uniform cell size and per-axis counts. Reset-class grid
or particle changes rebuild `GpuFluid` and renderers that bind sim buffers.

## Reset and resize

`GpuContext::recreate_fluid` rebuilds the simulation, wireframe/environment geometry,
particle renderer, slice renderer, and diffuse system against fresh sim buffers.
Rebuilding `DiffuseSystem` clears foam particles. `GpuTimers` is also rebuilt when
timestamp queries are available.

`resize` recreates the surface-sized targets and rebinds smoothing/composite views.
There are no temporal or caustic stable-view rebinding paths after Phase 2.

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
- `gpu_buffer_mb` reports simulation buffers, not all render targets.
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
