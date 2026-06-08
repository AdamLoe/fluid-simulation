//! Divergence + Jacobi pressure projection — Phase 0.2 reference math.
//!
//! Implements the pressure rules in `simulation_contract.md`:
//! - Divergence at liquid cell centers from staggered face velocities.
//! - Pressure Poisson `∇²p = (ρ/dt) ∇·u*` solved with Jacobi.
//! - Boundary handling: solid neighbours are Neumann (excluded from the stencil);
//!   air neighbours are Dirichlet `p = 0` (free surface); faces touching a solid
//!   carry zero normal velocity.
//! - Velocity correction `u ← u − (dt/ρ) ∇p`, applied only to faces between two
//!   non-solid cells; solid faces are forced to zero.
//!
//! These are standalone, testable functions — NOT a running sim loop. The WGSL
//! pressure passes in 0.3 mirror this stencil exactly (Jacobi is chosen because it
//! is embarrassingly parallel and ping-pongs cleanly on GPU).

use super::{CellType, GridDims};

/// Physical constants for projection. With `rho = dt = 1` the solver reduces to
/// the clean unit form used in tests; real runs set these from the registry.
#[derive(Clone, Copy, Debug)]
pub struct ProjectionParams {
    pub rho: f32,
    pub dt: f32,
}

impl ProjectionParams {
    pub fn unit() -> Self {
        ProjectionParams { rho: 1.0, dt: 1.0 }
    }

    /// RHS scale `ρ h² / dt` in `sum(p_n) - n·p_c = scale·div_c`.
    fn rhs_scale(&self, h: f32) -> f32 {
        self.rho * h * h / self.dt
    }

    /// Velocity-correction coefficient `(dt/ρ)/h` in `u -= coeff·(p_hi - p_lo)`.
    fn grad_coeff(&self, h: f32) -> f32 {
        (self.dt / self.rho) / h
    }
}

/// Compute divergence at every cell. Non-liquid cells get 0 (they don't
/// participate in the solve). `div_c = (u[i+1]-u[i] + v[j+1]-v[j] + w[k+1]-w[k])/h`.
pub fn compute_divergence(
    dims: &GridDims,
    u: &[f32],
    v: &[f32],
    w: &[f32],
    cell_type: &[CellType],
) -> Vec<f32> {
    let mut div = vec![0.0f32; dims.cell_count()];
    for k in 0..dims.nz {
        for j in 0..dims.ny {
            for i in 0..dims.nx {
                let c = dims.cell_idx(i, j, k);
                if cell_type[c] != CellType::Liquid {
                    continue;
                }
                let du = u[dims.u_idx(i + 1, j, k)] - u[dims.u_idx(i, j, k)];
                let dv = v[dims.v_idx(i, j + 1, k)] - v[dims.v_idx(i, j, k)];
                let dw = w[dims.w_idx(i, j, k + 1)] - w[dims.w_idx(i, j, k)];
                div[c] = (du + dv + dw) / dims.h;
            }
        }
    }
    div
}

/// One Jacobi sweep: `p_out[c] = (Σ p_in[non-solid neighbour] − scale·div[c]) / n`,
/// where air neighbours contribute `p = 0` but still count toward `n` (Dirichlet),
/// and solid neighbours are excluded entirely (Neumann). Only liquid cells are
/// updated; everything else stays 0.
pub fn jacobi_iteration(
    dims: &GridDims,
    cell_type: &[CellType],
    div: &[f32],
    scale: f32,
    p_in: &[f32],
    p_out: &mut [f32],
) {
    for k in 0..dims.nz {
        for j in 0..dims.ny {
            for i in 0..dims.nx {
                let c = dims.cell_idx(i, j, k);
                if cell_type[c] != CellType::Liquid {
                    p_out[c] = 0.0;
                    continue;
                }
                // Liquid cells are always interior (boundary is Solid), so all 6
                // neighbours are in range.
                let neighbours = [
                    dims.cell_idx(i - 1, j, k),
                    dims.cell_idx(i + 1, j, k),
                    dims.cell_idx(i, j - 1, k),
                    dims.cell_idx(i, j + 1, k),
                    dims.cell_idx(i, j, k - 1),
                    dims.cell_idx(i, j, k + 1),
                ];
                let mut sum = 0.0f32;
                let mut n = 0.0f32;
                for &nb in &neighbours {
                    match cell_type[nb] {
                        CellType::Solid => {}      // Neumann: excluded
                        CellType::Air => n += 1.0, // Dirichlet p=0
                        CellType::Liquid => {
                            sum += p_in[nb];
                            n += 1.0;
                        }
                    }
                }
                p_out[c] = if n > 0.0 {
                    (sum - scale * div[c]) / n
                } else {
                    0.0
                };
            }
        }
    }
}

/// Run `iters` Jacobi sweeps (ping-pong) and return the pressure field.
pub fn jacobi_solve(
    dims: &GridDims,
    cell_type: &[CellType],
    div: &[f32],
    params: ProjectionParams,
    iters: usize,
) -> Vec<f32> {
    let scale = params.rhs_scale(dims.h);
    let mut a = vec![0.0f32; dims.cell_count()];
    let mut b = vec![0.0f32; dims.cell_count()];
    for _ in 0..iters {
        jacobi_iteration(dims, cell_type, div, scale, &a, &mut b);
        std::mem::swap(&mut a, &mut b);
    }
    a
}

/// Apply the symmetric pressure-Poisson operator `A` to a field `x`, restricted to
/// liquid cells. `(A x)_c = n_c·x_c − Σ_{liquid nb} x_nb`, where `n_c` is the count
/// of non-solid (liquid+air) neighbours. Air neighbours raise `n_c` but contribute
/// `x=0` (Dirichlet); solid neighbours are excluded (Neumann). Non-liquid cells → 0.
///
/// This is exactly the operator implied by `jacobi_iteration` (`n·p_c − Σ p_nb =
/// −scale·div`), written as a single SPD matvec so a Krylov solver (CG) can use it.
pub fn apply_poisson(dims: &GridDims, cell_type: &[CellType], x: &[f32]) -> Vec<f32> {
    let mut out = vec![0.0f32; dims.cell_count()];
    for k in 0..dims.nz {
        for j in 0..dims.ny {
            for i in 0..dims.nx {
                let c = dims.cell_idx(i, j, k);
                if cell_type[c] != CellType::Liquid {
                    continue;
                }
                let neighbours = [
                    dims.cell_idx(i - 1, j, k),
                    dims.cell_idx(i + 1, j, k),
                    dims.cell_idx(i, j - 1, k),
                    dims.cell_idx(i, j + 1, k),
                    dims.cell_idx(i, j, k - 1),
                    dims.cell_idx(i, j, k + 1),
                ];
                let mut n = 0.0f32;
                let mut sum = 0.0f32;
                for &nb in &neighbours {
                    match cell_type[nb] {
                        CellType::Solid => {}      // Neumann: excluded
                        CellType::Air => n += 1.0, // Dirichlet p=0
                        CellType::Liquid => {
                            n += 1.0;
                            sum += x[nb];
                        }
                    }
                }
                out[c] = n * x[c] - sum;
            }
        }
    }
    out
}

/// Dot product over all cells (non-liquid entries are 0 by construction).
fn dot(a: &[f32], b: &[f32]) -> f64 {
    let mut acc = 0.0f64;
    for c in 0..a.len() {
        acc += (a[c] as f64) * (b[c] as f64);
    }
    acc
}

/// Solve the pressure Poisson system with **Conjugate Gradient** (unpreconditioned).
/// Same `A`/`b` as `jacobi_solve` (`A p = b`, `b_c = −scale·div_c` over liquid),
/// but Krylov convergence is O(N) iterations instead of Jacobi's O(N²) — it resolves
/// the low-frequency (deep-column hydrostatic) mode that under-resolves a settled
/// pool. The GPU WGSL port (`pressure` passes in `gpu/fluid.rs`) mirrors this math.
pub fn cg_solve(
    dims: &GridDims,
    cell_type: &[CellType],
    div: &[f32],
    params: ProjectionParams,
    iters: usize,
) -> Vec<f32> {
    let scale = params.rhs_scale(dims.h);
    let n = dims.cell_count();

    // b = -scale*div on liquid cells, 0 elsewhere.  p0 = 0  →  r0 = b - A·0 = b.
    let mut b = vec![0.0f32; n];
    for c in 0..n {
        if cell_type[c] == CellType::Liquid {
            b[c] = -scale * div[c];
        }
    }
    let mut p = vec![0.0f32; n];
    let mut r = b.clone();
    let mut d = r.clone();
    let mut rs_old = dot(&r, &r);

    for _ in 0..iters {
        if rs_old <= 0.0 {
            break; // exactly converged
        }
        let q = apply_poisson(dims, cell_type, &d);
        let dq = dot(&d, &q);
        if dq.abs() < 1e-30 {
            break;
        }
        let alpha = (rs_old / dq) as f32;
        for c in 0..n {
            p[c] += alpha * d[c];
            r[c] -= alpha * q[c];
        }
        let rs_new = dot(&r, &r);
        let beta = (rs_new / rs_old) as f32;
        for c in 0..n {
            d[c] = r[c] + beta * d[c];
        }
        rs_old = rs_new;
    }
    p
}

/// Subtract the pressure gradient from the face velocities. A face is updated only
/// if both adjacent cells are non-solid (Neumann otherwise); faces adjacent to a
/// solid are forced to zero (no normal flow at walls).
pub fn subtract_gradient(
    dims: &GridDims,
    params: ProjectionParams,
    p: &[f32],
    cell_type: &[CellType],
    u: &mut [f32],
    v: &mut [f32],
    w: &mut [f32],
) {
    let coeff = params.grad_coeff(dims.h);

    // u faces: between cell (i-1,j,k) [lo] and (i,j,k) [hi], for i in 0..=nx.
    for k in 0..dims.nz {
        for j in 0..dims.ny {
            for i in 0..=dims.nx {
                let idx = dims.u_idx(i, j, k);
                let lo = if i == 0 {
                    None
                } else {
                    Some(dims.cell_idx(i - 1, j, k))
                };
                let hi = if i == dims.nx {
                    None
                } else {
                    Some(dims.cell_idx(i, j, k))
                };
                apply_face(coeff, p, cell_type, lo, hi, &mut u[idx]);
            }
        }
    }
    // v faces.
    for k in 0..dims.nz {
        for j in 0..=dims.ny {
            for i in 0..dims.nx {
                let idx = dims.v_idx(i, j, k);
                let lo = if j == 0 {
                    None
                } else {
                    Some(dims.cell_idx(i, j - 1, k))
                };
                let hi = if j == dims.ny {
                    None
                } else {
                    Some(dims.cell_idx(i, j, k))
                };
                apply_face(coeff, p, cell_type, lo, hi, &mut v[idx]);
            }
        }
    }
    // w faces.
    for k in 0..=dims.nz {
        for j in 0..dims.ny {
            for i in 0..dims.nx {
                let idx = dims.w_idx(i, j, k);
                let lo = if k == 0 {
                    None
                } else {
                    Some(dims.cell_idx(i, j, k - 1))
                };
                let hi = if k == dims.nz {
                    None
                } else {
                    Some(dims.cell_idx(i, j, k))
                };
                apply_face(coeff, p, cell_type, lo, hi, &mut w[idx]);
            }
        }
    }
}

#[inline]
fn apply_face(
    coeff: f32,
    p: &[f32],
    cell_type: &[CellType],
    lo: Option<usize>,
    hi: Option<usize>,
    vel: &mut f32,
) {
    let lo_solid = lo.map(|c| cell_type[c] == CellType::Solid).unwrap_or(true);
    let hi_solid = hi.map(|c| cell_type[c] == CellType::Solid).unwrap_or(true);
    if lo_solid || hi_solid {
        // Face touches a wall (or domain edge): no normal flow.
        *vel = 0.0;
        return;
    }
    let p_hi = hi.map(|c| p[c]).unwrap_or(0.0);
    let p_lo = lo.map(|c| p[c]).unwrap_or(0.0);
    *vel -= coeff * (p_hi - p_lo);
}

// --- diagnostics (used by tests and, later, throttled profiler snapshots) ---

/// L2 norm of divergence over liquid cells.
pub fn l2_divergence(dims: &GridDims, div: &[f32], cell_type: &[CellType]) -> f32 {
    let mut acc = 0.0f64;
    for c in 0..dims.cell_count() {
        if cell_type[c] == CellType::Liquid {
            acc += (div[c] as f64) * (div[c] as f64);
        }
    }
    acc.sqrt() as f32
}

/// Max |divergence| over liquid cells.
pub fn max_abs_divergence(dims: &GridDims, div: &[f32], cell_type: &[CellType]) -> f32 {
    let mut m = 0.0f32;
    for c in 0..dims.cell_count() {
        if cell_type[c] == CellType::Liquid {
            m = m.max(div[c].abs());
        }
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::*;

    // Deterministic [-1,1) pseudo-random for repeatable test fields.
    fn lcg(state: &mut u32) -> f32 {
        *state = state.wrapping_mul(1664525).wrapping_add(1013904223);
        ((*state >> 8) as f32 / (1u32 << 24) as f32) * 2.0 - 1.0
    }

    /// Build a 16³ tank: walls solid, top interior layer air (free surface),
    /// rest liquid. Fill interior faces with a deterministic divergent field,
    /// zero wall faces, then assert Jacobi projection drives divergence down.
    #[test]
    fn jacobi_reduces_divergence_16cubed() {
        let dims = GridDims::cubic(16, 1.0);
        let params = ProjectionParams::unit();

        // Cell types: interior liquid, except top interior layer (j = ny-2) = air.
        let mut occ = vec![false; dims.cell_count()];
        for k in 1..dims.nz - 1 {
            for j in 1..dims.ny - 1 {
                for i in 1..dims.nx - 1 {
                    if j != dims.ny - 2 {
                        occ[dims.cell_idx(i, j, k)] = true;
                    }
                }
            }
        }
        let cell_type = classify_cells(&dims, &occ);

        // Sanity: top interior layer is air, layer below is liquid.
        assert_eq!(cell_type[dims.cell_idx(8, dims.ny - 2, 8)], CellType::Air);
        assert_eq!(
            cell_type[dims.cell_idx(8, dims.ny - 3, 8)],
            CellType::Liquid
        );

        // Deterministic divergent velocity field.
        let mut st = 12345u32;
        let mut u = vec![0.0f32; dims.u_count()];
        let mut v = vec![0.0f32; dims.v_count()];
        let mut w = vec![0.0f32; dims.w_count()];
        for x in u.iter_mut() {
            *x = lcg(&mut st);
        }
        for x in v.iter_mut() {
            *x = lcg(&mut st);
        }
        for x in w.iter_mut() {
            *x = lcg(&mut st);
        }

        // Zero faces touching solids so the only divergence is inside the fluid
        // (also what enforce_solid_boundaries does before the solve).
        zero_solid_faces(&dims, &cell_type, &mut u, &mut v, &mut w);

        let div0 = compute_divergence(&dims, &u, &v, &w, &cell_type);
        let l2_before = l2_divergence(&dims, &div0, &cell_type);
        let max_before = max_abs_divergence(&dims, &div0, &cell_type);
        assert!(l2_before > 1.0, "test field should start clearly divergent");

        let p = jacobi_solve(&dims, &cell_type, &div0, params, 200);
        subtract_gradient(&dims, params, &p, &cell_type, &mut u, &mut v, &mut w);

        let div1 = compute_divergence(&dims, &u, &v, &w, &cell_type);
        let l2_after = l2_divergence(&dims, &div1, &cell_type);
        let max_after = max_abs_divergence(&dims, &div1, &cell_type);

        println!(
            "divergence L2: {l2_before:.4} -> {l2_after:.4} ({:.1}% of original); \
             max |div|: {max_before:.4} -> {max_after:.4}; iters=200",
            100.0 * l2_after / l2_before
        );

        assert!(
            l2_after < 0.1 * l2_before,
            "Jacobi projection should cut L2 divergence by >90%: before={l2_before}, after={l2_after}"
        );
        assert!(max_after < max_before, "max divergence should decrease");
    }

    /// Zero every face adjacent to a solid cell or the domain edge.
    fn zero_solid_faces(
        dims: &GridDims,
        ct: &[CellType],
        u: &mut [f32],
        v: &mut [f32],
        w: &mut [f32],
    ) {
        for k in 0..dims.nz {
            for j in 0..dims.ny {
                for i in 0..=dims.nx {
                    let lo_solid = i == 0 || ct[dims.cell_idx(i - 1, j, k)] == CellType::Solid;
                    let hi_solid = i == dims.nx || ct[dims.cell_idx(i, j, k)] == CellType::Solid;
                    if lo_solid || hi_solid {
                        u[dims.u_idx(i, j, k)] = 0.0;
                    }
                }
            }
        }
        for k in 0..dims.nz {
            for j in 0..=dims.ny {
                for i in 0..dims.nx {
                    let lo_solid = j == 0 || ct[dims.cell_idx(i, j - 1, k)] == CellType::Solid;
                    let hi_solid = j == dims.ny || ct[dims.cell_idx(i, j, k)] == CellType::Solid;
                    if lo_solid || hi_solid {
                        v[dims.v_idx(i, j, k)] = 0.0;
                    }
                }
            }
        }
        for k in 0..=dims.nz {
            for j in 0..dims.ny {
                for i in 0..dims.nx {
                    let lo_solid = k == 0 || ct[dims.cell_idx(i, j, k - 1)] == CellType::Solid;
                    let hi_solid = k == dims.nz || ct[dims.cell_idx(i, j, k)] == CellType::Solid;
                    if lo_solid || hi_solid {
                        w[dims.w_idx(i, j, k)] = 0.0;
                    }
                }
            }
        }
    }

    /// Build the same 16³ divergent tank and assert CG converges in *far fewer*
    /// iterations than Jacobi — the whole point of the solver upgrade (it resolves
    /// the low-frequency mode that compacts a settled pool). 40 CG iters should
    /// beat 200 Jacobi iters on residual divergence.
    #[test]
    fn cg_beats_jacobi_16cubed() {
        let dims = GridDims::cubic(16, 1.0);
        let params = ProjectionParams::unit();

        let mut occ = vec![false; dims.cell_count()];
        for k in 1..dims.nz - 1 {
            for j in 1..dims.ny - 1 {
                for i in 1..dims.nx - 1 {
                    if j != dims.ny - 2 {
                        occ[dims.cell_idx(i, j, k)] = true;
                    }
                }
            }
        }
        let cell_type = classify_cells(&dims, &occ);

        let mut st = 12345u32;
        let mut u = vec![0.0f32; dims.u_count()];
        let mut v = vec![0.0f32; dims.v_count()];
        let mut w = vec![0.0f32; dims.w_count()];
        for x in u.iter_mut() {
            *x = lcg(&mut st);
        }
        for x in v.iter_mut() {
            *x = lcg(&mut st);
        }
        for x in w.iter_mut() {
            *x = lcg(&mut st);
        }
        zero_solid_faces(&dims, &cell_type, &mut u, &mut v, &mut w);

        let div0 = compute_divergence(&dims, &u, &v, &w, &cell_type);
        let l2_before = l2_divergence(&dims, &div0, &cell_type);
        assert!(l2_before > 1.0);

        // Jacobi reference at 200 iters.
        let pj = jacobi_solve(&dims, &cell_type, &div0, params, 200);
        let (mut uj, mut vj, mut wj) = (u.clone(), v.clone(), w.clone());
        subtract_gradient(&dims, params, &pj, &cell_type, &mut uj, &mut vj, &mut wj);
        let l2_jac = l2_divergence(
            &dims,
            &compute_divergence(&dims, &uj, &vj, &wj, &cell_type),
            &cell_type,
        );

        // CG at 40 iters.
        let pc = cg_solve(&dims, &cell_type, &div0, params, 40);
        let (mut uc, mut vc, mut wc) = (u.clone(), v.clone(), w.clone());
        subtract_gradient(&dims, params, &pc, &cell_type, &mut uc, &mut vc, &mut wc);
        let l2_cg = l2_divergence(
            &dims,
            &compute_divergence(&dims, &uc, &vc, &wc, &cell_type),
            &cell_type,
        );

        println!(
            "16^3 residual L2 (start {l2_before:.4}): Jacobi-200 -> {l2_jac:.6}, CG-40 -> {l2_cg:.6}"
        );

        // CG-40 should be at least as good as Jacobi-200, and well under 1% of start.
        assert!(
            l2_cg < 0.01 * l2_before,
            "CG-40 should cut L2 div by >99%: {l2_cg}"
        );
        assert!(
            l2_cg <= l2_jac,
            "CG-40 should beat Jacobi-200: cg={l2_cg} jac={l2_jac}"
        );
    }

    #[test]
    fn divergence_free_field_has_zero_divergence() {
        // A uniform field (all faces equal) is divergence-free in the interior.
        let dims = GridDims::cubic(8, 0.5);
        let mut occ = vec![false; dims.cell_count()];
        for k in 1..dims.nz - 1 {
            for j in 1..dims.ny - 1 {
                for i in 1..dims.nx - 1 {
                    occ[dims.cell_idx(i, j, k)] = true;
                }
            }
        }
        let ct = classify_cells(&dims, &occ);
        let u = vec![3.0; dims.u_count()];
        let v = vec![-2.0; dims.v_count()];
        let w = vec![1.5; dims.w_count()];
        let div = compute_divergence(&dims, &u, &v, &w, &ct);
        assert!(max_abs_divergence(&dims, &div, &ct) < 1e-5);
    }
}
