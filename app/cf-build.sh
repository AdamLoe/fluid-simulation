#!/usr/bin/env bash
# Cloudflare Pages production build.
#
# Compiles the fluid-lab crate to WASM (release) and assembles a CLEAN static
# deploy directory at app/web/dist containing only what the browser needs:
# the canonical shell (index.html, main.js, panels.js), the wasm-bindgen
# runtime (pkg/fluid_lab.js + fluid_lab_bg.wasm), and _headers. None of the
# repo's dev cruft (node_modules, the orphaned Vite src/, *.d.ts) ships.
#
# Cloudflare Pages settings:
#   Root directory:          (repo root — leave blank)
#   Build command:           bash app/cf-build.sh
#   Build output directory:  app/web/dist
#
# Also runnable locally to preview the exact production bundle:
#   bash app/cf-build.sh && python3 -m http.server -d app/web/dist 8080
set -euo pipefail

APP="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"   # app/
WEB="$APP/web"
PKG="$WEB/pkg"
OUT="$WEB/dist"

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

echo "==> Assembling deploy dir → $OUT"
rm -rf "$OUT"
mkdir -p "$OUT/pkg"
cp "$WEB/index.html" "$WEB/main.js" "$WEB/panels.js" "$WEB/_headers" "$OUT/"
cp "$PKG/fluid_lab.js" "$PKG/fluid_lab_bg.wasm" "$OUT/pkg/"

echo "==> Done — $(du -sh "$OUT" | cut -f1) in $OUT"
ls -la "$OUT" "$OUT/pkg"
