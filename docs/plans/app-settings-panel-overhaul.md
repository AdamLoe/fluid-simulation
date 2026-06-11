---
status:        shipped
owner:         unassigned
last_updated:  2026-06-11
okay_to_delete: true
long_lived:    false
owning_docs:
  - architecture/web-shell.md
  - architecture/settings.md
  - architecture/app-shell.md
  - decisions/observability.md
  - decisions/platform.md
---

# App Settings Panel Overhaul

## Mission

Replace the current Workspace/General/Advanced/Dev settings experience with a right-side
settings panel that sits beside the main canvas, uses normal wrapping tabs, keeps the
Profiler available, and groups controls by clear product function. Done means a user can
find water, wall, lighting, scenario, simulation, interaction, and profiler controls
without understanding internal `panel_group` tiers or the old Workspace vocabulary.

## Scope

In scope:

- Move the settings surface to the right of the main panel/canvas in the layout instead
  of overlaying it.
- Remove the "Workspace" concept from user-facing UI copy, ARIA labels, capture helpers,
  and docs. Use Settings or another direct product label.
- Remove the General tab. The top of the side panel should be a normal tab list, not a
  Workspace title plus active-tab subtitle.
- Keep tab wrapping. Many functional tabs are expected and acceptable.
- Remove the Advanced and Dev concepts entirely from the app settings model. Do not just
  hide collapsed drawers; update the registry/panel contract so rows are grouped by the
  new functional tab taxonomy.
- Keep the Profiler as a tab in the same right-side panel. It is not a config tab and
  should not participate in reset-to-default behavior.
- Preserve settings persistence, per-setting reset, tab-level reset-to-default for config
  tabs, reset/reload hints, help affordances, and enum/color/log slider behavior.

Out of scope:

- Replacing the vanilla JS shell with a framework.
- Changing solver/render behavior while regrouping settings.
- Removing or renaming stable setting ids unless an id is truly obsolete and a migration
  path is provided.
- Broad visual redesign of the simulation canvas, toolbar, or profiler metrics beyond
  what the side-panel layout requires.

## Proposed Tab Taxonomy

Use product-facing tabs with about five or more settings each where practical. Small tabs
are acceptable only when the function is sharply distinct and would be harder to find if
merged.

Recommended first pass:

| Tab | Settings to collect |
|---|---|
| Scenario | `scene.*`, particle count, grid dimensions, reset-class initial-condition controls such as the new drop-height setting |
| Simulation | gravity, damping, fixed dt, max substeps, volume correction, liquid classification, solver iterations/tolerance |
| Modes | auto-rotate and wave-maker tuning; keep raw hidden scheduler booleans hidden if product mode still owns them |
| Camera & View | camera pose/distance plus user-facing view/render-mode controls |
| Water Surface | hero-water enable/debug, smoothing, normals, refraction, particle size/edge, water body shape controls |
| Water Color | absorption, tint, transparency, deep-water darkening, whitewater and speed-color controls |
| Environment | floor pattern, wall visibility, environment mode/rotation/brightness, skybox |
| Sun & Reflection | reflection, roughness, specular, sun direction/intensity, micro-normal controls |
| Wall Fill | `render.hero.flat_water.*` controls |
| Wet Wall | `render.hero.wet_wall.*` controls |
| Caustics | `render.hero.caustics.*` controls |
| Temporal | `render.hero.temporal.*` controls |
| Diffuse Water | `render.diffuse.*` controls |
| Profiler | existing profiler DOM, polled only when visible |

The implementer may merge adjacent tabs if the registry has fewer active controls than
expected, but avoid returning to broad buckets like General, Render, Physics, Advanced,
or Dev.

## Approach

Owned streams:

| Stream | Area | Owned files |
|---|---|---|
| Registry taxonomy | Replace `PanelGroup`-first grouping with functional tab metadata | `crates/fluid-lab/src/settings/mod.rs` |
| Panel rendering | Build config tabs from the new taxonomy; remove Advanced/Dev drawers and General/Workspace assumptions | `web/panels.js`, `web/index.html` |
| Layout | Make the right panel occupy layout space beside the canvas, with responsive behavior for narrow viewports | `web/index.html`, any shell CSS in that file |
| Shell helpers | Rename or alias helper methods so captures can still open a tab while the user-facing concept becomes Settings | `web/main.js`, `web/panels.js`, `tools/capture.mjs` if needed |
| Docs | Migrate shipped current-state facts | `architecture/web-shell.md`, `architecture/settings.md`, `architecture/app-shell.md` |

Implementation notes:

- Treat the new functional tab as part of the serialized `config_json` contract or a
  deterministic JS-side mapping from stable ids. Prefer registry-owned metadata if this
  taxonomy is now product behavior.
- If `PanelGroup` remains in Rust only as a compatibility shim, it must not appear in
  `config_json` or UI behavior. Tests should reflect the new contract.
- Preserve append-safe setting lookup by id.
- Avoid a migration that depends on row order.
- Keep hidden scheduler state hidden: `interaction.auto_roll_enabled` and
  `interaction.wave_enabled` remain shell-owned booleans unless the interaction plan
  explicitly changes that policy.

## Exit Gate

- `cd /home/adamg/fluid-simulation/app && cargo test --lib`
- `cd /home/adamg/fluid-simulation/app && cargo build --target wasm32-unknown-unknown`
- Browser capture against `http://localhost:5184/` with the settings panel open on at
  least three representative tabs: Scenario, Wall Fill, Profiler.
- Manual/browser assertion: panel is to the right of the main canvas on desktop, does not
  cover the canvas, tabs wrap, no General/Workspace/Advanced/Dev labels are visible, and
  Profiler still updates only while visible.
- Stored settings replay from `localStorage`; reset-class restored settings still trigger
  one reset; tab-level reset-to-default still clears persisted config for that tab.

## Discipline Rules

- Do not change simulation or render defaults just to make tab grouping easier.
- Do not bury active settings in an "internal" tab to recreate Advanced/Dev under another
  name.
- Keep the right-side layout usable on narrow screens; if it must overlay on mobile, the
  desktop acceptance still requires side-by-side layout.
- Update capture helper names carefully. Keep backward-compatible aliases if existing
  tests or docs still call `openWorkspace`/`selectWorkspaceTab`.

## Migration Notes

- Migrated the right-side settings panel, registry-derived tab model, Profiler tab
  behavior, settings helpers, and Mode/Control relationship into
  `docs/architecture/web-shell.md`.
- Migrated the functional tab metadata contract and removed the old panel-tier taxonomy
  from `docs/architecture/settings.md`.
- Migrated the camera/pointer bridge changes into `docs/architecture/app-shell.md`.
- Updated `docs/decisions/observability.md` so help copy and functional tabs are the
  durable schema decision.

## See Also

- `docs/architecture/web-shell.md`
- `docs/architecture/settings.md`
- `docs/architecture/app-shell.md`
- `docs/decisions/observability.md`
