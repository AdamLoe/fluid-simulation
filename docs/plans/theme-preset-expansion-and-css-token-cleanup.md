---
status:        shipped
owner:         unassigned
last_updated:  2026-06-17
okay_to_delete: true
long_lived:    false
owning_docs:
  - architecture/web-shell.md
---

# Theme preset expansion and CSS token cleanup

## Mission

Expand the dev-only theme system so the app has a broader, more useful set of visual
presets and a cleaner CSS token surface for manual tweaking. Done means `?dev=true`
still gates the Theme tab, the preset list grows to roughly 10-12 visually distinct
themes, at least one theme uses a true `#000000` app background, theme selector
swatches truthfully show colors used by each theme, and the top-level CSS variables are
organized enough that a maintainer can adjust colors and common treatments without
hunting through component rules.

## Scope

In scope:

- Keep Theme dev-only behind `?dev=true`.
- Expand `web/panels.js` theme metadata from the current small set to roughly 10-12
  presets.
- Include at least one truly black theme with `--app-bg: #000000`.
- Add stronger color variety across the preset set, including themes that do not read
  as minor off-black hue shifts.
- Update the theme selector preview from three swatches to about six semantic swatches.
  Suggested semantic slots are page background, main text, accent/action color, action
  background, panel/control background, and border/status color.
- Make selector swatches use actual theme colors. Prefer deriving swatches from the
  same theme token data used to apply CSS variables, or otherwise keep the preview
  metadata explicitly tied to named variables.
- Consolidate reusable colors and treatments into top-level CSS variables in
  `web/index.html`: backgrounds, surfaces, text hierarchy, borders/dividers, hover
  states, action/accent treatments, status colors, focus ring, shadows, radii, spacing,
  and stable control dimensions.
- Replace remaining hard-coded component colors where a shared variable is the natural
  owner, especially hover borders, reset buttons, badges, tooltips, and theme selector
  styles.

Out of scope:

- Making themes visible outside dev mode.
- Routing themes through the Rust settings registry or simulation config export/import.
- Changing WebGPU render colors, water material settings, or simulation visuals unless a
  shell CSS variable already controls that surface.
- Large layout redesign of the settings panel, toolbar, or bottom controls.
- Abstracting every one-off layout value. Prefer variables that make theme or compact
  UI tuning easier.

## Approach

This is one web-shell implementer workstream. Start from the canonical static shell:
`app/web/index.html`, `app/web/main.js`, and `app/web/panels.js`. The existing theme
contract is shell-owned: `panels.js` persists `fluidlab.theme.v1`, sets `data-theme` on
the root, and exposes `window.__fluidShell.setTheme()` / `activeTheme()`. Preserve that
shape unless an implementation blocker appears.

First define the semantic token contract the selector and CSS will share. The current
CSS already has useful variables (`--app-bg`, `--panel-bg`, `--text-body`,
`--text-strong`, `--accent`, `--accent-strong`, `--button-bg`, `--panel-border`, and
status colors), but several component rules still bake in literal colors. Normalize
those literals into named variables when they represent reusable treatment.

Then expand presets. The preset set should include a restrained default, several
dark-but-not-black variants, at least one pure black option, and more colorful action
systems. Avoid a set that is only teal/blue off-black variants; the user explicitly
wants more color.

Finally update the Theme tab UI so each option shows six compact semantic swatches.
The preview should help compare real theme behavior, not decorate the card with
hand-picked colors. If the implementation keeps CSS definitions in `index.html` and
theme metadata in `panels.js`, document the contract clearly so future edits keep them
in sync.

## Exit gate

- Without `?dev=true`, the Theme tab is hidden.
- With `?dev=true`, the Theme tab shows roughly 10-12 selectable presets.
- At least one preset applies `--app-bg: #000000` and visibly renders as true black
  behind the app chrome.
- Every theme option shows about six swatches, and those swatches correspond to actual
  theme variables/colors used by that preset.
- Selecting each theme updates the UI immediately, persists through
  `fluidlab.theme.v1`, and remains available through `window.__fluidShell.setTheme(id)`
  and `activeTheme()`.
- Desktop and narrow/mobile browser smoke checks show toolbar, launcher, settings tabs,
  inputs, badges, and theme cards remain legible with no text overlap in the highest
  contrast and most colorful themes.
- Run the relevant static/browser verification for visible web-shell changes. If the
  implementation touches only `web/*`, a full Rust test run is optional unless the local
  workflow or reviewer requires it.
- Update `architecture/web-shell.md` for the new preset count, true-black theme
  expectation, six-swatch selector behavior, and CSS variable/token contract.

## Handoff notes

- `docs/plans/dev-theme-system.md` is already shipped and delete-ready. Treat this as a
  follow-up plan, not a reopening of that shipped plan.
- `architecture/web-shell.md` currently documents the Theme tab as dev-only with
  `default`, `harbor`, and `signal`. That doc must be migrated when this ships.
- The implementation should keep theme state shell-owned and separate from registry
  settings unless a deliberate architecture change is made.
- Review focus should be on visual variety, truthful previews, and maintainability of
  the CSS token surface.

## Migration notes (filled in at ship time)

- Migrated the current theme contract, true-black `void` expectation, CSS token
  ownership, and six-swatch selector behavior into `docs/architecture/web-shell.md`.

## See also

- `docs/plans/index.md`
- `docs/plans/dev-theme-system.md`
- `docs/architecture/web-shell.md`
- `app/web/index.html`
- `app/web/panels.js`
