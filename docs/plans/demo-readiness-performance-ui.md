---
status:        draft
owner:         adamg
last_updated:  2026-06-24
okay_to_delete: false
long_lived:    false
owning_docs:
  - architecture/app-shell.md
  - architecture/settings.md
  - architecture/profiler.md
  - architecture/rendering.md
  - architecture/web-shell.md
  - decisions/performance.md
  - decisions/rendering.md
---

# Demo readiness, performance presets, and UI polish

## Mission

Make the live app suitable for manual high-quality video capture and ordinary startup
use on weaker gaming laptops. Done means the app has honest slow-simulation support,
four quality presets with startup auto-selection targeting at least 30 FPS/TPS, an
option to hide or fade the two matte back tank walls, a corrected settings-panel
splitter hit target, and a professional visual polish pass that preserves the current
layout.

The earlier traffic/intersection/grid-snapping notes are explicitly out of scope; they
belong to a different project.

## Scope

In scope:

- Add a Live simulation-speed/time-scale setting for manual capture. It slows wall-clock
  simulation advance without changing physics constants such as fixed dt, gravity, CFL,
  pressure iterations, or solver behavior.
- Add four quality presets, initially named Performance, Balanced, Quality, and Ultra.
  Presets may set reset-class scale controls such as grid resolution and particle
  density, plus selected quality/cost controls such as pressure iterations and water
  smoothing.
- Auto-select a startup quality preset before the first meaningful frame. Explicit URL
  settings, imported config, or saved localStorage settings override auto-selection.
- Make the two matte tank walls drawn by the environment prepass optionally see-through.
  The floor, wireframe, simulation boundary walls, and procedural world background stay
  separate.
- Fix the settings panel resize thumb so only the visible rectangular grabber starts
  pointer resizing, while keyboard resizing remains accessible.
- Improve typography, spacing, hierarchy, control styling, and text fit while preserving
  the existing right-side panel, tabs, toolbar, bottom controls, and vanilla shell.

Out of scope:

- Offline or in-app high-quality video generation. That is tracked separately in
  `video-generation.md`.
- Solver rewrites, new simulation features, or performance claims without profiler
  evidence.
- A structural redesign of the settings layout, tab model, panel placement, or product
  launcher.
- Changing physical tank-wall collision behavior when visual wall opacity changes.

## Approach

### Stream 1: slow sim and quality presets

Implement slow simulation as a separate time-scale control, not by changing
`physics.fixed_dt` and not by using render FPS throttling. `FluidApp::frame` should keep
profiler frame timing tied to raw rAF wall time, while scheduled interactions and the
timestep accumulator consume scaled simulation time. Profiler stats should expose the
active simulation speed so `real_time_factor` remains honest.

Four presets should be data-driven enough that the shell can apply them before the rAF
loop starts. The initial preset values should be treated as measured tuning targets:
Performance below the current default, Balanced as the weak-laptop default candidate,
Quality near the current visual baseline, and Ultra above current default for strong
machines or manual capture. Startup auto-selection may begin as a conservative heuristic
from adapter/capability and viewport facts, but the implementation must not present it
as a reliable benchmark unless it actually measures.

Likely owned files:

- `app/crates/fluid-lab/src/lib.rs`
- `app/crates/fluid-lab/src/timestep.rs`
- `app/crates/fluid-lab/src/settings/mod.rs`
- `app/crates/fluid-lab/src/profiler/mod.rs`
- `app/crates/fluid-lab/src/gpu/mod.rs`
- `app/web/main.js`
- `app/web/panels.js`

### Stream 2: tank back-wall opacity

Reuse the existing `render.hero.wall_visibility` path if practical, but make its
behavior match the product meaning: opacity/visibility for the two environment wall
quads, not wall brightness. The Environment tab is the right conceptual home because
this affects tank/background scene setup rather than water optics.

At value `0`, the back and left matte walls should not visually block the world
background/prepass color. At value `1`, the walls should match today's fully visible
look as closely as possible. The floor remains opaque and patterned, and the wireframe
outline remains visible. If true partial opacity requires depth/refraction semantics
that become misleading, implement a robust hide/show first or document a limited visual
fade rather than pretending blended scene depth is physically correct.

Likely owned files:

- `app/crates/fluid-lab/src/settings/mod.rs`
- `app/crates/fluid-lab/src/gpu/environment.rs`
- `app/crates/fluid-lab/src/gpu/shaders/environment.wgsl`
- `app/crates/fluid-lab/src/gpu/mod.rs` only if floor/wall draws must split

Open implementation choice: decide whether the control should be continuous opacity or
a visibility/hide threshold. Recommended first pass is value `0` = hidden, value `1` =
current, and mid values only if they can avoid dishonest depth/refraction behavior.

### Stream 3: splitter and UI polish

Fix the splitter by making the visible grabber rectangle the interactive element. The
current model uses a full-height flex strip as the hit target and a smaller pseudo
element as the visual thumb, so resizing starts from invisible areas. The corrected
desktop behavior should center the thumb on the canvas/panel boundary, with roughly
half its width over each side, no visible gap, pointer drag only from the thumb, and
keyboard resizing still available through the separator.

The polish pass should primarily be CSS in `app/web/index.html`, with small
`app/web/panels.js` updates only where accessibility/state requires it. Preserve the
layout. Improve the visual system: professional UI font stack, clearer text hierarchy,
consistent radii/borders/focus states, stable control dimensions, restrained colors,
tabular numeric/stat text only where helpful, clean wrapping at the minimum panel
width, and no overlapping text.

Likely owned files:

- `app/web/index.html`
- `app/web/panels.js`

## Exit gate

- `cd /home/adamg/fluid-simulation/app && cargo test --lib`
- `cd /home/adamg/fluid-simulation/app && cargo build --target wasm32-unknown-unknown`
- A real-browser capture via `tools/capture.mjs` against `http://localhost:5184/` after
  visible/GPU changes.
- Manual or captured evidence that a slow-sim setting such as `0.25` advances roughly
  one quarter as much simulation time per wall second while preserving fixed physics
  step size.
- Manual or captured evidence that startup auto-selection chooses a lower preset when
  appropriate, and explicit URL/localStorage settings override it.
- Visual checks at desktop and narrow widths: splitter hit target, panel fit, no text
  overlap, and preserved mobile overlay behavior.
- Visual checks for wall opacity at `0` and `1`; if partial opacity ships, also check a
  mid value.

## Discipline rules

- Do not claim a preset meets 30 FPS/TPS without profiler or capture evidence from the
  relevant machine or an explicitly named proxy.
- Do not use `render.fps_target` as the slow-simulation control; render cadence and
  simulation time scale are separate concerns.
- Keep user config authoritative over auto-selection.
- Keep the Environment tab visibility decision explicit. If this wall-control option
  should be user-facing outside `?dev=true`, update the shell/docs deliberately.
- Preserve accessibility for the splitter while narrowing the pointer hit target.

## Implementer briefs

### Slow sim and presets

Goal:
Add a Live simulation-speed control, four quality presets, and startup auto-selection
that targets at least 30 FPS/TPS on weaker gaming laptops.

Non-goals:
No offline video renderer, no fixed-dt trickery for slow motion, no solver rewrite.

Authoritative docs:
`architecture/app-shell.md`, `architecture/settings.md`, `architecture/profiler.md`,
`architecture/web-shell.md`, `decisions/performance.md`.

Expected behavior:
At `1.0`, current timestep behavior is preserved. At `0.25`, wall-clock capture sees
quarter-speed motion while each executed physics substep still uses the normal fixed
dt. Profiler output includes enough data to distinguish slow motion from performance
overload.

Cheapest sufficient checks:
Native timestep tests for scaled time accounting; registry tests for the new Live row;
shell checks that explicit config beats auto preset; capture/manual profiler evidence
for 30+ target behavior.

Stop and report if:
Auto-selection needs reliable performance prediction but only weak adapter-name
heuristics are available, a preset reset fails GPU preflight, or the video-generation
request starts expanding into offline rendering.

### Tank back-wall opacity

Goal:
Make the two environment wall quads optionally see-through from a visible setting.

Non-goals:
No background change, no physical wall collision change, no removed render feature
revival.

Authoritative docs:
`architecture/rendering.md`, `architecture/settings.md`, and `architecture/gpu-resources.md`
if render target or depth behavior changes.

Expected behavior:
Wall opacity `0` removes matte back/left wall fill; wall opacity `1` matches current
look; floor and wireframe remain visible.

Cheapest sufficient checks:
Settings JSON still exposes the control in the intended tab; visual capture/manual
check in Water mode at `0` and `1`.

Stop and report if:
True partial transparency requires changing water refraction/depth semantics beyond a
visual wall fade, or if the control must affect non-Water render modes too.

### Splitter and UI polish

Goal:
Fix the settings splitter hit target and make the existing UI look professional without
changing layout.

Non-goals:
No tab/layout redesign, no new settings model, no mobile overlay change.

Authoritative docs:
`architecture/web-shell.md`, `architecture/settings.md`.

Expected behavior:
Only the visible thumb rectangle starts pointer resizing; keyboard resizing and
double-click reset still work; UI text and controls fit at minimum panel width.

Cheapest sufficient checks:
Desktop manual hit test around the splitter, narrow-width check below `820px`, and
visual pass for wrapping/overlap across the settings panel.

Stop and report if:
The requested hit target cannot be narrowed without losing keyboard accessibility, or
the polish work expands into structural redesign.

## Migration notes (filled in at ship time)

Before shipping, migrate final current-state facts into:

- `architecture/app-shell.md` for timestep/time-scale frame-loop behavior.
- `architecture/settings.md` for new/changed registry rows, presets, and tab exposure.
- `architecture/profiler.md` for any `stats_json` additions.
- `architecture/rendering.md` for wall-opacity render behavior.
- `architecture/web-shell.md` for startup preset selection, splitter behavior, and UI
  shell contracts.
- `decisions/performance.md` if runtime quality presets supersede the current
  "measurement output, not runtime system" decision.
- `decisions/rendering.md` if wall opacity introduces a render-policy trade-off.

## See also

- `docs/plans/index.md`
- `docs/plans/video-generation.md`
- `~/.agentdocs/plan-lifecycle.md`
