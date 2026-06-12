---
status:        draft
owner:         codex
last_updated:  2026-06-12
okay_to_delete: false
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
2. Record baseline drift for default tank/settings.
3. Choose the smallest correction that fits the existing particle/MAC grid shape and
   GPU-native no-readback rule.
4. Implement behind a setting if the correction carries visual trade-offs.
5. Verify with capture evidence and update simulation docs.

## Subagents

- Planner/red-team: volume correction design after drift metric exists.
- Worker: bounded correction implementation.
- Reviewer: simulation correctness review before shipping.

## Exit Gate

- `cd app && cargo test --lib`
- `cd app && cargo build --target wasm32-unknown-unknown`
- Capture evidence comparing baseline and corrected volume drift over the same run.

## Migration Notes

Fill at ship time:

- Correction behavior -> `architecture/simulation.md`.
- Any pressure coupling -> `architecture/pressure-solver.md`.
- Trade-off and default policy -> `decisions/simulation.md` and
  `decisions/performance.md`.
