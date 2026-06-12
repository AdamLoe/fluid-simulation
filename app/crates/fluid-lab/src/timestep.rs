//! Fixed-timestep accumulator for the fluid simulation frame loop.
//!
//! `TimestepController` decouples the browser rAF delta (`render_dt`) from the
//! physics fixed timestep (`fixed_dt`). Incoming render deltas are clamped to
//! 1/30 s (≈33 ms) before accumulation so a single long browser hitch cannot
//! produce an unbounded burst of simulation steps. If the number of steps that
//! would naturally result exceeds `max_substeps`, the excess accumulated time is
//! dropped this frame and tracked in `dropped_time`; the browser catches up by
//! rendering the next frame rather than making one frame longer.
//!
//! This module is pure Rust (no wasm/web-sys deps) so it compiles on all targets
//! and is testable via `cargo test --lib`.

/// Maximum render dt clamped before adding to the accumulator (≈33 ms).
const MAX_RENDER_DT_S: f32 = 1.0 / 30.0;

/// Human-readable label for the active fixed-timestep scheduling policy.
pub const TIMESTEP_POLICY_LABEL: &str = "fixed-drop-cap";

/// Per-frame stats recorded by the last `steps_for_frame` call.
#[derive(Clone, Copy, Default)]
pub struct TimestepFrameStats {
    /// Number of substeps executed this frame.
    pub substeps: u32,
    /// Fixed simulation timestep in seconds.
    pub fixed_dt: f32,
    /// Maximum substeps allowed in one frame.
    pub max_substeps: u32,
    /// Number of substeps the accumulator naturally wanted before capping.
    pub natural_substeps: u32,
    /// Whether the natural substep count exceeded `max_substeps`.
    pub substep_cap_hit: bool,
    /// Accumulator value immediately after adding the clamped render dt.
    pub accumulated_before: f32,
    /// Accumulator value after draining the executed substeps (and dropping excess).
    pub accumulated_after: f32,
    /// Seconds of sim time dropped this frame due to the substep cap.
    pub dropped_this_frame: f32,
    /// Seconds of simulation time actually advanced this frame.
    pub sim_advanced: f32,
    /// Raw sanitized browser rAF wall time in seconds, before the hitch clamp.
    pub wall_dt: f32,
    /// Executed sim time divided by raw sanitized rAF wall time.
    pub real_time_factor: f32,
    /// Human-readable label for the active policy.
    pub policy_label: &'static str,
}

impl TimestepFrameStats {
    fn idle(fixed_dt: f32, max_substeps: u32, wall_dt: f32) -> Self {
        Self {
            fixed_dt,
            max_substeps,
            wall_dt,
            policy_label: TIMESTEP_POLICY_LABEL,
            ..Self::default()
        }
    }
}

pub struct TimestepController {
    accumulator: f32,
    fixed_dt: f32,
    max_substeps: u32,
    /// Cumulative seconds of simulation time dropped due to substep capping.
    dropped_time: f32,
    /// Stats recorded for the most recent `steps_for_frame` call.
    last: TimestepFrameStats,
}

impl TimestepController {
    pub fn new(fixed_dt: f32, max_substeps: u32) -> Self {
        Self {
            accumulator: 0.0,
            fixed_dt,
            max_substeps,
            dropped_time: 0.0,
            last: TimestepFrameStats::default(),
        }
    }

    /// Advance the accumulator by `render_dt_s` (already in seconds) and return
    /// how many fixed-dt substeps should be executed this frame.
    ///
    /// The incoming `render_dt_s` is clamped to `1/30 s` before accumulation.
    /// We run at most `max_substeps` substeps; if more time accumulated than
    /// that, the excess (whole extra steps + sub-step remainder) is dropped this
    /// frame and added to the cumulative `dropped_time`. This prefers
    /// interactivity: a slow frame stays cheap and the browser catches up by
    /// rendering the next frame rather than by making one frame longer.
    pub fn steps_for_frame(&mut self, render_dt_s: f32) -> u32 {
        let wall_dt_s = finite_nonnegative(render_dt_s);
        let clamped = wall_dt_s.min(MAX_RENDER_DT_S);
        self.accumulator += clamped;

        let accumulated_before = self.accumulator;

        let n_natural = if self.fixed_dt > 0.0 {
            (self.accumulator / self.fixed_dt).floor() as u32
        } else {
            0
        };
        let n = n_natural.min(self.max_substeps);

        // Drain only the steps we actually run.
        self.accumulator -= n as f32 * self.fixed_dt;

        // If we were capped, drop the remaining accumulator entirely this frame
        // so we don't carry stale time forward and compound the lateness.
        let dropped_this_frame = if n_natural > self.max_substeps {
            let dropped = self.accumulator;
            self.dropped_time += dropped;
            self.accumulator = 0.0;
            dropped
        } else {
            0.0
        };

        let accumulated_after = self.accumulator;
        let sim_advanced = n as f32 * self.fixed_dt;

        self.last = TimestepFrameStats {
            substeps: n,
            fixed_dt: self.fixed_dt,
            max_substeps: self.max_substeps,
            natural_substeps: n_natural,
            substep_cap_hit: n_natural > self.max_substeps,
            accumulated_before,
            accumulated_after,
            dropped_this_frame,
            sim_advanced,
            wall_dt: wall_dt_s,
            real_time_factor: real_time_factor(sim_advanced, wall_dt_s),
            policy_label: TIMESTEP_POLICY_LABEL,
        };

        n
    }

    /// Record substeps that were executed outside the accumulator policy, such
    /// as a single manual step while paused.
    pub fn record_manual_steps(&mut self, render_dt_s: f32, substeps: u32) {
        let wall_dt_s = finite_nonnegative(render_dt_s);
        let sim_advanced = substeps as f32 * self.fixed_dt;
        self.last = TimestepFrameStats {
            substeps,
            fixed_dt: self.fixed_dt,
            max_substeps: self.max_substeps,
            natural_substeps: 0,
            substep_cap_hit: false,
            accumulated_before: 0.0,
            accumulated_after: 0.0,
            dropped_this_frame: 0.0,
            sim_advanced,
            wall_dt: wall_dt_s,
            real_time_factor: real_time_factor(sim_advanced, wall_dt_s),
            policy_label: TIMESTEP_POLICY_LABEL,
        };
    }

    /// Record a no-op frame outside the accumulator, preserving the rAF wall dt
    /// so stats still show the display cadence while paused.
    pub fn record_idle_frame(&mut self, render_dt_s: f32) {
        self.last = TimestepFrameStats::idle(
            self.fixed_dt,
            self.max_substeps,
            finite_nonnegative(render_dt_s),
        );
    }

    /// Stats recorded during the most recent `steps_for_frame` call.
    pub fn last_stats(&self) -> TimestepFrameStats {
        self.last
    }

    /// Total seconds of simulation time that have been dropped due to substep
    /// capping since this controller was created (or last `reset`).
    pub fn total_dropped(&self) -> f32 {
        self.dropped_time
    }

    /// Total seconds of simulation time dropped (cumulative). Delegates to
    /// `total_dropped`; kept for backward compatibility with existing callers.
    pub fn dropped_time(&self) -> f32 {
        self.dropped_time
    }

    /// Zero the accumulator. Call when resuming from pause or on sim reset so
    /// stale accumulated time does not burst the sim on the next frame.
    pub fn reset(&mut self) {
        self.accumulator = 0.0;
        // Zero the per-frame stats too so a paused frame reports 0 substeps / 0
        // dropped rather than echoing the last running frame. Cumulative
        // `dropped_time` is intentionally preserved.
        self.last = TimestepFrameStats::idle(self.fixed_dt, self.max_substeps, 0.0);
    }
}

fn finite_nonnegative(v: f32) -> f32 {
    if v.is_finite() {
        v.max(0.0)
    } else {
        0.0
    }
}

fn real_time_factor(sim_advanced: f32, wall_dt: f32) -> f32 {
    if wall_dt > 0.0 {
        sim_advanced / wall_dt
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_close(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() < 1.0e-6,
            "expected {expected}, got {actual}"
        );
    }

    #[test]
    fn one_hundred_twentieth_frame_with_cap_one_runs_realtime() {
        let fixed_dt = 1.0 / 120.0;
        let mut tc = TimestepController::new(fixed_dt, 1);

        let n = tc.steps_for_frame(1.0 / 120.0);
        let stats = tc.last_stats();

        assert_eq!(n, 1);
        assert_eq!(stats.substeps, 1);
        assert_eq!(stats.natural_substeps, 1);
        assert_eq!(stats.max_substeps, 1);
        assert!(!stats.substep_cap_hit);
        assert_close(stats.fixed_dt, fixed_dt);
        assert_close(stats.wall_dt, 1.0 / 120.0);
        assert_close(stats.sim_advanced, fixed_dt);
        assert_close(stats.real_time_factor, 1.0);
        assert_eq!(stats.policy_label, TIMESTEP_POLICY_LABEL);
        assert_eq!(tc.total_dropped(), 0.0);
    }

    #[test]
    fn sixty_hz_frame_with_cap_one_reports_half_realtime() {
        let fixed_dt = 1.0 / 120.0;
        let mut tc = TimestepController::new(fixed_dt, 1);

        let n = tc.steps_for_frame(1.0 / 60.0);
        let stats = tc.last_stats();

        assert_eq!(n, 1);
        assert_eq!(stats.substeps, 1);
        assert_eq!(stats.natural_substeps, 2);
        assert_eq!(stats.max_substeps, 1);
        assert!(stats.substep_cap_hit);
        assert_close(stats.wall_dt, 1.0 / 60.0);
        assert_close(stats.sim_advanced, fixed_dt);
        assert_close(stats.real_time_factor, 0.5);
        assert_close(stats.dropped_this_frame, fixed_dt);
        assert_close(tc.total_dropped(), fixed_dt);
        assert!(stats.accumulated_after.abs() < 1.0e-6);
    }

    #[test]
    fn large_hitch_uses_raw_wall_time_for_realtime_factor() {
        let fixed_dt = 1.0 / 120.0;
        let mut tc = TimestepController::new(fixed_dt, 1);

        let n = tc.steps_for_frame(1.0);
        let stats = tc.last_stats();

        assert_eq!(n, 1);
        assert_eq!(stats.substeps, 1);
        assert_eq!(stats.natural_substeps, 4);
        assert!(stats.substep_cap_hit);
        assert_close(stats.accumulated_before, MAX_RENDER_DT_S);
        assert_close(stats.wall_dt, 1.0);
        assert_close(stats.sim_advanced, fixed_dt);
        assert_close(stats.real_time_factor, fixed_dt);
        assert_close(stats.dropped_this_frame, 3.0 * fixed_dt);
        assert_close(tc.total_dropped(), 3.0 * fixed_dt);
        assert!(stats.accumulated_after.abs() < 1.0e-6);
    }

    /// At fixed_dt = 1/120 s, one 1/60 s render frame should yield exactly 2 steps
    /// with no remainder.
    #[test]
    fn two_steps_per_half_period_frame() {
        let mut tc = TimestepController::new(1.0 / 120.0, 4);
        let n = tc.steps_for_frame(1.0 / 60.0);
        assert_eq!(n, 2, "expected 2 substeps for a 1/60 s frame");
        assert!(tc.accumulator.abs() < 1e-6, "accumulator should be ~0");
        assert_eq!(tc.dropped_time(), 0.0);
        assert_eq!(tc.last_stats().substeps, 2);
        assert_eq!(tc.last_stats().dropped_this_frame, 0.0);
        assert_eq!(tc.last_stats().natural_substeps, 2);
        assert!(!tc.last_stats().substep_cap_hit);
    }

    /// A 1-second render dt is clamped to 1/30 s. With fixed_dt=1/120 and
    /// max_substeps=4 the natural count from 1/30 s is exactly 4, so no drop.
    /// Then a second controller with max_substeps=2 exercises the cap: natural=4,
    /// cap at 2, remaining accumulator is dropped.
    #[test]
    fn large_frame_clamped_and_capped() {
        let fixed_dt = 1.0 / 120.0;
        let max_substeps = 4_u32;
        let mut tc = TimestepController::new(fixed_dt, max_substeps);

        // A huge dt (1 s) should be clamped to MAX_RENDER_DT_S = 1/30 s.
        // 1/30 / (1/120) = 4.0 → natural = 4, which equals max_substeps.
        let n = tc.steps_for_frame(1.0);
        assert_eq!(n, max_substeps);
        // No excess because 4 == max_substeps.
        assert_eq!(tc.dropped_time(), 0.0);
        assert_eq!(tc.last_stats().dropped_this_frame, 0.0);

        // Now use a max_substeps=2 controller so capping actually fires.
        // natural=4, cap at 2 → we run 2 steps, drain 2*fixed_dt, drop the rest.
        let mut tc2 = TimestepController::new(fixed_dt, 2);
        let n2 = tc2.steps_for_frame(1.0); // clamped to 1/30 → 4 natural, cap at 2
        assert_eq!(n2, 2);
        assert!(
            tc2.dropped_time() > 0.0,
            "dropped_time should be positive after cap"
        );
        // Under the new policy accumulator is zeroed when capped.
        assert!(tc2.accumulator.abs() < 1e-6, "accumulator zeroed after cap");
        assert!(tc2.last_stats().dropped_this_frame > 0.0);
        assert!(tc2.last_stats().accumulated_after.abs() < 1e-6);
    }

    /// Sub-fixed_dt frames accumulate across multiple calls until a step is due.
    #[test]
    fn tiny_frames_accumulate() {
        let fixed_dt = 1.0 / 120.0; // ~8.333 ms
        let mut tc = TimestepController::new(fixed_dt, 4);

        // Feed 5 frames of 2 ms each (= 10 ms total) — should yield 1 step
        // once we cross 8.333 ms.
        let dt_s = 0.002_f32;
        let mut total_steps = 0u32;
        for _ in 0..5 {
            total_steps += tc.steps_for_frame(dt_s);
        }
        assert_eq!(total_steps, 1, "5 × 2 ms = 10 ms → 1 step at 1/120 s");
        assert!(tc.dropped_time() == 0.0);
    }

    /// A 1-second hitch with max_substeps=1 and fixed_dt=1/120: clamped to 1/30 s,
    /// natural=4, capped at 1. We run 1 step, drain 1*fixed_dt, drop the rest.
    #[test]
    fn large_frame_drops_excess_and_records_stats() {
        let mut tc = TimestepController::new(1.0 / 120.0, 1);
        let n = tc.steps_for_frame(1.0); // 1 s hitch
        assert_eq!(n, 1, "should run exactly 1 substep");
        assert_eq!(tc.last_stats().substeps, 1);
        assert!(
            tc.last_stats().dropped_this_frame > 0.0,
            "excess must be dropped"
        );
        assert!(
            tc.last_stats().accumulated_after.abs() < 1e-6,
            "accumulator must be zeroed after cap"
        );
        assert!(
            tc.total_dropped() > 0.0,
            "cumulative dropped_time must be positive"
        );
    }
}
