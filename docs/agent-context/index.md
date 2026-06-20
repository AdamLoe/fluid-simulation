# Agent context index

Procedural docs: when working on X, do Y, don't do Z.

## How to use this folder

Load only the procedural doc that matches your situation. Each opens with a "when
does this apply" framing. Under **agent-docs v1** the generic workflow discipline
lives in the global kit (`~/agent-docs/v1/rules/`); the docs here hold what's
specific to **this** app and link up to the matching global rule.

## Routing

| Situation | Read | Generic rule (global kit) |
|---|---|---|
| Editing Rust / WGSL / TypeScript code | [`coding-style.md`](coding-style.md) | `rules/coding-style.md` |
| Building the WASM, serving the web shell, browser-verifying with the capture harness | [`build-run.md`](build-run.md) | — (app-specific) |
| Packaging a release build and deploying through Cloudflare Pages | [`deploy.md`](deploy.md) | — (app-specific) |
| Running the host test suite, deciding what is host-testable | [`testing.md`](testing.md) | — (app-specific) |
| Updating docs after a code change | [`maintaining-docs.md`](maintaining-docs.md) | `rules/authoring-rules.md` |
| Orchestrating multi-step work via sub-agents | [`orchestrating.md`](orchestrating.md) | `rules/orchestrator/lifecycle.md` + `rules/orchestrator/dispatch.md` |
| Creating / shipping a plan | [`../plans/index.md`](../plans/index.md) (landing); [`~/agent-docs/v1/plan-lifecycle.md`](~/agent-docs/v1/plan-lifecycle.md) + [`plan-template.md`](~/agent-docs/v1/plan-template.md) | `rules/authoring-rules.md` §workflow |

## See also

- [`../index.md`](../index.md) — global router.
- [`../architecture/index.md`](../architecture/index.md) — the current-state facts these procedures reference.
- [`../_meta/manifest.md`](../_meta/manifest.md) — app slot-data the global rules plug into.
