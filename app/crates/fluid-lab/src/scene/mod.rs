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
        let res = UVec3::new(settings.grid_res_x(), settings.grid_res_y(), settings.grid_res_z());
        let preset = ScenePreset::from_u32(settings.scene_preset());
        Self {
            name: preset.name().to_string(),
            preset,
            grid_resolution: res,
            particle_count: settings.particle_count(),
            initial_liquid: InitialLiquidConfig {
                blocks: preset_blocks(preset),
            },
        }
    }

    /// The historical default scene (falling blob), independent of the registry's
    /// scene selector. Kept for any caller that wants the canonical default look.
    pub fn default_tank(settings: &Registry) -> Self {
        let res = UVec3::new(settings.grid_res_x(), settings.grid_res_y(), settings.grid_res_z());
        Self {
            name: ScenePreset::FallingBlob.name().to_string(),
            preset: ScenePreset::FallingBlob,
            grid_resolution: res,
            particle_count: settings.particle_count(),
            initial_liquid: InitialLiquidConfig {
                blocks: preset_blocks(ScenePreset::FallingBlob),
            },
        }
    }
}

/// The deterministic liquid layout for each preset (normalized [0,1]^3, y up).
fn preset_blocks(preset: ScenePreset) -> Vec<LiquidBlock> {
    match preset {
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
    }
}
