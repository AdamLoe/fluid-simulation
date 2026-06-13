//! Scene configuration — Phase 1.3 scripted scenarios.
//!
//! Per `decisions.md` (use a tiny scene config before the full scenario system):
//! a `SceneConfig` is built from the settings registry instead of being hardcoded
//! into solver code. 1.3 adds deterministic presets (a scene selector) on top of
//! that shape: each preset is just a different set of initial liquid blocks, all
//! released into the same closed tank. Static interior solids stay OUT
//! (static-before-dynamic, see `decisions.md`).

use crate::settings::Registry;
use glam::{UVec3, Vec3};

const DEFAULT_DROP_HEIGHT: f32 = 0.72;

/// Reference particle density that reproduces the historical look (8 particles
/// per seeded cell). Used by the renderer to keep splat coverage volume-neutral
/// and by the auto surface-dilation trigger. Mirrors `particles.density`'s default.
pub const REFERENCE_DENSITY: f32 = 8.0;

/// Default `scene.fill_level` as a [0,1] tank-fill fraction (20% of tank height).
/// The registry stores this as a 0–100 percentage; `Registry::fill_level()`
/// converts to this fraction. `fill_level` is a literal waterline: the resting
/// fluid is a full-footprint floor slab from y=0 up to `fill_level` of the tank
/// height, so `fill_level = 1.0` fills the whole tank and `0.2` fills the bottom
/// fifth. See `preset_blocks` for how the dynamic scenarios scale with it.
pub const DEFAULT_FILL_LEVEL: f32 = 0.2;

/// Selectable scripted scenarios. The integer values are the wire format of the
/// `scene.preset` registry setting (a small enum exposed as a dropdown in the
/// config panel). Adding a variant = add a match arm here + a label in
/// `Registry::enum_options`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScenePreset {
    /// A blob of liquid in the upper-middle of the tank that falls and splashes.
    /// This is the historical default look.
    FallingBlob = 0,
    /// A tall column of liquid held against one wall, released to slam across the
    /// tank — the classic high-impact dam-break.
    DamBreak = 1,
    /// Two separated columns that fall and collide in the middle, throwing a
    /// double crown splash.
    DoubleSplash = 2,
}

impl ScenePreset {
    pub fn from_u32(v: u32) -> Self {
        match v {
            1 => ScenePreset::DamBreak,
            2 => ScenePreset::DoubleSplash,
            _ => ScenePreset::FallingBlob,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            ScenePreset::FallingBlob => "falling-blob",
            ScenePreset::DamBreak => "dam-break",
            ScenePreset::DoubleSplash => "double-splash",
        }
    }
}

/// An axis-aligned block of liquid in normalized tank space [0,1]^3 (y up).
#[derive(Clone, Copy)]
pub struct LiquidBlock {
    pub min: Vec3,
    pub max: Vec3,
}

impl LiquidBlock {
    fn new(min: [f32; 3], max: [f32; 3]) -> Self {
        Self {
            min: Vec3::from(min),
            max: Vec3::from(max),
        }
    }
}

#[derive(Clone)]
pub struct InitialLiquidConfig {
    /// One or more disjoint AABB regions of liquid, all in normalized tank space.
    /// The requested particle count is distributed across them by volume.
    pub blocks: Vec<LiquidBlock>,
}

#[derive(Clone)]
pub struct SceneConfig {
    pub name: String,
    pub preset: ScenePreset,
    pub grid_resolution: UVec3,
    pub particle_count: u32,
    pub initial_liquid: InitialLiquidConfig,
    // Solids: only tank walls. Static interior solids remain deferred.
}

impl SceneConfig {
    /// Build the scene from the current registry: grid resolution, particle count,
    /// and the selected `scene.preset`.
    pub fn from_settings(settings: &Registry) -> Self {
        let res = UVec3::new(
            settings.grid_res_x(),
            settings.grid_res_y(),
            settings.grid_res_z(),
        );
        let preset = ScenePreset::from_u32(settings.scene_preset());
        let blocks = preset_blocks(preset, settings.drop_height(), settings.fill_level());
        Self {
            name: preset.name().to_string(),
            preset,
            grid_resolution: res,
            particle_count: resolved_particle_count(settings, res, &blocks),
            initial_liquid: InitialLiquidConfig { blocks },
        }
    }

    /// Representative inter-particle lattice spacing (world units) for the seeded
    /// layout, i.e. `H * effective_density^(-1/3)`. A uniformly seeded cell of
    /// volume `H³` holds `effective_density` particles, so the lattice spacing is
    /// `(H³ / density)^(1/3)`. The renderer scales the splat radius by this so
    /// lowering density keeps the visible water volume-neutral (just blobbier).
    pub fn seeded_spacing(&self, settings: &Registry) -> f32 {
        let density =
            effective_particle_density(settings, self.grid_resolution, &self.initial_liquid.blocks)
                .max(1.0e-3);
        crate::sim::H * density.powf(-1.0 / 3.0)
    }

    /// The historical default scene (falling blob), independent of the registry's
    /// scene selector. Kept for any caller that wants the canonical default look.
    pub fn default_tank(settings: &Registry) -> Self {
        let res = UVec3::new(
            settings.grid_res_x(),
            settings.grid_res_y(),
            settings.grid_res_z(),
        );
        let blocks = preset_blocks(
            ScenePreset::FallingBlob,
            settings.drop_height(),
            settings.fill_level(),
        );
        Self {
            name: ScenePreset::FallingBlob.name().to_string(),
            preset: ScenePreset::FallingBlob,
            grid_resolution: res,
            particle_count: resolved_particle_count(settings, res, &blocks),
            initial_liquid: InitialLiquidConfig { blocks },
        }
    }
}

/// Fraction of the tank volume occupied by the seeded liquid blocks, in normalized
/// [0,1]^3 tank space. Blocks are treated as disjoint (the shipped presets are), so
/// their normalized volumes simply sum. Multiplying by the total grid-cell count
/// gives the number of grid cells the fluid initially fills ("seeded cells").
fn seeded_volume_fraction(blocks: &[LiquidBlock]) -> f32 {
    blocks
        .iter()
        .map(|b| {
            let ext = b.max - b.min;
            (ext.x.max(0.0) * ext.y.max(0.0) * ext.z.max(0.0)).max(0.0)
        })
        .sum::<f32>()
        .clamp(0.0, 1.0)
}

/// Resolve the spawn particle count.
///
/// "Per cell" means **per seeded fluid cell**, not per total grid cell: the seeded
/// region is the liquid-block volume measured in grid cells
/// (`seeded_volume_fraction * total_grid_cells`), and the density is particles per
/// one of those cells. This keeps the default 64^3 scene near the historical ~250k
/// particles (~8/seeded-cell) and scales correctly with grid resolution and with how
/// much of the tank a scenario fills.
///
/// The advanced `particles.count` override wins when it is nonzero; `0` means Auto.
fn resolved_particle_count(settings: &Registry, res: UVec3, blocks: &[LiquidBlock]) -> u32 {
    let override_count = settings.particle_count_override();
    if override_count > 0 {
        return override_count;
    }
    let total_cells = (res.x as f64) * (res.y as f64) * (res.z as f64);
    let seeded_cells = total_cells * seeded_volume_fraction(blocks) as f64;
    let density = settings.particle_density().max(0.0) as f64;
    let count = (seeded_cells * density).round();
    // Keep a small floor so degenerate scenes still seed something the solver and
    // the GPU dispatch can handle.
    (count as u32).max(1_024)
}

/// The deterministic liquid layout for each preset (normalized [0,1]^3, y up).
///
/// `fill_level` is a literal tank-fill fraction in [0,1] (0 = empty, 1 = full):
///
/// - **FallingBlob (the default / resting scene):** a single full-footprint floor
///   slab `(0,0,0)`–`(1, fill, 1)`. The waterline sits at `fill` of the tank
///   height, so `fill = 0.5` fills the bottom half and `fill = 1.0` fills the
///   whole tank. `seeded_volume_fraction` then equals `fill`. This is a resting
///   body of water, so `drop_height` does not apply to it.
/// - **DamBreak:** a slab pinned to the -X wall over its historical x/z footprint,
///   with its top at `fill` of the tank height (`max.y = fill`). Released, it
///   collapses and races across the floor. More fill = a taller, heavier column.
///   `drop_height` does not apply (floor-anchored).
/// - **DoubleSplash:** two suspended drops near opposite walls that fall and
///   collide. `fill` scales each drop's size about its (drop-height-shifted)
///   center, so more fill drops a bigger pair of bodies. `drop_height` positions
///   them vertically.
fn preset_blocks(preset: ScenePreset, drop_height: f32, fill_level: f32) -> Vec<LiquidBlock> {
    let fill = fill_level.clamp(0.0, 1.0);
    match preset {
        // Resting tank: a full-footprint floor slab whose waterline is at `fill`
        // of the tank height. This is the literal "how full is the tank" scene.
        ScenePreset::FallingBlob => {
            let top = fill.clamp(0.0, 1.0);
            if top <= 1.0e-4 {
                // Empty tank: keep a vanishingly thin valid slab so the solver and
                // GPU dispatch still seed something the floor.
                vec![LiquidBlock::new([0.0, 0.0, 0.0], [1.0, 1.0e-3, 1.0])]
            } else {
                vec![LiquidBlock::new([0.0, 0.0, 0.0], [1.0, top, 1.0])]
            }
        }
        // Wall slab pinned to -X over its footprint; height = fill * tank height.
        ScenePreset::DamBreak => {
            const CEILING: f32 = 0.98;
            let top = (fill * CEILING).clamp(1.0e-3, CEILING);
            vec![LiquidBlock::new([0.05, 0.0, 0.05], [0.42, top, 0.95])]
        }
        // Two suspended drops near opposite walls — fall and collide. `fill`
        // scales each drop's size about its (drop-height-shifted) center.
        ScenePreset::DoubleSplash => {
            let delta = drop_height.clamp(0.0, 1.0) - DEFAULT_DROP_HEIGHT;
            vec![
                LiquidBlock::new([0.1, 0.45, 0.3], [0.34, 0.92, 0.7]),
                LiquidBlock::new([0.66, 0.45, 0.3], [0.9, 0.92, 0.7]),
            ]
            .into_iter()
            .map(|block| shift_block_y(block, delta))
            .map(|block| scale_suspended_drop(block, fill))
            .collect()
        }
    }
}

fn shift_block_y(mut block: LiquidBlock, delta: f32) -> LiquidBlock {
    let shift = delta.clamp(-block.min.y, 1.0 - block.max.y);
    block.min.y += shift;
    block.max.y += shift;
    block
}

/// Scale a suspended drop (DoubleSplash) by the tank-fill fraction.
///
/// The block's vertical extent is scaled about its (drop-height-shifted) center by
/// `fill / DEFAULT_FILL_LEVEL`, so the default fill reproduces the historical drop
/// size and larger fills drop a bigger body. Clamped inside the tank, preserving a
/// valid (non-empty) block.
fn scale_suspended_drop(mut block: LiquidBlock, fill: f32) -> LiquidBlock {
    let center = 0.5 * (block.min.y + block.max.y);
    let half = 0.5 * (block.max.y - block.min.y);
    let scaled = half * (fill / DEFAULT_FILL_LEVEL);
    let mut lo = (center - scaled).clamp(0.0, 1.0);
    let hi = (center + scaled).clamp(lo + 1.0e-3, 1.0);
    lo = lo.min(hi - 1.0e-3).max(0.0);
    block.min.y = lo;
    block.max.y = hi;
    block
}

/// The effective particles-per-seeded-cell density that the *resolved* spawn count
/// implies for this scene. With Auto count this is just `particles.density`; when
/// the advanced `particles.count` override is set it is the override's implied
/// density (`count / seeded_cells`). The renderer uses this so the splat radius
/// follows the real seeded spacing even under an override, keeping the visible
/// water volume-neutral. Returns [`REFERENCE_DENSITY`] for degenerate (empty)
/// scenes so the radius falls back to today's value.
pub fn effective_particle_density(settings: &Registry, res: UVec3, blocks: &[LiquidBlock]) -> f32 {
    let total_cells = (res.x as f64) * (res.y as f64) * (res.z as f64);
    let seeded_cells = total_cells * seeded_volume_fraction(blocks) as f64;
    if seeded_cells <= 0.0 {
        return REFERENCE_DENSITY;
    }
    let count = resolved_particle_count(settings, res, blocks) as f64;
    (count / seeded_cells).max(1.0e-3) as f32
}

/// Effective one-ring surface dilation for the classify pass, given the user's
/// `classify.surface_dilation` setting and the scene's effective particle density.
///
/// Returns `max(user_dilation, auto)`, where `auto = 1` when `density` is **below**
/// the reference (8/cell) and `0` at/above it. Lowering density coarsens the seeded
/// lattice, so without the one-ring dilation the physics liquid region pinholes
/// (cells fall below the occupancy threshold); auto-enabling it keeps the seeded
/// body density-invariant, matching the splat-radius scaling on the render side. At
/// the reference density the auto ring is off to preserve the historical tight
/// surface. Pure host-side (no GPU types) so it is unit-testable; the GPU classify
/// pass already implements the dilation (no shader change).
pub fn effective_surface_dilation(user_dilation: u32, density: f32) -> u32 {
    let auto = if density < REFERENCE_DENSITY { 1 } else { 0 };
    user_dilation.max(auto)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scene_for(preset: ScenePreset, drop_height: f32) -> SceneConfig {
        let mut settings = Registry::default();
        settings.set_value_f64("scene.preset", preset as u32 as f64);
        settings.set_value_f64("scene.drop_height", drop_height as f64);
        SceneConfig::from_settings(&settings)
    }

    #[test]
    fn default_fill_level_is_twenty_percent_floor_slab() {
        // The default scene (Falling blob) is a full-footprint floor slab whose
        // waterline sits at the 20% default fill, so it fills the bottom fifth.
        let falling = scene_for(ScenePreset::FallingBlob, DEFAULT_DROP_HEIGHT);
        let block = falling.initial_liquid.blocks[0];
        assert!((block.min - Vec3::new(0.0, 0.0, 0.0)).length() < 1.0e-6);
        assert!((block.max.x - 1.0).abs() < 1.0e-6);
        assert!((block.max.z - 1.0).abs() < 1.0e-6);
        assert!(
            (block.max.y - DEFAULT_FILL_LEVEL).abs() < 1.0e-6,
            "default waterline {} should be {DEFAULT_FILL_LEVEL}",
            block.max.y
        );
    }

    #[test]
    fn falling_blob_ignores_drop_height() {
        // The resting tank slab is floor-anchored, so drop_height does not move it.
        let low = scene_for(ScenePreset::FallingBlob, 0.0).initial_liquid.blocks[0];
        let high = scene_for(ScenePreset::FallingBlob, 1.0).initial_liquid.blocks[0];
        assert!((low.min.y - high.min.y).abs() < 1.0e-6);
        assert!((low.max.y - high.max.y).abs() < 1.0e-6);
        assert!((low.min.y - 0.0).abs() < 1.0e-6);
        assert!((low.max.y - DEFAULT_FILL_LEVEL).abs() < 1.0e-6);
    }

    #[test]
    fn double_splash_drop_height_clamps_inside_tank() {
        let low = scene_for(ScenePreset::DoubleSplash, 0.0)
            .initial_liquid
            .blocks[0];
        assert!(low.min.y >= -1.0e-6 && low.max.y <= 1.0 + 1.0e-6);
        assert!(low.max.y > low.min.y);

        let high = scene_for(ScenePreset::DoubleSplash, 1.0)
            .initial_liquid
            .blocks[0];
        assert!(high.min.y >= -1.0e-6 && high.max.y <= 1.0 + 1.0e-6);
        assert!(high.max.y > high.min.y);
    }

    #[test]
    fn default_density_derives_count_from_seeded_cells() {
        // Default registry: density 8/seeded-cell, 64^3 grid, falling-blob preset.
        // The default scene is a full-footprint floor slab at 20% fill, so the
        // seeded fraction is ~0.2 and count ≈ 8 * 0.2 * 64^3 ≈ 419k.
        let scene = SceneConfig::from_settings(&Registry::default());
        assert!(
            (418_000..=420_000).contains(&scene.particle_count),
            "default count {} should be ~419k (8/seeded-cell, 20% fill)",
            scene.particle_count
        );
    }

    #[test]
    fn density_scales_count_with_grid_resolution() {
        let mut settings = Registry::default();
        settings.set_value_f64("grid.res_x", 128.0);
        settings.set_value_f64("grid.res_z", 128.0);
        // 128x64x128, density 8, falling blob at 20% fill -> 8 * 0.2 * 1_048_576 ≈ 1.677M.
        let blob = SceneConfig::from_settings(&settings).particle_count;
        assert!(
            (1_670_000..=1_685_000).contains(&blob),
            "falling-blob count {blob} should be ~1.68M"
        );
    }

    #[test]
    fn nonzero_particle_count_override_wins_over_density() {
        let mut settings = Registry::default();
        settings.set_value_f64("particles.count", 500_000.0);
        assert_eq!(SceneConfig::from_settings(&settings).particle_count, 500_000);
    }

    #[test]
    fn dam_break_ignores_drop_height() {
        let low = scene_for(ScenePreset::DamBreak, 0.0).initial_liquid.blocks[0];
        let high = scene_for(ScenePreset::DamBreak, 1.0).initial_liquid.blocks[0];

        // Floor-anchored slab; only its footprint and fill-scaled height matter.
        assert!((low.min.y - 0.0).abs() < 1.0e-6);
        assert!((high.min.y - low.min.y).abs() < 1.0e-6);
        assert!((high.max.y - low.max.y).abs() < 1.0e-6);
    }

    #[test]
    fn full_fill_fills_the_tank() {
        // fill_level = 100% -> the default scene is a full-footprint slab spanning
        // the whole tank, so the seeded fraction is ~1.0.
        let mut settings = Registry::default();
        settings.set_value_f64("scene.fill_level", 100.0);
        let scene = SceneConfig::from_settings(&settings);
        let frac = seeded_volume_fraction(&scene.initial_liquid.blocks);
        assert!(frac > 0.99, "full fill seeded fraction {frac} should be ~1.0");
    }
}

/// Tank-fill (`scene.fill_level`) + volume-neutral density decoupling.
///
/// These assert the pure host derivations that back the visual feature:
/// (a) `fill_level` is a literal tank-fill fraction — the default scene's seeded
/// fraction equals it, and it is monotone in the resolved count for every preset;
/// (b) at a fixed fill the count scales linearly with density while the *seeded
/// fraction* (the body of water) is density-invariant.
#[cfg(test)]
mod fill_level_tests {
    use super::*;

    /// `fill` is a [0,1] tank-fill fraction; the registry stores it as a 0–100
    /// percentage, so multiply by 100 here.
    fn registry(preset: ScenePreset, fill: f32, density: f32) -> Registry {
        let mut s = Registry::default();
        s.set_value_f64("scene.preset", preset as u32 as f64);
        s.set_value_f64("scene.fill_level", (fill * 100.0) as f64);
        s.set_value_f64("particles.density", density as f64);
        s
    }

    fn scene(preset: ScenePreset, fill: f32, density: f32) -> SceneConfig {
        SceneConfig::from_settings(&registry(preset, fill, density))
    }

    const PRESETS: [ScenePreset; 3] = [
        ScenePreset::FallingBlob,
        ScenePreset::DamBreak,
        ScenePreset::DoubleSplash,
    ];

    #[test]
    fn default_scene_seeded_fraction_equals_fill_fraction() {
        // The default scene (Falling blob) is a full-footprint floor slab, so its
        // seeded fraction equals the tank-fill fraction. 20% -> ~0.2, 100% -> ~1.0.
        for &fill in &[0.1f32, 0.2, 0.5, 1.0] {
            let blocks = preset_blocks(ScenePreset::FallingBlob, DEFAULT_DROP_HEIGHT, fill);
            let frac = seeded_volume_fraction(&blocks);
            assert!(
                (frac - fill).abs() < 1.0e-4,
                "fill {fill}: seeded fraction {frac} should equal the fill fraction"
            );
        }
    }

    #[test]
    fn fill_level_is_monotone_in_seeded_fraction_and_count() {
        // Higher fill => strictly more seeded body => more particles, for every
        // preset, at fixed density and grid.
        for preset in PRESETS {
            let res = UVec3::new(64, 64, 64);
            let mut last_frac = -1.0f32;
            let mut last_count = 0u32;
            for &fill in &[0.1f32, 0.2, 0.5, 1.0] {
                let blocks = preset_blocks(preset, DEFAULT_DROP_HEIGHT, fill);
                let frac = seeded_volume_fraction(&blocks);
                let count = resolved_particle_count(&registry(preset, fill, 8.0), res, &blocks);
                assert!(
                    frac > last_frac + 1.0e-6,
                    "{:?}: seeded fraction not increasing at fill {fill}: {frac} <= {last_frac}",
                    preset.name()
                );
                assert!(
                    count >= last_count,
                    "{:?}: count not increasing at fill {fill}: {count} < {last_count}",
                    preset.name()
                );
                last_frac = frac;
                last_count = count;
            }
        }
    }

    #[test]
    fn default_scene_fill_is_roughly_linear() {
        // For the default full-footprint floor slab the seeded fraction tracks the
        // fill fraction linearly, so 50% seeds ~2.5x what 20% does.
        let f20 = seeded_volume_fraction(&preset_blocks(
            ScenePreset::FallingBlob,
            DEFAULT_DROP_HEIGHT,
            0.2,
        ));
        let f50 = seeded_volume_fraction(&preset_blocks(
            ScenePreset::FallingBlob,
            DEFAULT_DROP_HEIGHT,
            0.5,
        ));
        let ratio = f50 / f20;
        assert!(
            (2.4..=2.6).contains(&ratio),
            "50%/20% seeded ratio {ratio} should be ~2.5"
        );
    }

    #[test]
    fn count_scales_with_density_at_fixed_fill_level() {
        // At a fixed fill the seeded body (fraction) is density-INVARIANT, and the
        // resolved count scales ~linearly with density.
        for preset in PRESETS {
            let res = UVec3::new(64, 64, 64);
            let blocks_8 = preset_blocks(preset, DEFAULT_DROP_HEIGHT, 0.5);
            let blocks_2 = preset_blocks(preset, DEFAULT_DROP_HEIGHT, 0.5);
            // Geometry (hence seeded fraction) does not depend on density.
            assert!(
                (seeded_volume_fraction(&blocks_8) - seeded_volume_fraction(&blocks_2)).abs()
                    < 1.0e-9,
                "{:?}: seeded fraction must be density-invariant",
                preset.name()
            );

            let c8 = resolved_particle_count(&registry(preset, 0.5, 8.0), res, &blocks_8);
            let c2 = resolved_particle_count(&registry(preset, 0.5, 2.0), res, &blocks_2);
            // density 8 vs 2 -> ~4x more particles (above the 1024 floor for these
            // presets at 64^3). Allow a small tolerance for rounding/floor.
            let ratio = c8 as f64 / c2 as f64;
            assert!(
                (3.6..=4.4).contains(&ratio),
                "{:?}: count ratio {ratio} (c8={c8}, c2={c2}) should be ~4x",
                preset.name()
            );
        }
    }

    #[test]
    fn seeded_spacing_grows_as_density_drops() {
        // Volume-neutral density lever: lower density => coarser lattice => larger
        // spacing (hence larger splat radius). At the reference density the spacing
        // is H * 8^(-1/3) = H * 0.5, which the renderer maps to today's H*0.35.
        let res = UVec3::new(64, 64, 64);
        let dense = scene(ScenePreset::DamBreak, 0.5, 8.0);
        let sparse = scene(ScenePreset::DamBreak, 0.5, 2.0);
        let reg8 = registry(ScenePreset::DamBreak, 0.5, 8.0);
        let reg2 = registry(ScenePreset::DamBreak, 0.5, 2.0);
        let s8 = dense.seeded_spacing(&reg8);
        let s2 = sparse.seeded_spacing(&reg2);
        assert!(s2 > s8, "spacing should grow at lower density: {s2} <= {s8}");

        // density 8 -> spacing ~ H * 0.5 (within rounding of the resolved count).
        let expected_8 = crate::sim::H * 0.5;
        assert!(
            (s8 / expected_8 - 1.0).abs() < 0.05,
            "density-8 spacing {s8} should be ~{expected_8} (H*0.5)"
        );
        let _ = res;
    }

    #[test]
    fn effective_density_is_density_invariant_geometry() {
        // effective_particle_density must equal the slider density when Auto count
        // is used (it is just count/seeded_cells with count = density*seeded_cells).
        let res = UVec3::new(64, 64, 64);
        for &d in &[1.0f32, 2.0, 4.0, 8.0, 16.0] {
            let blocks = preset_blocks(ScenePreset::DamBreak, DEFAULT_DROP_HEIGHT, 0.5);
            let eff = effective_particle_density(&registry(ScenePreset::DamBreak, 0.5, d), res, &blocks);
            assert!(
                (eff - d).abs() / d < 0.02,
                "effective density {eff} should track slider {d}"
            );
        }
    }

    #[test]
    fn auto_surface_dilation_threshold() {
        // Below reference density -> auto ring on; at/above -> off; user forces on.
        assert_eq!(effective_surface_dilation(0, 1.0), 1);
        assert_eq!(effective_surface_dilation(0, 4.0), 1);
        assert_eq!(effective_surface_dilation(0, 7.99), 1);
        assert_eq!(effective_surface_dilation(0, 8.0), 0);
        assert_eq!(effective_surface_dilation(0, 16.0), 0);
        assert_eq!(effective_surface_dilation(1, 8.0), 1);
        assert_eq!(effective_surface_dilation(1, 16.0), 1);
    }

    #[test]
    fn fill_level_clamps_keep_blocks_valid() {
        for preset in PRESETS {
            for &fill in &[0.0f32, 0.1, 1.0] {
                for b in preset_blocks(preset, DEFAULT_DROP_HEIGHT, fill) {
                    assert!(b.max.y > b.min.y, "{:?}: empty block at fill {fill}", preset.name());
                    assert!(b.min.y >= -1.0e-6 && b.max.y <= 1.0 + 1.0e-6);
                }
            }
        }
    }
}
