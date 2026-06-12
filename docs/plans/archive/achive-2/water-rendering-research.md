---
status:        draft
owner:         adamg
last_updated:  2026-06-08
okay_to_delete: false
long_lived:    false
owning_docs:
  - architecture/rendering.md
  - architecture/gpu-resources.md
  - architecture/settings.md
  - decisions/rendering.md
  - decisions/performance.md
---

# Water rendering research

## Mission

Choose a water-look direction for the deleted `v1.10.0-water-rendering-optical-depth.md`
implementation plan. This is an archived research/planning doc for agents. It should
not update architecture or decisions docs before implementation starts.

## User constraints

- The first useful direction can be particle-native, weighted transparency, volume-like,
  or another measured approach. The user has no fixed preference yet.
- The result should improve both lower particle counts reading denser and higher
  particle counts reading more beautiful where practical.
- Heavy configuration is a priority. Do not collapse the look into one hardcoded
  cinematic preset.
- Multi-pass rendering is acceptable if the cost is measured and the implementation
  plan names the resource/timing changes.
- Marching cubes can be discussed as context, but the previous attempt had serious
  visual quality problems. Do not recommend reviving it unless the new plan explains
  exactly what is different.
- Preserve inspectability. A prettier default is fine, but the app is still a fluid
  lab with configurable/debug views.

## Current renderer and failure modes

`crates/fluid-lab/src/gpu/particles.rs -> ParticleRenderer` binds the live particle
storage buffer and draws one camera-facing quad per particle through
`crates/fluid-lab/src/gpu/shaders/particles.wgsl`. The shader colors particles by
speed, applies a circular soft edge, and optionally shades each billboard like a
small sphere. The pipeline uses alpha blending and currently writes depth.

The important visual failures are:

- **Flat opacity, not optical depth.** `render.particle_alpha` is a direct alpha
  multiplier after edge fade. It does not distinguish a single thin droplet from a
  dense stack of droplets.
- **Depth writes fight transparent buildup.** Because particle fragments write depth,
  the nearest transparent billboard can prevent deeper particles from contributing,
  so overlap does not reliably read as accumulated water thickness.
- **Sphere shading reads as beads.** The diffuse/specular billboard shading helps 3D
  shape, but at medium/high strength it makes water look like shaded balls instead of
  a continuous translucent volume.
- **Low-count gaps are exposed.** Increasing `render.particle_size` hides gaps, but
  with flat alpha it also makes discs over-opaque and can smear motion.
- **Speed tint can bleach motion.** The slow-to-fast color ramp is useful, but fast
  particles can become too white if density and tint are not independently tunable.

## Visual target

Thin water should be pale and see-through at rims, spray, and one-particle-thick
regions. Dense overlapping water should accumulate blue/cyan absorption and read as
more volume without becoming an opaque wall of shaded dots. Motion should stay
legible through the existing speed color ramp, but the default should favor water
mass over individual sphere highlights. Low particle counts should read as coherent
splash/volume where practical, while high counts should look smoother and less
bead-like. Debug inspectability remains mandatory: users must still be able to tune
size, density, edge softness, tint, and shading enough to expose simulation artifacts.

## Candidate comparison

| Direction | Visual upside | Implementation risk | Resource/perf cost | Required controls | Observability trade-off |
|---|---|---|---|---|---|
| **Particle-native optical-density billboards** | Turns each particle into a translucent volume sample: thin rims fade, centers gain thickness, overlapping particles read denser. Keeps the current product surface. | Low/medium. Requires shader semantics, setting rename, and an explicit particle depth-write decision. Still approximate because particles are unsorted. | Smallest first step. No new storage buffers or offscreen targets if kept in the current pass; render cost should mostly be fragment math and any depth/blend-state change. | Optical density, particle size, edge softness, sphere-shading strength, slow/fast color, speed scale. | Strong. Particles remain visible and configurable. Depth-write-off or unsorted blending artifacts must be documented with captures. |
| **Weighted blended transparency / OIT approximation** | Better order-independent accumulation for overlapping particles; can make high-count water read more uniformly dense. | Medium/high. Needs accumulation/revealage render targets and a composite pass. Must thread resize/recreate, render order, and timing updates. | Adds offscreen color targets and at least one composite pass; render timing must split accumulation vs composite if this becomes the chosen path. | Density/weight scale, revealage scale, color/tint, size, edge softness, debug weight display or mode. | Moderate. Better beauty, but accumulation hides individual particles unless a debug mode remains. |
| **Screen-space density splats** | Can make low particle counts read as a smoother water mass by splatting density, optionally blurring, then compositing. | High. Reintroduces density/blur/blit-style resources that were removed with the old surface path, though not marching cubes itself. Needs careful viewport/depth behavior. | Offscreen density texture, likely blur pass, composite pass, possible depth/normal texture; cost scales with screen resolution as well as particles. | Splat radius, density scale, blur radius/iterations, absorption color, debug density view. | Lower by default. Good density maps can obscure particle-level artifacts; must keep raw particles and slice views available. |
| **Liquid-cell / voxel volume view** | Uses existing grid occupancy/cell-type structure to show actual simulated liquid regions and can reveal bulk volume independent of particle count. | Medium/high. Current slice renderer is a 2D inspection view, not a 3D volume renderer. A full volume view must define sampling, compositing, and depth behavior. | Cost scales with grid/cell samples or slices. Could stay cheaper than particle-heavy rendering at high counts, but needs measurement. | Density per cell, absorption, step count/slice count, mode selector, pressure/speed overlays. | Strong for lab inspection, weaker for beautiful free-surface water. Best as an inspection mode, not first default water look. |
| **Fresh surface renderer, not old marching cubes revived** | Potentially most "surface-like" if it solves smoothing, normals, lighting, and material coherently. | Very high. The old MC path was removed because it polluted product direction, settings, memory, and render timing. A return must be a new renderer with an explicit product role. | Extraction buffers/passes, mesh or surface textures, material pass, possible offscreen refraction/depth targets; must be justified with profiler data. | Extraction resolution, smoothing/kernel width, iso threshold, normal/material controls, debug raw-surface mode. | Weakest unless paired with particle/grid debug modes. Not recommended for v1.10. |

## Recommendation

Use **particle-native optical-density billboards** as the first implementation
direction. It is the smallest change that directly addresses the current opacity
failure, keeps the particle/liquid-cell product direction intact, and does not require
new render resources before evidence proves they are needed.

The first implementation should:

1. Replace direct opacity semantics with optical density/transmittance semantics in the
   particle shader. Do not keep the label "opacity" for the main control if the value
   now means density.
2. Treat billboard thickness as sphere-like path length so centers are denser than
   edges, while preserving the existing configurable edge softness.
3. Explicitly evaluate particle depth writes. The expected first version disables
   particle depth writes while keeping depth testing, so transparent overlap can build
   up; if implementation keeps depth writes, the plan must justify why with captures.
4. Keep the renderer in the existing swapchain pass for v1.10 unless baseline captures
   prove order artifacts are the dominant problem. Weighted blended transparency then
   becomes the follow-up, not the first step.
5. Keep raw particle controls and grid-slice inspection available. A prettier default
   cannot hide solver artifacts behind an unconfigurable material.

Do not revive marching cubes in v1.10. A future surface renderer would need to be
different from the removed path in product role and implementation: no hidden
default-off compatibility stack, no MC-only dead settings, no unmeasured lazy
resources, and a new plan that proves smoothing/normals/material quality and render
cost before implementation.

## Required configurable controls

The first implementation should expose these controls through the existing settings
registry and live `set_setting` path where practical:

| Setting | Action | Apply class | Purpose |
|---|---|---|---|
| `render.water_optical_density` | Add; replaces `render.particle_alpha` as the main transparency control. | Live | Controls per-particle optical density/transmittance. This must not be labeled opacity. |
| `render.particle_size` | Keep. | Live | Visual radius; main low-count gap and volume-readability control. |
| `render.particle_edge` | Keep, or rename only if the UI wording changes with semantics. | Live | Controls soft rim/thickness falloff so spray and edges can stay transparent. |
| `render.particle_shading` | Keep, with a less bead-like default if captures support it. | Live | Lets users trade water mass against individual sphere highlights. |
| `render.particle_slow_color` / `render.particle_fast_color` | Keep. | Live | Water tint and speed-highlight colors. |
| `render.speed_scale` | Keep. | Live | Keeps motion tint configurable independently from optical density. |

Optional follow-up controls such as `render.water_thickness_power`, weighted-blend
weight scale, blur radius, or density debug mode belong only if the first captures show
one optical-density knob cannot tune thin rims and dense centers separately.

## Evidence requirements

Before implementation starts, capture the current renderer as baseline. After the
change, repeat the same captures with the same scene, camera, colors, particle count,
grid, pressure iterations, interaction state, and wait window. Each capture must keep
the PNG and the `<capture>.console.txt` file with the final `stats_json` line.

Minimum evidence matrix:

| Case | Purpose | Required facts |
|---|---|---|
| Default count, default scene | Shows normal product look and cost. | requested/actual particles, grid dims, pressure iterations, camera settings or default assertion, render settings, `gpu.render_ms`, frame p50/p95, timing source. |
| Low-count stress, e.g. `PARTICLES=32768` | Shows whether gaps read denser without becoming opaque discs. | Same facts as default, plus screenshots before/after with identical particle request. |
| High-count practical check, e.g. `PARTICLES=2000000` if accepted on the machine | Shows whether the look improves rather than just masking low-count gaps. | Same facts as default; if rejected or too slow, record the rejection/skip from `stats_json` instead of inventing a result. |

Profiler evidence must be timing-source honest. Prefer real `gpu-timestamp` data; if the
adapter lacks timestamps, record the labeled fallback and avoid precise GPU-cost claims.
For the recommended first direction, render cost can use the existing `render_ms` timing.
If implementation adds any offscreen target, render pass, composite pass, persistent
buffer, or readback, this research recommendation no longer covers the full scope and
the implementation plan must name the new resource and timing changes before code lands.

## Discipline rules

- Do not claim "realistic water" without side-by-side evidence.
- Do not hide solver artifacts so completely that inspection becomes impossible.
- Do not update architecture or decisions docs from research alone.
- Do not make opacity controls lie. If opacity becomes optical density, rename and
  document that in the implementation plan.

## See also

- `v1.10.0-water-rendering-optical-depth.md` - deleted implementation plan
- `water-rendering-optical-depth.md` - deleted superseded source draft
- [`future-roadmap.md`](../../future-roadmap.md)
- [`../../../architecture/rendering.md`](../../../architecture/rendering.md)
- [`../../../decisions/rendering.md`](../../../decisions/rendering.md)
