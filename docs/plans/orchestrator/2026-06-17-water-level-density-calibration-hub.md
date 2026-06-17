---
status: shipped
owner: orchestrator
last_updated: 2026-06-17
okay_to_delete: true
long_lived: false
owning_docs:
  - architecture/simulation.md
  - architecture/settings.md
  - architecture/rendering.md
  - decisions/simulation.md
  - decisions/scope.md
---

# Water Level + Particle Density Calibration Hub

## Request

The water level and particle density controls do not behave predictably:

- Water level is imprecise and varies wildly across slider values and cube sizes.
- Particle density appears wrong; high water amounts add too much liquid, compress the fluid, and destabilize behavior.
- Do not jump straight to a solution. First write a plan, then experiment with likely causes and fixes.

## Lifecycle

Tracked plan lifecycle. This touches user-facing settings, simulation initialization/mass, and fluid stability, so the first output must be an implementation-ready experiment plan before any code changes.

## Streams

| Stream | Area | Status | Last observed fact | Next action | Blockers |
|---|---|---|---|---|---|
| Plan | Simulation/settings calibration | completed | Wrote and revised `docs/plans/water-level-density-calibration.md` with explicit whole-tank volume semantics, alternatives, metrics, tolerances, and a post-measurement checkpoint. | None. | None |
| Plan review | Experiment design/correctness | completed | Review found ambiguity in partial-footprint fill semantics, weak compression metrics, missing numeric tolerances, and no hard measurement checkpoint; the plan was revised to address those findings. | None. | None |
| Implementation | Code + tests + captures | completed | Runtime now uses canonical whole-tank represented volume semantics plus generated-count effective density for reset calibration. Host tests, WASM build, and browser captures are green. | None. | None |
| Work review | Shipped behavior verification | completed | Diff inspected; durable docs updated; plan context migrated. | None. | None |

## Decisions

- Treat this as a calibration/stability investigation, not a direct tuning patch.
- Evidence should include current formula tracing, bounded experiments across tank sizes and level/density values, and verification that target volume/mass maps predictably to initialized particles.
- Shipped semantics: `scene.fill_level` targets whole-tank represented volume;
  presets keep shape but not footprint-relative volume. A 2% top-air guardrail caps
  near-full/suspended cases. `particles.density` is fidelity/cost; generated lattice
  count calibrates reset-time effective density where known.

## Open Questions

- None for the user yet. Default assumption: the user wants the controls to map to intuitive physical targets such as filled volume fraction and particles-per-cell/spacing, with stable mass independent of visual tank shape.

## Agent Evidence

- Planning pass wrote `docs/plans/water-level-density-calibration.md`.
- Plan review completed and blocked direct implementation until the plan specified:
  whole-tank versus footprint-relative fill formulas, stronger compression metrics,
  provisional numeric tolerances, and a hard post-measurement checkpoint. The planning
  agent revised the plan accordingly.
- Key traced surfaces: `scene.fill_level`, `particles.density`, `preset_blocks`,
  `resolved_particle_count`, `generate_particles`, `effective_particle_density`,
  `effective_rest_density`, `divergence.wgsl`, `stats_json`, and
  `tools/capture.mjs`.
- Current behavior likely mixes two semantics: some docs/help text describe a literal
  waterline/full-footprint fill, while current Falling Blob code seeds a suspended
  central body whose normalized volume scales with fill.
- Measurement checkpoint completed on 2026-06-17:
  - Registry-default `scene.drop_height = 1.0` clamps suspended Falling Blob and Double
    Splash blocks to the tank ceiling, leaving zero top-air margin in the measured
    Falling Blob / Double Splash cases.
  - Current Dam Break is footprint-relative: 50% fill represents `0.163170` of the whole
    tank; 100% represents `0.326340`.
  - At fixed Falling Blob fill/grid, density `1/8/32` leaves seeded fraction unchanged
    at `0.433959`, while generated particles trail requested count (`107,736` vs
    `111,093`; `879,844` vs `888,747`; `3,493,413` vs `3,554,988`).
  - Host uniform-density clamp proxy is zero for the measured subset, so local
    occupancy/GPU warm-up evidence is still needed before tuning correction constants.
  - Selected next-pass hypothesis: canonical whole-tank target volume fraction for
    water amount, and actual-generated-count effective density for rest/dilation/splat
    calibration.
- Behavior pass shipped on 2026-06-17:
  - Code: `scene.fill_level` maps to whole-tank represented volume; Falling Blob and
    Double Splash preserve suspended shapes with a 2% top-air guardrail; Dam Break
    grows from height to footprint expansion when its old footprint cannot hold the
    whole-tank target.
  - Code: reset-time rest density, auto surface dilation, and splat spacing use
    generated particle count divided by seeded cells after lattice generation.
  - Tests: `cd app && cargo test --lib` passed, 79 tests.
  - Build: `cd app && cargo build --target wasm32-unknown-unknown` passed.
  - WASM package: `cd app && wasm-pack build crates/fluid-lab --target web --out-dir
    ../../web/pkg --dev` passed and regenerated tracked `app/web/pkg` outputs.
  - Browser capture: `cd app && ./local_dev.sh` served `http://localhost:5184/`;
    `tools/capture.mjs` captures passed for 64^3 Falling Blob fill 20/80,
    80x40x80 Falling Blob fill 50, 80x40x80 Dam Break fill 20/80, and fixed-fill
    density sweeps on 32^3 Falling Blob and Dam Break at density 1/8/32. All reported
    `scale_status: "ok"`, `timing: "gpu-timestamp"`, WebGPU smoke PASS, and
    `gpuDeviceStatus: "ok"`.
  - Capture caveat: post-warm-up `liquid_fraction` remains a proxy, not exact mass.
    Density-32 fixed-fill captures had a lower occupied-cell fraction than density
    1/8 because low/reference-density cases receive the auto dilation rind while high
    density does not; represented seed volume and generated particle count remained
    the authoritative volume/cost evidence.
  - Migration: durable facts moved into `architecture/simulation.md`,
    `architecture/settings.md`, `architecture/rendering.md`,
    `decisions/scope.md`, and `decisions/simulation.md`.
