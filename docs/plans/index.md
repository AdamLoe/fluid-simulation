# Plans

Working coordination docs for multi-step work on this app. Plans are **not** canonical
architecture: once work ships, the relevant architecture/decisions docs get updated and
the plan is deleted.

The **lifecycle rules, status-frontmatter spec, and ship-time migration workflow are
generic** (agent-docs v1) and live in the kit:
[`~/agent-docs/v1/plan-lifecycle.md`](~/agent-docs/v1/plan-lifecycle.md).
New plans start from
[`~/agent-docs/v1/plan-template.md`](~/agent-docs/v1/plan-template.md).

## What lives here

Active coordination lives here, alongside the landing doc for the plan layer. This
index does not maintain an inventory (it would rot the moment one is added) — list the
directory:

```
ls docs/plans/
```

[`roadmap.md`](roadmap.md) is the current long-lived coordination map. Deferred ideas
that should not be smuggled into current implementation plans live in
[`future-roadmap.md`](future-roadmap.md). Historical shipped plans belong in
`archive/`. Everything else should reach `shipped + okay_to_delete: true` and then be
deleted.

## Routing

| Need | Read |
|---|---|
| Current coordination map / what's next | [`roadmap.md`](roadmap.md) |
| Deferred future ideas | [`future-roadmap.md`](future-roadmap.md) |
| Historical shipped plans | [`archive/`](archive/) |
| Create a new plan | [`~/agent-docs/v1/plan-template.md`](~/agent-docs/v1/plan-template.md) |
| Plan lifecycle / status-metadata rules | [`~/agent-docs/v1/plan-lifecycle.md`](~/agent-docs/v1/plan-lifecycle.md) |
| The doc-update workflow a shipped plan triggers | [`../agent-context/maintaining-docs.md`](../agent-context/maintaining-docs.md) |

## See also

- [`~/agent-docs/v1/plan-lifecycle.md`](~/agent-docs/v1/plan-lifecycle.md)
- [`../agent-context/maintaining-docs.md`](../agent-context/maintaining-docs.md)
- [`roadmap.md`](roadmap.md)
- [`future-roadmap.md`](future-roadmap.md)
- [`../index.md`](../index.md) — global router.
