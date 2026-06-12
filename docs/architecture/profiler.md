---
status:        active
owner:         adamg
last_updated:  2026-06-12
okay_to_delete: false
long_lived:    true
---

# Profiler

The profiler is core infrastructure, not late polish. It owns a hierarchical,
config-tagged, timing-source-honest view of every frame — console logging and the
rendered panel consume the same `stats_json` data. Every performance number it emits
is uninterpretable without knowing what was measured and how, so timing-source honesty
is the single load-bearing design rule.

## What it owns

`app/crates/fluid-lab/src/profiler/mod.rs` owns:
- the `Profiler` struct — scope tree, rolling frame-time window, substep count, GPU sample cache, timestep stats, per-frame structural facts
- `scope_begin` / `scope_end` — accumulate CPU wall-clock time per named scope across a logging window, then reset
- `begin_frame` / `end_frame_and_maybe_log` — push a frame-time sample, emit console log every ~3 s tagged with the active config snapshot
- `stats_json` — serialize live state for the JS bridge and the rendered panel (see "stats_json shape" below)
- `set_timestep_stats(stats, total_dropped)` / `set_frame_facts(...)` — setters called from `FluidApp::frame` each tick
- `TimingSource` enum — `CpuWallClock | GpuTimestamp | CoarseFence`; `set_gpu_sample` switches the active source to `GpuTimestamp` and caches the per-pass numbers, including the substep count owned by the sampled readout

`app/crates/fluid-lab/src/gpu/timing.rs` owns:
- `GpuTimers` — constructed with `(max_substeps, detailed, pressure_iters)`; sizes its `QuerySet` so each substep owns its own slots, ensuring coarse frame totals are correct aggregates across all substeps that ran
- `record_resolve_and_maybe_copy` — resolves timestamps every frame; copies to the mappable read buffer **throttled** every 20 frames to avoid per-frame stall
- `map_readback` — async-maps the read buffer; on completion writes a `Readout` into an `Rc<Cell<Readout>>` and calls `unmap`; `pending` guard prevents overlapping maps
- the liveness counter (`liquid_cells`) is read back in the same throttled buffer copy, not in a separate readback
- surface-foam counters are read back in the same throttled buffer copy; visible
  profiler text reports foam alive/emitted/clamped only, while legacy JSON keys for
  spray and bubble remain present as zeroes

## Scope model & timing sources

Intended hierarchy (grows child scopes without restructuring):

```
Frame
  update (CPU wall-clock)
  sim substep × N
    p2g.scatter / p2g.normalize
    forces / divergence / pressure / gradient
    g2p / advect
  render (CPU wall-clock)
  diagnostics
```

Currently populated scopes are CPU wall-clock only (encode time, not GPU execution time). GPU per-pass execution times come through the separate `GpuTimers` readback path and are reported as a distinct block in the log, labeled `gpu-timestamp`.

**Timing source rules:**

- `GpuTimestamp` — real timestamp-query results from `gpu::timing`; available on HeadlessChrome/Dawn (this adapter); quantized ~100 µs on some browsers/adapters
- `CpuWallClock` — `performance.now()` around CPU-side work; always available; measures CPU encode time, not GPU execution time
- `CoarseFence` — one throttled `onSubmittedWorkDone`-style fence for a coarse sim-vs-render split; labeled as coarse; a future fallback path

CPU wall-clock around a GPU submit measures nothing about GPU execution time because submission is async. The profiler does not conflate them.

## GpuTimers modes

`GpuTimers` operates in two modes, selected at construction (Reset-class; controlled by `dev.detailed_gpu_profiling`):

**COARSE (default):** each substep gets three begin/end pairs — prep / pressure /
finish — plus one render pair for the frame. The render pair writes its begin timestamp
on the first render pass and its end timestamp on the final render pass, so
`gpu.render_ms` is one coarse total for the whole render path. `Readout.prep_ms`,
`pressure_ms`, `finish_ms` are **frame totals** summed across all substeps that ran.

**DETAILED (dev toggle):** each substep gets one begin/end pair per fine section. Fine sections are the 27 passes in `app/crates/fluid-lab/src/gpu/timing.rs → FINE_SECTIONS`. In addition, per CG iteration, six contiguous passes are timed and bucketed into four reported categories: `spmv` (SpMV pass), `reductions` (both dot-product passes — d·q and r·r), `updates` (the vector update p += α·d; r -= α·q), `scalars` (the alpha/beta/dir one-thread dispatches). All values are frame totals summed across substeps.

**Query-set sizing.** The `QuerySet` is sized at construction from `max_substeps × pressure_iters`, capped at 8192 slots. If a large dev config would exceed the cap, the timed CG iters are reduced and the reduction is logged via `crate::log()` — never silently. If live `pressure_iters` later exceeds the reset-time allocation, timed iters are clamped and logged once via `clamp_cg_iters`.

## stats_json shape

Always-present keys additionally include measurement/scale facts:
`frame_samples`, requested/estimated/actual particles, `scale_status`,
`max_compute_workgroups_per_dimension`, `max_particle_dispatch_count`,
`particle_dispatch_groups_x`, `particle_dispatch_groups_y`,
`particle_dispatch_capacity`, `max_particle_storage_count`,
`pressure_iterations`, and `render_mode`, alongside the existing frame percentile,
grid, memory, dropped-time, dispatch, and GPU fields.

The `gpu` sub-object always carries: `sim_ms`, `prep_ms`, `pressure_ms`,
`finish_ms`, `render_ms`, `liquid_cells`, `substeps`, `detailed`. When `detailed` is
true it also carries `sections` (name→ms map) and `cg` (`total_ms`,
`avg_ms_per_iter`, `spmv_ms`, `reductions_ms`, `updates_ms`, `scalars_ms`, `iters`).
Fine fields are only present when real timestamps and the dev toggle are both active
— never fabricated. When timestamp readback includes foam counters, `gpu.diffuse`
carries `alive`, `foam`, `spray`, `bubble`, `emitted`, `clamped`, and
`compute_timed`; `spray` and `bubble` stay as compatibility zeroes.
`compute_timed:false` means diffuse compute work is intentionally outside the
timestamp totals, so foam counter visibility must not be read as a timed cost.

## Non-obvious invariants and gotchas

**Timing-source honesty is non-negotiable.** Every logged sample declares its source (`timing: gpu-timestamp` or `timing: cpu-wallclock`). Per-pass GPU numbers (`prep / pressure / finish / render`) are emitted only when `GpuSample` is set — if it is `None`, the GPU block is absent from the log and `"gpu": null` in `stats_json`. The profiler never fabricates per-pass numbers when timestamps are missing.

**`timestamp-query` is not universally available.** In-browser it is often gated behind a flag or quantized to ~100 µs. The fallback minimum-honest profile is: total frame time (CPU rAF delta, always available), substep count, dispatch/draw counts, optional coarse fence for sim-vs-render split. These are clearly labeled; a gate asking for "top-5 GPU costs" is satisfied by the labeled fallback when the platform cannot provide timestamps.

**GPU readback is throttled, never per-frame.** `record_resolve_and_maybe_copy` → `app/crates/fluid-lab/src/gpu/timing.rs → GpuTimers::record_resolve_and_maybe_copy` copies to the mappable buffer only every 20 frames, and only when no map is already `pending`. Normal frames skip the copy. This is the only allowed readback class.

**Scope accumulators reset on log emit.** `end_frame_and_maybe_log` resets all `total_ms` and `calls` after printing, so reported values are per-frame averages over the logging window, not lifetime totals. The `frame_window` rolling buffer (cap 240) is not reset — it persists for percentile computation.

**A successful Reset starts a clean measurement window.** `Profiler::reset_measurement`
clears the rolling frame window, cached GPU sample/timing source, timestep snapshot,
and CPU scope accumulators after `GpuContext::recreate_fluid` succeeds. Rejected
Reset attempts leave the active measurement window intact because the active fluid
did not change.

**Scale facts come from the active GPU context, not estimates in JS.**
`FluidApp::frame` feeds requested/estimated/actual particles, tiled dispatch shape,
dispatch/storage ceilings, and `scale_status` from `GpuContext` into
`Profiler::set_frame_facts`. The profiler reports what the running or rejected Reset
attempt actually asked for; the panel does not derive those fields independently.

**The rendered panel sorts measured costs.** Coarse prep/pressure/finish/render rows
and detailed section rows are descending by their real timestamp values. It does not
hardcode pressure or any other pass as dominant.

**Config snapshot is caller-supplied.** The profiler receives the snapshot string at log time via `end_frame_and_maybe_log(config_snapshot)` rather than holding a reference to settings. This makes the profiler independent of the settings crate and prevents stale snapshots.

**Thresholds are named, not implicit.** Policy in `../decisions/observability.md`:
- hot GPU pass: ~2 ms (with real timestamps)
- slow sim step: ~8–16 ms
- long frame: ~33 ms
- stall / deep-drilldown: ~100 ms — this is the stall threshold; 100 ms is not the only threshold

**`liquid_cells` and foam liveness.** The occupied-cell count and diffuse counter
slots are read back in the same throttled copy as the timestamps
(`app/crates/fluid-lab/src/gpu/timing.rs -> GpuTimers::record_resolve_and_maybe_copy`).
`liquid_cells` is a single `u32`; foam counters are cursor/emitted/clamped/alive foam
with legacy spray/bubble slots forced to zero. `FluidApp::frame` writes the frame's
actual substep count into `GpuTimers` before render, including zero-substep paused
frames, so the readout owns the sampled substep count rather than borrowing the
profiler's current frame count.

## Update when

- A new GPU pass is added → add it to `FINE_SECTIONS` (detailed path) or the coarse pass split (coarse path) in `app/crates/fluid-lab/src/gpu/timing.rs`; update the `Readout` struct and the readback aggregation in `map_readback`; update `stats_json` and the panel consumer in `web/panels.js`
- A new CPU scope is added → call `scope_begin` / `scope_end` with a unique `&'static str`; depth is inferred from `open_stack`
- The logging interval or rolling-window cap change → `Profiler::new` in `app/crates/fluid-lab/src/profiler/mod.rs`
- The JS bridge schema for `stats_json` changes → update `Profiler::stats_json` and the panel consumer in `web/panels.js`
- The `GpuTimers` construction parameters change (mode, substeps, iters) → `gpu-resources.md` owns the rebuild path; note here if the `stats_json` shape changes
- Scale-status or particle-dispatch fact fields change → update both `stats_json` and
  the panel consumer wording.

## See also

- `gpu-resources.md` — buffer layout for `stats_buf` (liveness counter) that `GpuTimers` reads back
- `settings.md` — config snapshot format passed to `end_frame_and_maybe_log`
- `web-shell.md` — the rendered profiler panel that consumes `stats_json`
- `../decisions/observability.md` — why data model and logging are early infrastructure, rendered panel stays separate
- `../agent-context/maintaining-docs.md`
