# Build & run

## When does this apply

You want to compile the WASM, serve the web shell, or browser-verify a change with the
capture harness. Inner-loop operations.

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

**`app/run.sh` is the one command.** It always does all three steps — rebuild the WASM,
free port 5184, serve the shell — so "restart the app" / "rebuild and run" is just:

```
cd /home/adamg/fluid-simulation/app && ./run.sh           # rebuild + free port + serve
cd /home/adamg/fluid-simulation/app && ./run.sh --clean   # cargo clean first, then the same
```

It runs in the **foreground** (Ctrl-C stops the server); re-run it any time to rebuild
and reserve. Then open the **bare** URL:

> **http://localhost:5184/**

The canonical shell is **`web/index.html`**, so the bare `/` serves it under any server
(it is the default directory index — no path remap). `run.sh` wraps `http.server` only to
add **no-cache headers**, so an ordinary browser reload always picks up the freshly built
`.wasm` — no Ctrl-Shift-R, and you never put a filename in the URL or start a server by
hand. Port **5184 is the one fixed web port**; `run.sh` frees it (killing any stale
`http.server` or Vite instance) before serving, so there is never a second instance to
manage.

Do **not** hand-run `python3 -m http.server` — that skips the rebuild, the port-free, and
the no-cache headers (it serves the right `index.html` at `/`, but caches the `.wasm`),
i.e. most of the reason `run.sh` exists.

## What run.sh does under the hood

You only need these when debugging the loop or running a single piece in isolation;
`run.sh` already chains all three. Source of truth: `app/run.sh`.

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

2. **Free port 5184:** `pkill -f 'http.server'; pkill -f vite; fuser -k 5184/tcp`.

3. **Serve `app/web/`** via the no-cache `python3 -c` handler (not a bare
   `python3 -m http.server`, which caches the `.wasm`). The bare `/` resolves to
   `web/index.html`, the canonical shell.

The stale path is now just the orphaned `web/src/main.ts` (the old Vite/TS entry): it
lacks the panels and nothing loads it — `index.html` imports `./main.js`, not
`src/main.ts`. `vite.config.ts` still binds 5184 with `strictPort`, but `run.sh` does not
use Vite. See [`../architecture/web-shell.md`](../architecture/web-shell.md).

## Browser-verify with the capture harness (real GPU)

The capture harness drives real **Windows** Chrome headless against the dev server and is
the one acceptance signal that can't be faked — it writes a screenshot **plus** the
page console. It must run under **Windows** node + Windows Chrome:

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
  node app/tools/capture.mjs http://localhost:5184/ out.png [waitMs] [chromePath]
  ```

Point the harness at the **bare** `http://localhost:5184/` (the same URL you open in a
browser); the bare `/` serves `web/index.html`. `run.sh` must already be
serving in another shell — the harness only drives the page, it does not build or serve.

It writes the PNG + a `<out>.console.txt` beside it. **A bare output filename (e.g.
`boot.png`) lands in the repo `captures/` dir** (gitignored), anchored to the script
location — so screenshots never pollute `app/tools/`, whatever cwd you launched from.
Pass a path with a directory to override. The 4th positional arg overrides the Chrome
path (default `C:/Program Files/Google/Chrome/Application/chrome.exe`). Env hooks:
`DRAG=1` exercises the orbit camera; `EVAL=...` runs a JS expression against
`window.__fluid`; `FRAMES` / `FRAME_INTERVAL` capture a sequence; `SEQ_RESET` exercises
repeated resets. A console line `hasGpu: false` means the screenshot is the
`#unsupported` overlay, not the sim — a healthy boot instead logs `navigator.gpu present:
true`, the smoke-test PASS, and `fluid init: n=64 …`.

## Deploy (Cloudflare Pages)

Production hosting is **Cloudflare Pages**, auto-building from the GitHub repo on every
push to the production branch. Cloudflare compiles the WASM itself (no committed
artifacts; `web/pkg/` stays gitignored).

- **`app/cf-build.sh`** is the production build. It installs `wasm-pack` if absent,
  builds the crate `--release`, and assembles a **clean** deploy dir at `app/web/dist`
  with only `index.html` + `main.js` + `panels.js` + `pkg/{fluid_lab.js,fluid_lab_bg.wasm}`
  + `_headers` — none of the dev cruft (`node_modules`, the orphaned Vite `src/`, `*.d.ts`).
  The release WASM is ~355 KB (wasm-opt'd); the whole bundle is ~480 KB. `app/web/dist`
  is gitignored (regenerated each build).
- **`app/rust-toolchain.toml`** pins the channel (1.95.0) + wasm target so Cloudflare's
  build image matches local dev. rustup reads it from any ancestor of the build cwd.
- **`app/web/_headers`** sets CSP `frame-ancestors` (allows `self` + `adamloe.com`) so the
  page embeds in an `<iframe>` on adamloe.com while the standalone `*.pages.dev` URL keeps
  working; `frame-ancestors` restricts framing only, not direct loads. Also `X-Content-Type-Options`
  and short cache for `pkg/`, `no-cache` for `index.html`.

**Cloudflare Pages dashboard settings** (Settings → Builds & deployments):

| Field | Value |
|---|---|
| Root directory | *(blank — repo root)* |
| Build command | `bash app/cf-build.sh` |
| Build output directory | `app/web/dist` |

No COOP/COEP cross-origin-isolation headers are needed (single-threaded WASM, no
`SharedArrayBuffer`). Cloudflare serves `.wasm` as `application/wasm` automatically, which
the wasm-bindgen `--target web` streaming init requires.

Preview the exact production bundle locally:

```
bash app/cf-build.sh && python3 -m http.server 5184 -d app/web/dist
```

(then browser-verify at the bare `http://localhost:5184/` as below). This is distinct from
`run.sh`, which serves the source `web/` dir with a **dev** WASM build for the inner loop.

## Toolchain (pinned)

wgpu 29 · wasm-pack 0.15 · rustc/cargo ~1.95 · node 20 (WSL) / 24 (Windows). Chrome at
`C:/Program Files/Google/Chrome/Application/chrome.exe`; Windows node at
`/mnt/c/Program Files/nodejs/node.exe`.

## What NOT to do

- Don't hand-run `python3 -m http.server`. Use `./run.sh` and the bare
  `http://localhost:5184/` — the manual path skips the rebuild, port-free, and no-cache
  headers.
- Don't run builds from a Windows shell without the `wsl.exe` wrapper (and conversely,
  don't add the wrapper when your shell is already inside WSL). `run.sh` itself must run
  inside WSL.
- Don't serve on an ad-hoc port — the web instance lives on **5184**.
- Don't verify against the Vite path; use the static path (`run.sh` serves it).
- Don't try to drive the capture harness with WSL node — it can't launch Windows Chrome.
- Don't make a performance claim without profiler output (see [`testing.md`](testing.md)).

## See also

- [`../architecture/web-shell.md`](../architecture/web-shell.md) — the two entry paths + capture harness.
- [`../architecture/gpu-resources.md`](../architecture/gpu-resources.md) — boot diagnostics/limits.
- [`testing.md`](testing.md) — host tests + acceptance honesty.
