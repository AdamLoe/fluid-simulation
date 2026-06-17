---
status:        active
owner:         codex
last_updated:  2026-06-17
okay_to_delete: false
long_lived:    false
owning_docs:
  - architecture/web-shell.md
  - architecture/settings.md
  - architecture/rendering.md
  - architecture/simulation.md
---

# Settings panel resize and tab organization

## Outcome

The settings panel should feel like a normal resizable app sidebar rather than a fixed
drawer. Desktop users can drag a visible grip on the divider between the settings
panel and the canvas to make the panel wider or narrower. The panel has a clear active
tab header, and the settings are split into smaller user-facing tabs/sections:
whitewater is separate, water-surface smoothing is separate, scene density belongs in
Scenario, auto-roll controls are grouped together, and wave controls are grouped
together.

The smoothing controls must also support a true off state. The preferred expression is
that smooth iterations can be set to `0`; if the renderer needs a separate boolean to
bypass smoothing safely, the UI still needs to make "off" explicit and keep `0`
semantics intuitive.

## Scope

In scope:

- Add a desktop resize handle/grip on the settings/canvas divider, with stable min/max
  panel widths and no canvas overlap. The grip should look like a conventional app
  splitter, for example a narrow rectangle with short vertical lines.
- Preserve the existing mobile overlay behavior unless the current layout needs a
  small fix to avoid collisions.
- Ensure the active tab has an obvious header in the settings body. If the current
  code already has a header but it is visually ineffective, fix the presentation rather
  than adding duplicate headings.
- Split registry tab metadata so Whitewater and Smoothing become their own settings
  destinations.
- Remove the visible `particles.count` advanced override. The supported user control
  is particle density, which belongs in the Scenario area.
- Group auto-roll strength/cadence under an Auto Roll category and wave strength/cadence
  under a Wave category. If the shell still does not render registry groups, either
  add lightweight section rendering for these cases or split them into dedicated tabs;
  keep the result easy to scan.
- Update settings/rendering/simulation docs for the new visible settings surface and
  any changed smoothing/particle-count semantics.

Out of scope:

- Rebuilding the settings shell with a frontend framework.
- Adding new physics, whitewater, smoothing, auto-roll, or wave algorithms.
- Changing scenario startup/reset behavior beyond what is necessary after removing the
  manual particle-count override; that belongs to
  `scenario-bootstrap-visual-readiness.md`.
- Removing the bottom Mode/Control launcher.

## Context routes

- `docs/architecture/web-shell.md` for the settings panel, tab body/header, desktop vs
  narrow-screen behavior, `window.__fluidShell` helpers, and capture expectations.
- `docs/architecture/settings.md` for registry-owned tab metadata, visible settings,
  apply classes, legacy ids, and particle density/count semantics.
- `docs/architecture/rendering.md` for Water-mode whitewater and smoothing behavior.
- `docs/architecture/simulation.md` for density-derived particle counts and seeded
  volume semantics.
- Code routes: `app/web/index.html`, `app/web/main.js`, `app/web/panels.js`,
  `app/crates/fluid-lab/src/settings/mod.rs`,
  `app/crates/fluid-lab/src/scene/mod.rs`, and the water renderer settings snapshot
  path under `app/crates/fluid-lab/src/gpu/`.

## Open assumptions

- The resize handle only needs to be a desktop affordance; narrow/mobile can continue
  using the current overlay model.
- The resized width may persist in localStorage if that fits the shell's current
  persistence pattern, but persistence is not required for the first acceptable pass.
- Removing `particles.count` means removing it from the visible durable settings
  surface. If a hidden compatibility path remains for old localStorage or URLs, Rust
  should own that behavior and docs should describe it as compatibility only.

## Acceptance / verification

- Desktop capture shows a visible drag handle between canvas and settings panel; drag
  changes panel width, the canvas resizes, and no controls overlap.
- Narrow/mobile capture still opens the panel cleanly without introducing the desktop
  splitter as a broken overlay control.
- Each tab body has a clear active header and stable control layout.
- Whitewater and Smoothing are independently reachable destinations, not buried in a
  mixed render tab.
- Smooth iterations accepts `0`, and Water mode still renders without validation errors
  with smoothing off.
- `particles.count` is absent from visible settings/export/share surfaces; particle
  density is visible under Scenario.
- Auto Roll strength/cadence and Wave strength/cadence each appear under their own
  visible category or tab.
- `cargo build --target wasm32-unknown-unknown` and `cargo test --lib` pass from
  `app/`.
- Visible changes are verified with `tools/capture.mjs` on desktop and narrow/mobile
  viewports.
- `architecture/web-shell.md`, `architecture/settings.md`,
  `architecture/rendering.md`, and `architecture/simulation.md` reflect the shipped
  state before this plan is marked shipped.

## Handoff notes

- This stream shares `settings/mod.rs` and `scene/mod.rs` with
  `scenario-bootstrap-visual-readiness.md`. If both are implemented in parallel, agree
  first that particle count is derived from density/fill/scene and that the visible
  absolute override is gone.
- Keep the thin-shell contract: the panel should continue rendering registry metadata
  instead of growing a hand-coded list of settings.
- Review CSS carefully after adding the splitter. The panel already affects canvas
  client width, so the implementation should rely on the existing resize path rather
  than manually scaling the WebGPU canvas.

## See also

- `docs/plans/index.md`
- `docs/plans/scenario-bootstrap-visual-readiness.md`
- `docs/architecture/web-shell.md`
- `docs/architecture/settings.md`
- `docs/architecture/rendering.md`
- `docs/architecture/simulation.md`
