Can you take this chatGPT plan and help break it into realistic pieces taht could be done, and bring up any big questions or issues you find with the plan while you're doing it, that's it

````markdown
# Hero Real-Time Water Renderer Plan

## Goal

Build a new **interactive hero water renderer** for `fluid-lab` that consumes the existing particle-grid simulation and produces significantly more realistic tank water than the current simple particle renderer or current screen-space water composite.

This is **not** a solver rewrite.

The renderer should keep the existing simulation model:

- particles = moving visible liquid mass and velocity detail
- MAC grid = solver scaffold, cell classification, pressure, divergence, velocity field
- simple particle renderer = debug/fallback/spray reference
- current screen-space renderer = fallback and reusable thickness/depth infrastructure

The hero renderer should target this visual stack:

| Feature | Visual result |
|---|---|
| **Mesh/SDF surface** | Continuous water body, not particles pretending to connect. |
| **Fresnel reflection** | Bright reflective edges and glancing-angle highlights. |
| **Scene refraction** | Tank floor/backdrop visibly bend through the liquid. |
| **Absorption** | Deep water becomes darker/bluer/greener depending on material. |
| **Internal raymarch/thickness** | Water has optical depth instead of flat transparency. |
| **Persistent foam** | Whitewater remains after impacts and decays naturally. |
| **Bubbles** | Impact zones look aerated/milky instead of just white-tinted. |
| **Spray** | Isolated droplets stay particle-like and reflective. |
| **Caustics** | Floor/walls receive moving focused light patterns. |
| **Wet walls/meniscus** | Tank contact feels physical, not composited. |

---

# Reality of the 8-hour target

The full end-game renderer is a large renderer project. In 8 hours, the target should be:

## 8-hour outcome

A **working hero prototype** with:

1. Feature flag for `hero_water`.
2. Scene color/depth prepass.
3. First surface reconstruction path.
4. Transparent water material with Fresnel, refraction, absorption, and reflection.
5. Basic persistent foam/spray/bubble buffer.
6. Approximate caustics and wet-wall cues.
7. Large configuration surface for tuning.
8. Debug views for every major intermediate buffer.
9. Safe fallback to simple particles and current screen-space water.

## Do not target in 8 hours

Avoid trying to perfect:

- production-quality marching cubes,
- fully stable temporal reconstruction,
- physically exact caustics,
- perfect volumetric bubbles,
- high-quality anisotropic kernels,
- true multi-bounce refraction,
- adaptive sparse render grids.

The 8-hour version should be configurable and visually promising, not final.

---

# Non-goals

Do **not** do these in the hero renderer sprint:

- Do not replace the particle-grid solver.
- Do not remove the simple particle renderer.
- Do not remove grid/debug inspection modes.
- Do not require CPU readback in normal frames.
- Do not increase particle size as the main realism solution.
- Do not make whitewater equal to `speed > threshold`.
- Do not hardcode material constants without exposing them in settings.
- Do not make the renderer depend on one specific scene.
- Do not build a capture/offline renderer.

---

# Renderer mode structure

Add or preserve these render modes:

```text
RenderMode::SimpleParticles
RenderMode::OpticalParticles
RenderMode::ScreenSpaceWater
RenderMode::HeroWater
RenderMode::HeroWaterDebug
````

The renderer should be switchable at runtime.

`HeroWater` should be allowed to internally use pieces of the existing screen-space renderer, but the final hero path should be conceptually:

```text
particle/grid buffers
→ render surface field
→ reconstructed surface / mesh / fallback surface
→ transparent water material
→ foam/bubbles/spray overlay
→ caustics/wet-wall/environment interaction
→ final composite
```

---

# High-level architecture

## Frame pipeline

A hero-water frame should eventually follow this structure:

```text
1. Sim step runs as usual.
2. Render scene/tank/background into offscreen scene_color + scene_depth.
3. Build hero water render data from particles + MAC grid.
4. Reconstruct or approximate water surface.
5. Render water surface with transparent material.
6. Render foam, bubbles, spray, mist.
7. Render caustics / wet walls / meniscus cues.
8. Composite to swapchain.
9. Optionally render debug overlays.
```

---

# Required GPU resources

## Existing inputs

Use these existing simulation buffers:

```text
particle positions
particle velocities
grid cell types
grid face velocities
pressure
divergence
occupancy / classification
```

## New hero-water resources

Add these buffers/textures behind a feature flag:

```text
scene_color_target
scene_depth_target

hero_density_grid
hero_density_accum_fixed_point
hero_density_weight_accum
hero_surface_active_cells

hero_mesh_vertex_buffer
hero_mesh_index_or_indirect_buffer
hero_mesh_counter

hero_thickness_target
hero_front_depth_target
hero_back_depth_target optional
hero_normal_target

foam_particle_buffer
foam_alive_counter
foam_emit_counter

bubble_particle_buffer optional or shared diffuse buffer
spray_particle_buffer optional or shared diffuse buffer

caustic_target
wet_wall_target
meniscus_mask_target

hero_debug_target
```

Do not assume all of these need to be perfect in the first implementation. The key is to define the structure so multiple implementers can work in parallel.

---

# Surface reconstruction plan

## Target

Create a coherent water surface from the existing particles and grid.

The hero renderer should not draw the main liquid body as visible particles. Particles should become the source data for a reconstructed surface.

## Input

Use:

```text
particle positions
particle velocities
grid Liquid/Air/Solid classification
grid occupancy
grid velocity
```

## Surface-adjacent detection

A particle is a surface candidate if:

```text
particle cell is Liquid
and at least one neighboring cell is Air
```

A cell is an active render-surface cell if:

```text
cell is Liquid and has Air neighbor
or cell is Air and has Liquid neighbor
or cell is near Solid and Liquid contact
```

This prevents expensive reconstruction over the entire tank volume.

---

## Surface method options

Implement as configurable modes.

```rust
enum HeroSurfaceMode {
    Disabled,
    ScreenSpaceFallback,
    OccupancyQuads,
    DenseDensityMarchingCubes,
    DenseSdfMarchingCubes,
    HybridMeshAndScreenSpace,
}
```

### Mode 1: Occupancy quads

Fastest emergency fallback.

Generate quads on Liquid/Air cell boundaries.

Pros:

* fast,
* easy,
* gives an actual surface,
* good debug path.

Cons:

* blocky,
* not final quality.

Use this if marching cubes is too much for the first sprint.

### Mode 2: Dense density field + marching cubes

Main 8-hour target if feasible.

Pipeline:

```text
clear render density grid
splat particles into render density grid
smooth density field
extract iso-surface
compute normals
draw mesh
```

Start with a dense render grid.

Recommended first values:

```text
render_grid_scale = 1.0x solver grid
render_grid_scale_high = 1.5x or 2.0x solver grid
iso_value = tunable
smoothing_iterations = 1-4
```

Do not start with a sparse/adaptive grid unless the dense version already works.

### Mode 3: Dense SDF field + marching cubes

Better than pure density, but more work.

Pipeline:

```text
splat particle kernels into scalar field
convert to signed-ish field or density threshold
smooth field
extract surface
```

This should be a second implementation after simple density marching cubes.

### Mode 4: Hybrid mesh + screen-space

Use mesh for the main continuous body and screen-space particles/thickness for:

```text
thin sheets
spray
small droplets
subpixel foam
fallback thickness
```

This is likely the long-term hero architecture.

---

# Surface reconstruction settings

Expose all of these.

```toml
[render.hero_water.surface]
enabled = true
mode = "dense_density_marching_cubes"

render_grid_scale = 1.0
max_active_cells = 1000000

particle_splat_radius_cells = 1.25
particle_splat_strength = 1.0
density_fixed_point_scale = 4096

iso_value = 0.45
smoothing_iterations = 2
smoothing_strength = 0.35

normal_mode = "density_gradient"
normal_smoothing = 0.5

surface_only_particles = true
surface_air_neighbor_radius = 1

mesh_laplacian_iterations = 1
mesh_laplacian_strength = 0.15

max_vertices = 4000000
max_triangles = 2000000

fallback_to_screen_space = true
fallback_to_simple_particles = true
```

---

# Mesh extraction requirements

## First implementation

For the first version, implement either:

```text
A. marching cubes over active cells
B. occupancy-boundary quads
C. marching cubes over the full dense render grid
```

Preferred order:

```text
1. Occupancy-boundary quads if time is tight.
2. Dense marching cubes if possible.
3. Active-cell compacted marching cubes as follow-up.
```

## Requirements

The mesh path must provide:

```text
position
normal
velocity or velocity-derived value
foam influence optional
thickness hint optional
```

Example vertex:

```rust
struct HeroWaterVertex {
    position: vec4<f32>,
    normal: vec4<f32>,
    velocity_foam: vec4<f32>,
}
```

Where:

```text
velocity_foam.xyz = local water velocity
velocity_foam.w   = foam or turbulence hint
```

## Debug views

Expose:

```text
density grid slice
active cells
iso threshold preview
surface normals
mesh triangle count
mesh fallback reason
```

---

# Water material plan

## Target

The hero water material should make water visible through optics, not through fake opacity.

The material should include:

```text
Fresnel reflection
scene-color refraction
Beer-Lambert absorption
environment reflection
roughness
specular highlights
thickness/internal depth
velocity-driven roughness
foam masking
```

---

## Scene prepass

Before rendering water, render the tank environment into offscreen targets:

```text
scene_color
scene_depth
optional scene_normal
```

Include:

```text
tank floor
tank walls
wireframe/glass if applicable
background/backdrop
objects if present
```

The water shader then samples this background through the refracted water surface.

---

## Water shader inputs

```text
water mesh position
water mesh normal
view direction
scene_color
scene_depth
environment map or procedural sky
thickness target
front/back depth if available
foam target
caustic target
wet_wall target
```

---

## Material formula

The approximate shader should follow this structure:

```text
N = surface normal
V = view direction
F = fresnel_schlick(dot(N, V), f0)

refract_uv = screen_uv + N.xy * refraction_strength * thickness_factor
refracted_scene = sample(scene_color, refract_uv)

absorption = exp(-absorption_color * optical_thickness)

reflected_env = sample_environment(reflect(-V, N), roughness)

water_color = refracted_scene * absorption
water_color = mix(water_color, reflected_env, F)

water_color += specular_highlight
water_color = mix(water_color, foam_color, foam_amount)
```

---

## Material settings

Expose:

```toml
[render.hero_water.material]
enabled = true

ior = 1.333
f0 = 0.02037

base_tint = [0.72, 0.92, 1.0]
absorption_color = [0.08, 0.035, 0.015]
absorption_strength = 1.0

transparency = 0.92
deep_water_darkening = 0.45

refraction_strength = 0.035
refraction_thickness_scale = 1.0
refraction_max_offset_px = 48.0
invalid_refraction_fallback = "scene_color"

reflection_strength = 1.0
environment_strength = 0.8
environment_mode = "procedural_room"
skybox_enabled = true

roughness_base = 0.015
roughness_velocity_scale = 0.05
roughness_normal_variance_scale = 0.1
roughness_foam_scale = 0.35

specular_strength = 1.0
sun_direction = [0.25, -0.7, 0.45]
sun_intensity = 1.0

normal_strength = 1.0
micro_normal_enabled = true
micro_normal_strength = 0.035
micro_normal_scale = 18.0
micro_normal_velocity_scale = 0.5
```

---

# Thickness and internal depth plan

## Target

Water should have optical depth.

Deep water should look different from shallow water. Overlapping water should absorb more light.

## Options

Expose this as configurable modes.

```rust
enum HeroThicknessMode {
    Disabled,
    ExistingScreenSpaceThickness,
    MeshDepthDifference,
    ParticleThickness,
    Hybrid,
}
```

## First implementation

Reuse the current screen-space thickness target.

Current renderer already accumulates normalized world-space thickness from particle billboards. Keep that path and feed it into the hero water material.

## Better implementation

Add water front/back depth passes:

```text
render water front faces
render water back faces
thickness = back_depth - front_depth
```

This only works well once the mesh is coherent and closed enough.

## Settings

```toml
[render.hero_water.thickness]
mode = "hybrid"

use_existing_particle_thickness = true
use_mesh_depth_difference = false

thickness_scale = 1.0
thickness_bias = 0.0
thickness_min = 0.0
thickness_max = 3.0

smooth_thickness = true
thickness_smoothing_radius = 2.0
temporal_thickness = true
temporal_alpha = 0.85
```

---

# Foam, bubbles, spray, and mist

## Target

Whitewater must become a persistent render-state system.

It should not be a pure speed mask.

Implement a separate diffuse-water particle system:

```text
foam = surface flecks
bubbles = submerged air
spray = airborne droplets
mist = tiny impact haze
```

This system does not need to conserve mass and does not need to affect the solver initially.

---

## Diffuse particle structure

Use one shared buffer with a type field.

```rust
struct DiffuseParticle {
    pos_type: vec4<f32>,       // xyz + type
    vel_age: vec4<f32>,        // xyz + age
    params: vec4<f32>,         // lifetime, radius, alpha, seed
}
```

Type values:

```text
0 = foam
1 = bubble
2 = spray
3 = mist
```

---

## Emission sources

Emit from grid and particle conditions.

Good first signals:

```text
liquid-air interface
liquid-solid wall impact
high velocity magnitude
high outward surface velocity
high vorticity
high pressure near boundary
negative pre-projection divergence / compression
sudden particle direction change near walls
```

First implementation can use simplified signals:

```text
is_surface = Liquid cell with Air neighbor
speed = length(grid_velocity)
impact = velocity into Solid neighbor
turbulence = local velocity difference
```

Emission score:

```text
emit_score =
    k_surface_speed * surface_speed
  + k_wall_impact   * wall_impact
  + k_turbulence    * turbulence
  + k_compression   * compression
  - threshold
```

Then stochastic spawn using hash noise.

---

## Behavior by type

| Type   | Spawn condition                   | Motion                                        | Render                        |
| ------ | --------------------------------- | --------------------------------------------- | ----------------------------- |
| Foam   | Surface turbulence, wall impacts  | Advect with surface/grid velocity, slow decay | Soft white patches on surface |
| Bubble | Impact entrainment, plunging flow | Advect with liquid, buoyant upward            | Small bright/milky inclusions |
| Spray  | Strong outward surface velocity   | Ballistic with gravity and drag               | Reflective droplets           |
| Mist   | Violent impact                    | Short-lived screen-space haze                 | Low-alpha soft particles      |

---

## Update rules

Each frame:

```text
age += dt
if age > lifetime: kill

sample grid velocity at position

if type == foam:
    velocity = mix(velocity, grid_velocity, foam_advection)
    stay near surface if possible
    alpha decays slowly

if type == bubble:
    velocity = mix(velocity, grid_velocity, bubble_advection)
    velocity.y += bubble_buoyancy
    alpha decays by depth and age

if type == spray:
    velocity += gravity * dt
    velocity *= drag
    if reenters liquid: convert to foam or bubble

if type == mist:
    velocity *= high_drag
    alpha decays quickly
```

---

## Diffuse settings

Expose many knobs.

```toml
[render.hero_water.diffuse]
enabled = true
max_particles = 300000

emit_enabled = true
emit_rate = 1.0
emit_budget_per_frame = 12000

surface_emission = true
surface_speed_threshold = 0.8
surface_speed_gain = 1.0

wall_impact_emission = true
wall_impact_threshold = 0.5
wall_impact_gain = 2.0

vorticity_emission = true
vorticity_threshold = 0.4
vorticity_gain = 1.0

compression_emission = true
compression_threshold = 0.2
compression_gain = 0.8

foam_enabled = true
foam_lifetime_min = 1.0
foam_lifetime_max = 5.0
foam_radius = 0.015
foam_alpha = 0.85
foam_decay = 0.96
foam_advection = 0.9
foam_surface_lock = 0.6

bubble_enabled = true
bubble_lifetime_min = 0.7
bubble_lifetime_max = 4.0
bubble_radius = 0.008
bubble_alpha = 0.35
bubble_buoyancy = 0.4
bubble_advection = 0.95
bubble_milkiness = 0.45

spray_enabled = true
spray_lifetime_min = 0.25
spray_lifetime_max = 1.4
spray_radius = 0.004
spray_alpha = 0.9
spray_drag = 0.985
spray_reflection_strength = 1.0

mist_enabled = true
mist_lifetime_min = 0.15
mist_lifetime_max = 0.7
mist_radius = 0.035
mist_alpha = 0.18
mist_drag = 0.92

random_seed = 1337
```

---

# Caustics

## Target

Add moving light patterns on the tank floor and walls.

This is a high-value visual cue for bounded transparent water.

## First implementation

Use an approximate screen/world-space caustic map.

Inputs:

```text
water normal
water thickness
light direction
surface curvature / normal gradient
floor/wall receiver position
```

Approximate caustic strength:

```text
caustic =
    normal_focus_term
  * light_intensity
  * thickness_visibility
  * receiver_visibility
```

Composite caustics onto:

```text
tank floor
back wall
side walls optional
```

## Better implementation later

Project refracted light from the water surface onto floor/wall receivers and splat into a caustic texture.

## Settings

```toml
[render.hero_water.caustics]
enabled = true
mode = "normal_gradient"

resolution_scale = 0.5
intensity = 0.35
focus_strength = 1.2
thickness_scale = 1.0

floor_enabled = true
back_wall_enabled = true
side_walls_enabled = false

blur_radius = 2.0
temporal_enabled = true
temporal_alpha = 0.9

motion_scale = 1.0
max_intensity = 1.5
```

---

# Wet walls and meniscus

## Target

Tank water should feel like it contacts the boundary.

Add:

```text
wet-wall darkening
thin meniscus highlight
waterline cue
contact shadow near floor/walls
```

## Wet-wall system

Maintain a wetness texture or grid for tank walls.

Write wetness when:

```text
particles are near wall
Liquid cells neighbor Solid wall cells
spray impacts wall
foam contacts wall
```

Decay over time:

```text
wetness_next = max(new_contact, wetness_prev * decay)
```

Render on walls as:

```text
darker wall
slightly glossier wall
subtle vertical streaking optional
```

## Meniscus

Approximate meniscus at liquid-wall interface.

First implementation:

```text
detect wall-adjacent liquid surface cells
render a thin highlight band near waterline
```

Do not overdo it. It should be subtle.

## Settings

```toml
[render.hero_water.wet_wall]
enabled = true

wetness_decay = 0.985
wetness_contact_gain = 1.0
wetness_spray_gain = 0.6

darkening_strength = 0.22
gloss_strength = 0.35
streak_strength = 0.1

meniscus_enabled = true
meniscus_width = 0.018
meniscus_strength = 0.45
meniscus_fresnel_boost = 0.4

contact_shadow_enabled = true
contact_shadow_strength = 0.25
contact_shadow_radius = 0.04
```

---

# Tank environment requirements

Clear water needs something to refract.

Add or improve:

```text
textured tank floor
subtle back wall pattern
scale markings
matte floor material
environment reflection
procedural room/sky gradient
optional background checker strip
```

Settings:

```toml
[render.hero_water.environment]
enabled = true

floor_pattern_enabled = true
floor_pattern_scale = 12.0
floor_pattern_strength = 0.18

backdrop_enabled = true
backdrop_mode = "gradient_grid"
backdrop_strength = 1.0

tank_wall_visibility = 0.35
tank_wall_roughness = 0.08

environment_reflection_mode = "procedural_room"
environment_rotation = 0.0
environment_brightness = 1.0
```

---

# Temporal stabilization

## Target

Prevent shimmer/flicker from:

```text
particle density changes
mesh extraction changes
normal noise
foam popping
caustic noise
thickness noise
```

## First implementation

Add temporal smoothing for:

```text
thickness
normals
caustics
foam target
```

Use conservative reprojection if camera matrices and velocity are available.

If full reprojection is too much in the first sprint, use simple history blending with reset on camera movement.

## Settings

```toml
[render.hero_water.temporal]
enabled = true

thickness_history = true
normal_history = true
caustic_history = true
foam_history = true

history_alpha = 0.85
camera_motion_reset_threshold = 0.02
depth_reject_threshold = 0.04
normal_reject_threshold = 0.35

jitter_enabled = false
```

---

# Debug and inspection modes

Every implementer must expose debug views.

Required debug modes:

```rust
enum HeroWaterDebugView {
    None,
    SceneColor,
    SceneDepth,
    DensityGridSlice,
    ActiveSurfaceCells,
    MeshNormals,
    MeshTriangles,
    Thickness,
    RefractionUvOffset,
    Fresnel,
    Absorption,
    FoamParticles,
    BubbleParticles,
    SprayParticles,
    Caustics,
    WetWall,
    FinalWaterOnly,
}
```

Settings:

```toml
[render.hero_water.debug]
enabled = false
view = "none"

grid_slice_axis = "z"
grid_slice_index = 32

show_mesh_wireframe = false
show_surface_particles = false
show_active_cells = false
show_diffuse_particles = false

freeze_diffuse = false
freeze_caustics = false
freeze_surface = false

stats_overlay = true
```

Stats overlay should include:

```text
particle count
surface particle count estimate
active render cells
mesh vertices
mesh triangles
diffuse particles alive
foam/spray/bubble counts
hero water GPU time if available
fallback mode if active
```

---

# Quality tiers

Add preset quality tiers.

```rust
enum HeroWaterQuality {
    Low,
    Medium,
    High,
    Ultra,
}
```

## Low

```text
screen-space fallback or occupancy quads
half-res thickness
low foam count
no mesh smoothing
cheap caustics
```

## Medium

```text
dense density grid
basic marching cubes or surface mesh
scene refraction
Fresnel
absorption
basic foam/spray
cheap caustics
```

## High

```text
higher render grid scale
mesh smoothing
better normals
persistent diffuse particles
caustics with temporal smoothing
wet walls
micro normals
```

## Ultra

```text
higher active-cell budget
larger diffuse-particle budget
better thickness
back-depth if available
more smoothing controls
higher caustic resolution
```

Settings:

```toml
[render.hero_water]
enabled = true
quality = "high"
fallback_mode = "screen_space_water"
allow_dynamic_quality = true
target_frame_ms = 16.6
```

---

# Parallel implementation plan

Use multiple implementers. Each implementer must work behind feature flags and expose settings.

---

## Implementer A: Hero renderer shell and settings

### Goal

Create the hero renderer mode, settings plumbing, debug routing, and pass structure.

### Tasks

```text
1. Add HeroWater render mode.
2. Add render.hero_water settings block.
3. Add quality presets.
4. Add debug-view enum.
5. Add pass skeletons:
   - scene prepass
   - surface reconstruction
   - water material pass
   - diffuse pass
   - caustics pass
   - wet-wall pass
   - final composite
6. Add safe fallback to current screen-space water.
7. Add runtime toggles.
8. Add stats overlay values.
```

### Deliverables

```text
hero_water.rs or equivalent module
hero_water_settings.rs or equivalent
hero_water.wgsl placeholder shaders
debug UI/config controls
fallback mode working
```

### Must not do

```text
Do not hardcode constants.
Do not remove existing render modes.
Do not require other branches to compile.
```

---

## Implementer B: Scene color/depth prepass and water material

### Goal

Make transparent water optically believable using refraction, reflection, Fresnel, and absorption.

### Tasks

```text
1. Render tank/background/floor/walls into scene_color and scene_depth.
2. Add hero water material shader.
3. Implement Fresnel.
4. Implement scene-color refraction.
5. Implement Beer-Lambert absorption.
6. Add environment reflection.
7. Add roughness controls.
8. Add invalid-refraction fallback.
9. Add debug views:
   - scene color
   - scene depth
   - Fresnel
   - absorption
   - refraction UV offset
```

### Deliverables

```text
scene prepass target creation
water material WGSL shader
material settings
debug outputs
```

### Required settings

Use the material settings block from this plan.

---

## Implementer C: Surface reconstruction

### Goal

Create a coherent surface from particles/grid data.

### Tasks

```text
1. Add render density grid.
2. Mark active surface cells from Liquid/Air classification.
3. Splat particles or occupancy into density field.
4. Smooth density field.
5. Implement at least one surface output:
   - occupancy boundary quads, or
   - dense marching cubes, or
   - dense SDF marching cubes.
6. Generate normals.
7. Draw surface with hero material.
8. Add debug views:
   - density slice
   - active cells
   - mesh normals
   - mesh wireframe
```

### Minimum acceptable implementation

If full marching cubes is too much, implement occupancy-boundary quads first.

The renderer can still look better than particles once the material/refraction pass is active.

### Better implementation

Dense marching cubes over the render density grid.

### Required settings

Use the surface settings block from this plan.

---

## Implementer D: Thickness/internal depth

### Goal

Give water optical depth.

### Tasks

```text
1. Reuse existing screen-space particle thickness target.
2. Feed thickness into hero water material.
3. Add thickness smoothing controls.
4. Add thickness debug view.
5. Optional: render mesh front/back depth and compute mesh thickness.
6. Add hybrid thickness mode.
```

### Minimum acceptable implementation

Use existing screen-space thickness.

### Better implementation

Combine:

```text
particle thickness
+ mesh front/back depth
+ depth-difference estimate
```

---

## Implementer E: Foam, bubbles, spray, mist

### Goal

Replace speed-weighted white tint with persistent diffuse-water state.

### Tasks

```text
1. Add DiffuseParticle buffer.
2. Add GPU counters for alive/emitted particles.
3. Add emission shader.
4. Emit from:
   - liquid-air interface
   - wall impacts
   - high speed
   - vorticity/turbulence if available
5. Add update shader:
   - age
   - decay
   - advection
   - buoyancy
   - drag
   - type transitions
6. Add render pass:
   - foam flecks
   - bubbles
   - spray droplets
   - mist
7. Add debug views and counters.
```

### Minimum acceptable implementation

One shared diffuse buffer with three visible types:

```text
foam
bubbles
spray
```

Mist can be added later.

### Required settings

Use the diffuse settings block from this plan.

---

## Implementer F: Caustics, wet walls, meniscus

### Goal

Make water visibly affect the tank environment.

### Tasks

```text
1. Add caustic target.
2. Generate approximate caustics from water normal/thickness.
3. Composite caustics onto floor/back wall.
4. Add wet-wall accumulation target.
5. Write wetness from particles/grid near walls.
6. Decay wetness over time.
7. Add meniscus highlight near wall/liquid interface.
8. Add contact shadow near tank boundaries.
9. Add debug views.
```

### Minimum acceptable implementation

```text
normal-gradient caustics
+ wet-wall darkening
+ meniscus highlight
```

### Required settings

Use caustic and wet-wall settings blocks from this plan.

---

## Implementer G: Integration, presets, tuning, acceptance tests

### Goal

Make all pieces controllable and comparable.

### Tasks

```text
1. Add quality presets.
2. Add hotkeys/UI toggles for hero features.
3. Add capture/test scenes:
   - calm fill
   - slosh
   - dam break
   - wall impact
   - corner churn
4. Add side-by-side mode if possible:
   - simple particles
   - current screen-space water
   - hero water
5. Add performance stats.
6. Add fallback reporting.
7. Tune default settings.
```

### Deliverables

```text
preset configs
debug UI
acceptance scene list
default hero_water config
```

---

# 8-hour execution plan

## Hour 0: Branch setup and contracts

Do this first.

```text
1. Create hero_water feature branch.
2. Define settings structs/enums.
3. Define shader/resource naming conventions.
4. Define render pass order.
5. Define debug-view enum.
6. Assign implementers to modules.
```

Outcome:

```text
project still compiles
existing render modes unchanged
hero_water mode exists but may fallback
```

---

## Hours 1-2: Scene prepass + renderer shell

Parallel work:

```text
Implementer A:
  hero renderer shell
  settings
  quality presets
  fallback path

Implementer B:
  scene_color / scene_depth prepass
  basic transparent water material shader
```

Outcome:

```text
HeroWater mode can render existing water surface/fallback through new material pass.
Scene refraction infrastructure exists.
```

---

## Hours 2-4: Surface reconstruction first pass

Parallel work:

```text
Implementer C:
  density grid or occupancy-quads surface
  normals
  mesh draw path

Implementer D:
  reuse existing thickness buffer
  feed thickness into material
```

Outcome:

```text
HeroWater can show a coherent-ish surface.
Water material receives normal + thickness.
```

Fallback if mesh extraction fails:

```text
use occupancy quads
or current smoothed front-depth surface
but keep hero material path alive
```

---

## Hours 4-6: Diffuse water + caustics/wet cues

Parallel work:

```text
Implementer E:
  diffuse particle buffer
  basic foam/spray/bubble emission
  update/render pass

Implementer F:
  approximate caustics
  wet-wall accumulation
  meniscus/contact cue
```

Outcome:

```text
Impacts create persistent visual detail.
Tank environment receives water cues.
```

---

## Hours 6-7: Integration and debug views

Parallel work:

```text
Implementer G:
  presets
  debug views
  stats overlay
  fallback reporting

All implementers:
  expose missing config knobs
  remove hardcoded constants
  add feature toggles
```

Outcome:

```text
All major features can be toggled and tuned independently.
```

---

## Hours 7-8: Tuning and acceptance pass

Tune against fixed scenes.

Compare:

```text
SimpleParticles
ScreenSpaceWater
HeroWater
```

Tune in this order:

```text
1. Surface iso/smoothing
2. Refraction strength
3. Absorption color/strength
4. Reflection/Fresnel
5. Foam emission/lifetime
6. Bubble milkiness
7. Spray radius/alpha
8. Caustic intensity
9. Wet-wall/meniscus strength
```

Outcome:

```text
HeroWater should produce at least one scene that clearly beats simple particles and current screen-space water.
```

---

# Acceptance scenes

Use fixed seed, fixed particle count, fixed camera.

## Scene 1: Calm fill

Tests:

```text
surface smoothness
transparency
absorption
floor refraction
reflection
```

Pass condition:

```text
Water reads as transparent liquid without foam.
Floor/backdrop visibly refracts through the water.
```

---

## Scene 2: Slosh against wall

Tests:

```text
wall contact
wet wall
meniscus
foam birth
surface continuity
```

Pass condition:

```text
Water feels attached to the tank boundary.
Wall impacts leave visible but decaying wetness/foam.
```

---

## Scene 3: Dam break

Tests:

```text
large moving surface
turbulence
foam
spray
caustics
temporal stability
```

Pass condition:

```text
Hero renderer beats simple particles in still captures and motion.
```

---

## Scene 4: Corner churn

Tests:

```text
bubbles
aeration
foam persistence
whitewater decay
```

Pass condition:

```text
Whitewater persists after impact and decays instead of flashing only while fast.
```

---

## Scene 5: Thin sheet / splash

Tests:

```text
screen-space failure
mesh/surface robustness
spray fallback
particle fallback
```

Pass condition:

```text
Thin features do not become giant blobs.
Spray remains particle-like and reflective.
```

---

# Definition of done for the 8-hour prototype

The prototype is acceptable if:

```text
1. Existing render modes still work.
2. HeroWater mode can be toggled on/off.
3. HeroWater has at least one coherent surface path.
4. HeroWater uses scene refraction.
5. HeroWater uses Fresnel/environment reflection.
6. HeroWater uses thickness/absorption.
7. Foam/spray/bubbles are persistent state, not only a speed mask.
8. Caustics or wet-wall cues exist.
9. Debug views exist for surface, thickness, material, diffuse particles, and caustics.
10. Major constants are exposed as config settings.
11. The renderer safely falls back if hero resources fail.
12. At least one fixed test scene clearly looks better than SimpleParticles.
```

---

# Definition of done for the real end-game version

The end-game interactive renderer is acceptable if:

```text
1. Mesh/SDF surface is stable across all scenes.
2. Surface does not look blobby, blocky, or particle-like.
3. Transparent material works from multiple camera angles.
4. Refraction has valid fallbacks and does not shimmer excessively.
5. Deep water has believable absorption.
6. Foam persists, advects, clumps, breaks up, and decays.
7. Bubbles make impact zones look aerated.
8. Spray reads as reflective droplets.
9. Caustics move plausibly across floor and walls.
10. Wet walls and meniscus make tank contact feel physical.
11. Quality presets scale across devices.
12. SimpleParticles remains available as debug/fallback.
```

---

# Major risks and fallbacks

## Risk: marching cubes takes too long

Fallback:

```text
implement occupancy-boundary quads first
then apply hero material
then replace with density marching cubes later
```

## Risk: surface is too blocky

Mitigations:

```text
increase render_grid_scale
increase smoothing_iterations
use density instead of binary occupancy
add mesh Laplacian smoothing
improve normal smoothing
```

## Risk: surface is too blobby

Mitigations:

```text
lower particle_splat_radius_cells
raise iso_value
reduce smoothing_strength
detect thin sheets separately
use simple particles for spray/droplets
```

## Risk: refraction looks broken

Mitigations:

```text
clamp max refraction offset
fallback to unrefracted scene color
blur scene_color mip for invalid samples
reduce refraction_strength
increase absorption so errors are less obvious
```

## Risk: foam looks like white paint

Mitigations:

```text
lower foam alpha
add noise breakup
increase foam decay
make foam patchy
emit only near interface/impacts
separate bubbles from surface foam
```

## Risk: bubbles look like noise

Mitigations:

```text
lower bubble count
increase bubble radius slightly
make bubbles depth-dependent
render bubbles as milkiness instead of hard dots
```

## Risk: caustics look fake

Mitigations:

```text
reduce intensity
blur more
temporal smooth more
only show on floor/back wall
drive by surface normal variation
```

## Risk: performance collapses

Mitigations:

```text
lower render_grid_scale
lower caustic resolution
lower diffuse max_particles
turn off mesh smoothing
use half-res thickness
use dynamic quality
fallback to ScreenSpaceWater
```

---

# Recommended default configuration

Use this as the first default hero-water config.

```toml
[render.hero_water]
enabled = true
quality = "medium"
fallback_mode = "screen_space_water"
allow_dynamic_quality = true
target_frame_ms = 16.6

[render.hero_water.surface]
enabled = true
mode = "dense_density_marching_cubes"
render_grid_scale = 1.0
max_active_cells = 1000000
particle_splat_radius_cells = 1.2
particle_splat_strength = 1.0
density_fixed_point_scale = 4096
iso_value = 0.45
smoothing_iterations = 2
smoothing_strength = 0.3
normal_mode = "density_gradient"
normal_smoothing = 0.5
surface_only_particles = true
surface_air_neighbor_radius = 1
mesh_laplacian_iterations = 1
mesh_laplacian_strength = 0.12
max_vertices = 4000000
max_triangles = 2000000
fallback_to_screen_space = true
fallback_to_simple_particles = true

[render.hero_water.material]
enabled = true
ior = 1.333
f0 = 0.02037
base_tint = [0.72, 0.92, 1.0]
absorption_color = [0.08, 0.035, 0.015]
absorption_strength = 1.0
transparency = 0.92
deep_water_darkening = 0.45
refraction_strength = 0.035
refraction_thickness_scale = 1.0
refraction_max_offset_px = 48.0
invalid_refraction_fallback = "scene_color"
reflection_strength = 1.0
environment_strength = 0.8
environment_mode = "procedural_room"
skybox_enabled = true
roughness_base = 0.015
roughness_velocity_scale = 0.05
roughness_normal_variance_scale = 0.1
roughness_foam_scale = 0.35
specular_strength = 1.0
sun_direction = [0.25, -0.7, 0.45]
sun_intensity = 1.0
normal_strength = 1.0
micro_normal_enabled = true
micro_normal_strength = 0.035
micro_normal_scale = 18.0
micro_normal_velocity_scale = 0.5

[render.hero_water.thickness]
mode = "hybrid"
use_existing_particle_thickness = true
use_mesh_depth_difference = false
thickness_scale = 1.0
thickness_bias = 0.0
thickness_min = 0.0
thickness_max = 3.0
smooth_thickness = true
thickness_smoothing_radius = 2.0
temporal_thickness = true
temporal_alpha = 0.85

[render.hero_water.diffuse]
enabled = true
max_particles = 300000
emit_enabled = true
emit_rate = 1.0
emit_budget_per_frame = 12000
surface_emission = true
surface_speed_threshold = 0.8
surface_speed_gain = 1.0
wall_impact_emission = true
wall_impact_threshold = 0.5
wall_impact_gain = 2.0
vorticity_emission = true
vorticity_threshold = 0.4
vorticity_gain = 1.0
compression_emission = true
compression_threshold = 0.2
compression_gain = 0.8
foam_enabled = true
foam_lifetime_min = 1.0
foam_lifetime_max = 5.0
foam_radius = 0.015
foam_alpha = 0.85
foam_decay = 0.96
foam_advection = 0.9
foam_surface_lock = 0.6
bubble_enabled = true
bubble_lifetime_min = 0.7
bubble_lifetime_max = 4.0
bubble_radius = 0.008
bubble_alpha = 0.35
bubble_buoyancy = 0.4
bubble_advection = 0.95
bubble_milkiness = 0.45
spray_enabled = true
spray_lifetime_min = 0.25
spray_lifetime_max = 1.4
spray_radius = 0.004
spray_alpha = 0.9
spray_drag = 0.985
spray_reflection_strength = 1.0
mist_enabled = true
mist_lifetime_min = 0.15
mist_lifetime_max = 0.7
mist_radius = 0.035
mist_alpha = 0.18
mist_drag = 0.92
random_seed = 1337

[render.hero_water.caustics]
enabled = true
mode = "normal_gradient"
resolution_scale = 0.5
intensity = 0.35
focus_strength = 1.2
thickness_scale = 1.0
floor_enabled = true
back_wall_enabled = true
side_walls_enabled = false
blur_radius = 2.0
temporal_enabled = true
temporal_alpha = 0.9
motion_scale = 1.0
max_intensity = 1.5

[render.hero_water.wet_wall]
enabled = true
wetness_decay = 0.985
wetness_contact_gain = 1.0
wetness_spray_gain = 0.6
darkening_strength = 0.22
gloss_strength = 0.35
streak_strength = 0.1
meniscus_enabled = true
meniscus_width = 0.018
meniscus_strength = 0.45
meniscus_fresnel_boost = 0.4
contact_shadow_enabled = true
contact_shadow_strength = 0.25
contact_shadow_radius = 0.04

[render.hero_water.environment]
enabled = true
floor_pattern_enabled = true
floor_pattern_scale = 12.0
floor_pattern_strength = 0.18
backdrop_enabled = true
backdrop_mode = "gradient_grid"
backdrop_strength = 1.0
tank_wall_visibility = 0.35
tank_wall_roughness = 0.08
environment_reflection_mode = "procedural_room"
environment_rotation = 0.0
environment_brightness = 1.0

[render.hero_water.temporal]
enabled = true
thickness_history = true
normal_history = true
caustic_history = true
foam_history = true
history_alpha = 0.85
camera_motion_reset_threshold = 0.02
depth_reject_threshold = 0.04
normal_reject_threshold = 0.35
jitter_enabled = false

[render.hero_water.debug]
enabled = false
view = "none"
grid_slice_axis = "z"
grid_slice_index = 32
show_mesh_wireframe = false
show_surface_particles = false
show_active_cells = false
show_diffuse_particles = false
freeze_diffuse = false
freeze_caustics = false
freeze_surface = false
stats_overlay = true
```

---

# Final implementation instruction for Codex implementers

Each implementer should follow these constraints:

```text
1. Preserve all existing render modes.
2. Work behind render.hero_water.enabled.
3. Expose all meaningful constants as settings.
4. Add debug views for intermediate outputs.
5. Avoid CPU readback in normal frames.
6. Prefer GPU buffers/textures and compute passes.
7. Use existing particle and MAC grid buffers.
8. Use fixed-point/integer atomics where accumulation needs atomics.
9. Fail safely to ScreenSpaceWater or SimpleParticles.
10. Make the result tunable rather than trying to guess perfect constants.
```

The desired final architecture is:

```text
SimpleParticles        = debug/fallback/spray reference
ScreenSpaceWater       = medium-quality fallback and thickness infrastructure
HeroWater              = mesh/SDF surface + optical material + diffuse water + caustics + wet-wall cues
```

The immediate 8-hour goal is not perfection. The goal is to produce a configurable hero-water pipeline where each major realism cue exists, can be toggled, can be tuned, and can be improved independently.

```
```
