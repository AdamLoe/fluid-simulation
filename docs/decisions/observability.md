---
status:        active
owner:         adamg
last_updated:  2026-06-05
---

# Decisions — Observability

## The core product is observability-first

**Decision** — The project is framed as an observability-first, highly configurable
3D fluid lab: the sim is real enough to inspect, and its performance and parameters
are explainable and tunable. The split is deliberate: the **instrumentation data
model** (hierarchical profiler + console logging + typed config registry) remains the
source of truth, while the rendered config/profiler side panels are consumers of that
model.

**Why** — The distinctive portfolio value is exposing how the sim works and where time
goes — it reads as a systems tool, not a graphics toy. The visible panels should make
that instrumentation inspectable without creating a second source of truth.

**Tradeoffs** — The MVP carries more instrumentation than a pure visual demo, but not
more rendered UI; measurements become reproducible from the first GPU loop.

**Applies to** — `architecture/profiler.md`, `architecture/settings.md`, `architecture/web-shell.md`.

## Configuration flows through one schema-driven registry

**Decision** — Every tunable parameter is declared once in a single typed,
schema-driven registry. UI controls are rendered from the registry, never hand-wired
against ad-hoc state.

**Why** — The sim has many knobs; a registry keeps Rust config, defaults, validation,
persistence, help copy, grouping, and runtime behavior from drifting apart, and lets
profiler samples be tagged with a config snapshot.

**Code anchors** — `crates/fluid-lab/src/settings/mod.rs → Registry`; bridge `crates/fluid-lab/src/lib.rs → config_json,
set_setting, stats_json`.

**Applies to** — `architecture/settings.md`.

## Help copy and functional tabs are schema metadata

**Decision** — Functional help (`tooltip`), technical help (`technical_tooltip`), and
product-facing settings tabs are explicit registry metadata. The panel may render rows
with no help, functional help only, or functional plus technical help, and it groups
controls by registry-owned functional tabs instead of JavaScript-only buckets.

**Why** — A single long help string makes every row heavier and hides technical detail
inside prose conventions. Explicit optional fields let obvious rows stay quiet,
technical rows remain inspectable, and the Rust registry remain the source of truth for
where controls belong.

**Code anchors** — `crates/fluid-lab/src/settings/mod.rs → Setting`,
`settings_tab`, `Registry::config_json`; `web/panels.js → buildConfigPanel`,
`appendHelpIcons`.

**Applies to** — `architecture/settings.md`, `architecture/web-shell.md`.

## Every setting declares an apply class (live / reset / reload)

**Decision** — Each setting is Live (applies immediately), Reset (needs buffer
reallocation / scene rebuild), or Reload (needs page/device reload). The apply class
is a property of the registry data model from the start; an unsafe setting is never
forced to apply live.

**Why** — Some settings mutate safely live; others require expensive rebuilds.
Encoding this in the schema prevents fragile "apply everything live" hacks and makes
the eventual UI honest.

**Applies to** — `architecture/settings.md`, `architecture/gpu-resources.md` (the
`recreate_fluid` rebuild path Reset settings trigger).

## Setting writes must report their real outcome

**Decision** — The JS bridge exposes an honest setting-mutation result instead of
forcing callers to infer meaning from a boolean. Accepted finite values report the
requested value, stored value, whether validation clamped it, the registry apply
class, whether a Live side effect was pushed, and whether reset/reload is required.
Unknown ids and non-finite values are rejected distinctly. Legacy ids report
`legacy_mapped` or `legacy_ignored` instead of disappearing in JavaScript.

**Why** — A single boolean hid important differences: clamped values looked
unmodified, Reset/Reload settings looked like failures, and stale saved ids could
drift between JS and Rust. The registry is the only durable config authority, so the
bridge result needs to carry the registry's answer all the way to persistence, URL
import, and UI badges.

**Code anchors** — `crates/fluid-lab/src/settings/mod.rs → SettingMutationResult`;
`crates/fluid-lab/src/lib.rs → set_setting_result_json`.

**Applies to** — `architecture/settings.md`, `architecture/web-shell.md`.

## The profiler is hierarchical, config-tagged, and timing-source honest from the start

**Decision** — The profiler supports nested scopes (Frame → Simulation → substep →
P2G/forces/divergence/pressure/gradient/G2P/advection → render) from the beginning.
Every sample and capture carries the active config snapshot and an explicit timing
source: GPU timestamp / coarse fence / CPU fallback / unavailable.

**Why** — Flat timing labels are useless once a pass gets expensive, and numbers
without config context are uninterpretable. Explicit timing source prevents displaying
fake precision: in-browser `timestamp-query` is frequently unavailable or quantized
(~100µs) and CPU wall-clock around an async submit measures nothing — so per-pass GPU
timing may not exist, and when it doesn't the profiler shows a clearly-labeled
minimum-honest fallback and **never fabricates per-pass numbers**.

**Applies to** — `architecture/profiler.md`.

## GPU profiling is coarse by default; detailed mode is Reset-class diagnostics

**Decision** — `GpuTimers` defaults to coarse mode: three monolithic pass groups (prep / pressure / finish) per substep, with frame totals summed correctly across all substeps that ran. Detailed mode (one begin/end pair per fine section + per-CG-iteration timing) is off by default and toggled via `dev.detailed_gpu_profiling` (Reset-class, default 0), rendered as Diagnostics rather than user-facing dev chrome.

**Why** — Detailed mode sizes the `QuerySet` from `max_substeps × pressure_iters` (capped at 8192 slots) and adds some GPU overhead from the extra timestamp writes. The coarse default gives honest aggregate timings — each substep owns its own query slots so frame totals are correct aggregates — without the query-set bloat or the overhead cost. Detailed mode is a power-user / profiling-session tool, not always-on instrumentation.

**Tradeoffs** — Coarse mode gives honest frame-total aggregates but cannot attribute time to individual passes within a substep. Detailed mode can attribute to 27 named sections and the four CG categories (spmv, reductions=both dots, updates=vector update, scalars=alpha/beta/dir), but carries a Reset cost and may truncate timed CG iters if the query budget is exceeded (logged, never silent).

**Code anchors** — `app/crates/fluid-lab/src/gpu/timing.rs → GpuTimers`, `FINE_SECTIONS`, `CG_CATS`, `CG_BUCKET`; `app/crates/fluid-lab/src/settings/mod.rs → dev.detailed_gpu_profiling`.

**Applies to** — `architecture/profiler.md`, `architecture/gpu-resources.md`.

## Capture gates assert only source-honest observability fields

**Decision** — The browser capture harness may fail builds on explicit stats budgets
and scale status, but timestamp-specific GPU pass budgets only apply to final samples
whose `timing` is `gpu-timestamp` and whose `gpu` block is non-null.

**Why** — Acceptance gates should turn profiler data into repeatable signals without
smuggling in fake precision on adapters that expose only the CPU fallback.

**Code anchors** — `app/tools/capture.mjs → collectAssertionFailures`;
`app/crates/fluid-lab/src/profiler/mod.rs → Profiler::stats_json`.

**Applies to** — `architecture/profiler.md`.

## Hot-pass thresholds are configurable; 100ms is a stall threshold, not the only one

**Decision** — The profiler exposes multiple thresholds: hot GPU pass ~2 ms (with real
timestamps), slow sim step ~8–16 ms, long frame ~33 ms, stall/deep-drilldown ~100 ms.

**Why** — A real-time sim targeting 30–60 FPS needs small per-pass thresholds; a lone
100 ms threshold only catches catastrophic stalls, not real-time bottlenecks.

**Applies to** — `architecture/profiler.md`.

## See also

- [`../architecture/profiler.md`](../architecture/profiler.md) · [`../architecture/settings.md`](../architecture/settings.md)
- [`rendering.md`](rendering.md) · [`performance.md`](performance.md)
- [`../agent-context/maintaining-docs.md`](../agent-context/maintaining-docs.md)
