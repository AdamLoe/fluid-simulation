---
status:        active
owner:         adamg
last_updated:  2026-06-07
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
- **Device / surface / depth** — created and owned here; resize re-configures surface and recreates the `Depth32Float` texture (`create_depth`).
- **`GpuFluid`** (`app/crates/fluid-lab/src/gpu/fluid.rs`) — simulation buffer set and all compute pipelines; `GpuContext` drives it via `record_prep` / `record_pressure` / `record_finish`.
- **Sub-renderers** — `WireframeRenderer`, `ParticleRenderer`, `SliceRenderer`; each receives buffer handles from `GpuFluid` at construction, not raw device buffers. `GpuFluid::grid_dims()` and `GpuFluid::tank_bounds()` thread the per-axis cell counts and the world-space tank AABB into the renderers on both construct and recreate.
- **`GpuTimers`** (`app/crates/fluid-lab/src/gpu/timing.rs`) — wraps timestamp-query sets; `None` when the feature is absent. When present, constructed with `(max_substeps, detailed, pressure_iters)` from the registry so the `QuerySet` is sized at construction time.

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

**Reset-class settings require buffer reallocation.** The per-axis grid resolutions `grid.res_x/res_y/res_z`, particle count, `fixed_dt`, `max_substeps`, and `dev.detailed_gpu_profiling` are baked into buffer sizes, uniforms, or timer layout at construction. Changing them requires calling `GpuContext::recreate_fluid`, which calls `GpuFluid::new` and rebuilds the `WireframeRenderer`, `ParticleRenderer`, and `SliceRenderer` from the new buffer handles. `GpuTimers` is also rebuilt from the new `max_substeps` / mode / `pressure_iters`. The device, surface, and format are untouched. Live/tweak-class settings are written to uniforms or renderer state without a rebuild.

**Particle-scale preflight happens before allocation/submission.** Particle-linear
passes currently use one-dimensional workgroup dispatches at workgroup size 64.
`GpuContext::recreate_fluid` computes the exact deterministic seeded count before
allocation and rejects a Reset when that count exceeds either
`max_compute_workgroups_per_dimension * 64` or the single particle storage-binding
limit. A rejected Reset preserves the running fluid and exposes the requested,
estimated, actual, and limiting values through `stats_json`.

**Memory accounting covers the active simulation buffers.** `GpuContext::buffer_memory_bytes()` forwards `GpuFluid::buffer_memory_bytes()`. Rendering owns only small renderer uniforms/geometry plus the shared depth texture; there is no extracted-surface vertex allocation or offscreen water-target allocation included in the runtime.

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

## See also

- `simulation.md` — MAC loop pass sequence and FLIP/PIC blend semantics.
- `pressure-solver.md` — CG solver pass breakdown (`cg_init` → `cg_spmv` → reduce → update cycle).
- `rendering.md` — `WireframeRenderer`, `ParticleRenderer`, and `SliceRenderer` internals.
- `profiler.md` — `GpuTimers` throttled readback and timing readout structure.
- `../decisions/performance.md` — rationale for SoA layout and pass-splitting strategy.
- `../agent-context/maintaining-docs.md` — doc maintenance rules.
