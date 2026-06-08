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

**Tradeoffs** - Render/debug views are more involved to implement GPU-side, but they
do not turn observation into the dominant frame cost.

**Applies to** - `architecture/rendering.md`, `architecture/profiler.md`.

## Particle and liquid-cell views are the product rendering surface

**Decision** - The product rendering surface is the particle view, tank wireframe,
and liquid-cell/grid inspection; there is no extracted-surface compatibility path.

**Why** - These views make scale, motion, and solver state legible without carrying a
second heavyweight representation that distracts from the fluid-lab direction.

**Tradeoffs** - The app does not currently offer a smooth cinematic water surface. A
future surface renderer must justify its own cost and product role as a new feature.

**Code anchors** - `app/crates/fluid-lab/src/gpu/mod.rs -> GpuContext::render`;
`app/crates/fluid-lab/src/gpu/particles.rs -> ParticleRenderer`;
`app/crates/fluid-lab/src/gpu/slice.rs -> SliceRenderer`.

**Applies to** - `architecture/rendering.md`, `architecture/gpu-resources.md`,
`architecture/web-shell.md`.

## Rendering stays single-pass until a measured feature requires more

**Decision** - The wireframe, particles, and optional grid slice share one swapchain
render pass and one depth attachment.

**Why** - The current views need no offscreen color/depth targets, and keeping the
path single-pass makes render timing and memory costs straightforward.

**Revisit when** - A measured, approved rendering feature requires an offscreen or
multi-pass composition path.

**Applies to** - `architecture/rendering.md`, `architecture/gpu-resources.md`.

## Particle and grid representations stay separate

**Decision** - Fluid particles and the MAC simulation grid remain distinct
representations that can be rendered and inspected independently.

**Why** - Particles track moving mass while the grid owns pressure and velocity
solves; preserving that distinction makes simulation behavior easier to inspect.

**Tradeoffs** - Separate buffers and renderers cost some plumbing in exchange for
clear ownership and replaceable views.

**Applies to** - `architecture/rendering.md`, `architecture/simulation.md`.

## Visual styling must preserve observability

**Decision** - Particle color, opacity, edge softness, size, and sphere shading may
improve volume perception, but the primary view must continue to expose motion and
simulation problems.

**Why** - Rendering is useful here when it helps a viewer read the fluid, not when it
hides solver behavior behind cinematic polish.

**Applies to** - `architecture/rendering.md`, `architecture/settings.md`,
`architecture/profiler.md`.

## Water look stays in the existing particle pass until measurements demand more

**Decision** - The water-look upgrade uses the existing particle billboard pass with
optical-density alpha, depth testing preserved, and particle depth writes disabled.
`render.water_optical_density` is the public control; `render.particle_alpha` is
legacy compatibility only and not part of the public settings surface.

**Why** - The shipped v1.10 captures improved dense-water readability without adding
offscreen targets, composite passes, or persistent GPU resources. The setting name has
to match the shader semantics; calling an optical-density term "opacity" would make
the UI lie about what the renderer does.

**Tradeoffs** - Same-pass transparent billboards still have ordinary unsorted
transparency limits. A future weighted-blend or surface path remains possible, but it
must justify extra pass/resource cost with measured evidence.

**Code anchors** - `app/crates/fluid-lab/src/gpu/particles.rs -> ParticleRenderer`;
`app/crates/fluid-lab/src/gpu/shaders/particles.wgsl`;
`app/crates/fluid-lab/src/settings/mod.rs -> Registry`;
`app/crates/fluid-lab/src/lib.rs -> FluidApp::set_setting`.

**Applies to** - `architecture/rendering.md`, `architecture/settings.md`,
`architecture/gpu-resources.md`.

## See also

- [`../architecture/rendering.md`](../architecture/rendering.md)
- [`performance.md`](performance.md) - render memory/cost policy
- [`scope.md`](scope.md) - product framing
- [`../agent-context/maintaining-docs.md`](../agent-context/maintaining-docs.md)
