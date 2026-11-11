//! Machines that act on entities, and the panel actions of the special
//! machines.
//!
//! | Machine         | Behaviour                                                        |
//! |-----------------|------------------------------------------------------------------|
//! | Repulsor        | Pushes the entities, the player and the movables in its radius (strength set in its panel). |
//! | Ringer          | Attracts the entities (all, or one kind) in its radius.          |
//! | Ethereaper      | Every second takes 1 Ethereal from each entity on it (x2 into its storage); the entity dies when it has none left. |
//! | Mnemo-Digestor  | Every second hurts the entities walking on it.                    |
//! | Ressourcer      | Collects the resources of the entities on it, or distributes its storage to the player. |
//! | Kineticster     | An entity standing on it -> 1 Voltic every 12 s.                  |
//! | Mnemo-Cloner    | Keeps its Mnemo clones alive with 1 Ethereal per second (and 1 per damage point); a clone too far away or without Ethereal dissolves. |
//! | Invocateral     | Summons an entity of a discovered biome (every entity of it killed once) for Ethereal. |
//! | Astral-Modifier | Permanent player boosts paid in Astral.                           |
//! | Televerseur     | Saves the box above it to `project/exports/<name>.ron` and loads it back. |

use crate::core::game_rules::game_rules;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy_rapier3d::prelude::ExternalImpulse;

use crate::audio::dsp_engine::SoundKind;
use crate::audio::sound_events::SoundQueue;
use crate::core::app_state::EditorUiState;
use crate::core::pie_manager::PieState;
use crate::core::project_io::{load_ron, save_ron, ProjectLayout};
use crate::gameplay::energy_grid::EnergyGrid;
use crate::gameplay::inventory::Inventory;
use crate::gameplay::machines::config::MachineConfigs;
use crate::gameplay::mob_ai::Bestiary;
use crate::gameplay::mobs::library::MobLibrary;
use crate::gameplay::mobs::runtime::{MobInstance, MobSpawnRequests};
use crate::gameplay::tools::StatBoosts;
use crate::physics::movable_compound::MovableBody;
use crate::physics::player_controller::Player;
use crate::renderer::particles::{ParticleBurst, ParticleQueue, ParticleStyle};
use crate::scripting::vm::ScriptRuntime;
use crate::voxel::blocks::{BlockFunction, BlockRegistry};
use crate::voxel::structures::{StructureCell, StructureDef};
use crate::voxel::world::VoxelWorld;


/// Astral-Modifier boosts: (label, Astral cost).
/// Boosts of the Astral-Modifier, with their price in Astral.
pub fn boosts() -> [(&'static str, u32); 5] {
    let rules = game_rules();
    let costs = &rules.machines;
    [
        ("+1 heart", costs.boost_heart_cost),
        ("+1 stamina", costs.boost_stamina_cost),
        ("+0.5 attack damage", costs.boost_damage_cost),
        ("+0.1 regeneration / s", costs.boost_regen_cost),
        ("+0.5 armour", costs.boost_armour_cost),
    ]
}

/// A Mnemo double made by a Mnemo-Cloner.
#[derive(Component, Debug, Clone, Copy)]
pub struct MnemoClone {
    pub cloner: IVec3,
    pub last_health: f32,
}

/// What the machine panel actions need besides the grid and the inventory.
#[derive(SystemParam)]
pub struct CommandExtras<'w, 's> {
    pub players: Query<'w, 's, (&'static mut Player, &'static mut Transform), Without<MobInstance>>,
    pub library: Res<'w, MobLibrary>,
    pub requests: ResMut<'w, MobSpawnRequests>,
    pub boosts: ResMut<'w, StatBoosts>,
    pub bestiary: Res<'w, Bestiary>,
    pub configs: ResMut<'w, MachineConfigs>,
    pub project: Res<'w, ProjectLayout>,
    pub world: ResMut<'w, VoxelWorld>,
}

pub fn buy_boost(
    cell: IVec3,
    index: usize,
    grid: &EnergyGrid,
    inventory: &mut Inventory,
    extra: &mut CommandExtras,
    ui: &mut EditorUiState,
    sounds: &mut SoundQueue,
) {
    if !grid.machine(cell).is_some_and(|m| m.powered) {
        ui.warn("The Astral-Modifier is not powered.");
        return;
    }
    let Some((label, cost)) = boosts().get(index).copied() else { return };
    if inventory.survival && !inventory.take("astral", cost) {
        ui.warn(format!("Needs {cost} Astral."));
        return;
    }
    let boosts = &mut extra.boosts;
    match index {
        0 => boosts.health += 1.0,
        1 => boosts.stamina += 1.0,
        2 => boosts.damage += 0.5,
        3 => boosts.regen += 0.1,
        _ => boosts.armor += 0.5,
    }
    sounds.play_kind(SoundKind::PowerOn);
    ui.action(format!("Astral boost: {label}."));
}

pub fn summon(cell: IVec3, biome: &str, inventory: &mut Inventory, extra: &mut CommandExtras, ui: &mut EditorUiState, sounds: &mut SoundQueue) {
    if !extra.bestiary.summonable(&extra.library).iter().any(|b| b == biome) {
        ui.warn("Discover this biome and kill each of its entities once first.");
        return;
    }
    let living: Vec<String> = extra
        .library
        .mob_rigs()
        .filter(|rig| crate::gameplay::mob_ai::lives_in(&rig.biomes, biome) && !rig.biomes.iter().any(|b| b == "*"))
        .map(|rig| rig.name.clone())
        .collect();
    if living.is_empty() {
        ui.warn("No entity lives there.");
        return;
    }
    if inventory.survival && !inventory.take("ethereal", game_rules().machines.summon_cost) {
        ui.warn(format!("Needs {} Ethereal.", game_rules().machines.summon_cost));
        return;
    }
    let pick = living[(cell.x.unsigned_abs() as usize + extra.requests.pending.len() + inventory.revision as usize) % living.len()].clone();
    ui.action(format!("{pick} summoned."));
    extra.requests.pending.push((pick, cell.as_vec3() + Vec3::new(0.5, 1.05, 2.0)));
    sounds.play_kind(SoundKind::PowerOn);
}

pub fn start_clone(
    cell: IVec3,
    rig: &str,
    grid: &mut EnergyGrid,
    inventory: &mut Inventory,
    extra: &mut CommandExtras,
    ui: &mut EditorUiState,
    sounds: &mut SoundQueue,
) {
    if !extra.bestiary.killed.contains(rig) {
        ui.warn("Only an entity you killed can be cloned.");
        return;
    }
    let ethereal = grid.machine(cell).and_then(|m| m.storage.get("ethereal").copied()).unwrap_or(0);
    if inventory.survival && ethereal == 0 {
        ui.warn("Put Ethereal in the Mnemo-Cloner first (the clone lives on it).");
        return;
    }
    if inventory.survival && !inventory.take("mnemonical", game_rules().machines.clone_cost) {
        ui.warn(format!("Needs {} Mnemonical.", game_rules().machines.clone_cost));
        return;
    }
    extra.requests.clones.push((rig.to_string(), cell.as_vec3() + Vec3::new(0.5, 1.05, 1.8), cell));
    sounds.play_kind(SoundKind::PowerOn);
    ui.action(format!("Mnemo {rig} created."));
}

fn export_path(project: &ProjectLayout, name: &str) -> std::path::PathBuf {
    let stem = crate::voxel::biomes::file_stem(if name.trim().is_empty() { "construction" } else { name });
    project.root.join("exports").join(format!("{stem}.ron"))
}

/// Minimum corner of the Televerseur box (on top of the machine, centred).
fn box_origin(cell: IVec3, size: [i32; 3]) -> IVec3 {
    cell + IVec3::new(-size[0] / 2, 1, -size[2] / 2)
}

pub fn export_box(cell: IVec3, registry: &BlockRegistry, extra: &mut CommandExtras, ui: &mut EditorUiState) {
    let config = extra.configs.get_or_default(cell, BlockFunction::Televerseur).clone();
    let size = config.size.map(|v| v.clamp(1, 64));
    let origin = box_origin(cell, size);
    let mut structure = StructureDef::new(&config.text, size);
    for y in 0..size[1] {
        for z in 0..size[2] {
            for x in 0..size[0] {
                let Some(voxel) = extra.world.get(origin + IVec3::new(x, y, z)) else { continue };
                structure.cells.push(StructureCell {
                    pos: [x, y, z],
                    block: registry.get(voxel.block).id.clone(),
                    rot: voxel.orientation.0,
                    sub: voxel.sub.map(|sub| (registry.get(sub.block).id.clone(), sub.orientation.0)),
                });
            }
        }
    }
    let path = export_path(&extra.project, &config.text);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match save_ron(&path, &structure) {
        Ok(()) => ui.action(format!("{} block(s) saved to {}.", structure.cells.len(), path.display())),
        Err(error) => ui.error(error),
    }
}

pub fn import_box(cell: IVec3, registry: &BlockRegistry, extra: &mut CommandExtras, ui: &mut EditorUiState) {
    let config = extra.configs.get_or_default(cell, BlockFunction::Televerseur).clone();
    let path = export_path(&extra.project, &config.text);
    let structure: StructureDef = match load_ron(&path) {
        Ok(structure) => structure,
        Err(error) => {
            ui.warn(format!("Nothing to load: {error}"));
            return;
        }
    };
    let origin = box_origin(cell, structure.size);
    let mut placed = 0;
    for structure_cell in &structure.cells {
        if let Some(voxel) = StructureDef::voxel(structure_cell, registry) {
            let at = origin + IVec3::from_array(structure_cell.pos);
            extra.world.set(at, Some(voxel));
            extra.world.mark_placed(at);
            placed += 1;
        }
    }
    ui.action(format!("{placed} block(s) loaded from {}.", path.display()));
}

/// Entities standing on top of a machine.
pub(crate) fn on_top(cell: IVec3, position: Vec3, half_height: f32) -> bool {
    let center = cell.as_vec3() + Vec3::new(0.5, 1.0, 0.5);
    let flat = Vec2::new(position.x - center.x, position.z - center.z).length();
    let feet = position.y - half_height;
    flat < 0.9 && feet > center.y - 0.4 && feet < center.y + 0.8
}

#[allow(clippy::too_many_arguments)]
pub fn run_entity_machines(
    time: Res<Time>,
    pie: Res<PieState>,
    configs: Res<MachineConfigs>,
    mut grid: ResMut<EnergyGrid>,
    mut inventory: ResMut<Inventory>,
    mut players: Query<(&mut Player, &Transform), Without<MobInstance>>,
    mut mobs: Query<(Entity, &mut MobInstance, &Transform, Option<&mut MnemoClone>)>,
    mut movables: Query<(&Transform, &mut ExternalImpulse), (With<MovableBody>, Without<Player>, Without<MobInstance>)>,
    mut runtime: ResMut<ScriptRuntime>,
    mut particles: ResMut<ParticleQueue>,
    mut second: Local<f32>,
) {
    if !pie.is_playing() {
        return;
    }
    let delta = time.delta_secs();
    *second += delta;
    let tick = *second >= 1.0;
    if tick {
        *second = 0.0;
    }
    let machines: Vec<(IVec3, BlockFunction, bool)> = grid.machines.iter().map(|(c, m)| (*c, m.function, m.powered)).collect();
    let player = players.single().ok().map(|(_, t)| t.translation);
    let player_half = game_rules().player.half_height + game_rules().player.radius;

    for (cell, function, powered) in machines {
        if !powered || configs.get(cell).is_some_and(|config| !config.enabled) {
            continue;
        }
        let center = cell.as_vec3() + Vec3::splat(0.5);
        let config = configs.get(cell);
        match function {
            BlockFunction::Repulsor => {
                let radius = config.map(|c| c.radius).unwrap_or(5.0).clamp(0.5, 32.0);
                let strength = config.map(|c| c.number).unwrap_or(10.0).clamp(-100.0, 100.0);
                for (_, mut mob, transform, _) in &mut mobs {
                    let to = transform.translation - center;
                    if to.length() < radius {
                        mob.knockback += to.normalize_or_zero() * strength * delta * 3.0;
                    }
                }
                if let Ok((mut body, transform)) = players.single_mut() {
                    let to = transform.translation - center;
                    if to.length() < radius {
                        let push = to.normalize_or_zero() * strength * 0.6;
                        body.knockback = body.knockback.lerp(push, 0.2);
                    }
                }
                for (transform, mut impulse) in &mut movables {
                    let to = transform.translation - center;
                    if to.length() < radius {
                        impulse.impulse += to.normalize_or_zero() * strength * delta * 4.0;
                    }
                }
            }
            BlockFunction::Ringer => {
                let radius = config.map(|c| c.radius).unwrap_or(16.0).clamp(1.0, 64.0);
                let target = config.map(|c| c.target.trim().to_string()).unwrap_or_default();
                for (_, mut mob, transform, _) in &mut mobs {
                    if transform.translation.distance(center) < radius && !mob.tamed && (target.is_empty() || mob.rig.eq_ignore_ascii_case(&target)) {
                        mob.lure = Some((center + Vec3::Y, 0.5));
                    }
                }
            }
            BlockFunction::Ethereaper if tick => {
                let mut gained = 0;
                for (entity, mut mob, transform, _) in &mut mobs {
                    if !on_top(cell, transform.translation, mob.half_height) || mob.is_dead() {
                        continue;
                    }
                    let damage = if mob.ethereal >= 1.0 { mob.health / mob.ethereal.max(1.0) } else { mob.health };
                    mob.ethereal = (mob.ethereal - 1.0).max(0.0);
                    gained += 2;
                    crate::gameplay::combat::hurt_mob(entity, &mut mob, damage.max(0.5), Vec3::ZERO, center, &mut runtime);
                    particles.burst(ParticleBurst::new(transform.translation, Vec3::new(0.8, 0.6, 1.0), 6, ParticleStyle::Spark));
                }
                if gained > 0 {
                    if let Some(machine) = grid.machine_mut(cell) {
                        machine.add("ethereal", gained);
                    }
                }
            }
            BlockFunction::MnemoDigestor if tick => {
                for (entity, mut mob, transform, _) in &mut mobs {
                    if on_top(cell, transform.translation, mob.half_height) && !mob.tamed && !mob.is_dead() {
                        crate::gameplay::combat::hurt_mob(entity, &mut mob, 2.0, Vec3::Y * 1.5, center, &mut runtime);
                        particles.burst(ParticleBurst::new(transform.translation, Vec3::new(0.4, 1.0, 0.6), 6, ParticleStyle::Crumble));
                    }
                }
            }
            BlockFunction::Ressourcer if tick => {
                let distribute = config.is_some_and(|c| c.mode == 1);
                let player_on = player.is_some_and(|p| on_top(cell, p, player_half));
                let Some(machine) = grid.machine_mut(cell) else { continue };
                if distribute {
                    if player_on {
                        if let Some(key) = machine.storage.keys().next().cloned() {
                            let amount = machine.storage.get(&key).copied().unwrap_or(0).min(10);
                            let amount = inventory.room_for(&key, amount);
                            if amount > 0 {
                                machine.take(&key, amount);
                                inventory.add(&key, amount);
                            }
                        }
                    }
                } else {
                    if player_on {
                        let key = inventory
                            .resources
                            .iter()
                            .find(|(key, count)| **count > 0 && crate::gameplay::crafting::is_resource(key) && key.as_str() != "omnicore")
                            .map(|(key, _)| key.clone());
                        if let Some(key) = key {
                            let amount = inventory.count(&key).min(10);
                            if inventory.take(&key, amount) {
                                machine.add(&key, amount);
                            }
                        }
                    }
                    for (_, mut mob, transform, _) in &mut mobs {
                        if on_top(cell, transform.translation, mob.half_height) {
                            for (key, count) in std::mem::take(&mut mob.carried) {
                                machine.add(&key, count);
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    // Mnemo clones live on the Ethereal of their cloner.
    if tick {
        for (_, mut mob, transform, clone) in &mut mobs {
            let Some(mut clone) = clone else { continue };
            if mob.clone_of.is_none() {
                mob.clone_of = Some(clone.cloner);
                mob.tamed = true;
                mob.hostile = false;
                clone.last_health = mob.health;
            }
            let lost = (clone.last_health - mob.health).max(0.0).ceil() as u32;
            clone.last_health = mob.health;
            let paid = grid.machine_mut(clone.cloner).is_some_and(|machine| machine.function == BlockFunction::MnemoCloner && machine.take("ethereal", 1 + lost));
            let near = transform.translation.distance(clone.cloner.as_vec3()) <= game_rules().machines.clone_range;
            if !paid || !near {
                mob.health = 0.0;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entities_on_top_are_detected() {
        let cell = IVec3::new(0, 0, 0);
        assert!(on_top(cell, Vec3::new(0.5, 1.5, 0.5), 0.5));
        assert!(!on_top(cell, Vec3::new(3.0, 1.5, 0.5), 0.5));
        assert!(!on_top(cell, Vec3::new(0.5, 4.0, 0.5), 0.5));
    }
}
