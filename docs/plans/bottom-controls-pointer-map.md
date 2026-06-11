---
status:        shipped
owner:         unassigned
last_updated:  2026-06-11
okay_to_delete: true
long_lived:    false
owning_docs:
  - architecture/web-shell.md
  - architecture/app-shell.md
  - architecture/settings.md
---

# Bottom Controls And Pointer Map

## Mission

Replace the current bottom launcher/manual-pointer split with two always-visible
segmented controls:

`Mode: Auto rotate / Waves / Manual`

`Control: Camera / Cube`

Done means mode selection controls the automatic interaction scheduler, control selection
chooses what pointer gestures manipulate, and mouse buttons have a consistent meaning:
left drag rotates, right drag performs the alternate rotation, and middle drag moves.

## Scope

In scope:

- Replace the current product-mode strip plus Manual-only pointer-mode strip with the two
  requested controls.
- Keep product modes: Auto rotate, Waves, Manual.
- Replace manual pointer modes Camera / Rotate / Roll / Slosh with Control: Camera / Cube.
- Make Control independent from Mode. Users should not have to enter Manual just to orbit
  the camera or adjust the cube.
- Suppress the browser context menu on right-click over the canvas.
- Keep wheel zoom for the camera.
- Keep keyboard shortcuts only if they still map cleanly; otherwise remove or remap them
  and update docs.

Out of scope:

- Changing auto-roll or wave-maker physics.
- Adding a new slosh product mode.
- Reworking settings tabs beyond whatever labels need to stay in sync with the settings
  overhaul.

## Pointer Semantics

Required mapping:

| Control | Left drag | Right drag | Middle drag |
|---|---|---|---|
| Camera | Primary camera rotate/orbit | Alternate camera rotate, preferably roll/twist around view direction | Camera move/pan in screen plane |
| Cube | `rotate_box(dx, dy)` | `rotate_box_roll(dx, dy)` | `move_box(dx, dy)` |

Implementation notes:

- The Cube mapping can reuse existing Rust exports for rotate, alternate rotate, and
  move.
- The Camera mapping currently has only `camera_orbit(dx, dy)` plus wheel zoom. If there
  is no existing camera roll/pan export at implementation time, add small WASM methods
  rather than silently making right or middle drag a no-op.
- Do not route middle drag to `slosh_box`; the requested control is move-around, and the
  current product Mode handles wave/auto behavior separately.
- Track pointer button state from `PointerEvent.button`/`buttons`, not from the old
  selected manual mode.

## Approach

Owned streams:

| Stream | Area | Owned files |
|---|---|---|
| Shell state | Replace `manualPointerMode` with `controlTarget`, keep `productMode` | `web/main.js` |
| DOM/CSS | Render two labeled segmented controls and active/ARIA states | `web/index.html` |
| Pointer dispatch | Dispatch by selected Control plus pressed mouse button | `web/main.js` |
| WASM bridge | Add camera alternate-rotate/pan methods if missing | `crates/fluid-lab/src/lib.rs`, `crates/fluid-lab/src/camera.rs` |
| Docs | Migrate final control model | `architecture/web-shell.md`, `architecture/app-shell.md`, `architecture/settings.md` if hidden scheduler state changes |

Shell behavior:

- `Mode` writes the hidden scheduler booleans the same way the current product mode does:
  Auto rotate enables auto-roll only, Waves enables wave-maker only, Manual disables both.
- `Control` never writes scheduler booleans.
- Reset should preserve current Mode and Control by reasserting them after `app.reset()`.
- Full reload should keep the current product default policy unless the lead asks for a
  new default.

## Exit Gate

- `cd /home/adamg/fluid-simulation/app && cargo test --lib`
- `cd /home/adamg/fluid-simulation/app && cargo build --target wasm32-unknown-unknown`
- Browser smoke at `http://localhost:5184/`:
  - Auto rotate, Waves, and Manual visibly select and update scheduler state.
  - Control Camera and Cube visibly select independently of Mode.
  - Left/right/middle drags perform the expected action for Camera and Cube.
  - Right-click drag does not open the browser context menu.
  - Reset preserves Mode and Control.

## Discipline Rules

- Do not keep the old Rotate / Roll / Slosh buttons as hidden product UI.
- Do not make Control disappear outside Manual mode.
- Do not change interaction setting defaults unless the mode scheduler needs a real
  default migration.
- Avoid adding browser-specific button handling that breaks trackpads or pointer capture.

## Migration Notes

- Migrated the two bottom segmented controls, pointer-button mapping, keyboard shortcut
  policy, reset behavior, and shell helpers into `docs/architecture/web-shell.md`.
- Migrated the new camera WASM methods and pointer-method semantics into
  `docs/architecture/app-shell.md`.
- Migrated the hidden scheduler-state and visible Modes-tab behavior into
  `docs/architecture/settings.md`.

## See Also

- `docs/architecture/web-shell.md`
- `docs/architecture/app-shell.md`
- `docs/architecture/settings.md`
