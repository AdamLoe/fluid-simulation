---
status: active
owner: codex-orchestrator
last_updated: 2026-06-17
okay_to_delete: false
long_lived: false
owning_docs:
  - architecture/web-shell.md
---

# UI/UX Screenshot Review Loop

## Request

Capture the app, settings, and each settings tab. Use the screenshots to review UX/UI,
update the app for a cleaner interface, then review again. Repeat a couple of passes
as needed and ship the result.

## Lifecycle

Tracked plan lifecycle. This is user-facing UI work with browser screenshots,
implementation, and repeated review loops.

## Streams

| Stream | Area | Status | Last observed fact | Next action | Blockers |
|---|---|---|---|---|---|
| Screenshot audit 1 | Current app/screens/settings tabs | Complete | Captured main app and Scenario, Simulation, Camera, Surface, Color, Refraction, Reflection, Environment, Profiler, Theme via real Chrome/WebGPU | Use findings for implementation | None |
| Code map | Web shell ownership | Complete | Edits likely belong in `app/web/index.html` CSS/layout and `app/web/panels.js` panel behavior; static shell only | Feed implementation prompt | None |
| Implementation pass 1 | Web shell UI cleanup | Complete | Added safe-area launcher inset, compact settings header/legend, scannable rows, scroll padding, and profiler summary grouping | Feed review findings into pass 2 if requested | None |
| Review pass 1 | Browser verification and UX review | Complete | Captured updated main, Scenario, Simulation top/bottom, Profiler, and dev Theme via real Chrome/WebGPU | Use screenshots for any follow-up polish | None |
| Implementation pass 2 | Follow-up polish | Pending | Waiting on review | Address review findings | Review findings |
| Final review/gates | Build/tests/capture/docs/commit | Pending | Waiting on loop | Run final verification and ship | Prior passes |

## Decisions And Assumptions

- Default dials apply: `review-medium`, `cost-medium`; the user's explicit request to
  orchestrate with subagents floors this to a multi-agent tracked lifecycle.
- Screenshots should use the app's real browser capture path where practical.
- Durable web-shell facts belong in `docs/architecture/web-shell.md` if behavior or
  verification workflow changes.

## Evidence Log

- 2026-06-17: Intake complete. Manifest routes web shell changes to
  `architecture/web-shell.md`; visible changes require browser capture via
  `app/tools/capture.mjs`.
- 2026-06-17: Code-map agent reported settings tabs: Scenario, Simulation,
  Camera, Surface, Color, Refraction, Reflection; dev-only Environment and Theme
  require `?dev=true`; Profiler is always shell-appended.
- 2026-06-17: Screenshot audit captured files under `app/captures/ui-audit-*`.
  Browser capture worked with Windows Node/Chrome and WebGPU smoke passed. Key
  findings: bottom launcher too close to viewport edge; Simulation scroll loses
  context; settings rows are dense; status dots are unexplained; short tabs leave
  heavy empty panel space; Profiler is hard to scan.
- 2026-06-17: Implementation pass 1 updated `app/web/index.html` and
  `app/web/panels.js` only for shell UI polish: launcher safe-area inset, compact
  active-tab header with Live/Reset/Reload legend, wrapping labels with stable
  controls, longer-tab scroll padding, and Profiler summary grouping.
- 2026-06-17: Targeted static checks passed: `node --check app/web/panels.js` and
  `node --check app/web/main.js`.
- 2026-06-17: Browser capture pass 1 used a no-rebuild static server on
  `http://localhost:5185/` to avoid rewriting `app/web/pkg`. Required captures passed
  WebGPU smoke with `gpuDeviceStatus:"ok"` and wrote:
  `app/captures/ui-pass1-main.png`, `ui-pass1-tab-scenario.png`,
  `ui-pass1-tab-simulation.png`, `ui-pass1-tab-simulation-bottom.png`,
  `ui-pass1-tab-profiler.png`, and `ui-pass1-tab-theme-dev.png`.
- 2026-06-17: Visual spot check found and fixed a sticky section-label ghost at the
  Simulation bottom scroll position; final captures were refreshed after the fix.

## Open Questions

- None yet.
