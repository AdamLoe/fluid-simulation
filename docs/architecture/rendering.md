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
`crates/fluid-lab/src/gpu/timing.rs` own the runtime readback path.

## What it owns

`crates/fluid-lab/src/gpu/mod.rs -> GpuContext` owns the surface, shared depth
texture, water offscreen targets, hero-water `scene_color`/`scene_depth` prepass
targets, renderer instances, `RenderMode`, `HeroParams`, and render pass order.
`RenderMode { Water, OpticalParticles, SimpleParticles }` is still selected by
`render.particle_view` values 0/1/2.

`FluidApp::frame` folds the tank model matrix into `view_proj` and passes a
camera-only eye-to-world rotation into `GpuContext::render`. The skybox and water
reflection use that camera-only basis so rotating the tank changes gravity, not the
world background.

```
Water mode:
  scene prepass -> scene_color + scene_depth:
    skybox + environment floor/back-left walls + wireframe
  particle thickness + whitewater + nearest-Z MRT
  thickness blur + whitewater blur + bilateral depth smoothing
  opaque water composite
  optional surface-foam billboard pass

OpticalParticles mode:
  opaque pass -> optical-depth particle billboards

SimpleParticles mode:
  opaque pass -> simple alpha particle billboards

optional grid slice overlay
```

The current runtime does not schedule, allocate, rebind, or draw caustics, temporal
stabilization, wet walls, or dense wall fill. Their old setting ids are accepted only
for legacy replay compatibility.

## Views

| View | Runtime state | Renderer | Shader |
|---|---|---|---|
| Wireframe tank | always on | `gpu/renderer.rs -> WireframeRenderer` | inline WGSL in `renderer.rs` |
| World skybox | Water mode, `render.hero.skybox_enabled` | `gpu/skybox.rs -> SkyboxRenderer` | `gpu/shaders/{env,skybox}.wgsl` |
| Hero-water environment | Water mode only | `gpu/environment.rs -> EnvironmentRenderer` | `gpu/shaders/environment.wgsl` |
| Screen-space water | `RenderMode::Water` | `gpu/particles.rs`, `gpu/smoothing.rs`, `gpu/composite.rs` | `particles.wgsl`, `{water_smooth,thickness_smooth}.wgsl`, `composite.wgsl` |
| Surface foam | Water mode, `render.diffuse.enabled` | `gpu/diffuse.rs -> DiffuseSystem` | `gpu/shaders/diffuse_{emit,update,render}.wgsl` |
| Optical particles | alternate `render.particle_view` | `gpu/particles.rs -> ParticleRenderer` | `particles.wgsl` |
| Simple particles | alternate `render.particle_view` | `gpu/particles.rs -> ParticleRenderer` | `particles.wgsl` |
| Grid slice | optional overlay | `gpu/slice.rs -> SliceRenderer` | `slice.wgsl` |

## Screen-space water

Water mode accumulates normalized thickness, speed-weighted whitewater, and nearest
front-surface eye distance into `R16Float` targets. Thickness and whitewater are
blurred with a separable Gaussian, while nearest-Z is smoothed with an edge-preserving
bilateral filter. The composite samples the smoothed targets, reconstructs a
screen-space normal, refracts `scene_color`, applies Beer-Lambert absorption and body
tint, mixes a Fresnel-weighted procedural environment reflection, and writes the final
swapchain pixel opaquely.

The environment prepass writes:

- a fullscreen procedural skybox/background,
- a textured tank floor and matte back + left walls,
- the wireframe tank outline.

There is no wet-wall material path. The environment shader has one bind group: the
camera/material uniform. The right and front tank faces remain open viewing faces.

`render.hero.wall_contact_enabled` keeps the cheap near-wall snap. It gates
`render.hero.flat_water.strength`, `render.hero.flat_water.epsilon`, and
`render.hero.flat_water.depth_strength`, which flatten near-wall normals/depth in the
composite without allocating a dense wall-fill atlas.

`render.hero.debug_view` routes retained intermediate views only: scene color/depth,
thickness, refraction offset, Fresnel, absorption, final water, reflection, environment,
nearest-Z, and whitewater. Removed caustics and wall-fill debug views are gone.

### Splat radius tracks particle spacing (volume-neutral density)

The visible water body is built from screen-space-smoothed particle splats, so its
apparent volume depends on splat *coverage*, not on liquid cells. A fixed splat
radius therefore makes the body look smaller when `particles.density` drops (the
splats stop overlapping and pinhole). To make density a pure fidelity/cost knob, the
**base splat radius tracks the seeded inter-particle spacing**:

```
particle_radius = scene.seeded_spacing(settings) * SPLAT_RADIUS_PER_SPACING
seeded_spacing  = H * effective_density^(-1/3)
```

`SPLAT_RADIUS_PER_SPACING` is the single tunable renderer constant
(`app/crates/fluid-lab/src/gpu/mod.rs`). At the reference density (8/cell) the
spacing is `H * 8^(-1/3) = H * 0.5`, so the constant is **0.7** to reproduce the
historical `H * 0.35` exactly. Lowering density coarsens the lattice (larger
spacing) and the splats grow to keep coverage ~constant — the body stays the same
size, just blobbier. `effective_density` is `resolved_count / seeded_cells`, so an
advanced `particles.count` override changes the spacing too and the splat follows
(no silent volume change). The radius is recomputed on every Reset (density / count /
`scene.fill_level` are all Reset-class) at both `GpuRenderer::new` and
`recreate_fluid`. `render.particle_size` remains the **Live** user multiplier applied
on top via `ParticleRenderer::set_radius_scale`; `recompute_volume_scale` keeps the
kernel normalization consistent when the radius changes.

Calibration: tune `SPLAT_RADIUS_PER_SPACING` if a coverage sweep shows low density
under- or over-covering. `app/tools/vdd_sweep.mjs` runs the real-GPU density sweep
(8 vs 2 at a fixed waterline), screenshots each, and reports the `liquid_cells` /
`filled_volume` ratios used as the fast Phase-1 invariance proxy (the visible-volume
acceptance is the screenshots; expect the physics-cell ratio to sit within ~15% as
the dilation rind is density-dependent). The SDF/level-set surface rewrite — the
"proper" coverage fix — is deliberately deferred (`../decisions/scope.md`).

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

The following files are intentionally absent from the current runtime:

- `gpu/caustics.rs`, `gpu/shaders/caustics_{generate,composite}.wgsl`
- `gpu/temporal.rs`, `gpu/shaders/temporal_blend.wgsl`
- `gpu/wetwall.rs`, `gpu/shaders/wetwall_update.wgsl`
- `gpu/wallfill.rs`, `gpu/shaders/wallfill.wgsl`

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
