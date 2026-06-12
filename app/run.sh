#!/usr/bin/env bash
# Rebuild the WASM and serve the static web shell on the canonical port 5184.
# Foreground: Ctrl-C stops the server. Re-run to rebuild + reserve.
#
# Always does all three: rebuild WASM, kill any running server, serve.
#
#   ./run.sh            rebuild (dev profile) + kill + serve
#   ./run.sh --clean    cargo clean first, then rebuild + kill + serve
#
# Must run inside WSL (cargo/wasm-pack/python3 on PATH). See
# docs/agent-context/build-run.md.
set -euo pipefail

APP_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PORT=5184

# Tear down only the process that owns the canonical port. Safe to call when
# nothing is running.
free_port() {
  echo "==> Freeing port $PORT…"
  fuser -k "${PORT}/tcp" 2>/dev/null || true
  sleep 0.5
}

echo "==> Building WASM (wasm-pack, dev)…"
if [[ "${1:-}" == "--clean" ]]; then
  echo "    (cargo clean first)"
  (cd "$APP_DIR" && cargo clean)
fi
(cd "$APP_DIR" && wasm-pack build crates/fluid-lab --target web --out-dir ../../web/pkg --dev)

free_port

echo "==> Serving on http://localhost:${PORT}/  (Ctrl-C to stop)"
# The canonical shell is web/index.html, so the bare "/" serves it under any
# server (http.server's default directory index) — no path remap needed. We wrap
# http.server only to add no-cache headers, so an ordinary browser reload always
# picks up the freshly built .wasm (plain http.server caches it, requiring
# Ctrl-Shift-R).
cd "$APP_DIR/web" && exec python3 -c '
import sys, http.server
class Handler(http.server.SimpleHTTPRequestHandler):
    def end_headers(self):
        self.send_header("Cache-Control", "no-store, no-cache, must-revalidate, max-age=0")
        self.send_header("Pragma", "no-cache")
        self.send_header("Expires", "0")
        super().end_headers()
http.server.test(HandlerClass=Handler, port=int(sys.argv[1]), bind="0.0.0.0")
' "$PORT"
