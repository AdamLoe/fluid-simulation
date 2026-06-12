---
status:        active
owner:         adamg
last_updated:  2026-06-12
okay_to_delete: false
long_lived:    true
owning_docs:
  - architecture/web-shell.md
  - architecture/app-shell.md
  - architecture/settings.md
  - architecture/profiler.md
  - architecture/rendering.md
  - architecture/gpu-resources.md
  - architecture/simulation.md
  - decisions/performance.md
  - decisions/rendering.md
  - decisions/scope.md
---

# Roadmap - current coordination map

This is the long-lived coordination page for work that still needs ordering or
cross-cutting judgment. It is not a stash for raw source plans or shipped handoff
records. Those belong in versioned plans while work is in flight, then in
`architecture/` and `decisions/` once shipped.

The active implementation layer is intentionally small right now:

- New implementation work should start as a versioned plan.
- Code-touching work that shares GPU/runtime files should stay serialized unless a
  specific ownership boundary is documented.
- `future-roadmap.md` holds deferred ideas that are not ready to become current work.
- Historical shipped plans belong in `archive/`.

## Current status

No versioned implementation plans are active in this layer right now.

## What to keep here

- Current coordination that affects multiple future plans.
- Ordering notes that are still relevant before a new versioned plan is written.
- Routing that helps people pick the right plan-layer document.

## Migration rule

Once a versioned plan ships, move durable facts and still-valid decisions into the
owning architecture/decisions docs, then delete the plan when it is marked
`okay_to_delete: true`.

## See also

- [`future-roadmap.md`](future-roadmap.md)
- [`index.md`](index.md)
- [`../agent-context/orchestrating.md`](../agent-context/orchestrating.md)
- [`../agent-context/build-run.md`](../agent-context/build-run.md)
