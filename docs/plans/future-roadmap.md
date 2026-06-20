---
status:        active
owner:         adamg
last_updated:  2026-06-20
okay_to_delete: false
long_lived:    true
owning_docs:
  - architecture/rendering.md
  - architecture/gpu-resources.md
  - architecture/simulation.md
  - architecture/settings.md
  - decisions/performance.md
  - decisions/rendering.md
  - decisions/scope.md
---

# Future roadmap

This doc holds ideas that should not be smuggled into the current implementation
plans. Promote an item into a versioned plan only when the user explicitly wants it or
fresh measurement/design evidence makes it the next practical move.

## Future ideas

- **Render decimation / LOD for very high particle counts.** This may become useful
  after tiled dispatch makes larger simulations legal, but it should stay out of the
  current dispatch work. It changes visual truthfulness and needs measured
  before/after evidence.
- **High-count presets.** There should be no low/default/high preset system for now.
  If presets return, they should come from measurement and have honest labels.
- **Source/drain.** Still future mass-mutation work. It should create and destroy
  particles or water volume through an explicit allocation/recycling policy, not fake
  the effect with rendering or impulses.
- **Surface rendering / marching-cubes-class work.** Marching cubes may be discussed
  as research context, but the previous path had major visual quality problems. Any
  future surface renderer must be a new measured product decision, not a revival of the
  removed stack. This is also the "proper" fix for the residual low-density volume
  divergence below — an SDF/level-set surface would replace the splat-coverage
  approximation entirely.
- **Tighten the volume-neutral-density residual (Phase 2 of the shipped
  volume/density decoupling).** Density is now volume- and motion-neutral via
  splat-radius scaling + auto surface dilation + the rest-density coupling, but the
  physics `liquid_cells` count is only invariant within ~12% across a `{1,8,32}` density
  sweep (a density-dependent dilation rind). A pixel/thickness coverage metric plus a
  `SPLAT_RADIUS_PER_SPACING` tuning loop (use `app/tools/density_motion_sweep.mjs`)
  could tighten this further. Visible volume is already held by the splat scaling, so
  this is polish, not a correctness gap — promote only if the residual becomes visible.
- **Richer water configuration.** Water-look work should remain highly configurable:
  density, tint, depth buildup, particle/surface balance, and inspectability should be
  tunable rather than hardcoded into one cinematic preset.
- **Unsupported-WebGPU presentation.** Before public sharing, verify the unsupported
  overlay and any static poster/caveat copy.
- **Full GPU numeric golden harness.** Add a durable GPU-side numeric regression
  oracle when the extra readback/test infrastructure is worth the cost. Until then,
  host tests cover CPU-reference math and browser captures prove runtime acceptance,
  not bit-for-bit GPU numerics.

## Promotion rule

When an item is promoted, create a versioned plan in `docs/plans/` and move only the
actionable current context into that plan. Architecture and decisions docs update when
implementation is underway or shipped, not while this doc is just holding future
ideas.

## See also

- [`index.md`](index.md)
