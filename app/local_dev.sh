#!/usr/bin/env bash
# Rebuild the dev WASM package and serve the static web shell on port 5184.
# Foreground: Ctrl-C stops the server. Re-run to rebuild + reserve.
#
# Always does all three: rebuild WASM, kill any running server, serve.
#
#   ./local_dev.sh            rebuild (dev profile) + kill + serve
#   ./local_dev.sh --clean    cargo clean first, then rebuild + kill + serve
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

echo "==> Building WASM (wasm-pack, dev → web/pkg-dev)…"
if [[ "${1:-}" == "--clean" ]]; then
  echo "    (cargo clean first)"
  (cd "$APP_DIR" && cargo clean)
fi
(cd "$APP_DIR" && wasm-pack build crates/fluid-lab --target web --out-dir ../../web/pkg-dev --dev)

free_port

echo "==> Serving on http://localhost:${PORT}/  (Ctrl-C to stop)"
# The canonical shell imports ./pkg/fluid_lab.js. In local dev, /pkg/* is served
# from ignored web/pkg-dev so dev builds do not overwrite the tracked release pkg.
# The wrapper also adds no-cache headers, so an ordinary browser reload picks up
# the freshly built .wasm.
cd "$APP_DIR/web" && exec python3 -c '
import os, posixpath, sys, urllib.parse, http.server
PKG_DEV = os.path.join(os.getcwd(), "pkg-dev")
class Handler(http.server.SimpleHTTPRequestHandler):
    def translate_path(self, path):
        url_path = urllib.parse.urlparse(path).path
        if url_path == "/pkg" or url_path.startswith("/pkg/"):
            rel = posixpath.normpath(urllib.parse.unquote(url_path[len("/pkg"):]).lstrip("/"))
            parts = [part for part in rel.split("/") if part and part not in (os.curdir, os.pardir)]
            return os.path.join(PKG_DEV, *parts)
        return super().translate_path(path)

    def end_headers(self):
        self.send_header("Cache-Control", "no-store, no-cache, must-revalidate, max-age=0")
        self.send_header("Pragma", "no-cache")
        self.send_header("Expires", "0")
        super().end_headers()
http.server.test(HandlerClass=Handler, port=int(sys.argv[1]), bind="0.0.0.0")
' "$PORT"
