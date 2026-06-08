---
status:        active
owner:         adamg
last_updated:  2026-06-08
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
second heavyweight representation that distracts from the fluid-lab direction.

**Tradeoffs** - The default water view hides some per-particle detail in exchange for
a more coherent liquid body; the optical and simple particle views remain selectable
for motion, solver inspection, and fallback comparison.

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
`u32 particle_view` dispatch; hero features (refraction now; foam, marching-cubes
surface, env reflection, caustics, wet walls, temporal later) are Live-toggleable
settings under the `Water` category with their own `enabled` flags, mirrored into one
`HeroParams` uniform.

**Why** - The composite already does most of the material (thickness, smoothed front
depth, reconstructed normal, Fresnel, Beer-Lambert absorption). A second top-level mode
would duplicate that renderer. Keeping hero features Live keeps the ~150-control surface
navigable with no reset and no pipeline rebuilds.

**Tradeoffs** - One composite shader grows in complexity and uniform size across the
series, gated by `render.hero.mode_enabled` and per-feature flags, instead of being
split into independent pipelines.

**Code anchors** - `crates/fluid-lab/src/gpu/mod.rs -> RenderMode`;
`crates/fluid-lab/src/gpu/composite.rs`; `crates/fluid-lab/src/gpu/environment.rs`;
`crates/fluid-lab/src/settings/mod.rs -> HeroParams`.

**Applies to** - `architecture/rendering.md`, `architecture/settings.md`,
`architecture/gpu-resources.md`.

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
