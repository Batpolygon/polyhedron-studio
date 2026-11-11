//! Per-block settings of the configurable machines (Terminal lines, Emitter
//! value, Repulsor strength, Laser output, Ringer target...). Saved in the
//! played world's `.meta` (`WorldMeta::machines`).

use std::collections::HashMap;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::core::pie_manager::{PieSet, PieState};
use crate::voxel::blocks::BlockFunction;

/// Settings of one machine block. The fields are generic: each machine uses
/// the ones it needs (see [`MachineConfig::for_function`]).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MachineConfig {
    /// Terminal: command lines (one per line). Comandor: the command.
    /// Emitter: the string value. Receiver: the variable name. Televerseur:
    /// the file name. Etheraltar: its name.
    #[serde(default)]
    pub text: String,
    /// Transmitter: gate (see [`GATES`]). Emitter: value type (0 bool, 1
    /// float, 2 string). Laser: output (0 hit, 1 length, 2 name). Ressourcer:
    /// 0 collect, 1 distribute. Astral-Vacum: 0 = everything, 1 = filter.
    #[serde(default)]
    pub mode: u8,
    /// Emitter float value / Repulsor strength / Laser length.
    #[serde(default)]
    pub number: f32,
    /// Emitter boolean value / machine switched on (Repulsor, Ringer...).
    #[serde(default = "yes")]
    pub enabled: bool,
    /// Radius of the Repulsor, Ringer, Astral-Vacum.
    #[serde(default = "default_radius")]
    pub radius: f32,
    /// Ringer: targeted entity (empty = every entity). Astral-Vacum: the
    /// resource to pull. Reconstructor: the learnt resource. Invocateral:
    /// the biome. Mnemo-Cloner: the entity to clone.
    #[serde(default)]
    pub target: String,
    /// Televerseur box size (blocks).
    #[serde(default = "default_size")]
    pub size: [i32; 3],
    /// Runtime: next Terminal line, Reconstructor learnt units...
    #[serde(skip)]
    pub cursor: usize,
    #[serde(skip)]
    pub timer: f32,
    /// Comandor: run once requested by the panel or a rising signal.
    #[serde(skip)]
    pub trigger: bool,
}

fn yes() -> bool {
    true
}

fn default_radius() -> f32 {
    8.0
}

fn default_size() -> [i32; 3] {
    [8, 8, 8]
}

/// Gates of the Transmitter (index = `mode`); the last one is the resource
/// decoder.
pub const GATES: [&str; 7] = ["AND", "OR", "XOR", "NOT", "NAND", "NOR", "Decoder"];
/// Index of the decoder mode of the Transmitter.
pub const DECODER: u8 = 6;

impl Default for MachineConfig {
    fn default() -> Self {
        Self {
            text: String::new(),
            mode: 0,
            number: 0.0,
            enabled: true,
            radius: default_radius(),
            target: String::new(),
            size: default_size(),
            cursor: 0,
            timer: 0.0,
            trigger: false,
        }
    }
}

impl MachineConfig {
    /// Default settings of a freshly placed machine.
    pub fn for_function(function: BlockFunction) -> Self {
        let mut config = Self::default();
        match function {
            BlockFunction::Repulsor => {
                config.number = 10.0;
                config.radius = 5.0;
            }
            BlockFunction::Laser => config.number = 32.0,
            BlockFunction::Terminal => config.text = "/sendchat Hello from the Terminal".to_string(),
            BlockFunction::Comandor => config.text = "/settime day".to_string(),
            BlockFunction::Receiver => config.text = "value".to_string(),
            BlockFunction::Emitter => {
                config.mode = 0;
                config.number = 1.0;
            }
            BlockFunction::Ringer => config.radius = 16.0,
            BlockFunction::Vacuum => config.radius = 12.0,
            BlockFunction::Televerseur => config.text = "construction".to_string(),
            _ => {}
        }
        config
    }
}

/// Settings of every configured machine of the world being played.
#[derive(Resource, Debug, Default)]
pub struct MachineConfigs {
    pub map: HashMap<IVec3, MachineConfig>,
    /// Changed since the world meta was last written.
    pub dirty: bool,
}

impl MachineConfigs {
    /// Settings of `cell`, created with the defaults of `function`.
    pub fn get_or_default(&mut self, cell: IVec3, function: BlockFunction) -> &mut MachineConfig {
        self.map.entry(cell).or_insert_with(|| MachineConfig::for_function(function))
    }

    pub fn get(&self, cell: IVec3) -> Option<&MachineConfig> {
        self.map.get(&cell)
    }

    /// Serializable form for the world meta.
    pub fn saved(&self) -> Vec<SavedConfig> {
        let mut list: Vec<SavedConfig> = self.map.iter().map(|(cell, config)| SavedConfig { cell: cell.to_array(), config: config.clone() }).collect();
        list.sort_by_key(|saved| saved.cell);
        list
    }

    pub fn restore(&mut self, saved: &[SavedConfig]) {
        self.map = saved.iter().map(|saved| (IVec3::from_array(saved.cell), saved.config.clone())).collect();
        self.dirty = false;
    }
}

/// One machine setting stored in the world meta.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SavedConfig {
    pub cell: [i32; 3],
    pub config: MachineConfig,
}

pub struct MachineConfigPlugin;

impl Plugin for MachineConfigPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MachineConfigs>().add_systems(Update, forget_removed.after(PieSet::Transition));
    }
}

/// Settings of a removed machine are dropped; play from the studio starts
/// empty.
fn forget_removed(
    pie: Res<PieState>,
    world: Res<crate::voxel::world::VoxelWorld>,
    registry: Res<crate::voxel::blocks::BlockRegistry>,
    mut configs: ResMut<MachineConfigs>,
    mut last_revision: Local<u64>,
) {
    if pie.started_this_frame && pie.persist_world.is_none() {
        configs.map.clear();
    }
    if !pie.is_playing() || *last_revision == world.revision {
        return;
    }
    *last_revision = world.revision;
    let before = configs.map.len();
    configs.map.retain(|cell, _| world.get(*cell).is_some_and(|voxel| registry.get(voxel.block).function != BlockFunction::None));
    if configs.map.len() != before {
        configs.dirty = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configs_round_trip_through_the_meta_form() {
        let mut configs = MachineConfigs::default();
        configs.get_or_default(IVec3::new(1, 2, 3), BlockFunction::Repulsor).number = 4.0;
        let saved = configs.saved();
        let text = ron::ser::to_string(&saved).expect("serialize");
        let back: Vec<SavedConfig> = ron::from_str(&text).expect("parse");
        let mut restored = MachineConfigs::default();
        restored.restore(&back);
        assert_eq!(restored.get(IVec3::new(1, 2, 3)).map(|c| c.number), Some(4.0));
    }
}
