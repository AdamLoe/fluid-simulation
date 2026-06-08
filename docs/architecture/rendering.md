---
status:        active
owner:         adamg
last_updated:  2026-06-08
okay_to_delete: false
long_lived:    true
---

# Rendering & inspection views

The GPU-native render layer draws the wireframe tank, the hero-water screen-space
composite (scene-color refraction over a textured environment), selectable
optical/simple particle views, and an optional grid-slice inspection overlay.
These views sample live GPU buffers directly; normal frames do not read simulation
state back to the CPU. Throttled diagnostics in
`crates/fluid-lab/src/gpu/timing.rs` own the only runtime readback.

## What it owns

`crates/fluid-lab/src/gpu/mod.rs -> GpuContext` owns the surface, shared depth
texture, water offscreen targets, the hero-water `scene_color`/`scene_depth` prepass
targets, renderer instances, the typed `RenderMode` enum, the `HeroParams` snapshot,
and render pass order. `RenderMode { Water, OpticalParticles, SimpleParticles }`
replaces the old bare `u32 particle_view` dispatch; `render.particle_view` still maps
0/1/2 onto these (`RenderMode::from_u32`). The tank model matrix
`translate(box_pos) * from_quat(box_orient)` is folded into `view_proj` by
`crates/fluid-lab/src/lib.rs -> FluidApp::frame`; renderers receive that combined
matrix and do not recompute the tank transform.

```
FluidApp::frame
  -> GpuContext::step(n)
  -> GpuContext::render(view_proj, billboard_right, billboard_up)
       Water mode (hero):
         scene prepass -> scene_color (Rgba16Float) + scene_depth (R16Float, eye dist):
           environment (floor/backdrop/walls) + wireframe
         thickness + whitewater + nearest-Z MRT -> separable depth smoothing ->
         composite (opaque) samples scene_color at a normal-driven refract UV,
           absorbs/tints over it, writes the final swapchain pixel
       OpticalParticles mode:
         opaque pass (wireframe) -> v1.10 optical-depth billboard pass
       SimpleParticles mode:
         opaque pass (wireframe) -> pre-v1.10 alpha billboard pass
       optional grid slice overlay
```

The render path is timestamped through `GpuTimers` when the adapter supports timestamp
queries. `gpu.render_ms` is one coarse public number spanning all render passes in the
frame, not a per-pass breakdown.

## Views

| View | Runtime state | Renderer | Shader |
|---|---|---|---|
| Wireframe tank | always on | `crates/fluid-lab/src/gpu/renderer.rs -> WireframeRenderer` (swapchain pipeline + dual-target scene pipeline) | inline WGSL in `renderer.rs` |
| Hero-water environment | Water mode only | `crates/fluid-lab/src/gpu/environment.rs -> EnvironmentRenderer` | `crates/fluid-lab/src/gpu/shaders/environment.wgsl` |
| Screen-space water | `RenderMode::Water` (default) | `crates/fluid-lab/src/gpu/particles.rs -> ParticleRenderer`; `crates/fluid-lab/src/gpu/smoothing.rs -> WaterSmoothRenderer`; `crates/fluid-lab/src/gpu/composite.rs -> CompositeRenderer` | `crates/fluid-lab/src/gpu/shaders/particles.wgsl`; `crates/fluid-lab/src/gpu/shaders/water_smooth.wgsl`; `crates/fluid-lab/src/gpu/shaders/composite.wgsl` |
| Optical particles | alternate `render.particle_view` | `crates/fluid-lab/src/gpu/particles.rs -> ParticleRenderer` | `crates/fluid-lab/src/gpu/shaders/particles.wgsl` |
| Simple particles | alternate `render.particle_view` | `crates/fluid-lab/src/gpu/particles.rs -> ParticleRenderer` | `crates/fluid-lab/src/gpu/shaders/particles.wgsl` |
| Grid slice | optional overlay | `crates/fluid-lab/src/gpu/slice.rs -> SliceRenderer` | `crates/fluid-lab/src/gpu/shaders/slice.wgsl` |

The tank is a uniform-cell-size rectangular box. `GpuFluid::tank_bounds()` sizes the
wireframe, and `GpuFluid::grid_dims()` supplies the per-axis dimensions used by the
slice renderer.

**Wireframe tank.** `WireframeRenderer::new` builds a fixed line-list for the current
tank AABB. Floor edges use a distinct tint for orientation. It is rebuilt with the
other renderers when Reset-class settings change the tank bounds.

**Screen-space water.** The default water view moves optical density and lighting out
of the shaded particle fragment and into screen space. `ParticleRenderer` still binds
the simulation particle buffer directly, but in water mode it renders camera-facing
billboards into offscreen `R16Float` targets: normalized thickness with additive
blending, speed-weighted thickness for whitewater with additive blending, and nearest
front-surface positive eye distance with min blending. Thickness is normalized by
represented liquid volume per actual particle count and the same tapered kernel the
fragment writes, so `render.particle_size` changes coverage and smoothness without
becoming a hidden opacity control.

`WaterSmoothRenderer` runs a narrow separable depth filter over nearest-Z using
point `textureLoad` reads. `CompositeRenderer` then samples the thickness target and
smoothed depth, reconstructs a screen-space normal, and — as of v1.12 — **refracts**:
it offsets the sample UV along the surface normal's xy (scaled by thickness, clamped
to a pixel budget), taps `scene_color` at that refract UV for the bent background,
applies per-channel Beer-Lambert absorption + the water body tint over it, and adds
Fresnel/specular. The composite is now **opaque** (`BlendState::REPLACE`): it samples
`scene_color` itself and writes the final pixel — the unrefracted background where
there is no water — so there is no separate blit pass. The speed-weighted target is
still divided by total thickness to mix rough regions toward white/ice-blue.

**Hero-water environment & scene prepass.** In Water mode, `EnvironmentRenderer`
draws a textured tank floor (procedural checker + grid), a gradient backdrop quad,
and matte side/back walls into the offscreen `scene_color` (linear `Rgba16Float`) and
`scene_depth` (`R16Float`, positive eye distance = `clip.w`), alongside the wireframe
via its dual-target scene pipeline. This gives refraction visible detail to bend. A
depth guard in the composite compares `scene_depth` at the refract UV against the
water front surface and falls back (unrefracted, or flat tint) when the tap would
grab foreground geometry.

**Hero-water parameters & debug views.** All Water-tab settings (`render.hero.*`) are
mirrored into one `HeroParams` snapshot (`settings::Registry::hero_params`), pushed
into the composite + environment uniforms whenever a slider changes — Live, no
pipeline rebuild, no per-setting plumbing. `f0` is derived from `ior` (Schlick), never
stored independently. `render.hero.mode_enabled` is the master toggle (off forces the
refraction offset to zero — the non-refractive comparison). `render.hero.debug_view`
routes an intermediate stage (scene color/depth, thickness, refraction UV offset,
Fresnel, absorption, water-only) to the swapchain.

**Optical particles.** This alternate particle view keeps the v1.10 shaded billboard
path reachable through `render.particle_view`. It exposes per-particle motion and
speed color directly, with Beer-Lambert per-billboard optical depth, depth testing
against the shared depth attachment, and particle depth writes off.

**Simple particles.** This alternate particle view restores the pre-v1.10 simple
billboard path. It uses the speed-color ramp, soft-edge alpha, sphere lighting, and
depth writes enabled, so it behaves like the old tried-and-true particle renderer
rather than the optical-depth or screen-space water paths.

The billboard basis starts in world space and is rotated into tank-local space by
`FluidApp::frame`. This cancels the tank rotation baked into `view_proj`, keeping the
quads camera-facing while the tank moves or rotates.

**Grid slice.** `SliceRenderer` binds cell type, pressure, and staggered velocity
buffers directly. `set_slice_mode` selects cell-type, pressure, or speed inspection.
The overlay draws the mid-depth XY cross-section and derives its shape from
`[nx, ny, nz]`, so non-cubic tanks remain correctly indexed.

## Removed surface path

There is no extracted surface renderer. Marching-cubes modules, shaders, host tables,
settings, and web controls are absent from the runtime. The screen-space water path is
an image-space composite over particle data, not a mesh extraction compatibility path.

## Non-obvious invariants and gotchas

- **No normal-frame readback.** Renderers bind live simulation buffers. Only the
  throttled profiler/timing path may map GPU data during routine execution.
- **Water is multi-pass, but still GPU-native.** `GpuContext::render` owns the pass
  order and water targets. Normal frames still avoid simulation readback.
- **The model matrix is baked into `view_proj`.** Renderers operate in tank-local
  coordinates. Thread a separate model matrix only if a new renderer genuinely needs
  it.
- **Slice bind groups reference live GPU buffers.** `GpuContext::recreate_fluid`
  rebuilds `WireframeRenderer`, `EnvironmentRenderer`, `ParticleRenderer`, and
  `SliceRenderer` after recreating `GpuFluid` (the wireframe + environment depend on
  tank bounds); old renderer bind groups must not survive that reset. The current
  `HeroParams` are re-applied to the rebuilt environment.
- **Particle look survives fluid recreation.** `GpuContext` stores current particle
  look values and reapplies them to the newly built renderers.
- **`WireframeRenderer` uses inline WGSL.** It has no separate shader file.

## Update when

- A view is added or removed: update the view table, render-pass order, settings, and
  `decisions/rendering.md`.
- Render-pass order or water target ownership changes: update this doc and
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
