---
status:        active
owner:         adamg
last_updated:  2026-06-05
---

# Decisions — Rendering

## Debug/inspection views are GPU-native — no normal-frame readback

**Decision** — Pressure, velocity, scalar, grid-slice, particle, and mesh views
sample GPU buffers/textures directly. CPU/GPU readback is allowed only for throttled
diagnostics, explicit captures, and profiler snapshots — never for routine frame
rendering.

**Why** — A fluid lab exposes many views; if each pulled state back to the CPU the
inspection layer would become slower than the simulation it inspects. Keeping it
GPU-native means the same data path serves both final rendering and inspection.

**Tradeoffs** — Some views are harder to implement GPU-side, in exchange for credible
performance.

**Applies to** — `architecture/rendering.md`, `architecture/profiler.md`.

## Marching cubes is demoted to a lazy dev-only opt-in; particles are the default

**Decision** — MC resources (~73 MB vertex buffer + pipelines) are **not allocated at
boot**. `GpuContext.mesh` is `Option<mesh::MeshExtractor>` — `None` by default. The
`dev.mesh_enabled` setting (Reset-class, default 0) controls lazy allocation: enabling
it allocates the extractor; disabling it drops it. Particles render whenever MC is
absent or off.

**Why** — GPU marching cubes is the project's single biggest memory cost (73 MB) and
a Reset-class concern. Allocating it unconditionally at boot penalises every session
regardless of whether MC is being inspected. The observable, inspectable pipeline
needs no mesh — MC is a dev/debugging view, not a product feature that needs to be
cheap to reach. Lazy allocation keeps the common path lightweight.

**Tradeoffs** — Enabling MC requires a Reset (the ~73 MB allocation and pipeline
construction happen during recreate_fluid). Disabling it recovers the memory
immediately. The web toolbar Mesh button is wired through the same lazy-alloc path.

**Code anchors** — `app/crates/fluid-lab/src/gpu/mod.rs → GpuContext.mesh`; `app/crates/fluid-lab/src/gpu/mesh.rs → MeshExtractor`; `app/crates/fluid-lab/src/gpu/shaders/mc.wgsl`; host table reference + tests in `app/crates/fluid-lab/src/sim/marching_cubes.rs`.

**Applies to** — `architecture/rendering.md`, `architecture/gpu-resources.md`.

## Render/debug modes are centrally organized, each with a cost contract

**Decision** — Render/debug modes are registered/organized rather than scattered as
ad-hoc toggles; each mode is associated with its required buffers, a profiler label,
a cost class (cheap/medium/expensive), and low-tier availability.

**Why** — The product depends on many views. A registry keeps the renderer from
becoming a pile of one-off flags and keeps every view's cost trackable.

**Applies to** — `architecture/rendering.md`.

## Keep particles, grid, and surface field as separate representations

**Decision** — Fluid particles, the 3D MAC simulation grid, and the 3D scalar surface
field are distinct representations, not one fused buffer.

**Why** — Each has a different job (particles track mass, the grid solves physics, the
scalar field feeds marching cubes) and each can be visualized and replaced
independently.

**Tradeoffs** — More buffers and synchronization for a cleaner, more replaceable
architecture.

**Applies to** — `architecture/rendering.md`, `architecture/simulation.md`.

## Visual realism is subordinate to observability — except in the opt-in MC water view

**Decision** — The **default** path (readable particles/coarse voxels) plus strong
profiler + config instrumentation stays observability-first, with no reliance on
cinematic polish. Visual realism — translucent glassy water, velocity-driven foam,
Fresnel/specular shading — lives only in the **opt-in marching-cubes view**
(`dev.mesh_enabled`), tuned by Live `render.mesh_*` knobs.

**Why** — Premature visual polish on the primary view hides solver problems; the
particle view must keep exposing them. But the MC view is already a deliberate,
heavyweight opt-in, so giving *it* believable water (rather than the old opaque white
blob) is a usability win that costs the default path nothing.

**Tradeoffs** — The MC fragment shader and the foam/smoothing fields carry more
complexity than a flat-shaded surface, isolated behind the lazy-allocated extractor.

**Applies to** — `architecture/rendering.md`, `architecture/profiler.md`.

## MC water shades in tank-local space; foam is velocity-driven

**Decision** — The MC water surface is shaded with the camera eye transformed into the
tank's local frame (vertices are local; the model matrix is baked into `view_proj`
only). White appears only as a tight sun specular and as foam keyed to a per-cell
**speed** field (`density.wgsl` → `mc.wgsl` `nrm.w` → `mesh.wgsl`), not as a blanket
Fresnel rim.

**Why** — A world-space (or approximate) eye made the view vector swing as the tank
moved, blowing the Fresnel term to white — the white-flash bug. Keying foam to speed
matches the particle speed→white cue, so "white" means "fast/aerated water" in both
views rather than an artifact of viewing angle.

**Code anchors** — `app/crates/fluid-lab/src/lib.rs → FluidApp::frame` (eye_local);
`app/crates/fluid-lab/src/gpu/shaders/mesh.wgsl`; `app/crates/fluid-lab/src/gpu/shaders/density.wgsl` (speed).

**Applies to** — `architecture/rendering.md`.

## See also

- [`../architecture/rendering.md`](../architecture/rendering.md)
- [`observability.md`](observability.md) · [`performance.md`](performance.md) · [`scope.md`](scope.md)
- [`../agent-context/maintaining-docs.md`](../agent-context/maintaining-docs.md)
