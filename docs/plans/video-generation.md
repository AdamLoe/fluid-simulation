---
status:        draft
owner:         adamg
last_updated:  2026-06-24
okay_to_delete: false
long_lived:    false
owning_docs:
  - architecture/app-shell.md
  - architecture/rendering.md
  - architecture/profiler.md
  - architecture/web-shell.md
  - decisions/performance.md
  - decisions/rendering.md
  - decisions/scope.md
---

# High-quality video generation

## Mission

Add an in-app or app-adjacent way to generate extremely high-quality renders or videos
from the fluid simulation. This is separate from real-time demo readiness because it
may need deterministic stepping, frame export, supersampling, non-realtime render
loops, and different acceptance gates than the live interactive app.

Done means the product can generate a high-quality output sequence or video through a
clear workflow, with honest progress/status reporting and without weakening the normal
real-time frame loop.

## Scope

In scope:

- Define the target workflow for generated output: image sequence, encoded video, or
  both.
- Add deterministic stepping/export hooks that can advance simulation and render frames
  for capture without depending on wall-clock rAF cadence.
- Support higher-quality render settings than the real-time startup presets can safely
  default to.
- Report enough metadata with the output to know preset, resolution, sim speed, frame
  count, and timing source.

Out of scope for this plan until explicitly chosen:

- Using `render.fps_target` as a video generator.
- Replacing the existing `tools/capture.mjs` acceptance harness.
- Server-side/cloud rendering.
- Solver rewrites purely for video output.

## Approach

Start with product shape before implementation. The key decision is whether video
generation is:

- a browser UI workflow that records frames from the live canvas,
- a headless capture-harness workflow that emits image sequences,
- or an offline/non-realtime app mode with explicit stepping and frame export.

Recommended direction: use the demo-readiness work only for lightweight hooks and
honest stats, then build video generation as an explicit non-realtime export workflow.
The generator should not rely on browser rAF to decide simulation progress. It should
own frame count, output FPS, simulation seconds per output frame, quality preset,
viewport/output resolution, and output destination.

Likely owned files after product direction is chosen:

- `app/crates/fluid-lab/src/lib.rs` for explicit stepping/export bridge methods.
- `app/crates/fluid-lab/src/timestep.rs` if export needs a separate deterministic
  stepping policy.
- `app/crates/fluid-lab/src/profiler/mod.rs` for export metadata.
- `app/crates/fluid-lab/src/gpu/mod.rs` and render modules if offscreen/supersampled
  render targets are needed.
- `app/web/main.js`, `app/web/panels.js`, and `app/web/index.html` if this becomes a
  browser UI workflow.
- `app/tools/capture.mjs` or a new tool under `app/tools/` if this becomes a headless
  sequence generator.

## Exit gate

The exit gate depends on the chosen workflow, but should include:

- A deterministic short export test that produces a known number of frames.
- Metadata proving output FPS, simulation duration, quality preset, and render size.
- A real-GPU capture/export check using the same honesty rules as
  `agent-context/build-run.md`.
- A manual visual review of generated frames or video at the target high-quality preset.

## Discipline rules

- Keep this separate from real-time startup preset selection and manual slow-sim capture.
- Do not make video quality claims without generated output and profiler/capture data.
- Do not hide long-running generation behind the normal interactive rAF loop without
  progress, cancellation, and clear output state.

## Open decisions

- Output type: PNG sequence, WebM/MP4, or both.
- Workflow owner: browser UI, headless tool, or explicit offline mode.
- Whether output should supersample/accumulate frames or only use higher render
  settings.
- Whether generation must be deterministic across machines or only reproducible within
  one run/environment.
- Whether audio, camera paths, scripted tank motion, or timeline editing are part of
  the first version.

## Migration notes (filled in at ship time)

Before shipping, migrate final facts into:

- `architecture/app-shell.md` for explicit stepping or export-loop behavior.
- `architecture/rendering.md` for offscreen/high-quality render behavior.
- `architecture/profiler.md` for export metadata and timing-source reporting.
- `architecture/web-shell.md` for any browser or tool workflow.
- `decisions/performance.md`, `decisions/rendering.md`, or `decisions/scope.md` for
  durable trade-offs.

## See also

- `docs/plans/index.md`
- `docs/plans/demo-readiness-performance-ui.md`
- `docs/agent-context/build-run.md`
- `~/.agentdocs/plan-lifecycle.md`
