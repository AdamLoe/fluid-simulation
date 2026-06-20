# Deploy

## When does this apply

You need to package a release build or ship the app via **Cloudflare Pages**.

## The deploy path

Production hosting is Cloudflare Pages, auto-deploying from the GitHub repo on every
push to the production branch. Cloudflare does **not** compile: the release WASM is
built locally and committed, and CF just assembles and serves the static bundle.

- `app/cf-build.sh` has two modes and always writes a clean deploy directory at
  `app/web/dist`:
  - `--prebuilt` is what Cloudflare runs. It skips compilation and expects the committed
    release package to already be present.
  - no flag release-compiles the WASM from source. Use this locally when refreshing the
    committed package.
- The committed release artifacts are `app/web/pkg/{fluid_lab.js,fluid_lab_bg.wasm}`.
  The rest of `pkg/` stays gitignored. Dev builds go to ignored `app/web/pkg-dev/`
  and are never used by `--prebuilt`. `app/web/dist` is regenerated on each deploy build.
- `app/web/_headers` carries the deploy-time HTTP headers, including the iframe
  allowance for `adamloe.com` and the cache policy for the static bundle.
- `app/rust-toolchain.toml` pins the release compile toolchain and wasm target.

## Cloudflare settings

| Field | Value |
|---|---|
| Root directory | *(blank — repo root)* |
| Build command | `bash app/cf-build.sh --prebuilt` |
| Build output directory | `app/web/dist` |

No COOP/COEP cross-origin-isolation headers are needed. Cloudflare serves `.wasm` as
`application/wasm`, which the wasm-bindgen `--target web` streaming init requires.

## Shipping a Rust change

```
bash app/cf-build.sh
git add -f app/web/pkg/fluid_lab.js app/web/pkg/fluid_lab_bg.wasm
git commit -m "…" && git push
```

HTML/JS-only changes can skip refreshing `app/web/pkg` if the committed package already
matches the deployable bundle.

## Preview the production bundle locally

```
bash app/cf-build.sh && python3 -m http.server 5184 -d app/web/dist
```

Then browser-verify at the bare `http://localhost:5184/`.

## What not to do

- Don't expect `local_dev.sh` to prepare a deployable package. It is for the inner loop,
  writes a dev build into ignored `web/pkg-dev`, and serves browser `/pkg/*` from that
  directory only for the local dev session.
- Don't hand-edit the generated `app/web/dist` contents.

## See also

- [`build-run.md`](build-run.md) — inner-loop build, serve, and browser verify.
- [`../architecture/web-shell.md`](../architecture/web-shell.md) — static shell facts.
