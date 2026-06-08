---
status:        abandoned
owner:         adamg
last_updated:  2026-06-08
okay_to_delete: true
long_lived:    false
owning_docs:
  - architecture/web-shell.md
  - architecture/app-shell.md
  - architecture/settings.md
  - decisions/scope.md
---

# Bottom mode launcher

## Superseded

This source draft has been folded into
[`v1.7.0-ui-shell-reorganization.md`](v1.7.0-ui-shell-reorganization.md). Use the
versioned plan for implementation. This file remains only as source context until the
next plan cleanup.

## Mission

Replace the always-visible four-button pointer-mode bar with a simpler first choice:
Auto Rotate, Waves, or Manual. Done means the first-run bottom controls present those
three product modes, Manual reveals the existing pointer-mode buttons, and choosing
Auto Rotate or Waves forces the pointer mode back to camera orbit so the user can
watch or inspect without accidentally dragging the tank.

## Scope

In scope:

- Add the initial bottom launcher with three choices:
  - Auto Rotate
  - Waves
  - Manual
- Treat those choices as mutually exclusive product modes. The app is in Auto Rotate
  mode, Waves mode, or Manual mode, never a combined bottom-mode state.
- In Manual, show the current pointer-mode controls: camera, rotate, rotate/roll, and
  slosh. Manual disables the scheduled Auto Rotate/Waves bottom modes.
- In Auto Rotate and Waves, set the pointer mode to camera/orbit and keep it there
  while that product mode is active. Auto Rotate disables Waves; Waves disables Auto
  Rotate.
- Connect Auto Rotate and Waves to the existing Live interaction settings rather than
  inventing new physics.
- Keep number-key shortcuts coherent after the UI changes.

Out of scope:

- Changing the Rust auto-roll or wave-maker scheduling model.
- Adding source/drain.
- Adding new wave patterns or auto-roll randomness beyond what exists.
- Reworking the top config workspace; Modes settings still live there after the tab
  plan.

## Approach

This is a product-state layer above the existing pointer modes. Keep the Rust model as
the owner of scheduled interactions, and let JavaScript choose which existing settings
are enabled.

Auto Rotate, Waves, and Manual are mutually exclusive states. The UI should make the
selected state obvious and should route the existing `interaction.*` settings so only
the selected scheduled behavior is enabled.

Manual should reveal the existing low-level pointer controls rather than deleting
them. The user still needs direct rotate/roll/slosh for hands-on inspection.

## High-level questions

- Should Auto Rotate/Waves use current config values from the Modes tab, or should the
  bottom buttons apply named presets for strength/frequency?
- Should number keys select the three product modes first, or keep targeting the
  manual pointer modes only after Manual is open?
- Should Reset preserve the selected product mode, or return to the initial launcher
  state?

## Exit gate

- Initial bottom UI shows Auto Rotate, Waves, and Manual.
- Manual reveals the current four pointer-mode buttons.
- Auto Rotate, Waves, and Manual are mutually exclusive; switching modes disables the
  other bottom-mode behaviors.
- Auto Rotate and Waves force camera/orbit pointer mode while active.
- Existing interaction settings remain the source of truth for auto-roll and wave
  behavior.
- Keyboard shortcuts are documented in `web-shell.md` after the new behavior is chosen.
- A browser capture verifies the bottom controls do not overlap the header or config
  workspace at desktop and narrow widths.

## Discipline rules

- Do not hide manual camera orbit. It remains the safe pointer mode.
- Do not create a parallel interaction scheduler in JavaScript.
- Do not use the bottom controls to mask physics or wall-contact problems.

## Migration notes (filled in at ship time)

- Update `architecture/web-shell.md` with the launcher state model, keyboard behavior,
  and which controls are visible in each state.
- Update `architecture/app-shell.md` only if Rust interaction scheduling semantics
  change.
- Update `architecture/settings.md` if the bottom buttons add or rename
  `interaction.*` settings.
- Update `decisions/scope.md` if the launcher changes product scope around automatic
  interactions.

## See also

- `architecture/web-shell.md`
- `architecture/app-shell.md`
- `architecture/settings.md`
- `v1.6.0-interactive-forces.md`
