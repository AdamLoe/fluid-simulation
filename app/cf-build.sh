#!/usr/bin/env bash
# Cloudflare Pages deploy build.
#
# Assembles a CLEAN static deploy directory at app/web/dist containing only what
# the browser needs: the canonical shell (index.html, main.js, panels.js), the
# wasm-bindgen runtime (pkg/fluid_lab.js + fluid_lab_bg.wasm), and _headers.
# None of the repo's dev cruft (node_modules, the orphaned Vite src/, *.d.ts) ships.
#
# Two modes:
#   --prebuilt   Don't compile. Use the release pkg already committed at
#                app/web/pkg (built locally, see below). Cloudflare runs THIS —
#                deploys finish in seconds, no Rust toolchain on CI.
#   (default)    Compile the WASM (release) from source. Used locally to refresh
#                the committed pkg, or on CI if you'd rather Cloudflare compile.
#
# Cloudflare Pages settings (no-compile path):
#   Root directory:          (repo root — leave blank)
#   Build command:           bash app/cf-build.sh --prebuilt
#   Build output directory:  app/web/dist
#
# Local "I changed Rust, refresh what gets deployed" workflow:
#   bash app/cf-build.sh                 # release-compile → app/web/pkg
#   git add -f app/web/pkg/fluid_lab.js app/web/pkg/fluid_lab_bg.wasm
#   git commit && git push               # Cloudflare just copies it
#
# Preview the exact production bundle locally:
#   bash app/cf-build.sh && python3 -m http.server 5184 -d app/web/dist
set -euo pipefail

MODE="build"
[[ "${1:-}" == "--prebuilt" ]] && MODE="prebuilt"

APP="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"   # app/
WEB="$APP/web"
PKG="$WEB/pkg"
OUT="$WEB/dist"

if [[ "$MODE" == "prebuilt" ]]; then
  echo "==> --prebuilt: skipping WASM compile, using committed $PKG"
  if [[ ! -f "$PKG/fluid_lab_bg.wasm" || ! -f "$PKG/fluid_lab.js" ]]; then
    echo "ERROR: --prebuilt needs a committed release pkg, but $PKG is missing it." >&2
    echo "       Run 'bash app/cf-build.sh' locally, then commit:" >&2
    echo "       git add -f app/web/pkg/fluid_lab.js app/web/pkg/fluid_lab_bg.wasm" >&2
    exit 1
  fi
else
  # Rust: Cloudflare's build image ships no Rust toolchain, so bootstrap rustup on
  # CI (channel + wasm target come from rust-toolchain.toml). On a dev box cargo is
  # already on PATH and this block is skipped.
  if ! command -v cargo >/dev/null 2>&1; then
    echo "==> Installing Rust (rustup)…"
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | \
      sh -s -- -y --profile minimal --default-toolchain 1.95.0
    export PATH="$HOME/.cargo/bin:$PATH"
  fi

  # Ensure the wasm target is present (no-op if rust-toolchain.toml already added it).
  rustup target add wasm32-unknown-unknown 2>/dev/null || true

  # wasm-pack: prebuilt-binary installer (fast, reliable on CI). Needs Rust present,
  # so it runs after the rustup bootstrap above.
  if ! command -v wasm-pack >/dev/null 2>&1; then
    echo "==> Installing wasm-pack…"
    curl -sSf https://rustwasm.github.io/wasm-pack/installer/init.sh | sh
    export PATH="$HOME/.cargo/bin:$PATH"
  fi

  echo "==> Building WASM (release)…"
  ( cd "$APP" && wasm-pack build crates/fluid-lab --target web --out-dir "$PKG" --release )
fi

echo "==> Assembling deploy dir → $OUT"
rm -rf "$OUT"
mkdir -p "$OUT/pkg"
cp "$WEB/index.html" "$WEB/main.js" "$WEB/panels.js" "$WEB/_headers" "$OUT/"
cp "$PKG/fluid_lab.js" "$PKG/fluid_lab_bg.wasm" "$OUT/pkg/"

echo "==> Done — $(du -sh "$OUT" | cut -f1) in $OUT"
ls -la "$OUT" "$OUT/pkg"
