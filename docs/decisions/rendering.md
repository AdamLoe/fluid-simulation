---
status:        active
owner:         adamg
last_updated:  2026-06-12
---

# Decisions - Rendering

## Debug and inspection views are GPU-native

**Decision** - Particle and grid-slice views sample GPU buffers directly; CPU/GPU
readback is reserved for throttled diagnostics, explicit captures, and profiler
snapshots.

**Why** - The inspection layer must remain credible at the same scales as the
simulation it exposes.

**Tradeoffs** - Render/inspection views are more involved to implement GPU-side, but they
do not turn observation into the dominant frame cost.

**Applies to** - `architecture/rendering.md`, `architecture/profiler.md`.

## Screen-space water and inspection views are the product rendering surface

**Decision** - The product rendering surface is the screen-space water composite,
optical/simple particle views, tank wireframe, and liquid-cell/grid inspection. There
is no extracted-surface compatibility path in the current product.

**Why** - These views make scale, motion, and solver state legible without carrying a
second heavyweight representation. A reconstructed surface has not earned a runtime
slot because the current inspection views already cover the solver's behavior.

**Tradeoffs** - The default water view hides some per-particle detail in exchange for
a more coherent liquid body; the optical and simple particle views remain selectable
for motion, solver inspection, and fallback comparison.

**Revisit when** - A reconstructed surface clearly beats screen-space on thin
features without pushing the frame budget out of range.

**Code anchors** - `crates/fluid-lab/src/gpu/mod.rs -> GpuContext::render`;
`crates/fluid-lab/src/gpu/particles.rs -> ParticleRenderer`;
`crates/fluid-lab/src/gpu/composite.rs -> CompositeRenderer`;
`crates/fluid-lab/src/gpu/smoothing.rs -> WaterSmoothRenderer`;
`crates/fluid-lab/src/gpu/slice.rs -> SliceRenderer`.

**Applies to** - `architecture/rendering.md`, `architecture/gpu-resources.md`,
`architecture/web-shell.md`.

## Water rendering uses a measured multi-pass screen-space path

**Decision** - In Water mode a scene prepass renders the environment and wireframe
into offscreen `scene_color`/`scene_depth` targets; water accumulates thickness,
speed-weighted whitewater, and nearest depth into R16 screen-space targets; smoothing
filters the front depth; and the composite samples `scene_color` at a refracted UV
before writing the final pixel. The optional grid slice remains an overlay. The
optical/simple particle modes keep the direct opaque pass.

**Why** - Same-pass transparent billboards cannot accumulate path length or produce a
coherent lit front surface for deep water. The multi-pass path pays explicit render
memory and pass cost for an order-independent thickness signal and smoothed surface
normal.

**Tradeoffs** - Render timing and memory are less trivial than a single swapchain
pass, and the Water path adds more swapchain-sized targets. The public profiler still
reports one `gpu.render_ms` total for the whole render path rather than per-pass water
timing.

**Applies to** - `architecture/rendering.md`, `architecture/gpu-resources.md`.

## Surface fidelity uses a curvature-adaptive screen-space filter, not an extracted surface

**Decision** - The front-surface depth smoothing and normal reconstruction are
**curvature-adaptive**: where local surface curvature (measured at a coarse stencil) is
high, the depth bilateral's spatial Gaussian narrows and the normal cross-average is
suppressed, so crests/ridges/tips stay sharp while flat sheets stay glassy. One Live knob
(`render.hero.feature_preservation`, 0 = legacy isotropic) drives it. It stays in the
existing screen-space composite; no SDF / level-set / marching-cubes surface is added.

**Why** - An isotropic bilateral rounds everything equally, so no single radius yields
smooth-sheets-and-sharp-features at once (more iterations = blobbier, fewer = speckled
spheres). Modulating by curvature separates the two regimes in one cheap pass. The
curvature is sampled at a *coarse* stencil on purpose: a genuine ridge spans several
pixels and registers, while a single-splat bump does not — otherwise the filter would
preserve the per-splat speckle it exists to remove. A reconstructed surface remains a
heavier, separate project that has not earned a runtime slot.

**Alternatives considered** - A globally narrower range ("narrow-range bilateral") — but
that preserves splat noise everywhere, not just at real features. Anisotropic
(ellipsoid / Yu–Turk) splats — deferred as a gated stretch because they add cost to the
per-particle hot path that is the known bottleneck
([`performance.md`](performance.md)); screen-space curvature flow was sufficient on the
common scenes (verified on real-GPU captures of the default scene, settled pool and
mid-splash).

**Tradeoffs** - Feature preservation reintroduces some real surface detail that reads as
chop; at extreme settings it can surface faint per-splat structure. The default (0.6) is
a conservative middle. Cost is a handful of extra texture taps per smoothed pixel.

**Code anchors** - `crates/fluid-lab/src/gpu/shaders/water_smooth.wgsl`;
`crates/fluid-lab/src/gpu/shaders/composite.wgsl -> water_normal`;
`crates/fluid-lab/src/gpu/smoothing.rs -> WaterSmoothRenderer`;
`crates/fluid-lab/src/settings/mod.rs -> HeroParams` (`feature_preservation`).

**Revisit when** - Thin airborne droplet tips still read too round on violent slosh and
anisotropic splats (Phase 2) earn their per-particle cost on measured captures.

**Applies to** - `architecture/rendering.md`, `architecture/settings.md`.

## Hero water features are Live sub-features of the Water view, not new render modes

**Decision** - The Water mode keeps hero-water controls as Live-toggleable sub-features
of the existing render path rather than as a separate top-level render mode.
`RenderMode { Water, OpticalParticles, SimpleParticles }` remains the mode switch;
hero features are Live settings under `Water` and mirror into one `HeroParams`
uniform.

**Why** - The composite already does most of the material (thickness, smoothed front
depth, reconstructed normal, Fresnel, Beer-Lambert absorption). A second top-level mode
would duplicate that renderer. Keeping hero features Live keeps the broad control
surface navigable with no reset and no pipeline rebuilds.

**Tradeoffs** - One composite shader grows in complexity and uniform size across the
series instead of being split into independent pipelines. The old
`render.hero.mode_enabled` id is compatibility-only; visible controls split refraction,
reflection, body color, and wall-contact correction so comparisons do not disable
unrelated effects.

**Code anchors** - `crates/fluid-lab/src/gpu/mod.rs -> RenderMode`;
`crates/fluid-lab/src/gpu/composite.rs`; `crates/fluid-lab/src/gpu/environment.rs`;
`crates/fluid-lab/src/settings/mod.rs -> HeroParams`.

**Applies to** - `architecture/rendering.md`, `architecture/settings.md`,
`architecture/gpu-resources.md`.

## Weak hero-water add-ons are removed until they earn a new case

**Decision** - Caustics, temporal stabilization, wet walls, and dense wall fill are
not shipped runtime feature groups. Their modules, shaders, pass scheduling, visible
settings, debug views, and resource allocations are removed. Old persisted ids replay
safely as hidden compatibility no-ops. The cheap wall-contact normal/depth snap
remains.

**Why** - Default Water should be a coherent liquid body with a small, understandable
control surface. The removed add-ons had cost, artifacts, or weak visual value without
enough evidence to justify runtime ownership.

**Tradeoffs** - Reintroducing any of those effects requires a fresh plan, measured
captures, runtime ownership, settings, docs, and profiler evidence. Legacy replay is
kept so old localStorage payloads do not break startup.

**Code anchors** - `crates/fluid-lab/src/settings/mod.rs -> Registry`;
`crates/fluid-lab/src/gpu/mod.rs -> GpuContext::render`.

**Applies to** - `architecture/rendering.md`, `architecture/settings.md`,
`architecture/gpu-resources.md`.

## Whitewater foam is conservative surface particles, not spray/bubbles

**Decision** - Whitewater can use persistent, render-only surface foam particles born
only at moving liquid-air cells. There is no spray, no bubbles, no wall-impact spawn,
no airborne confetti, and no wall decals. The original speed-weighted whitewater
target remains the fallback signal. Foam particles do not conserve mass, affect
pressure, or feed back into the solver.

**Why** - A fast-water mask alone reads as a white *tint* that flashes only while
water is moving; it cannot show foam that lingers a second or more after an impact and
fades. Persistent diffuse state is what makes churn read as foam (see the
`fluid-system-llm-brief.md` diagnosis). Keeping it render-only avoids touching the
fixed-point P2G determinism invariant (`../architecture/simulation.md`).

**Tradeoffs** - A fixed-capacity particle buffer (~12.6 MB) and two extra compute
passes per enabled frame. Emission is bounded by an integer-atomic per-frame budget,
and over-budget frames are reported (`stats_json.gpu.diffuse.clamped`). Slot choice is
not deterministic because of the atomic ring cursor, which is acceptable because the
system is render-only.

**Code anchors** - `crates/fluid-lab/src/gpu/diffuse.rs -> DiffuseSystem`;
`crates/fluid-lab/src/gpu/shaders/diffuse_emit.wgsl`;
`crates/fluid-lab/src/settings/mod.rs -> DiffuseParams`.

**Applies to** - `architecture/rendering.md`, `architecture/settings.md`,
`architecture/gpu-resources.md`.

## The reflected environment is procedural-only and world-fixed

**Decision** - The hero water reflects a *procedural* sky/room
(`gpu/shaders/env.wgsl -> env_sample`), not an image-based cubemap/HDRI and not
screen-space reflections of the actual tank/particles. The same function also draws
the world background as a fullscreen skybox. Both are sampled in **world space via a
camera-only rotation**, so they follow the camera but stay fixed when the box rotates.

**Why** - A procedural environment costs no texture memory or asset pipeline and gives the
water believable Fresnel edges and a plausible reflected room/sky for near-zero render
cost. Sampling in world space (not the box-folded `view_proj`) keeps the conceptual model
honest: rotating the box re-aims gravity (`../architecture/app-shell.md`); it must not spin
the world. SSR and real IBL are a heavier, separate project and out of this series.

**Tradeoffs** - The reflection is a stylized environment, not a true mirror of the scene;
it cannot show the tank's own geometry reflected. Roughness softening blends toward an
averaged sky rather than a true pre-filtered mip. Micro-normals can shimmer, so they
default off unless a future stabilizer returns.

**Code anchors** - `crates/fluid-lab/src/gpu/shaders/env.wgsl -> env_sample`;
`crates/fluid-lab/src/gpu/skybox.rs -> SkyboxRenderer`;
`crates/fluid-lab/src/gpu/composite.rs -> hero_uniform`;
`crates/fluid-lab/src/lib.rs -> FluidApp::frame` (the camera-only `eye_to_world`).

**Applies to** - `architecture/rendering.md`, `architecture/settings.md`.

## Removed add-ons keep only compatibility decisions

**Decision** - Caustics, wet-wall material cues, dense wall fill, and temporal history
blend are not active product decisions. Their old design notes remain history in git,
but current docs describe only their removed status and legacy replay compatibility.

**Why** - Keeping stale runtime decisions in active docs makes future implementers
rebuild deleted systems by accident.

**Applies to** - `architecture/rendering.md`, `architecture/gpu-resources.md`,
`architecture/settings.md`.

## The tank has an open viewing corner

**Decision** - The environment prepass omits the right (+x) and front (+z) walls, leaving
two adjacent clear faces that form an open vertical corner aimed at the default camera; the
back and left walls stay matte. The wireframe still outlines all 12 edges.

**Why** - Two opposing-pair open faces let the viewer look straight down the corner into
the liquid without matte glass occluding the hero shot, while the remaining two walls still
give refraction/reflection a backdrop and the floor checker reads through the water.

**Code anchors** - `crates/fluid-lab/src/gpu/environment.rs -> environment_mesh`.

**Applies to** - `architecture/rendering.md`.

## Particle and grid representations stay separate

**Decision** - Fluid particles and the MAC simulation grid remain distinct
representations that can be rendered and inspected independently.

**Why** - Particles track moving mass while the grid owns pressure and velocity
solves; preserving that distinction makes simulation behavior easier to inspect.

**Tradeoffs** - Separate buffers and renderers cost some plumbing in exchange for
clear ownership and replaceable views.

**Applies to** - `architecture/rendering.md`, `architecture/simulation.md`.

## Visual styling must preserve observability

**Decision** - The default water material may use accumulated thickness, smoothing,
lighting, and a whitewater mask, but the app must keep direct particle/grid inspection
reachable.

**Why** - Rendering is useful here when it helps a viewer read the fluid, not when it
hides solver behavior behind cinematic polish.

**Applies to** - `architecture/rendering.md`, `architecture/settings.md`,
`architecture/profiler.md`.

## Water opacity is normalized screen-space thickness, not particle alpha

**Decision** - Water opacity comes from Beer-Lambert absorption over normalized
screen-space thickness, while rough whitewater comes from speed-weighted thickness.
`render.water_optical_density` is the public absorption control; `render.particle_alpha`
is legacy compatibility only and not part of the public settings surface.

**Why** - Per-billboard optical depth made thin water readable but could not distinguish
a lone particle from the front of a deep volume. Normalized screen-space thickness
keeps one absorption setting meaningful across particle counts and stops particle size
from acting as hidden opacity.

**Refinement (thickness and whitewater are spatially smoothed)** - The thickness target,
and the whitewater (foam) target, are each blurred by a plain separable Gaussian
(`ThicknessSmoothRenderer`) before they drive composite colour. Left raw, the per-particle
splat noise read directly as speckle: thickness as a "sandy" body that let the dark wall
show through inter-splat gaps near the glass, and whitewater as a field of white foam
speckle dots all over moving water — i.e. the *accumulation signals*, not the surface
normal (which the bilateral depth blur already smooths), were the source of the "pixelly"
and "gap against the glass" artifacts. The Gaussian makes both spatially coherent (a
continuous sheet up to the wall; foam as soft regions) while keeping the models unchanged:
opacity is still Beer-Lambert over normalized screen-space thickness, foam is still the
speed-weighted whitewater mix — just denoised. This replaced an earlier bandage that
suppressed particle thickness in a band near vertical glass, which was unnecessary once
thickness was smoothed. **Limitation:** sparse *airborne* water (spray thrown by a violent
slosh/crash) is still individual particles, so it renders as discrete soft billboards, not
a smooth sheet — a known screen-space-particle limit, distinct from the speckle this fixed.

**Tradeoffs** - Absorption-over-thickness stays the opacity model. Refraction samples
an offscreen scene-color prepass and distorts the background, but opacity is still
normalized screen-space thickness, not particle alpha. The scene-color target and
visible scene detail make the cost legible.

**Code anchors** - `crates/fluid-lab/src/gpu/particles.rs -> ParticleRenderer`;
`crates/fluid-lab/src/gpu/shaders/particles.wgsl`;
`crates/fluid-lab/src/gpu/composite.rs -> CompositeRenderer`;
`crates/fluid-lab/src/settings/mod.rs -> Registry`;
`crates/fluid-lab/src/lib.rs -> FluidApp::set_setting`.

**Applies to** - `architecture/rendering.md`, `architecture/settings.md`,
`architecture/gpu-resources.md`.

## See also

- [`../architecture/rendering.md`](../architecture/rendering.md)
- [`performance.md`](performance.md) - render memory/cost policy
- [`scope.md`](scope.md) - product framing
- [`../agent-context/maintaining-docs.md`](../agent-context/maintaining-docs.md)
