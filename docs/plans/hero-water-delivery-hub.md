---
status:        active
owner:         adamg
last_updated:  2026-06-09
okay_to_delete: false
long_lived:    false
owning_docs:
  - architecture/rendering.md
  - architecture/gpu-resources.md
  - architecture/settings.md
  - decisions/rendering.md
  - decisions/performance.md
---

# Hero-water delivery hub (v1.14 / v1.16 / v1.17 / v1.18)

Orchestration hub for autonomously delivering the back half of the hero-water series.
This is the **map** (the orchestrator's record of observed state); the per-plan detail
lives in each versioned plan doc. Follows
[`~/.claude/agent-docs/v1/rules/orchestrating.md`](~/.claude/agent-docs/v1/rules/orchestrating.md)
and [`../agent-context/orchestrating.md`](../agent-context/orchestrating.md).

## Mission

Deliver, in order, v1.14 (marching-cubes surface), v1.16 (caustics), v1.17 (wet walls),
v1.18 (temporal). opus plans & reviews, sonnet implements. Each plan ships through its own
pipeline and is committed before the next starts.

## Decisions log (from the lead, 2026-06-09)

- **Baseline:** v1.13 (foam) + v1.15 (environment) "work — just commit and move on." They
  were `status: shipped, okay_to_delete: true` with docs already migrated, but their code
  sat uncommitted in the working tree. Commit them as the baseline before new work. Only
  refuse to commit if the WASM build is actually broken.
- **Gates: full autonomy.** Sub-agents self-judge captures against each plan's exit-gate
  text. The orchestrator stops only on build/test failure. No pause for the lead at visual
  gates, including v1.14's de-risk go/no-go. The lead reviews the whole assembled stack at
  the end.
- **Sequential, not parallel** (forced, not chosen): all four plans edit the same core
  files (`composite.rs`, `gpu/mod.rs`, `settings/mod.rs`, `composite.wgsl`) and the same
  arch docs; the project bans worktrees and parallel code agents; one GPU. v1.18 stabilizes
  12–17 so it is strictly last.
- **Branching:** baseline commits to `main` (matches repo practice — version commits land
  on main; the lead asked to commit it). The four new/speculative plans land on branch
  `hero-water-14-18` so `main` stays at the known-good baseline and the autonomous stack is
  reviewable/revertable as a unit. Nothing is pushed.

## Constraints baked into every sub-agent prompt

- Shell is **inside WSL** → run build/test/serve commands **bare** (no `wsl.exe` wrapper).
- **Compile gate:** `cd /home/adamg/fluid-simulation/app && cargo build --target wasm32-unknown-unknown`
- **Host tests:** `cd /home/adamg/fluid-simulation/app && cargo test --lib`
- **Build + serve for capture:** `cd /home/adamg/fluid-simulation/app && ./run.sh` (run in
  background; it rebuilds the dev WASM, frees port 5184, serves `web/index.html` at the
  bare `http://localhost:5184/`).
- **Real-GPU capture** (Windows Chrome via Windows node — WSL node cannot launch it):
  `cd /home/adamg/fluid-simulation/app/tools && cmd.exe /c 'pushd \\wsl.localhost\Ubuntu-24.04\home\adamg\fluid-simulation\app\tools && node capture.mjs http://localhost:5184/ <out>.png 3500 & popd'`
  Healthy boot console: `navigator.gpu present: true`, smoke PASS, `fluid init: n=64`.
  `hasGpu: false` = unsupported overlay (capture failed). Captures land in gitignored
  `captures/`. Scene/particle env hooks: `PARTICLES=N`, `DRAG=1`, `EVAL=...`, `DETAILED=1`.
- **Per-stream gate is narrow** (compile + the new test). The full `run.sh`+capture is the
  per-plan acceptance gate. Never fabricate GPU timings; performance claims need profiler
  output.
- **Scope fence:** only the plan in flight edits code. No other plan's files in parallel.

## Per-plan pipeline (each plan runs this in sequence)

1. **Plan (opus)** — recon the *current* code (post-baseline), rewrite the draft plan into
   a concrete, code-grounded implementation plan; persist into the versioned plan doc.
2. **Implement (sonnet)** — build per the refined plan; gate = compile + `cargo test --lib`;
   report files changed + pasted gate output.
3. **Review (opus)** — adversarial diff review for GPU/WGSL correctness, no-readback, the
   fixed-point P2G contract, `params`-binding gotcha, perf risk. Returns findings.
4. **Fix (sonnet)** — apply review findings; re-gate.
5. **Capture + self-judge (sonnet)** — `run.sh` + capture the plan's target scene; judge
   against the exit-gate text; record PASS/FAIL + capture path + console health + render_ms.
6. **Orchestrator** — record observed outcome here, migrate durable facts to owning docs,
   commit on the branch, decide next plan.

## Streams table (observed state — update from agent reports + disk, not optimism)

| Stream | Area | Status | Last observed fact | Next action | Blockers |
|---|---|---|---|---|---|
| Baseline | commit v1.13+v1.15, prove capture loop | dispatched | agent running: verify build/test, validate capture, commit to main | await report | — |
| v1.14 | marching-cubes surface (de-risk gate) | pending | — | start after baseline + branch | baseline |
| v1.16 | approximate caustics | pending | — | after v1.14 | v1.14 |
| v1.17 | wet walls & meniscus | pending | — | after v1.16 | v1.16 |
| v1.18 | temporal stabilization | pending | — | after v1.17 (scope depends on what 14/16 shipped) | v1.14–v1.17 |

## Open questions / risks

- **Autonomous visual judgment is weak.** Sub-agents judging "reads as light, not noise"
  is inherently unreliable; the lead's end-of-run review is the real gate. Hub records will
  flag low-confidence PASSes honestly.
- **v1.14 de-risk may exit early** (quads don't beat screen-space → skip MC). If so, v1.18's
  thickness/normal-history scope shrinks. Record the call here and in the roadmap.
- **v1.16 needs v1.15's light direction**; v1.17 wants v1.13's foam; the planner for each
  must verify those hooks exist in the committed baseline, not assume the plan's wording.

## See also

- [`roadmap.md`](roadmap.md) — series order + the de-risk gate outcome goes here when known.
- The four versioned plan docs.
- [`../agent-context/build-run.md`](../agent-context/build-run.md) — gate commands.
