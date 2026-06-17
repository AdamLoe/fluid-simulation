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

/// Default `scene.fill_level` as a [0,1] scenario amount fraction.
/// The registry stores this as a 0-100 percentage; `Registry::fill_level()`
/// converts to this fraction. Presets map the fraction into their authored
/// geometry differently, but the resulting seeded block volume is monotone in
/// this value. See `preset_blocks` for the per-scenario mapping.
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
        self.seeded_spacing_for_density(density)
    }

    pub fn seeded_spacing_for_particle_count(&self, particle_count: u32) -> f32 {
        let density = effective_particle_density_for_count(
            self.grid_resolution,
            &self.initial_liquid.blocks,
            particle_count,
        )
        .max(1.0e-3);
        self.seeded_spacing_for_density(density)
    }

    fn seeded_spacing_for_density(&self, density: f32) -> f32 {
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

fn seeded_cell_count(res: UVec3, blocks: &[LiquidBlock]) -> f64 {
    let total_cells = (res.x as f64) * (res.y as f64) * (res.z as f64);
    total_cells * seeded_volume_fraction(blocks) as f64
}

/// Resolve the spawn particle count.
///
/// "Per cell" means **per seeded fluid cell**, not per total grid cell: the seeded
/// region is the liquid-block volume measured in grid cells
/// (`seeded_volume_fraction * total_grid_cells`), and the density is particles per
/// one of those cells. This keeps the default scene near the historical particle
/// budget (~8/seeded-cell) and scales correctly with grid resolution and with how
/// much of the tank a scenario fills.
///
/// The advanced `particles.count` override wins when it is nonzero; `0` means Auto.
fn resolved_particle_count(settings: &Registry, res: UVec3, blocks: &[LiquidBlock]) -> u32 {
    let override_count = settings.particle_count_override();
    if override_count > 0 {
        return override_count;
    }
    let seeded_cells = seeded_cell_count(res, blocks);
    if seeded_cells <= 0.0 {
        return 0;
    }
    let density = settings.particle_density().max(0.0) as f64;
    let count = (seeded_cells * density).round();
    // Keep a small floor so degenerate scenes still seed something the solver and
    // the GPU dispatch can handle.
    (count as u32).max(1_024)
}

/// The deterministic liquid layout for each preset (normalized [0,1]^3, y up).
///
/// `fill_level` is a scenario amount fraction in [0,1]. Presets keep their
/// authored scale so the visual composition stays close to the tuned scene:
///
/// - **FallingBlob (the default):** a suspended central blob. `drop_height`
///   positions the blob vertically; `fill_level` scales the blob around the
///   default 20% amount.
/// - **DamBreak:** a floor-anchored wall slab over its historical X/Z footprint.
///   `fill_level` controls height inside that footprint; `drop_height` does not
///   apply.
/// - **DoubleSplash:** two suspended drops near opposite walls that fall and
///   collide. `fill_level` scales each authored drop vertically around the
///   default 20% amount.
fn preset_blocks(preset: ScenePreset, drop_height: f32, fill_level: f32) -> Vec<LiquidBlock> {
    let fill = fill_level.clamp(0.0, 1.0);
    match preset {
        ScenePreset::FallingBlob => {
            let delta = drop_height.clamp(0.0, 1.0) - DEFAULT_DROP_HEIGHT;
            let block = LiquidBlock::new([0.19, 0.2, 0.19], [0.81, 0.72, 0.81]);
            vec![scale_block_about_center(shift_block_y(block, delta), fill)]
        }
        ScenePreset::DamBreak => {
            const CEILING: f32 = 0.98;
            let top = (fill * CEILING).clamp(1.0e-3, CEILING);
            vec![LiquidBlock::new([0.05, 0.0, 0.05], [0.42, top, 0.95])]
        }
        ScenePreset::DoubleSplash => vec![
            LiquidBlock::new([0.1, 0.45, 0.3], [0.34, 0.92, 0.7]),
            LiquidBlock::new([0.66, 0.45, 0.3], [0.9, 0.92, 0.7]),
        ]
        .into_iter()
        .map(|block| shift_block_y(block, drop_height.clamp(0.0, 1.0) - DEFAULT_DROP_HEIGHT))
        .map(|block| scale_suspended_drop(block, fill))
        .collect(),
    }
}

fn scale_block_about_center(block: LiquidBlock, fill: f32) -> LiquidBlock {
    let factor = (fill.max(1.0e-4) / DEFAULT_FILL_LEVEL).powf(1.0 / 3.0);
    scale_block_axes(block, Vec3::splat(factor))
}

fn shift_block_y(mut block: LiquidBlock, delta: f32) -> LiquidBlock {
    let shift = delta.clamp(-block.min.y, 1.0 - block.max.y);
    block.min.y += shift;
    block.max.y += shift;
    block
}

fn scale_suspended_drop(block: LiquidBlock, fill: f32) -> LiquidBlock {
    scale_block_axes(block, Vec3::new(1.0, fill / DEFAULT_FILL_LEVEL, 1.0))
}

fn scale_block_axes(mut block: LiquidBlock, factor: Vec3) -> LiquidBlock {
    let center = 0.5 * (block.min + block.max);
    let half = 0.5 * (block.max - block.min);
    let scaled = half * factor.max(Vec3::splat(1.0e-3));
    block.min = (center - scaled).clamp(Vec3::ZERO, Vec3::ONE);
    block.max = (center + scaled).clamp(block.min + Vec3::splat(1.0e-3), Vec3::ONE);
    block.min = block
        .min
        .min(block.max - Vec3::splat(1.0e-3))
        .max(Vec3::ZERO);
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
    effective_particle_density_for_count(
        res,
        blocks,
        resolved_particle_count(settings, res, blocks),
    )
}

pub fn effective_particle_density_for_count(
    res: UVec3,
    blocks: &[LiquidBlock],
    particle_count: u32,
) -> f32 {
    let seeded_cells = seeded_cell_count(res, blocks);
    if seeded_cells <= 0.0 {
        return REFERENCE_DENSITY;
    }
    (particle_count as f64 / seeded_cells).max(1.0e-3) as f32
}

/// Effective anti-clump rest target (particles per liquid cell) the divergence
/// pass should aim for, given the user's manual `physics.rest_density` setting and
/// the scene's effective particle density.
///
/// `manual_rest > 0` pins a fixed target (the historical behavior). `manual_rest <= 0`
/// is Auto: the target tracks the actual seeded particles-per-cell so the anti-clump
/// source sees `occ/rest ~= 1` at every density. Because `occ` is the raw per-cell
/// particle count (which scales linearly with density), a frozen target made the
/// water look like a different *volume* in motion across densities; coupling the
/// target to the density makes density motion-neutral. Pure host-side (no GPU types)
/// so it is unit-testable.
pub fn effective_rest_density(manual_rest: f32, density: f32) -> f32 {
    if manual_rest > 0.0 {
        manual_rest
    } else {
        density.max(1.0e-3)
    }
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
#[derive(Debug)]
struct BlockSeedMeasurement {
    normalized_volume: f32,
    requested_particles: u32,
    generated_particles: u32,
    lattice_counts: UVec3,
    lattice_spacing: f32,
    top_margin_fraction: f32,
    empty_cell_layers_above: f32,
}

#[cfg(test)]
#[derive(Debug)]
struct SceneSeedMeasurement {
    seeded_fraction: f32,
    requested_particles: u32,
    generated_particles: u32,
    requested_effective_density: f32,
    actual_effective_density: f32,
    seeded_cells: f32,
    top_margin_fraction: f32,
    empty_cell_layers_above: f32,
    non_seeded_interior_fraction: f32,
    uniform_density_clamp_proxy: f32,
    blocks: Vec<BlockSeedMeasurement>,
}

#[cfg(test)]
fn measure_current_seed(settings: &Registry) -> SceneSeedMeasurement {
    let scene = SceneConfig::from_settings(settings);
    measure_current_scene_seed(settings, &scene)
}

#[cfg(test)]
fn measure_current_scene_seed(settings: &Registry, scene: &SceneConfig) -> SceneSeedMeasurement {
    let total_cells = (scene.grid_resolution.x as f32)
        * (scene.grid_resolution.y as f32)
        * (scene.grid_resolution.z as f32);
    let seeded_fraction = seeded_volume_fraction(&scene.initial_liquid.blocks);
    let seeded_cells = total_cells * seeded_fraction;
    let total_volume = scene
        .initial_liquid
        .blocks
        .iter()
        .map(|block| block_measure_volume(block, scene.grid_resolution))
        .sum::<f32>()
        .max(1.0e-6);
    let requested_particles = scene.particle_count;
    let requested_effective_density = effective_particle_density(
        settings,
        scene.grid_resolution,
        &scene.initial_liquid.blocks,
    );

    let mut generated_particles = 0u32;
    let mut top_margin_fraction = 1.0f32;
    let mut blocks = Vec::with_capacity(scene.initial_liquid.blocks.len());
    for block in &scene.initial_liquid.blocks {
        let volume = block_measure_volume(block, scene.grid_resolution).max(1.0e-6);
        let target = (requested_particles.max(1) as f32 * (volume / total_volume)).max(1.0);
        let spacing = (volume / target).cbrt().max(1.0e-4);
        let ext = block_measure_extent(block, scene.grid_resolution);
        let lattice_counts = UVec3::new(
            ((ext.x / spacing).floor() as u32).max(1),
            ((ext.y / spacing).floor() as u32).max(1),
            ((ext.z / spacing).floor() as u32).max(1),
        );
        let count = lattice_counts
            .x
            .saturating_mul(lattice_counts.y)
            .saturating_mul(lattice_counts.z);
        generated_particles = generated_particles.saturating_add(count);
        let block_top_margin = (1.0 - block.max.y).max(0.0);
        top_margin_fraction = top_margin_fraction.min(block_top_margin);
        blocks.push(BlockSeedMeasurement {
            normalized_volume: block_normalized_volume(block),
            requested_particles: target.round() as u32,
            generated_particles: count,
            lattice_counts,
            lattice_spacing: spacing,
            top_margin_fraction: block_top_margin,
            empty_cell_layers_above: block_top_margin * scene.grid_resolution.y as f32,
        });
    }

    let actual_effective_density = if seeded_cells > 0.0 {
        generated_particles as f32 / seeded_cells
    } else {
        REFERENCE_DENSITY
    };
    let rest_density = effective_rest_density(settings.rest_density(), requested_effective_density);
    let unclamped = if rest_density > 0.0 {
        settings.volume_stiffness()
            * ((actual_effective_density - rest_density).max(0.0) / rest_density)
    } else {
        0.0
    };

    SceneSeedMeasurement {
        seeded_fraction,
        requested_particles,
        generated_particles,
        requested_effective_density,
        actual_effective_density,
        seeded_cells,
        top_margin_fraction,
        empty_cell_layers_above: top_margin_fraction * scene.grid_resolution.y as f32,
        non_seeded_interior_fraction: (1.0 - seeded_fraction).clamp(0.0, 1.0),
        uniform_density_clamp_proxy: unclamped.min(settings.drift_clamp()),
        blocks,
    }
}

#[cfg(test)]
fn block_normalized_volume(block: &LiquidBlock) -> f32 {
    let ext = block.max - block.min;
    (ext.x.max(0.0) * ext.y.max(0.0) * ext.z.max(0.0)).max(0.0)
}

#[cfg(test)]
fn block_measure_extent(block: &LiquidBlock, res: UVec3) -> Vec3 {
    let grid_extent = Vec3::new(res.x as f32, res.y as f32, res.z as f32) * crate::sim::H;
    (block.max - block.min) * grid_extent
}

#[cfg(test)]
fn block_measure_volume(block: &LiquidBlock, res: UVec3) -> f32 {
    let ext = block_measure_extent(block, res);
    ext.x.max(0.0) * ext.y.max(0.0) * ext.z.max(0.0)
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
    fn default_fill_level_is_falling_blob() {
        let falling = scene_for(ScenePreset::FallingBlob, DEFAULT_DROP_HEIGHT);
        let block = falling.initial_liquid.blocks[0];
        assert!(block.min.y > 0.0, "falling blob should be suspended");
        assert!(
            block.max.y < 1.0,
            "default-height helper blob should not be ceiling-clamped"
        );
        assert!(
            (seeded_volume_fraction(&falling.initial_liquid.blocks) - DEFAULT_FILL_LEVEL).abs()
                < 0.01,
            "default blob volume should track the default fill fraction"
        );
    }

    #[test]
    fn falling_blob_uses_drop_height() {
        let low = scene_for(ScenePreset::FallingBlob, 0.0)
            .initial_liquid
            .blocks[0];
        let high = scene_for(ScenePreset::FallingBlob, 1.0)
            .initial_liquid
            .blocks[0];
        assert!((low.min.y - 0.0).abs() < 1.0e-6);
        assert!(high.min.y > low.min.y + 0.25);
        assert!((high.max.y - 1.0).abs() < 1.0e-6);
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
        // Default registry: density 8/seeded-cell, 80x40x80 grid, falling-blob preset.
        // The default blob's seeded fraction is ~0.2, so count is near 410k.
        let scene = SceneConfig::from_settings(&Registry::default());
        assert!(
            (409_000..=410_000).contains(&scene.particle_count),
            "default count {} should be ~410k (8/seeded-cell, 20% fill)",
            scene.particle_count
        );
    }

    #[test]
    fn density_scales_count_with_grid_resolution() {
        let mut settings = Registry::default();
        settings.set_value_f64("grid.res_x", 128.0);
        settings.set_value_f64("grid.res_z", 128.0);
        // 128x40x128, density 8, falling blob at ~20% seeded volume -> ~1.049M.
        let blob = SceneConfig::from_settings(&settings).particle_count;
        assert!(
            (1_040_000..=1_055_000).contains(&blob),
            "falling-blob count {blob} should be ~1.05M"
        );
    }

    #[test]
    fn nonzero_particle_count_override_wins_over_density() {
        let mut settings = Registry::default();
        settings.set_value_f64("particles.count", 500_000.0);
        assert_eq!(
            SceneConfig::from_settings(&settings).particle_count,
            500_000
        );
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
    fn full_fill_uses_preset_authored_max_not_whole_tank() {
        let mut settings = Registry::default();
        settings.set_value_f64("scene.fill_level", 100.0);
        let scene = SceneConfig::from_settings(&settings);
        let frac = seeded_volume_fraction(&scene.initial_liquid.blocks);
        assert!(
            (0.55..=0.75).contains(&frac),
            "full fill seeded fraction {frac} should stay on the tuned falling-blob scale"
        );
    }
}

/// Scenario amount (`scene.fill_level`) + volume-neutral density decoupling.
///
/// These assert the pure host derivations that back the visual feature:
/// (a) `fill_level` is monotone in seeded fraction and resolved count;
/// (b) at a fixed fill the count scales linearly with density while the *seeded
/// fraction* (the body of water) is density-invariant.
#[cfg(test)]
mod fill_level_tests {
    use super::*;

    /// `fill` is a [0,1] scenario amount fraction; the registry stores it as a 0-100
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
    fn preset_seeded_fraction_keeps_authored_scale() {
        let cases = [
            (ScenePreset::FallingBlob, 0.2, 0.199888),
            (ScenePreset::FallingBlob, 0.5, 0.499720),
            (ScenePreset::DamBreak, 0.5, 0.163170),
            (ScenePreset::DoubleSplash, 0.5, 0.173280),
        ];
        for (preset, fill, expected) in cases {
            let blocks = preset_blocks(preset, DEFAULT_DROP_HEIGHT, fill);
            let frac = seeded_volume_fraction(&blocks);
            assert!(
                (frac - expected).abs() < 0.005,
                "{} fill {fill}: seeded fraction {frac} should stay near authored scale {expected}",
                preset.name()
            );
        }
    }

    #[test]
    fn zero_fill_keeps_minimum_viable_seed() {
        for preset in PRESETS {
            let blocks = preset_blocks(preset, DEFAULT_DROP_HEIGHT, 0.0);
            assert!(
                !blocks.is_empty(),
                "{} should keep a tiny compatibility seed at 0% fill",
                preset.name()
            );
            assert!(seeded_volume_fraction(&blocks) > 0.0);
            let scene = scene(preset, 0.0, 8.0);
            assert_eq!(scene.particle_count, 1_024);
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
    fn default_scene_fill_is_roughly_linear_before_clamping() {
        let f20 = seeded_volume_fraction(&preset_blocks(
            ScenePreset::FallingBlob,
            DEFAULT_DROP_HEIGHT,
            0.2,
        ));
        assert!((f20 - 0.2).abs() < 0.001);
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
        assert!(
            s2 > s8,
            "spacing should grow at lower density: {s2} <= {s8}"
        );

        // density 8 -> spacing ~ H * 0.5 (within rounding of the resolved count).
        let expected_8 = crate::sim::H * 0.5;
        assert!(
            (s8 / expected_8 - 1.0).abs() < 0.05,
            "density-8 spacing {s8} should be ~{expected_8} (H*0.5)"
        );
        let _ = res;
    }

    #[test]
    fn requested_effective_density_tracks_slider_density() {
        // effective_particle_density must equal the slider density when Auto count
        // is used (it is just count/seeded_cells with count = density*seeded_cells).
        let res = UVec3::new(64, 64, 64);
        for &d in &[1.0f32, 2.0, 4.0, 8.0, 16.0] {
            let blocks = preset_blocks(ScenePreset::DamBreak, DEFAULT_DROP_HEIGHT, 0.5);
            let eff =
                effective_particle_density(&registry(ScenePreset::DamBreak, 0.5, d), res, &blocks);
            assert!(
                (eff - d).abs() / d < 0.02,
                "effective density {eff} should track slider {d}"
            );
        }
    }

    #[test]
    fn actual_effective_density_uses_generated_count() {
        let res = UVec3::new(80, 40, 80);
        let settings = registry(ScenePreset::FallingBlob, 0.5, 8.0);
        let scene = SceneConfig::from_settings(&settings);
        let requested = effective_particle_density(&settings, res, &scene.initial_liquid.blocks);
        let generated = measure_current_scene_seed(&settings, &scene).generated_particles;
        let actual =
            effective_particle_density_for_count(res, &scene.initial_liquid.blocks, generated);

        assert!((requested - 8.0).abs() < 0.001);
        assert!(actual < requested);
        assert!(
            (actual - 7.920).abs() < 0.02,
            "actual density {actual} should reflect lattice flooring"
        );
    }

    #[test]
    fn auto_rest_density_tracks_particle_density() {
        // Auto (manual_rest = 0): the anti-clump target equals the scene's effective
        // particles-per-cell at every density, so occ/rest ~= 1 and density is
        // motion-neutral. A nonzero manual value overrides.
        let res = UVec3::new(64, 64, 64);
        for &d in &[1.0f32, 8.0, 32.0] {
            let settings = registry(ScenePreset::DamBreak, 0.5, d);
            let blocks = preset_blocks(ScenePreset::DamBreak, DEFAULT_DROP_HEIGHT, 0.5);
            let eff_density = effective_particle_density(&settings, res, &blocks);
            // Auto: rest target follows the actual density.
            let auto = effective_rest_density(settings.rest_density(), eff_density);
            assert!(
                (auto - d).abs() / d < 0.02,
                "auto rest target {auto} should track density {d}"
            );
            // Manual override wins regardless of density.
            assert_eq!(effective_rest_density(12.5, eff_density), 12.5);
        }
        // Degenerate density floors at a tiny positive value (never 0, which would
        // disable the anti-clump source in the shader).
        assert!(effective_rest_density(0.0, 0.0) > 0.0);
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
                    assert!(
                        b.max.y > b.min.y,
                        "{:?}: empty block at fill {fill}",
                        preset.name()
                    );
                    assert!(b.min.y >= -1.0e-6 && b.max.y <= 1.0 + 1.0e-6);
                }
            }
        }
    }
}

#[cfg(test)]
mod measurement_tests {
    use super::*;

    fn registry(
        preset: ScenePreset,
        res: UVec3,
        fill: f32,
        density: f32,
        override_count: u32,
    ) -> Registry {
        let mut settings = Registry::default();
        settings.set_value_f64("scene.preset", preset as u32 as f64);
        settings.set_value_f64("grid.res_x", res.x as f64);
        settings.set_value_f64("grid.res_y", res.y as f64);
        settings.set_value_f64("grid.res_z", res.z as f64);
        settings.set_value_f64("scene.fill_level", (fill * 100.0) as f64);
        settings.set_value_f64("particles.density", density as f64);
        settings.set_value_f64("particles.count", override_count as f64);
        settings
    }

    fn measured(preset: ScenePreset, res: UVec3, fill: f32, density: f32) -> SceneSeedMeasurement {
        measure_current_seed(&registry(preset, res, fill, density, 0))
    }

    #[test]
    fn current_formula_measurement_matrix_is_deterministic() {
        let cases = [
            (ScenePreset::FallingBlob, UVec3::new(32, 32, 32), 0.2, 8.0),
            (ScenePreset::FallingBlob, UVec3::new(64, 64, 64), 0.5, 8.0),
            (ScenePreset::FallingBlob, UVec3::new(80, 40, 80), 0.5, 1.0),
            (ScenePreset::FallingBlob, UVec3::new(80, 40, 80), 0.5, 8.0),
            (ScenePreset::FallingBlob, UVec3::new(80, 40, 80), 0.5, 32.0),
            (ScenePreset::FallingBlob, UVec3::new(80, 40, 80), 0.8, 8.0),
            (ScenePreset::DamBreak, UVec3::new(80, 40, 80), 0.5, 8.0),
            (ScenePreset::DamBreak, UVec3::new(80, 40, 80), 1.0, 8.0),
            (ScenePreset::DoubleSplash, UVec3::new(80, 40, 80), 0.5, 8.0),
            (ScenePreset::FallingBlob, UVec3::new(96, 24, 96), 0.8, 8.0),
        ];

        for (preset, res, fill, density) in cases {
            let m = measured(preset, res, fill, density);
            println!(
                "{},{:?},{fill:.2},{density:.1},seeded={:.6},req={},gen={},req_eff={:.4},act_eff={:.4},top_margin={:.4},empty_layers={:.2},free_frac={:.4},clamp_proxy={:.4}",
                preset.name(),
                res,
                m.seeded_fraction,
                m.requested_particles,
                m.generated_particles,
                m.requested_effective_density,
                m.actual_effective_density,
                m.top_margin_fraction,
                m.empty_cell_layers_above,
                m.non_seeded_interior_fraction,
                m.uniform_density_clamp_proxy,
            );
            for (i, block) in m.blocks.iter().enumerate() {
                println!(
                    "  block {i}: vol={:.6}, req={}, gen={}, counts={}x{}x{}, spacing={:.6}, top_margin={:.4}, empty_layers={:.2}",
                    block.normalized_volume,
                    block.requested_particles,
                    block.generated_particles,
                    block.lattice_counts.x,
                    block.lattice_counts.y,
                    block.lattice_counts.z,
                    block.lattice_spacing,
                    block.top_margin_fraction,
                    block.empty_cell_layers_above,
                );
            }

            assert!(m.seeded_fraction > 0.0);
            assert!(m.requested_particles >= 1_024);
            assert!(m.generated_particles > 0);
            assert!(m.generated_particles <= m.requested_particles);
            assert!(m.actual_effective_density <= m.requested_effective_density);
            assert_eq!(m.uniform_density_clamp_proxy, 0.0);
        }
    }

    #[test]
    fn density_changes_count_not_seed_geometry() {
        let res = UVec3::new(80, 40, 80);
        let low = measured(ScenePreset::FallingBlob, res, 0.5, 1.0);
        let reference = measured(ScenePreset::FallingBlob, res, 0.5, 8.0);
        let high = measured(ScenePreset::FallingBlob, res, 0.5, 32.0);

        assert_eq!(low.seeded_fraction, reference.seeded_fraction);
        assert_eq!(high.seeded_fraction, reference.seeded_fraction);
        assert_eq!(low.seeded_cells, reference.seeded_cells);
        assert_eq!(high.seeded_cells, reference.seeded_cells);
        assert!(low.generated_particles < reference.generated_particles);
        assert!(reference.generated_particles < high.generated_particles);
        assert!(
            (low.actual_effective_density - 1.0).abs() / 1.0 < 0.10,
            "low-density actual effective density drifted: {}",
            low.actual_effective_density
        );
        assert!(
            (reference.actual_effective_density - 8.0).abs() / 8.0 < 0.05,
            "reference-density actual effective density drifted: {}",
            reference.actual_effective_density
        );
        assert!(
            (high.actual_effective_density - 32.0).abs() / 32.0 < 0.05,
            "high-density actual effective density drifted: {}",
            high.actual_effective_density
        );
    }

    #[test]
    fn behavior_formula_supports_preset_authored_amount() {
        let res = UVec3::new(80, 40, 80);
        let falling = measured(ScenePreset::FallingBlob, res, 0.5, 8.0);
        let dam = measured(ScenePreset::DamBreak, res, 0.5, 8.0);
        let double = measured(ScenePreset::DoubleSplash, res, 0.5, 8.0);

        assert!(
            (falling.seeded_fraction - 0.434).abs() < 0.005,
            "falling blob should stay near its tuned visual scale"
        );
        assert!(
            (dam.seeded_fraction - 0.163).abs() < 0.005,
            "dam break should use its authored footprint-relative scale"
        );
        assert!(
            (double.seeded_fraction - 0.158).abs() < 0.005,
            "double splash should use its authored suspended-drop scale"
        );
    }
}
