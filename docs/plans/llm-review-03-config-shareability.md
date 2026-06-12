---
status:        active
owner:         codex
last_updated:  2026-06-12
okay_to_delete: false
long_lived:    false
owning_docs:
  - architecture/settings.md
  - architecture/web-shell.md
  - decisions/observability.md
  - decisions/scope.md
---

# LLM Review 03 - Config Shareability

## Mission

Make the settings bridge honest and shareable: setting writes report whether they were
applied, clamped, or require reset/reload, and URL parameters route through the typed
registry instead of an ad hoc parallel channel.

## Scope

In scope:

- WASM bridge return shape for `set_setting` or a compatible companion method.
- UI handling for applied/reset/reload/clamped outcomes.
- Registry-backed URL setting import.
- A shareable URL/export affordance if it can use existing non-default config data.
- Legacy-id sunset policy in docs.

Out of scope:

- Replacing the whole panel framework.
- Removing compatibility ids before the policy is documented.
- Cloud persistence or account-level presets.

## Approach

1. Audit current registry setters, clamping behavior, bridge ABI limits, localStorage
   save format, and URL parsing.
2. Add an internal Rust mutation result that can distinguish applied, stored reset,
   stored reload, unknown id, non-finite, legacy ignored/mapped, requested value,
   stored value, clamp status, and apply class.
3. Expose the result through a new JS-friendly WASM method while keeping the old
   boolean wrapper until callers are migrated.
4. Route generic `?set=id:value` URL entries through the registry validation path.
5. Add shareable URL/export behavior using the same source as persisted non-default
   settings.
6. Centralize legacy replay policy so JS submits stored/imported IDs to the bridge
   instead of mirroring a drifting compatibility list.
7. Document legacy-id retention and removal criteria.

## Subagents

- Read-only audit: settings/web explorer.
- Worker: settings bridge and web implementation. This worker may touch
  `app/crates/fluid-lab/src/settings/`, `app/crates/fluid-lab/src/lib.rs`,
  `app/web/main.js`, `app/web/panels.js`, `app/web/index.html`, and owning docs only.

## Audit Notes

- `FluidApp::set_setting(id, value)` returns `bool`; `false` conflates non-finite,
  unknown id, Reset/Reload-class stored values, and rejected paths.
- `Registry::set_value_f64` rejects non-finite values but silently clamps finite
  out-of-range values and reports only whether the id was found.
- Current URL params are hard-coded (`pressure`, `paused`, `flip`, `slice`,
  `slicemode`); `flip` bypasses the registry through `set_flip_blend`.
- Persistence is localStorage-only under `fluidlab.config.v1`, saving visible
  non-default rows.
- Legacy replay policy is split between Rust and JS. Known drift: direct bridge calls
  accept `render.particle_alpha`, but JS restore skips it unless the policy changes.
- Enum labels in `config_json` are positional; URL/import contracts should use stable
  slugs or explicit values.

## Exit Gate

- `cd app && cargo test --lib`
- `cd app && cargo build --target wasm32-unknown-unknown`
- Capture or browser smoke with URL-provided settings and one clamped value.

## Migration Notes

Stage 1 migration (2026-06-12):

- Bridge/result surface and registry URL shape -> `architecture/settings.md`.
- Web-shell URL workflow and legacy ad hoc compatibility -> `architecture/web-shell.md`.
- Honesty/clamping policy -> `decisions/observability.md`.
- Preset/export/import UI scope deferral -> `decisions/scope.md`.

Stage 1 implemented:

- `Registry::set_value_f64_result` reports accepted/stored/rejected/legacy outcomes,
  requested value, stored value, clamp status, and apply class. The old
  `set_value_f64` bool wrapper remains.
- `FluidApp::set_setting_result_json` exposes the mutation result to JS while
  preserving `set_setting(id, value) -> bool` for old callers.
- `web/main.js` accepts repeated `?set=id:value` params and routes legacy `?flip=N`
  through `physics.flip_blend`; `pressure`, `paused`, `slice`, and `slicemode` remain
  compatible shell-only params.
- URL and localStorage registry imports apply all entries first, then trigger one
  reset if any accepted setting needs reset. Reload-class settings are stored and
  warned, not auto-reloaded.
- JS localStorage restore no longer mirrors the Rust legacy-id list, fixing
  `render.particle_alpha` replay drift by letting the bridge report
  `legacy_ignored`.

Deferred:

- Visible preset/export/import controls and named preset management.
- Sunset/removal of existing legacy ad hoc URL params.
- Stable enum slug import/export; current URL/import values are numeric registry
  values.
