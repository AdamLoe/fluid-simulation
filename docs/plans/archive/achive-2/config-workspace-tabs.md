---
status:        abandoned
owner:         adamg
last_updated:  2026-06-08
okay_to_delete: true
long_lived:    false
owning_docs:
  - architecture/web-shell.md
  - architecture/settings.md
  - architecture/profiler.md
  - decisions/observability.md
---

# Config workspace tabs

## Superseded

This source draft has been folded into
[`v1.7.0-ui-shell-reorganization.md`](v1.7.0-ui-shell-reorganization.md). Use the
versioned plan for implementation. This file remains only as source context until the
next plan cleanup.

## Mission

Rework the config/profiler UI from two independent side panels into one left-side
workspace with first-class tabs. Done means config is no longer one long panel, the
profiler lives as a tab in the same workspace, and the panel structure matches the way
users scan the app: render controls, general scale/scene controls, physics controls,
interaction modes, and profiler data.

## Scope

In scope:

- Move the config workspace chrome to the left of the main panel content rather than
  making the tab affordance part of the scrollable config body.
- Split config into top-level tabs:
  - Render: current Render and Camera controls.
  - General: scene, grid, and particle-count controls.
  - Physics: physics, solver, compactness, classification, and related simulation
    controls.
  - Modes: current Interaction controls.
  - Profiler: the whole current profiler side panel.
- Remove the separate right-side profiler panel once the Profiler tab owns that view.
- The cog button opens this workspace. On first open it defaults to the General tab;
  after the user changes tabs, it reopens the last active tab.
- Advanced and Dev remain as collapsible groups inside the relevant tabs. They do not
  become global filters, and they do not disappear.
- Preserve the existing settings registry as the source of truth. If the implementer
  needs extra tab-routing metadata, it should be explicit and schema-driven, not based
  on label text parsing.
- Preserve localStorage restore, reset/reload badges, per-setting defaults, and the
  profiler polling model.

Out of scope:

- Changing simulation settings or defaults.
- Rewriting tooltip copy or compactness semantics; those already belong to the shipped
  settings-help work.
- Adding source/drain or new interaction physics.
- Redesigning the whole visual identity beyond the workspace structure.

## Approach

Treat this as a web-shell organization change with a small settings-schema question:
either map existing categories to tabs in `web/panels.js`, or add explicit tab metadata
to the Rust registry if category-to-tab mapping would become fragile. Prefer the
smallest durable option.

The profiler tab should reuse the existing `stats_json` data and rendering logic. It
should not introduce a second polling loop, a second stats bridge, or a new profiler
state model.

The workspace should have one visibility toggle owned by the top-right cog button. The
previous separate Config and Profiler toolbar buttons should disappear after the
workspace has a tab for Profiler.

## High-level questions

- Does Physics own every compactness/classification control, or should some
  user-facing compactness controls stay in General?
- When `?panels=on` is present, should it open the workspace on a specific tab or
  follow the same General-first / last-active behavior as the cog?

## Exit gate

- There is one left-side config workspace with Render, General, Physics, Modes, and
  Profiler tabs.
- The cog opens General first and later reopens the last active tab.
- Advanced and Dev controls remain available as collapsible groups inside tabs.
- The profiler no longer exists as a separate right-side panel.
- Settings still render from `config_json`, apply through `set_setting`, and persist
  through the existing localStorage path.
- Profiler data still renders from `stats_json` and updates on the existing cadence.
- A real browser capture shows the tabbed workspace without overlapping the canvas,
  toolbar, or bottom controls.

## Discipline rules

- Do not make a second hand-written settings inventory in JavaScript if Rust metadata
  is needed for durable routing.
- Do not hide advanced/dev controls permanently. Re-home or collapse them.
- Do not change simulation behavior while moving controls.

## Migration notes (filled in at ship time)

- Update `architecture/web-shell.md` with the new workspace/tab contract and removal
  of the standalone profiler panel.
- Update `architecture/settings.md` if tab routing becomes registry metadata.
- Update `architecture/profiler.md` only if the profiler panel data shape or polling
  behavior changes.
- Update `decisions/observability.md` if the tab model becomes a durable
  observability policy.

## See also

- `architecture/web-shell.md`
- `architecture/settings.md`
- `architecture/profiler.md`
