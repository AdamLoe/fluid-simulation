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

## Restart the app (the canonical loop)

When the instruction is "restart the app," do all three, in order:

1. **Rebuild the WASM from the Rust** (see [Build the WASM](#build-the-wasm)). Add
   `cargo clean` first only when a genuinely-clean rebuild is wanted.
2. **Tear down every running instance** — all static servers and any stale Vite dev
   server, on any port:

   ```
   pkill -f 'http.server'; pkill -f vite; fuser -k 5184/tcp 2>/dev/null; true
   ```

3. **Serve the static shell on port 5184** (see [Serve](#serve-the-web-shell)).

Port **5184 is the one fixed web port** — always serve the instance there and reuse it.
If a process is already on 5184, stop it first (step 2 frees it). Note: 5184 is also the
Vite path's `strictPort`, so a leftover the loop finds on 5184 is usually a stale Vite
dev server — kill it; the canonical instance is the **static** shell on 5184.

## Build the WASM

```
cd /home/adamg/fluid-simulation/app && wasm-pack build crates/fluid-lab --target web --out-dir ../../web/pkg --dev
```

`--out-dir` is relative to the crate dir, so `../../web/pkg` resolves to `app/web/pkg/`.
`wasm-pack` writes the glue + `fluid_lab_bg.wasm` there, which `web/main.js` imports as a
normal ES module. A clean build is ~35s.

For a from-scratch build, prepend `cargo clean &&`.

Quick compile-only check (no bindgen):

```
cd /home/adamg/fluid-simulation/app && cargo build --target wasm32-unknown-unknown
```

## Serve the web shell

The verified shell is the no-bundler static path. Serve `app/web/` on the fixed port
**5184** and open `static.html`:

```
cd /home/adamg/fluid-simulation/app/web && python3 -m http.server 5184
```

Then open `http://localhost:5184/static.html`. The Vite/TS path
(`web/index.html` + `web/src/main.ts`, which also binds 5184 with `strictPort`) is a
stale stub and lacks the panels — do not verify against it. See
[`../architecture/web-shell.md`](../architecture/web-shell.md).

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
  cd /home/adamg/fluid-simulation/app/tools && cmd.exe /c 'pushd \\wsl.localhost\Ubuntu-24.04\home\adamg\fluid-simulation\app\tools && node capture.mjs http://localhost:5184/static.html boot.png 3500 & popd'
  ```

- If your shell is already on the **Windows** side, run it plainly:

  ```
  node app/tools/capture.mjs http://localhost:5184/static.html out.png [waitMs] [chromePath]
  ```

It writes `out.png` + `out.png.console.txt`. The 4th positional arg overrides the Chrome
path (default `C:/Program Files/Google/Chrome/Application/chrome.exe`). Env hooks:
`DRAG=1` exercises the orbit camera; `EVAL=...` runs a JS expression against
`window.__fluid`; `FRAMES` / `FRAME_INTERVAL` capture a sequence; `SEQ_RESET` exercises
repeated resets. A console line `hasGpu: false` means the screenshot is the
`#unsupported` overlay, not the sim — a healthy boot instead logs `navigator.gpu present:
true`, the smoke-test PASS, and `fluid init: n=64 …`.

## Toolchain (pinned)

wgpu 29 · wasm-pack 0.15 · rustc/cargo ~1.95 · node 20 (WSL) / 24 (Windows). Chrome at
`C:/Program Files/Google/Chrome/Application/chrome.exe`; Windows node at
`/mnt/c/Program Files/nodejs/node.exe`.

## What NOT to do

- Don't run builds from a Windows shell without the `wsl.exe` wrapper (and conversely,
  don't add the wrapper when your shell is already inside WSL).
- Don't serve on an ad-hoc port — the web instance lives on **5184**.
- Don't verify against the Vite path; use the static path.
- Don't try to drive the capture harness with WSL node — it can't launch Windows Chrome.
- Don't make a performance claim without profiler output (see [`testing.md`](testing.md)).

## See also

- [`../architecture/web-shell.md`](../architecture/web-shell.md) — the two entry paths + capture harness.
- [`../architecture/gpu-resources.md`](../architecture/gpu-resources.md) — boot diagnostics/limits.
- [`testing.md`](testing.md) — host tests + acceptance honesty.
</content>
</invoke>
