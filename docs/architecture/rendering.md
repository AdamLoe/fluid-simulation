---
status:        active
owner:         adamg
last_updated:  2026-06-08
okay_to_delete: false
long_lived:    true
---

# Rendering & inspection views

The GPU-native render layer draws the wireframe tank, the hero-water screen-space
composite (scene-color refraction plus a reflected procedural environment over a
textured tank floor + walls), a procedural world skybox background, selectable
optical/simple particle views, and an optional grid-slice inspection overlay.
These views sample live GPU buffers directly; normal frames do not read simulation
state back to the CPU. Throttled diagnostics in
`crates/fluid-lab/src/gpu/timing.rs` own the only runtime readback.

## What it owns

`crates/fluid-lab/src/gpu/mod.rs -> GpuContext` owns the surface, shared depth
texture, water offscreen targets, the hero-water `scene_color`/`scene_depth` prepass
targets, renderer instances, the typed `RenderMode` enum, the `HeroParams` snapshot,
the world skybox, and render pass order. `RenderMode { Water, OpticalParticles, SimpleParticles }`
replaces the old bare `u32 particle_view` dispatch; `render.particle_view` still maps
0/1/2 onto these (`RenderMode::from_u32`). The tank model matrix
`translate(box_pos) * from_quat(box_orient)` is folded into `view_proj` by
`crates/fluid-lab/src/lib.rs -> FluidApp::frame`; renderers receive that combined
matrix and do not recompute the tank transform.

`FluidApp::frame` also passes a **camera-only** eye→world rotation (the world-space
camera basis, with NO box orientation) into `GpuContext::render`. The composite's
reflected environment and the skybox sample the procedural environment in world space
through it, so the world background follows the camera but **does not rotate with the
box** — box rotation only changes the source of gravity (`app-shell.md`), not the world
(`../decisions/rendering.md`). The screen-space water normal is already box-independent
(reconstructed from screen depth), so this rotation is all the reflection needs.

```
FluidApp::frame
  -> GpuContext::step(n)
  -> GpuContext::render(view_proj, billboard_right, billboard_up)
       Water mode (hero):
         scene prepass -> scene_color (Rgba16Float) + scene_depth (R16Float, eye dist):
           procedural skybox (world background) + environment (floor + back/left walls) + wireframe
         thickness + whitewater + nearest-Z MRT -> separable depth smoothing ->
         composite (opaque) samples scene_color at a normal-driven refract UV,
           absorbs/tints over it, mixes a reflected procedural environment by Fresnel,
           adds a sun specular, writes the final swapchain pixel ->
         diffuse particle pass (foam/spray/bubbles) over the composite, when enabled
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
| World skybox | Water mode, `render.hero.skybox_enabled` | `crates/fluid-lab/src/gpu/skybox.rs -> SkyboxRenderer` | `crates/fluid-lab/src/gpu/shaders/{env,skybox}.wgsl` |
| Hero-water environment | Water mode only | `crates/fluid-lab/src/gpu/environment.rs -> EnvironmentRenderer` | `crates/fluid-lab/src/gpu/shaders/environment.wgsl` |
| Screen-space water | `RenderMode::Water` (default) | `crates/fluid-lab/src/gpu/particles.rs -> ParticleRenderer`; `crates/fluid-lab/src/gpu/smoothing.rs -> WaterSmoothRenderer`; `crates/fluid-lab/src/gpu/composite.rs -> CompositeRenderer` | `crates/fluid-lab/src/gpu/shaders/particles.wgsl`; `crates/fluid-lab/src/gpu/shaders/water_smooth.wgsl`; `crates/fluid-lab/src/gpu/shaders/composite.wgsl` |
| Diffuse water (foam/spray/bubbles) | Water mode, `render.diffuse.enabled` | `crates/fluid-lab/src/gpu/diffuse.rs -> DiffuseSystem` | `crates/fluid-lab/src/gpu/shaders/diffuse_{emit,update,render}.wgsl` |
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
smoothed depth, reconstructs a screen-space normal, and **refracts**:
it offsets the sample UV along the surface normal's xy (scaled by thickness, clamped
to a pixel budget), taps `scene_color` at that refract UV for the bent background,
applies per-channel Beer-Lambert absorption + the water body tint over it. It then
**reflects a procedural environment**: the screen-space normal's eye-space reflection
vector is rotated into world space (the camera-only `Cam` uniform, binding 8) and fed to
the shared `env_sample` (`shaders/env.wgsl`), mixed in by Fresnel and softened by a
roughness term (base + a speed/chop/foam blend), with a roughness-widened sun specular
on top. Optional screen-space micro-normals (`render.hero.micro_normal_*`, off by
default) perturb the normal for surface "tooth." The composite is **opaque**
(`BlendState::REPLACE`): it samples `scene_color` itself and writes the final pixel — the
unrefracted background where there is no water — so there is no separate blit pass. The
speed-weighted target is still divided by total thickness to drive both whitewater and
the reflection roughness.

**World skybox & scene prepass.** In Water mode the scene prepass fills the offscreen
`scene_color` (linear `Rgba16Float`) + `scene_depth` (`R16Float`, positive eye distance
= `clip.w`) in three steps, all into the same dual targets:

- `SkyboxRenderer` first draws a fullscreen procedural sky/room (a world-background) with
  depth-write off / `CompareFunction::Always`, sampling `env_sample` by the per-pixel
  **world-space** view ray (camera-only `eye_to_world`, never the box) and writing the
  far eye-distance sentinel. It is gated by `render.hero.skybox_enabled`.
- `EnvironmentRenderer` then draws the textured tank floor (procedural checker + grid) and
  the matte **back + left** walls. The right (+x) and front (+z) walls are intentionally
  omitted so those two faces form an open vertical corner you can look straight down into
  the tank through (`../decisions/rendering.md`). There is no longer a backdrop quad — the
  skybox is the background.
- `WireframeRenderer::draw_scene` draws the tank outline (all 12 edges).

The floor/walls/wireframe carry the box rotation (folded into `view_proj`); the skybox
does not. This gives refraction visible detail to bend and reflections something to
sample. A depth guard in the composite compares `scene_depth` at the refract UV against
the water front surface and falls back (unrefracted, or flat tint) when the tap would
grab foreground geometry.

**Hero-water parameters & debug views.** All Water-tab settings (`render.hero.*`) are
mirrored into one `HeroParams` snapshot (`settings::Registry::hero_params`), pushed
into the composite + environment + skybox uniforms whenever a slider changes — Live, no
pipeline rebuild, no per-setting plumbing. `f0` is derived from `ior` (Schlick), never
stored independently. `render.hero.mode_enabled` is the master toggle (off forces both
the refraction offset and the reflection strength to zero — the non-hero comparison).
`render.hero.debug_view` routes an intermediate stage (scene color/depth, thickness,
refraction UV offset, Fresnel, absorption, water-only, reflection, env-only) to the
swapchain; the authoritative list is `settings/mod.rs -> enum_options`. The reflected
environment and the world skybox share one procedural function (`shaders/env.wgsl ->
env_sample`), so they stay coherent; `environment_mode` selects Sky/Room/Studio.

**Diffuse water (foam / spray / bubbles).** In Water mode, `DiffuseSystem`
(`crates/fluid-lab/src/gpu/diffuse.rs`) maintains a persistent, **render-only** GPU
particle set that replaces the old speed-mask whitewater tint with diffuse state
that is *born* at fast/breaking surfaces and wall impacts, advects/ages over
seconds, and *decays*. It conserves no mass and never writes the sim buffers (see
`../decisions/rendering.md`). Three GPU passes, all readback-free:

- **emit** (`diffuse_emit.wgsl`) — one invocation per grid cell; reads cell types +
  MAC face velocities, picks the strongest of surface-speed / wall-impact / fast-
  interior signals, and stochastically spawns one particle (foam/spray/bubble) into
  a ring buffer. Spawning is deterministic per `(cell, frame, seed)` via an integer
  hash (no wall-clock RNG) and bounded by an integer-atomic per-frame budget (no
  float atomics, consistent with `scatter.wgsl`).
- **update** (`diffuse_update.wgsl`) — one invocation per active slot; age/lifetime
  kill, type-specific motion (foam couples to the flow while on the liquid surface
  but falls ballistically once stranded in an air cell so it can't hang in midair;
  spray is ballistic with drag; bubbles rise by buoyancy), surface type transitions,
  and an integer-atomic alive-per-type recount.
- **render** (`diffuse_render.wgsl`) — instanced premultiplied-alpha billboards over
  the composite, depth-tested against the shared scene depth (depth-write off),
  carrying the frame's `render_end` timestamp so `gpu.render_ms` still spans it.

`GpuContext::update_diffuse` runs emit+update once per frame (own encoder, outside
the timestamped sim passes) using the summed substep dt; it is a no-op while
disabled or paused. All `render.diffuse.*` settings mirror into one `DiffuseParams`
uniform via `GpuContext::set_diffuse_params` (Live, like `HeroParams`).
`render.diffuse.max_particles` is an **active cap** within a fixed buffer capacity
(`diffuse::DIFFUSE_CAPACITY`), so it stays Live with no reallocation. Alive-per-type
counts + emitted/clamped flow through the throttled `timing.rs` readback into
`stats_json.gpu.diffuse` and the profiler console line.

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
- **The world background is camera-only, box-independent.** The skybox and the
  composite's reflected environment sample `env_sample` in world space via the
  camera-only `eye_to_world` rotation, NOT `view_proj`. Rotating the box (manual or
  auto-roll) must not move the sky/reflection — that decoupling is the contract; box
  rotation only re-aims gravity (`app-shell.md`). The skybox carries no fluid/tank-bounds
  dependency, so `recreate_fluid` does not rebuild it (unlike the renderers below).
- **Slice bind groups reference live GPU buffers.** `GpuContext::recreate_fluid`
  rebuilds `WireframeRenderer`, `EnvironmentRenderer`, `ParticleRenderer`,
  `SliceRenderer`, and `DiffuseSystem` after recreating `GpuFluid` (the wireframe +
  environment depend on tank bounds; the diffuse compute bind groups reference the
  sim cell-type/face-velocity buffers); old renderer bind groups must not survive
  that reset. The current `HeroParams` are re-applied to the rebuilt environment.
  Rebuilding `DiffuseSystem` also clears all diffuse particles (a fresh, zeroed
  buffer) on Reset.
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
