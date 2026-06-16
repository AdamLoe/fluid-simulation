---
status:        draft
owner:         unassigned
last_updated:  2026-06-16
okay_to_delete: false
long_lived:    false
owning_docs:
  - architecture/web-shell.md
  - architecture/settings.md
---

# Dev theme system

## Mission

Add a dev-only Theme tab that lets the user switch among multiple app themes and makes
future themes easy to add. Done means `?dev=true` reveals a Theme tab, theme selection
persists in localStorage, the app styling is expressed through a broad CSS variable
surface, and the initial theme set includes neutral, dual-color, and tri-color options
with meaningful visual variety.

## Scope

In scope:

- Add a Theme tab that is hidden unless the URL contains `dev=true`.
- Use the same shared dev-mode check as the Environment tab planned in
  `ui-shell-settings-simplification.md`.
- Persist the selected theme in localStorage for later dev sessions.
- Abstract the app's themeable colors, borders, text, surfaces, controls, focus states,
  and action treatments into CSS variables as far as practical.
- Provide several theme presets, including dual-color and tri-color themes.
- Include themes with neutral backgrounds plus colored borders/text/actions, and at
  least one theme with stronger colored action/button backgrounds.
- Avoid a one-note palette where the whole interface reads as a single hue family.

Out of scope:

- Shipping theme selection as an end-user feature; it remains dev-only for this plan.
- Reworking simulation/render colors inside the WebGPU water renderer unless a CSS
  variable is already clearly wired to those controls.
- Settings-tab compaction, tab renaming, or Environment-tab gating beyond sharing the
  `dev=true` helper with `ui-shell-settings-simplification.md`.

## Approach

This is primarily a web-shell/CSS stream. Prefer shell-owned theme state unless the
implementation finds a strong reason to make theme a registry setting. A shell-owned
theme keeps it separate from simulation config import/export, which is being removed
from the visible UI in the companion plan.

Define a named theme contract before creating presets: base page/canvas chrome,
panel/surface/background, text hierarchy, borders/dividers, controls, icon buttons,
segmented controls, sliders/inputs, focus/active/warning states, and settings-tab
selection should all have explicit variables where they exist today.

The initial themes should demonstrate different design directions instead of minor hue
shifts. Keep the default restrained and compatible with the compact UI work; add dual
and tri-color variants that use color deliberately for borders, text accents, and
actions.

## Exit gate

- `?dev=true` reveals a Theme tab; without it, the tab is hidden.
- Selecting a theme updates the UI immediately and persists across reloads through
  localStorage.
- The default theme still works with no stored theme.
- Theme presets visibly differ across background, border/text accents, and action
  treatment without relying on a single dominant hue family.
- Desktop and narrow/mobile captures show controls and settings text remain legible and
  non-overlapping in every included theme.
- Update `architecture/web-shell.md` for the dev-gated Theme tab, localStorage key, and
  CSS variable/theme contract. Update `architecture/settings.md` only if theme is routed
  through registry metadata.

## Handoff notes

- The repo may already contain unrelated modified/deleted files. Do not revert them.
- Coordinate the shared `dev=true` helper with `ui-shell-settings-simplification.md`.
- If this plan runs before the settings-tab simplification, isolate theme-tab rendering
  so the simplification implementer can flatten tab styles without rewriting theme state.

## Migration notes (filled in at ship time)

- Shell-owned theme state, dev gating, localStorage behavior, and CSS variable contract
  go to `architecture/web-shell.md`.
- Registry-backed theme metadata, if added, goes to `architecture/settings.md`.

## See also

- `docs/plans/index.md`
- `docs/architecture/web-shell.md`
- `docs/architecture/settings.md`
- `docs/plans/ui-shell-settings-simplification.md`
