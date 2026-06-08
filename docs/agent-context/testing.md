# Testing

## When does this apply

You're adding or running tests, or deciding whether a claim is proven.

## Two layers of verification

This project has two distinct verification surfaces, and they prove different things:

1. **Host unit tests** prove the *math* (indexing, transfers, divergence reduction, CG
   convergence, wall-aware gather behaviour) on the CPU reference in `app/crates/fluid-lab/src/sim/`.
2. **Browser capture** proves the *running GPU app* — see
   [`build-run.md`](build-run.md). This is the real acceptance signal; host tests
   cannot exercise the wgpu/WGSL paths.

## Host tests

The `wgpu` / `web-sys` paths are wasm-only and excluded from the native test build;
only the indexing/solver/simulation-reference math in `app/crates/fluid-lab/src/sim/` runs on the host. Run them in WSL:

```
wsl.exe -d Ubuntu-24.04 -- bash -lc 'cd /home/adamg/fluid-simulation/app && cargo test --lib'
```

What is covered (point to the `#[test]` functions, don't trust this prose to stay
complete): cell/face index bijectivity & staggered buffer counts, world↔grid
round-trip + escaped-particle clamp, cell classification, wall-aware MAC gather cases,
an interior divergence-free check, a deterministic divergence-reduction case, and
CG-vs-Jacobi convergence
(`app/crates/fluid-lab/src/sim/pressure.rs → cg_beats_jacobi_16cubed`).

## Acceptance honesty (non-negotiable)

- Never claim "interactive", "30 FPS", "64³ works", or "fast" without raw profiler
  output (or the labeled minimum-honest fallback). Don't fabricate per-pass GPU times
  when `timestamp-query` is unavailable. See
  [`../architecture/profiler.md`](../architecture/profiler.md).
- A numerically-passing sim can still be visibly broken (volume loss, clumping,
  wall-stick). The cheap liveness gate is occupied-cell count staying within ~±10%
  over ~10 s — a single throttled counter, not a readback-heavy diagnostic.

## See also

- [`build-run.md`](build-run.md) — the capture harness (browser acceptance).
- [`../architecture/pressure-solver.md`](../architecture/pressure-solver.md) — the CG host reference + tests.
- [`../decisions/platform.md`](../decisions/platform.md) — why the CPU reference stays tiny.
