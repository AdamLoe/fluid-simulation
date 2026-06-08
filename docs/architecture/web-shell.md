---
status:        active
owner:         adamg
last_updated:  2026-06-07
okay_to_delete: false
long_lived:    true
---

# Web shell & capture harness

The browser front-end is a thin vanilla-ES-module shell. Its job is to mount the WASM
module, hand an `HTMLCanvasElement` to Rust, ferry pointer/keyboard input in, and
render two observability panels driven entirely by the WASM JS bridge. A headless
Puppeteer harness (`tools/capture.mjs`) runs real-GPU Chrome on the Windows host and
provides the visible acceptance signal.

```
index.html  ──loads──▶  main.js  ──imports──▶  panels.js
                             │
                             └──▶  ./pkg/fluid_lab.js  (wasm-bindgen glue)
```

## What it owns

- Feature-detecting `navigator.gpu` and showing `#unsupported` if absent.
- Calling `FluidApp.create(canvas)` and exposing the result as `window.__fluid`.
- Owning `requestAnimationFrame`; calling `app.frame(dtMs)` each tick.
- Wiring header toolbar buttons (Pause / Reset / Config / Profiler), four interaction modes (camera / rotate / rotateRoll / slosh), keyboard shortcuts (1–4 select modes, `r` resets the sim), pointer drag dispatch, and wheel zoom.
- URL params `?pressure=off`, `?paused=1`, `?flip=N`, `?slice=1`, `?slicemode=N`, `?panels=on` for scripted capture (panels start closed; `?panels=on` opens both on load).
- The capture harness has explicit `PARTICLES`, `DETAILED`, and `MEASURE_WAIT`
  environment hooks for fresh-browser scale rows and always records final
  machine-readable `stats_json`; use this instead of shell-quoted `EVAL` for the
  performance matrix.
- Calling `initPanels(app)` once after WASM init — panels own everything after that.

## The canonical shell and one orphaned stub

There is one shell: `web/index.html` -> `web/main.js` -> `web/panels.js`. Because the
canonical file is named `index.html`, the bare `/` serves it under any static server
with no path remap and no second filename to type.

What remains stale is the orphaned **`web/src/main.ts`** (the old Vite/TS entry, a Phase 0.1 subset with no panels). **Nothing loads it** — `index.html` imports `./main.js`, not `src/main.ts`. `vite.config.ts` still binds 5184 with `strictPort: true`, but `run.sh` does not use Vite; treat `src/main.ts` as dead until it is brought to parity or deleted.

**Port convention:** the shell is served on the fixed port **5184** by `app/run.sh` (procedure in [`../agent-context/build-run.md`](../agent-context/build-run.md)), which rebuilds the WASM, frees the port, and serves `web/` with no-cache headers. That port collides with the orphaned Vite path's `strictPort` 5184, so a stray process found on 5184 is usually a stale Vite dev server — `run.sh` kills it before serving. Open `http://localhost:5184/`; you never reference a filename in the URL.

The capture harness defaults to `http://localhost:5173/` in its argv but should be pointed at the static server — the bare `http://localhost:5184/`.

## The rendered panels

`web/panels.js -> initPanels` drives both side panels. Neither panel hardcodes setting
metadata; the config panel is built from `app.config_json()` and the profiler panel is
built from `app.stats_json()`.

**Config panel (left):** settings are grouped by `panel_group` first, then by semantic
`category`. The default group's core simulation sections render open first.
`advanced` and `dev` render as collapsed `details` drawers before the default
presentation sections; when closed they share a compact row, and when opened they span
the panel width. Expert controls remain discoverable without dominating the scan path.
Ordinary numeric settings use slider + number input,
enum settings use dropdowns, `slider_scale: "log2"` uses an exponent-space particle
count slider, and `slider_scale: "color"` uses a native color picker for packed RGB.

Rows render help only when the registry supplies help. No `tooltip` and no
`technical_tooltip` means no help affordance. `tooltip` renders a functional `?`
affordance. `technical_tooltip` renders an adjacent, visually distinct technical `T`
affordance and uses a distinct tooltip treatment. Both affordances show the shared
instant hover/focus tooltip instead of relying on the native delayed `title` bubble.

Changes call `app.set_setting(id, value)` and persist to `localStorage` under
`fluidlab.config.v1`. The `apply` field drives the row dot and the reset/reload badge:
`live` applies immediately, `reset` takes effect after `app.reset()`, and `reload`
requires a page reload. `scene.preset` auto-calls `app.reset()` after selection. Stored
settings replay on startup; if any restored setting is reset-class, the panel calls
`app.reset()` once to materialize the restored state. Every row has a per-setting
reset-to-default button that reuses the same apply path as manual edits.

**Profiler panel (right):** polls `app.stats_json()` at 4 Hz via `setInterval(250)` and
rebuilds its DOM each tick. It shows FPS, timing source, grid/particle scale, GPU memory,
frame percentiles, substep/drop accounting, pressure iterations, render mode, and sorted
GPU cost rows when timestamp data is available. Detailed dev mode adds per-section and
CG timing rows.

Both panels **start closed**; `?panels=on` opens both on load. Each panel has its own header toolbar button — **Config** (`#btn-config`) and **Profiler** (`#btn-profiler`) — toggled independently via `setConfigVisible` / `setProfVisible`. The buttons keep constant labels and convey open/closed state only through the `btn-active` class (they do not relabel themselves).

## Capture harness

`tools/capture.mjs` runs on Windows via `node tools/capture.mjs <url> <out.png> [waitMs] [chromePath]`. It launches real-GPU headless Chrome with `--enable-unsafe-webgpu --enable-features=Vulkan`, navigates to the dev server, waits `waitMs` ms (default 6 s) for the simulation to warm up, then screenshots.

Outputs: `<out.png>` (screenshot) and `<out>.console.txt` (all console/pageerror/requestfailed lines including Rust boot diagnostics).

Env-var hooks for scripted scenarios:

| Var | Effect |
|---|---|
| `EVAL=<js>` | Evaluates a JS snippet in the page (e.g. `window.__fluid.reset()`); waits `EVAL_WAIT` ms after. |
| `DRAG=1` | Drags the orbit camera across the canvas centre before screenshotting. |
| `FRAMES=N` | Captures N frames at `FRAME_INTERVAL` ms spacing into `<out.png>.frames/`. |
| `SEQ_RESET` | Calls `window.__fluid.reset()` before the frame sequence. |

The harness reports `navigator.gpu` presence in the console log — if `hasGpu: false` appears, the screenshot shows the `#unsupported` overlay, not the simulation, and is worthless as evidence.

## Non-obvious invariants and gotchas

- **Two separate control clusters.** The header `#toolbar` (chromeless — no background, no border) holds the "Fluid Lab" title and Pause / Reset / Config / Profiler buttons; there is no Step button or surface-mode toggle. The bottom-center `#mode-bar` holds the four interaction-mode buttons and is driven by `main.js` independently of the header. The 1–4 number-key mode shortcuts target the mode-bar, not the toolbar; `r`/`R` calls `app.reset()` (same as the toolbar Reset button). All keyboard shortcuts are ignored while an `INPUT`/`TEXTAREA` config field is focused.
- **`window.__fluid` is the control surface.** The capture harness and any EVAL snippets drive the sim through this handle. It is set in `web/main.js → main` after `FluidApp.create` succeeds and is absent if WASM init fails.
- **`#unsupported` starts `display:none`.** It is shown (via `showUnsupported`) only when `navigator.gpu` is absent or `FluidApp.create` throws. Both conditions are captured in the console log.
- **Canvas sizing uses DPR.** `sizeCanvas` multiplies `clientWidth/Height` by `devicePixelRatio`; a `ResizeObserver` re-applies on layout changes. This means the capture at 1280×800 with `deviceScaleFactor:1` produces a 1280×800 canvas.
- **Profiler panel fires on the JS frame timer, not GPU.** The `stats_json` timing field indicates whether real GPU timestamps are available (`gpu-timestamp`) or if the adapter fell back to CPU-side wall-clock estimates.
- **localStorage survives hard-reload but not origin clear.** If a `reload`-class setting was changed and the user clears storage, it silently reverts to the Rust-compiled default on next load.
- **Panels are absent from the Vite/TS path** — any screenshot taken via `npm run dev` will have no config or profiler panels, making it useless for observability verification.
- **The static path has no build step.** `web/pkg/` must already contain a freshly built `fluid_lab.js` + `fluid_lab_bg.wasm` (from `wasm-pack build --target web`). If `pkg/` is stale the page loads but runs old Rust logic.

## Update when

- A new WASM bridge method is added or removed (`config_json`, `stats_json`, `set_setting`, `FluidApp.*`) — update the panels section.
- The orphaned `web/src/main.ts` is deleted or brought to parity (panels + interaction model) — update the canonical-shell section.
- The capture harness gains new env hooks or output artefacts.
- A new interaction mode is added to `main.js` → `modeOrder`.

## See also

- `app-shell.md` — the WASM bridge (`FluidApp`, JS-exposed methods, config/profiler registry)
- `settings.md` — how settings are declared in Rust and surfaced via `config_json`
- `profiler.md` — GPU timestamp query machinery behind `stats_json`
- `../agent-context/build-run.md` — how to build WASM and serve the static path
- `../decisions/platform.md` — why no framework, why static-first
- `../agent-context/maintaining-docs.md` — ALWAYS read before editing any doc
