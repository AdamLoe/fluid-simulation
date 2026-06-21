---
status:        active
owner:         adamg
last_updated:  2026-06-20
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
  `?pressure=off`, `?paused=1`, `?flip=N`, `?slice=1`, `?slicemode=N`, and
  `?dev=true`; `?flip=N` is only a fallback when no canonical
  `set=physics.flip_blend:N` entry is present.
- Exposing `window.__fluidShell` helpers for captures, including
  `openSettings`, `selectSettingsTab`, `selectProductMode`, `selectControlTarget`,
  `reset`, `applySettings`, `importConfigPayload`, `exportConfig`, `shareUrl`,
  `setting`, `setTheme`, `activeTheme`, and `state`; close/workspace/manual-pointer
  aliases remain available for compatibility. `reset` returns the underlying
  `FluidApp::reset` boolean.

## Canonical shell

The live shell is `web/index.html -> web/main.js -> web/panels.js`. `app/local_dev.sh`
rebuilds the dev wasm package into ignored `web/pkg-dev/`, frees port 5184, and serves
`web/` with no-cache headers. The shell import remains `./pkg/fluid_lab.js`; the local
server maps browser `/pkg/*` requests to `web/pkg-dev/*`. Open `http://localhost:5184/`.
Release packaging uses the tracked `web/pkg/{fluid_lab.js,fluid_lab_bg.wasm}` pair.

## Settings panel

`web/panels.js -> initPanels` drives one right-side settings panel. It starts closed
and opens from the toolbar settings icon. On desktop it is a layout column beside the
canvas with a visible splitter grip between canvas and panel; dragging the grip updates
`--panel-width`, persists the width in localStorage, and lets the existing canvas
`ResizeObserver` drive `FluidApp::resize`. On narrow screens the panel still overlays
to preserve viewport space and the splitter is hidden.

The panel uses `.settings-content` with a tab navigator plus `.settings-main`, which
contains a compact header and the active scroll body. The header names the active tab,
shows the setting count or shell/profiler state, and keeps the Live/Reset/Reload
apply-class legend visible. There is no `#settings-nav-toggle`, no `nav-collapsed`
class, and no navigator-collapse behavior. On desktop the navigator sits beside the
body; on narrow screens the tabs become a single-row horizontal scroller above the
body with a visible right-edge affordance. Opening or selecting a tab scrolls the
active tab into view. The tab strip is an ARIA `tablist`: tabs use roving focus,
ArrowLeft/ArrowRight/Home/End move and activate tabs, and Enter/Space activates the
focused tab. The toolbar settings button remains the open/close control.

Tabs are derived directly from registry metadata in `app.config_json()`, sorted by
`tab_order`, and followed by Profiler. Whitewater and Smoothing are ordinary
registry-owned tabs. The shell does not render registry tab groups.
`Environment` is hidden unless `?dev=true`; `Theme` is a shell-owned dev-only tab
that also appears only with `?dev=true`. Rows support slider+number controls,
dropdowns, color pickers, log2 sliders, color swatches, and per-setting reset buttons;
labels can wrap within the panel while controls keep stable value/reset affordances.
The shell renders only functional help affordances; registry `technical_tooltip`
metadata is not surfaced in the panel.

Changes call `app.set_setting_result_json(id, value)` and persist visible non-default
overrides to `localStorage` under `fluidlab.config.v1`. The result JSON reports
acceptance, clamping, stored value, apply class, and whether reset/reload is needed,
so sliders and number boxes reflect clamped stored values instead of assuming the
requested value stuck.

Hidden scheduler booleans and the compatibility-only `particles.count` override are
not rendered or persisted. Removed render ids are sent to the bridge during
restore/import; Rust owns legacy mapping/ignoring/rejection and reports structured
outcomes. Future saves walk visible non-default rows only, so removed ids and hidden
compatibility overrides disappear on the next save.

Portable config actions are no longer visible buttons in the panel. The capture/API
helpers remain: `exportConfig()` emits `{schema:"fluidlab.config.v1",
settings:{id:value}}` over visible non-default rows, `importConfigPayload()` applies
entries through the same bridge-backed batch path as URL and localStorage restore, and
`shareUrl()` returns a URL containing repeated `set` params and strips the legacy
`flip` param from the returned URL.

Shareable registry settings use repeated `set` URL params:

```
?set=physics.flip_blend:0.65&set=grid.res_x:32
```

The shell parses all `set` entries once at boot, appends legacy `?flip=N` as
`physics.flip_blend` only when no canonical `set=physics.flip_blend:N` entry is
present, applies the batch after default product-mode initialization, and triggers one
`app.reset()` if any accepted entry reports `needs_reset`. LocalStorage
restore happens inside `initPanels` before product-mode and URL settings, and the rAF
loop starts only after those synchronous reset-class batches finish, so the first
meaningful rendered frame uses the restored scenario, fill, density, grid, and derived
particle count. On narrow first loads with no explicit or stored `camera.distance`,
the shell applies a one-time live camera zoom-out before the first frame; it does not
mutate the registry value and therefore is not exported or shared. Reload-class entries
are stored and warned about; the shell does not auto-reload the page.
The old `pressure`, `paused`, `slice`, and `slicemode` params remain ad hoc shell
controls for this stage.

`window.__fluidShell.state().urlApplyResult` retains the boot `set` batch summary for
browser smoke checks. `window.__fluidShell.setting(id)` returns the current registry
row from `config_json()`, and the shell methods above expose the same import/export
and share URL behavior without needing to click the panel.

The Profiler tab polls `app.stats_json()` at 4 Hz while open. It starts with a compact
summary of FPS, real-time factor, timing source, and scale status, then groups the
remaining timing, memory, scale, GPU status, and liveness rows. Persistent foam
particle rows were removed with `DiffuseSystem`.

The Theme tab is shell-owned rather than registry-owned. The preset catalog lives in
`web/panels.js → THEMES`, with matching CSS variable blocks in `web/index.html`; the
set is intentionally broad enough for manual visual testing and includes `void`, whose
app background is true `#000000`. The top-level CSS variables own shell backgrounds,
surfaces, text hierarchy, borders/dividers, hover states, action/accent treatments,
status colors, focus rings, shadows, radii, spacing, and stable control dimensions.
Component rules should consume those tokens instead of reintroducing local color
literals.

Each theme option previews six semantic swatches from named variables:
`--app-bg`, `--text-body`, `--accent`, `--button-bg`, `--control-bg`, and
`--panel-border`. The selected id is written to `localStorage` under
`fluidlab.theme.v1` and applied by setting `data-theme` on the root. The choice is
included in `window.__fluidShell.state().theme` and can be changed through
`window.__fluidShell.setTheme(id)`.

GPU platform status is exposed through `app.gpu_device_status()` and mirrored in
`window.__fluidShell.state().gpuDeviceStatus` and `stats_json.gpu_device_status`.
Current values are `ok`, `surface-lost`, `device-lost`, and
`surface-validation-error`. `surface-lost` is transient: the Rust side recreates
swapchain-sized targets and continues. `device-lost` and
`surface-validation-error` stop the shell frame loop and show the existing WebGPU
overlay with reload guidance. The unsupported/error overlay is an alert dialog, moves
focus to itself, and makes the underlying app inert/hidden to assistive technology
while shown; the app does not attempt in-place WebGPU device recovery. Generic WebGPU
validation console messages remain capture-console evidence unless wgpu exposes them
as surface validation or device loss.

The toolbar, launcher, and canvas start busy/disabled before WASM creation and panel
binding. The shell enables buttons and gives the canvas a focusable `tabindex` only
after app creation, panel initialization, URL/localStorage replay, shell helper
binding, and event-handler registration complete.

## Bottom controls and pointer dispatch

The bottom launcher is raised above the viewport edge with a safe-area-aware inset and
has two always-visible segmented controls:

- `Mode: Auto rotate / Waves / Manual`
- `Control: Camera / Cube`

Mode writes hidden scheduler booleans. Control chooses what pointer drags manipulate.
On narrow screens the launcher remains visible and wraps to full-width groups.

The canvas itself is focusable after boot. Pointer drags and keyboard commands dispatch
through the same bridge methods: arrows orbit/rotate, Shift+arrows pan/move,
Alt+arrows twist/roll, and PageUp/PageDown or +/- zoom the camera. The selected
Control target chooses whether arrow commands act on Camera or Cube, matching pointer
drag mode.

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
`device-lost` or `surface-validation-error`, or the boot smoke test reports failure.
If `navigator.gpu` is false, the screenshot is the unsupported overlay and is not
valid visual evidence.

Every run also writes `<out>.trace.ndjson` and `<out>.stats.json`, including the raw
final `stats_json` and occupied-cell drift proxy. When `MEASURE_WAIT` is set, ordinary
captures and scale/detailed measurement captures both poll during that window, so the
trace normally contains multiple samples before the final row. Opt-in assertion env
vars can fail the run on timing source, frame/GPU budgets, scale status, or missing
GPU stats; GPU sim/render budget assertions are valid only when the final sample is
`gpu-timestamp`. `FLUID_ASSERT_TEST_STATS='<json>'` runs those assertion checks against
provided stats and exits before launching Chrome.

## Gotchas

- Static release serving depends on fresh `web/pkg/fluid_lab.js` and
  `fluid_lab_bg.wasm`; local dev serves the same `/pkg/*` URL space from `web/pkg-dev/`.
- `window.__fluid` is the Rust control surface; `window.__fluidShell` is the shell
  control surface.
- Resizing the settings panel changes the canvas client width and triggers
  `app.resize(...)`; the splitter itself is a desktop-only affordance.
- The old Vite/TS path is not valid panel evidence.

## Update when

- The WASM bridge, shell helper API, settings-tab contract, bottom controls, pointer
  mapping, capture harness hooks, or static serving path changes.

## See also

- [`settings.md`](settings.md)
- [`profiler.md`](profiler.md)
- [`../decisions/platform.md`](../decisions/platform.md)
- [`../agent-context/build-run.md`](../agent-context/build-run.md)
- [`~/.agentdocs/rules/authoring-rules.md`](~/.agentdocs/rules/authoring-rules.md)
