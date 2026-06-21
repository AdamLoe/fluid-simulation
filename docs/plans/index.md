# Plans

Working coordination docs for multi-step work on this app. Plans are **not** canonical
architecture: once work ships, the relevant architecture/decisions docs get updated and
the plan is deleted.

The **lifecycle rules, status-frontmatter spec, and ship-time migration workflow are
generic** (agent-docs v1) and live in the kit:
[`~/.agentdocs/plan-lifecycle.md`](~/.agentdocs/plan-lifecycle.md).
New plans start from
[`~/.agentdocs/plan-template.md`](~/.agentdocs/plan-template.md).

## What lives here

Active coordination lives here, alongside the landing doc for the plan layer. This
index does not maintain an inventory (it would rot the moment one is added) — list the
directory:

```
ls docs/plans/
```

[`future-roadmap.md`](future-roadmap.md) is the only long-lived coordination doc in
this directory. It holds deferred ideas that should not be smuggled into current
implementation work. Versioned implementation plans may be created here when work is
active; once they ship, durable facts move to `architecture/` / `decisions/` and the
plan is deleted.

## Routing

| Need | Read |
|---|---|
| Deferred future ideas | [`future-roadmap.md`](future-roadmap.md) |
| Create a new plan | [`~/.agentdocs/plan-template.md`](~/.agentdocs/plan-template.md) |
| Plan lifecycle / status-metadata rules | [`~/.agentdocs/plan-lifecycle.md`](~/.agentdocs/plan-lifecycle.md) |
| The doc-update workflow a shipped plan triggers | [`../agent-context/maintaining-docs.md`](../agent-context/maintaining-docs.md) |

## See also

- [`~/.agentdocs/plan-lifecycle.md`](~/.agentdocs/plan-lifecycle.md)
- [`../agent-context/maintaining-docs.md`](../agent-context/maintaining-docs.md)
- [`future-roadmap.md`](future-roadmap.md)
- [`../index.md`](../index.md) — global router.
