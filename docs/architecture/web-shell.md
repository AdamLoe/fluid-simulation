---
status:        active
owner:         adamg
last_updated:  2026-06-11
okay_to_delete: false
long_lived:    true
---

# Web shell & capture harness

The browser front-end is a thin vanilla-ES-module shell. It mounts the WASM module,
hands an `HTMLCanvasElement` to Rust, ferries input into exported methods, keeps small
UI shell state, and renders settings/profiler panels from the WASM bridge. The headless
capture harness (`tools/capture.mjs`) runs real-GPU Chrome on the Windows host and is
the visual acceptance path.

```
index.html  ──loads──▶  main.js  ──imports──▶  panels.js
                             │
                             └──▶  ./pkg/fluid_lab.js  (wasm-bindgen glue)
```

## What it owns

- Feature-detecting `navigator.gpu` and showing `#unsupported` if absent.
- Calling `FluidApp.create(canvas)` and exposing the result as `window.__fluid`.
- Owning `requestAnimationFrame`; calling `app.frame(dtMs)` each tick.
- Wiring the global toolbar (Pause / Reset / Settings), the right-side settings panel,
  bottom Mode and Control segmented controls, keyboard shortcuts, pointer dispatch, and
  wheel zoom.
- URL params `?pressure=off`, `?paused=1`, `?flip=N`, `?slice=1`, and `?slicemode=N`
  for scripted capture.
- Calling `initPanels(app)` once after WASM init; `panels.js` owns settings tabs,
  config row rendering, profiler rendering, persistence replay, and tab-level reset.
- Exposing `window.__fluidShell` helpers for capture scripting:
  `openSettings`, `selectSettingsTab`, `selectProductMode`, `selectControlTarget`,
  `reset`, and `state`. Backward-compatible `openWorkspace` / `selectWorkspaceTab`
  aliases remain for older scripts but are not the product vocabulary.

## Canonical shell

There is one live shell: `web/index.html` -> `web/main.js` -> `web/panels.js`.
Because the file is named `index.html`, the bare `/` serves it under any static server
with no path remap.

`web/src/main.ts` is an orphaned old Vite/TS entry and nothing loads it. `vite.config.ts`
still binds 5184 with `strictPort`, but `app/run.sh` serves the static path directly:
it rebuilds the WASM package, frees port 5184, and serves `web/` with no-cache headers.
Open the bare `http://localhost:5184/`.

## Settings panel

`web/panels.js -> initPanels` drives one right-side settings panel. On desktop it is a
layout column beside the canvas, not an overlay; on narrow screens it may overlay to
preserve viewport space. The panel starts closed and opens on the Scenario tab.

Config tabs are derived from registry-owned metadata in `app.config_json()`: every row
has `tab`, `tab_label`, and `tab_order` from
`crates/fluid-lab/src/settings/mod.rs -> settings_tab`. `panels.js` sorts those tabs,
appends a Profiler tab, and then renders rows by `category` inside the active config
tab. Profiler is not a config tab and is excluded from tab-level reset.

Rows support:

- slider + number input for numeric settings,
- dropdowns for `options` enum settings,
- native color pickers for `slider_scale: "color"`,
- logarithmic particle-count slider for `slider_scale: "log2"`,
- per-setting reset-to-default buttons,
- optional functional and technical help affordances.

Changes call `app.set_setting(id, value)` and persist non-default visible setting
overrides to `localStorage` under `fluidlab.config.v1`. Hidden scheduler booleans
(`interaction.auto_roll_enabled`, `interaction.wave_enabled`) are real registry
settings but are not rendered or persisted as user choices. The `apply` field drives
the row dot and reset/reload badge. Stored settings replay on startup; if any restored
setting is reset-class, the panel calls `app.reset()` once to materialize them.

Tab-level "Reset to Defaults" restores only the active config tab's rows, removes
default-valued rows from persisted overrides, and keeps reset/reload badges visible for
settings that still need the running app to reset or reload. `scene.preset` still
auto-calls `app.reset()` when changed directly; other reset-class scene settings, such as
`scene.drop_height`, show the normal reset badge.

The Modes tab exposes only product-mode tuning (auto-roll strength/cadence and wave
strength/frequency). Product Mode itself is owned by the bottom control in `main.js`.

The Profiler tab polls `app.stats_json()` at 4 Hz only while the panel is open on the
Profiler tab. It shows FPS, timing source, grid/particle scale, GPU memory, frame
percentiles, substep/drop accounting, pressure iterations, render mode, dispatch-shape
facts, and GPU cost rows when timestamp data is available.

## Bottom controls and pointer dispatch

The bottom launcher has two always-visible segmented controls:

- `Mode: Auto rotate / Waves / Manual`
- `Control: Camera / Cube`

`Mode` writes only the hidden scheduler booleans:
Auto rotate enables auto-roll, Waves enables the wave-maker, and Manual disables both.
`Control` is independent of Mode and chooses what pointer drags manipulate.

Pointer-button mapping in `web/main.js`:

| Control | Left drag | Right drag | Middle drag |
|---|---|---|---|
| Camera | `camera_orbit(dx,dy)` | `camera_twist(dx,dy)` | `camera_pan(dx,dy)` |
| Cube | `rotate_box(dx,dy)` | `rotate_box_roll(dx,dy)` | `move_box(dx,dy)` |

Right-click context menus are suppressed on the canvas. Wheel input always calls
`camera_zoom(deltaY)`. Number keys `1` and `2` select Camera/Cube when focus is not in
an input; `r`/`R` resets the simulation.

Reset preserves shell state by reassertion: `main.js -> resetSimulation` calls
`app.reset()`, reapplies the current Mode, and rerenders the open tab. Control is JS
state and remains selected across reset. A full reload returns to Auto rotate + Camera.

## Capture harness

`tools/capture.mjs` runs on Windows via
`node tools/capture.mjs <url> <out.png> [waitMs] [chromePath]`. It launches real-GPU
headless Chrome with WebGPU flags, navigates to the dev server, waits, then writes a PNG
and `<out>.console.txt`.

Environment hooks:

| Var | Effect |
|---|---|
| `EVAL=<js>` | Evaluates JS in the page and waits `EVAL_WAIT` ms after. |
| `PARTICLES=N` | Applies an exact requested particle count and resets for scale smoke. |
| `DETAILED=1` | Enables detailed GPU profiling before the scale reset. |
| `DRAG=1` | Drags the orbit camera across the canvas centre before screenshotting. |
| `FRAMES=N` | Captures N frames at `FRAME_INTERVAL` ms spacing. |
| `SEQ_RESET` | Calls `window.__fluid.reset()` before the frame sequence. |

The harness records `navigator.gpu` presence and final `stats_json`; if `hasGpu: false`
appears, the screenshot is the unsupported overlay and is not valid visual evidence.

## Gotchas

- The static shell depends on a fresh `web/pkg/fluid_lab.js` + `fluid_lab_bg.wasm` from
  `wasm-pack build --target web`; stale pkg files mean stale Rust exports.
- `window.__fluid` is the Rust control surface; `window.__fluidShell` is the shell
  control surface.
- Canvas sizing uses CSS client size multiplied by `devicePixelRatio`; resizing the
  settings panel changes the canvas client width and triggers `app.resize(...)`.
- Panels are absent from the orphaned Vite/TS path, so screenshots from that path are
  not valid panel evidence.

## Update when

- The WASM bridge, shell helper API, settings-tab contract, bottom controls, pointer
  mapping, capture harness hooks, or static serving path changes.

## See also

- `app-shell.md` — WASM bridge, camera, pointer methods, reset behavior.
- `settings.md` — registry metadata, `config_json`, apply classes.
- `profiler.md` — GPU timestamp machinery behind `stats_json`.
- `../agent-context/build-run.md` — build, serve, and capture commands.
- `../decisions/platform.md` — static-first shell rationale.
- `../agent-context/maintaining-docs.md` — doc maintenance rules.
