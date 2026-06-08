//! GPU timestamp profiling + liveness readback.
//!
//! Honest per-stage GPU timing using `timestamp-query` (available on this adapter).
//!
//! TWO MODES, both fixing the historical "last-substep-only" bug by giving every
//! substep its OWN query slots and summing across the substeps run this frame:
//!
//! * COARSE (default): each substep gets three begin/end pairs — prep / pressure /
//!   finish — plus one render pair at the end of the frame. `Readout.prep_ms` etc.
//!   are FRAME TOTALS (summed over the substeps that actually ran).
//!
//! * DETAILED (dev toggle `dev.detailed_gpu_profiling`, Reset-class): each substep
//!   gets one begin/end pair per fine SECTION (clear, mark, classify, scatter_*,
//!   …, g2p) plus, per CG iteration, the three category passes (spmv / reduce /
//!   update) and a small `cg_scalars` pass. Per-section ms are summed over substeps.
//!
//! Mode + `max_substeps` + `pressure_iters` are fixed at construction (Reset-class)
//! so the query set is sized once. If the live `pressure_iters` later exceeds the
//! allocation we clamp the timed CG iters and `crate::log()` once.
//!
//! Results + the liquid-cell liveness counter are read back **throttled** (every
//! `THROTTLE` frames) via async map — the only allowed readback class. Normal
//! frames never read back.

use std::cell::Cell;
use std::rc::Rc;

const THROTTLE: u32 = 20;

/// Fine sections timed per substep (each is one begin/end pair). The CG-iteration
/// category passes are appended AFTER these, `CG_CATS_PER_ITER` pairs per iter.
pub const FINE_SECTIONS: [&str; 27] = [
    "clear",
    "mark",
    "classify",
    "scatter_u",
    "scatter_v",
    "scatter_w",
    "normalize_u",
    "normalize_v",
    "normalize_w",
    "savevel_u",
    "savevel_v",
    "savevel_w",
    "forces_u",
    "forces_v",
    "forces_w",
    "bound_pre_u",
    "bound_pre_v",
    "bound_pre_w",
    "divergence",
    "cg_init",
    "gradient_u",
    "gradient_v",
    "gradient_w",
    "bound_post_u",
    "bound_post_v",
    "bound_post_w",
    "g2p",
];
const N_FINE: usize = FINE_SECTIONS.len(); // 27

/// CG reported categories (frame totals), in `Readout.cg_cats` order:
///   0 = cg_spmv   : q = A·d
///   1 = cg_reduce : BOTH dot-product reductions (d·q and r·r)
///   2 = cg_update : the vector update p += α·d ; r -= α·q
///   3 = cg_scalars: the 1-thread scalar dispatches (alpha, beta, dir)
pub const CG_CATS: [&str; 4] = ["cg_spmv", "cg_reduce", "cg_update", "cg_scalars"];
const CG_CATS_LEN: usize = CG_CATS.len(); // 4

/// Per CG iteration we time SIX contiguous passes — each begin/end span is an
/// honest measurement of one real operation — and bucket them on the CPU into the
/// four reported categories above. The solver order is:
///   spmv · dot(d·q) · alpha · update · dot(r·r) · beta+dir
/// so reductions = passes 1+4 and scalars = passes 2+5; the r·r reduction is
/// honestly counted as a reduction rather than folded into the update.
const CG_TIMED_PER_ITER: usize = 6;
/// timed-pass index (0..6) → reported category index (0..4).
const CG_BUCKET: [usize; CG_TIMED_PER_ITER] = [0, 1, 3, 2, 1, 3];

/// Coarse per-substep timed sections: prep, pressure, finish.
const N_COARSE: usize = 3;

#[derive(Clone, Copy, Default)]
pub struct Readout {
    /// Frame-total prep time (summed over the substeps run this frame).
    pub prep_ms: f32,
    pub pressure_ms: f32,
    pub finish_ms: f32,
    /// Single render-pass span this frame.
    pub render_ms: f32,
    pub liquid_cells: u32,
    pub valid: bool,
    /// Detailed mode only: per-section frame totals (ms), indexed by FINE_SECTIONS.
    /// All zero / `detailed=false` in coarse mode.
    pub detailed: bool,
    pub sections: [f32; N_FINE],
    /// CG category frame totals (ms): [spmv, reduce, update, scalars].
    pub cg_cats: [f32; CG_CATS_LEN],
    /// CG iterations that were actually timed (== allocated iters, possibly clamped).
    pub cg_iters: u32,
}

pub struct GpuTimers {
    query_set: wgpu::QuerySet,
    ts_resolve: wgpu::Buffer,
    read_buf: wgpu::Buffer,
    period_ns: f32,
    frame: Cell<u32>,
    pending: Rc<Cell<bool>>,
    readout: Rc<Cell<Readout>>,

    // layout
    detailed: bool,
    max_substeps: u32,
    /// CG iterations allocated slots at construction (detailed mode).
    alloc_iters: u32,
    /// begin/end PAIRS per substep (coarse=3, fine=N_FINE + CG_TIMED_PER_ITER*alloc_iters).
    pairs_per_substep: u32,
    /// total query slots in the set.
    slots: u32,
    /// byte offset where the liquid count is copied (after all timestamps).
    liquid_offset: u64,
    /// substeps actually submitted this frame (set by step()).
    frame_substeps: Cell<u32>,
    /// one-shot guard so the "iters exceed allocation" truncation logs once.
    iter_warned: Cell<bool>,
}

fn cw(qs: &wgpu::QuerySet, b: u32, e: u32) -> wgpu::ComputePassTimestampWrites<'_> {
    wgpu::ComputePassTimestampWrites {
        query_set: qs,
        beginning_of_pass_write_index: Some(b),
        end_of_pass_write_index: Some(e),
    }
}

impl GpuTimers {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        max_substeps: u32,
        detailed: bool,
        pressure_iters: u32,
    ) -> Self {
        let max_substeps = max_substeps.max(1);
        // WebGPU caps a QuerySet at 8192 queries. A large dev config
        // (high max_substeps × pressure_iters) can exceed that, so shrink the
        // timed CG iters to fit and log — never silently over-allocate.
        const MAX_SLOTS: u32 = 8192;
        let pairs = |iters: u32| -> u32 {
            if detailed {
                (N_FINE as u32) + (CG_TIMED_PER_ITER as u32) * iters
            } else {
                N_COARSE as u32
            }
        };
        // 2 slots per pair, all substeps, + 2 for the render pair.
        let slots_for = |iters: u32| -> u32 { 2 * pairs(iters) * max_substeps + 2 };

        let mut alloc_iters = pressure_iters.max(1);
        if detailed && slots_for(alloc_iters) > MAX_SLOTS {
            let fixed = 2 * (N_FINE as u32) * max_substeps + 2;
            let per_iter = (2 * (CG_TIMED_PER_ITER as u32) * max_substeps).max(1);
            let capped = (MAX_SLOTS.saturating_sub(fixed) / per_iter)
                .max(1)
                .min(alloc_iters);
            crate::log(&format!(
                "[fluid-lab][timing] detailed profiling: query budget caps timed CG iters at \
                 {capped} (requested {alloc_iters}); lower max_substeps to time more"
            ));
            alloc_iters = capped;
        }
        let pairs_per_substep = pairs(alloc_iters);
        let slots = slots_for(alloc_iters);

        let ts_bytes = (slots as u64) * 8;
        let liquid_offset = ts_bytes;
        let read_bytes = ts_bytes + 16; // + liquid count (+ padding)

        let query_set = device.create_query_set(&wgpu::QuerySetDescriptor {
            label: Some("sim+render timestamps"),
            ty: wgpu::QueryType::Timestamp,
            count: slots,
        });
        let ts_resolve = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ts_resolve"),
            size: ts_bytes,
            usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let read_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ts_read"),
            size: read_bytes,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        crate::log(&format!(
            "[fluid-lab][timing] GpuTimers: detailed={detailed} max_substeps={max_substeps} \
             alloc_iters={alloc_iters} pairs/substep={pairs_per_substep} slots={slots}"
        ));

        GpuTimers {
            query_set,
            ts_resolve,
            read_buf,
            period_ns: queue.get_timestamp_period(),
            frame: Cell::new(0),
            pending: Rc::new(Cell::new(false)),
            readout: Rc::new(Cell::new(Readout::default())),
            detailed,
            max_substeps,
            alloc_iters,
            pairs_per_substep,
            slots,
            liquid_offset,
            frame_substeps: Cell::new(0),
            iter_warned: Cell::new(false),
        }
    }

    pub fn detailed(&self) -> bool {
        self.detailed
    }

    /// CG iterations that have allocated timing slots (detailed mode).
    pub fn alloc_iters(&self) -> u32 {
        self.alloc_iters
    }

    /// Record how many substeps were actually submitted this frame so readback
    /// only sums the valid range (slots beyond it are stale/zero).
    pub fn set_frame_substeps(&self, n: u32) {
        self.frame_substeps.set(n.min(self.max_substeps));
    }

    /// First slot index for substep `s` (each substep owns `pairs_per_substep`
    /// pairs == `2*pairs_per_substep` slots).
    fn substep_base(&self, s: u32) -> u32 {
        2 * self.pairs_per_substep * s
    }

    // ── COARSE accessors (per substep) ──────────────────────────────────────
    pub fn prep_writes(&self, substep: u32) -> wgpu::ComputePassTimestampWrites<'_> {
        let b = self.substep_base(substep);
        cw(&self.query_set, b, b + 1)
    }
    pub fn pressure_writes(&self, substep: u32) -> wgpu::ComputePassTimestampWrites<'_> {
        let b = self.substep_base(substep);
        cw(&self.query_set, b + 2, b + 3)
    }
    pub fn finish_writes(&self, substep: u32) -> wgpu::ComputePassTimestampWrites<'_> {
        let b = self.substep_base(substep);
        cw(&self.query_set, b + 4, b + 5)
    }

    // ── FINE accessors (per substep, per section) ───────────────────────────
    /// Section index in `0..N_FINE` → its begin/end pair within the substep block.
    pub fn fine_section_writes(
        &self,
        substep: u32,
        section: usize,
    ) -> wgpu::ComputePassTimestampWrites<'_> {
        let b = self.substep_base(substep) + 2 * (section as u32);
        cw(&self.query_set, b, b + 1)
    }
    /// Timed-pass pair for iteration `iter` (0-based, < alloc_iters) and timed pass
    /// `tpass` in `0..CG_TIMED_PER_ITER`. Laid out after the N_FINE fixed pairs.
    pub fn fine_cg_writes(
        &self,
        substep: u32,
        iter: u32,
        tpass: usize,
    ) -> wgpu::ComputePassTimestampWrites<'_> {
        let pair = (N_FINE as u32) + iter * (CG_TIMED_PER_ITER as u32) + tpass as u32;
        let b = self.substep_base(substep) + 2 * pair;
        cw(&self.query_set, b, b + 1)
    }

    /// Render pair: the last two slots in the set.
    pub fn render_writes(&self) -> wgpu::RenderPassTimestampWrites<'_> {
        wgpu::RenderPassTimestampWrites {
            query_set: &self.query_set,
            beginning_of_pass_write_index: Some(self.slots - 2),
            end_of_pass_write_index: Some(self.slots - 1),
        }
    }

    /// If the live CG iters exceed the allocation, clamp and log once. Returns the
    /// number of CG iters that may be timed this frame (the rest still run, just
    /// without timing slots).
    pub fn clamp_cg_iters(&self, live_iters: u32) -> u32 {
        if live_iters > self.alloc_iters && !self.iter_warned.get() {
            self.iter_warned.set(true);
            crate::log(&format!(
                "[fluid-lab][timing] detailed GPU profiling: timing first {} of {} CG iters; \
                 Reset to resize",
                self.alloc_iters, live_iters
            ));
        }
        live_iters.min(self.alloc_iters)
    }

    pub fn latest(&self) -> Readout {
        self.readout.get()
    }

    /// Resolve queries and, throttled, copy timing + liveness into the mappable
    /// buffer. Returns true if a readback was initiated (caller maps after submit).
    pub fn record_resolve_and_maybe_copy(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        stats_buf: &wgpu::Buffer,
    ) -> bool {
        encoder.resolve_query_set(&self.query_set, 0..self.slots, &self.ts_resolve, 0);

        let f = self.frame.get().wrapping_add(1);
        self.frame.set(f);
        if f % THROTTLE != 0 || self.pending.get() {
            return false;
        }
        encoder.copy_buffer_to_buffer(&self.ts_resolve, 0, &self.read_buf, 0, self.liquid_offset);
        encoder.copy_buffer_to_buffer(stats_buf, 0, &self.read_buf, self.liquid_offset, 4);
        self.pending.set(true);
        true
    }

    /// Begin the async map of the readback buffer (call after submitting the
    /// encoder used in `record_resolve_and_maybe_copy`).
    pub fn map_readback(&self) {
        let pending = self.pending.clone();
        let readout = self.readout.clone();
        let buf = self.read_buf.clone();
        let period = self.period_ns;
        let slots = self.slots as usize;
        let liquid_offset = self.liquid_offset as usize;
        let detailed = self.detailed;
        let pairs_per_substep = self.pairs_per_substep;
        let n_substeps = self.frame_substeps.get();
        let alloc_iters = self.alloc_iters;
        let buf_for_cb = buf.clone();
        buf.slice(..).map_async(wgpu::MapMode::Read, move |res| {
            if res.is_ok() {
                let data = buf_for_cb.slice(..).get_mapped_range();
                let ts = |i: usize| -> u64 {
                    let o = i * 8;
                    u64::from_le_bytes(data[o..o + 8].try_into().unwrap())
                };
                let liquid =
                    u32::from_le_bytes(data[liquid_offset..liquid_offset + 4].try_into().unwrap());
                let span = |a: u64, b: u64| -> f32 {
                    if b > a {
                        (b - a) as f32 * period * 1e-6
                    } else {
                        0.0
                    }
                };
                // Render pair is always the last two slots.
                let render_ms = span(ts(slots - 2), ts(slots - 1));

                let base = |s: u32| -> usize { (2 * pairs_per_substep * s) as usize };

                let mut out = Readout {
                    render_ms,
                    liquid_cells: liquid,
                    valid: true,
                    detailed,
                    ..Readout::default()
                };

                if !detailed {
                    // Sum prep/pressure/finish across the substeps that ran.
                    for s in 0..n_substeps {
                        let b = base(s);
                        out.prep_ms += span(ts(b), ts(b + 1));
                        out.pressure_ms += span(ts(b + 2), ts(b + 3));
                        out.finish_ms += span(ts(b + 4), ts(b + 5));
                    }
                } else {
                    out.cg_iters = alloc_iters;
                    for s in 0..n_substeps {
                        let b = base(s);
                        // Fixed fine sections.
                        for (i, slot_ms) in out.sections.iter_mut().enumerate() {
                            let p = b + 2 * i;
                            *slot_ms += span(ts(p), ts(p + 1));
                        }
                        // CG: six timed passes per iteration, bucketed honestly
                        // (reductions = both dots, updates = the vector update).
                        for it in 0..alloc_iters {
                            for t in 0..CG_TIMED_PER_ITER {
                                let pair = N_FINE + (it as usize) * CG_TIMED_PER_ITER + t;
                                let p = b + 2 * pair;
                                out.cg_cats[CG_BUCKET[t]] += span(ts(p), ts(p + 1));
                            }
                        }
                    }
                    // Roll fine sections up into the coarse prep/pressure/finish
                    // totals so coarse consumers still see something sensible.
                    // prep = sections[clear..=bound_pre_w] (indices 0..18)
                    // pressure = divergence + cg_init + all cg cats (indices 18,19 + cats)
                    // finish = gradient_* + bound_post_* + g2p (indices 20..27)
                    let sec = &out.sections;
                    out.prep_ms = sec[0..18].iter().sum();
                    out.pressure_ms = sec[18] + sec[19] + out.cg_cats.iter().sum::<f32>();
                    out.finish_ms = sec[20..27].iter().sum();
                }

                readout.set(out);
                drop(data);
                buf_for_cb.unmap();
            }
            pending.set(false);
        });
    }
}
