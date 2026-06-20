//! Simulation math reference.
//!
//! This module is the *spec-as-code* for the MAC grid: indexing, buffer sizing,
//! world↔grid mapping, cell typing, divergence, and a Jacobi pressure solve. These
//! are the functions the WGSL path mirrors, so they are unit-tested here on the
//! host. There is intentionally **no** time-stepped CPU sim loop (no gravity,
//! advection, P2G, or G2P here); the current runtime map lives in
//! `docs/architecture/simulation.md`.
//!
//! Conventions:
//! - Staggered MAC grid, `nx*ny*nz` cells. Cell index `i + nx*(j + ny*k)`.
//! - Face counts: u=(nx+1)*ny*nz, v=nx*(ny+1)*nz, w=nx*ny*(nz+1).
//! - `u[i,j,k]` is the x-velocity on the face at the low-x side of cell (i,j,k),
//!   shared with cell (i-1,j,k). Likewise v in y, w in z.
//! - Cell centers at `origin + (i+0.5, j+0.5, k+0.5)*h`.
//! - Tank walls: all boundary cells are Solid, so every Liquid cell is interior
//!   and always has in-range 6-neighbours (no bounds checks in the solver).

pub mod pressure;

/// Base (uniform) cell size in world units. An all-64 grid reproduces the exact
/// original `[-1,1]^3` cube: extent `64 * H = 2.0`, centered origin `-1.0`. The
/// cell size stays uniform across all axes; the tank becomes rectangular only by
/// varying per-axis cell counts (`nx`, `ny`, `nz`), so the pressure operator stays
/// isotropic (`1/H^2`).
pub const H: f32 = 2.0 / 64.0;

/// Cell classification at cell centers. `u32` repr so it maps directly to a WGSL
/// storage buffer of `u32`.
#[repr(u32)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CellType {
    Solid = 0,
    Liquid = 1,
    Air = 2,
}

/// MAC grid dimensions in cells, plus world placement.
#[derive(Clone, Copy, Debug)]
pub struct GridDims {
    pub nx: usize,
    pub ny: usize,
    pub nz: usize,
    /// Cell size in world units (uniform/cubic).
    pub h: f32,
    /// World-space minimum corner of cell (0,0,0).
    pub origin: [f32; 3],
}

impl GridDims {
    pub fn cubic(n: usize, h: f32) -> Self {
        GridDims {
            nx: n,
            ny: n,
            nz: n,
            h,
            origin: [0.0, 0.0, 0.0],
        }
    }

    // --- counts (buffer sizing) ---
    pub fn cell_count(&self) -> usize {
        self.nx * self.ny * self.nz
    }
    pub fn u_count(&self) -> usize {
        (self.nx + 1) * self.ny * self.nz
    }
    pub fn v_count(&self) -> usize {
        self.nx * (self.ny + 1) * self.nz
    }
    pub fn w_count(&self) -> usize {
        self.nx * self.ny * (self.nz + 1)
    }

    // --- indexing ---
    #[inline]
    pub fn cell_idx(&self, i: usize, j: usize, k: usize) -> usize {
        i + self.nx * (j + self.ny * k)
    }
    #[inline]
    pub fn u_idx(&self, i: usize, j: usize, k: usize) -> usize {
        // u dims: (nx+1, ny, nz)
        i + (self.nx + 1) * (j + self.ny * k)
    }
    #[inline]
    pub fn v_idx(&self, i: usize, j: usize, k: usize) -> usize {
        // v dims: (nx, ny+1, nz)
        i + self.nx * (j + (self.ny + 1) * k)
    }
    #[inline]
    pub fn w_idx(&self, i: usize, j: usize, k: usize) -> usize {
        // w dims: (nx, ny, nz+1)
        i + self.nx * (j + self.ny * k)
    }

    #[inline]
    pub fn in_cell_range(&self, i: i64, j: i64, k: i64) -> bool {
        i >= 0
            && j >= 0
            && k >= 0
            && (i as usize) < self.nx
            && (j as usize) < self.ny
            && (k as usize) < self.nz
    }

    /// True if (i,j,k) is on the tank wall (boundary cell → Solid by convention).
    #[inline]
    pub fn is_boundary_cell(&self, i: usize, j: usize, k: usize) -> bool {
        i == 0 || j == 0 || k == 0 || i == self.nx - 1 || j == self.ny - 1 || k == self.nz - 1
    }

    // --- world <-> grid ---
    pub fn cell_center_world(&self, i: usize, j: usize, k: usize) -> [f32; 3] {
        [
            self.origin[0] + (i as f32 + 0.5) * self.h,
            self.origin[1] + (j as f32 + 0.5) * self.h,
            self.origin[2] + (k as f32 + 0.5) * self.h,
        ]
    }

    /// Map a world position to the containing cell index, clamped into range.
    /// Out-of-range positions clamp to the nearest cell (recovery is handled by
    /// the particle policy in 0.3; this just never indexes out of bounds).
    pub fn world_to_cell(&self, p: [f32; 3]) -> (usize, usize, usize) {
        let fi = ((p[0] - self.origin[0]) / self.h).floor();
        let fj = ((p[1] - self.origin[1]) / self.h).floor();
        let fk = ((p[2] - self.origin[2]) / self.h).floor();
        let ci = clamp_idx(fi, self.nx);
        let cj = clamp_idx(fj, self.ny);
        let ck = clamp_idx(fk, self.nz);
        (ci, cj, ck)
    }
}

fn clamp_idx(f: f32, n: usize) -> usize {
    if f < 0.0 {
        0
    } else {
        let i = f as usize;
        if i >= n {
            n - 1
        } else {
            i
        }
    }
}

/// Classify every cell: boundary → Solid; interior occupied → Liquid; interior
/// empty → Air. `occupied` is indexed by `cell_idx` and is typically produced by
/// binning particles (see [`mark_occupancy_from_particles`]).
pub fn classify_cells(dims: &GridDims, occupied: &[bool]) -> Vec<CellType> {
    assert_eq!(occupied.len(), dims.cell_count());
    let mut out = vec![CellType::Air; dims.cell_count()];
    for k in 0..dims.nz {
        for j in 0..dims.ny {
            for i in 0..dims.nx {
                let c = dims.cell_idx(i, j, k);
                out[c] = if dims.is_boundary_cell(i, j, k) {
                    CellType::Solid
                } else if occupied[c] {
                    CellType::Liquid
                } else {
                    CellType::Air
                };
            }
        }
    }
    out
}

/// Bin particle positions into an occupancy bitmap (one bool per cell). Interior
/// cells containing ≥1 particle are marked occupied. Boundary cells are ignored
/// here (they become Solid in classification regardless).
pub fn mark_occupancy_from_particles(dims: &GridDims, positions: &[[f32; 3]]) -> Vec<bool> {
    let mut occ = vec![false; dims.cell_count()];
    for &p in positions {
        let (i, j, k) = dims.world_to_cell(p);
        occ[dims.cell_idx(i, j, k)] = true;
    }
    occ
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buffer_sizes_match_staggered_layout() {
        let d = GridDims::cubic(16, 1.0);
        assert_eq!(d.cell_count(), 16 * 16 * 16);
        assert_eq!(d.u_count(), 17 * 16 * 16);
        assert_eq!(d.v_count(), 16 * 17 * 16);
        assert_eq!(d.w_count(), 16 * 16 * 17);
        // Non-cubic to catch axis mix-ups.
        let d2 = GridDims {
            nx: 4,
            ny: 5,
            nz: 6,
            h: 1.0,
            origin: [0.0; 3],
        };
        assert_eq!(d2.u_count(), 5 * 5 * 6);
        assert_eq!(d2.v_count(), 4 * 6 * 6);
        assert_eq!(d2.w_count(), 4 * 5 * 7);
    }

    #[test]
    fn cell_and_face_indices_are_unique_and_in_range() {
        let d = GridDims {
            nx: 4,
            ny: 5,
            nz: 6,
            h: 1.0,
            origin: [0.0; 3],
        };
        // Cell indices: bijection onto 0..cell_count.
        let mut seen = vec![false; d.cell_count()];
        for k in 0..d.nz {
            for j in 0..d.ny {
                for i in 0..d.nx {
                    let c = d.cell_idx(i, j, k);
                    assert!(c < d.cell_count());
                    assert!(!seen[c], "duplicate cell index");
                    seen[c] = true;
                }
            }
        }
        assert!(seen.iter().all(|&b| b));

        // u faces: bijection onto 0..u_count.
        let mut su = vec![false; d.u_count()];
        for k in 0..d.nz {
            for j in 0..d.ny {
                for i in 0..=d.nx {
                    let idx = d.u_idx(i, j, k);
                    assert!(idx < d.u_count());
                    assert!(!su[idx]);
                    su[idx] = true;
                }
            }
        }
        assert!(su.iter().all(|&b| b));
    }

    #[test]
    fn world_to_cell_roundtrips_centers_and_clamps_escapes() {
        let d = GridDims {
            nx: 8,
            ny: 8,
            nz: 8,
            h: 0.25,
            origin: [-1.0, -1.0, -1.0],
        };
        // Center of cell (3,4,5) must map back to (3,4,5).
        let c = d.cell_center_world(3, 4, 5);
        assert_eq!(d.world_to_cell(c), (3, 4, 5));
        // Escaped particle below origin clamps to cell 0; far above clamps to n-1.
        assert_eq!(d.world_to_cell([-100.0, -100.0, -100.0]), (0, 0, 0));
        assert_eq!(d.world_to_cell([100.0, 100.0, 100.0]), (7, 7, 7));
    }

    #[test]
    fn boundary_cells_are_solid_interior_marked_by_occupancy() {
        let d = GridDims::cubic(6, 1.0);
        let mut occ = vec![false; d.cell_count()];
        occ[d.cell_idx(2, 2, 2)] = true; // one interior liquid cell
        let ct = classify_cells(&d, &occ);
        // Every face of the box is solid.
        assert_eq!(ct[d.cell_idx(0, 3, 3)], CellType::Solid);
        assert_eq!(ct[d.cell_idx(5, 3, 3)], CellType::Solid);
        assert_eq!(ct[d.cell_idx(3, 0, 3)], CellType::Solid);
        // Occupied interior = liquid; empty interior = air.
        assert_eq!(ct[d.cell_idx(2, 2, 2)], CellType::Liquid);
        assert_eq!(ct[d.cell_idx(3, 3, 3)], CellType::Air);
    }

    #[test]
    fn particles_mark_their_containing_cells() {
        let d = GridDims::cubic(8, 1.0);
        // Two particles in distinct interior cells.
        let pos = [[2.5, 3.5, 4.5], [5.5, 5.5, 5.5]];
        let occ = mark_occupancy_from_particles(&d, &pos);
        assert!(occ[d.cell_idx(2, 3, 4)]);
        assert!(occ[d.cell_idx(5, 5, 5)]);
        let ct = classify_cells(&d, &occ);
        assert_eq!(ct[d.cell_idx(2, 3, 4)], CellType::Liquid);
        assert_eq!(ct[d.cell_idx(5, 5, 5)], CellType::Liquid);
    }

    // --- rectangular (nx != ny != nz) coverage for the v1.1 box refactor ---

    /// A deliberately non-cubic, off-center tank to catch axis mix-ups in the
    /// world<->grid mapping and the staggered face indexers.
    fn rect_dims() -> GridDims {
        GridDims {
            nx: 5,
            ny: 9,
            nz: 7,
            h: 0.3,
            origin: [-0.75, 1.35, -2.1],
        }
    }

    #[test]
    fn world_to_cell_roundtrips_on_rectangular_tank() {
        let d = rect_dims();
        // Every cell center must round-trip back to its own index.
        for k in 0..d.nz {
            for j in 0..d.ny {
                for i in 0..d.nx {
                    let c = d.cell_center_world(i, j, k);
                    assert_eq!(
                        d.world_to_cell(c),
                        (i, j, k),
                        "center of ({i},{j},{k}) did not round-trip"
                    );
                }
            }
        }
        // Escapes clamp to the per-axis extremes (note nx-1, ny-1, nz-1 all differ).
        assert_eq!(d.world_to_cell([-1e3, -1e3, -1e3]), (0, 0, 0));
        assert_eq!(
            d.world_to_cell([1e3, 1e3, 1e3]),
            (d.nx - 1, d.ny - 1, d.nz - 1)
        );
    }

    #[test]
    fn cell_center_world_uses_per_axis_origin_and_uniform_h() {
        let d = rect_dims();
        let c = d.cell_center_world(2, 3, 4);
        let expected = [
            d.origin[0] + 2.5 * d.h,
            d.origin[1] + 3.5 * d.h,
            d.origin[2] + 4.5 * d.h,
        ];
        for a in 0..3 {
            assert!(
                (c[a] - expected[a]).abs() < 1e-6,
                "axis {a}: {} != {}",
                c[a],
                expected[a]
            );
        }
    }

    #[test]
    fn face_indexers_are_bijections_on_rectangular_tank() {
        let d = rect_dims();
        // u faces: dims (nx+1, ny, nz).
        let mut su = vec![false; d.u_count()];
        for k in 0..d.nz {
            for j in 0..d.ny {
                for i in 0..=d.nx {
                    let idx = d.u_idx(i, j, k);
                    assert!(idx < d.u_count(), "u idx out of range");
                    assert!(!su[idx], "duplicate u idx");
                    su[idx] = true;
                }
            }
        }
        assert!(su.iter().all(|&b| b));

        // v faces: dims (nx, ny+1, nz).
        let mut sv = vec![false; d.v_count()];
        for k in 0..d.nz {
            for j in 0..=d.ny {
                for i in 0..d.nx {
                    let idx = d.v_idx(i, j, k);
                    assert!(idx < d.v_count(), "v idx out of range");
                    assert!(!sv[idx], "duplicate v idx");
                    sv[idx] = true;
                }
            }
        }
        assert!(sv.iter().all(|&b| b));

        // w faces: dims (nx, ny, nz+1).
        let mut sw = vec![false; d.w_count()];
        for k in 0..=d.nz {
            for j in 0..d.ny {
                for i in 0..d.nx {
                    let idx = d.w_idx(i, j, k);
                    assert!(idx < d.w_count(), "w idx out of range");
                    assert!(!sw[idx], "duplicate w idx");
                    sw[idx] = true;
                }
            }
        }
        assert!(sw.iter().all(|&b| b));
    }

    #[derive(Clone, Copy)]
    enum Axis {
        U,
        V,
        W,
    }

    fn mac_face_count(d: &GridDims, axis: Axis) -> usize {
        match axis {
            Axis::U => d.u_count(),
            Axis::V => d.v_count(),
            Axis::W => d.w_count(),
        }
    }

    fn mac_face_idx(d: &GridDims, axis: Axis, i: usize, j: usize, k: usize) -> usize {
        match axis {
            Axis::U => d.u_idx(i, j, k),
            Axis::V => d.v_idx(i, j, k),
            Axis::W => d.w_idx(i, j, k),
        }
    }

    fn mac_face_dims(d: &GridDims, axis: Axis) -> (usize, usize, usize) {
        match axis {
            Axis::U => (d.nx + 1, d.ny, d.nz),
            Axis::V => (d.nx, d.ny + 1, d.nz),
            Axis::W => (d.nx, d.ny, d.nz + 1),
        }
    }

    fn mac_gather_offset(axis: Axis) -> [f32; 3] {
        match axis {
            Axis::U => [0.0, -0.5, -0.5],
            Axis::V => [-0.5, 0.0, -0.5],
            Axis::W => [-0.5, -0.5, 0.0],
        }
    }

    fn face_touches_static_solid(d: &GridDims, axis: Axis, i: usize, j: usize, k: usize) -> bool {
        match axis {
            Axis::U => {
                i <= 1 || i >= d.nx - 1 || j == 0 || j >= d.ny - 1 || k == 0 || k >= d.nz - 1
            }
            Axis::V => {
                i == 0 || i >= d.nx - 1 || j <= 1 || j >= d.ny - 1 || k == 0 || k >= d.nz - 1
            }
            Axis::W => {
                i == 0 || i >= d.nx - 1 || j == 0 || j >= d.ny - 1 || k <= 1 || k >= d.nz - 1
            }
        }
    }

    fn fill_wall_zeroed_faces(d: &GridDims, axis: Axis, value: f32) -> Vec<f32> {
        let mut out = vec![0.0; mac_face_count(d, axis)];
        let (nx, ny, nz) = mac_face_dims(d, axis);
        for k in 0..nz {
            for j in 0..ny {
                for i in 0..nx {
                    if !face_touches_static_solid(d, axis, i, j, k) {
                        let idx = mac_face_idx(d, axis, i, j, k);
                        out[idx] = value;
                    }
                }
            }
        }
        out
    }

    fn sample_mac_pair(
        d: &GridDims,
        axis: Axis,
        p: [f32; 3],
        final_vel: &[f32],
        saved_vel: &[f32],
        wall_aware: bool,
    ) -> (f32, f32) {
        let (nx, ny, nz) = mac_face_dims(d, axis);
        let off = mac_gather_offset(axis);
        let g = [
            (p[0] - d.origin[0]) / d.h + off[0],
            (p[1] - d.origin[1]) / d.h + off[1],
            (p[2] - d.origin[2]) / d.h + off[2],
        ];
        let base = [
            g[0].floor() as isize,
            g[1].floor() as isize,
            g[2].floor() as isize,
        ];
        let t = [
            g[0] - base[0] as f32,
            g[1] - base[1] as f32,
            g[2] - base[2] as f32,
        ];
        let mut fin = 0.0;
        let mut sav = 0.0;
        let mut wsum = 0.0;
        for dk in 0..2 {
            let k = base[2] + dk;
            if k < 0 || k >= nz as isize {
                continue;
            }
            let wz = if dk == 1 { t[2] } else { 1.0 - t[2] };
            for dj in 0..2 {
                let j = base[1] + dj;
                if j < 0 || j >= ny as isize {
                    continue;
                }
                let wy = if dj == 1 { t[1] } else { 1.0 - t[1] };
                for di in 0..2 {
                    let i = base[0] + di;
                    if i < 0 || i >= nx as isize {
                        continue;
                    }
                    let (i, j, k) = (i as usize, j as usize, k as usize);
                    if wall_aware && face_touches_static_solid(d, axis, i, j, k) {
                        continue;
                    }
                    let wx = if di == 1 { t[0] } else { 1.0 - t[0] };
                    let weight = wx * wy * wz;
                    let idx = mac_face_idx(d, axis, i, j, k);
                    fin += weight * final_vel[idx];
                    sav += weight * saved_vel[idx];
                    wsum += weight;
                }
            }
        }
        if wsum > 0.0 {
            (fin / wsum, sav / wsum)
        } else {
            (0.0, 0.0)
        }
    }

    #[test]
    fn wall_aware_mac_gather_preserves_tangential_velocity_near_static_wall() {
        let d = GridDims::cubic(8, 1.0);
        let final_v = fill_wall_zeroed_faces(&d, Axis::V, 10.0);
        let saved_v = fill_wall_zeroed_faces(&d, Axis::V, 7.0);
        let p = [1.05, 3.5, 3.5];

        let old = sample_mac_pair(&d, Axis::V, p, &final_v, &saved_v, false);
        let fixed = sample_mac_pair(&d, Axis::V, p, &final_v, &saved_v, true);

        assert!((old.0 - 5.5).abs() < 1e-6, "old final sample was {:?}", old);
        assert!(
            (old.1 - 3.85).abs() < 1e-6,
            "old saved sample was {:?}",
            old
        );
        assert!(
            (fixed.0 - 10.0).abs() < 1e-6,
            "fixed final sample was {:?}",
            fixed
        );
        assert!(
            (fixed.1 - 7.0).abs() < 1e-6,
            "fixed saved sample was {:?}",
            fixed
        );
    }

    #[test]
    fn wall_aware_mac_gather_preserves_away_from_ceiling_normal_velocity() {
        let d = GridDims::cubic(8, 1.0);
        let final_v = fill_wall_zeroed_faces(&d, Axis::V, -10.0);
        let saved_v = fill_wall_zeroed_faces(&d, Axis::V, -6.0);
        let p = [3.5, 6.95, 3.5];

        let old = sample_mac_pair(&d, Axis::V, p, &final_v, &saved_v, false);
        let fixed = sample_mac_pair(&d, Axis::V, p, &final_v, &saved_v, true);

        assert!(
            (old.0 - -0.5).abs() < 1e-5,
            "old final sample was {:?}",
            old
        );
        assert!(
            (old.1 - -0.3).abs() < 1e-5,
            "old saved sample was {:?}",
            old
        );
        assert!(
            (fixed.0 - -10.0).abs() < 1e-6,
            "fixed final sample was {:?}",
            fixed
        );
        assert!(
            (fixed.1 - -6.0).abs() < 1e-6,
            "fixed saved sample was {:?}",
            fixed
        );
    }

    #[test]
    fn all_64_tank_reproduces_the_original_unit_cube() {
        // The default all-64 grid with the base cell size H must reproduce the
        // historical [-1,1]^3 cube: extent 2.0, centered origin -1.0.
        let n = 64usize;
        let origin = [-(n as f32) * H / 2.0; 3];
        let d = GridDims {
            nx: n,
            ny: n,
            nz: n,
            h: H,
            origin,
        };
        assert!((d.h * n as f32 - 2.0).abs() < 1e-6, "extent != 2.0");
        assert!((d.origin[0] - (-1.0)).abs() < 1e-6, "origin != -1.0");
    }
}
