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

### Round-2 polish diagnosis (2026-06-09, read-only)

Round-1 shipped the iterated bilateral (`smooth_iterations` 1-4, `smooth_radius` 3-8) and
`smooth_thickness_splat_scale` (0.5-3.0, default 1.3), plus wetness bilinear/bicubic-smooth
interpolation. Lead's round-2 verdict: water still not smooth, walls still pixely + wet effect
invisible. Highest-leverage untried levers identified:

- **WATER — the normal, not (only) the depth.** `composite.wgsl::water_normal` is still a
  *1-pixel* central difference (`pixel±(1,0)`/`±(0,1)`) off `smoothed_z_tex`; even on a
  well-smoothed depth the 1px stencil re-amplifies residual per-splat ripples into spiky
  normals → spherical lighting. Fixes (all Live): (1) widen the stencil to a tunable
  `normal_stencil` px (2-3) central difference; (2) add an optional explicit normal-smoothing
  blur — either a small box/Gaussian over the reconstructed normal, or one more wide bilateral
  iteration; (3) raise the splat default/range so depth is continuous *before* smoothing
  (`fs_thickness` writes `nearest_z` with `radius_world * splat_scale * nz`). Honest ceiling:
  at ~247k splats screen-space normals will never be glass-flat; aim is "calm pool reads as a
  surface, not beads," not CG-perfect. Do NOT over-blob thin sheets — keep the depth-range
  Gaussian tight and gate stencil width so tongues survive.

- **WET WALLS — two bugs.** (1) *Pixely*: the wetness buffer is **one f32 per sim wall cell**
  (`wetwall.rs`, grid default 64); interpolation alone can't invent detail. Supersample: make
  the buffer `S×` per wall axis (Live `wet_resolution` factor 1-4), update `wetwall_update.wgsl`
  to write S texels per sim cell (interpolating contact from the 4 surrounding inner cells so
  adjacent hi-res texels differ), and update all three read mappings in `environment.wgsl`
  consistently (the `dims`/`face_counts` in `WetWallUniform` must reflect the supersampled
  counts). (2) *Invisible on black*: wall base is `vec3(0.10,0.12,0.16)` matte — darken+gloss do
  nothing. Redesign WALL branch so wet = **reflective**: reflect `env.wgsl::env_sample` (already
  the skybox/water reflection source) off the wall's world normal (constant per face: back =
  `+z`, left = `+x`), view dir = `normalize(world_pos - eye_world)`, blended by `wet *
  wet_reflectivity` + a sun specular sheen scaled by wetness. Requires plumbing `env_sample`
  (concat into the environment module), the camera world eye position (`camera.eye()`, NOT in
  `eye_to_world` which is rotation-only), and sun/env-ctrl into the env group-0 uniform.

**Settled-surface capture:** FallingBlob (`scene.preset` 0) with a long `EVAL_WAIT` (~9000 ms)
so the blob pools into a calm sheet, `camera.distance` ~2.0-2.5 — judging smoothness needs a
SETTLED low-spray surface, not a mid-splash frame.

## Round-3 diagnosis (wall regression + de-pixelation + flat-water)

**A. Wall regression root cause (commit ee5328a, "polish 2").** The supersample change made
`WetWallUniform.dims` store the *supersampled* counts (`dims.x = nx_ss = nx*ss`, default ss=2).
But `wetwall_update.wgsl::cell_idx` (line ~34) still strides the `cell_type` buffer with
`wu.dims.x`/`wu.dims.y` — now `nx_ss`/`ny_ss` instead of the original sim `nx`/`ny`. The
`cell_type` buffer is laid out `i + j*nx + k*nx*ny` with ORIGINAL nx/ny (`sim::Dims::cell_idx`,
`i + nx*(j + ny*k)`). So every contact lookup reads the wrong (and frequently out-of-range,
wrapping) cell → a wall texel's wetness is sampled from a cell `~ss×` away, producing the
"80% of a wall missing, leftover chunk duplicated above and below" wrap/scale artifact. The
write *index* into the wetness buffer (`tid`) and all three `environment.wgsl` READ mappings are
internally consistent on the supersampled dims — they are NOT the bug. The single defect is the
`cell_type` stride in `cell_idx`.

  - **Fix (shader-only):** in `wetwall_update.wgsl::cell_idx`, recover the original grid dims by
    dividing out `ss`: `let nx = wu.dims.x / ss; let ny = wu.dims.y / ss;` and stride
    `i + j*nx + k*nx*ny`. Exact because `nx_ss = nx*ss`. (`face_counts.w` already carries the
    original nx, but ny is not stored — the divide recovers both with no Rust change.)

**B. De-pixelation (supersample + blur), done right.** With (A) fixed, write/read dims already
agree on the supersampled grid and the buffer is sized to match. `supersample` is `ApplyClass::
Reset` (registry line ~1608), so the buffer is rebuilt on change — no Live desync (the Live
`set_params` path only rewrites the uniform; `total_texels`/buffer stay construction-sized, which
is correct precisely because the setting is Reset-class). The remaining smoothness gap is the
READ: `environment.wgsl` already does bicubic-smooth (`wet_smooth`) bilinear, but a real blur on
the read (a small box/tent over neighbour texels, gated by a Live width) removes residual
blockiness without inventing wrap. Keep ss default 2 (ss=3-4 for hero close-ups), add a Live
`render.hero.wet_wall.blur` (texel radius 0-2) consumed in the three `*_wetness` readers.

**C. Flat-water-against-walls (composite normal-snap) — difficulty M.** `composite.wgsl` has the
eye-space normal `n`, the eye-space front depth `front_z` (= -z_eye from `smoothed_z_tex`), and
the per-pixel eye ray (reconstructed exactly as the dbg=9 branch does:
`fdir_eye = normalize(vec3(ndc.x*thf*aspect, ndc.y*thf, -1))`). What it LACKS to test against the
tank planes (x=±1, z=±1, y=-1, box-local): the camera world eye *position* and the box-local
transform. Both already exist in `mod.rs::render` (`eye_world`, `box_pos`, `box_orient`, and
`tank_bounds`) — `environment.rs::set_eye_world` already plumbs box-local eye + box_rot into the
env uniform; the composite `Cam` uniform (composite.rs `CamUniform`, currently rotation-only)
must be expanded the same way (add box-local eye, box_rot columns, tank_lo/hi). Then per pixel:
`eye_pos_eye = fdir_eye * (front_z / -fdir_eye.z)`; rotate+translate into box-local; if it lies
within an epsilon of a wall/floor plane, `mix(n, plane_normal_eye, flatten_strength)` (the plane
normal must be carried into eye space via the inverse of `eye_to_world`·box_rot). Minimal-viable
= the normal-snap; a fuller per-wall water-fill pass is a larger lift and only needed if the snap
reads too abrupt at the waterline.

**C-round4. Flat-water DEPTH/silhouette flatten (composite) — difficulty M.** Round-3 (cba2b15)
shipped the normal-only snap above; the lead confirms it flattens SHADING but not the SILHOUETTE
— the bumpy front-surface DEPTH (the per-particle sphere traced into `smoothed_z_tex`) still
shows at the glass. Round-4 snaps the front DEPTH too, for the SAME near-wall pixels, so the
surface becomes coplanar with the glass.

*Reuse the round-3 reconstruction verbatim.* The block already computes `pos_bl` (front-surface
position in box-local) and the five signed plane distances `d_left/d_right/d_back/d_front/d_floor`.
Round-4 adds a depth snap before the normal blend, keyed on the SAME nearest plane:

1. Pick the nearest in-range plane (smallest `d_*` that is `< epsilon && >= neg_tol`). Reuse the
   exact selection the normal loop uses; the depth snap and the normal snap must agree on which
   plane (so refraction UV, thickness consumption, Fresnel, and lighting all see one consistent
   surface).
2. Intersect the EYE RAY with that wall plane in box-local. The eye ray in box-local is:
   origin `o = cam.box_eye_local.xyz`, direction `dir_bl = normalize(pos_bl - o)` (pos_bl already
   lies on the ray, so this is exact and avoids re-deriving the ray). The plane is axis-aligned in
   box-local: e.g. left wall `x = lo.x` → `t = (lo.x - o.x) / dir_bl.x`; floor `y = lo.y` →
   `t = (lo.y - o.y) / dir_bl.y`; etc. Guard the denominator (`abs(dir_bl.axis) > 1e-5`) and
   require `t > 0`; if the intersection is degenerate, skip the depth snap (keep round-3 normal
   behaviour). `hit_bl = o + t * dir_bl`.
3. Convert `hit_bl` back to the eye-distance the composite calls `front_z`. Mirror the round-3
   forward transform in reverse: `delta_world = box_rot * (hit_bl - cam.box_eye_local.xyz)`;
   `pos_eye = eye3_t * delta_world` (eye3_t = transpose(eye3) = world→eye since eye3 is a
   rotation); snapped `front_z_flat = -pos_eye.z` (front_z is `-z_eye`, the eye distance).
4. Blend by the SAME `strength * t_ramp` weight as the normal snap (so strength=0 is a no-op and
   the waterline ramp matches): `front_z = mix(front_z, front_z_flat, depth_strength * t_ramp)`.
   Apply BEFORE the refraction depth guard and the offset/thickness reads so they all consume the
   flattened surface. Keep a separate Live `depth_strength` knob from the normal `strength` so the
   two effects can be tuned independently (normal flatten is cheaper/safer; depth flatten is the
   silhouette fix).

*Refraction safety.* The snap only moves `front_z` toward the camera-ray/plane hit for pixels
ALREADY classified near-wall water (`has_water` + a `d_* < epsilon`), so it cannot create water
where there is none, cannot touch the no-water sentinel (`front_z >= 60000` short-circuits the
whole block — `has_water` is false), and leaves the refraction UV math (driven by `n.xy`+thickness,
not front_z) qualitatively unchanged while the depth guard (`scene_z_refr < front_z - 0.02`) now
compares against the coplanar surface (correct — geometry behind the glass still passes). All of it
is Live-guarded: `depth_strength <= 0.001` skips the snap entirely (no-op, matches round-3).

*Gap-fill decision: NOT needed now.* The wetwall occupancy signal (cell_type Liquid-adjacent-to-
Solid, `wetwall_update.wgsl`) projects to WALL-texel space, not screen space, and would require a
new screen-space reprojection pass to fill inter-splat holes. The thickness/smooth_z front surface
is already gap-filled upstream by the smoothing + temporal passes (the splats overlap and the
bilateral fill closes pinholes), so snapping the already-detected near-wall water yields a
continuous sheet without it. Defer the per-wall liquid-occupancy fill to a later round only if the
snapped sheet still reads as discontinuous at the waterline in capture.

*New knobs.* `render.hero.flat_water.depth_strength` (F32 0..1, default 0.8, Live) routed into a
new `flat.z` slot of the composite `CamUniform` (currently `flat = [strength, epsilon, 0, 0]` →
`[strength, epsilon, depth_strength, 0]`) + `HeroParams.flat_water_depth_strength`. The existing
`render.hero.flat_water.epsilon` is reused unchanged for the depth band. No new uniform STRUCT
fields are required — `flat.z`/`flat.w` are already reserved zero slots.

## See also

- [`roadmap.md`](roadmap.md) — series order + the de-risk gate outcome goes here when known.
- The four versioned plan docs.
- [`../agent-context/build-run.md`](../agent-context/build-run.md) — gate commands.
