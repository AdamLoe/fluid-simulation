---
status:        active
owner:         adamg
last_updated:  2026-06-09
okay_to_delete: false
long_lived:    true
---

# Rendering & inspection views

The GPU-native render layer draws the wireframe tank, the hero-water screen-space
composite (scene-color refraction plus a reflected procedural environment over a
textured tank floor + walls, with approximate caustics, wet-wall cues, and temporal
stabilization), a procedural world skybox background, selectable optical/simple particle
views, and an optional grid-slice inspection overlay.
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
           procedural skybox (world background) + environment (floor + back/left walls,
             reading the wetness field for darken/gloss/streak/meniscus/contact-shadow) + wireframe
         thickness + whitewater + nearest-Z MRT -> wall-fill sheet injection ->
         separable thickness + whitewater blur (plain Gaussian) + bilateral depth smoothing ->
         temporal history-blend of thickness / smooth-Z / whitewater into stabilized targets ->
         caustic generation (half-res) -> caustic composite (additive into scene_color) ->
         composite (opaque) samples scene_color (now caustic-lit) at a normal-driven refract UV,
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
| Screen-space water | `RenderMode::Water` (default) | `crates/fluid-lab/src/gpu/particles.rs -> ParticleRenderer`; `crates/fluid-lab/src/gpu/smoothing.rs -> WaterSmoothRenderer` (depth) + `ThicknessSmoothRenderer` (thickness); `crates/fluid-lab/src/gpu/composite.rs -> CompositeRenderer` | `crates/fluid-lab/src/gpu/shaders/particles.wgsl`; `crates/fluid-lab/src/gpu/shaders/{water_smooth,thickness_smooth}.wgsl`; `crates/fluid-lab/src/gpu/shaders/composite.wgsl` |
| Diffuse water (foam/spray/bubbles) | Water mode, `render.diffuse.enabled` | `crates/fluid-lab/src/gpu/diffuse.rs -> DiffuseSystem` | `crates/fluid-lab/src/gpu/shaders/diffuse_{emit,update,render}.wgsl` |
| Caustics (floor/back-wall light focusing) | Water mode, `render.hero.caustics.enabled` | `crates/fluid-lab/src/gpu/caustics.rs -> CausticsSystem` | `crates/fluid-lab/src/gpu/shaders/caustics_{generate,composite}.wgsl` |
| Wet walls / meniscus / contact shadow | Water mode, `render.hero.wet_wall.enabled` | `crates/fluid-lab/src/gpu/wetwall.rs -> WetWallSystem` (write) + `EnvironmentRenderer` (read) | `crates/fluid-lab/src/gpu/shaders/wetwall_update.wgsl`; reads in `environment.wgsl` |
| Dense wall fill | Water mode, `render.hero.flat_water.fill_enabled` | `crates/fluid-lab/src/gpu/wallfill.rs -> WallOccupancySystem` (write) + `WallFillRenderer` (MRT injection) | `crates/fluid-lab/src/gpu/shaders/wallfill.wgsl` |
| Temporal stabilization | Water mode, `render.hero.temporal.enabled` | `crates/fluid-lab/src/gpu/temporal.rs -> TemporalSystem` | `crates/fluid-lab/src/gpu/shaders/temporal_blend.wgsl` |
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

`WaterSmoothRenderer` runs a narrow separable **bilateral** filter over nearest-Z using
point `textureLoad` reads (edge-preserving, so the reconstructed normal stays crisp).
`ThicknessSmoothRenderer` (`crates/fluid-lab/src/gpu/smoothing.rs`,
`shaders/thickness_smooth.wgsl`) separately runs a **plain separable Gaussian** — in two
instances, over the **thickness** target and the **whitewater** target, in place. Both are
per-particle accumulation signals that drive composite colour, so leaving them raw made the
individual splats read as speckle: the thickness noise as a "sandy" body that let the dark
wall show through inter-splat gaps near the glass, and the whitewater noise as a field of
white foam speckle dots all over moving water. The Gaussian makes both spatially coherent
(a continuous sheet up to the wall; foam as soft regions, not dots) without needing extra
render targets — each blur reuses the depth pass's `smooth_z_ping` scratch in sequence and
shares `render.hero.smooth_radius` / `smooth_iterations`. All three run after wall-fill
injection and before temporal. `CompositeRenderer` then samples the (smoothed) thickness
target and
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
refraction UV offset, Fresnel, absorption, water-only, reflection, env-only, caustics, and
the wall-diagnosis views nearest-Z / whitewater / wallfill-mask) to the swapchain; the
authoritative list is `settings/mod.rs -> enum_options`. The reflected
environment and the world skybox share one procedural function (`shaders/env.wgsl ->
env_sample`), so they stay coherent; `environment_mode` selects Sky/Room/Studio.

**Diffuse water (foam / spray / bubbles).** In Water mode, `DiffuseSystem`
(`crates/fluid-lab/src/gpu/diffuse.rs`) maintains a persistent, **render-only** GPU
particle set that can replace the speed-mask whitewater tint with diffuse state. It is
available through `render.diffuse.enabled` but defaults off so glass-adjacent wall
rendering starts smooth. When enabled, diffuse state is *born* at fast/breaking surfaces
and wall impacts, advects/ages over seconds, and *decays*. It conserves no mass and
never writes the sim buffers (see `../decisions/rendering.md`). Three GPU passes, all
readback-free:

- **emit** (`diffuse_emit.wgsl`) — one invocation per grid cell; reads cell types +
  MAC face velocities, picks the strongest of surface-speed / wall-impact / fast-
  interior signals, and stochastically spawns one particle (foam/spray/bubble) into
  a ring buffer. Surface-speed emits foam/spray; wall impacts emit short-lived spray
  rather than long-lived foam decals on vertical glass. Spawning is deterministic per
  `(cell, frame, seed)` via an integer hash (no wall-clock RNG) and bounded by an
  integer-atomic per-frame budget (no float atomics, consistent with `scatter.wgsl`).
- **update** (`diffuse_update.wgsl`) — one invocation per active slot; age/lifetime
  kill, type-specific motion (foam couples to the flow while on the liquid surface
  but falls ballistically once stranded in an air cell so it can't hang in midair;
  foam that hugs vertical glass above the floor is retired quickly so the wet-wall
  material owns that cue; spray is ballistic with drag; bubbles rise by buoyancy),
  surface type transitions, and an integer-atomic alive-per-type recount.
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

**Approximate caustics.** `CausticsSystem` (`crates/fluid-lab/src/gpu/caustics.rs`)
casts focused-light patterns on the tank floor and back/left walls. It runs two
fullscreen passes inserted *before* the water composite so refraction sees lit
caustics. The **generation** pass (`caustics_generate.wgsl`) writes a half-res scalar
caustic map from the stabilized water normal/thickness and the shared
`HeroParams.sun_direction` (the same sun the skybox/specular use). The
**composite** pass (`caustics_composite.wgsl`) reconstructs each receiver's world hit
point from `scene_depth` along the eye ray (no kind G-buffer; it gates floor/back/left
by world position) and blends additively into `scene_color` — so the water's refracted
background tap picks up the caustic lighting, and non-receiver geometry is untouched.
Caustics are screen-space normal-gradient focusing, **not** projected/refracted photons
(`../decisions/rendering.md`). Default off; all `render.hero.caustics.*` are Live.

**Wet walls, meniscus & contact shadow.** `WetWallSystem`
(`crates/fluid-lab/src/gpu/wetwall.rs`) owns a persistent supersampled per-wall-texel
`f32` wetness field (`gpu-resources.md`) updated once per frame by a compute pass
(`wetwall_update.wgsl`, own encoder, run from `GpuContext::update_wetwall` after
`update_diffuse`, outside the timestamped sim passes). Each supersampled texel reads
fractional **current** `cell_type` coverage from the adjacent interior layer (Liquid cells
adjacent to a Solid wall, bilinear over the wall axes) and decay-blends
`wetness = max(new_contact, prev * pow(decay, dt*60))` — framerate-corrected so streaks
linger in seconds, not frames. The wall material in `environment.wgsl` reads the field
through a blur-aware wetness sampler and a soft material-response threshold to suppress
isolated contact speckles, darken/streak wet regions, blend environment reflections,
scale sun sheen by wall gloss/specular, draw a thin Fresnel meniscus band from the same
smoothed wetness gradient, and add a contact shadow at the floor/wall join. This is a
procedural **render-only** cue, not simulated
thin-film drainage (`../decisions/rendering.md`); particle→wetness coupling
(`wetness_spray_gain`) is registered but stubbed at 0. Wetness persists across frames and
clears on Reset (`WetWallSystem` rebuilt with a fresh zeroed buffer in `recreate_fluid`,
which also rebinds the fresh buffer into the rebuilt `EnvironmentRenderer`). Most
`render.hero.wet_wall.*` settings are Live; `render.hero.wet_wall.supersample` is
Reset-class because it changes the wetness buffer dimensions. `WetWallSystem` stores the
allocation-time supersampled dimensions and keeps using them until Reset, so changing the
Reset-class setting cannot desynchronize the uniform from the allocated buffer mid-run.

**Dense wall fill.** `WallOccupancySystem` (`crates/fluid-lab/src/gpu/wallfill.rs`)
maintains a dense, current-frame wall occupancy buffer from particle splats near the
tank walls, using the supersampled atlas dimensions allocated from
`render.hero.flat_water.fill_supersample`. The render pass runs after particle
thickness/nearest-Z writes and before smoothing, intersects the **rendered** back and
left wall planes per pixel, bilinearly samples the dense atlas in both wall axes, smooths
the coverage curve by `waterline_softness`, and injects a subtle flat sheet into the
same MRT targets (`thickness` Add, `nearest_z` Min, `whitewater` Add zero). It applies a
local repair at the shared back-left edge by sampling a few supersampled texels inward
from both visible faces, so still water fills the rendered corner without globally
inflating wall coverage. The right and front planes remain open viewing faces like the
environment prepass; the fill pass does not inject hidden sheets there because those
projected planes create camera/box-rotation-dependent mask seams. The pass also writes a
full-resolution `wallfill_mask` target, cleared every frame, so `composite.wgsl` can tune
fill-only color, absorption, reflection, and roughness without changing normal water.
Rounded billboard kernels no longer draw visible blobs on the wall because the thickness
target is Gaussian-smoothed before composite (see **Screen-space water** above), so the
fill sheet and the smoothed particle thickness together form a continuous contact band —
the core particle splats are no longer suppressed near the glass.
Because occupancy is stored by wall texel instead of as one topmost waterline per column,
a dry gap between lower and upper water patches remains dry. `fill_strength` scales the
injected slab, `fill_slab` controls optical body thickness, and `waterline_softness`
controls the final coverage easing rather than supersampling more texels.

**Temporal stabilization.** `TemporalSystem` (`crates/fluid-lab/src/gpu/temporal.rs`)
reduces the shimmer across the hero stack by **history-blending** the three full-res
R16 screen-space targets — `thickness`, `smooth_z` (which derives the surface normal and
curvature; there is no stored normal target), and `whitewater` (the screen-space foam
signal; the diffuse particles are alpha-blended geometry and cannot be blended) — into
ping-pong stabilized outputs that the caustics and water composite read instead of the
raw targets. The blend (`temporal_blend.wgsl`) runs **after** smooth-Y and the thickness
pass and **before** caustics generation: `out = mix(current, history, history_alpha)`,
with per-pixel depth/normal validity rejects. **Known limitation:** this is history-blend
plus a hard **camera reset**, *not* motion-vector reprojection. The reset metric is a CPU
camera-motion delta from the model-free `eye_to_world` (`crates/fluid-lab/src/gpu/mod.rs
-> camera_motion`), covering both rotation and translation over
`camera_motion_reset_threshold`; on a reset (or after resize/`Outdated` rebuild) history
is dropped for one frame. Content motion under a static camera is *not* stabilized — a
future reprojection follow-up would address it (`../decisions/rendering.md`). The v1.16
caustics in-shader history blend is driven through this unified
`render.hero.temporal.*` control. All temporal settings are Live; default off.

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
  `SliceRenderer`, `DiffuseSystem`, and `WetWallSystem` after recreating `GpuFluid`
  (the wireframe + environment depend on tank bounds; the diffuse + wetwall compute
  bind groups reference the sim cell-type/face-velocity buffers); old renderer bind
  groups must not survive that reset. The current `HeroParams` are re-applied to the
  rebuilt environment. Rebuilding `DiffuseSystem` clears all diffuse particles, and
  rebuilding `WetWallSystem` clears the wetness field (fresh, zeroed buffers), on
  Reset; the rebuilt `EnvironmentRenderer` is bound to the fresh wetness buffer.
  `recreate_fluid` also drops the temporal + caustics history (one clean post-reset
  frame). The swapchain-sized temporal/caustics ping-pong targets themselves are
  rebuilt on `resize`/`Outdated`, not on `recreate_fluid`.
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
