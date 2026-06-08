---
status:        active
owner:         adamg
last_updated:  2026-06-08
okay_to_delete: false
long_lived:    true
---

# Rendering & debug views

The GPU-native render layer draws the wireframe tank, particles, and an optional
grid-slice inspection overlay. These views sample live GPU buffers directly; normal
frames do not read simulation state back to the CPU. Throttled diagnostics in
`app/crates/fluid-lab/src/gpu/timing.rs` own the only runtime readback.

## What it owns

`app/crates/fluid-lab/src/gpu/mod.rs -> GpuContext` owns the surface, depth texture,
renderer instances, render-mode state, and the single render pass. The tank model
matrix `translate(box_pos) * from_quat(box_orient)` is folded into `view_proj` by
`app/crates/fluid-lab/src/lib.rs -> FluidApp::frame`; renderers receive that combined
matrix and do not recompute the tank transform.

```
FluidApp::frame
  -> GpuContext::step(n)
  -> GpuContext::render(view_proj, billboard_right, billboard_up)
       single pass to swapchain:
       wireframe -> particles -> optional grid slice
```

The render pass is timestamped through `GpuTimers` when the adapter supports timestamp
queries. It clears the swapchain and shared depth attachment once, then draws each
enabled view in order.

## Views

| View | Runtime state | Renderer | Shader |
|---|---|---|---|
| Wireframe tank | always on | `app/crates/fluid-lab/src/gpu/renderer.rs -> WireframeRenderer` | inline WGSL in `renderer.rs` |
| Particles | always on | `app/crates/fluid-lab/src/gpu/particles.rs -> ParticleRenderer` | `app/crates/fluid-lab/src/gpu/shaders/particles.wgsl` |
| Grid slice | optional overlay | `app/crates/fluid-lab/src/gpu/slice.rs -> SliceRenderer` | `app/crates/fluid-lab/src/gpu/shaders/slice.wgsl` |

The tank is a uniform-cell-size rectangular box. `GpuFluid::tank_bounds()` sizes the
wireframe, and `GpuFluid::grid_dims()` supplies the per-axis dimensions used by the
slice renderer.

**Wireframe tank.** `WireframeRenderer::new` builds a fixed line-list for the current
tank AABB. Floor edges use a distinct tint for orientation. It is rebuilt with the
other renderers when Reset-class settings change the tank bounds.

**Particles.** `ParticleRenderer` binds the simulation particle buffer directly as
read-only storage and draws one instanced camera-facing quad per particle. The camera
uniform carries the tank-local billboard basis, particle radius, speed scale, slow
and fast colors, water optical density, edge softness, and sphere-shading strength.
`particles.wgsl` derives fragment alpha from sphere-like billboard thickness with
`1 - exp(-water_optical_density * thickness)` instead of treating the control as a
flat opacity slider. Live render settings update renderer state; `update_camera`
uploads the complete uniform before each draw.

The billboard basis starts in world space and is rotated into tank-local space by
`FluidApp::frame`. This cancels the tank rotation baked into `view_proj`, keeping the
quads camera-facing while the tank moves or rotates.

Particles still render in the same shared swapchain pass as the tank and optional
slice overlay. The particle pipeline keeps depth testing against the shared depth
attachment, but `depth_write_enabled` is off for particles so transparent overlap can
accumulate instead of the nearest billboard sealing the layer. v1.10 kept this
same-pass path; it did not add an offscreen accumulation target, composite pass, or
persistent render buffers.

**Grid slice.** `SliceRenderer` binds cell type, pressure, and staggered velocity
buffers directly. `set_slice_mode` selects cell-type, pressure, or speed inspection.
The overlay draws the mid-depth XY cross-section and derives its shape from
`[nx, ny, nz]`, so non-cubic tanks remain correctly indexed.

## Removed surface path

There is no extracted surface renderer. Marching-cubes modules, shaders, host tables,
settings, web controls, and offscreen water targets are absent from the runtime. A
future surface technique would be a new product and architecture decision, not a
hidden compatibility path.

## Non-obvious invariants and gotchas

- **No normal-frame readback.** Renderers bind live simulation buffers. Only the
  throttled profiler/timing path may map GPU data during routine execution.
- **Rendering is single-pass.** `GpuContext::render` has one swapchain render pass and
  one shared depth attachment. Adding a multi-pass renderer changes the resource and
  timing contracts and must be documented explicitly.
- **The model matrix is baked into `view_proj`.** Renderers operate in tank-local
  coordinates. Thread a separate model matrix only if a new renderer genuinely needs
  it.
- **Slice bind groups reference live GPU buffers.** `GpuContext::recreate_fluid`
  rebuilds `WireframeRenderer`, `ParticleRenderer`, and `SliceRenderer` after
  recreating `GpuFluid`; old renderer bind groups must not survive that reset.
- **Particle look survives fluid recreation.** `GpuContext` stores current particle
  look values and reapplies them to the newly built `ParticleRenderer`.
- **`WireframeRenderer` uses inline WGSL.** It has no separate shader file.

## Update when

- A view is added or removed: update the view table, render-pass order, settings, and
  `decisions/rendering.md`.
- A renderer starts requiring an additional pass or texture: update this doc and
  `gpu-resources.md`.
- Particle uniform/look semantics change: update the particle section and
  `settings.md`.
- Grid buffer layout or slice indexing changes: update `SliceRenderer::new` and the
  grid-slice contract here.
- The no-normal-frame-readback rule changes: update this doc, `profiler.md`, and the
  rendering decision.

## See also

- `gpu-resources.md` - device, surface, buffers, renderer recreation, and timing
- `simulation.md` - simulation buffers sampled by the renderers
- `app-shell.md` - frame loop and tank-local billboard basis
- `settings.md` - particle render controls and apply classes
- `../decisions/rendering.md` - particle/liquid-cell product direction
- `../agent-context/maintaining-docs.md` - doc maintenance rules
