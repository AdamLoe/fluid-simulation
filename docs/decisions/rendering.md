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
optical/simple particle views, tank wireframe, and liquid-cell/grid inspection; there
is no extracted-surface compatibility path.

**Why** - These views make scale, motion, and solver state legible without carrying a
second heavyweight representation that distracts from the fluid-lab direction. A v1.14
de-risk re-evaluated this: a throwaway occupancy-boundary-quad extracted surface was
built and A/B-captured against the screen-space composite on dam-break, double-splash,
and a thin mid-flight tongue. The extracted surface **lost in all three** (worst on the
thin tongue, which disintegrated into stippled noise) — at 64³ the occupancy surface is
too coarse for thin features, and dense marching cubes on top of an already
over-budget frame was not justified by a clear win. So screen-space stays and marching
cubes was **not** built; this re-affirms the decision rather than reversing it.

**Tradeoffs** - The default water view hides some per-particle detail in exchange for
a more coherent liquid body; the optical and simple particle views remain selectable
for motion, solver inspection, and fallback comparison.

**Revisit when** - A higher render-grid resolution makes a reconstructed surface beat
screen-space on thin features (the v1.14 design record for full marching cubes is in git).

**Code anchors** - `crates/fluid-lab/src/gpu/mod.rs -> GpuContext::render`;
`crates/fluid-lab/src/gpu/particles.rs -> ParticleRenderer`;
`crates/fluid-lab/src/gpu/composite.rs -> CompositeRenderer`;
`crates/fluid-lab/src/gpu/smoothing.rs -> WaterSmoothRenderer`;
`crates/fluid-lab/src/gpu/slice.rs -> SliceRenderer`.

**Applies to** - `architecture/rendering.md`, `architecture/gpu-resources.md`,
`architecture/web-shell.md`.

## Water rendering uses a measured multi-pass screen-space path

**Decision** - In the hero Water mode a scene prepass renders the environment +
wireframe into offscreen `scene_color`/`scene_depth` targets; water accumulates
thickness, speed-weighted whitewater, and nearest depth into R16 screen-space targets;
smoothing filters the front depth; and the composite (opaque) samples `scene_color` at
a refracted UV and writes the final pixel. The optional grid slice remains an overlay.
The optical/simple particle modes keep the older direct-to-swapchain opaque pass.

**Why** - Same-pass transparent billboards cannot accumulate path length or produce a
coherent lit front surface for deep water. The multi-pass path pays explicit render
memory and pass cost for an order-independent thickness signal and smoothed surface
normal. As of v1.12, sampling an offscreen scene-color prepass lets the water refract
the background instead of merely tinting the swapchain.

**Tradeoffs** - Render timing and memory are less trivial than a single swapchain
pass, and the hero path adds two more swapchain-sized targets. The public profiler
still reports one `gpu.render_ms` total for the whole render path rather than per-pass
water timing. (Measured: `gpu.render_ms` ≈ 0.27 ms at 1280×800 with refraction on — the
prepass + refraction cost is negligible against the pressure solve.)

**Applies to** - `architecture/rendering.md`, `architecture/gpu-resources.md`.

## Hero water features are Live sub-features of the Water view, not new render modes

**Decision** - The hero-water series (v1.12 refraction onward) evolves the existing
screen-space composite into the hero path rather than adding a parallel `HeroWater`
render mode. `RenderMode { Water, OpticalParticles, SimpleParticles }` replaces the bare
`u32 particle_view` dispatch; hero features are Live-toggleable settings under the
`Water` category with their own controls, mirrored into one `HeroParams` uniform.

**Why** - The composite already does most of the material (thickness, smoothed front
depth, reconstructed normal, Fresnel, Beer-Lambert absorption). A second top-level mode
would duplicate that renderer. Keeping hero features Live keeps the ~150-control surface
navigable with no reset and no pipeline rebuilds.

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

## Weak hero-water add-ons are opt-in, not startup defaults

**Decision** - Caustics, temporal stabilization, diffuse particles, wet walls, and
dense wall fill remain shipped feature groups but default off in the normal Water path.

**Why** - The default view should first read as a coherent liquid body; weak or expensive
optional cues should not add cost or artifacts unless the user explicitly enables them.

**Tradeoffs** - The UI still exposes the feature groups for deliberate tuning, so the
renderer keeps their pass wiring and buffer ownership. This is not a decision to remove
or abandon those features.

**Code anchors** - `crates/fluid-lab/src/settings/mod.rs -> Registry`;
`crates/fluid-lab/src/gpu/mod.rs -> GpuContext::render`.

**Applies to** - `architecture/rendering.md`, `architecture/settings.md`,
`architecture/gpu-resources.md`.

## Whitewater is persistent diffuse particles, not a speed mask

**Decision** - Whitewater can use a persistent, render-only GPU particle system (foam,
spray, bubbles) that is born at fast/breaking surfaces and wall impacts, advects, and
decays over seconds (`gpu/diffuse.rs -> DiffuseSystem`, v1.13). The original
speed-weighted thickness mask in the composite remains the shipped fallback while
diffuse particles default off to keep glass walls clean. Diffuse particles **do not**
conserve mass, affect pressure, or feed back into the solver — they are render state,
explicitly decoupled from the simulation's determinism contract.

**Why** - A fast-water mask alone reads as a white *tint* that flashes only while
water is moving; it cannot show foam that lingers a second or more after an impact and
fades. Persistent diffuse state is what makes churn read as foam (see the
`fluid-system-llm-brief.md` diagnosis). Keeping it render-only avoids touching the
fixed-point P2G determinism invariant (`../architecture/simulation.md`).

**Tradeoffs** - A fixed-capacity particle buffer (~12.6 MB) and two extra compute
passes per frame when enabled; emission is bounded by an integer-atomic per-frame
budget, and over-budget frames are reported (`stats_json.gpu.diffuse.clamped`) rather
than silently dropping foam. Wall impacts are intentionally biased toward brief spray
and away from long-lived vertical-wall foam; diffuse particles near vertical glass are
retired before they can read as wall decals, and persistent wall wetness is owned by the
wet-wall material. Determinism of *which slot* a spawn lands in is not guaranteed
(atomic ring cursor), which is acceptable because the system is render-only.

**Code anchors** - `crates/fluid-lab/src/gpu/diffuse.rs -> DiffuseSystem`;
`crates/fluid-lab/src/gpu/shaders/diffuse_emit.wgsl`;
`crates/fluid-lab/src/settings/mod.rs -> DiffuseParams`.

**Applies to** - `architecture/rendering.md`, `architecture/settings.md`,
`architecture/gpu-resources.md`.

## The reflected environment is procedural-only and world-fixed

**Decision** - The hero water reflects a *procedural* sky/room (`gpu/shaders/env.wgsl ->
env_sample`, v1.15), not an image-based cubemap/HDRI and not screen-space reflections of
the actual tank/particles. The same function also draws the world background as a
fullscreen skybox. Both are sampled in **world space via a camera-only rotation**, so they
follow the camera but stay fixed when the box rotates.

**Why** - A procedural environment costs no texture memory or asset pipeline and gives the
water believable Fresnel edges and a plausible reflected room/sky for near-zero render
cost. Sampling in world space (not the box-folded `view_proj`) keeps the conceptual model
honest: rotating the box re-aims gravity (`../architecture/app-shell.md`); it must not spin
the world. SSR and real IBL are a heavier, separate project and out of this series.

**Tradeoffs** - The reflection is a stylized environment, not a true mirror of the scene;
it cannot show the tank's own geometry reflected. Roughness softening blends toward an
averaged sky rather than a true pre-filtered mip. Micro-normals can shimmer, so they
default off until temporal stabilization lands.

**Code anchors** - `crates/fluid-lab/src/gpu/shaders/env.wgsl -> env_sample`;
`crates/fluid-lab/src/gpu/skybox.rs -> SkyboxRenderer`;
`crates/fluid-lab/src/gpu/composite.rs -> hero_uniform`;
`crates/fluid-lab/src/lib.rs -> FluidApp::frame` (the camera-only `eye_to_world`).

**Applies to** - `architecture/rendering.md`, `architecture/settings.md`.

## Caustics are approximate normal-gradient focusing, not projected photons

**Decision** - Hero-water caustics are a screen-space **normal-gradient** model: a
half-res pass focuses the sun by the convergence of the water surface normal × thickness
visibility, then composites additively into `scene_color` on the floor/back/left receivers
before the water composite (`gpu/caustics.rs`, v1.16). They are **not** physically
projected/refracted photon splatting and use no shadow map.

**Why** - Real projected caustics need a photon/light-transport pass and a receiver
shadow map — a much heavier system for a tank lab. The normal-gradient model reuses the
surface normal the composite already reconstructs and the existing sun direction, reads as
"light focusing on the floor through transparent water" for near-zero extra cost, and stays
coherent with the refraction it feeds (same normal). Compositing into `scene_color` *before*
the water composite is what lets refraction bend the lit caustics through the liquid.

**Tradeoffs** - The pattern is a plausible focusing cue, not a physically correct caustic;
receivers are reconstructed from `scene_depth` along the eye ray (no kind G-buffer), so the
floor/wall gate is a world-position tolerance band rather than an exact surface id. Default
off.

**Code anchors** - `crates/fluid-lab/src/gpu/caustics.rs -> CausticsSystem`;
`crates/fluid-lab/src/gpu/shaders/caustics_{generate,composite}.wgsl`.

**Applies to** - `architecture/rendering.md`, `architecture/gpu-resources.md`,
`architecture/settings.md`.

**Revisit when** - A projected-photon mode is wanted.

## Wet walls are a procedural render-only cue, not simulated drainage

**Decision** - The wet-wall look — darken/streak/reflect/gloss on touched walls, a thin
meniscus band at the waterline, a contact shadow at the floor/wall join — is driven by a
persistent supersampled per-wall-texel wetness field written each frame from the
**current** cell-type classification (Liquid adjacent to Solid, sampled as fractional
coverage on the supersampled wall axes) and decay-blended over time, then read by the
wall material (`gpu/wetwall.rs` + `environment.wgsl`). The wall-fill sheet is likewise
render-only: `gpu/wallfill.rs` writes a supersampled dense occupancy atlas from near-wall
particle splats, injects the sheet into the screen-space water MRTs before smoothing on
the rendered back/left glass faces, applies a local repair at their shared corner, and
writes a screen-space `wallfill_mask` that lets composite apply fill-only color,
absorption, reflection, and roughness controls. These cues do **not** simulate thin-film
drainage or per-droplet rivulets, and they never touch the sim buffers.

**Why** - A real thin-film/drainage simulation is its own physics project; the cell-type
adjacency signal already marks every wall contact (the same signal that spawns spray), so a
decaying procedural wetness field gives believable lingering wet streaks for almost nothing
and keeps the sim's determinism contract untouched. Streaks are a cheap procedural cue.

**Tradeoffs** - Wetness is render state only (clears on Reset, persists across frames); it
cannot show flow direction or true rivulets. Supersampling and bilinear coverage reduce
visible blocks, but wetness still originates from grid classification rather than
individual droplets. Direct particle/spray→wetness coupling is registered
(`wetness_spray_gain`) but stubbed at 0 — airborne spray re-wetting a wall above the
waterline is a follow-up. The dense wall-fill mask is a screen-space visual sheet rather
than additional simulated water mass, and it follows the open viewing-corner policy by
not projecting hidden sheets onto the front/right faces. Wet walls and dense wall fill
default off so the startup path favors the cheaper flat-water contact correction until
the optional effects are explicitly enabled.

**Code anchors** - `crates/fluid-lab/src/gpu/wetwall.rs -> WetWallSystem`;
`crates/fluid-lab/src/gpu/shaders/wetwall_update.wgsl`; the wall reads in
`crates/fluid-lab/src/gpu/shaders/environment.wgsl`; `crates/fluid-lab/src/gpu/wallfill.rs
-> WallOccupancySystem`.

**Applies to** - `architecture/rendering.md`, `architecture/gpu-resources.md`,
`architecture/settings.md`.

## Temporal stabilization is history-blend + camera-reset, not reprojection

**Decision** - Hero-water temporal stabilization is per-target exponential **history
blending** of the screen-space thickness / smooth-Z (→ normal) / whitewater targets plus
a **hard camera reset** when camera motion exceeds a threshold (`gpu/temporal.rs`, v1.18).
It is deliberately **not** motion-vector reprojection, neighborhood variance clamping, or
TAA jitter. The v1.16 caustics in-shader blend is unified under this one control.

**Why** - The app bakes the box model matrix into `view_proj` and rotates billboards to
compensate; there is **no motion-vector / history-reprojection infrastructure**, and
building it (motion vectors, disocclusion, neighborhood clamping) is its own project. The
achievable, useful version is exponential blend + reset-on-camera-move, which kills most of
the hero-stack shimmer without smearing on orbits. The reset metric uses the model-free
`eye_to_world` (camera-only) so box rotation/translation does not trigger spurious resets.

**Tradeoffs** - Content motion under a static camera (fast water) is **not** stabilized —
only camera-move ghosting is guarded, by the reset threshold. Each stabilized full-res
target doubles (ping-pong) — the series' largest memory add (`gpu-resources.md`).

**Code anchors** - `crates/fluid-lab/src/gpu/temporal.rs -> TemporalSystem`;
`crates/fluid-lab/src/gpu/shaders/temporal_blend.wgsl`;
`crates/fluid-lab/src/gpu/mod.rs -> camera_motion`.

**Applies to** - `architecture/rendering.md`, `architecture/gpu-resources.md`,
`architecture/settings.md`.

**Revisit when** - True motion-vector reprojection (with `jitter_enabled` / TAA) is built
to stabilize content motion under a static camera.

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

**Tradeoffs** - Absorption-over-thickness stays the opacity model even now that
refraction landed in v1.12: refraction samples an offscreen scene-color prepass and
distorts the background, but opacity is still normalized screen-space thickness, not
particle alpha. (Refraction was deferred until there was a scene-color target and
visible scene detail to justify the cost — both added in v1.12.)

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
