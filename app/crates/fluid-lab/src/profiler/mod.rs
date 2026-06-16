//! Hierarchical profiler — Phase 0.1 skeleton.
//!
//! Per `decisions.md` (profiler is hierarchical and config-tagged from the start)
//! and the observability split: the *data model + console logging* are early
//! infrastructure (here), the *rendered panel* is 1.2.
//!
//! 0.1 populates only top-level CPU scopes (update/render) under Frame. The nested
//! scope machinery and config-snapshot tagging exist now so 0.3 can grow child
//! scopes (P2G → scatter/normalize, pressure, …) without restructuring. Timing is
//! CPU wall-clock via `performance.now()`; the timing source is reported honestly.

use crate::log;

/// How the reported per-scope times were measured. 0.1 only has CPU rAF/wall-clock.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TimingSource {
    /// CPU wall-clock (performance.now) around CPU-side work. Always available.
    CpuWallClock,
    /// Real GPU timestamp-query results (0.3+ when the adapter supports it).
    #[allow(dead_code)]
    GpuTimestamp,
    /// Coarse single-fence sim-vs-render split (0.3 fallback).
    #[allow(dead_code)]
    CoarseFence,
}

impl TimingSource {
    fn label(self) -> &'static str {
        match self {
            TimingSource::CpuWallClock => "cpu-wallclock",
            TimingSource::GpuTimestamp => "gpu-timestamp",
            TimingSource::CoarseFence => "coarse-fence",
        }
    }
}

/// One accumulated scope over the logging window.
struct ScopeAcc {
    name: &'static str,
    depth: u32,
    total_ms: f64,
    calls: u32,
    /// Wall-clock start of the currently-open instance, if open.
    open_start: Option<f64>,
}

/// Rolling frame-time window for percentile stats.
struct FrameWindow {
    samples: Vec<f64>,
    cap: usize,
}

impl FrameWindow {
    fn new(cap: usize) -> Self {
        Self {
            samples: Vec::with_capacity(cap),
            cap,
        }
    }
    fn push(&mut self, ms: f64) {
        if self.samples.len() == self.cap {
            self.samples.remove(0);
        }
        self.samples.push(ms);
    }
    fn percentile(&self, p: f64) -> f64 {
        if self.samples.is_empty() {
            return 0.0;
        }
        let mut s = self.samples.clone();
        s.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let idx = ((p / 100.0) * (s.len() as f64 - 1.0)).round() as usize;
        s[idx]
    }
}

pub struct Profiler {
    perf: Option<web_sys::Performance>,
    timing_source: TimingSource,
    scopes: Vec<ScopeAcc>,
    /// Stack of open scope indices for nesting.
    open_stack: Vec<usize>,
    frame_window: FrameWindow,
    frames_since_log: u32,
    last_log_ms: f64,
    log_interval_ms: f64,
    gpu: Option<GpuSample>,
    /// Latest substep count reported by the frame loop.
    last_substeps: u32,
    /// Latest timestep-controller stats (this-frame) + cumulative dropped time.
    timestep: TimestepStats,
    /// Static-ish per-frame facts sourced from the GPU context.
    facts: FrameFacts,
}

/// Latest timestep accounting fed by the frame loop (all times in milliseconds).
#[derive(Clone, Copy, Default)]
struct TimestepStats {
    substeps_this_frame: u32,
    fixed_dt_ms: f32,
    max_substeps: u32,
    natural_substeps: u32,
    substep_cap_hit: bool,
    accumulated_before_ms: f32,
    accumulated_after_ms: f32,
    dropped_this_frame_ms: f32,
    total_dropped_ms: f32,
    sim_advanced_ms: f32,
    wall_raf_ms: f32,
    real_time_factor: f32,
    policy_label: &'static str,
}

/// Per-frame structural facts about the sim (grid/particle/memory/dispatch).
#[derive(Clone, Copy, Default)]
struct FrameFacts {
    total_cells: u32,
    particles: u32,
    grid_res: [u32; 3],
    buffer_bytes: u64,
    memory: crate::gpu::GpuMemoryStats,
    dispatches_per_substep: u32,
    requested_particles: u32,
    estimated_particles: u32,
    max_compute_workgroups_per_dimension: u32,
    max_particle_dispatch_count: u32,
    particle_dispatch_groups_x: u32,
    particle_dispatch_groups_y: u32,
    particle_dispatch_capacity: u32,
    max_particle_storage_count: u32,
    scale_status: &'static str,
}

/// Latest real GPU-timestamp sample (from `gpu::timing`), if available. All
/// per-pass times are FRAME TOTALS (summed across the substeps that ran).
#[derive(Clone, Copy)]
struct GpuSample {
    prep_ms: f32,
    pressure_ms: f32,
    finish_ms: f32,
    render_ms: f32,
    liquid_cells: u32,
    /// Detailed (per-section) breakdown present?
    detailed: bool,
    /// Per-section frame totals (ms), indexed by `gpu::timing::FINE_SECTIONS`.
    sections: [f32; crate::gpu::FINE_SECTIONS.len()],
    /// CG category frame totals (ms): [spmv, reduce, update, scalars].
    cg_cats: [f32; 4],
    cg_iters: u32,
    /// Substeps the per-pass totals were summed over (for ms/substep display).
    substeps: u32,
}

impl Profiler {
    pub fn new() -> Self {
        let perf = web_sys::window().and_then(|w| w.performance());
        Self {
            perf,
            timing_source: TimingSource::CpuWallClock,
            scopes: Vec::new(),
            open_stack: Vec::new(),
            frame_window: FrameWindow::new(240),
            frames_since_log: 0,
            last_log_ms: 0.0,
            log_interval_ms: 3000.0,
            gpu: None,
            last_substeps: 0,
            timestep: TimestepStats::default(),
            facts: FrameFacts::default(),
        }
    }

    /// Record the latest timestep-controller stats. `stats` is the per-frame
    /// accounting (seconds); `total_dropped` is the cumulative dropped sim time
    /// (seconds). Both are converted to milliseconds for display.
    pub fn set_timestep_stats(
        &mut self,
        stats: crate::timestep::TimestepFrameStats,
        total_dropped: f32,
    ) {
        self.timestep = TimestepStats {
            substeps_this_frame: stats.substeps,
            fixed_dt_ms: stats.fixed_dt * 1000.0,
            max_substeps: stats.max_substeps,
            natural_substeps: stats.natural_substeps,
            substep_cap_hit: stats.substep_cap_hit,
            accumulated_before_ms: stats.accumulated_before * 1000.0,
            accumulated_after_ms: stats.accumulated_after * 1000.0,
            dropped_this_frame_ms: stats.dropped_this_frame * 1000.0,
            total_dropped_ms: total_dropped * 1000.0,
            sim_advanced_ms: stats.sim_advanced * 1000.0,
            wall_raf_ms: stats.wall_dt * 1000.0,
            real_time_factor: stats.real_time_factor,
            policy_label: stats.policy_label,
        };
    }

    /// Record structural per-frame facts sourced from the GPU context.
    pub fn set_frame_facts(
        &mut self,
        total_cells: u32,
        particles: u32,
        grid_res: [u32; 3],
        buffer_bytes: u64,
        dispatches_per_substep: u32,
        requested_particles: u32,
        estimated_particles: u32,
        max_compute_workgroups_per_dimension: u32,
        max_particle_dispatch_count: u32,
        particle_dispatch_groups: [u32; 2],
        particle_dispatch_capacity: u32,
        max_particle_storage_count: u32,
        scale_status: &'static str,
    ) {
        self.facts = FrameFacts {
            total_cells,
            particles,
            grid_res,
            buffer_bytes,
            memory: crate::gpu::latest_memory_stats(),
            dispatches_per_substep,
            requested_particles,
            estimated_particles,
            max_compute_workgroups_per_dimension,
            max_particle_dispatch_count,
            particle_dispatch_groups_x: particle_dispatch_groups[0],
            particle_dispatch_groups_y: particle_dispatch_groups[1],
            particle_dispatch_capacity,
            max_particle_storage_count,
            scale_status,
        };
    }

    /// Start a clean measurement window after any Reset attempt. This prevents
    /// pre-reset frame percentiles and GPU samples from being attributed to the
    /// newly requested scale.
    pub fn reset_measurement(&mut self) {
        self.timing_source = TimingSource::CpuWallClock;
        self.frame_window.samples.clear();
        self.frames_since_log = 0;
        self.last_log_ms = self.now();
        self.gpu = None;
        self.last_substeps = 0;
        self.timestep = TimestepStats::default();
        self.open_stack.clear();
        for scope in &mut self.scopes {
            scope.total_ms = 0.0;
            scope.calls = 0;
            scope.open_start = None;
        }
    }

    /// Record the number of physics substeps executed this frame so it can be
    /// included in the periodic log output.
    pub fn set_substeps(&mut self, n: u32) {
        self.last_substeps = n;
    }

    /// Feed the latest real GPU-timestamp readback (sets timing source to GPU).
    /// All per-pass times are FRAME TOTALS summed across the readout's own substep count.
    pub fn set_gpu_sample(&mut self, r: crate::gpu::GpuReadout) {
        self.timing_source = TimingSource::GpuTimestamp;
        self.gpu = Some(GpuSample {
            prep_ms: r.prep_ms,
            pressure_ms: r.pressure_ms,
            finish_ms: r.finish_ms,
            render_ms: r.render_ms,
            liquid_cells: r.liquid_cells,
            detailed: r.detailed,
            sections: r.sections,
            cg_cats: r.cg_cats,
            cg_iters: r.cg_iters,
            substeps: r.substeps,
        });
    }

    fn now(&self) -> f64 {
        self.perf.as_ref().map(|p| p.now()).unwrap_or(0.0)
    }

    pub fn begin_frame(&mut self, render_dt_ms: f64) {
        self.frame_window.push(render_dt_ms);
        self.frames_since_log += 1;
        if self.last_log_ms == 0.0 {
            self.last_log_ms = self.now();
        }
    }

    /// Open a named scope. Reuses the accumulator across the logging window so
    /// repeated frames sum into one entry; depth is the current nesting level.
    pub fn scope_begin(&mut self, name: &'static str) {
        let depth = self.open_stack.len() as u32;
        let idx = match self.scopes.iter().position(|s| s.name == name) {
            Some(i) => i,
            None => {
                self.scopes.push(ScopeAcc {
                    name,
                    depth,
                    total_ms: 0.0,
                    calls: 0,
                    open_start: None,
                });
                self.scopes.len() - 1
            }
        };
        self.scopes[idx].open_start = Some(self.now());
        self.open_stack.push(idx);
    }

    pub fn scope_end(&mut self, name: &'static str) {
        let now = self.now();
        if let Some(pos) = self
            .open_stack
            .iter()
            .rposition(|&i| self.scopes[i].name == name)
        {
            let idx = self.open_stack.remove(pos);
            let s = &mut self.scopes[idx];
            if let Some(start) = s.open_start.take() {
                s.total_ms += now - start;
                s.calls += 1;
            }
        }
    }

    /// Emit a hierarchical, config-tagged sample every `log_interval_ms` and reset
    /// the window accumulators.
    pub fn end_frame_and_maybe_log(&mut self, config_snapshot: &str) {
        let now = self.now();
        if now - self.last_log_ms < self.log_interval_ms {
            return;
        }

        let frames = self.frames_since_log.max(1);
        let avg = self.frame_window.samples.iter().sum::<f64>()
            / self.frame_window.samples.len().max(1) as f64;
        let p50 = self.frame_window.percentile(50.0);
        let p95 = self.frame_window.percentile(95.0);
        let p99 = self.frame_window.percentile(99.0);
        let fps = if avg > 0.0 { 1000.0 / avg } else { 0.0 };

        let mut out = String::new();
        out.push_str("┌─ [fluid-lab] profiler ─────────────────────────────────\n");
        out.push_str(&format!("│ config : {config_snapshot}\n"));
        out.push_str(&format!("│ timing : {}\n", self.timing_source.label()));
        out.push_str(&format!(
            "│ frame  : avg={avg:.2}ms (~{fps:.0} fps)  p50={p50:.2} p95={p95:.2} p99={p99:.2}  frames={frames}  substeps={}\n",
            self.last_substeps
        ));
        if let Some(g) = self.gpu {
            let sim = g.prep_ms + g.pressure_ms + g.finish_ms;
            let n = g.substeps.max(1);
            out.push_str(&format!(
                "│ GPU (real timestamps, ms/frame summed over {n} substeps; ms/substep in []):\n"
            ));
            out.push_str(&format!(
                "│   sim total        {sim:>7.3} [{:>6.3}]   render {:>7.3}\n",
                sim / n as f32,
                g.render_ms
            ));
            out.push_str(&format!(
                "│     prep (clear/mark/P2G/forces) {:>7.3} [{:>6.3}]\n",
                g.prep_ms,
                g.prep_ms / n as f32,
            ));
            out.push_str(&format!(
                "│     pressure (divergence+CG)      {:>7.3} [{:>6.3}]\n",
                g.pressure_ms,
                g.pressure_ms / n as f32,
            ));
            out.push_str(&format!(
                "│     finish (gradient/G2P/advect)  {:>7.3} [{:>6.3}]\n",
                g.finish_ms,
                g.finish_ms / n as f32,
            ));
            if g.detailed {
                out.push_str(&format!(
                    "│     CG: spmv {:.3}  reduce {:.3}  update {:.3}  scalars {:.3}  (iters={})\n",
                    g.cg_cats[0], g.cg_cats[1], g.cg_cats[2], g.cg_cats[3], g.cg_iters,
                ));
            }
            out.push_str(&format!("│   liquid cells     {}\n", g.liquid_cells));
        }
        out.push_str("│ CPU scopes (encode time, ms/frame over window):\n");
        for s in &self.scopes {
            let indent = "  ".repeat(s.depth as usize + 1);
            let avg_ms = s.total_ms / frames as f64;
            out.push_str(&format!(
                "│ {indent}{:<18} {avg_ms:>7.3} ms   x{}\n",
                s.name, s.calls
            ));
        }
        out.push_str("└────────────────────────────────────────────────────────");
        log(&out);

        // Reset window accumulators.
        for s in &mut self.scopes {
            s.total_ms = 0.0;
            s.calls = 0;
        }
        self.frames_since_log = 0;
        self.last_log_ms = now;
    }

    /// Expose the timing source label for stats_json.
    pub fn source_label(&self) -> &'static str {
        self.timing_source.label()
    }

    /// Serialize live profiler + GPU stats to a JSON object string.
    /// grid_n and particles are passed in from FluidApp (which owns settings/gpu).
    pub fn stats_json(
        &self,
        grid_n: u32,
        particles: u32,
        pressure_iterations: u32,
        render_mode: &str,
    ) -> String {
        let samples = &self.frame_window.samples;
        let count = samples.len().max(1);
        let avg = samples.iter().sum::<f64>() / count as f64;
        let fps = if avg > 0.0 { 1000.0 / avg } else { 0.0 };
        let p50 = self.frame_window.percentile(50.0);
        let p95 = self.frame_window.percentile(95.0);
        let p99 = self.frame_window.percentile(99.0);

        let gpu_json = match self.gpu {
            None => "null".to_string(),
            Some(g) => {
                let sim_ms = g.prep_ms + g.pressure_ms + g.finish_ms;
                // Detailed mode adds a per-section object + a CG category summary.
                // All values are FRAME TOTALS (summed over the substeps that ran).
                let detail = if g.detailed {
                    let mut secs = String::from(",\"sections\":{");
                    for (i, name) in crate::gpu::FINE_SECTIONS.iter().enumerate() {
                        if i > 0 {
                            secs.push(',');
                        }
                        secs.push_str(&format!("\"{name}\":{}", fmt_ms(g.sections[i] as f64)));
                    }
                    secs.push('}');
                    let cg_total: f32 = g.cg_cats.iter().sum();
                    let iters = g.cg_iters.max(1);
                    let avg = cg_total / iters as f32;
                    // cg_cats order: [spmv, reduce(both dots), update, scalars(alpha/beta/dir)].
                    let cg = format!(
                        ",\"cg\":{{\"total_ms\":{tot},\"avg_ms_per_iter\":{avg},\"spmv_ms\":{sp},\"reductions_ms\":{re},\"updates_ms\":{up},\"scalars_ms\":{sc},\"iters\":{it}}}",
                        tot = fmt_ms(cg_total as f64),
                        avg = fmt_ms(avg as f64),
                        sp  = fmt_ms(g.cg_cats[0] as f64),
                        re  = fmt_ms(g.cg_cats[1] as f64),
                        up  = fmt_ms(g.cg_cats[2] as f64),
                        sc  = fmt_ms(g.cg_cats[3] as f64),
                        it  = g.cg_iters,
                    );
                    format!("{secs}{cg}")
                } else {
                    String::new()
                };
                format!(
                    r#"{{"sim_ms":{sim},"prep_ms":{prep},"pressure_ms":{pres},"finish_ms":{fin},"render_ms":{ren},"liquid_cells":{lc},"substeps":{subs},"detailed":{det}{detail}}}"#,
                    sim = fmt_ms(sim_ms as f64),
                    prep = fmt_ms(g.prep_ms as f64),
                    pres = fmt_ms(g.pressure_ms as f64),
                    fin = fmt_ms(g.finish_ms as f64),
                    ren = fmt_ms(g.render_ms as f64),
                    lc = g.liquid_cells,
                    subs = g.substeps,
                    det = g.detailed,
                    detail = detail,
                )
            }
        };

        let f = &self.facts;
        let ts = &self.timestep;
        let [rx, ry, rz] = f.grid_res;
        // grid_res string like "64x64x64"; fall back to grid_n if facts unset.
        let grid_res_str = if rx > 0 {
            format!("{rx}x{ry}x{rz}")
        } else {
            format!("{grid_n}")
        };
        let total_cells = f.total_cells;
        let particles_out = if f.particles > 0 {
            f.particles
        } else {
            particles
        };
        let gpu_buffer_mb = format!("{:.1}", f.buffer_bytes as f64 / 1.0e6);
        let sim_buffers_bytes = if f.memory.sim_buffers_bytes > 0 {
            f.memory.sim_buffers_bytes
        } else {
            f.buffer_bytes
        };
        let sim_buffers_mb = format!("{:.1}", sim_buffers_bytes as f64 / 1.0e6);
        let render_targets_mb = format!("{:.1}", f.memory.render_targets_bytes as f64 / 1.0e6);
        let timing_mb = if f.memory.timing_bytes > 0 {
            format!("{:.1}", f.memory.timing_bytes as f64 / 1.0e6)
        } else {
            "null".to_string()
        };
        let total_tracked_bytes = if f.memory.total_tracked_bytes > 0 {
            f.memory.total_tracked_bytes
        } else {
            sim_buffers_bytes
        };
        let total_tracked_mb = format!("{:.1}", total_tracked_bytes as f64 / 1.0e6);
        let dispatches_per_substep = f.dispatches_per_substep;
        let dispatches_this_frame = dispatches_per_substep * ts.substeps_this_frame;

        // Phase-1 calibration proxy: filled water volume = liquid_cells * H^3 (world
        // units), and the fraction of the tank it fills. With the auto surface
        // dilation on, this is ~density-invariant at a fixed waterline, so it is the
        // fast proxy the volume/density decoupling asserts on. Null when no GPU
        // liquid-cell count is available yet.
        let cell_volume = (crate::sim::H as f64).powi(3);
        let (filled_volume, liquid_fraction) = match self.gpu {
            Some(g) => {
                let fv = g.liquid_cells as f64 * cell_volume;
                let lf = if total_cells > 0 {
                    g.liquid_cells as f64 / total_cells as f64
                } else {
                    0.0
                };
                (format!("{fv:.6}"), format!("{lf:.6}"))
            }
            None => ("null".to_string(), "null".to_string()),
        };

        format!(
            r#"{{"timing":"{timing}","frame_samples":{sample_count},"frame_avg_ms":{avg},"fps":{fps},"p50":{p50},"p95":{p95},"p99":{p99},"substeps":{subs},"grid_n":{gn},"grid_res":"{gres}","total_cells":{tc},"filled_volume":{fv},"liquid_fraction":{lf},"requested_particles":{req},"estimated_particles":{est},"particles":{par},"scale_status":"{scale_status}","max_compute_workgroups_per_dimension":{max_wg},"max_particle_dispatch_count":{max_dispatch},"particle_dispatch_groups_x":{pdgx},"particle_dispatch_groups_y":{pdgy},"particle_dispatch_capacity":{pdcap},"max_particle_storage_count":{max_storage},"pressure_iterations":{pressure_iterations},"render_mode":"{render_mode}","gpu_buffer_mb":{gmb},"sim_buffers_mb":{sim_mb},"render_targets_mb":{rt_mb},"timing_mb":{timing_mb},"total_tracked_mb":{total_mb},"substeps_this_frame":{stf},"fixed_dt_ms":{fdt},"max_substeps":{max_substeps},"natural_substeps":{natural_substeps},"substep_cap_hit":{cap_hit},"sim_advanced_ms":{sim_adv},"wall_raf_ms":{wall_raf},"real_time_factor":{rtf},"timestep_policy":"{policy}","accumulated_before_ms":{ab},"accumulated_after_ms":{aa},"dropped_sim_time_ms":{drop},"total_dropped_sim_time_ms":{tdrop},"dispatches_per_substep":{dps},"dispatches_this_frame":{dtf},"gpu":{gpu}}}"#,
            timing = self.timing_source.label(),
            sample_count = samples.len(),
            avg = fmt_ms(avg),
            fps = fmt_ms(fps),
            p50 = fmt_ms(p50),
            p95 = fmt_ms(p95),
            p99 = fmt_ms(p99),
            subs = self.last_substeps,
            gn = grid_n,
            gres = grid_res_str,
            tc = total_cells,
            fv = filled_volume,
            lf = liquid_fraction,
            par = particles_out,
            req = f.requested_particles,
            est = f.estimated_particles,
            scale_status = f.scale_status,
            max_wg = f.max_compute_workgroups_per_dimension,
            max_dispatch = f.max_particle_dispatch_count,
            pdgx = f.particle_dispatch_groups_x,
            pdgy = f.particle_dispatch_groups_y,
            pdcap = f.particle_dispatch_capacity,
            max_storage = f.max_particle_storage_count,
            pressure_iterations = pressure_iterations,
            render_mode = render_mode,
            gmb = gpu_buffer_mb,
            sim_mb = sim_buffers_mb,
            rt_mb = render_targets_mb,
            timing_mb = timing_mb,
            total_mb = total_tracked_mb,
            stf = ts.substeps_this_frame,
            fdt = fmt_ms(ts.fixed_dt_ms as f64),
            max_substeps = ts.max_substeps,
            natural_substeps = ts.natural_substeps,
            cap_hit = ts.substep_cap_hit,
            sim_adv = fmt_ms(ts.sim_advanced_ms as f64),
            wall_raf = fmt_ms(ts.wall_raf_ms as f64),
            rtf = fmt_ratio(ts.real_time_factor as f64),
            policy = ts.policy_label,
            ab = fmt_ms(ts.accumulated_before_ms as f64),
            aa = fmt_ms(ts.accumulated_after_ms as f64),
            drop = fmt_ms(ts.dropped_this_frame_ms as f64),
            tdrop = fmt_ms(ts.total_dropped_ms as f64),
            dps = dispatches_per_substep,
            dtf = dispatches_this_frame,
            gpu = gpu_json,
        )
    }
}

/// Format a f64 millisecond value to 3 decimal places (no trailing zeros).
fn fmt_ms(v: f64) -> String {
    if v == 0.0 {
        return "0".to_string();
    }
    let s = format!("{:.3}", v);
    let s = s.trim_end_matches('0');
    let s = s.trim_end_matches('.');
    s.to_string()
}

fn fmt_ratio(v: f64) -> String {
    if v == 0.0 {
        return "0".to_string();
    }
    let s = format!("{:.4}", v);
    let s = s.trim_end_matches('0');
    let s = s.trim_end_matches('.');
    s.to_string()
}
