//! Machine behaviours.
//!
//! Machines are ordinary blocks whose `function` is a machine kind. Their
//! runtime state (powered, timers, internal storage, fuel) lives in the
//! [`EnergyGrid`]; their settings in [`config::MachineConfigs`]. Behaviours
//! run while powered (or always, see `BlockFunction::needs_no_power`) and
//! while their setting `enabled` is on (`/stopmachine` turns every machine
//! off).
//!
//! `E` on a machine opens its panel (drawn by `editor::machine_ui`); the
//! panel queues [`MachineCommand`]s that [`apply_machine_commands`] runs.
//!
//! | Machine          | Behaviour                                                        |
//! |------------------|------------------------------------------------------------------|
//! | Combustor        | 10 Floral / 10 Faunal -> 1 Voltic of power for a small field.     |
//! | Solaronel        | Daylight -> 1 Voltic every 9 s (storage / Resource output).       |
//! | Kineticster      | Pressure (an entity on it) -> 1 Voltic every 12 s.                |
//! | Energiser        | Spends Voltic to power the machines of its field.                 |
//! | Synthesizer      | Floral + Mineral / Crystalline -> Synthetic (by hand).            |
//! | Assembler        | Synthetic, Technic, Conflux, Omnicore.                            |
//! | Infuser          | Two-resource infusions; a failed one gives Garbage.               |
//! | Atomiser         | Conflux (or 8 Garbage) + 100 Voltic -> Atomic (panel or storage). |
//! | Reconstructor    | Learns a resource fed 10 times, then 2 Atomic + 1000 Voltic -> 1 copy. |
//! | Excavator        | 10 Voltic -> 4 to 8 resources of its biome every 4 s.             |
//! | Ethereaper       | Drains the Ethereal of the entities on it (x2) until they die.    |
//! | Mnemo-Harvestor  | Gathers raw resource blocks around it into a container.           |
//! | Astral-Vacum     | Pulls the chosen resources out of the containers in its radius.   |
//! | Container / Astral-Container | Store resources (27 slots / 1 000 000 units).        |
//! | Omnichorus       | Sends Omnicore into the network.                                  |
//! | Omnireactor      | Power field fed by the Omnicore network (1 Omnicore / 60 s).      |
//! | Repulsor / Ringer / Mnemo-Digestor / Ressourcer / Mnemo-Cloner | see [`advanced`]. |
//! | Motor            | Turns the blocks on its top face ([`motors`]).                    |
//! | Light            | Lights up.                                                        |
//!
//! [`visuals`] adds the animated parts (hub, blades, fan, crystal, ring).

pub mod advanced;
pub mod components;
pub mod config;
pub mod motors;
pub mod visuals;

use crate::core::game_rules::game_rules;
use bevy::prelude::*;

use crate::audio::dsp_engine::SoundKind;
use crate::audio::sound_events::SoundQueue;
use crate::core::app_state::EditorUiState;
use crate::core::pie_manager::{PieSet, PieState};
use crate::gameplay::energy_grid::EnergyGrid;
use crate::gameplay::inventory::Inventory;
use crate::gameplay::mining::InteractRequests;
use crate::renderer::particles::{ParticleBurst, ParticleQueue, ParticleStyle};
use crate::voxel::blocks::{BlockFunction, BlockRegistry};
use crate::voxel::world::VoxelWorld;



pub struct MachinePlugin;

impl Plugin for MachinePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((config::MachineConfigPlugin, components::MachineComponentPlugin))
            .init_resource::<MachinePanel>()
            .init_resource::<motors::Motors>()
            .init_resource::<visuals::MachineVisuals>()
            .add_systems(
                Update,
                (
                    machine_interactions,
                    apply_machine_commands,
                    run_machines,
                    advanced::run_entity_machines,
                    motors::run_motors,
                    visuals::sync_machine_visuals,
                )
                    .chain()
                    .after(PieSet::Transition)
                    .after(crate::gameplay::energy_grid::GridSet)
                    .after(crate::gameplay::mining::MiningSet),
            );
    }
}

/// Action requested from a machine panel.
#[derive(Clone, Debug, PartialEq)]
pub enum MachineCommand {
    /// Put 10 Floral (or 10 Faunal) from the inventory into a Combustor.
    AddFuel(IVec3),
    Collect(IVec3),
    /// Astral-Modifier: buy the boost at this index of `advanced::BOOSTS`.
    Boost(IVec3, usize),
    /// Invocateral: summon an entity of this biome.
    Summon(IVec3, String),
    /// Mnemo-Cloner: make a Mnemo double of this entity.
    Clone(IVec3, String),
    /// Etheraltar: teleport to this other Etheraltar.
    Teleport(IVec3),
    /// Televerseur: save the box to its file / load the file into the box.
    Export(IVec3),
    Import(IVec3),
    /// Craft this recipe `n` times.
    CraftRecipe(IVec3, crate::gameplay::crafting::Recipe, u32),
    /// Move inventory Omnicore into the network (`true` = everything).
    TransferOmnicore(IVec3, bool),
    /// Move one stack of this resource from the inventory into the machine.
    Deposit(IVec3, String),
    /// Move one stack of this resource from the machine into the inventory.
    Withdraw(IVec3, String),
    /// Put the stack of this resource in this storage slot (reorganising).
    MoveStored(IVec3, String, usize),
}

/// Settings of a motor mounted on a launched movable, edited by its panel
/// and applied by `physics::movable_compound`.
#[derive(Clone, Debug)]
pub struct BodyMotorEdit {
    pub body: Entity,
    pub index: usize,
    pub enabled: bool,
    pub rpm: f32,
    pub clockwise: bool,
    /// Changed by the panel, not yet applied to the body.
    pub dirty: bool,
}

/// The machine panel currently open (`E` on a machine).
#[derive(Resource, Debug, Default)]
pub struct MachinePanel {
    pub open: Option<IVec3>,
    /// Panel of a motor on a launched movable.
    pub body_motor: Option<BodyMotorEdit>,
    /// Set on the opening frame (the `E` press must not close it again).
    pub just_opened: bool,
    pub commands: Vec<MachineCommand>,
    /// Slot crafting of the open station: what lies in each slot.
    pub slots: Vec<Option<String>>,
    pub slots_cell: Option<IVec3>,
}

/// Functions that open a panel on `E`.
pub fn has_panel(function: BlockFunction) -> bool {
    function.is_machine()
        || matches!(
            function,
            BlockFunction::Controller | BlockFunction::Assembler | BlockFunction::Omnichorus
        )
}

/// `E` on a machine opens its panel; the panel closes when the machine is
/// gone or the player walks away.
fn machine_interactions(
    pie: Res<PieState>,
    requests: Res<InteractRequests>,
    world: Res<VoxelWorld>,
    registry: Res<BlockRegistry>,
    mut panel: ResMut<MachinePanel>,
    mut motors: ResMut<motors::Motors>,
    players: Query<&Transform, With<crate::physics::player_controller::Player>>,
    mut sounds: ResMut<SoundQueue>,
    held: Res<crate::gameplay::items::HeldTool>,
) {
    if !pie.is_playing() {
        panel.open = None;
        panel.commands.clear();
        return;
    }
    for cell in &requests.cells {
        let Some(voxel) = world.get(*cell) else {
            continue;
        };
        let function = registry.get(voxel.block).function;
        if !has_panel(function) {
            continue;
        }
        // With the Remote in hand, `E` connects the controller instead.
        if held.remote && function == BlockFunction::Controller {
            continue;
        }
        if function == BlockFunction::Motor {
            motors.config_mut(*cell);
        }
        panel.open = Some(*cell);
        panel.body_motor = None;
        panel.just_opened = true;
        sounds.play_kind(SoundKind::UiClick);
    }
    if let Some(cell) = panel.open {
        let present = world.get(cell).is_some_and(|voxel| has_panel(registry.get(voxel.block).function));
        let near = players
            .single()
            .map(|transform| transform.translation.distance(cell.as_vec3() + Vec3::splat(0.5)) < 9.0)
            .unwrap_or(false);
        if !present || !near {
            panel.open = None;
        }
    }
}

/// Run the actions queued by the machine panels.
#[allow(clippy::too_many_arguments)]
fn apply_machine_commands(
    pie: Res<PieState>,
    mut panel: ResMut<MachinePanel>,
    mut grid: ResMut<EnergyGrid>,
    mut inventory: ResMut<Inventory>,
    mut sounds: ResMut<SoundQueue>,
    mut ui: ResMut<EditorUiState>,
    mut network: ResMut<crate::gameplay::crafting::OmnicoreNetwork>,
    registry: Res<BlockRegistry>,
    items: Res<crate::gameplay::items::ItemLibrary>,
    mut extra: advanced::CommandExtras,
) {
    if !pie.is_playing() {
        return;
    }
    for command in std::mem::take(&mut panel.commands) {
        match command {
            MachineCommand::AddFuel(cell) => {
                let batch = game_rules().energy.fuel_batch;
                let Some(machine) = grid.machine_mut(cell) else { continue };
                let fuel = ["floral", "faunal"].into_iter().find(|key| inventory.count(key) >= batch).map(str::to_string).or_else(|| {
                    inventory.resources.iter().find(|(key, count)| **count >= batch && crate::gameplay::crafting::accepts("@floral", key)).map(|(key, _)| key.clone())
                });
                match fuel {
                    Some(key) if inventory.take(&key, batch) => {
                        machine.add(&key, batch);
                        sounds.play_kind(SoundKind::PowerOn);
                        ui.action(format!("{batch} {} put in the Combustor.", crate::gameplay::crafting::resource_label(&key)));
                    }
                    _ if !inventory.survival => {
                        machine.fuel += game_rules().energy.fuel_per_unit;
                        sounds.play_kind(SoundKind::PowerOn);
                    }
                    _ => ui.warn(format!("Needs {batch} Floral or {batch} Faunal in the inventory.")),
                }
            }
            MachineCommand::Boost(cell, index) => advanced::buy_boost(cell, index, &grid, &mut inventory, &mut extra, &mut ui, &mut sounds),
            MachineCommand::Summon(cell, biome) => advanced::summon(cell, &biome, &mut inventory, &mut extra, &mut ui, &mut sounds),
            MachineCommand::Clone(cell, rig) => advanced::start_clone(cell, &rig, &mut grid, &mut inventory, &mut extra, &mut ui, &mut sounds),
            MachineCommand::Teleport(to) => {
                if inventory.survival && !inventory.take("ethereal", game_rules().vitals.respawn_altar_cost) {
                    ui.warn(format!("Needs {} Ethereal.", game_rules().vitals.respawn_altar_cost));
                    continue;
                }
                if let Ok((mut player, mut transform)) = extra.players.single_mut() {
                    transform.translation = to.as_vec3() + Vec3::new(0.5, 2.2, 0.5);
                    player.velocity = Vec3::ZERO;
                }
                panel.open = None;
                sounds.play_kind(SoundKind::PowerOn);
            }
            MachineCommand::Export(cell) => advanced::export_box(cell, &registry, &mut extra, &mut ui),
            MachineCommand::Import(cell) => advanced::import_box(cell, &registry, &mut extra, &mut ui),
            MachineCommand::CraftRecipe(cell, recipe, times) => {
                if recipe.station.needs_power() && !grid.machine(cell).is_some_and(|m| m.powered) {
                    ui.warn(format!("The {} is not powered.", recipe.station.label()));
                    continue;
                }
                let mut made = 0;
                let mut name = String::new();
                for _ in 0..times.max(1) {
                    match crate::gameplay::crafting::craft(&mut inventory, &recipe, &registry, &items) {
                        Ok((label, count)) => {
                            made += count;
                            name = label;
                        }
                        Err(error) => {
                            if made == 0 {
                                ui.warn(error);
                            }
                            break;
                        }
                    }
                }
                if made > 0 {
                    sounds.play_kind(SoundKind::Pickup);
                    ui.action(format!("Crafted {made} {name}."));
                }
            }
            MachineCommand::TransferOmnicore(_cell, all) => {
                let owned = inventory.count("omnicore");
                let amount = if all { owned } else { owned.min(1) };
                if amount == 0 {
                    ui.warn("No Omnicore in the inventory: craft it at an Assembler.");
                    continue;
                }
                inventory.take("omnicore", amount);
                network.deposit(amount);
                sounds.play_kind(SoundKind::PowerOn);
                ui.action(format!("{amount} Omnicore transferred: the network holds {}.", network.stored));
            }
            MachineCommand::Deposit(cell, key) => {
                let Some(machine) = grid.machine_mut(cell) else { continue };
                let stack = game_rules().inventory.stack_size;
                let used: u32 = machine.storage.values().sum();
                let free = machine.function.capacity().saturating_sub(used);
                let amount = if inventory.survival { inventory.count(&key).min(stack) } else { stack }.min(free);
                if amount == 0 {
                    ui.warn("The storage is full.");
                    continue;
                }
                if inventory.survival && !inventory.take(&key, amount) {
                    continue;
                }
                machine.add(&key, amount);
                sounds.play_kind(SoundKind::Pickup);
            }
            MachineCommand::MoveStored(cell, key, index) => {
                if let Some(machine) = grid.machine_mut(cell) {
                    machine.move_to(&key, index);
                }
            }
            MachineCommand::Withdraw(cell, key) => {
                let Some(machine) = grid.machine_mut(cell) else { continue };
                let stored = machine.storage.get(&key).copied().unwrap_or(0);
                let amount = inventory.room_for(&key, stored.min(game_rules().inventory.stack_size));
                if amount == 0 {
                    ui.warn("The inventory is full.");
                    continue;
                }
                machine.take(&key, amount);
                inventory.add(&key, amount);
                sounds.play_kind(SoundKind::Pickup);
            }
            MachineCommand::Collect(cell) => {
                let Some(machine) = grid.machine_mut(cell) else { continue };
                let mut moved = 0;
                while let Some(item) = machine.storage.keys().next().cloned() {
                    if inventory.room_for(&item, 1) == 0 {
                        break;
                    }
                    machine.take(&item, 1);
                    inventory.add(&item, 1);
                    moved += 1;
                }
                if moved > 0 {
                    sounds.play_kind(SoundKind::Pickup);
                    ui.action(format!("Collected {moved} item(s)."));
                } else {
                    ui.info("Nothing stored yet.");
                }
            }
        }
    }
}

/// Storage of the container connected to `cell` (Resource cable from it),
/// else an adjacent container, else the machine itself.
pub(crate) fn output_target(cell: IVec3, grid: &EnergyGrid, cables: &crate::gameplay::data_cables::CableNetwork) -> IVec3 {
    let wired = cables
        .cables
        .iter()
        .filter(|cable| cable.kind == crate::voxel::blocks::PortKind::Resource && IVec3::from_array(cable.from) == cell)
        .map(|cable| IVec3::from_array(cable.to))
        .find(|to| grid.machine(*to).is_some_and(|m| m.function.is_storage()));
    if let Some(to) = wired {
        return to;
    }
    for offset in [IVec3::X, IVec3::NEG_X, IVec3::Z, IVec3::NEG_Z, IVec3::Y, IVec3::NEG_Y] {
        if grid.machine(cell + offset).is_some_and(|m| m.function.is_storage()) {
            return cell + offset;
        }
    }
    cell
}

#[allow(clippy::too_many_arguments)]
fn run_machines(
    time: Res<Time>,
    pie: Res<PieState>,
    configs: Res<config::MachineConfigs>,
    mut network: ResMut<crate::gameplay::crafting::OmnicoreNetwork>,
    mut grid: ResMut<EnergyGrid>,
    mut particles: ResMut<ParticleQueue>,
    mut ticks: Local<u32>,
) {
    if !pie.is_playing() {
        return;
    }
    let delta = time.delta_secs();
    *ticks = ticks.wrapping_add(1);
    let cells: Vec<IVec3> = grid.machines.keys().copied().collect();

    for cell in cells {
        let (function, powered) = {
            let machine = &grid.machines[&cell];
            (machine.function, machine.powered)
        };
        if !powered || configs.get(cell).is_some_and(|config| !config.enabled) {
            continue;
        }
        let center = cell.as_vec3() + Vec3::splat(0.5);
        let Some(machine) = grid.machines.get_mut(&cell) else {
            continue;
        };
        machine.timer += delta;

        match function {
            BlockFunction::Combustor => {
                if machine.timer > 0.6 && machine.fuel > 0.0 {
                    machine.timer = 0.0;
                    particles.burst(ParticleBurst::new(center + Vec3::Y * 0.7, Vec3::new(0.35, 0.35, 0.38), 2, ParticleStyle::Smoke));
                }
            }
            BlockFunction::Omnireactor => {
                if machine.timer >= 60.0 {
                    machine.timer = 0.0;
                    network.spend(1);
                }
            }
            BlockFunction::Reconstructor => {
                if machine.timer < game_rules().machines.reconstructor_period {
                    continue;
                }
                machine.timer = 0.0;
                // The learnt resource: the first one fed 10 times.
                let learnt = machine
                    .storage
                    .iter()
                    .filter(|(key, count)| !matches!(key.as_str(), "atomic" | "voltic") && **count >= game_rules().machines.reconstructor_learn)
                    .map(|(key, _)| key.clone())
                    .next();
                let atomic = machine.storage.get("atomic").copied().unwrap_or(0);
                let voltic = machine.storage.get("voltic").copied().unwrap_or(0);
                if let Some(key) = learnt {
                    if atomic >= 2 && voltic >= 1000 {
                        machine.take("atomic", 2);
                        machine.take("voltic", 1000);
                        machine.add(&key, 1);
                        particles.burst(ParticleBurst::new(center + Vec3::Y * 0.6, Vec3::new(0.7, 0.9, 1.0), 10, ParticleStyle::Spark));
                    }
                }
            }
            _ => {}
        }
    }
}
