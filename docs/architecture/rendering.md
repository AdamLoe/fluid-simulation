---
status:        active
owner:         adamg
last_updated:  2026-06-12
okay_to_delete: false
long_lived:    true
---

# Rendering & inspection views

The render layer draws the wireframe tank, the default screen-space Water view,
the procedural world skybox/environment, selectable optical/simple particle views,
an optional grid-slice overlay, and conservative surface foam. Normal frames do
not read simulation state back to the CPU; throttled diagnostics in
`crates/fluid-lab/src/gpu/timing.rs -> GpuTimers` own the runtime readback path.

## What it owns

`crates/fluid-lab/src/gpu/mod.rs -> GpuContext` owns the surface, shared depth
texture, water offscreen targets, hero-water `scene_color`/`scene_depth` prepass
targets, renderer instances, `RenderMode`, `HeroParams`, and render pass order.
`crates/fluid-lab/src/gpu/mod.rs -> RenderMode` is still selected by
`render.particle_view`; the wired modes are Water, OpticalParticles, and
SimpleParticles.

`FluidApp::frame` folds the tank model matrix into `view_proj` and passes a
camera-only eye-to-world rotation into `GpuContext::render`. The skybox and water
reflection use that camera-only basis so rotating the tank changes gravity, not the
world background.

`GpuContext::render` records all visible render paths. Water mode owns the
scene-color/depth prepass, screen-space water targets, smoothing, composite, and
optional foam pass; the two particle modes use the opaque tank/background pass plus
their particle renderer. The optional grid slice is recorded after the selected mode
as an overlay.

The current runtime does not schedule, allocate, rebind, or draw caustics, temporal
stabilization, wet walls, or dense wall fill. Their old setting ids are accepted only
for legacy replay compatibility.

## View anchors

The render-view map lives in code, not in a duplicated doc table:
`crates/fluid-lab/src/gpu/mod.rs -> GpuContext::render` for dispatch and pass order,
`crates/fluid-lab/src/gpu/renderer.rs -> WireframeRenderer` for the tank outline,
`crates/fluid-lab/src/gpu/skybox.rs -> SkyboxRenderer` and
`crates/fluid-lab/src/gpu/environment.rs -> EnvironmentRenderer` for the Water-mode
background/prepass, `crates/fluid-lab/src/gpu/particles.rs -> ParticleRenderer` for
screen-space water accumulation and particle views,
`crates/fluid-lab/src/gpu/smoothing.rs -> WaterSmoothRenderer` plus
`crates/fluid-lab/src/gpu/composite.rs -> CompositeRenderer` for the water surface,
`crates/fluid-lab/src/gpu/diffuse.rs -> DiffuseSystem` for surface foam, and
`crates/fluid-lab/src/gpu/slice.rs -> SliceRenderer` for the optional grid overlay.

## Screen-space water

Water mode accumulates normalized thickness, speed-weighted whitewater, and nearest
front-surface eye distance into `R16Float` targets. Thickness and whitewater are
blurred with a separable Gaussian, while nearest-Z is smoothed with a
**feature-preserving** (curvature-flow) bilateral filter. The composite samples the
smoothed targets, reconstructs a screen-space normal, refracts `scene_color`, applies
Beer-Lambert absorption and body tint, mixes a Fresnel-weighted procedural environment
reflection, and writes the final swapchain pixel opaquely.

The depth filter (`crates/fluid-lab/src/gpu/shaders/water_smooth.wgsl -> fs`) and
the normal reconstruction
(`crates/fluid-lab/src/gpu/shaders/composite.wgsl -> water_normal`) are
**curvature-adaptive**: a plain isotropic bilateral rounds everything equally, so
glassy sheets and sharp crests cannot coexist. Both passes estimate local surface
curvature at a *coarse* stencil — wide enough that a genuine multi-pixel ridge
registers but a single-splat bump does not, so the per-splat speckle the filter
exists to remove is **not** preserved — and where curvature is high they narrow the
spatial Gaussian / suppress the normal cross-average, keeping crests, ridges, and
droplet tips pointy while flat faces stay smooth. The single Live knob is
`render.hero.feature_preservation` (0 reproduces the legacy isotropic behaviour); it
routes through `crates/fluid-lab/src/gpu/smoothing.rs -> WaterSmoothRenderer` (the
`SmoothUniform.feature` slot) and the composite `Hero.norm.z` slot. This stays
entirely in the screen-space composite — there is still no SDF / level-set surface
([`../decisions/rendering.md`](../decisions/rendering.md)).

The environment prepass writes:

- a fullscreen procedural skybox/background,
- a textured tank floor and matte back + left walls,
- the wireframe tank outline.

There is no wet-wall material path. The environment shader has one bind group: the
camera/material uniform. The right and front tank faces remain open viewing faces.

`render.hero.wall_contact_enabled` keeps the cheap near-wall snap. It gates
`render.hero.flat_water.strength`, `render.hero.flat_water.epsilon`,
`render.hero.flat_water.depth_strength`, and `render.hero.flat_water.contact_fill`, which
flatten near-wall normals/depth **and** lift the contact-band coverage in the composite
without allocating a dense wall-fill atlas. The `contact_fill` term lifts the near-wall
band's effective thickness (ramped by the same wall-proximity weight as the normal/depth
snap) so water reads as a continuous sheet flush to the wall instead of a faded fringe
that lets the dark matte wall show through; it is kept moderate so the water still
refracts the background (see-through), and it is routed through the composite
`Cam.flat.w` slot. This is the aquarium-contact look achieved within the surviving cheap
screen-space snap — a dense wall-fill pass is still **not** allocated (see "Removed
features" and [`../decisions/rendering.md`](../decisions/rendering.md)).

`render.hero.debug_view` routes retained intermediate views only: scene color/depth,
thickness, refraction offset, Fresnel, absorption, final water, reflection, environment,
nearest-Z, and whitewater. Removed caustics and wall-fill debug views are gone.

### Splat radius tracks particle spacing (volume-neutral density)

The visible water body is built from screen-space-smoothed particle splats, so its
apparent volume depends on splat *coverage*, not on liquid cells. A fixed splat
radius therefore makes the body look smaller when `particles.density` drops (the
splats stop overlapping and pinhole). To make density a pure fidelity/cost knob, the
base splat radius tracks the seeded inter-particle spacing through
`crates/fluid-lab/src/gpu/mod.rs -> SPLAT_RADIUS_PER_SPACING` and
`crates/fluid-lab/src/scene/mod.rs -> SceneConfig::seeded_spacing`. Lowering density
coarsens the lattice and the splats grow to keep coverage approximately constant: the
body stays the same size, just blobbier. An advanced `particles.count` override
changes the effective spacing too, so the splat follows with no silent volume change.
The radius is recomputed on every Reset at both `GpuContext::new` and
`GpuContext::recreate_fluid`. `render.particle_size` remains the **Live** user
multiplier applied on top via `ParticleRenderer::set_radius_scale`;
`ParticleRenderer::recompute_volume_scale` keeps the kernel normalization consistent
when the radius changes.

Calibration: tune `SPLAT_RADIUS_PER_SPACING` if a coverage sweep shows low density
under- or over-covering. `tools/density_motion_sweep.mjs` runs the real-GPU density
sweep at a fixed waterline, screenshots each run, and reports the
`liquid_cells` / `filled_volume` ratios used as the fast invariance proxy. The
visible-volume acceptance is the screenshots; the physics-cell ratio is only a proxy
because the dilation rind is density-dependent. The SDF/level-set surface rewrite is
deliberately deferred (`../decisions/scope.md`).

## Surface foam

`DiffuseSystem` is now conservative surface foam only. It is render-only and never
writes simulation buffers. The compute passes read `cell_type` and MAC velocities:

- `diffuse_emit.wgsl` spawns foam only from liquid cells touching air and moving above
  the configured surface-speed threshold.
- `diffuse_update.wgsl` advects foam with local MAC flow while it remains on a
  liquid-air surface, kills stranded particles, and kills vertical-wall-hugging foam
  above the floor band.
- `diffuse_render.wgsl` draws soft off-white billboards over the water composite.

There is no spray, no bubbles, no wall-impact spawning, no airborne confetti, and no
wall decals. Legacy profiler JSON keys for `spray` and `bubble` remain zero for shape
compatibility, while visible profiler text reports foam only.

## Removed features

This section is the canonical owner for removed render features. Resource and
settings docs should link here instead of repeating the old subsystem inventory.

The following app-relative files are intentionally absent from the current runtime:

- `crates/fluid-lab/src/gpu/caustics.rs`,
  `crates/fluid-lab/src/gpu/shaders/caustics_{generate,composite}.wgsl`
- `crates/fluid-lab/src/gpu/temporal.rs`,
  `crates/fluid-lab/src/gpu/shaders/temporal_blend.wgsl`
- `crates/fluid-lab/src/gpu/wetwall.rs`,
  `crates/fluid-lab/src/gpu/shaders/wetwall_update.wgsl`
- `crates/fluid-lab/src/gpu/wallfill.rs`,
  `crates/fluid-lab/src/gpu/shaders/wallfill.wgsl`

Persisted ids under `render.hero.caustics.*`, `render.hero.temporal.*`,
`render.hero.wet_wall.*`, dense `render.hero.flat_water.fill_*`, and obsolete
diffuse spray/bubble/wall-impact ids are accepted and ignored during restore.

## Update when

- Render pass order, target formats, view modes, or debug-view ids change.
- A removed feature returns or a new render subsystem allocates targets.
- Surface-foam spawning, update, render, profiler, or settings semantics change.

## See also

- `settings.md` - registry ids, tabs, and legacy compatibility.
- `gpu-resources.md` - render target and buffer ownership.
- `profiler.md` - timing/readback semantics.
- `../decisions/rendering.md`
- `../agent-context/maintaining-docs.md`
