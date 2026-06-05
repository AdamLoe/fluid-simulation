# Plans

Working coordination docs for multi-step work on this app. Plans are **not** canonical
architecture: once work ships, the relevant architecture/decisions docs get updated and
the plan is deleted.

The **lifecycle rules, status-frontmatter spec, and ship-time migration workflow are
generic** (agent-docs v1) and live in the kit:
[`~/.claude/agent-docs/v1/plan-lifecycle.md`](~/.claude/agent-docs/v1/plan-lifecycle.md).
New plans start from
[`~/.claude/agent-docs/v1/plan-template.md`](~/.claude/agent-docs/v1/plan-template.md).

## What lives here

Active and recently-shipped plans live in this directory. This index does not maintain
an inventory (it would rot the moment one is added) — list the directory:

```
ls docs/plans/
```

[`roadmap.md`](roadmap.md) is the one long-lived plan (the remaining 1.x direction).
Everything else should reach `shipped + okay_to_delete: true` and then be deleted.

## Routing

| Need | Read |
|---|---|
| The remaining roadmap / what's next | [`roadmap.md`](roadmap.md) |
| Create a new plan | [`~/.claude/agent-docs/v1/plan-template.md`](~/.claude/agent-docs/v1/plan-template.md) |
| Plan lifecycle / status-metadata rules | [`~/.claude/agent-docs/v1/plan-lifecycle.md`](~/.claude/agent-docs/v1/plan-lifecycle.md) |
| The doc-update workflow a shipped plan triggers | [`../agent-context/maintaining-docs.md`](../agent-context/maintaining-docs.md) |

## See also

- [`~/.claude/agent-docs/v1/plan-lifecycle.md`](~/.claude/agent-docs/v1/plan-lifecycle.md)
- [`../agent-context/maintaining-docs.md`](../agent-context/maintaining-docs.md)
- [`../index.md`](../index.md) — global router.
