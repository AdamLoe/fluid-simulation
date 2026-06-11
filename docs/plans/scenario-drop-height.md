---
status:        shipped
owner:         unassigned
last_updated:  2026-06-11
okay_to_delete: true
long_lived:    false
owning_docs:
  - architecture/app-shell.md
  - architecture/settings.md
  - decisions/scope.md
---

# Scenario Drop Height

## Mission

Add a Scenario settings control that lets the user choose the height water is dropped
from. Done means the setting appears with the scenario controls, persists like other
settings, requires or performs reset consistently with scene setup, and visibly changes
the initial suspended water height for the relevant presets.

## Scope

In scope:

- Add a reset-class setting, tentatively `scene.drop_height`, labeled `Drop height`.
- Place the setting in the Scenario tab created by the settings overhaul, or in the
  existing Scene/General surface if this ships first.
- Apply the setting in `SceneConfig::from_settings` when constructing initial liquid
  blocks.
- Preserve existing preset names and enum wire values.
- Keep blocks in normalized tank space and clamp them inside `[0, 1]` after applying the
  height.
- Add focused host tests for low/default/high drop heights.

Out of scope:

- Adding source/drain behavior or continuous water spawning.
- Adding moving solids or paddles.
- Replacing `LiquidBlock` with a new scene graph.
- Making the setting live without reset.

## Product Semantics

Use normalized tank height for the first implementation:

- `0.0` means near the floor.
- `1.0` means near the top of the tank.
- The default should reproduce the current shipped look.
- For suspended presets such as Falling Blob and Double Splash, shift the suspended blocks
  vertically while preserving their size and x/z footprint.
- For Dam Break, keep the floor-anchored water column behavior unless the lead explicitly
  wants drop height to re-anchor that preset too. The tooltip should explain any preset
  where the setting has limited effect.

The implementer may choose tighter min/max bounds if needed to prevent degenerate blocks,
but the UI should still communicate the control as a height, not an arbitrary normalized
coordinate.

## Approach

Owned streams:

| Stream | Area | Owned files |
|---|---|---|
| Registry | Add setting row, getter, tests, enum/tab metadata | `crates/fluid-lab/src/settings/mod.rs` |
| Scene config | Apply drop height to preset blocks during scene construction | `crates/fluid-lab/src/scene/mod.rs` |
| Panel | Ensure it appears in Scenario settings and reset-class behavior is clear | `web/panels.js`, settings taxonomy if already changed |
| Docs | Migrate scene-setting behavior | `architecture/app-shell.md`, `architecture/settings.md`, `decisions/scope.md` only if scenario policy changes |

Implementation notes:

- Prefer shifting a block by center height then clamping the full block to stay inside the
  tank, so the visual mass stays stable across heights.
- Preserve deterministic reset behavior.
- If the settings overhaul has not landed, keep this compatible with the current panel
  grouping and let that plan regroup it later.

## Exit Gate

- `cd /home/adamg/fluid-simulation/app && cargo test --lib`
- `cd /home/adamg/fluid-simulation/app && cargo build --target wasm32-unknown-unknown`
- Browser smoke:
  - Change drop height low/high.
  - Reset after each change.
  - Falling Blob and Double Splash start at visibly different heights.
  - Dam Break behavior matches the chosen semantics and tooltip.
- Verify persisted `scene.drop_height` restores after reload and materializes with one
  reset, like other reset-class scene settings.

## Discipline Rules

- Do not fake a falling-water source by creating particles during runtime.
- Do not change preset enum values.
- Do not change particle count, grid dimensions, or solver settings as part of this work.
- Do not make a reset-class setting appear live.

## Migration Notes

- Migrated `SceneConfig::from_settings` drop-height behavior and affected presets into
  `docs/architecture/app-shell.md`.
- Migrated the `scene.drop_height` reset-class Scenario setting into
  `docs/architecture/settings.md`.
- No `docs/decisions/scope.md` update was needed because this is a local reset-class
  scene parameter, not a broader scenario-system policy change.

## See Also

- `docs/architecture/app-shell.md`
- `docs/architecture/settings.md`
- `docs/decisions/scope.md`
