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

# UI shell and settings simplification

## Mission

Make the default app UI smaller, closer to the viewport edges, and less text-heavy.
The main canvas should feel like the primary surface, with pause/reset/settings/lag and
the mode/control launcher taking less space. The settings panel should expose fewer
layers of navigation, clearer tab names, less helper copy, and no portability actions.
Done means the default page is visibly more compact, the settings tabs are flatter and
easier to scan, and the settings surface no longer presents controls that the user asked
to hide or remove.

## Scope

In scope:

- Shrink the Fluid lag, pause, restart, and settings controls and move them closer to
  their viewport edges.
- Keep icon buttons small while increasing the icon-to-button-size ratio so the icons
  remain legible.
- Pin the Mode/Control launcher to the bottom edge, remove its bottom border, remove its
  bottom corner radius, and make it more compact.
- Remove Setup/Core/category grouping in the settings navigator so tab selectors render
  directly with simpler separators and minimal per-tab padding/radius.
- Rename or split confusing tab categories so names map to user concepts instead of
  mixed labels such as "Render & Water" or "Render & Camera".
- Merge the current Mode and Scenario settings into one Scenario-oriented tab. The user
  does not want the runtime bottom Mode control removed as part of this request.
- Remove the Effective scenario summary area from the top of the Scenario tab.
- Remove helper text in modes and cut back most technical tooltips. Keep tooltips only
  where a user genuinely needs clarification to make a choice.
- Split reflection and refraction settings into their own tabs.
- Hide the Environment tab unless the URL contains `dev=true`.
- Remove Copy share URL, Export JSON, and Import JSON from the visible settings panel.
- Change the default camera pitch to `-0.3`.

Out of scope:

- Removing foam, micronormals, or flat-water internals; that belongs to
  `render-feature-removals.md`.
- Adding the theme tab or theme variable system; that belongs to
  `dev-theme-system.md`.
- Redesigning the profiler or changing simulation behavior beyond the camera-pitch
  default and settings/tab ownership requested here.

## Approach

One implementer can own this as a web-shell/settings-registry stream. Start from the
existing vanilla shell path (`web/index.html`, `web/main.js`, `web/panels.js`) and the
registry tab metadata in `crates/fluid-lab/src/settings/mod.rs`.

The implementation should preserve the thin-shell model documented in
`architecture/web-shell.md`: the panel is still rendered from `config_json()`, settings
mutations still pass through `set_setting_result_json`, and hidden scheduler settings
remain non-durable. If tab routing changes require registry metadata changes, keep the
mapping centralized in `settings_tab` rather than hard-coding special cases in the
panel.

The tooltip pass should be editorial, not cosmetic: remove technical explanations and
leave only concise labels/affordances for ambiguous controls. Avoid adding visible
instructional text in the UI to replace removed tooltips.

The `dev=true` gate should be a shared shell concept used here for the Environment tab
and by `dev-theme-system.md` for the Theme tab.

## Exit gate

- Browser smoke on the static app path shows the compact main controls and bottom
  launcher at desktop and narrow/mobile viewports.
- Settings panel opens, tab navigation works, Environment is hidden by default, and
  Environment appears with `?dev=true`.
- Scenario contains the former Mode settings, the old separate Mode settings tab is
  gone, and the bottom runtime Mode control is still present.
- Copy share URL, Export JSON, and Import JSON are absent from visible tab bodies and
  shell helper compatibility is reviewed before removal or retention.
- Camera pitch defaults to `-0.3` on a clean load/reset path.
- `cargo build --target wasm32-unknown-unknown` and `cargo test --lib` pass from
  `app/`; for visible changes, capture evidence is produced with `tools/capture.mjs`.
- Update `architecture/web-shell.md` and `architecture/settings.md` for the shipped
  tab, tooltip, dev-gate, and control-surface changes.

## Handoff notes

- The repo may already contain unrelated modified/deleted files. Do not revert them.
- Coordinate the `dev=true` helper shape with `dev-theme-system.md` so the shell does
  not grow two different dev-mode checks.
- If removing share/export/import also changes `window.__fluidShell` helper methods,
  document the helper API change in `architecture/web-shell.md`; otherwise note that
  helpers remain capture-only compatibility surfaces.

## Migration notes (filled in at ship time)

- Current-state UI and shell facts go to `architecture/web-shell.md`.
- Registry tab metadata, visible setting ownership, tooltip fields, and default changes
  go to `architecture/settings.md`.

## See also

- `docs/plans/index.md`
- `docs/architecture/web-shell.md`
- `docs/architecture/settings.md`
- `docs/plans/dev-theme-system.md`
- `docs/plans/render-feature-removals.md`
