# Orchestrating

## When does this apply

You're driving multi-step fluid-lab work, especially across sub-agents. The generic
orchestration discipline is in the global kit
([`~/agent-docs/v1/rules/orchestrating.md`](~/agent-docs/v1/rules/orchestrating.md));
this doc holds what's specific to this app.

## Canonical truth

The durable solver invariants and product decisions now live in
[`../architecture/`](../architecture/index.md) (what is) and
[`../decisions/`](../decisions/index.md) (why). When two docs disagree, the owner doc
named in [`../_meta/ownership.json`](../_meta/ownership.json) wins. (The pre-v1
`simulation_contract.md` / `decisions.md` planning docs have been migrated into these
trees and removed.)

## Model selection

Prefer **Sonnet** for implementation, documentation, validation, and routine tasks.
Reserve **Opus** for work where it earns its cost:

- resolving ambiguous architecture / simulation-invariant decisions,
- reviewing high-risk GPU/WGSL, pressure-solver, P2G, G2P, or particle-scale work,
- debugging failures multiple Sonnet attempts couldn't isolate,
- gate calls where the evidence is mixed or risky.

Do not default to Opus for ordinary scaffolding, straightforward tickets, or docs
bookkeeping.

## Non-negotiables (these protect the next agent, not bookkeeping)

- **Never fabricate GPU timings.** When `timestamp-query` is unavailable, use the
  labeled minimum-honest fallback profiler — never invent per-pass numbers.
- **Performance claims require raw measurements** (browser/GPU, grid res, particle
  count, pressure iters, render mode, frame time, top costs).
- **If you change a simulation invariant or a decision, say so explicitly** and rewrite
  the owner doc in place — do not append a version-flavoured note.
- **A GPU-native screenshot/capture is the one acceptance signal that can't be faked.**
  Drop one for each visible win.

## This-project constraints

- **No git, no worktrees.** Sub-agents that touch code run **sequentially**, not in
  parallel, to avoid clobbering each other. (Read-only doc/code research agents can run
  in parallel.)
- Keep tasks small and measurable; large vague prompts ("implement the GPU fluid sim")
  cause drift. Split by subsystem.

## See also

- [`~/agent-docs/v1/rules/orchestrating.md`](~/agent-docs/v1/rules/orchestrating.md) — the generic rule.
- [`../decisions/index.md`](../decisions/index.md) · [`../architecture/index.md`](../architecture/index.md)
- [`../plans/index.md`](../plans/index.md) — staging in-flight work.
