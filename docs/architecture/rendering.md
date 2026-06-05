---
status:        active
owner:         adamg
last_updated:  2026-06-05
okay_to_delete: false
long_lived:    true
---

# Rendering & debug views

The GPU-native render/debug-view layer draws the wireframe tank and the active render mode — particles (default), grid-slice inspection, or the marching-cubes water mesh — all sampling GPU buffers directly with no normal-frame CPU/GPU readback. Readback is restricted to the throttled diagnostics path in `app/crates/fluid-lab/src/gpu/timing.rs`.

## What it owns

`app/crates/fluid-lab/src/gpu/mod.rs → GpuContext` holds and orchestrates all renderer instances. It owns the surface, the depth texture, and the `mesh_enabled` / `slice_enabled` flags that gate per-mode work each frame. The model matrix `translate(box_pos) · from_quat(box_orient)` is folded into view-projection in `app/crates/fluid-lab/src/lib.rs → FluidApp::frame` before being passed to all renderers; no renderer recomputes it.

```
frame()
  model    = translate(box_pos) · from_quat(box_orient)
  vp       = camera.view_proj(aspect) · model
  eye_local = box_orient⁻¹ · (camera.eye − box_pos)   ← eye in tank-local space
  GpuContext::step(n)             ← sim + optional MC extract
  GpuContext::render(vp, …, eye_local)  ← all draw calls in one render pass
```

The MC vertices live in **tank-local** space (the model matrix is baked into `vp`
only), so the camera eye is transformed into that same local frame in
`app/crates/fluid-lab/src/lib.rs → FluidApp::frame` and threaded through
`GpuContext::render` to the mesh shader. View-dependent shading (Fresnel/specular)
therefore stays correct while the tank is moved or rotated.

## Render modes

| Mode | Default | Renderer | Shader |
|------|---------|----------|--------|
| Wireframe tank | always on | `app/crates/fluid-lab/src/gpu/renderer.rs → WireframeRenderer` | inline WGSL in `renderer.rs` |
| Particles | on when mesh off | `app/crates/fluid-lab/src/gpu/particles.rs → ParticleRenderer` | `app/crates/fluid-lab/src/gpu/shaders/particles.wgsl` |
| Mesh (MC surface) | off (`dev.mesh_enabled=0`, lazily allocated) | `app/crates/fluid-lab/src/gpu/mesh.rs → MeshExtractor` | `app/crates/fluid-lab/src/gpu/shaders/mesh.wgsl` (translucent, alpha-blended) |
| Grid-slice debug | off (`slice_enabled=false`) | `app/crates/fluid-lab/src/gpu/slice.rs → SliceRenderer` | `app/crates/fluid-lab/src/gpu/shaders/slice.wgsl` |

Mesh and particles are mutually exclusive: `GpuContext::render` draws `mesh` when `mesh_enabled`, otherwise `particles`. Slice is additive — drawn after either, when `slice_enabled`. Cost summary: wireframe is a fixed 24-vertex line-list; particles are instanced quads (one per particle, 6 verts); mesh uses an indirect draw from a pre-built vertex buffer; slice draws `nx*ny` instanced quads for the mid-depth XY cross-section.

The tank is rectangular (uniform cell size `crate::sim::H`, independent per-axis cell counts `nx, ny, nz` — see `gpu-resources.md`). The wireframe and both debug-visualization paths are parameterized to the actual per-axis grid rather than assuming a cube.

**Wireframe** (`app/crates/fluid-lab/src/gpu/renderer.rs → WireframeRenderer`) is parameterized to an arbitrary AABB: `WireframeRenderer::new` and `tank_wireframe(lo, hi)` take the box `lo`/`hi`, supplied from `GpuFluid::tank_bounds()`. Floor edges are tinted blue for orientation.

**Particles** (`app/crates/fluid-lab/src/gpu/particles.rs → ParticleRenderer`) bind the simulation position buffer directly as a read-only storage buffer. Camera uniform carries `right`/`up` billboard axes plus radius and speed-scale for velocity-tinted quads.

**Grid slice** (`app/crates/fluid-lab/src/gpu/slice.rs → SliceRenderer`) binds `cell_type`, `pressure`, `u_vel`, `v_vel`, `w_vel` buffers from `GpuFluid` as read-only storage. Three sub-modes (0 = cell-type, 1 = pressure diverging palette, 2 = speed sequential palette) are controlled by `set_slice_mode`; the slice is fixed at `k = nz/2` and draws `nx*ny` instanced quads. `SliceUniform` (112 B) carries `dims: [u32;4] = [nx, ny, nz, 0]` plus the grid origin and cell size; `slice.wgsl` decomposes the per-axis cell index, so the slice is correct on a non-cubic tank. No readback.

## Marching cubes (scalar field → mesh)

**Status: dev-only, off by default, lazily allocated.**

The MC feature is controlled by `dev.mesh_enabled` (Reset-class, default 0). `GpuContext.mesh` is `Option<mesh::MeshExtractor>`: it is `None` until the feature is enabled. Allocation timing:

- **Boot:** `GpuContext::new` reads `settings.mesh_enabled()` — `Some` only if the setting is on at boot.
- **Runtime enable:** `set_mesh_enabled(true)` lazily allocates the ~73 MB MC GPU resources if `mesh` is currently `None`.
- **Runtime disable:** `set_mesh_enabled(false)` drops `mesh` to `None`, freeing the resources.
- **Reset:** `recreate_fluid` rebuilds `Some/None` per the current flag.

All MC paths are Option-guarded: `set_mesh_iso`, `update_camera`, `step`'s `record_extract`, and `render`'s draw branch each check `self.mesh.as_mut/ref()` and skip when absent.

`app/crates/fluid-lab/src/gpu/mesh.rs → MeshExtractor` owns a GPU pipeline of compute passes feeding one indirect draw:

```
density.wgsl   occupancy → scalar[] (light blur); MAC velocities → speed[] (foam)
blur.wgsl      ×N ping-pong (scalar↔scalar2) smoothing  ← render.mesh_smooth
mc.wgsl        (nx-1)·(ny-1)·(nz-1) cubes → verts[] (pos, normal, nrm.w=foam) + atomic counter
mc_args.wgsl   counter → indirect draw args
mesh.wgsl      translucent glassy water render (storage-buffer vertex fetch)
```

The density source is the `occupancy` buffer from `GpuFluid` (i32 per cell, populated by the P2G classify pass); `density.wgsl` also samples the staggered `u/v/w_vel` buffers into a cell-centered `speed[]` field that drives **velocity foam** (the mesh whitens only where the water moves fast, mirroring the particle speed→white cue). `blur.wgsl` runs `render.mesh_smooth` ping-pong iterations (each = two passes, `scalar→scalar2→scalar`) so the raw per-cell occupancy blobs round off into a smooth surface — the fix for the lumpy-surface problem. `MeshExtractor::record_extract` is called inside `GpuContext::step` after the final substep, gated by `mesh_enabled`. The vertex buffer is allocated at `MAX_VERTS = 2_400_000` (~73 MB, dominating; plus three cell-sized `scalar`/`scalar2`/`speed` buffers) only when the extractor is constructed; the atomic counter is cleared each frame and the indirect-draw arg buffer is written by `mc_args.wgsl`.

**Glassy water shading** (`app/crates/fluid-lab/src/gpu/shaders/mesh.wgsl`): a translucent blue body, Schlick-Fresnel reflection toward a sky tint (more reflective/opaque at grazing angles), a tight white sun specular, and white foam only where `nrm.w` (the per-vertex speed foam) is high. The render pipeline uses **alpha blending** with depth-test+write (`app/crates/fluid-lab/src/gpu/mesh.rs → render_pl`, `wgpu::BlendState::ALPHA_BLENDING`), so the tank/background reads through the surface. Normals are flipped toward the camera in the fragment shader to neutralize MC's ambiguous winding. The water look is tuned Live via `render.mesh_opacity` / `render.mesh_fresnel` / `render.mesh_foam` (carried in the camera uniform) and `render.mesh_iso`; `MeshLook` preserves these across (re)allocation.

Both the density blur and the MC march are per-axis: `MeshParams` carries `dims: [u32;4] = [nx, ny, nz, 0]` and `origin: [f32;4]`, with `h = crate::sim::H` and `cell_count = nx·ny·nz`. The MC cube grid is `(nx-1)·(ny-1)·(nz-1)`; `density.wgsl`/`mc.wgsl` decompose the per-axis cell index, so the mesh extracts correctly on a non-cubic tank.

The host reference lives in `app/crates/fluid-lab/src/sim/marching_cubes.rs → polygonize` with `EDGE_TABLE` and `TRI_TABLE` — the same tables embedded in `mc.wgsl`. The host module has three `#[test]` gates: `edge_table_matches_tri_table`, `tri_table_well_formed`, and `sphere_surface_is_watertight` (the gold-standard manifold test on an analytic sphere). If any of these fail, the WGSL tables are wrong.

**Fallback contract**: particles render whenever `dev.mesh_enabled = 0` (the default) or whenever `GpuContext.mesh` is `None`. There is no automatic fallback — the caller (TypeScript or `set_mesh_enabled`) chooses the mode explicitly.

**Camera eye is passed in tank-local space.** The mesh vertices are in tank-local space (model matrix baked into `view_proj` only), so `FluidApp::frame` transforms the true camera eye into the local frame (`box_orient⁻¹·(eye − box_pos)`) and threads it through `GpuContext::render` into the mesh camera uniform. This is load-bearing: an eye in the wrong space makes the view vector swing as the tank moves and the Fresnel term blow up to white (the old white-flash bug). Do not substitute a world-space or approximate eye.

## Non-obvious invariants and gotchas

- **No normal-frame readback.** `GpuContext::render` never calls `map_async` or copies buffers to the CPU on the hot path. The only readback is in `timing::GpuTimers::map_readback`, which is throttled and opt-in.
- **Tank model matrix is baked into `view_proj`.** All renderers receive a single pre-multiplied matrix; none is aware of `box_pos`/`box_orient` separately. If you add a renderer that needs the raw model matrix, thread it through `GpuContext::render`.
- **Mesh and particles share the same render pass.** There is only one `begin_render_pass` call per frame. Switching modes does not split the pass.
- **The mesh is translucent (alpha-blended).** Unlike the opaque wireframe/particles, the mesh pipeline blends. It is drawn after the wireframe (so the tank reads through it) and writes depth, so overlapping water resolves to the nearest surface rather than over-accumulating. Foam/opacity ramp with Fresnel and the per-vertex speed (`nrm.w`).
- **MC extract happens in `step`, not `render`.** `record_extract` submits its own command encoder via `queue.submit`. The render pass then reads the already-written vertex buffer. If `step` is skipped (paused, no pending steps), the stale vertex buffer from the prior frame is drawn.
- **Slice bind group references live GPU buffers.** If `recreate_fluid` is called (on reset), `SliceRenderer` is rebuilt entirely — the old bind group becomes invalid. `GpuContext::recreate_fluid` rebuilds all sub-renderers atomically — including `WireframeRenderer` (tank box dimensions can change) and the `MeshExtractor` (`Some/None` per the current flag) — and preserves live settings (`slice_mode`, `particle_size`, `mesh_iso`).
- **MC vertex buffer is STORAGE only, not VERTEX.** `mesh.wgsl` fetches vertices by index from a storage binding, not via the vertex pipeline. Cull mode is `None` because MC winding can be ambiguous.
- **`WireframeRenderer`'s shader is inline** (not a separate `.wgsl` file).

## Update when

- A new render mode is added → add a flag to `GpuContext`, a new renderer struct under `app/crates/fluid-lab/src/gpu/`, and a draw call in `GpuContext::render`.
- The grid buffers or occupancy layout change → update `SliceRenderer::new` and `MeshExtractor::new` bind group entries.
- Box transform is exposed to shaders → thread the model matrix separately into `GpuContext::render`.
- MC tables change → regenerate both `app/crates/fluid-lab/src/sim/marching_cubes.rs` tables and the embedded constants in `app/crates/fluid-lab/src/gpu/shaders/mc.wgsl`; the three host unit tests must pass.
- The water look (color/opacity/Fresnel/foam) or the smoothing/foam fields change → update the "Marching cubes" section here and the `render.mesh_*` entries owned by `settings.md`; the velocity-foam path spans `density.wgsl` (speed) → `mc.wgsl` (`nrm.w`) → `mesh.wgsl`.
- The MC lazy-allocation lifecycle changes (e.g., mesh becomes always-on) → update the "Marching cubes" section and the `gpu-resources.md` "Lazy mesh" invariant.

## See also

- `simulation.md` — the buffers (`particle_buffer`, `cell_type_buffer`, `pressure_buffer`, `u/v/w_vel_buffer`, `occupancy_buffer`) that renderers sample.
- `app-shell.md` — `FluidApp::frame`, model-matrix construction, TS/WASM boundary for mode controls.
- `../decisions/rendering.md` — GPU-native no-readback rule, and marching cubes demoted/default-off with a fallback contract.
- `../agent-context/maintaining-docs.md`
