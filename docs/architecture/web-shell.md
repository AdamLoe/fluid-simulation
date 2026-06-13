---
status:        active
owner:         adamg
last_updated:  2026-06-12
okay_to_delete: false
long_lived:    true
---

# Web shell & capture harness

The browser front-end is a thin vanilla-ES-module shell. It mounts the WASM module,
hands an `HTMLCanvasElement` to Rust, ferries input into exported methods, keeps small
UI shell state, and renders settings/profiler panels from the WASM bridge. The
headless capture harness (`tools/capture.mjs`) runs real-GPU Chrome on the Windows
host and is the visual acceptance path.

```
index.html -> main.js -> panels.js
                    -> ./pkg/fluid_lab.js
```

## What it owns

- Feature-detecting `navigator.gpu`.
- Calling `FluidApp.create(canvas)` and exposing `window.__fluid`.
- Owning `requestAnimationFrame` and calling `app.frame(dtMs)`.
- Wiring Pause, Reset, Settings, the right-side settings panel, bottom Mode/Control
  segmented controls, keyboard shortcuts, pointer dispatch, and wheel zoom.
- URL params `?set=id:value` (repeatable, registry-backed), plus legacy shell params
  `?pressure=off`, `?paused=1`, `?flip=N`, `?slice=1`, and `?slicemode=N`.
- Exposing `window.__fluidShell` helpers for captures:
  `openSettings`, `selectSettingsTab`, `selectProductMode`, `selectControlTarget`,
  `reset`, `applySettings`, `importConfigPayload`, `exportConfig`, `shareUrl`,
  `setting`, and `state`, plus backward-compatible workspace aliases. `reset` returns
  the underlying `FluidApp::reset` boolean.

## Canonical shell

The live shell is `web/index.html -> web/main.js -> web/panels.js`. `app/run.sh`
rebuilds the wasm package, frees port 5184, and serves `web/` with no-cache headers.
Open `http://localhost:5184/`.

## Settings panel

`web/panels.js -> initPanels` drives one right-side settings panel. It starts closed
and opens from the toolbar settings icon. On desktop it is a layout column beside the
canvas; on narrow screens it overlays to preserve viewport space.

The panel contains only `.settings-content`: the settings navigator and active settings
body. There is no separate header band, no `#settings-nav-toggle`, no
`nav-collapsed` class, and no navigator-collapse behavior. On desktop the navigator
sits beside the body; on narrow screens the tabs wrap above the body. The toolbar
settings button remains the open/close control.

Tabs are derived from registry metadata in `app.config_json()`, grouped by
`tab_group`, sorted by `tab_order`, and followed by a Profiler tab. Rows support
slider+number controls, dropdowns, color pickers, log2 sliders (the color swatches),
per-setting reset buttons, and help affordances. The Scenario tab also renders a
read-only "Effective scenario" summary at its top (grid resolution, total cells, and
the resolved particle count from `stats_json`), so the derived `particles.density`
count is visible without opening the Profiler.

Changes call `app.set_setting_result_json(id, value)` and persist visible non-default
overrides to `localStorage` under `fluidlab.config.v1`. The result JSON reports
acceptance, clamping, stored value, apply class, and whether reset/reload is needed,
so sliders and number boxes reflect clamped stored values instead of assuming the
requested value stuck.

Hidden scheduler booleans are not rendered or persisted. Removed render ids are sent
to the bridge during restore/import; Rust owns legacy mapping/ignoring and reports
`legacy_mapped` or `legacy_ignored`. Future saves walk visible non-default rows only,
so removed ids disappear on the next save.

Each config tab ends with compact portability actions: copy a share URL, export JSON,
and import JSON. Export emits `{schema:"fluidlab.config.v1", settings:{id:value}}`
over visible non-default rows. File import also accepts the older raw settings map,
applies every entry through the same bridge-backed batch path as URL and
localStorage restore, persists the resulting visible non-default settings, logs the
structured outcomes, and shows a small applied/rejected/clamped/reset/reload summary.

Shareable registry settings use repeated `set` URL params:

```
?set=physics.flip_blend:0.65&set=grid.res_x:32
```

The shell parses all `set` entries once at boot, appends legacy `?flip=N` as
`physics.flip_blend`, applies the batch after default product-mode initialization,
and triggers one `app.reset()` if any accepted entry reports `needs_reset`. Reload
class entries are stored and warned about; the shell does not auto-reload the page.
The old `pressure`, `paused`, `slice`, and `slicemode` params remain ad hoc shell
controls for this stage.

`window.__fluidShell.state().urlApplyResult` retains the boot `set` batch summary for
browser smoke checks. `window.__fluidShell.setting(id)` returns the current registry
row from `config_json()`, and the shell methods above expose the same import/export
and share URL behavior without needing to click the panel.

The Profiler tab polls `app.stats_json()` at 4 Hz while open. It reports foam
particles/emitted/clamped only; legacy JSON keys for spray and bubble may be present
as zeroes but are not shown as visible feature counts.

GPU platform status is exposed through `app.gpu_device_status()` and mirrored in
`window.__fluidShell.state().gpuDeviceStatus`. Current values are `ok`,
`surface-lost`, `device-lost`, and `validation-error`. `surface-lost` is transient:
the Rust side recreates swapchain-sized targets and continues. `device-lost` and
`validation-error` stop the shell frame loop and show the existing WebGPU overlay
with reload guidance; the app does not attempt in-place WebGPU device recovery.

## Bottom controls and pointer dispatch

The bottom launcher has two always-visible segmented controls:

- `Mode: Auto rotate / Waves / Manual`
- `Control: Camera / Cube`

Mode writes hidden scheduler booleans. Control chooses what pointer drags manipulate.
On narrow screens the launcher remains visible and wraps to full-width groups.

## Capture harness

`tools/capture.mjs` runs on Windows via:

```
node tools/capture.mjs <url> <out.png> [waitMs] [chromePath] [evalJs] [viewportWidth] [viewportHeight]
```

It launches real-GPU headless Chrome with WebGPU flags, records console output and
page errors, writes a PNG plus `<out>.console.txt`, records `navigator.gpu`, and
records final `stats_json` plus final shell state.

Useful environment hooks: `EVAL`, `EVAL_WAIT`, `VIEWPORT_WIDTH`,
`VIEWPORT_HEIGHT`, `PARTICLES`, `DETAILED`, `DRAG`, `FRAMES`, `FRAME_INTERVAL`,
and `SEQ_RESET`.

The harness exits non-zero when WebGPU is unavailable, `stats_json` is missing, page
errors/request failures occur, requested reset setup is rejected, console output
reports WebGPU validation/device-loss failures, final shell `gpuDeviceStatus` is
`device-lost` or `validation-error`, or the boot smoke test reports failure. If
`navigator.gpu` is false, the screenshot is the unsupported overlay and is not valid
visual evidence.

Every run also writes `<out>.trace.ndjson` and `<out>.stats.json`, including the raw
final `stats_json` and occupied-cell drift proxy. When `MEASURE_WAIT` is set, ordinary
captures and scale/detailed measurement captures both poll during that window, so the
trace normally contains multiple samples before the final row. Opt-in assertion env
vars can fail the run on timing source, frame/GPU budgets, scale status, or missing
GPU stats; GPU sim/render budget assertions are valid only when the final sample is
`gpu-timestamp`. `FLUID_ASSERT_TEST_STATS='<json>'` runs those assertion checks against
provided stats and exits before launching Chrome.

## Gotchas

- Static serving depends on fresh `web/pkg/fluid_lab.js` and `fluid_lab_bg.wasm`.
- `window.__fluid` is the Rust control surface; `window.__fluidShell` is the shell
  control surface.
- Resizing the settings panel changes the canvas client width and triggers
  `app.resize(...)`.
- The old Vite/TS path is not valid panel evidence.

## Update when

- The WASM bridge, shell helper API, settings-tab contract, bottom controls, pointer
  mapping, capture harness hooks, or static serving path changes.

## See also

- `settings.md`
- `profiler.md`
- `../agent-context/build-run.md`
