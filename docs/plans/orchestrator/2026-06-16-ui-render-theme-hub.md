---
status: shipped
owner: codex
last_updated: 2026-06-16
okay_to_delete: true
long_lived: false
owning_docs:
  - architecture/web-shell.md
  - architecture/settings.md
  - architecture/rendering.md
  - decisions/rendering.md
---

# UI, Render, And Theme Plans Orchestration

Disposable hub for orchestrating:

- `docs/plans/ui-shell-settings-simplification.md`
- `docs/plans/render-feature-removals.md`
- `docs/plans/dev-theme-system.md`

## Lifecycle

Tracked plan lifecycle: review the existing plans, implement them, review the shipped
work, migrate durable facts into architecture/decisions docs, then mark the source
plans shipped when green.

## Streams

| Stream | Area | Status | Last observed fact | Next action | Blockers |
|---|---|---|---|---|---|
| Plan review | Existing plan quality and sequencing | Completed | Reviewer says plans should not run in parallel; recommended order is render removals, UI simplification, then dev theme. Readiness edits were applied to the three source plans. | None | None |
| Implementation | Web shell/settings/render/theme | Completed | Implementer reported commit `bef859e Ship render cleanup UI simplification and themes`; local git status was clean and `git log -1` showed that commit. | Work review. | None |
| Work review | Shipped state verification | Completed | Independent review found the shipped plans substantially satisfied. Reviewer fixed stale app-shell diffuse prose and mobile dev-tab truncation, then re-ran targeted static/Rust checks; fresh Chrome capture launch was unavailable in this environment. | None | None |

## Decisions And Assumptions

- Use a tracked plan lifecycle because the request spans three existing plans across
  UI shell, settings, rendering, and theme behavior.
- Run a plan-review pass before implementation; the user asked to orchestrate plans,
  not just apply a known patch.
- Final visible/GPU verification should include the manifest build/test gates and a
  browser capture if the implementation changes visible behavior.
- Implement in sequence, not in parallel: `render-feature-removals.md`,
  `ui-shell-settings-simplification.md`, then `dev-theme-system.md`.
- Add ownership fences and promote plans from `draft` to `active` before implementation.
- Foam-removal scope: remove the Foam-tab/persistent `DiffuseSystem` feature only; keep
  the Water composite's speed-weighted whitewater/foam tint.

## Open Questions

- None currently.

## Shipped State

- `docs/plans/render-feature-removals.md`, `docs/plans/ui-shell-settings-simplification.md`, and `docs/plans/dev-theme-system.md` are `shipped` and `okay_to_delete: true`.
- Durable facts migrated into `architecture/rendering.md`, `architecture/settings.md`,
  `architecture/gpu-resources.md`, `architecture/profiler.md`,
  `architecture/web-shell.md`, and `decisions/rendering.md`.
- Implementation commit: `bef859e Ship render cleanup UI simplification and themes`.
- Review-fix commit: `7c929ee Fix review misses in UI render theme shipment`.
- Capture artifacts:
  - `captures/capture-default-water.png`
  - `captures/capture-dev-theme-default.png`
  - `captures/capture-theme-signal-mobile.png`
  - `captures/capture-env-hidden-default.png`
  - `captures/capture-env-visible-dev.png`
