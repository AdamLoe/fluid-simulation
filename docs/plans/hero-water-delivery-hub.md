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
- **Doc migration deferred:** each shipped plan is committed on the branch with its plan
  doc updated, but architecture/decisions migration (rendering.md, gpu-resources.md,
  settings.md, decisions/rendering.md) is batched into ONE consolidated doc-migrate pass at
  the end, not per-plan — cheaper and avoids churning the same docs four times.
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
- **Real-GPU capture** (Windows Chrome via Windows node — WSL node cannot launch it).
  Plain boot capture (no scene/setting control): `cd app/tools && cmd.exe /c 'pushd
  \\wsl.localhost\Ubuntu-24.04\home\adamg\fluid-simulation\app\tools && node capture.mjs
  http://localhost:5184/ <out>.png 3500 & popd'`. Healthy boot console: `navigator.gpu
  present: true`, smoke PASS, `fluid init: n=64`. `hasGpu: false` = unsupported overlay
  (capture failed). Captures land in gitignored `captures/`.
- **⚠️ PROVEN capture-with-EVAL invocation (REUSE for every gate — do NOT re-derive).**
  `EVAL=`/scene/setting control does NOT cross `bash → cmd.exe → Windows node`, and
  **`WSLENV` did not work** for arbitrary strings. The working path is **PowerShell with
  `$env:` and double-quotes escaped as `\"`**:
  ```
  powershell.exe -Command "$env:EVAL = 'window.__fluid.set_setting(\"scene.preset\",1); window.__fluid.set_setting(\"render.hero.debug_surface_source\",1); window.__fluid.reset(); \"label\"'; $env:EVAL_WAIT = '5000'; pushd '\\wsl.localhost\Ubuntu-24.04\home\adamg\fluid-simulation\app\tools'; node capture.mjs http://localhost:5184/ <out>.png 5000; popd"
  ```
  **Validity proof required:** every gate capture must show `[harness] EVAL -> "label"` in
  its `.console.txt` (else the toggle/scene did not actually apply — see the v1.14 attempt-1
  failure). `render_mode` in `stats_json` always reads `particles+pressure` (the physics
  mode); confirm the hero/composite *visual* path via `render.particle_view = 0` in the
  console + the higher `gpu.render_ms` signature, not via `render_mode`.
- Scene presets (Reset-class `scene.preset`, needs `reset()` after set): `0`=FallingBlob,
  `1`=DamBreak, `2`=DoubleSplash. **No thin-sheet preset exists** — use DoubleSplash or a
  mid-flight DamBreak tongue (short post-reset wait) as the thin stressor.
- Other env hooks: `PARTICLES=N`, `DRAG=1`, `DETAILED=1`, `FRAMES`/`FRAME_INTERVAL`.
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

## FINAL STATUS & HANDOFF (2026-06-09)

**Delivered on branch `hero-water-14-18` (5 commits on top of baseline `239239b` on main; nothing pushed):**

| Plan | Outcome | Confidence | Commit |
|---|---|---|---|
| v1.14 marching cubes | **Abandoned at de-risk gate** — occupancy surface lost to screen-space (worst on thin sheets); MC not built; removed-surface decision re-affirmed | high (clear loss, orchestrator-confirmed) | `b115240` |
| v1.16 caustics | **Shipped** — normal-gradient half-res, additive receiver composite before water; reuses v1.15 sun; default off; 2 must-fix GPU bugs caught+fixed; +0.82 ms | low-med (subtle, still-frame) | `040009a` |
| v1.17 wet walls | **Shipped** — per-texel wetness from live classification, decay, clears on Reset; wall darken/gloss/streak + meniscus + contact shadow; 0 must-fix; +0.6 ms | **low** (subtle by design, weak A/B) | `213f96c` |
| v1.18 temporal | **Shipped** — history-blend + camera-reset (NOT reprojection); ping-pongs thickness/smooth_z/whitewater; unifies caustics blend; 3 must-fix caught+fixed; no orbit ghosting; +0.69 ms, +~8 MB | infra solid; **motion-calm unverified from stills** | `81b8431` |
| docs migration | rendering/gpu-resources/settings/decisions + ownership updated; plans flipped okay_to_delete | — | `353985d` |

Combined-feature gate (all three on one frame): **no device/WGSL errors, 1.16 ms**.

### Live-review checklist (what stills genuinely cannot confirm — these are the LEAD's gates)

Run `cd app && ./run.sh`, open `http://localhost:5184/`, Water render mode, then toggle each Live `render.hero.*` group:
1. **Caustics** (`render.hero.caustics.enabled`) — in motion, does floor/back-wall light read as *focused light* not noise? Tune `intensity`/`focus_strength`.
2. **Wet walls** (`render.hero.wet_wall.enabled`) — slosh/dam-break: do impacts leave *decaying* wet streaks; is the meniscus visible but not overdone? (Most uncertain feature.)
3. **Temporal** (`render.hero.temporal.enabled`) — **#1 item:** is the dam-break visibly *calmer* in motion vs off, with **no ghosting/smearing on a camera orbit**? Tune `history_alpha` / `camera_motion_reset_threshold`.

Visual tuning changes default *values* only — the architecture (passes, buffers, render order) is already migrated and stable regardless.

### Follow-ups left for the lead (honest deferrals, not bugs)

- **Caustics-temporal settings divergence:** v1.18 plan claimed `render.hero.caustics.temporal_enabled/_alpha` were *removed/unified*; in the committed code they were **kept** (caustics blend = `(temporal_enabled && caustic_history) || caustics_temporal_enabled`). settings.md documents the actual state. If a clean removal + `panels.js` migration was intended, that cleanup is outstanding.
- **Manifest change-to-doc table** doesn't yet name the new files (`gpu/caustics.rs`, `gpu/wetwall.rs`, `gpu/temporal.rs` + shaders); they fit existing row patterns — add explicit rows if desired.
- v1.16 `caustics.mode` / `resolution_scale` / `blur_radius` are **reserved/not-yet-wired** (tooltips say so) — wire or remove later.
- Direct foam/spray→wetness coupling **deferred** in v1.17 (`wetness_spray_gain` stubbed at 0; cell-type signal already covers wall contacts).
- After live sign-off: merge `hero-water-14-18` → main (or cherry-pick), then the plan docs (now `okay_to_delete`) can be deleted via `/clear-plans`.

## Observed baseline facts (capture `239239b`, 2026-06-09)

- **247,500 particles, 64³ grid, CG press_iters=30, ~42 FPS (~24 ms/frame), 309
  dispatches/frame, `gpu-timestamp` profiling active.** Frame is already over a 16 ms
  budget — perf headroom for v1.14 MC is thin; the screen-space fallback is load-bearing.
- Foam (v1.13) live: `diffuse.alive≈9878 (foam 9638 / spray 235 / bubble 5)`.
- Environment reflection (v1.15) merged. Both confirmed in the boot capture.

## Streams table (observed state — update from agent reports + disk, not optimism)

| Stream | Area | Status | Last observed fact | Next action | Blockers |
|---|---|---|---|---|---|
| Baseline | commit v1.13+v1.15, prove capture loop | **DONE** | compile+27 tests green; capture loop WORKS (gpu present, smoke PASS, n=64, foam live); committed `239239b` on main | — | — |
| v1.14 | marching-cubes surface (de-risk gate) | **ABANDONED at gate** | valid A/B (toggle verified, EVAL echo present): occupancy quads clearly LOSE to screen-space on all 3 scenes, worst on thin tongue (noise). Orchestrator confirmed via own eyes. MC not built; screen-space kept; removed-surface decision re-affirmed. Throwaway code reverted; plan→abandoned; roadmap decision resolved. | — | — |
| ↳ infra | capture EVAL plumbing (cross-cutting) | **RESOLVED** | PowerShell `$env:EVAL` w/ `\"`-escaped quotes works; WSLENV + bash→cmd.exe do NOT. Invocation recorded in Constraints above. | reuse for all gates | — |
| v1.16 | approximate caustics | **SHIPPED (branch)** low-med conf | normal-gradient half-res caustics, additive receiver composite before water pass; reuses v1.15 sun_dir; Live `render.hero.caustics.*`, default off. Opus review caught 2 must-fix GPU bugs (scene_color read-write hazard; eye/world normal mismatch) — fixed. Valid on/off A/B, +0.82 ms render. Orchestrator eyeballed: on-floor focused brightening, subtle but coherent. | doc-migrate at final pass | — |
| v1.17 | wet walls & meniscus | **SHIPPED (branch)** LOW conf | WetWallSystem: per-texel wetness buffer, once-per-frame compute reads current classification, framerate-corrected decay, clears on Reset. environment.wgsl WALL reads wetness (darken/gloss/streak) + subtle meniscus + contact shadow. foam→wetness coupling deferred (cell-type signal covers contacts). Review 0 must-fix. +0.6 ms. **Visual A/B weak (mismatched camera angles, intentionally subtle) → flag for live review.** | doc-migrate at final pass | — |
| v1.18 | temporal stabilization | **SHIPPED (branch)** infra-solid / motion unverified | TemporalSystem ping-pongs thickness + smooth_z (feeds normals) + whitewater (foam); unifies v1.16 caustics blend; camera-delta from model-free eye_to_world; hard reset on rotation+translation; depth/normal rejects. Opus review caught 3 must-fix (ping-pong desync, reject-wiring, reset-missed-translation) — fixed. No artifacts, no orbit ghosting. +0.69 ms, +~8 MB. **"Calmer in motion" needs live review (#1 item).** | doc-migrate at final pass | — |
| FINAL | consolidated gate + doc migration | **DONE** | build+28 tests green; combined caustics+wet_wall+temporal boot smoke = NO errors, 1.16 ms; docs migrated (committed `353985d`); plans flipped okay_to_delete | hand off to lead | — |

## Open questions / risks

- **Autonomous visual judgment is weak.** Sub-agents judging "reads as light, not noise"
  is inherently unreliable; the lead's end-of-run review is the real gate. Hub records will
  flag low-confidence PASSes honestly.
- **v1.14 de-risk may exit early** (quads don't beat screen-space → skip MC). If so, v1.18's
  thickness/normal-history scope shrinks. Record the call here and in the roadmap.
- **v1.16 needs v1.15's light direction**; v1.17 wants v1.13's foam; the planner for each
  must verify those hooks exist in the committed baseline, not assume the plan's wording.

## Polish iteration (2026-06-09)

Live-review feedback (lead looked UP CLOSE): (1) wet walls "way too pixely"; (2) water
"still looks like spheres" up close. Marching cubes stays OFF the table — fix in screen space.

**Diagnosis (read-only pass, no code touched):**

- **Sphere look.** `smooth_z` is produced by `gpu/smoothing.rs` + `shaders/water_smooth.wgsl`
  as a single separable bilateral pass — exactly one X draw (`draw_x`) then one Y draw
  (`draw_y`) in `gpu/mod.rs` (`water smooth x/y pass`, ~lines 1350–1396), radius **3**,
  `sigma_spatial 1.65`, `sigma_range = max(0.035, center*0.018)`. The normal in
  `composite.wgsl::water_normal` is a 1-px central finite-difference off that under-smoothed
  `smooth_z`, so each particle billboard splat survives as a bump. The thickness/`nearest_z`
  splat in `particles.wgsl::fs_thickness` uses `cam.right.w` (particle world radius) — same
  small footprint, so splats don't overlap enough to fuse pre-smoothing.
- **Pixely walls.** Wetness is **one f32 per simulation-grid wall texel** (grid default 64,
  `wetwall.rs`), and `environment.wgsl` reads it with `u32(fi)`/`u32(fj)` **nearest** truncation
  (`back_wall_wetness` etc., ~lines 68–165). At a close camera that's a coarse, hard-edged
  texel grid. The `_pair` helpers already interpolate in j for the meniscus, but the base read
  is nearest in both axes at grid res.

**Plan:** raise smoothing to Live-tunable iteration count + radius (loop the X/Y bilateral
N times), optionally widen the thickness splat radius; add a bilinear (and small-blur) wetness
read and/or supersample the wetness field per wall. Capture UP CLOSE via Live `camera.distance`
(min 2.0) — NOT a wide tank shot.

## See also

- [`roadmap.md`](roadmap.md) — series order + the de-risk gate outcome goes here when known.
- The four versioned plan docs.
- [`../agent-context/build-run.md`](../agent-context/build-run.md) — gate commands.
