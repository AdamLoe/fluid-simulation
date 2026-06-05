---
status:        active
owner:         adamg
last_updated:  2026-06-05
okay_to_delete: false
long_lived:    true
---

# Web shell & capture harness

The browser front-end is intentionally thin — no React, no framework — intentionally thin vanilla ES modules. Its job is to mount the WASM module, hand an `HTMLCanvasElement` to Rust, ferry pointer/keyboard input in, and render two observability panels driven entirely by the WASM JS bridge. A headless Puppeteer harness (`tools/capture.mjs`) runs real-GPU Chrome on the Windows host and provides the only acceptance signal that cannot be faked.

```
static.html  ──loads──▶  main.js  ──imports──▶  panels.js
                              │
                              └──▶  ./pkg/fluid_lab.js  (wasm-bindgen glue)
```

## What it owns

- Feature-detecting `navigator.gpu` and showing `#unsupported` if absent.
- Calling `FluidApp.create(canvas)` and exposing the result as `window.__fluid`.
- Owning `requestAnimationFrame`; calling `app.frame(dtMs)` each tick.
- Wiring header toolbar buttons (Pause / Reset / Mesh / Settings), four interaction modes (camera / rotate / rotateRoll / slosh), keyboard shortcuts (1–4 select modes, `r` resets the sim), pointer drag dispatch, and wheel zoom.
- URL params `?pressure=off`, `?paused=1`, `?flip=N`, `?slice=1`, `?mesh=1`, `?panels=off` for scripted capture.
- Calling `initPanels(app)` once after WASM init — panels own everything after that.

## Two entry paths (IMPORTANT gotcha)

There are two distinct HTML+JS stacks and they are **not equivalent**:

| | Verified / canonical | Stale / incomplete |
|---|---|---|
| HTML | `web/static.html` | `web/index.html` |
| JS | `web/main.js` + `web/panels.js` | `web/src/main.ts` (compiled by Vite) |
| Server | `python3 -m http.server 5184` (fixed port — see below) | `npm run dev` → strict port 5184 |
| Panels | Yes — config + profiler | No — panels absent |
| Interaction modes | 4 modes + keyboard shortcuts | Camera-only |
| Reliability | Verified working | npm/Vite unreliable on dev machine |

**Always use the static path.** The Vite/TS path (`web/src/main.ts`) is a Phase 0.1 stub that was superseded and never updated to include panels or the expanded interaction model. It still compiles but its UI is a strict subset. The `vite.config.ts` binds port 5184 with `strictPort: true`; if npm is unavailable the whole path fails.

**Port convention:** the static instance is served on the fixed port **5184** (procedure in [`../agent-context/build-run.md`](../agent-context/build-run.md)). That collides with the Vite path's `strictPort` 5184, so a stray process found on 5184 is usually a stale Vite dev server to kill before serving the static shell.

The capture harness defaults to `http://localhost:5173/` in its argv but should be pointed at the static server — `http://localhost:5184/static.html`.

## The rendered panels

`web/panels.js` → `initPanels` drives both side panels. Neither panel hardcodes any settings — they are built from WASM bridge calls:

- **Config panel (left):** calls `app.config_json()` once on init, groups settings by `category`, renders each as a slider+number-input (f32) or dropdown (enum). Changes call `app.set_setting(id, value)` and persist to `localStorage` under key `fluidlab.config.v1`. The `apply` field on each setting determines the dot color and badge:
  - `live` (green dot) — takes effect immediately in the running sim.
  - `reset` (amber dot + badge) — takes effect after `app.reset()`.
  - `reload` (red dot + badge) — requires a full page reload.
  - `scene.preset` (enum) auto-calls `app.reset()` on change so no manual step is needed.
  - On startup, stored settings are replayed; if any are `reset`-class, `app.reset()` is called once to materialize them.
  - Every control row carries a per-setting reset-to-default icon button (`.cfg-reset-btn`, ⟲) to the right of the slider/number (`buildSettingRow`) or dropdown (`buildEnumRow`). It calls `app.set_setting(id, s.default)` through the same `applyChange` path as a manual edit, so a reset-class reset shows the usual badge and a `scene.preset` reset auto-resets the sim. `config_json` supplies each setting's `default`.

- **Profiler panel (right):** polls `app.stats_json()` at 4 Hz via `setInterval(250)` and rebuilds its DOM each tick. Always-on rows: FPS, timing mode (`gpu-timestamp` vs CPU fallback), grid resolution, total/liquid cells, particle count, GPU buffer memory, frame avg / p50/p95/p99, substeps this frame, dropped sim time (per-frame and cumulative total), dispatches per frame and per substep. When `stats.gpu` is present: prep / pressure / finish / render pass times (frame totals); pressure flagged dominant with a progress bar. When `stats.gpu.sections` is present (detailed dev mode): per-section list (`FINE_SECTIONS`) and a CG block showing total/avg-per-iter, SpMV, reductions, updates, and scalars.

Both panels respect `?panels=off` and the header **Settings** toolbar button (`#btn-settings`). That button toggles both side panels together via `setPanelsVisible`; its label stays the constant "Settings" and it conveys open/closed state only through the `btn-active` class (it does not relabel itself).

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

- **Two separate control clusters.** The header `#toolbar` (chromeless — no background, no border) holds only the "Fluid Lab" title and the Pause / Reset / Mesh / Settings buttons; there is no Step button. The bottom-center `#mode-bar` holds the four interaction-mode buttons and is driven by `main.js` independently of the header. The 1–4 number-key mode shortcuts target the mode-bar, not the toolbar; `r`/`R` calls `app.reset()` (same as the toolbar Reset button). All keyboard shortcuts are ignored while an `INPUT`/`TEXTAREA` config field is focused.
- **`window.__fluid` is the control surface.** The capture harness and any EVAL snippets drive the sim through this handle. It is set in `web/main.js → main` after `FluidApp.create` succeeds and is absent if WASM init fails.
- **`#unsupported` starts `display:none`.** It is shown (via `showUnsupported`) only when `navigator.gpu` is absent or `FluidApp.create` throws. Both conditions are captured in the console log.
- **Canvas sizing uses DPR.** `sizeCanvas` multiplies `clientWidth/Height` by `devicePixelRatio`; a `ResizeObserver` re-applies on layout changes. This means the capture at 1280×800 with `deviceScaleFactor:1` produces a 1280×800 canvas.
- **Profiler panel fires on the JS frame timer, not GPU.** The `stats_json` timing field indicates whether real GPU timestamps are available (`gpu-timestamp`) or if the adapter fell back to CPU-side wall-clock estimates.
- **localStorage survives hard-reload but not origin clear.** If a `reload`-class setting was changed and the user clears storage, it silently reverts to the Rust-compiled default on next load.
- **Panels are absent from the Vite/TS path** — any screenshot taken via `npm run dev` will have no config or profiler panels, making it useless for observability verification.
- **The static path has no build step.** `web/pkg/` must already contain a freshly built `fluid_lab.js` + `fluid_lab_bg.wasm` (from `wasm-pack build --target web`). If `pkg/` is stale the page loads but runs old Rust logic.

## Update when

- A new WASM bridge method is added or removed (`config_json`, `stats_json`, `set_setting`, `FluidApp.*`) — update the panels section.
- The two-entry-path situation is resolved (static path becomes the sole path, or Vite path is brought to parity) — rewrite the entry-paths section.
- The capture harness gains new env hooks or output artefacts.
- A new interaction mode is added to `main.js` → `modeOrder`.

## See also

- `app-shell.md` — the WASM bridge (`FluidApp`, JS-exposed methods, config/profiler registry)
- `settings.md` — how settings are declared in Rust and surfaced via `config_json`
- `profiler.md` — GPU timestamp query machinery behind `stats_json`
- `../agent-context/build-run.md` — how to build WASM and serve the static path
- `../decisions/platform.md` — why no framework, why static-first
- `../agent-context/maintaining-docs.md` — ALWAYS read before editing any doc
