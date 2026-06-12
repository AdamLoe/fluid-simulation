---
status:        shipped
owner:         codex
last_updated:  2026-06-12
okay_to_delete: true
long_lived:    false
owning_docs:
  - architecture/simulation.md
  - architecture/pressure-solver.md
  - decisions/simulation.md
  - decisions/performance.md
---

# LLM Review 05 - Volume Fidelity

## Mission

Attack the visible FLIP volume-loss defect with a measurable correction strategy. Done
means volume drift has a baseline, a selected correction mechanism, and capture/test
evidence that it improves the defect without destabilizing the liquid.

## Scope

In scope:

- Selecting between density projection, position-based correction, or a smaller
  occupancy-bias improvement after plan 02 exposes drift clearly.
- One bounded implementation of the selected correction.
- Tests or capture assertions that compare volume drift against baseline.

Out of scope:

- Whitewater/spray/bubble systems.
- Higher grid resolution as a substitute for fixing drift.
- Active-cell compaction unless it becomes a hard prerequisite.

## Approach

1. Wait for plan 02's volume-drift metric.
2. Record baseline drift for default tank/settings and for
   `physics.volume_stiffness=0` to prove whether the existing bias helps.
3. Sweep existing `physics.rest_density`, `physics.volume_stiffness`, and
   `physics.drift_clamp` before adding new code.
4. Choose the weakest default/settings change that reduces negative occupied-cell
   drift without visible inflation or pulsing. If tuning does not clearly help, leave
   code/defaults unchanged and record the evidence.
5. Only add code if the existing occupancy-bias surface fails; any formula change must
   remain one-sided, clamped, GPU-native, and pressure-projection-coupled.
6. Verify with capture evidence and update simulation docs.

## Measurement Pass - 2026-06-12

All captures used the default 64x64x64 tank, 247500 particles, `MEASURE_WAIT=12000`,
the real Chrome/WebGPU capture harness, and `classify.surface_dilation=0`. The drift
numbers below are the capture sidecar occupied-cell-count proxy, not physical volume.

| Capture | URL settings | Baseline cells | Final cells | Drift ratio | Outcome |
|---|---|---:|---:|---:|---|
| `captures/llm-review-05-default.png` | default bias (`rest_density=8`, `volume_stiffness=0.45`, `drift_clamp=0.5`) | 34005 | 38708 | +0.1383 | Default has positive occupied-cell drift over this window. |
| `captures/llm-review-05-stiffness0.png` | `physics.volume_stiffness=0` | 13544 | 9510 | -0.2978 | Disabling the existing bias causes large negative drift. |
| `captures/llm-review-05-rd6-vs075-dc075.png` | `physics.rest_density=6`, `physics.volume_stiffness=0.75`, `physics.drift_clamp=0.75` | 49491 | 50969 | +0.0299 | Lower in-window drift, but final cells are inflated versus default. |
| `captures/llm-review-05-rd4-vs12-dc10.png` | `physics.rest_density=4`, `physics.volume_stiffness=1.2`, `physics.drift_clamp=1.0` | 62780 | 62413 | -0.0058 | Near-flat in-window drift, but final cells are strongly inflated versus default. |

The sweep proves the current pressure-coupled occupancy bias is materially better than
`volume_stiffness=0`, but neither stronger candidate qualifies as a default change:
both drive substantially higher final occupied-cell counts than the default run. No
shader, solver, or settings-default change was made in this pass.

## Narrow Sweep - 2026-06-12

A read-only review recommended testing only nearby values with `physics.rest_density`
held at the default `8`, because lowering rest density made earlier candidates look
better by inflating occupied cells. These captures used the same default tank,
`MEASURE_WAIT=12000`, real Chrome/WebGPU, detailed GPU stats, and
`classify.surface_dilation=0`.

| Capture | URL settings | Baseline cells | Final cells | Drift ratio | Outcome |
|---|---|---:|---:|---:|---|
| `captures/llm-review-05-narrow-default.png` | current default (`rest_density=8`, `volume_stiffness=0.45`, `drift_clamp=0.5`) | 34423 | 34350 | -0.0021 | Best result in this sweep; final cells stayed near the control. |
| `captures/llm-review-05-rd8-vs030-dc050.png` | `rest_density=8`, `volume_stiffness=0.30`, `drift_clamp=0.5` | 34349 | 31434 | -0.0849 | Softer correction lost substantially more occupied cells. |
| `captures/llm-review-05-rd8-vs035-dc050.png` | `rest_density=8`, `volume_stiffness=0.35`, `drift_clamp=0.5` | 34364 | 32649 | -0.0499 | Worse than default. |
| `captures/llm-review-05-rd8-vs045-dc035.png` | `rest_density=8`, `volume_stiffness=0.45`, `drift_clamp=0.35` | 34426 | 33654 | -0.0224 | Worse than default. |
| `captures/llm-review-05-rd8-vs035-dc035.png` | `rest_density=8`, `volume_stiffness=0.35`, `drift_clamp=0.35` | 34362 | 32225 | -0.0622 | Worse than default. |

Conclusion: ship no code/default change from this overhaul. The existing one-sided,
clamped, pressure-coupled occupancy bias remains the selected correction surface:
`volume_stiffness=0` proved bad, stronger earlier candidates inflated occupied-cell
counts, and nearby softer/clamped candidates were worse than the current default.
Any future formula work needs a better physical-volume metric or visual pulsing gate,
not another default tweak from the current proxy alone.

## Design Review Notes

- Use the existing pressure-coupled occupancy-bias surface first:
  `physics.rest_density`, `physics.volume_stiffness`, and `physics.drift_clamp`.
- Do not introduce PBF/position-based correction, particle spawning/deletion, active
  cell compaction, source/drain behavior, or CPU feedback loops in this overhaul.
- Keep `classify.surface_dilation=0` for gates so occupied-cell drift is not improved
  by counting empty adjacent cells as liquid.
- `gpu.liquid_cells` remains a liveness proxy, not physical volume.

## Subagents

- Planner/red-team: volume correction design after drift metric exists.
- Worker: bounded correction implementation.
- Reviewer: simulation correctness review before shipping.

## Exit Gate

- `cd app && cargo test --lib` passed, 54 tests.
- `cd app && cargo build --target wasm32-unknown-unknown` passed.
- Narrow capture sweep above compared the existing default against nearby candidates.

## Migration Notes

- Measurement passes found no justified default/code change.
- Correction behavior -> `architecture/simulation.md`.
- Pressure coupling -> `architecture/pressure-solver.md`.
- Trade-off and default policy -> `decisions/simulation.md` and
  `decisions/performance.md`.
