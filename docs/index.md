# Documentation index

This tree is the canonical, current-state snapshot of **fluid-lab** — a browser-native
Rust/WASM/WebGPU 3D fluid simulation lab. It is optimized for LLM consumption: start
small, route by task, load only the subtree that matches the work. It follows
**agent-docs v1** — the generic doc-authoring rules and the workflow/maintenance
commands live in the global kit (`~/agent-docs/v1/` and `~/.claude/skills/`);
this tree holds only what's specific to this app.

If you are a fresh AI chat: run the **`/fresh-chat`** skill. It routes you here, then
to [`overview.md`](overview.md), then to the smallest matching subtree index.

## How to use this tree

- **System overview** — [`overview.md`](overview.md). Read at chat start for the shape.
- **File inventory** — [`repository-layout.md`](repository-layout.md). Read only when
  you need to find where code lives.
- **Ownership** — data in [`_meta/ownership.json`](_meta/ownership.json) (concept →
  owner doc); [`ownership.md`](ownership.md) explains how to use it.
- **App bindings** for the global kit — [`_meta/manifest.md`](_meta/manifest.md)
  (`code_root`, the change→doc table, drift gates, decisions domains).
- **Architecture** — what currently IS. Route via [`architecture/index.md`](architecture/index.md).
- **Decisions** — why the design is this way. Route via [`decisions/index.md`](decisions/index.md).
- **Agent-context** — procedural "when working on X, do Y." Route via [`agent-context/index.md`](agent-context/index.md).
- **Release deploy** — [`agent-context/deploy.md`](agent-context/deploy.md).
- **Plans** — coordination docs for in-flight work. Route via [`plans/index.md`](plans/index.md).

## Global routing

| Need | Read |
|---|---|
| System at a glance | [`overview.md`](overview.md) |
| Where files live | [`repository-layout.md`](repository-layout.md) |
| Current subsystem facts | [`architecture/index.md`](architecture/index.md) |
| Design rationale | [`decisions/index.md`](decisions/index.md) |
| Workflow / build / verify / code-edit guardrails | [`agent-context/index.md`](agent-context/index.md) |
| Build, run, browser-verify | [`agent-context/build-run.md`](agent-context/build-run.md) |
| Release packaging / Cloudflare Pages deploy | [`agent-context/deploy.md`](agent-context/deploy.md) |
| App bindings for the global kit | [`_meta/manifest.md`](_meta/manifest.md) |
| Plan status rules or active plans | [`plans/index.md`](plans/index.md) |
| Canonical owner for a concept | [`_meta/ownership.json`](_meta/ownership.json) |

## See also

- [`overview.md`](overview.md)
- [`repository-layout.md`](repository-layout.md)
- [`ownership.md`](ownership.md)
