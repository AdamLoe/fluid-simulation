---
status:        abandoned
owner:         adamg
last_updated:  2026-06-08
okay_to_delete: true
long_lived:    false
owning_docs:
  - architecture/web-shell.md
  - architecture/settings.md
---

# Top toolbar and header cleanup

## Superseded

This source draft has been folded into
[`v1.7.0-ui-shell-reorganization.md`](v1.7.0-ui-shell-reorganization.md). Use the
versioned plan for implementation. This file remains only as source context until the
next plan cleanup.

## Mission

Make the top chrome quieter and more app-like: the Fluid Lab header should read as
transparent overlay chrome, and the top-right commands should be compact icon buttons
for pause/play, restart, and config. Done means the header no longer feels like a
solid control strip, the visible title includes the current app version, unnecessary
text buttons are gone, and the config JSON copy button is removed.

## Scope

In scope:

- Make the Fluid Lab header see-through/transparent over the stage.
- Keep the Fluid Lab title visible in the transparent header with the current app
  version attached. Prefer a single source of truth such as the crate/package version
  over a second hardcoded label. Current crate version observed during planning:
  `1.1.0`.
- Replace the current text buttons with top-right icon buttons:
  - pause/play stateful icon
  - restart/reset icon
  - cog/config icon
- Remove the separate Profiler toolbar button if the profiler is moved into the config
  workspace tab plan.
- Remove the "Copy Config JSON" action from the config panel. It is not product-facing
  enough to keep in the UI.
- Keep accessible labels/tooltips for icon-only buttons.
- Keep keyboard behavior that already exists: `r` resets, typing in config fields
  ignores shortcuts.

Out of scope:

- Rebuilding the config workspace tabs. That is owned by `config-workspace-tabs.md`.
- Changing pause/reset semantics in Rust.
- Adding a debug/export replacement for copied config JSON unless the user asks for
  one later.

## Approach

This is a small web-shell cleanup. Keep it mostly in `web/index.html`, `web/main.js`,
and `web/panels.js`.

The cog should become the single public entry point for the config workspace. If the
workspace tab plan has not shipped yet, the cog can temporarily toggle the existing
config panel, but the final shape should assume the profiler is reachable through the
workspace tabs.

The pause button should communicate state through the icon and accessible label, not
by growing/shrinking text from "Pause" to "Resume".

## High-level questions

- Should restart reset only the sim, or should it also return the bottom mode launcher
  to its initial choice?

## Exit gate

- The header is transparent/see-through and does not visually fight the canvas.
- The header still shows Fluid Lab with the current app version attached.
- Top-right controls are icon buttons for pause/play, restart, and config.
- Pause state remains correct when loaded with `?paused=1`.
- Reset still rebuilds reset-class settings and refreshes any config badges.
- The config panel no longer includes "Copy Config JSON".

## Discipline rules

- Icon-only buttons still need accessible names.
- Do not add a new toolbar Profiler button after the workspace plan moves Profiler
  into a tab.
- Do not remove `config_json`; remove only the UI copy action.

## Migration notes (filled in at ship time)

- Update `architecture/web-shell.md` for the toolbar button set, transparent header,
  and removal of the config JSON copy action.
- Update `architecture/settings.md` only if a dev/export config affordance is added
  elsewhere.

## See also

- `architecture/web-shell.md`
- `config-workspace-tabs.md`
