---
status:        active
owner:         adamg
last_updated:  2026-06-08
okay_to_delete: false
long_lived:    true
owning_docs:
  - architecture/web-shell.md
  - architecture/app-shell.md
  - architecture/settings.md
  - architecture/profiler.md
  - architecture/rendering.md
  - architecture/gpu-resources.md
  - architecture/simulation.md
  - decisions/performance.md
  - decisions/rendering.md
  - decisions/scope.md
---

# Roadmap - current coordination map

This is the current coordination map for the presentation, particle-scale, and
water-look work. It is a plan router, not canonical architecture. Architecture and
decisions docs should be updated only while the implementation plan is in progress or
at ship time; research agents update the relevant plan/research docs first.

The orchestrator may run read-only research in parallel. Code-touching work should be
serialized unless the orchestrator has explicit file ownership boundaries, because the
UI work shares `web/main.js`, `web/panels.js`, and `web/index.html`, and particle work
shares GPU shader/runtime contracts.

## Current implementation map

| Order | Doc | Purpose | Start condition |
|---|---|---|---|
| v1.7.0 | [`v1.7.0-ui-shell-reorganization.md`](v1.7.0-ui-shell-reorganization.md) | Combined toolbar/header cleanup, right-side tabbed workspace, and bottom product-mode launcher. | Ready. |
| Planning | [`particle-dispatch-audit.md`](particle-dispatch-audit.md) | Audit particle-linear paths and update the dispatch plan if needed. | Can run before or alongside v1.7.0. |
| v1.8.0 | [`v1.8.0-particle-dispatch-tiling.md`](v1.8.0-particle-dispatch-tiling.md) | Raise the legal particle dispatch ceiling, preserve preflight safety, and measure the new scale facts. | After the audit is good enough, or as the first phase of this plan. |
| v1.9.0 | [`v1.9.0-particle-performance-followup.md`](v1.9.0-particle-performance-followup.md) | Optimize the measured bottleneck from v1.8.0. | Blocked on v1.8.0 evidence. |
| Research | [`water-rendering-research.md`](water-rendering-research.md) | Choose the water rendering direction and update the water implementation plan. | Can run in parallel with non-rendering work. |
| v1.10.0 | [`v1.10.0-water-rendering-optical-depth.md`](v1.10.0-water-rendering-optical-depth.md) | Implement the selected water-look change after research sharpens the plan. | Blocked on the research doc updating the plan. |

## Hero-water series (v1.12 → v1.18)

Seven sequential plans decomposed from [`chatgpt_plan.md`](chatgpt_plan.md), each
shippable on its own and capture-gated against the simple-particle baseline. They
**evolve the existing screen-space composite** into a hero water path rather than adding a
parallel renderer; the cross-cutting decisions live in the v1.12 plan and the rest inherit
them. Workflow per plan: detailed rewrite, then implementation.

| Order | Plan | Depends on | Note |
|---|---|---|---|
| v1.12.0 | [`v1.12.0-hero-water-refraction.md`](v1.12.0-hero-water-refraction.md) | — | Refraction **+ shared foundation** (RenderMode enum, Water tab, scene-color prepass, refractable environment). Build first. |
| v1.13.0 | [`v1.13.0-hero-water-foam.md`](v1.13.0-hero-water-foam.md) | v1.12 (loose) | Persistent diffuse particles. Most independent; order with v1.14 is flexible. |
| v1.14.0 | [`v1.14.0-hero-water-marching-cubes.md`](v1.14.0-hero-water-marching-cubes.md) | v1.12 | **Reverses the removed-surface decision.** Gated by a de-risk experiment (occupancy quads vs the v1.12 composite) — may exit without building MC. |
| v1.15.0 | [`v1.15.0-hero-water-environment-reflection.md`](v1.15.0-hero-water-environment-reflection.md) | v1.12 | Reflected procedural sky/room (distinct from v1.12's refracted background). |
| v1.16.0 | [`v1.16.0-hero-water-caustics.md`](v1.16.0-hero-water-caustics.md) | v1.12 | Needs the v1.12 floor/wall receivers + a light dir (shared with v1.15). |
| v1.17.0 | [`v1.17.0-hero-water-wet-walls.md`](v1.17.0-hero-water-wet-walls.md) | v1.12 (v1.13 helps) | Wet walls + meniscus; needs rendered walls + waterline. |
| v1.18.0 | [`v1.18.0-hero-water-temporal.md`](v1.18.0-hero-water-temporal.md) | v1.12–v1.17 | History-blend + camera-reset (NOT reprojection — no motion-vector infra). Lands last. |

Open decision recorded here when resolved: **does v1.14 marching cubes beat the v1.12
screen-space composite?** (the de-risk gate outcome).

## Locked user decisions

- The initial bottom product mode is **Auto Rotate**.
- Reset preserves the selected bottom mode; page reload does not. The bottom mode is
  not saved to localStorage.
- Auto Rotate and Waves are mutually exclusive. Manual exposes the existing pointer
  modes.
- Raw auto-roll and wave enable toggles should not be exposed as user config controls.
  Mode-specific values can remain visible, grouped under the mode they affect.
- Number keys target manual pointer modes only; they are not product-mode shortcuts.
- The config/profiler workspace moves to the **right** side only.
- The workspace always opens on **General**; no last-active tab restore and no tab
  routing metadata.
- Panels/tabs should be closed on initial load. Fix the load/init bug where panels
  appear open during startup.
- Remove or neutralize existing panel auto-open query behavior. Do not add query params
  for opening a specific tab. Capture defaults should be Auto Rotate mode with tabs
  closed.
- The top title should read its version from `web/package.json` or the package source
  that feeds that file.
- Remove the Copy Config JSON product affordance, including copy-specific plumbing;
  do not remove the `config_json` bridge needed by the rendered config panel.
- No high-count presets are planned. Render decimation/LOD is future work unless a
  later measured plan promotes it.
- Water rendering should prioritize configurable controls and improve both low-count
  density and high-count beauty where practical.

## Orchestration rules

- Treat v1.7.0 as one UI stream unless the orchestrator explicitly assigns non-
  overlapping files.
- Treat v1.8.0 as correctness first, measurement second. Do not fold the measured
  optimization into the dispatch-tiling pass unless the win is trivial and evidenced.
- Treat v1.9.0 as a placeholder until v1.8.0 produces the measurement table.
- Treat v1.10.0 as blocked until `water-rendering-research.md` records the visual
  target and chosen implementation direction.
- Research/planning docs may update versioned plans. They should not update
  architecture or decisions docs before implementation starts.

## Migration notes

When each version ships, migrate only the durable current-state facts and still-valid
decisions into the owning architecture/decisions docs. Keep raw research evidence in
the research doc or archive it; do not force screenshots and scratch comparisons into
architecture docs.

## See also

- [`future-roadmap.md`](future-roadmap.md)
- [`index.md`](index.md)
- [`../agent-context/orchestrating.md`](../agent-context/orchestrating.md)
- [`../agent-context/build-run.md`](../agent-context/build-run.md)
