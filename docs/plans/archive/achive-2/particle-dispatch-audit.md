---
status:        draft
owner:         adamg
last_updated:  2026-06-08
okay_to_delete: false
long_lived:    false
owning_docs:
  - architecture/gpu-resources.md
  - architecture/simulation.md
  - architecture/profiler.md
  - decisions/performance.md
---

# Particle dispatch audit

## Mission

Create the path inventory and indexing design needed before tiled particle dispatch is
implemented. This is a planning/research doc for agents to update
[`v1.8.0-particle-dispatch-tiling.md`](v1.8.0-particle-dispatch-tiling.md), not a
request to update architecture or decisions docs.

## Scope

In scope:

- Audit every particle-linear compute path that assumes one-dimensional dispatch or a
  simple global particle index.
- Include mark/classify inputs, scatter, G2P, advect/recover, impulse, wave-maker, and
  any runtime dispatch/preflight path.
- Identify bounds checks, partial-tile behavior, workgroup size assumptions, and shared
  helper opportunities.
- Record adapter limits that matter to dispatch and particle storage.
- Propose the minimum test/capture matrix for v1.8.0.

Out of scope:

- Implementing tiled dispatch.
- Optimizing the measured bottleneck after tiling.
- Render decimation/LOD.
- Adding particle-count presets.
- Updating architecture or decisions docs before implementation starts.

## Research tasks

1. List each shader/runtime path with its current dispatch shape and global-index
   formula.
2. Mark whether the path reads or writes particle data, grid data, or both.
3. Identify paths that are wall-contact sensitive, especially G2P and recovery.
4. Propose one coordinated tiled index formula and partial-tile guard.
5. Define how preflight limits change once 2D/tiled dispatch is supported.
6. Define measurement evidence for requested 2M, 4M, and 8M particles.
7. Patch `v1.8.0-particle-dispatch-tiling.md` if this audit finds missing paths,
   stricter gates, or a better phase split.

## Deliverables

- A particle-path inventory table.
- A recommended tiled dispatch/indexing contract.
- A list of required code surfaces for v1.8.0.
- Known risks and any blockers that should stop implementation.
- A proposed measurement table schema.

## Findings summary

The hard blocker is limited to particle-linear compute paths and their runtime
dispatch/preflight owners. The grid, face, pressure, and renderer paths were reviewed
because they share one-dimensional indexing, but they do not currently require particle
dispatch tiling for v1.8.0.

The particle-linear paths that must change together are:

- `mark.wgsl` occupancy scatter.
- `scatter.wgsl` P2G fixed-point atomics for U/V/W.
- `g2p.wgsl` PIC/FLIP gather, RK1 advect, wall recovery, and wall friction.
- `impulse.wgsl`, including slosh and scheduled wave-maker callers.
- `GpuFluid` particle dispatch helpers and `GpuContext` preflight/stats.

`classify.wgsl` is included in the inventory because it consumes mark output and feeds
G2P/pressure behavior, but it is cell-linear rather than particle-linear.

## Path inventory

`WG` below is `PARTICLE_WG = 64` in `crates/fluid-lab/src/gpu/fluid.rs`.
Current particle dispatches all submit `(ceil(particle_count / WG), 1, 1)`.

| Path | Current dispatch shape | Shader/runtime owner | Reads / writes | Bounds checks and partial work | Tiling risk |
|---|---|---|---|---|---|
| Reset preflight and dispatch-limit stats | No dispatch; rejects before allocation when `estimated > max_compute_workgroups_per_dimension * WG` | `crates/fluid-lab/src/gpu/mod.rs` -> `GpuContext::recreate_fluid`, `max_particle_dispatch_count`, `log_boot_diagnostics`; `crates/fluid-lab/src/profiler/mod.rs` -> `stats_json`; `web/panels.js` profiler panel | Reads adapter caps and deterministic seeded estimate; writes `requested_particles`, `estimated_particles`, `scale_status` | Current guard preserves running sim and reports `rejected-dispatch-limit`; storage guard then reports `rejected-storage-binding-limit` | High. Must become a 2D dispatch-capacity check without weakening storage/preflight safety or stale "one-dimensional" labels. |
| Main particle dispatch helpers | `dispatch_mark`, `dispatch_scatter`, and `dispatch_g2p` each submit `(ceil(particle_count / WG), 1, 1)` | `crates/fluid-lab/src/gpu/fluid.rs` -> `dispatch_mark`, `dispatch_scatter`, `dispatch_g2p`; detailed timing calls the same helpers from `crates/fluid-lab/src/gpu/mod.rs` -> `record_substep_detailed` | No direct buffer access in Rust; bind groups point shaders at particle/grid buffers | Shader guards own partial work; Rust has no shared shape helper today | High. Add one shared runtime `particle_dispatch_shape` used by every particle-linear pass so coarse and detailed timing cannot diverge. |
| Occupancy mark | `(ceil(particle_count / WG), 1, 1)` | `crates/fluid-lab/src/gpu/fluid.rs` -> `dispatch_mark`; `crates/fluid-lab/src/gpu/shaders/mark.wgsl` | Reads `particles`; atomic writes `occupancy` | `p = gid.x`; `if p >= params.dims.y return`; particle position is clamped into `[0,nx/y/z-1]` before `occ[c]` | High. Replace only the particle index formula; keep occupancy atomics and cell clamp unchanged. |
| Classify mark output | `(ceil(cell_count / WG), 1, 1)` | `crates/fluid-lab/src/gpu/fluid.rs` -> `dispatch_classify`; `crates/fluid-lab/src/gpu/shaders/classify.wgsl` | Reads `occupancy`; writes `cell_type`; atomic writes `stats[0]` | `c = gid.x`; `if c >= nx * ny * nz return`; boundary cells return early; dilation reads 6-neighbours only after boundary reject | Low. Not a particle dispatch, but must be verified after tiled mark because occupancy is its input. |
| P2G scatter U/V/W | Three dispatches, each `(ceil(particle_count / WG), 1, 1)` | `crates/fluid-lab/src/gpu/fluid.rs` -> `dispatch_scatter`; `crates/fluid-lab/src/gpu/shaders/scatter.wgsl` with `AXIS` override | Reads `particles`; atomic writes axis `num`/`den` buffers | `p = gid.x`; `if p >= params.dims.y return`; each 2x2x2 face stencil skips out-of-range faces before atomic add | High. All three axes must use the same tiled particle index. Keep fixed-point `i32` atomics and per-face bounds checks intact. |
| P2G normalize U/V/W | Per-axis `(ceil(face_count_axis / WG), 1, 1)` via `counts()[axis]` | `crates/fluid-lab/src/gpu/fluid.rs` -> `dispatch_normalize`; `crates/fluid-lab/src/gpu/shaders/normalize.wgsl` | Reads axis `num`/`den`; writes axis velocity | `idx = gid.x`; `if idx >= arrayLength(&vel) return` | Low. Face-linear, not particle-linear. No v1.8 tiling change expected. |
| Save pre-force velocity U/V/W | Per-axis `(ceil(face_count_axis / WG), 1, 1)` | `crates/fluid-lab/src/gpu/fluid.rs` -> `dispatch_savevel`; `crates/fluid-lab/src/gpu/shaders/save_vel.wgsl` | Reads axis velocity; writes axis saved velocity | `i = gid.x`; `if i < arrayLength(&dst)` | Low. Face-linear FLIP baseline copy; no particle tiling change expected. |
| Forces U/V/W | Per-axis `(ceil(face_count_axis / WG), 1, 1)` | `crates/fluid-lab/src/gpu/fluid.rs` -> `dispatch_forces`; `crates/fluid-lab/src/gpu/shaders/forces.wgsl` | Reads/writes axis velocity; reads `cell_type` | `idx = gid.x`; `if idx >= face_dim_product return`; adjacent-cell reads are guarded by face coordinate | Low. Face-linear. Verify unchanged because G2P samples the final velocities. |
| Boundaries U/V/W, pre and post pressure | Per-axis `(ceil(face_count_axis / WG), 1, 1)` | `crates/fluid-lab/src/gpu/fluid.rs` -> `dispatch_enforce`; `crates/fluid-lab/src/gpu/shaders/boundaries.wgsl` | Reads `cell_type`; writes axis velocity | `idx = gid.x`; `if idx >= face_dim_product return`; domain-edge and solid-adjacent faces are zeroed | Low. Face-linear. Verify unchanged because G2P wall-aware sampling depends on these zeroed faces. |
| Divergence and pressure-adjacent grid passes | Cell/reduction dispatches: `(ceil(cell_count / WG), 1, 1)`, `(ceil(cell_count / 256), 1, 1)`, or `(1,1,1)` | `crates/fluid-lab/src/gpu/fluid.rs` -> `record_pressure` helpers; `divergence.wgsl`, `cg_*.wgsl` | Reads face velocity, `cell_type`, occupancy, pressure/CG buffers; writes divergence, pressure/CG buffers | Cell kernels guard `c >= nx*ny*nz`; reduction kernels guard chunk length or use one workgroup | Low for v1.8. Current grid setting max is 128 per axis, so these do not drive the particle-count dispatch ceiling. |
| Gradient U/V/W | Per-axis `(ceil(face_count_axis / WG), 1, 1)` | `crates/fluid-lab/src/gpu/fluid.rs` -> `dispatch_gradient`; `crates/fluid-lab/src/gpu/shaders/gradient.wgsl` | Reads pressure and `cell_type`; writes axis velocity | `idx = gid.x`; `if idx >= face_dim_product return`; returns on domain-edge faces and solid/air-only neighbours | Low. Face-linear. No v1.8 tiling change expected. |
| G2P, advect, recover | `(ceil(particle_count / WG), 1, 1)` | `crates/fluid-lab/src/gpu/fluid.rs` -> `dispatch_g2p`, `record_finish`; `crates/fluid-lab/src/gpu/shaders/g2p.wgsl` | Reads/writes `particles`; reads `u/v/w_vel` and `u/v/w_saved` | `p = gid.x`; `if p >= params.dims.y return`; each MAC sample skips out-of-range and static-wall face stencils; advect clamps position inside tank and zeroes crossed wall-normal velocity | Highest. Use the shared tiled particle index only. Do not change PIC/FLIP blend, saved-velocity sampling, CFL clamp, wall-aware stencil skipping, wall friction, or recovery. |
| Manual slosh impulse | One-shot `(ceil(particle_count / WG), 1, 1)` outside the main substep encoder | `crates/fluid-lab/src/lib.rs` -> `FluidApp::slosh_box`; `crates/fluid-lab/src/gpu/mod.rs` -> `apply_impulse`; `crates/fluid-lab/src/gpu/fluid.rs` -> `apply_impulse`; `crates/fluid-lab/src/gpu/shaders/impulse.wgsl` | Runtime writes impulse uniform; shader reads/writes `particles.vel` | `p = gid.x`; `if p >= params.dims.y return` | High. Easy to miss because it is outside `record_prep`/`record_finish`; tile the standalone encoder path too. |
| Scheduled wave-maker impulse | Same one-shot impulse path as slosh | `crates/fluid-lab/src/lib.rs` -> `InteractionState::update_wave`, `FluidApp::update_interactions`; then same GPU impulse path | Same as impulse row | Same as impulse row | High. No separate shader, source/drain, or particle allocation; verify this caller after tiling `impulse.wgsl`. |
| Generic clears | Ten separate `(ceil(buffer_len / WG), 1, 1)` clear dispatches | `crates/fluid-lab/src/gpu/fluid.rs` -> `dispatch_clear`; `crates/fluid-lab/src/gpu/shaders/clear.wgsl` | Writes P2G num/den, occupancy, pressure ping-pong, and stats buffers | `i = gid.x`; `if i < arrayLength(&buf)` | Low. Not particle-linear; no v1.8 tiling change expected. |
| Particle render | Draw call, not compute dispatch: `draw(0..6, 0..particle_count)` | `crates/fluid-lab/src/gpu/particles.rs` -> `ParticleRenderer::draw`; `crates/fluid-lab/src/gpu/shaders/particles.wgsl` | Vertex shader reads `particles`; render pass writes color/depth | Instance index selects particle; draw count is runtime particle count | Measurement risk only. Do not introduce render decimation/LOD in v1.8. |

## Recommended tiled indexing contract

Use one 2D workgroup contract for every particle-linear compute shader:

- Keep `@workgroup_size(64, 1, 1)` and the existing `params.dims.y` particle count.
- Runtime computes particle workgroups in `u64`:
  - `total_groups = ceil(particle_count / PARTICLE_WG)`.
  - `groups_x = min(total_groups, max_compute_workgroups_per_dimension)`.
  - `groups_y = ceil(total_groups / groups_x)`.
  - reject before allocation/submission if `groups_y > max_compute_workgroups_per_dimension`,
    if the padded dispatch capacity would overflow the WGSL `u32` particle index, or if
    the existing storage-binding limit is exceeded.
- Dispatch every particle-linear pass as `(groups_x, groups_y, 1)`.
- In WGSL, compute `p` from workgroup position, not from `gid.x` alone. Entry points
  should pass `@builtin(workgroup_id)`, `@builtin(local_invocation_id)`, and
  `@builtin(num_workgroups)` into the shared helper:

```wgsl
const PARTICLE_WG: u32 = 64u;

fn particle_index(wid: vec3<u32>, lid: vec3<u32>, nwg: vec3<u32>) -> u32 {
    return ((wid.y * nwg.x + wid.x) * PARTICLE_WG) + lid.x;
}
```

Each particle shader entry point then calls the helper and keeps the same partial-tile
guard before touching `particles[p]`:

```wgsl
let p = particle_index(wid, lid, nwg);
if (p >= params.dims.y) { return; }
```

Do not mix this with a second stride formula. If an implementation discovers that
`@builtin(num_workgroups)` is unavailable on the target stack, stop and amend this
contract before coding a uniform-stride fallback.

## Required code surfaces for v1.8.0

Implementation must touch or deliberately verify these surfaces:

| Surface | Required v1.8.0 work |
|---|---|
| `crates/fluid-lab/src/gpu/fluid.rs` | Add a shared particle dispatch-shape helper or stored shape; use it in `dispatch_mark`, all `dispatch_scatter` axes, `dispatch_g2p`, and `apply_impulse`; keep `PARTICLE_WG = 64` synchronized with shader helper constants. |
| `crates/fluid-lab/src/gpu/shaders/mark.wgsl` | Replace `p = gid.x` with the shared tiled particle index; keep occupancy clamp and atomic add unchanged. |
| `crates/fluid-lab/src/gpu/shaders/scatter.wgsl` | Replace `p = gid.x` once for all `AXIS` specializations; keep fixed-point `i32` atomics and face-stencil bounds unchanged. |
| `crates/fluid-lab/src/gpu/shaders/g2p.wgsl` | Replace `p = gid.x`; preserve wall-aware MAC sampling, CFL clamp, RK1 advect, wall friction, and deterministic recovery. |
| `crates/fluid-lab/src/gpu/shaders/impulse.wgsl` | Replace `p = gid.x`; verify manual slosh and wave-maker both route through the tiled standalone dispatch. |
| `crates/fluid-lab/src/gpu/mod.rs` | Update preflight capacity, boot diagnostics, scale-status messages, `max_particle_dispatch_count`, and any helper plumbing needed to pass adapter limits into `GpuFluid`; detailed timing should keep calling the same dispatch helpers. |
| `crates/fluid-lab/src/profiler/mod.rs`, `crates/fluid-lab/src/lib.rs`, `web/panels.js` | Keep `stats_json` truthful after the limit changes. Rename labels or fields if `max_particle_dispatch_count` stops meaning the old one-dimensional limit. |
| `crates/fluid-lab/src/gpu/timing.rs` | No section-count change expected; verify detailed mode still times `mark`, `scatter_*`, `g2p`, and does not need an added `impulse` section because impulse is outside the normal substep. |
| `crates/fluid-lab/src/settings/mod.rs` | No preset or range expansion for v1.8.0. Existing high-count input remains guarded by preflight. |
| `crates/fluid-lab/src/gpu/particles.rs`, `crates/fluid-lab/src/gpu/shaders/particles.wgsl` | No v1.8.0 render decimation work. Keep as measurement context only unless a renderer limit blocks valid submission. |

## Known risks and blockers

- Inconsistent indexing between mark, scatter, G2P, and impulse would corrupt the
  particle-grid transfer even if each individual shader compiles.
- Missing the partial-tile guard before `particles[p]` access can cause out-of-bounds
  storage reads/writes on the final row.
- G2P is wall-contact sensitive. Any tiling edit that also changes stencil skipping,
  wall friction, clamp epsilon, CFL, or recovery is out of scope and should block the
  implementation review.
- P2G must remain fixed-point integer atomics. A float accumulation rewrite is out of
  scope and would be a simulation decision, not a dispatch fix.
- Preflight must use `u64` for capacity math, then reject counts that cannot be
  represented by the WGSL `u32` particle index or the particle storage binding.
- Legal dispatch capacity is not an interactivity promise. Requested 8M can still be
  rejected by storage limits, browser watchdog behavior, memory pressure, or measured
  frame-time cost.

## Measurement table schema

After v1.8.0 implementation, record a fresh real-GPU matrix with one row per attempted
scale. Include requested 2M, 4M, and 8M when the adapter can safely attempt them.

| Requested | Seeded estimate | Actual particles | Particle dispatch shape | Scale status | Frame avg / p50 / p95 / p99 ms | Dropped this / total ms | GPU MB | Timing source | GPU costs ms/frame, sorted | Artifact | Notes/blocker |
|---:|---:|---:|---|---|---|---|---:|---|---|---|---|
| TBD | TBD | TBD | `groups_x x groups_y x 1` | TBD | TBD | TBD | TBD | `gpu-timestamp` or labeled fallback | TBD | TBD | TBD |

## Discipline rules

- Do not remove preflight safety as a shortcut.
- Do not treat 8M as a preset or a promise.
- Do not move render decimation/LOD into v1.8.0; put it in
  [`future-roadmap.md`](future-roadmap.md) unless the user promotes it.
- Do not update architecture or decisions docs from this audit alone.

## See also

- [`v1.8.0-particle-dispatch-tiling.md`](v1.8.0-particle-dispatch-tiling.md)
- [`v1.9.0-particle-performance-followup.md`](v1.9.0-particle-performance-followup.md)
- [`v1.3.0-scale-measurements.md`](v1.3.0-scale-measurements.md)
- [`../architecture/gpu-resources.md`](../architecture/gpu-resources.md)
- [`../architecture/simulation.md`](../architecture/simulation.md)
