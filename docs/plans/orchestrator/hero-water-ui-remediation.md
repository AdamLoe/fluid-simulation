---
status:        shipped
owner:         codex
last_updated:  2026-06-12
okay_to_delete: true
long_lived:    false
owning_docs:
  - architecture/rendering.md
  - architecture/settings.md
  - architecture/web-shell.md
  - architecture/gpu-resources.md
  - decisions/rendering.md
  - decisions/performance.md
---

# Hero Water And UI Remediation

Temporary orchestration hub for the 2026-06-12 visual/UI cleanup request. This file is
the lifecycle map; durable facts must be migrated into the owning docs before shipping.

## Mission

Make the default app look better and feel easier to tune by replacing the vague
`render.hero.mode_enabled` switch with explicit feature controls, making the main water
less clear/insubstantial, default-disabling weak or expensive visual add-ons in the
normal path, reducing the wall-fill quality/performance regression, and cleaning the
settings/UI/CSS enough that future tuning is not fighting the shell.

Done means the first loaded Water view has a credible opaque/refractive water body, no
bad-looking experimental features are on by default, wall fill no longer imposes the
current high-cost/high-artifact default, and the settings/control chrome is easier to
navigate without changing the app framework. Phase 1 must ship without a new user
decision; destructive removal/gutting of shipped feature groups is a later decision.

## Product Assumptions

- Prefer a cleaner default over preserving every shipped hero-water effect in the normal
  startup path. Phase 1 may default-disable weak/expensive effects and clean up
  reserved/no-op controls, but it must not delete pass wiring or hide whole shipped
  feature groups based only on a quick capture.
- "Hero water" is no longer a product-level boolean. Users should be able to toggle
  separate behaviors such as refraction/body color, reflection/environment, flat-wall
  contact correction, wall-fill sheet, caustics, wet walls, temporal history, and diffuse
  foam/spray.
- Keep the screen-space water architecture. Marching cubes stays out of scope; older
  plans already recorded that it lost to the screen-space path.
- Treat wall fill as a quality/performance regression until proven otherwise. The
  acceptable Phase 1 fix is default-off or a lower-cost default with verified skip/cost
  evidence, not a broad rewrite.
- Do not ask the Phase 1 implementer to salvage caustics, diffuse foam/spray, or
  temporal. Default-disable and clearly group/collapse weak feature controls. Full
  removal, pass gutting, or hiding whole groups requires a Phase 2 user decision.
- Current worktree changes are not assumed to belong to the implementer. Read before
  editing and do not revert unrelated dirty files.

## Current Source Facts Used

- `code_root` is `app/`; the Rust crate is under `app/crates/fluid-lab/src/`.
- Settings are registry-owned in `crates/fluid-lab/src/settings/mod.rs`; tabs are
  `SettingsTab` metadata emitted in `config_json`.
- The current master switch is `render.hero.mode_enabled`; `CompositeRenderer` gates
  refraction and reflection from that single flag.
- Water feature groups already have separate setting families:
  `render.hero.flat_water.*`, `render.hero.wet_wall.*`,
  `render.hero.caustics.*`, `render.hero.temporal.*`, and `render.diffuse.*`.
- Current defaults make wet walls and wall fill on by default:
  `render.hero.wet_wall.enabled = 1`,
  `render.hero.flat_water.fill_enabled = 1`,
  `render.hero.wet_wall.supersample = 8`, and
  `render.hero.flat_water.fill_supersample = 16`.
- Wall fill uses `WallOccupancySystem` plus `WallFillRenderer`
  (`gpu/wallfill.rs`, `shaders/wallfill.wgsl`): an every-frame particle-splat compute
  pass into a dense per-wall atlas, then MRT injection into thickness/nearest-Z.
- Diffuse water lives in `gpu/diffuse.rs` and `shaders/diffuse_{emit,update,render}.wgsl`;
  it is already default-off but still exposes a full user-facing tab.
- Caustics and temporal live in `gpu/caustics.rs` / `gpu/temporal.rs`; several caustics
  settings are documented in-code as reserved/not wired, and temporal has a reserved TAA
  jitter setting.
- CSS is currently embedded in `web/index.html`; core tokens exist but many colors,
  borders, spacings, and panel styles are still hardcoded. The bottom launcher uses
  `.launcher-shell { width: min(760px, 100%); }`, which explains the too-wide panel.
- The shipped settings-panel plan already moved settings into a right-side panel. This
  remediation should refine navigation, not redo the whole shell.

## Scope

Phase 1 in scope:

- Replace visible use of `render.hero.mode_enabled` with the exact narrower controls
  listed in **Phase 1 Workstreams** and preserve legacy compatibility behavior.
- Retune main water opacity/body color/refraction/reflection defaults so maxed settings
  are not required for visible water.
- Default-disable weak/expensive add-ons where appropriate: caustics, diffuse foam/spray,
  temporal, wet-wall sub-effects, and dense wall fill.
- Remove or hide only clearly reserved/no-op controls that do not change behavior, with
  safe persisted-setting handling.
- Reduce wall-fill default cost and artifact risk through default-off or lower-cost
  defaults plus verified disabled-path skip.
- Add a collapsible side-panel settings navigator or equivalent compact navigation for
  many functional tabs.
- Make the bottom Mode/Control launcher shrink to its content on desktop and still wrap
  cleanly on mobile.
- Clean shell CSS by introducing core variables for background, borders, text, accents,
  spacing, radii, shadow, panel widths, and control sizing.
- Update architecture/decision docs at ship time for any changed current behavior.

Phase 2 candidate scope, not authorized by this Phase 1 plan:

- Remove caustics, temporal, diffuse, wet-wall, or wall-fill pass wiring.
- Hide whole shipped feature groups from all normal settings.
- Rebuild foam/spray, caustics, temporal reprojection, or wet-wall visuals.
- Make a product decision that an existing shipped feature is permanently abandoned.

Out of scope:

- Replacing vanilla JS with a framework or build step.
- Reopening marching-cubes/mesh water.
- Changing FLIP/PIC simulation, pressure solve, P2G fixed-point atomics, or tank
  topology.
- Adding CPU/GPU readback to normal render frames.
- Making unsupported features look good by hiding them in screenshots only.

## Approach

Implement Phase 1 as one serial pass. The files overlap too much for parallel code
agents. Phase 2 starts only after the user reviews Phase 1 captures and chooses which
weak shipped features should be removed, hidden, or redesigned.

## Lifecycle Tracker

| Stream | Area | Status | Next action |
|---|---|---|---|
| Phase 1 plan | Shippable remediation plan | shipped | Completed 2026-06-12 |
| Phase 1 implementation | Controls/defaults, wall-fill defaults, UI/CSS | shipped | Code/docs implemented and gated |
| Phase 1 review | Capture/profiler/UI review | shipped | Browser captures and repeated wall-fill samples recorded |
| Phase 2 decision | Weak feature removal/gutting | pending | User decides after Phase 1 evidence |

## Phase 1 Workstreams

### 1. Render-Control Contract And Default Water Quality

Owned files:

- `crates/fluid-lab/src/settings/mod.rs`
- `crates/fluid-lab/src/gpu/composite.rs`
- `crates/fluid-lab/src/gpu/shaders/composite.wgsl`
- `crates/fluid-lab/src/gpu/mod.rs` only if pass scheduling or feature gating changes

Work:

- Add these exact Live `u32` enum controls, all defaulting to enabled unless noted:
  - `render.hero.refraction_enabled` (`Enabled`/`Disabled`, default `1`): gates the
    normal-driven scene-color UV offset. Disabled means sample the unrefracted
    scene-color tap while leaving body color/absorption available.
  - `render.hero.reflection_enabled` (`Enabled`/`Disabled`, default `1`): gates Fresnel
    environment reflection and sun specular in the water composite. It must not disable
    the skybox or environment prepass.
  - `render.hero.body_color_enabled` (`Enabled`/`Disabled`, default `1`): gates
    Beer-Lambert absorption, base tint, transparency, and deep-water darkening so users
    can compare optical bending against water-body opacity.
  - `render.hero.wall_contact_enabled` (`Enabled`/`Disabled`, default `1`): gates the
    cheaper flat-water normal/depth correction (`flat_water.strength`,
    `flat_water.depth_strength`, `flat_water.epsilon`) independently of dense wall fill.
- Keep existing feature toggles for add-on groups:
  `render.hero.flat_water.fill_enabled`, `render.hero.wet_wall.enabled`,
  `render.hero.caustics.enabled`, `render.hero.temporal.enabled`, and
  `render.diffuse.enabled`.
- Legacy compatibility for `render.hero.mode_enabled`:
  - remove it from visible `config_json` after the new controls exist;
  - keep `set_setting("render.hero.mode_enabled", value)` accepted;
  - `value == 0` sets `refraction_enabled = 0`, `reflection_enabled = 0`, and
    `body_color_enabled = 0`;
  - `value != 0` sets those three controls back to `1`;
  - do not change `wall_contact_enabled`, `flat_water.fill_enabled`, wet walls,
    caustics, temporal, diffuse, camera, or environment settings through this legacy id;
  - persisted localStorage entries for the legacy id must not crash startup and should be
    removed from future saved visible settings once replayed.
- Retune defaults so default water is less transparent:
  - lower `render.hero.transparency`;
  - raise or rebalance `absorption_strength`, `deep_water_darkening`, and `base_tint`;
  - keep reflection/refraction useful at midrange values, not only maxed;
  - preserve thin-sheet readability and avoid turning all water into a solid blue blob.
- Remove or hide stale/no-op controls while touching the schema:
  `render.hero.caustics.mode`, `render.hero.caustics.resolution_scale`,
  `render.hero.caustics.blur_radius`, `render.hero.caustics.temporal_enabled`,
  `render.hero.caustics.temporal_alpha`, and `render.hero.temporal.jitter_enabled`
  unless the implementation truly wires them.
- Update settings tests that assert defaults, tab membership, and `hero_params()`.

Acceptance:

- `config_json` no longer presents a single "Hero water" master as the normal control.
- The four new core controls appear in appropriate settings groups with enum labels and
  apply Live.
- A default capture reads as a visible water body with meaningful tint/absorption, not
  mostly transparent glass.
- Turning refraction and reflection off separately produces understandable comparisons.
- Turning body color off separately shows clearer refractive water without changing the
  reflection toggle.
- Legacy persisted `render.hero.mode_enabled` values map exactly as specified above.

### 2. Weak Feature Defaults And Grouping

Owned files:

- `crates/fluid-lab/src/settings/mod.rs`
- `crates/fluid-lab/src/gpu/caustics.rs`
- `crates/fluid-lab/src/gpu/temporal.rs`
- `crates/fluid-lab/src/gpu/diffuse.rs`
- `crates/fluid-lab/src/gpu/shaders/caustics_*.wgsl`
- `crates/fluid-lab/src/gpu/shaders/temporal_blend.wgsl`
- `crates/fluid-lab/src/gpu/shaders/diffuse_*.wgsl`
- `crates/fluid-lab/src/gpu/mod.rs`
- `web/panels.js` if hidden/experimental rows need panel filtering

Work:

- Do not remove or gut shipped pass wiring in Phase 1.
- Default-disable weak/expensive optional effects unless they are already default-off:
  - caustics remain default off;
  - diffuse foam/spray remains default off;
  - temporal remains default off;
  - wet walls should become default off unless a before/after capture clearly proves the
    default-on effect helps more than it hurts;
  - dense wall fill is handled in Workstream 3.
- Remove/hide only reserved/no-op controls whose tooltips already say they are not wired.
  Preserve safe `set_setting` handling for old ids so persisted configs do not fail.
- Experimental/grouping contract:
  - Phase 1 should prefer grouping and collapsing weak feature tabs/sections over hiding
    whole groups.
  - If the panel needs metadata to distinguish normal versus experimental rows, add a
    small registry-owned field in `config_json` such as `visibility: "normal" |
    "experimental" | "hidden"`.
  - Do not build broad JS-only hardcoded hiding lists for feature groups. A short
    compatibility list for removed reserved/no-op ids is acceptable only if Rust still
    accepts those ids.
  - The default UI should still let a user intentionally enable caustics, temporal,
    diffuse, and wet walls in Phase 1 unless a control is truly reserved/no-op.

Acceptance:

- No weak optional effect is enabled in the default startup path without capture evidence.
- No shipped feature group is removed or fully hidden in Phase 1.
- Reserved/no-op controls are gone from normal UI, or explicitly marked hidden by
  registry-owned metadata, while old ids remain safe to replay.
- Settings tabs/sections make weak effects feel optional/experimental rather than part
  of the required default look.

### 3. Wall-Fill Performance And Quality Regression

Owned files:

- `crates/fluid-lab/src/settings/mod.rs`
- `crates/fluid-lab/src/gpu/wallfill.rs`
- `crates/fluid-lab/src/gpu/shaders/wallfill.wgsl`
- `crates/fluid-lab/src/gpu/composite.rs`
- `crates/fluid-lab/src/gpu/shaders/composite.wgsl`
- `crates/fluid-lab/src/gpu/mod.rs`

Work:

- Make wall fill cheap and conservative by default:
  - likely set `render.hero.flat_water.fill_enabled` default to off, or lower
    `fill_supersample` from 16 to a measured value such as 4 or 8;
  - consider lowering `fill_strength`, `fill_slab`, and fill-only reflection/absorption
    defaults if the sheet is visually heavy;
  - keep the cheaper flat-water normal/depth snap as the first near-wall aid if it looks
    better than the dense fill sheet.
- Ensure disabled wall fill skips costly work. `record_step` already returns before
  dispatch when disabled; verify the render pass and any clears/bind updates do not
  account for the reported slowdown in a meaningful way. If defaulting off, skip the
  injection pass entirely when disabled rather than only drawing a zero-strength pass.
- Use profiler evidence before claiming performance recovery:
  - collect at least three repeated post-warmup samples for current default, Phase 1
    default, and forced wall-fill-on comparison;
  - compare medians, not single frames;
  - capture `gpu.render_ms`, frame/FPS, and console health.
- Do not broaden the wall-fill algorithm unless a small targeted fix is clearly cheaper
  than default-disabling.

Acceptance:

- Default Water view does not show the reported worse wall-fill look.
- Wall-fill disabled/reduced state shows a median render-cost reduction across repeated
  samples, or the implementation report states that the slowdown was not reproduced and
  includes the sample values.
- Near-wall water remains acceptable through flat-water normal/depth correction even if
  dense fill is off by default.
- When `render.hero.flat_water.fill_enabled = 0`, no wall-fill occupancy dispatch or
  injection draw should appear in detailed profiler evidence if detailed pass names are
  available; otherwise the report must explain what evidence proves the skip.

### 4. Settings Navigation, Bottom Controls, And CSS Cleanup

Owned files:

- `web/index.html`
- `web/main.js`
- `web/panels.js`
- `tools/capture.mjs` only if helper names or panel scripting need updates
- `crates/fluid-lab/src/settings/mod.rs` only for tab labels/order/filter metadata

Work:

- Add compact settings navigation for the many functional tabs:
  - desktop: collapsible side navigator or grouped tab list with clear active state;
  - mobile: keep a usable wrapped/stacked version without overlapping the launcher.
- Preserve existing panel behavior: config rows from `config_json`, Profiler tab
  polling only when visible, reset-to-default behavior, localStorage replay, and
  backward-compatible capture helpers.
- Make the bottom launcher fit content on desktop:
  - remove `width: min(760px, 100%)`;
  - use `width: fit-content`/`max-width: calc(100% - ...)`, with mobile wrapping.
- Extract CSS variables in `:root` for recurring values:
  - surfaces: app background, chrome background, panel background/header;
  - borders/shadows;
  - text strong/body/muted;
  - accent, active, warning, danger, success;
  - radii, spacing, control heights, panel width;
  - focus ring.
- Replace hardcoded duplicates in the touched UI areas. Do not attempt a total CSS
  rewrite outside the panel/toolbar/launcher/settings controls.
- Keep the visual style restrained and utility-focused. No marketing hero treatment, no
  decorative gradients/orbs, no nested UI cards.

Acceptance:

- Settings navigation remains usable with all current tabs plus Profiler.
- Bottom Mode/Control launcher is only as wide as its content on desktop and wraps
  without text overflow on narrow screens.
- CSS has a clear token layer and fewer ad hoc one-off colors in the touched rules.
- Capture evidence includes settings panel open and launcher visible.

## Sequencing

1. Snapshot current behavior with named captures/profiler samples before changing
   visuals.
2. Implement the settings schema/control-contract changes first so later render gates use
   the final controls.
3. Retune the core water material defaults and default-disable/regroup weak effects
   without pass removal.
4. Reduce wall-fill default cost and verify disabled/reduced paths.
5. Clean UI navigation, launcher width, and CSS variables.
6. Run gates, capture desktop and narrow viewport states, update docs, then mark this
   plan shipped only after durable facts are migrated.

## Phase 2 Decision Parking Lot

Record Phase 1 evidence here during implementation/review, then ask the user which
follow-up to authorize:

- Remove or hide caustics entirely if still invisible.
- Remove or redesign diffuse foam/spray if it still looks poor.
- Remove or redesign temporal stabilization if it still has no visible value.
- Remove wall-fill pass wiring if default-off plus skip proves it is not worth keeping.
- Rebuild wet-wall visuals if default-off is accepted but the feature remains desired.

## Exit Gate

Required commands:

- `cd /home/adamg/fluid-simulation/app && cargo test --lib`
- `cd /home/adamg/fluid-simulation/app && cargo build --target wasm32-unknown-unknown`

Visible/GPU gates:

- Serve the static app path and use `app/tools/capture.mjs`.
- Use these output paths for Phase 1 visual evidence:
  - `captures/hero_ui_phase1_baseline_default.png` — pre-change default Water view,
    desktop target around 1440x900, after 4-6 seconds.
  - `captures/hero_ui_phase1_default_water.png` — post-change default Water view,
    same scene/camera/wait as baseline.
  - `captures/hero_ui_phase1_refraction_off.png` — post-change with
    `render.hero.refraction_enabled = 0`.
  - `captures/hero_ui_phase1_reflection_off.png` — post-change with
    `render.hero.reflection_enabled = 0`.
  - `captures/hero_ui_phase1_body_color_off.png` — post-change with
    `render.hero.body_color_enabled = 0`.
  - `captures/hero_ui_phase1_wallfill_default.png` — post-change default wall-fill
    state.
  - `captures/hero_ui_phase1_wallfill_forced_on.png` — same scene with
    `render.hero.flat_water.fill_enabled = 1`.
  - `captures/hero_ui_phase1_settings_desktop.png` — desktop target around 1440x900,
    settings navigator open on Water Surface.
  - `captures/hero_ui_phase1_settings_wallfill.png` — settings navigator open on Wall
    Fill or its grouped/collapsed equivalent.
  - `captures/hero_ui_phase1_settings_profiler.png` — settings navigator open on
    Profiler.
  - `captures/hero_ui_phase1_settings_narrow.png` — narrow target around 390x844 or the
    closest harness-supported mobile viewport, settings open and bottom launcher visible.
- For wall-fill performance claims, collect three repeated post-warmup profiler samples
  each for:
  - pre-change baseline default, if available;
  - post-change Phase 1 default;
  - forced wall-fill-on.
  Report the three `gpu.render_ms` values and the median for each state. Use detailed
  profiling if needed to prove disabled-path skip.

Console/health expectations:

- `navigator.gpu` present, smoke PASS, no WGSL validation/device errors.
- Any EVAL-driven capture must echo the expected harness label in the console log.
- Performance claims quote profiler output, not visual impressions.

Visual acceptance:

- Default water is materially less transparent and reads as a coherent liquid body.
- Refraction, reflection, and body-color toggles each produce a distinct, understandable
  change without disabling unrelated controls.
- Weak optional effects are not enabled in the default user path.
- Wall fill no longer dominates visual quality or frame cost by default.
- Settings and bottom controls feel intentional, compact, and navigable.
- Desktop settings capture shows a usable navigator and no overflow in tab/control text.
- Narrow settings capture shows no incoherent overlap among toolbar, panel, canvas, and
  bottom launcher.

## Discipline Rules

- Diagnose with capture before changing defaults, but do not delete pass wiring or hide
  whole shipped feature groups in Phase 1.
- Prefer default-disabling and grouping/collapsing over deep rewrites for weak features
  in this pass.
- Keep setting ids stable where possible; if removing visible settings, preserve safe
  handling of persisted localStorage values and old `set_setting` calls.
- Do not make performance claims without profiler evidence.
- Do not add normal-frame readback.
- Do not touch unrelated dirty files except where required by this plan.

## Migration Notes

Migrated at ship time:

- `architecture/rendering.md` records the split hero-water optical controls, hidden
  legacy `render.hero.mode_enabled` behavior, optional wall-fill injection, default-off
  weak feature groups, and wall-fill skip behavior.
- `architecture/settings.md` records the new `tab_group`/`tab_variant` metadata,
  visible replacement controls, hidden legacy/reserved ids, and default-off optional
  render effects.
- `architecture/web-shell.md` records the grouped/collapsible settings navigator,
  bottom launcher fit-content sizing, hidden legacy replay behavior, and CSS token
  policy.
- `architecture/gpu-resources.md` records the lower default wall-fill atlas
  supersample and default-off occupancy/injection behavior.
- `decisions/rendering.md` records that weak hero-water add-ons are opt-in startup
  features, not abandoned features.
- `decisions/performance.md` records dense wall fill as opt-in until profiler data
  justifies default cost.

Verification evidence:

- Code gates: `cargo test --lib` passed with 33 tests; `cargo build --target
  wasm32-unknown-unknown` passed; `node --check web/panels.js`, `node --check
  web/main.js`, and `node --check tools/capture.mjs` passed.
- Browser captures: `captures/hero_ui_phase1_default_water.png`,
  `captures/hero_ui_phase1_refraction_off.png`,
  `captures/hero_ui_phase1_reflection_off.png`,
  `captures/hero_ui_phase1_body_color_off.png`,
  `captures/hero_ui_phase1_wallfill_default.png`,
  `captures/hero_ui_phase1_wallfill_forced_on.png`,
  `captures/hero_ui_phase1_settings_desktop.png`,
  `captures/hero_ui_phase1_settings_wallfill.png`,
  `captures/hero_ui_phase1_settings_profiler.png`, and
  `captures/hero_ui_phase1_settings_narrow.png`.
- Capture health: all named Phase 1 captures reported `navigator.gpu present: true`,
  atomic smoke PASS, and no page/request/WGSL/device errors in console logs. EVAL-driven
  captures echoed the expected labels.
- Visual spot-check: default water is visibly more opaque/coherent; the desktop
  settings navigator is grouped and usable; the narrow `390x844` settings capture has no
  incoherent overlap with the bottom launcher; the bottom launcher fits its contents on
  desktop.
- Wall-fill samples: default/off `gpu.render_ms` samples were `0.479`, `0.919`,
  `0.495` (median `0.495`); forced-on samples were `0.514`, `0.885`, `0.766` (median
  `0.766`). The top-level render total is noisy, but the default-off path avoids the
  dense wall-fill visual by default and code review confirmed the occupancy dispatch and
  injection draw are skipped when disabled, with only the mask clear remaining.

## See Also

- `docs/plans/hero-water-delivery-hub.md`
- `docs/plans/app-settings-panel-overhaul.md`
- `docs/plans/wall-wetness-fill-polish.md`
- `docs/architecture/rendering.md`
- `docs/architecture/settings.md`
- `docs/architecture/web-shell.md`
