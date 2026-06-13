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
        let blocks = preset_blocks(preset, settings.drop_height());
        Self {
            name: preset.name().to_string(),
            preset,
            grid_resolution: res,
            particle_count: resolved_particle_count(settings, res, &blocks),
            initial_liquid: InitialLiquidConfig { blocks },
        }
    }

    /// The historical default scene (falling blob), independent of the registry's
    /// scene selector. Kept for any caller that wants the canonical default look.
    pub fn default_tank(settings: &Registry) -> Self {
        let res = UVec3::new(
            settings.grid_res_x(),
            settings.grid_res_y(),
            settings.grid_res_z(),
        );
        let blocks = preset_blocks(ScenePreset::FallingBlob, settings.drop_height());
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
fn preset_blocks(preset: ScenePreset, drop_height: f32) -> Vec<LiquidBlock> {
    let blocks = match preset {
        // Centered blob in the upper-middle — falls straight down and splashes.
        ScenePreset::FallingBlob => vec![LiquidBlock::new([0.2, 0.55, 0.2], [0.8, 0.9, 0.8])],
        // Tall slab pinned against the -X wall, filling the full depth/height of
        // that side. Released, it collapses and races across the floor.
        ScenePreset::DamBreak => vec![LiquidBlock::new([0.05, 0.05, 0.05], [0.42, 0.95, 0.95])],
        // Two tall columns near opposite walls — fall and collide in the middle.
        ScenePreset::DoubleSplash => vec![
            LiquidBlock::new([0.1, 0.45, 0.3], [0.34, 0.92, 0.7]),
            LiquidBlock::new([0.66, 0.45, 0.3], [0.9, 0.92, 0.7]),
        ],
    };

    if preset == ScenePreset::DamBreak {
        blocks
    } else {
        let delta = drop_height.clamp(0.0, 1.0) - DEFAULT_DROP_HEIGHT;
        blocks
            .into_iter()
            .map(|block| shift_block_y(block, delta))
            .collect()
    }
}

fn shift_block_y(mut block: LiquidBlock, delta: f32) -> LiquidBlock {
    let shift = delta.clamp(-block.min.y, 1.0 - block.max.y);
    block.min.y += shift;
    block.max.y += shift;
    block
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
    fn default_drop_height_preserves_suspended_presets() {
        let falling = scene_for(ScenePreset::FallingBlob, DEFAULT_DROP_HEIGHT);
        let block = falling.initial_liquid.blocks[0];
        assert!((block.min.y - 0.55).abs() < 1.0e-6);
        assert!((block.max.y - 0.9).abs() < 1.0e-6);

        let double = scene_for(ScenePreset::DoubleSplash, DEFAULT_DROP_HEIGHT);
        for block in double.initial_liquid.blocks {
            assert!((block.min.y - 0.45).abs() < 1.0e-6);
            assert!((block.max.y - 0.92).abs() < 1.0e-6);
        }
    }

    #[test]
    fn low_and_high_drop_height_clamp_suspended_blocks_inside_tank() {
        let low = scene_for(ScenePreset::FallingBlob, 0.0)
            .initial_liquid
            .blocks[0];
        assert!((low.min.y - 0.0).abs() < 1.0e-6);
        assert!((low.max.y - 0.35).abs() < 1.0e-6);

        let high = scene_for(ScenePreset::FallingBlob, 1.0)
            .initial_liquid
            .blocks[0];
        assert!((high.min.y - 0.65).abs() < 1.0e-6);
        assert!((high.max.y - 1.0).abs() < 1.0e-6);
    }

    #[test]
    fn default_density_derives_count_from_seeded_cells() {
        // Default registry: density 8/seeded-cell, 64^3 grid, falling-blob preset.
        // Seeded fraction of the falling blob is 0.6*0.35*0.6 = 0.126, so
        // count ≈ 8 * 0.126 * 64^3 ≈ 264k — near the historical ~254k default.
        let scene = SceneConfig::from_settings(&Registry::default());
        assert!(
            (264_000..=264_500).contains(&scene.particle_count),
            "default count {} should be ~264k (8/seeded-cell)",
            scene.particle_count
        );
    }

    #[test]
    fn density_scales_count_with_grid_resolution() {
        let mut settings = Registry::default();
        settings.set_value_f64("grid.res_x", 128.0);
        settings.set_value_f64("grid.res_z", 128.0);
        // 128x64x128, density 8, falling blob -> 8 * 0.126 * 1_048_576 ≈ 1.057M.
        let blob = SceneConfig::from_settings(&settings).particle_count;
        assert!(
            (1_050_000..=1_065_000).contains(&blob),
            "falling-blob count {blob} should be ~1.06M"
        );

        // Dam break fills ~0.30 of the tank, so the same density seeds far more.
        settings.set_value_f64("scene.preset", ScenePreset::DamBreak as u32 as f64);
        let dam = SceneConfig::from_settings(&settings).particle_count;
        assert!(
            (2_400_000..=2_600_000).contains(&dam),
            "dam-break count {dam} should be ~2.5M"
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

        assert!((low.min.y - 0.05).abs() < 1.0e-6);
        assert!((low.max.y - 0.95).abs() < 1.0e-6);
        assert!((high.min.y - low.min.y).abs() < 1.0e-6);
        assert!((high.max.y - low.max.y).abs() < 1.0e-6);
    }
}
