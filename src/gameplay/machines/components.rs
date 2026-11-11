//! Machine components: what a machine does, as data of its block.
//!
//! A block lists its [`MachineComponent`]s (Forge, "Machine components"); this
//! system runs them for every machine, whatever its function. A new machine
//! needs no code: the function `Custom` gives it a storage and a panel, the
//! components give it a behaviour.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::core::game_rules::game_rules;
use crate::core::pie_manager::{PieSet, PieState};
use crate::gameplay::combat::hurt_mob;
use crate::gameplay::data_cables::{CableNetwork, SignalRequests};
use crate::gameplay::energy_grid::{EnergyGrid, MachineState};
use crate::gameplay::inventory::Inventory;
use crate::gameplay::machines::config::MachineConfigs;
use crate::gameplay::machines::{advanced::on_top, output_target};
use crate::gameplay::mobs::runtime::MobInstance;
use crate::gameplay::player_stats::PlayerStats;
use crate::gameplay::world_rules::WorldRules;
use crate::physics::player_controller::Player;
use crate::renderer::particles::{ParticleBurst, ParticleQueue, ParticleStyle};
use crate::scripting::vm::ScriptRuntime;
use crate::voxel::blocks::BlockRegistry;
use crate::voxel::chunk_manager::WorldStreaming;
use crate::voxel::world::VoxelWorld;

/// When a component works.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum RunCondition {
    #[default]
    Always,
    Day,
    Night,
    /// While something stands on the block.
    Stepped,
}

impl RunCondition {
    pub const ALL: [Self; 4] = [Self::Always, Self::Day, Self::Night, Self::Stepped];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Always => "Always",
            Self::Day => "By day",
            Self::Night => "By night",
            Self::Stepped => "While stepped on",
        }
    }
}

/// Weighted resources for the biomes whose name contains one of `biomes`
/// (empty = every other biome).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct BiomeYield {
    pub biomes: Vec<String>,
    pub weights: Vec<(String, f32)>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum MachineOutput {
    Nothing,
    Fixed(String),
    Weighted(Vec<(String, f32)>),
    ByBiome(Vec<BiomeYield>),
}

impl MachineOutput {
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Nothing => "Nothing",
            Self::Fixed(_) => "One resource",
            Self::Weighted(_) => "Random resource",
            Self::ByBiome(_) => "By biome",
        }
    }

    /// The resource given by `roll`, in `[0, 1)`.
    pub fn pick(&self, biome: &str, roll: f32) -> Option<&str> {
        match self {
            Self::Nothing => None,
            Self::Fixed(key) => Some(key),
            Self::Weighted(weights) => pick_weighted(weights, roll),
            Self::ByBiome(tables) => {
                let lower = biome.to_ascii_lowercase();
                tables
                    .iter()
                    .find(|table| table.biomes.iter().any(|name| lower.contains(&name.to_ascii_lowercase())))
                    .or_else(|| tables.iter().find(|table| table.biomes.is_empty()))
                    .and_then(|table| pick_weighted(&table.weights, roll))
            }
        }
    }
}

fn pick_weighted(weights: &[(String, f32)], roll: f32) -> Option<&str> {
    let total: f32 = weights.iter().map(|(_, weight)| weight.max(0.0)).sum();
    let mut left = roll * total;
    for (key, weight) in weights {
        left -= weight.max(0.0);
        if left < 0.0 {
            return Some(key);
        }
    }
    weights.first().map(|(key, _)| key.as_str())
}

/// One conversion: the first recipe the machine can pay runs.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MachineRecipe {
    #[serde(default)]
    pub inputs: Vec<(String, u32)>,
    pub output: MachineOutput,
    #[serde(default = "one")]
    pub min: u32,
    #[serde(default = "one")]
    pub max: u32,
}

fn one() -> u32 {
    1
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ZoneEffect {
    #[default]
    HurtMobs,
    HurtPlayer,
    HealPlayer,
    PushAway,
    PullIn,
}

impl ZoneEffect {
    pub const ALL: [Self; 5] = [Self::HurtMobs, Self::HurtPlayer, Self::HealPlayer, Self::PushAway, Self::PullIn];

    pub const fn label(self) -> &'static str {
        match self {
            Self::HurtMobs => "Hurt entities",
            Self::HurtPlayer => "Hurt the player",
            Self::HealPlayer => "Heal the player",
            Self::PushAway => "Push away",
            Self::PullIn => "Pull in",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum SignalSource {
    Constant(f32),
    Powered,
    /// How many units of a resource the machine holds.
    Stock(String),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum MachineComponent {
    /// Every `every` seconds, pays the first recipe it can.
    Processor {
        every: f32,
        #[serde(default)]
        condition: RunCondition,
        recipes: Vec<MachineRecipe>,
        /// Sends the result to the connected container.
        #[serde(default)]
        to_container: bool,
        #[serde(default)]
        sparks: u32,
    },
    /// Breaks the listed blocks around it and stores their drops.
    Gatherer { every: f32, radius: i32, blocks: Vec<String> },
    /// Pulls resources out of the containers around it (radius and filter
    /// come from the machine's settings).
    Puller { every: f32 },
    /// Acts on what stands in a sphere.
    Zone {
        every: f32,
        radius: f32,
        strength: f32,
        effect: ZoneEffect,
    },
    /// Writes on an output port.
    Signal { port: String, source: SignalSource },
}

impl MachineComponent {
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Processor { .. } => "Processor",
            Self::Gatherer { .. } => "Gatherer",
            Self::Puller { .. } => "Puller",
            Self::Zone { .. } => "Effect zone",
            Self::Signal { .. } => "Signal",
        }
    }

    /// One of each, for the Add menu.
    pub fn palette() -> Vec<Self> {
        vec![
            Self::Processor {
                every: 5.0,
                condition: RunCondition::Always,
                recipes: vec![MachineRecipe { inputs: Vec::new(), output: MachineOutput::Fixed("voltic".to_string()), min: 1, max: 1 }],
                to_container: false,
                sparks: 0,
            },
            Self::Gatherer { every: 4.0, radius: 6, blocks: Vec::new() },
            Self::Puller { every: 1.0 },
            Self::Zone { every: 1.0, radius: 5.0, strength: 1.0, effect: ZoneEffect::HurtMobs },
            Self::Signal { port: "Out".to_string(), source: SignalSource::Powered },
        ]
    }
}

/// Advances a component's timer; true when its period is over.
fn due(machine: &mut MachineState, index: usize, delta: f32, every: f32) -> bool {
    let timer = &mut machine.timers[index];
    *timer += delta;
    if *timer >= every.max(0.01) {
        *timer = 0.0;
        return true;
    }
    false
}

fn can_pay(machine: &MachineState, inputs: &[(String, u32)]) -> bool {
    inputs.iter().all(|(key, amount)| machine.storage.get(key).copied().unwrap_or(0) >= *amount)
}

pub struct MachineComponentPlugin;

impl Plugin for MachineComponentPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, run_components.after(PieSet::Transition));
    }
}

#[allow(clippy::too_many_arguments)]
fn run_components(
    time: Res<Time>,
    pie: Res<PieState>,
    registry: Res<BlockRegistry>,
    rules: Res<WorldRules>,
    streaming: Res<WorldStreaming>,
    cables: Res<CableNetwork>,
    configs: Res<MachineConfigs>,
    inventory: Res<Inventory>,
    (mut world, mut grid): (ResMut<VoxelWorld>, ResMut<EnergyGrid>),
    (mut particles, mut signals, mut stats, mut runtime): (ResMut<ParticleQueue>, ResMut<SignalRequests>, ResMut<PlayerStats>, ResMut<ScriptRuntime>),
    mut players: Query<(&mut Player, &Transform), Without<MobInstance>>,
    mut mobs: Query<(Entity, &mut MobInstance, &Transform), Without<Player>>,
    mut ticks: Local<u32>,
) {
    if !pie.is_playing() {
        return;
    }
    let delta = time.delta_secs();
    *ticks = ticks.wrapping_add(1);
    let daylight = rules.daylight();
    let cells: Vec<IVec3> = grid.machines.keys().copied().collect();
    let player_half = game_rules().player.half_height + game_rules().player.radius;

    for cell in cells {
        let Some(voxel) = world.get(cell) else { continue };
        let def = registry.get(voxel.block);
        if def.components.is_empty() {
            continue;
        }
        if !grid.machines[&cell].powered || configs.get(cell).is_some_and(|config| !config.enabled) {
            continue;
        }
        let center = cell.as_vec3() + Vec3::splat(0.5);
        let roll = |salt: u32| {
            let hash = (cell.x as u32).wrapping_mul(73_856_093) ^ (cell.y as u32).wrapping_mul(19_349_663) ^ (cell.z as u32).wrapping_mul(83_492_791) ^ ticks.wrapping_mul(2_654_435_761) ^ salt;
            (hash % 10_000) as f32 / 10_000.0
        };
        if let Some(machine) = grid.machines.get_mut(&cell) {
            machine.timers.resize(def.components.len(), 0.0);
        }

        for (index, component) in def.components.iter().enumerate() {
            let Some(machine) = grid.machines.get_mut(&cell) else { break };
            match component {
                MachineComponent::Processor { every, condition, recipes, to_container, sparks } => {
                    let holds = match condition {
                        RunCondition::Always => true,
                        RunCondition::Day => daylight > 0.3,
                        RunCondition::Night => daylight <= 0.3,
                        RunCondition::Stepped => {
                            players.iter().any(|(_, transform)| on_top(cell, transform.translation, player_half))
                                || mobs.iter().any(|(_, mob, transform)| on_top(cell, transform.translation, mob.half_height))
                        }
                    };
                    if !holds {
                        if *condition != RunCondition::Stepped {
                            machine.timers[index] = 0.0;
                        }
                        continue;
                    }
                    if !due(machine, index, delta, *every) {
                        continue;
                    }
                    let Some(recipe) = recipes.iter().find(|recipe| can_pay(machine, &recipe.inputs)) else { continue };
                    for (key, amount) in &recipe.inputs {
                        machine.take(key, *amount);
                    }
                    let biome = streaming
                        .generator
                        .as_ref()
                        .map(|generator| generator.biome_name(generator.biome_at(cell.x, cell.z)).to_string())
                        .unwrap_or_default();
                    if let Some(resource) = recipe.output.pick(&biome, roll(1)) {
                        let span = recipe.max.saturating_sub(recipe.min) + 1;
                        let amount = recipe.min + ((roll(2) * span as f32) as u32).min(span - 1);
                        let target = if *to_container { output_target(cell, &grid, &cables) } else { cell };
                        if let Some(store) = grid.machines.get_mut(&target) {
                            store.add(resource, amount);
                        }
                    }
                    if *sparks > 0 {
                        particles.burst(ParticleBurst::new(center + Vec3::Y * 0.6, Vec3::new(0.9, 0.95, 1.0), *sparks, ParticleStyle::Spark));
                    }
                }
                MachineComponent::Gatherer { every, radius, blocks } => {
                    if !due(machine, index, delta, *every) {
                        continue;
                    }
                    let wanted: Vec<_> = blocks.iter().filter_map(|key| registry.find(key)).collect();
                    let mut found = None;
                    'search: for dy in -2..=*radius {
                        for dz in -*radius..=*radius {
                            for dx in -*radius..=*radius {
                                let probe = cell + IVec3::new(dx, dy, dz);
                                if let Some(voxel) = world.get(probe).filter(|voxel| wanted.contains(&voxel.block)) {
                                    found = Some((probe, voxel.block));
                                    break 'search;
                                }
                            }
                        }
                    }
                    let Some((probe, block)) = found else { continue };
                    let gathered = registry.get(block);
                    let drop = crate::gameplay::inventory::drop_for(&registry, block).unwrap_or_else(|| gathered.id.clone());
                    world.set(probe, None);
                    let target = output_target(cell, &grid, &cables);
                    if let Some(store) = grid.machines.get_mut(&target) {
                        store.add(&drop, 1);
                    }
                    particles.burst(ParticleBurst::new(probe.as_vec3() + Vec3::splat(0.5), gathered.color_vec(), 10, ParticleStyle::Crumble));
                }
                MachineComponent::Puller { every } => {
                    if !due(machine, index, delta, *every) {
                        continue;
                    }
                    let config = configs.get(cell);
                    let radius = config.map_or(12.0, |config| config.radius).clamp(1.0, 64.0);
                    let filter = config.filter(|config| config.mode == 1 && !config.target.is_empty()).map(|config| config.target.clone());
                    let target = output_target(cell, &grid, &cables);
                    let sources: Vec<IVec3> = grid
                        .machines
                        .iter()
                        .filter(|(other, state)| **other != target && **other != cell && state.function.is_storage() && other.as_vec3().distance(cell.as_vec3()) <= radius)
                        .map(|(other, _)| *other)
                        .collect();
                    for source in sources {
                        let pulled = grid.machines.get_mut(&source).and_then(|state| match &filter {
                            Some(key) => state.take(key, 1).then(|| key.clone()),
                            None => state.pop_any(),
                        });
                        if let (Some(item), Some(store)) = (pulled, grid.machines.get_mut(&target)) {
                            store.add(&item, 1);
                        }
                    }
                }
                MachineComponent::Zone { every, radius, strength, effect } => {
                    if !due(machine, index, delta, *every) {
                        continue;
                    }
                    let vulnerable = inventory.survival && !inventory.spectator;
                    if let Ok((mut player, transform)) = players.single_mut() {
                        let to = transform.translation - center;
                        if to.length() <= *radius {
                            match effect {
                                ZoneEffect::HurtPlayer if vulnerable => {
                                    stats.damage(*strength);
                                }
                                ZoneEffect::HealPlayer => stats.heal(*strength),
                                ZoneEffect::PushAway if vulnerable => player.knockback += to.normalize_or_zero() * *strength,
                                ZoneEffect::PullIn if vulnerable => player.knockback -= to.normalize_or_zero() * *strength,
                                _ => {}
                            }
                        }
                    }
                    for (entity, mut mob, transform) in &mut mobs {
                        let to = transform.translation - center;
                        if to.length() > *radius {
                            continue;
                        }
                        match effect {
                            ZoneEffect::HurtMobs => hurt_mob(entity, &mut mob, *strength, Vec3::ZERO, center, &mut runtime),
                            ZoneEffect::PushAway => mob.knockback += to.normalize_or_zero() * *strength,
                            ZoneEffect::PullIn => mob.knockback -= to.normalize_or_zero() * *strength,
                            _ => {}
                        }
                    }
                }
                MachineComponent::Signal { port, source } => {
                    if !due(machine, index, delta, 0.25) {
                        continue;
                    }
                    let value = match source {
                        SignalSource::Constant(value) => *value,
                        SignalSource::Powered => 1.0,
                        SignalSource::Stock(key) => machine.storage.get(key).copied().unwrap_or(0) as f32,
                    };
                    signals.pending.push((cell, port.clone(), value));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn biome_tables_pick_by_name_then_fall_back() {
        let table = |biomes: &[&str], key: &str| BiomeYield { biomes: biomes.iter().map(|b| b.to_string()).collect(), weights: vec![(key.to_string(), 1.0)] };
        let output = MachineOutput::ByBiome(vec![table(&["forest"], "floral"), table(&[], "mineral")]);
        assert_eq!(output.pick("Great Forest", 0.5), Some("floral"));
        assert_eq!(output.pick("Crater", 0.5), Some("mineral"));
    }

    #[test]
    fn weights_cover_the_whole_roll() {
        let output = MachineOutput::Weighted(vec![("a".into(), 3.0), ("b".into(), 1.0)]);
        assert_eq!(output.pick("", 0.1), Some("a"));
        assert_eq!(output.pick("", 0.9), Some("b"));
        assert_eq!(output.pick("", 0.9999), Some("b"));
    }

    #[test]
    fn the_shipped_machines_are_data() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("project/blocks");
        for name in ["solar", "kineticster", "atomiser", "excavator", "harvester", "vacuum"] {
            let text = std::fs::read_to_string(root.join(format!("{name}.ron"))).expect("block file");
            let def: crate::voxel::blocks::BlockDef = ron::from_str(&text).expect("block parses");
            assert!(!def.components.is_empty(), "{name} has no component");
        }
    }
}
