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
        Self {
            name: preset.name().to_string(),
            preset,
            grid_resolution: res,
            particle_count: settings.particle_count(),
            initial_liquid: InitialLiquidConfig {
                blocks: preset_blocks(preset, settings.drop_height()),
            },
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
        Self {
            name: ScenePreset::FallingBlob.name().to_string(),
            preset: ScenePreset::FallingBlob,
            grid_resolution: res,
            particle_count: settings.particle_count(),
            initial_liquid: InitialLiquidConfig {
                blocks: preset_blocks(ScenePreset::FallingBlob, settings.drop_height()),
            },
        }
    }
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
    fn dam_break_ignores_drop_height() {
        let low = scene_for(ScenePreset::DamBreak, 0.0).initial_liquid.blocks[0];
        let high = scene_for(ScenePreset::DamBreak, 1.0).initial_liquid.blocks[0];

        assert!((low.min.y - 0.05).abs() < 1.0e-6);
        assert!((low.max.y - 0.95).abs() < 1.0e-6);
        assert!((high.min.y - low.min.y).abs() < 1.0e-6);
        assert!((high.max.y - low.max.y).abs() < 1.0e-6);
    }
}
