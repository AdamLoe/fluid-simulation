//! GPU fluid state and compute passes (Phase 0.3).
//!
//! Implements the MAC particle-grid loop from `simulation_contract.md` on WebGPU:
//! mark/classify → P2G (fixed-point i32 atomics) → gravity → enforce boundaries →
//! divergence → CG pressure solve → subtract gradient → enforce → G2P/advect/recover.
//!
//! Pressure: unpreconditioned Conjugate Gradient on the SPD MAC-Poisson operator
//! (see `src/sim/pressure.rs::cg_solve` for the validated reference math). CG
//! replaced 120-iter Jacobi in the 1.5 solver upgrade — it converges in ~15 iters
//! (vs thousands for Jacobi on a 64-deep column), fixing settle-transient compaction.
//!
//! Structure-of-arrays buffers. No single compute pass binds more than 6 storage
//! buffers (`implementation_risks.md §5`). Buffers are created once; reset only
//! rewrites particle contents. Grid buffers are cleared each step. No readbacks.

use crate::log;
use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

use crate::scene::SceneConfig;
use crate::settings::Registry;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Params {
    dims:   [u32; 4], // nx (legacy "n"), particle_count, pressure_iters, _
    geom:   [f32; 4], // h, inv_h, dt, fixed_scale
    phys:   [f32; 4], // gravity_y (legacy), rho, flip_blend, wall_friction
    origin: [f32; 4],
    grav:   [f32; 4], // gx, gy, gz, _ (3-axis gravity)
    spc:    [f32; 4], // rest_per_cell, volume_stiffness, drift_clamp, _
    cls:    [f32; 4], // liquid_threshold, surface_dilation, _, _
    /// Per-axis cell counts for the rectangular tank: [nx, ny, nz, 0]. Appended at
    /// the END so shaders that don't decompose cell indices can keep their existing
    /// (prefix) Params mirror untouched; only the decomposing shaders mirror this.
    gdim:   [u32; 4],
}

const FIXED_SCALE: f32 = 65536.0; // 2^16 (see docs/p2g-strategy-note.md)
const WG: u32 = 64;

pub struct GpuFluid {
    particle_count: u32,
    nx: u32,
    ny: u32,
    nz: u32,
    cell_count: u32,
    u_count: u32,
    v_count: u32,
    w_count: u32,
    pressure_iters: u32,
    /// Total bytes of all storage buffers allocated in `new` (for the profiler).
    buffer_bytes: u64,

    // buffers
    /// Interleaved particles {pos:vec4, vel:vec4} (32 B each).
    particles: wgpu::Buffer,
    initial: Vec<[f32; 8]>,
    u_num: wgpu::Buffer,
    u_den: wgpu::Buffer,
    v_num: wgpu::Buffer,
    v_den: wgpu::Buffer,
    w_num: wgpu::Buffer,
    w_den: wgpu::Buffer,
    u_vel: wgpu::Buffer,
    v_vel: wgpu::Buffer,
    w_vel: wgpu::Buffer,
    /// Post-P2G, pre-force grid velocity snapshot for the FLIP delta.
    u_saved: wgpu::Buffer,
    v_saved: wgpu::Buffer,
    w_saved: wgpu::Buffer,
    pressure_a: wgpu::Buffer,
    pressure_b: wgpu::Buffer,
    divergence: wgpu::Buffer,
    occupancy: wgpu::Buffer,
    cell_type: wgpu::Buffer,
    /// stats[0] = liquid cell count (liveness counter), read back throttled.
    stats: wgpu::Buffer,
    // CG solver buffers
    cg_d: wgpu::Buffer,
    cg_q: wgpu::Buffer,
    cg_partials: wgpu::Buffer,
    cg_scalars: wgpu::Buffer,
    params: Params,
    params_buf: wgpu::Buffer,

    // pipelines
    clear_pl: wgpu::ComputePipeline,
    mark_pl: wgpu::ComputePipeline,
    classify_pl: wgpu::ComputePipeline,
    scatter_pl: [wgpu::ComputePipeline; 3],
    normalize_pl: wgpu::ComputePipeline,
    save_vel_pl: wgpu::ComputePipeline,
    gravity_pl: [wgpu::ComputePipeline; 3],
    enforce_pl: [wgpu::ComputePipeline; 3],
    divergence_pl: wgpu::ComputePipeline,
    rbgs_red_pl: wgpu::ComputePipeline,
    rbgs_black_pl: wgpu::ComputePipeline,
    gradient_pl: [wgpu::ComputePipeline; 3],
    g2p_pl: wgpu::ComputePipeline,
    // CG pipelines
    cg_init_pl: wgpu::ComputePipeline,
    cg_spmv_pl: wgpu::ComputePipeline,
    cg_reduce_pl: wgpu::ComputePipeline,
    cg_reduce_final_pl: wgpu::ComputePipeline,
    cg_alpha_pl: wgpu::ComputePipeline,
    cg_update_pl: wgpu::ComputePipeline,
    cg_beta_pl: wgpu::ComputePipeline,
    cg_dir_pl: wgpu::ComputePipeline,
    cg_set_rsold_pl: wgpu::ComputePipeline,

    // bind groups (built once; buffers are stable)
    clear_bg: Vec<(wgpu::BindGroup, u32)>, // (bind group, element count)
    mark_bg: wgpu::BindGroup,
    classify_bg: wgpu::BindGroup,
    scatter_bg: [wgpu::BindGroup; 3],
    normalize_bg: [wgpu::BindGroup; 3],
    save_vel_bg: [wgpu::BindGroup; 3],
    gravity_bg: [wgpu::BindGroup; 3],
    enforce_bg: [wgpu::BindGroup; 3],
    divergence_bg: wgpu::BindGroup,
    rbgs_bg: wgpu::BindGroup,   // kept for reference, no longer dispatched
    gradient_bg: [wgpu::BindGroup; 3], // read pressure_a
    g2p_bg: wgpu::BindGroup,
    // CG bind groups
    cg_init_bg: wgpu::BindGroup,
    cg_spmv_bg: wgpu::BindGroup,
    cg_reduce_rr_bg: wgpu::BindGroup,
    cg_reduce_dq_bg: wgpu::BindGroup,
    cg_reduce_final_bg: wgpu::BindGroup,
    cg_alpha_bg: wgpu::BindGroup,
    cg_update_bg: wgpu::BindGroup,
    cg_beta_bg: wgpu::BindGroup,
    cg_dir_bg: wgpu::BindGroup,
    cg_set_rsold_bg: wgpu::BindGroup,

    // impulse pass
    impulse_buf: wgpu::Buffer,
    impulse_pl: wgpu::ComputePipeline,
    impulse_bg: wgpu::BindGroup,
}

impl GpuFluid {
    pub fn new(device: &wgpu::Device, _queue: &wgpu::Queue, settings: &Registry, scene: &SceneConfig) -> Self {
        // Uniform cell size; the tank is made rectangular by per-axis cell counts.
        let nx = settings.grid_res_x();
        let ny = settings.grid_res_y();
        let nz = settings.grid_res_z();
        let h = crate::sim::H;
        // Centered domain: extent on axis a = n_a * h, origin = -n_a*h/2. All-64 → -1.
        let origin = [
            -(nx as f32) * h / 2.0,
            -(ny as f32) * h / 2.0,
            -(nz as f32) * h / 2.0,
        ];
        let extent = [nx as f32 * h, ny as f32 * h, nz as f32 * h];

        let positions = generate_particles(scene, h, origin, extent);
        let particle_count = positions.len() as u32;
        // Interleave into {pos.xyz, 0, vel=0,0,0,0}.
        let initial: Vec<[f32; 8]> = positions
            .iter()
            .map(|p| [p[0], p[1], p[2], 0.0, 0.0, 0.0, 0.0, 0.0])
            .collect();

        let cell_count = nx * ny * nz;
        let u_count = (nx + 1) * ny * nz;
        let v_count = nx * (ny + 1) * nz;
        let w_count = nx * ny * (nz + 1);
        // CG iteration count (result always lands in pressure_a). Min 1.
        let pressure_iters = settings.pressure_iterations().max(1);

        let params = Params {
            dims:   [nx, particle_count, pressure_iters, 0],
            geom:   [h, 1.0 / h, settings.fixed_dt(), FIXED_SCALE],
            phys:   [settings.gravity(), 1000.0, settings.flip_blend(), settings.wall_friction()],
            origin: [origin[0], origin[1], origin[2], 0.0],
            grav:   [0.0, settings.gravity(), 0.0, 0.0],
            spc:    [settings.rest_density(), settings.volume_stiffness(), settings.drift_clamp(), 0.0],
            cls:    [settings.liquid_threshold() as f32, settings.surface_dilation() as f32, settings.cfl(), 0.0],
            gdim:   [nx, ny, nz, 0],
        };
        let params_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("fluid params"),
            contents: bytemuck::bytes_of(&params),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let particles = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("particles"),
            contents: bytemuck::cast_slice(&initial),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        });

        let mk = |label: &str, elems: u32| -> wgpu::Buffer {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: (elems as u64) * 4,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            })
        };
        let u_num = mk("u_num", u_count);
        let u_den = mk("u_den", u_count);
        let v_num = mk("v_num", v_count);
        let v_den = mk("v_den", v_count);
        let w_num = mk("w_num", w_count);
        let w_den = mk("w_den", w_count);
        let u_vel = mk("u_vel", u_count);
        let v_vel = mk("v_vel", v_count);
        let w_vel = mk("w_vel", w_count);
        let u_saved = mk("u_saved", u_count);
        let v_saved = mk("v_saved", v_count);
        let w_saved = mk("w_saved", w_count);
        let pressure_a = mk("pressure_a", cell_count);
        let pressure_b = mk("pressure_b", cell_count);
        let divergence = mk("divergence", cell_count);
        let occupancy = mk("occupancy", cell_count);
        let cell_type = mk("cell_type", cell_count);
        let stats = mk("stats", 1);
        // CG solver buffers
        let cg_d = mk("cg_d", cell_count);
        let cg_q = mk("cg_q", cell_count);
        let red_wgs = cell_count.div_ceil(256);
        let cg_partials = mk("cg_partials", red_wgs);
        let cg_scalars = mk("cg_scalars", 4);

        // Sum of every storage buffer allocated above (for the profiler's GPU
        // buffer-memory readout). Each `mk()` buffer is `elems * 4`; particles is
        // `particle_count * 32` (interleaved {pos:vec4, vel:vec4}). The tiny
        // uniform/impulse buffers are excluded (negligible).
        let storage_elems: u64 = (u_count as u64) * 2  // u_num, u_den
            + (v_count as u64) * 2                       // v_num, v_den
            + (w_count as u64) * 2                       // w_num, w_den
            + (u_count as u64) + (v_count as u64) + (w_count as u64)        // u_vel, v_vel, w_vel
            + (u_count as u64) + (v_count as u64) + (w_count as u64)        // u_saved, v_saved, w_saved
            + (cell_count as u64) * 5                     // pressure_a/b, divergence, occupancy, cell_type
            + 1                                           // stats
            + (cell_count as u64) * 2                     // cg_d, cg_q
            + (red_wgs as u64)                            // cg_partials
            + 4;                                          // cg_scalars
        let buffer_bytes: u64 = storage_elems * 4 + (particle_count as u64) * 32;

        // --- pipelines ---
        let clear_pl = compute(device, "clear", include_str!("shaders/clear.wgsl"), "main", &[]);
        let mark_pl = compute(device, "mark", include_str!("shaders/mark.wgsl"), "main", &[]);
        let classify_pl = compute(device, "classify", include_str!("shaders/classify.wgsl"), "main", &[]);
        let scatter_src = include_str!("shaders/scatter.wgsl");
        let scatter_pl = [
            compute(device, "scatter_u", scatter_src, "main", &[("AXIS", 0.0)]),
            compute(device, "scatter_v", scatter_src, "main", &[("AXIS", 1.0)]),
            compute(device, "scatter_w", scatter_src, "main", &[("AXIS", 2.0)]),
        ];
        let normalize_pl = compute(device, "normalize", include_str!("shaders/normalize.wgsl"), "main", &[]);
        let save_vel_pl = compute(device, "save_vel", include_str!("shaders/save_vel.wgsl"), "main", &[]);
        let forces_src = include_str!("shaders/forces.wgsl");
        let gravity_pl = [
            compute(device, "gravity_u", forces_src, "main", &[("AXIS", 0.0)]),
            compute(device, "gravity_v", forces_src, "main", &[("AXIS", 1.0)]),
            compute(device, "gravity_w", forces_src, "main", &[("AXIS", 2.0)]),
        ];
        let bnd_src = include_str!("shaders/boundaries.wgsl");
        let enforce_pl = [
            compute(device, "enforce_u", bnd_src, "main", &[("AXIS", 0.0)]),
            compute(device, "enforce_v", bnd_src, "main", &[("AXIS", 1.0)]),
            compute(device, "enforce_w", bnd_src, "main", &[("AXIS", 2.0)]),
        ];
        let divergence_pl = compute(device, "divergence", include_str!("shaders/divergence.wgsl"), "main", &[]);
        // RBGS red/black share one EXPLICIT layout so the single `rbgs_bg` bind
        // group is compatible with BOTH pipelines. (Two pipelines with auto-layout
        // get distinct layout objects → a bind group built from one is rejected by
        // the other; this is the wgpu auto-layout pitfall.)
        let pressure_src = include_str!("shaders/pressure.wgsl");
        let st_ro = |b: u32| wgpu::BindGroupLayoutEntry {
            binding: b,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let rbgs_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("rbgs_bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                st_ro(1), // divergence (read)
                st_ro(2), // cell_type (read)
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: false },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let rbgs_pll = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("rbgs_pll"),
            bind_group_layouts: &[Some(&rbgs_bgl)],
            immediate_size: 0,
        });
        let rbgs_red_pl   = compute_with_layout(device, "rbgs_red",   pressure_src, "main", &[("PHASE", 0.0)], &rbgs_pll);
        let rbgs_black_pl = compute_with_layout(device, "rbgs_black", pressure_src, "main", &[("PHASE", 1.0)], &rbgs_pll);
        let grad_src = include_str!("shaders/gradient.wgsl");
        let gradient_pl = [
            compute(device, "gradient_u", grad_src, "main", &[("AXIS", 0.0)]),
            compute(device, "gradient_v", grad_src, "main", &[("AXIS", 1.0)]),
            compute(device, "gradient_w", grad_src, "main", &[("AXIS", 2.0)]),
        ];
        let g2p_pl = compute(device, "g2p", include_str!("shaders/g2p.wgsl"), "main", &[]);

        // --- CG pipelines ---
        let cg_init_pl = compute(device, "cg_init", include_str!("shaders/cg_init.wgsl"), "main", &[]);
        let cg_spmv_pl = compute(device, "cg_spmv", include_str!("shaders/cg_spmv.wgsl"), "main", &[]);
        let cg_reduce_pl = compute(device, "cg_reduce", include_str!("shaders/cg_reduce.wgsl"), "main", &[]);
        let cg_reduce_final_pl = compute(device, "cg_reduce_final", include_str!("shaders/cg_reduce_final.wgsl"), "main", &[]);
        let cg_alpha_pl = compute(device, "cg_alpha", include_str!("shaders/cg_alpha.wgsl"), "main", &[]);
        let cg_update_pl = compute(device, "cg_update", include_str!("shaders/cg_update.wgsl"), "main", &[]);
        let cg_beta_pl = compute(device, "cg_beta", include_str!("shaders/cg_beta.wgsl"), "main", &[]);
        let cg_dir_pl = compute(device, "cg_dir", include_str!("shaders/cg_dir.wgsl"), "main", &[]);
        let cg_set_rsold_pl = compute(device, "cg_set_rsold", include_str!("shaders/cg_set_rsold.wgsl"), "main", &[]);

        // --- impulse pipeline ---
        let impulse_pl = compute(device, "impulse", include_str!("shaders/impulse.wgsl"), "main", &[]);

        // --- bind groups ---
        let bg = |label: &str, pl: &wgpu::ComputePipeline, entries: &[&wgpu::Buffer]| -> wgpu::BindGroup {
            let e: Vec<wgpu::BindGroupEntry> = entries
                .iter()
                .enumerate()
                .map(|(i, b)| wgpu::BindGroupEntry {
                    binding: i as u32,
                    resource: b.as_entire_binding(),
                })
                .collect();
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(label),
                layout: &pl.get_bind_group_layout(0),
                entries: &e,
            })
        };

        let clear_targets: [(&wgpu::Buffer, u32); 10] = [
            (&u_num, u_count), (&u_den, u_count),
            (&v_num, v_count), (&v_den, v_count),
            (&w_num, w_count), (&w_den, w_count),
            (&occupancy, cell_count),
            (&pressure_a, cell_count), (&pressure_b, cell_count),
            (&stats, 1),
        ];
        let clear_bg: Vec<(wgpu::BindGroup, u32)> = clear_targets
            .iter()
            .map(|(b, c)| (bg("clear", &clear_pl, &[b]), *c))
            .collect();

        let mark_bg = bg("mark", &mark_pl, &[&params_buf, &particles, &occupancy]);
        let classify_bg = bg("classify", &classify_pl, &[&params_buf, &occupancy, &cell_type, &stats]);
        let scatter_bg = [
            bg("scatter_u", &scatter_pl[0], &[&params_buf, &particles, &u_num, &u_den]),
            bg("scatter_v", &scatter_pl[1], &[&params_buf, &particles, &v_num, &v_den]),
            bg("scatter_w", &scatter_pl[2], &[&params_buf, &particles, &w_num, &w_den]),
        ];
        let normalize_bg = [
            bg("norm_u", &normalize_pl, &[&params_buf, &u_num, &u_den, &u_vel]),
            bg("norm_v", &normalize_pl, &[&params_buf, &v_num, &v_den, &v_vel]),
            bg("norm_w", &normalize_pl, &[&params_buf, &w_num, &w_den, &w_vel]),
        ];
        let save_vel_bg = [
            bg("save_u", &save_vel_pl, &[&params_buf, &u_vel, &u_saved]),
            bg("save_v", &save_vel_pl, &[&params_buf, &v_vel, &v_saved]),
            bg("save_w", &save_vel_pl, &[&params_buf, &w_vel, &w_saved]),
        ];
        let gravity_bg = [
            bg("gravity_u", &gravity_pl[0], &[&params_buf, &u_vel, &cell_type]),
            bg("gravity_v", &gravity_pl[1], &[&params_buf, &v_vel, &cell_type]),
            bg("gravity_w", &gravity_pl[2], &[&params_buf, &w_vel, &cell_type]),
        ];
        let enforce_bg = [
            bg("enforce_u", &enforce_pl[0], &[&params_buf, &cell_type, &u_vel]),
            bg("enforce_v", &enforce_pl[1], &[&params_buf, &cell_type, &v_vel]),
            bg("enforce_w", &enforce_pl[2], &[&params_buf, &cell_type, &w_vel]),
        ];
        let divergence_bg = bg(
            "divergence",
            &divergence_pl,
            &[&params_buf, &u_vel, &v_vel, &w_vel, &cell_type, &divergence, &occupancy],
        );
        let rbgs_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("rbgs"),
            layout: &rbgs_bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: params_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: divergence.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: cell_type.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 3, resource: pressure_a.as_entire_binding() },
            ],
        });
        let gradient_bg = [
            bg("grad_u", &gradient_pl[0], &[&params_buf, &pressure_a, &cell_type, &u_vel]),
            bg("grad_v", &gradient_pl[1], &[&params_buf, &pressure_a, &cell_type, &v_vel]),
            bg("grad_w", &gradient_pl[2], &[&params_buf, &pressure_a, &cell_type, &w_vel]),
        ];
        let g2p_bg = bg(
            "g2p",
            &g2p_pl,
            &[&params_buf, &particles, &u_vel, &v_vel, &w_vel, &u_saved, &v_saved, &w_saved],
        );

        // --- CG bind groups ---
        let cg_init_bg = bg("cg_init", &cg_init_pl,
            &[&params_buf, &divergence, &cell_type, &pressure_a, &pressure_b, &cg_d]);
        let cg_spmv_bg = bg("cg_spmv", &cg_spmv_pl,
            &[&params_buf, &cell_type, &cg_d, &cg_q]);
        // cg_reduce is used for two different vector pairs; create both bind groups from the SAME pipeline layout
        let cg_reduce_rr_bg = {
            let layout = cg_reduce_pl.get_bind_group_layout(0);
            let entries = [
                wgpu::BindGroupEntry { binding: 0, resource: params_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: pressure_b.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: pressure_b.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 3, resource: cg_partials.as_entire_binding() },
            ];
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("cg_reduce_rr"),
                layout: &layout,
                entries: &entries,
            })
        };
        let cg_reduce_dq_bg = {
            let layout = cg_reduce_pl.get_bind_group_layout(0);
            let entries = [
                wgpu::BindGroupEntry { binding: 0, resource: params_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: cg_d.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: cg_q.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 3, resource: cg_partials.as_entire_binding() },
            ];
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("cg_reduce_dq"),
                layout: &layout,
                entries: &entries,
            })
        };
        let cg_reduce_final_bg = bg("cg_reduce_final", &cg_reduce_final_pl,
            &[&params_buf, &cg_partials, &cg_scalars]);
        let cg_alpha_bg = bg("cg_alpha", &cg_alpha_pl, &[&cg_scalars]);
        let cg_update_bg = bg("cg_update", &cg_update_pl,
            &[&params_buf, &cg_scalars, &cg_d, &cg_q, &pressure_a, &pressure_b]);
        let cg_beta_bg = bg("cg_beta", &cg_beta_pl, &[&cg_scalars]);
        let cg_dir_bg = bg("cg_dir", &cg_dir_pl,
            &[&params_buf, &cg_scalars, &pressure_b, &cg_d]);
        let cg_set_rsold_bg = bg("cg_set_rsold", &cg_set_rsold_pl, &[&cg_scalars]);

        // --- impulse buffer + bind group ---
        let impulse_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("impulse"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let impulse_bg = bg("impulse", &impulse_pl, &[&params_buf, &impulse_buf, &particles]);

        log(&format!(
            "[fluid-lab][gpu] fluid init: dims={nx}x{ny}x{nz} h={h:.4} particles={particle_count} cells={cell_count} press_iters={pressure_iters}"
        ));

        GpuFluid {
            particle_count,
            nx,
            ny,
            nz,
            cell_count,
            u_count,
            v_count,
            w_count,
            pressure_iters,
            buffer_bytes,
            particles,
            initial,
            u_num, u_den, v_num, v_den, w_num, w_den,
            u_vel, v_vel, w_vel,
            u_saved, v_saved, w_saved,
            pressure_a, pressure_b,
            divergence, occupancy, cell_type, stats,
            cg_d, cg_q, cg_partials, cg_scalars,
            params,
            params_buf,
            clear_pl, mark_pl, classify_pl, scatter_pl, normalize_pl, save_vel_pl,
            gravity_pl, enforce_pl, divergence_pl, rbgs_red_pl, rbgs_black_pl, gradient_pl, g2p_pl,
            cg_init_pl, cg_spmv_pl, cg_reduce_pl, cg_reduce_final_pl,
            cg_alpha_pl, cg_update_pl, cg_beta_pl, cg_dir_pl, cg_set_rsold_pl,
            clear_bg, mark_bg, classify_bg, scatter_bg, normalize_bg, save_vel_bg, gravity_bg,
            enforce_bg, divergence_bg, rbgs_bg, gradient_bg, g2p_bg,
            cg_init_bg, cg_spmv_bg, cg_reduce_rr_bg, cg_reduce_dq_bg, cg_reduce_final_bg,
            cg_alpha_bg, cg_update_bg, cg_beta_bg, cg_dir_bg, cg_set_rsold_bg,
            impulse_buf, impulse_pl, impulse_bg,
        }
    }

    pub fn particle_count(&self) -> u32 {
        self.particle_count
    }
    pub fn particle_buffer(&self) -> &wgpu::Buffer {
        &self.particles
    }
    pub fn stats_buffer(&self) -> &wgpu::Buffer {
        &self.stats
    }
    pub fn cell_type_buffer(&self) -> &wgpu::Buffer {
        &self.cell_type
    }
    pub fn pressure_buffer(&self) -> &wgpu::Buffer {
        &self.pressure_a
    }
    pub fn occupancy_buffer(&self) -> &wgpu::Buffer {
        &self.occupancy
    }
    pub fn params_buffer(&self) -> &wgpu::Buffer {
        &self.params_buf
    }
    pub fn u_vel_buffer(&self) -> &wgpu::Buffer {
        &self.u_vel
    }
    pub fn v_vel_buffer(&self) -> &wgpu::Buffer {
        &self.v_vel
    }
    pub fn w_vel_buffer(&self) -> &wgpu::Buffer {
        &self.w_vel
    }
    pub fn grid_n(&self) -> u32 {
        self.nx
    }
    /// Per-axis cell counts of the (possibly rectangular) tank.
    pub fn grid_dims(&self) -> [u32; 3] {
        [self.nx, self.ny, self.nz]
    }
    /// Total grid cells (nx*ny*nz).
    pub fn total_cells(&self) -> u32 {
        self.cell_count
    }
    /// Total bytes of all storage buffers allocated for this fluid (for the
    /// profiler's GPU buffer-memory readout). Computed once in `new`.
    pub fn buffer_memory_bytes(&self) -> u64 {
        self.buffer_bytes
    }
    /// Number of `dispatch_workgroups` calls issued per substep (prep + pressure +
    /// finish, assuming pressure is enabled). Formula:
    ///   prep    = 27 (clear×10, mark, classify, scatter×3, normalize×3,
    ///                 save_vel×3, gravity×3, enforce×3)
    ///   pressure= 5 + 9*pressure_iters (divergence, cg_init, init-reduce×3;
    ///                 per iter: spmv, reduce, reduce_final, alpha, update,
    ///                 reduce, reduce_final, beta, dir)
    ///   finish  = 7 (gradient×3, enforce×3, g2p) when pressure enabled
    /// Total = 39 + 9*pressure_iters.
    pub fn dispatches_per_substep(&self) -> u32 {
        39 + 9 * self.pressure_iters
    }
    /// Live CG iteration count (for sizing detailed timing slots).
    pub fn pressure_iters(&self) -> u32 {
        self.pressure_iters
    }
    /// World-space axis-aligned bounds of the tank: (lo, hi). Uniform cell size H,
    /// centered origin. Used to size the wireframe and visualization extents.
    pub fn tank_bounds(&self) -> ([f32; 3], [f32; 3]) {
        let h = crate::sim::H;
        let lo = [
            -(self.nx as f32) * h / 2.0,
            -(self.ny as f32) * h / 2.0,
            -(self.nz as f32) * h / 2.0,
        ];
        let hi = [-lo[0], -lo[1], -lo[2]];
        (lo, hi)
    }

    /// Live update of the FLIP blend (apply class Live).
    pub fn set_flip_blend(&mut self, queue: &wgpu::Queue, blend: f32) {
        self.params.phys[2] = blend.clamp(0.0, 1.0);
        queue.write_buffer(&self.params_buf, 0, bytemuck::bytes_of(&self.params));
    }

    /// Live update of the wall friction (apply class Live).
    pub fn set_wall_friction(&mut self, queue: &wgpu::Queue, f: f32) {
        self.params.phys[3] = f.clamp(0.0, 1.0);
        queue.write_buffer(&self.params_buf, 0, bytemuck::bytes_of(&self.params));
    }

    /// Live update of the rest packing the pressure solve targets (particles/cell).
    pub fn set_rest_density(&mut self, queue: &wgpu::Queue, v: f32) {
        self.params.spc[0] = v.max(0.1);
        queue.write_buffer(&self.params_buf, 0, bytemuck::bytes_of(&self.params));
    }

    /// Live update of the volume (anti-clump) stiffness fed into the divergence.
    pub fn set_volume_stiffness(&mut self, queue: &wgpu::Queue, v: f32) {
        self.params.spc[1] = v.max(0.0);
        queue.write_buffer(&self.params_buf, 0, bytemuck::bytes_of(&self.params));
    }

    /// Live update of the per-step volume-correction clamp (cell-divergence units).
    pub fn set_drift_clamp(&mut self, queue: &wgpu::Queue, v: f32) {
        self.params.spc[2] = v.max(0.0);
        queue.write_buffer(&self.params_buf, 0, bytemuck::bytes_of(&self.params));
    }

    /// Live update of the min particles/cell for a cell to count as liquid.
    pub fn set_liquid_threshold(&mut self, queue: &wgpu::Queue, v: u32) {
        self.params.cls[0] = v.max(1) as f32;
        queue.write_buffer(&self.params_buf, 0, bytemuck::bytes_of(&self.params));
    }

    /// Live update of the surface-dilation radius (0 = off, 1 = one-cell grow).
    pub fn set_surface_dilation(&mut self, queue: &wgpu::Queue, v: u32) {
        self.params.cls[1] = v as f32;
        queue.write_buffer(&self.params_buf, 0, bytemuck::bytes_of(&self.params));
    }

    /// Live update of the CFL number (max cells a particle may cross per step).
    pub fn set_cfl(&mut self, queue: &wgpu::Queue, v: f32) {
        self.params.cls[2] = v.max(1.0);
        queue.write_buffer(&self.params_buf, 0, bytemuck::bytes_of(&self.params));
    }

    /// Live update of the full 3-axis gravity vector (apply class Live).
    pub fn set_gravity_vec(&mut self, queue: &wgpu::Queue, gx: f32, gy: f32, gz: f32) {
        self.params.grav = [gx, gy, gz, self.params.grav[3]];
        queue.write_buffer(&self.params_buf, 0, bytemuck::bytes_of(&self.params));
    }

    /// Live update of CG pressure-solve iteration count (apply class Live).
    pub fn set_pressure_iters(&mut self, queue: &wgpu::Queue, n: u32) {
        let iters = n.max(1);
        self.pressure_iters = iters;
        self.params.dims[2] = iters;
        queue.write_buffer(&self.params_buf, 0, bytemuck::bytes_of(&self.params));
    }

    /// Apply a uniform velocity impulse to all particles (for the slosh mode).
    pub fn apply_impulse(&self, device: &wgpu::Device, queue: &wgpu::Queue, dv: [f32; 3]) {
        queue.write_buffer(&self.impulse_buf, 0, bytemuck::cast_slice(&[dv[0], dv[1], dv[2], 0.0f32]));
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("impulse") });
        {
            let mut p = enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("impulse"),
                timestamp_writes: None,
            });
            p.set_pipeline(&self.impulse_pl);
            p.set_bind_group(0, &self.impulse_bg, &[]);
            p.dispatch_workgroups(self.particle_count.div_ceil(WG), 1, 1);
        }
        queue.submit(std::iter::once(enc.finish()));
    }

    pub fn reset(&mut self, queue: &wgpu::Queue) {
        queue.write_buffer(&self.particles, 0, bytemuck::cast_slice(&self.initial));
    }

    fn counts(&self) -> [u32; 3] {
        [
            self.u_count.div_ceil(WG),
            self.v_count.div_ceil(WG),
            self.w_count.div_ceil(WG),
        ]
    }

    /// Sub-pass A: clear → mark → classify → P2G (scatter+normalize) → gravity →
    /// enforce boundaries. Recorded into a caller-provided (timestamped) pass.
    pub fn record_prep(&self, pass: &mut wgpu::ComputePass<'_>) {
        self.dispatch_clear(pass);
        self.dispatch_mark(pass);
        self.dispatch_classify(pass);
        for a in 0..3 {
            self.dispatch_scatter(pass, a);
        }
        for a in 0..3 {
            self.dispatch_normalize(pass, a);
        }
        // Snapshot post-P2G grid velocity for the FLIP delta (before forces).
        for a in 0..3 {
            self.dispatch_savevel(pass, a);
        }
        for a in 0..3 {
            self.dispatch_forces(pass, a);
        }
        for a in 0..3 {
            self.dispatch_enforce(pass, a);
        }
    }

    // ── Per-section dispatch helpers (shared by coarse `record_*` and the
    //    detailed one-pass-per-section path in `gpu::mod`). Each issues exactly
    //    the dispatch(es) for one timed SECTION. ─────────────────────────────
    pub fn dispatch_clear(&self, pass: &mut wgpu::ComputePass<'_>) {
        pass.set_pipeline(&self.clear_pl);
        for (bgrp, count) in &self.clear_bg {
            pass.set_bind_group(0, bgrp, &[]);
            pass.dispatch_workgroups(count.div_ceil(WG), 1, 1);
        }
    }
    pub fn dispatch_mark(&self, pass: &mut wgpu::ComputePass<'_>) {
        pass.set_pipeline(&self.mark_pl);
        pass.set_bind_group(0, &self.mark_bg, &[]);
        pass.dispatch_workgroups(self.particle_count.div_ceil(WG), 1, 1);
    }
    pub fn dispatch_classify(&self, pass: &mut wgpu::ComputePass<'_>) {
        pass.set_pipeline(&self.classify_pl);
        pass.set_bind_group(0, &self.classify_bg, &[]);
        pass.dispatch_workgroups(self.cell_count.div_ceil(WG), 1, 1);
    }
    pub fn dispatch_scatter(&self, pass: &mut wgpu::ComputePass<'_>, a: usize) {
        pass.set_pipeline(&self.scatter_pl[a]);
        pass.set_bind_group(0, &self.scatter_bg[a], &[]);
        pass.dispatch_workgroups(self.particle_count.div_ceil(WG), 1, 1);
    }
    pub fn dispatch_normalize(&self, pass: &mut wgpu::ComputePass<'_>, a: usize) {
        pass.set_pipeline(&self.normalize_pl);
        pass.set_bind_group(0, &self.normalize_bg[a], &[]);
        pass.dispatch_workgroups(self.counts()[a], 1, 1);
    }
    pub fn dispatch_savevel(&self, pass: &mut wgpu::ComputePass<'_>, a: usize) {
        pass.set_pipeline(&self.save_vel_pl);
        pass.set_bind_group(0, &self.save_vel_bg[a], &[]);
        pass.dispatch_workgroups(self.counts()[a], 1, 1);
    }
    pub fn dispatch_forces(&self, pass: &mut wgpu::ComputePass<'_>, a: usize) {
        pass.set_pipeline(&self.gravity_pl[a]);
        pass.set_bind_group(0, &self.gravity_bg[a], &[]);
        pass.dispatch_workgroups(self.counts()[a], 1, 1);
    }
    pub fn dispatch_enforce(&self, pass: &mut wgpu::ComputePass<'_>, a: usize) {
        pass.set_pipeline(&self.enforce_pl[a]);
        pass.set_bind_group(0, &self.enforce_bg[a], &[]);
        pass.dispatch_workgroups(self.counts()[a], 1, 1);
    }
    pub fn dispatch_gradient(&self, pass: &mut wgpu::ComputePass<'_>, a: usize) {
        pass.set_pipeline(&self.gradient_pl[a]);
        pass.set_bind_group(0, &self.gradient_bg[a], &[]);
        pass.dispatch_workgroups(self.counts()[a], 1, 1);
    }
    pub fn dispatch_g2p(&self, pass: &mut wgpu::ComputePass<'_>) {
        pass.set_pipeline(&self.g2p_pl);
        pass.set_bind_group(0, &self.g2p_bg, &[]);
        pass.dispatch_workgroups(self.particle_count.div_ceil(WG), 1, 1);
    }
    pub fn dispatch_divergence(&self, pass: &mut wgpu::ComputePass<'_>) {
        pass.set_pipeline(&self.divergence_pl);
        pass.set_bind_group(0, &self.divergence_bg, &[]);
        pass.dispatch_workgroups(self.cell_count.div_ceil(WG), 1, 1);
    }
    /// CG init + the initial rs_old = dot(r,r) reduction + set_rsold. Grouped as
    /// the detailed `cg_init` section.
    pub fn dispatch_cg_init(&self, pass: &mut wgpu::ComputePass<'_>) {
        let cells = self.cell_count.div_ceil(WG);
        let red_groups = self.cell_count.div_ceil(256);
        pass.set_pipeline(&self.cg_init_pl);
        pass.set_bind_group(0, &self.cg_init_bg, &[]);
        pass.dispatch_workgroups(cells, 1, 1);
        pass.set_pipeline(&self.cg_reduce_pl);
        pass.set_bind_group(0, &self.cg_reduce_rr_bg, &[]);
        pass.dispatch_workgroups(red_groups, 1, 1);
        pass.set_pipeline(&self.cg_reduce_final_pl);
        pass.set_bind_group(0, &self.cg_reduce_final_bg, &[]);
        pass.dispatch_workgroups(1, 1, 1);
        pass.set_pipeline(&self.cg_set_rsold_pl);
        pass.set_bind_group(0, &self.cg_set_rsold_bg, &[]);
        pass.dispatch_workgroups(1, 1, 1);
    }
    /// CG category — SpMV (q = A*d).
    pub fn dispatch_cg_spmv(&self, pass: &mut wgpu::ComputePass<'_>) {
        pass.set_pipeline(&self.cg_spmv_pl);
        pass.set_bind_group(0, &self.cg_spmv_bg, &[]);
        pass.dispatch_workgroups(self.cell_count.div_ceil(WG), 1, 1);
    }
    /// CG category — the dq = dot(d,q) reduction (reduce + reduce_final).
    pub fn dispatch_cg_reduce_dq(&self, pass: &mut wgpu::ComputePass<'_>) {
        let red_groups = self.cell_count.div_ceil(256);
        pass.set_pipeline(&self.cg_reduce_pl);
        pass.set_bind_group(0, &self.cg_reduce_dq_bg, &[]);
        pass.dispatch_workgroups(red_groups, 1, 1);
        pass.set_pipeline(&self.cg_reduce_final_pl);
        pass.set_bind_group(0, &self.cg_reduce_final_bg, &[]);
        pass.dispatch_workgroups(1, 1, 1);
    }
    /// CG category — the rs_new = dot(r,r) reduction (reduce + reduce_final).
    pub fn dispatch_cg_reduce_rr(&self, pass: &mut wgpu::ComputePass<'_>) {
        let red_groups = self.cell_count.div_ceil(256);
        pass.set_pipeline(&self.cg_reduce_pl);
        pass.set_bind_group(0, &self.cg_reduce_rr_bg, &[]);
        pass.dispatch_workgroups(red_groups, 1, 1);
        pass.set_pipeline(&self.cg_reduce_final_pl);
        pass.set_bind_group(0, &self.cg_reduce_final_bg, &[]);
        pass.dispatch_workgroups(1, 1, 1);
    }
    /// CG category — p += alpha*d ; r -= alpha*q (the cell-wide update).
    pub fn dispatch_cg_update(&self, pass: &mut wgpu::ComputePass<'_>) {
        pass.set_pipeline(&self.cg_update_pl);
        pass.set_bind_group(0, &self.cg_update_bg, &[]);
        pass.dispatch_workgroups(self.cell_count.div_ceil(WG), 1, 1);
    }
    /// CG category — alpha scalar (rs_old/dq).
    pub fn dispatch_cg_alpha(&self, pass: &mut wgpu::ComputePass<'_>) {
        pass.set_pipeline(&self.cg_alpha_pl);
        pass.set_bind_group(0, &self.cg_alpha_bg, &[]);
        pass.dispatch_workgroups(1, 1, 1);
    }
    /// CG category — beta scalar (rs_new/rs_old ; rs_old=rs_new) + d = r + beta*d.
    pub fn dispatch_cg_beta_dir(&self, pass: &mut wgpu::ComputePass<'_>) {
        pass.set_pipeline(&self.cg_beta_pl);
        pass.set_bind_group(0, &self.cg_beta_bg, &[]);
        pass.dispatch_workgroups(1, 1, 1);
        pass.set_pipeline(&self.cg_dir_pl);
        pass.set_bind_group(0, &self.cg_dir_bg, &[]);
        pass.dispatch_workgroups(self.cell_count.div_ceil(WG), 1, 1);
    }

    /// Sub-pass B: divergence + Conjugate Gradient pressure solve. The per-iter
    /// dispatch sequence matches the validated reference (`sim/pressure.rs`):
    /// spmv → dq-reduce → alpha → update → rr-reduce → beta+dir.
    pub fn record_pressure(&self, pass: &mut wgpu::ComputePass<'_>) {
        self.dispatch_divergence(pass);
        self.dispatch_cg_init(pass);
        for _ in 0..self.pressure_iters {
            self.dispatch_cg_spmv(pass);
            self.dispatch_cg_reduce_dq(pass);
            self.dispatch_cg_alpha(pass);
            self.dispatch_cg_update(pass);
            self.dispatch_cg_reduce_rr(pass);
            self.dispatch_cg_beta_dir(pass);
        }
        // Final pressure is in pressure_a, which the gradient pass already reads.
    }

    /// Sub-pass C: subtract gradient + enforce (only if pressure ran), then
    /// G2P + advect + recover.
    pub fn record_finish(&self, pass: &mut wgpu::ComputePass<'_>, pressure_enabled: bool) {
        if pressure_enabled {
            for a in 0..3 {
                self.dispatch_gradient(pass, a);
            }
            for a in 0..3 {
                self.dispatch_enforce(pass, a);
            }
        }
        self.dispatch_g2p(pass);
    }
}

fn compute(
    device: &wgpu::Device,
    label: &str,
    src: &str,
    entry: &str,
    constants: &[(&str, f64)],
) -> wgpu::ComputePipeline {
    compute_inner(device, label, src, entry, constants, None)
}

/// Like `compute` but with an explicit pipeline layout (shared across pipelines so
/// one bind group is compatible with several — see RBGS red/black).
fn compute_with_layout(
    device: &wgpu::Device,
    label: &str,
    src: &str,
    entry: &str,
    constants: &[(&str, f64)],
    layout: &wgpu::PipelineLayout,
) -> wgpu::ComputePipeline {
    compute_inner(device, label, src, entry, constants, Some(layout))
}

fn compute_inner(
    device: &wgpu::Device,
    label: &str,
    src: &str,
    entry: &str,
    constants: &[(&str, f64)],
    layout: Option<&wgpu::PipelineLayout>,
) -> wgpu::ComputePipeline {
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(src.into()),
    });
    device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some(label),
        layout,
        module: &module,
        entry_point: Some(entry),
        compilation_options: wgpu::PipelineCompilationOptions {
            constants,
            zero_initialize_workgroup_memory: true,
        },
        cache: None,
    })
}

/// Deterministic initial particles from the scene config (lattice with seeded
/// jitter, clamped inside the walls). Each scene preset supplies one or more
/// liquid blocks; the requested particle count is distributed across them in
/// proportion to volume so denser/larger blocks get proportionally more
/// particles. See `simulation_contract.md`.
fn generate_particles(scene: &SceneConfig, h: f32, origin: [f32; 3], extent: [f32; 3]) -> Vec<[f32; 4]> {
    // Normalized [0,1]^3 liquid-block space -> per-axis world span [origin, origin+extent].
    let to_world = |t: [f32; 3]| [
        origin[0] + t[0] * extent[0],
        origin[1] + t[1] * extent[1],
        origin[2] + t[2] * extent[2],
    ];
    let lo = [origin[0] + h * 1.05, origin[1] + h * 1.05, origin[2] + h * 1.05];
    let hi = [
        origin[0] + extent[0] - h * 1.05,
        origin[1] + extent[1] - h * 1.05,
        origin[2] + extent[2] - h * 1.05,
    ];

    let blocks = &scene.initial_liquid.blocks;
    // World-space extents + volumes for each block.
    let mut exts: Vec<([f32; 3], [f32; 3], f32)> = Vec::with_capacity(blocks.len());
    let mut total_vol = 0.0f32;
    for b in blocks {
        let wmin = to_world([b.min.x, b.min.y, b.min.z]);
        let wmax = to_world([b.max.x, b.max.y, b.max.z]);
        let ext = [wmax[0] - wmin[0], wmax[1] - wmin[1], wmax[2] - wmin[2]];
        let vol = (ext[0] * ext[1] * ext[2]).max(1e-6);
        total_vol += vol;
        exts.push((wmin, ext, vol));
    }
    let total_vol = total_vol.max(1e-6);
    let total_target = scene.particle_count.max(1) as f32;

    // One shared seeded RNG so the whole layout is deterministic regardless of how
    // the budget splits across blocks.
    let mut st = 0x1234_5678u32;
    let mut rand01 = move || {
        st = st.wrapping_mul(1664525).wrapping_add(1013904223);
        (st >> 8) as f32 / (1u32 << 24) as f32
    };

    let mut out = Vec::with_capacity(scene.particle_count as usize + blocks.len());
    for (wmin, ext, vol) in exts {
        // Per-block particle budget ∝ volume; per-block uniform lattice spacing.
        let target = (total_target * (vol / total_vol)).max(1.0);
        let spacing = (vol / target).cbrt().max(1e-4);
        let counts = [
            ((ext[0] / spacing).floor() as i32).max(1),
            ((ext[1] / spacing).floor() as i32).max(1),
            ((ext[2] / spacing).floor() as i32).max(1),
        ];
        for ix in 0..counts[0] {
            for iy in 0..counts[1] {
                for iz in 0..counts[2] {
                    let jx = (rand01() - 0.5) * spacing * 0.5;
                    let jy = (rand01() - 0.5) * spacing * 0.5;
                    let jz = (rand01() - 0.5) * spacing * 0.5;
                    out.push([
                        (wmin[0] + (ix as f32 + 0.5) * spacing + jx).clamp(lo[0], hi[0]),
                        (wmin[1] + (iy as f32 + 0.5) * spacing + jy).clamp(lo[1], hi[1]),
                        (wmin[2] + (iz as f32 + 0.5) * spacing + jz).clamp(lo[2], hi[2]),
                        0.0,
                    ]);
                }
            }
        }
    }
    out
}
