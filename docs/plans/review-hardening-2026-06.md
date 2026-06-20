---
status: shipped
owner: codex
last_updated: 2026-06-20
okay_to_delete: true
long_lived: false
owning_docs:
  - _meta/manifest.md
  - _meta/manifest.yaml
  - architecture/app-shell.md
  - architecture/web-shell.md
  - architecture/profiler.md
  - architecture/gpu-resources.md
  - agent-context/build-run.md
  - agent-context/deploy.md
  - agent-context/index.md
  - agent-context/testing.md
  - agent-context/orchestrating.md
  - decisions/observability.md
  - decisions/performance.md
  - overview.md
  - plans/future-roadmap.md
---

# Review hardening retrospective

This shipped retrospective records the June 2026 review-hardening slice. It is
coordination history only: durable facts were migrated into the owner docs listed in
frontmatter, so a fresh chat should not need this plan to understand the current app.

## Source commits

- `96139ef` Keep dev wasm builds out of release package
- `a08c3a5` Surface honest GPU timing status
- `f2af52b` Harden browser shell readiness and accessibility
- `39383a9` Clean up docs metadata drift
- `180b7b0` Clean up stale source doc references
- `1c5ae21` Remove obsolete source phase references
- `94f3be8` Refresh release wasm package
- `2c509b1` Fix stale COOP COEP config note
- `bc90bb7` Remove stale source phase comments

## What shipped

- Local development now builds ignored dev WASM into `app/web/pkg-dev/`, while release
  deployment uses the committed `app/web/pkg/` pair and Cloudflare's prebuilt path.
- Runtime observability reports honest timing sources and GPU device/surface status
  through the bridge, profiler panel, capture harness, and shell state.
- Browser shell boot readiness, fatal WebGPU overlays, focus behavior, and input/UI
  enablement were tightened so the shell does not expose controls before the app is
  ready.
- Documentation metadata, source comments, and stale phase/source references were
  cleaned up so durable docs point at current owners instead of retired phase language.
- The tracked release WASM package was refreshed, and stale COOP/COEP config wording
  was corrected.

## Migration

| Completed fact | Durable owner |
|---|---|
| Dev build vs release package split, static shell path, and `/pkg/*` local remap | `agent-context/build-run.md`, `agent-context/deploy.md`, `architecture/web-shell.md` |
| Cloudflare prebuilt deploy path, tracked release package, and COOP/COEP headers | `agent-context/deploy.md` |
| GPU timing-source honesty, timestamp-query fallback, zero-substep samples, and capture assertions | `architecture/profiler.md`, `decisions/observability.md`, `agent-context/testing.md` |
| GPU device/surface status values and fatal vs recoverable shell handling | `architecture/gpu-resources.md`, `architecture/web-shell.md`, `architecture/app-shell.md`, `decisions/observability.md` |
| Shell readiness, panel binding, focus/overlay accessibility, bottom controls, and helper API | `architecture/web-shell.md`, `architecture/app-shell.md` |
| Metadata cleanup, current orchestration constraints, and stale pre-v1 source-doc removal | `_meta/manifest.md`, `_meta/manifest.yaml`, `agent-context/index.md`, `agent-context/orchestrating.md`, `overview.md` |
| Performance-policy cleanup and future-only work boundaries | `decisions/performance.md`, `plans/future-roadmap.md` |

No durable implementation rule is intentionally left here. If one of the rows above is
needed for future work, read the owner doc rather than this retrospective.

## Verification

- Confirmed clean starting state with `git status --short`.
- Inspected the requested eight commits plus the follow-up cleanup commit `bc90bb7`
  with `git log --oneline` and `git show`.
- Read the owning architecture, decision, procedural, overview, and roadmap docs listed
  in frontmatter.
- Ran a targeted stale scan for package-path, timing-source, GPU-status, COOP/COEP, and
  stale phase/source-doc references before creating this plan.
