# Build & run

## When does this apply

You want to compile the WASM, serve the web shell, or browser-verify a change with the
capture harness. Inner-loop operations only. For release packaging and Cloudflare Pages
deploy, see [`deploy.md`](deploy.md).

## Which shell am I in? (read first)

The repo lives in WSL at `/home/adamg/fluid-simulation`. **All Rust / npm / wasm-pack /
serve commands must execute inside WSL** — but how you invoke them depends on where your
shell already is:

- **Shell already inside WSL** (`uname -a` shows `Linux … microsoft-standard-WSL2`, the
  default in this environment): run the commands **directly, no wrapper**. `cargo`,
  `wasm-pack`, `python3` are on `PATH` (`~/.cargo/bin`). This is the common case here.
- **Shell is Windows** (Git Bash / PowerShell over the `\\wsl.localhost\Ubuntu-24.04\`
  share): wrap every command:

  ```
  wsl.exe -d Ubuntu-24.04 -- bash -lc '<command>'
  ```

  Shell variable assignment gets mangled across the Windows→wsl.exe→bash layers — use
  full literal paths inline, never `VAR=...; $VAR`.

The command blocks below are written **bare** (for an in-WSL shell). If your shell is on
the Windows side, wrap each one in `wsl.exe -d Ubuntu-24.04 -- bash -lc '…'`.

`app/` is the Cargo workspace root (`app/Cargo.toml`). The Rust crate lives at
`app/crates/fluid-lab/` (manifest + `src/` source); the web shell is `app/web/`;
the capture harness is `app/tools/`. Build commands run from `app/`.

## Run the app (the canonical loop)

**`app/local_dev.sh` is the one command.** It always does all three steps — rebuild the WASM,
free port 5184, serve the shell — so "restart the app" / "rebuild and run" is just:

```
cd /home/adamg/fluid-simulation/app && ./local_dev.sh           # rebuild + free port + serve
cd /home/adamg/fluid-simulation/app && ./local_dev.sh --clean   # cargo clean first, then the same
```

It runs in the **foreground** (Ctrl-C stops the server); re-run it any time to rebuild
and reserve. Then open the **bare** URL:

> **http://localhost:5184/**

The canonical shell is **`web/index.html`**, so the bare `/` serves it under any server
(it is the default directory index — no path remap). `local_dev.sh` wraps `http.server` only to
add **no-cache headers**, so an ordinary browser reload always picks up the freshly built
`.wasm` — no Ctrl-Shift-R, and you never put a filename in the URL or start a server by
hand. Port **5184 is the one fixed web port**; `local_dev.sh` frees only listeners on that
port before serving, so there is never a second instance to manage without killing
unrelated development servers.

Do **not** hand-run `python3 -m http.server` — that skips the rebuild, the port-free, and
the no-cache headers (it serves the right `index.html` at `/`, but caches the `.wasm`),
i.e. most of the reason `local_dev.sh` exists.

## What local_dev.sh does under the hood

You only need these when debugging the loop or running a single piece in isolation;
`local_dev.sh` already chains all three. Source of truth: `app/local_dev.sh`.

1. **Rebuild the WASM:**

   ```
   cd /home/adamg/fluid-simulation/app && wasm-pack build crates/fluid-lab --target web --out-dir ../../web/pkg --dev
   ```

   `--out-dir` is relative to the crate dir, so `../../web/pkg` resolves to
   `app/web/pkg/`; `wasm-pack` writes the glue + `fluid_lab_bg.wasm` there, which
   `web/main.js` imports as a normal ES module. A clean build is ~35s. Quick
   compile-only check (no bindgen):

   ```
   cd /home/adamg/fluid-simulation/app && cargo build --target wasm32-unknown-unknown
   ```

2. **Free port 5184:** `fuser -k 5184/tcp` if something is listening there.

3. **Serve `app/web/`** via the no-cache `python3 -c` handler (not a bare
   `python3 -m http.server`, which caches the `.wasm`). The bare `/` resolves to
   `web/index.html`, the canonical shell.

The stale path is now just the orphaned `web/src/main.ts` (the old Vite/TS entry): it
lacks the panels and nothing loads it — `index.html` imports `./main.js`, not
`src/main.ts`. `vite.config.ts` still binds 5184 with `strictPort`, but `local_dev.sh` does not
use Vite. See [`../architecture/web-shell.md`](../architecture/web-shell.md).

## Browser-verify with the capture harness (real GPU)

The capture harness drives real **Windows** Chrome headless against the dev server and is
the one acceptance signal that can't be faked — it writes a screenshot **plus** the
page console and exits non-zero for missing WebGPU, page/request errors, missing
stats, rejected requested reset setup, or boot smoke-test failure. It must run under
**Windows** node + Windows Chrome:

- **WSL node cannot launch Windows Chrome** — the puppeteer process pipe doesn't cross
  the OS boundary (it fails with "Failed to launch the browser process").
- From a **WSL shell**, invoke Windows node (`/mnt/c/Program Files/nodejs/node.exe`, v24)
  over the `\\wsl.localhost\` share via `cmd.exe`. `localhost:5184` is reachable from
  Windows (WSL2 forwards localhost), and the script's default Windows Chrome path works
  when launched by Windows node:

  ```
  cd /home/adamg/fluid-simulation/app/tools && cmd.exe /c 'pushd \\wsl.localhost\Ubuntu-24.04\home\adamg\fluid-simulation\app\tools && node capture.mjs http://localhost:5184/ boot.png 3500 & popd'
  ```

- If your shell is already on the **Windows** side, run it plainly:

  ```
  node app/tools/capture.mjs http://localhost:5184/ out.png [waitMs] [chromePath] [evalJs] [viewportWidth] [viewportHeight]
  ```

Point the harness at the **bare** `http://localhost:5184/` (the same URL you open in a
browser); the bare `/` serves `web/index.html`. `local_dev.sh` must already be
serving in another shell — the harness only drives the page, it does not build or serve.

It writes the PNG + a `<out>.console.txt` beside it. **A bare output filename (e.g.
`boot.png`) lands in the repo `captures/` dir** (gitignored), anchored to the script
location — so screenshots never pollute `app/tools/`, whatever cwd you launched from.
Pass a path with a directory to override. The optional `chromePath` arg overrides the
default `C:/Program Files/Google/Chrome/Application/chrome.exe`; pass `""` to keep the
default while providing later args. The optional `evalJs` arg is the CLI equivalent of
`EVAL` and is useful when cross-shell env quoting is brittle. The optional viewport args
override the default `1280x800` viewport. Env hooks:
`PARTICLES=N` applies an exact integer requested particle count and resets; invalid
values or rejected resets fail the capture. `DETAILED=1` enables detailed GPU
profiling before that reset; `MEASURE_WAIT=ms` controls the sample window and is
polled into `<out>.trace.ndjson` for ordinary captures as well as scale/detailed
measurement captures. A final summary is written to
`<out>.stats.json` with the raw final `stats_json` and the occupied-cell drift proxy.
`DRAG=1` exercises the orbit camera; `EVAL=...` runs a JS
expression against `window.__fluid`; `FRAMES` / `FRAME_INTERVAL` capture a sequence;
`SEQ_RESET` exercises repeated resets. Every run records a final machine-readable
`stats_json` line. A console line `hasGpu: false` means the screenshot is the
`#unsupported` overlay, not the sim — a healthy boot instead logs `navigator.gpu present:
true`, the smoke-test PASS, and `fluid init: n=64 …`.

Opt-in assertion env vars:
`FLUID_ASSERT_MIN_TIMING_SOURCE=cpu-wallclock|coarse-fence|gpu-timestamp`,
`FLUID_ASSERT_MAX_FRAME_AVG_MS=N`, `FLUID_ASSERT_MAX_P95_MS=N`,
`FLUID_ASSERT_MAX_GPU_SIM_MS=N`, `FLUID_ASSERT_MAX_GPU_RENDER_MS=N`,
`FLUID_ASSERT_SCALE_STATUS_OK=1`, `FLUID_ASSERT_REQUIRE_GPU_STATS=1`, and
`FLUID_ASSERT_REQUIRE_GPU_TIMESTAMP=1`. GPU sim/render budget assertions require
`stats.timing === "gpu-timestamp"` and non-null `stats.gpu`; otherwise the harness
fails honestly instead of applying those budgets to CPU fallback timing.
To test assertion failure behavior without launching Chrome, set
`FLUID_ASSERT_TEST_STATS='<stats-json>'` with the assertion env vars; the script exits
after checking the supplied object.

## Toolchain (pinned)

wgpu 29 · wasm-pack 0.15 · rustc/cargo ~1.95 · node 20 (WSL) / 24 (Windows). Chrome at
`C:/Program Files/Google/Chrome/Application/chrome.exe`; Windows node at
`/mnt/c/Program Files/nodejs/node.exe`.

## What NOT to do

- Don't hand-run `python3 -m http.server`. Use `./local_dev.sh` and the bare
  `http://localhost:5184/` — the manual path skips the rebuild, port-free, and no-cache
  headers.
- Don't run builds from a Windows shell without the `wsl.exe` wrapper (and conversely,
  don't add the wrapper when your shell is already inside WSL). `local_dev.sh` itself must run
  inside WSL.
- Don't serve on an ad-hoc port — the web instance lives on **5184**.
- Don't verify against the Vite path; use the static path (`local_dev.sh` serves it).
- Don't try to drive the capture harness with WSL node — it can't launch Windows Chrome.
- Don't make a performance claim without profiler output (see [`testing.md`](testing.md)).

## See also

- [`../architecture/web-shell.md`](../architecture/web-shell.md) — the two entry paths + capture harness.
- [`deploy.md`](deploy.md) — release packaging and Cloudflare Pages.
- [`../architecture/gpu-resources.md`](../architecture/gpu-resources.md) — boot diagnostics/limits.
- [`testing.md`](testing.md) — host tests + acceptance honesty.
