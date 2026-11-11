//! Applies Blockly world commands and feeds sensors back to the VM.
//!
//! - Binds bound programs to their owners: blocks whose definition names a
//!   `script`, and mobs (rig `script`). Binding is idempotent and refreshed
//!   every second, so instances survive a VM restart after a library edit.
//! - Pushes sensor snapshots (distance to player, health, power, time of day,
//!   allies) into every instance before the VM ticks.
//! - Drains [`ScriptCommand`]s and applies them to the voxel world, mobs,
//!   inventory, particles, audio, cables and world rules. World mutations are
//!   only applied during PIE, so running scripts in the editor never edits
//!   the authored level by accident.

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;

use crate::audio::sound_events::SoundQueue;
use crate::core::app_state::EditorUiState;
use crate::core::pie_manager::{PieSet, PieState};
use crate::core::project_io::{load_ron, save_ron, ProjectLayout};
use crate::gameplay::chat::{Chat, ChatKind};
use crate::gameplay::combat::{MobStrike, MobStrikes};
use crate::gameplay::data_cables::SignalRequests;
use crate::gameplay::energy_grid::EnergyGrid;
use crate::gameplay::inventory::Inventory;
use crate::gameplay::mob_ai::MobBeams;
use crate::gameplay::mobs::library::MobLibrary;
use crate::gameplay::mobs::runtime::{spawn_mob, MobAssets, MobInstance};
use crate::gameplay::player_stats::PlayerStats;
use crate::gameplay::world_rules::WorldRules;
use crate::physics::player_controller::Player;
use crate::renderer::particles::{ParticleBurst, ParticleQueue, ParticleStyle};
use crate::scripting::context::{drain_game_events, PlayerStat, Question, ScriptOwner, SensorValues, WorldCommand};
use crate::scripting::vm::ScriptRuntime;
use crate::voxel::blocks::BlockRegistry;
use crate::voxel::shapes::Orientation;
use crate::voxel::world::{Voxel, VoxelWorld, VoxelChunk, CHUNK_VOLUME};

/// Marker: the mob's AI instance has been created.
#[derive(Component)]
pub struct ScriptBound;

#[derive(Resource, Debug, Default)]
pub struct ScriptBridgeState {
    /// Cells of blocks that reference a program.
    pub scripted_blocks: Vec<(IVec3, String)>,
    scanned_revision: Option<u64>,
    rebind_timer: f32,
    pub applied: u64,
}

/// Values kept by `Save value`, across runs (`project/saved_values.ron`).
#[derive(Resource, Debug, Default)]
pub struct SavedValues {
    values: std::collections::BTreeMap<String, f32>,
    loaded: bool,
}

impl SavedValues {
    const FILE: &'static str = "saved_values.ron";

    fn ensure_loaded(&mut self, project: &ProjectLayout) {
        if !self.loaded {
            self.loaded = true;
            self.values = load_ron(&project.root.join(Self::FILE)).unwrap_or_default();
        }
    }

    fn get(&mut self, project: &ProjectLayout, key: &str) -> f32 {
        self.ensure_loaded(project);
        self.values.get(key).copied().unwrap_or(0.0)
    }

    fn set(&mut self, project: &ProjectLayout, key: String, value: f32) -> Result<(), String> {
        self.ensure_loaded(project);
        self.values.insert(key, value);
        save_ron(&project.root.join(Self::FILE), &self.values)
    }
}

pub struct ScriptBridgePlugin;

impl Plugin for ScriptBridgePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ScriptBridgeState>().init_resource::<SavedValues>().add_systems(
            Update,
            (bind_instances, dispatch_game_events, push_sensors, apply_commands)
                .chain()
                .after(PieSet::Transition)
                .after(crate::scripting::ScriptVmSet),
        );
    }
}

fn scan_scripted_blocks(world: &VoxelWorld, registry: &BlockRegistry) -> Vec<(IVec3, String)> {
    let mut found = Vec::new();
    for chunk in world.chunks() {
        let origin = chunk.origin();
        for index in 0..CHUNK_VOLUME {
            let local = VoxelChunk::local_from_index(index);
            let Some(voxel) = chunk.get(local) else {
                continue;
            };
            if let Some(script) = &registry.get(voxel.block).script {
                found.push((origin + local.as_ivec3(), script.clone()));
                if found.len() >= 256 {
                    return found;
                }
            }
        }
    }
    found
}

#[allow(clippy::too_many_arguments)]
fn bind_instances(
    mut commands: Commands,
    time: Res<Time>,
    pie: Res<PieState>,
    world: Res<VoxelWorld>,
    registry: Res<BlockRegistry>,
    library: Res<MobLibrary>,
    mut state: ResMut<ScriptBridgeState>,
    mut runtime: ResMut<ScriptRuntime>,
    unbound: Query<(Entity, &MobInstance), Without<ScriptBound>>,
) {
    if pie.started_this_frame || pie.stopped_this_frame {
        *state = ScriptBridgeState::default();
    }
    if !pie.is_playing() || !runtime.is_running() {
        return;
    }
    if state.scanned_revision != Some(world.revision) {
        state.scripted_blocks = scan_scripted_blocks(&world, &registry);
        state.scanned_revision = Some(world.revision);
    }

    for (entity, mob) in &unbound {
        let scripts = library
            .rigs
            .iter()
            .find(|rig| rig.name == mob.rig)
            .map(|rig| rig.all_scripts())
            .unwrap_or_default();
        let mut bound = true;
        for script in &scripts {
            bound &= runtime.spawn_instance(script, MobInstance::owner(entity));
        }
        if bound {
            commands.entity(entity).insert(ScriptBound);
        }
    }

    state.rebind_timer -= time.delta_secs();
    if state.rebind_timer <= 0.0 {
        state.rebind_timer = 1.0;
        for (cell, script) in state.scripted_blocks.clone() {
            runtime.spawn_instance(&script, ScriptOwner::Block(cell.to_array()));
        }
    }
}

/// Hands what happened in the game to the programs listening to it.
fn dispatch_game_events(pie: Res<PieState>, mut runtime: ResMut<ScriptRuntime>, mut quests: ResMut<crate::gameplay::quests::QuestLog>) {
    let events = drain_game_events();
    if !pie.is_playing() {
        return;
    }
    let story = crate::gameplay::quests::story();
    for event in &events {
        runtime.fire_game_event(event);
        quests.on_event(event, &story);
    }
}

fn push_sensors(
    pie: Res<PieState>,
    rules: Res<WorldRules>,
    stats: Res<PlayerStats>,
    grid: Res<EnergyGrid>,
    state: Res<ScriptBridgeState>,
    players: Query<&Transform, With<Player>>,
    mobs: Query<(Entity, &MobInstance, &Transform)>,
    mut runtime: ResMut<ScriptRuntime>,
    inventory: Res<crate::gameplay::inventory::Inventory>,
) {
    if !runtime.is_running() {
        return;
    }
    let player = players.single().ok().map(|t| t.translation);
    // Scripted mobs do not see the player in creative or spectator.
    let player_for_mobs = player.filter(|_| inventory.survival && !inventory.spectator);
    let base = SensorValues {
        time_of_day: rules.time_of_day,
        player_health: stats.health,
        player_stamina: stats.stamina,
        player_position: player.unwrap_or(Vec3::ZERO).to_array(),
        ..Default::default()
    };
    runtime.set_global_sensors(base);
    if !pie.is_playing() {
        return;
    }

    let positions: Vec<Vec3> = mobs.iter().map(|(_, _, t)| t.translation).collect();
    for (entity, mob, transform) in &mobs {
        let allies = positions
            .iter()
            .filter(|p| p.distance(transform.translation) < 8.0)
            .count()
            .saturating_sub(1);
        runtime.set_sensors(
            MobInstance::owner(entity),
            SensorValues {
                distance_to_player: player_for_mobs
                    .map(|p| p.distance(transform.translation))
                    .unwrap_or(f32::MAX),
                health: mob.health,
                powered: grid.is_powered_at(transform.translation),
                nearby_allies: allies as f32,
                position: transform.translation.to_array(),
                ..base
            },
        );
    }
    for (cell, _) in &state.scripted_blocks {
        let center = cell.as_vec3() + Vec3::splat(0.5);
        runtime.set_sensors(
            ScriptOwner::Block(cell.to_array()),
            SensorValues {
                distance_to_player: player.map(|p| p.distance(center)).unwrap_or(f32::MAX),
                powered: grid
                    .machine(*cell)
                    .map(|machine| machine.powered)
                    .unwrap_or_else(|| grid.is_powered_at(center)),
                health: 1.0,
                position: center.to_array(),
                ..base
            },
        );
    }
}

/// Everything the command executor can touch.
#[derive(SystemParam)]
struct CommandTargets<'w, 's> {
    commands: Commands<'w, 's>,
    world: ResMut<'w, VoxelWorld>,
    registry: Res<'w, BlockRegistry>,
    inventory: ResMut<'w, Inventory>,
    particles: ResMut<'w, ParticleQueue>,
    sounds: ResMut<'w, SoundQueue>,
    signals: ResMut<'w, SignalRequests>,
    strikes: ResMut<'w, MobStrikes>,
    rules: ResMut<'w, WorldRules>,
    library: Res<'w, MobLibrary>,
    mob_assets: ResMut<'w, MobAssets>,
    meshes: ResMut<'w, Assets<Mesh>>,
    materials: ResMut<'w, Assets<StandardMaterial>>,
    ui: ResMut<'w, EditorUiState>,
    stats: ResMut<'w, PlayerStats>,
    chat: ResMut<'w, Chat>,
    project: Res<'w, ProjectLayout>,
    beams: ResMut<'w, MobBeams>,
    saved: ResMut<'w, SavedValues>,
    screens: ResMut<'w, crate::ui::OpenScreens>,
    weather: ResMut<'w, crate::gameplay::atmosphere::WeatherState>,
    quests: ResMut<'w, crate::gameplay::quests::QuestLog>,
}

fn apply_commands(
    pie: Res<PieState>,
    mut runtime: ResMut<ScriptRuntime>,
    mut state: ResMut<ScriptBridgeState>,
    mut targets: CommandTargets,
    mut players: Query<&mut Transform, (With<Player>, Without<MobInstance>)>,
    mut mobs: Query<(&mut MobInstance, &Transform)>,
) {
    let commands = runtime.drain_commands();
    if commands.is_empty() {
        return;
    }
    let playing = pie.is_playing();
    let player = players.single().ok().map(|t| t.translation);

    for command in commands {
        let entity = match command.owner {
            ScriptOwner::Entity(bits) => Entity::try_from_bits(bits),
            _ => None,
        };
        let origin = match command.owner {
            ScriptOwner::Global => Vec3::ZERO,
            ScriptOwner::Block(cell) => IVec3::from_array(cell).as_vec3() + Vec3::splat(0.5),
            ScriptOwner::Entity(_) => entity
                .and_then(|entity| mobs.get(entity).ok())
                .map(|(_, transform)| transform.translation)
                .unwrap_or(Vec3::ZERO),
        };
        let origin_cell = origin.floor().as_ivec3();
        state.applied += 1;

        match command.command {
            WorldCommand::PlaySound { sound } => {
                if !targets.sounds.play_named(&sound) {
                    targets.ui.warn(format!("[{}] unknown sound '{sound}'.", command.program));
                }
            }
            WorldCommand::SpawnParticle { kind, count } if crate::gameplay::atmosphere::atmosphere().particles.iter().any(|def| def.name == kind) => {
                let data = crate::gameplay::atmosphere::atmosphere();
                if let Some(def) = data.particles.iter().find(|def| def.name == kind) {
                    let mut burst = def.burst(origin + Vec3::Y * 0.5);
                    burst.count = count;
                    targets.particles.burst(burst);
                }
            }
            WorldCommand::SpawnParticle { kind, count } => {
                let style = ParticleStyle::from_name(&kind);
                let color = match style {
                    ParticleStyle::Spark => Vec3::new(1.0, 0.85, 0.4),
                    ParticleStyle::Smoke => Vec3::new(0.5, 0.5, 0.55),
                    ParticleStyle::Crumble => Vec3::new(0.6, 0.5, 0.4),
                };
                targets
                    .particles
                    .burst(ParticleBurst::new(origin + Vec3::Y * 0.5, color, count, style));
            }
            _ if !playing => {
                // World mutations are PIE-only.
            }
            WorldCommand::SetBlock { offset, block, rot } => match targets.registry.find(&block) {
                Some(id) => {
                    targets.world.set(
                        origin_cell + IVec3::from_array(offset),
                        Some(Voxel::oriented(id, Orientation(rot % 24))),
                    );
                }
                None => targets.ui.warn(format!("[{}] unknown block '{block}'.", command.program)),
            },
            WorldCommand::BreakBlock { offset, drop } => {
                let cell = origin_cell + IVec3::from_array(offset);
                if let Some(voxel) = targets.world.get(cell) {
                    targets.world.set(cell, None);
                    if drop {
                        if let Some(resource) =
                            crate::gameplay::inventory::drop_for(&targets.registry, voxel.block)
                        {
                            targets.inventory.add(&resource, 1);
                        }
                    }
                    let color = targets.registry.get(voxel.block).color_vec();
                    targets.particles.burst(ParticleBurst::new(
                        cell.as_vec3() + Vec3::splat(0.5),
                        color,
                        12,
                        ParticleStyle::Crumble,
                    ));
                }
            }
            WorldCommand::DropResource { resource, amount } => {
                targets.inventory.add(&resource, amount);
            }
            WorldCommand::SetSignal { port, value } => {
                targets.signals.pending.push((origin_cell, port, value));
            }
            WorldCommand::SetTimeOfDay { value } => {
                targets.rules.time_of_day = value.rem_euclid(1.0);
                targets.rules.scripted = true;
            }
            WorldCommand::Ask { name, question } => {
                let answer = |runtime: &mut ScriptRuntime, name: &str, value: f32| {
                    runtime.set_variable(command.owner, &command.program, name, value);
                };
                let value = match question {
                    Question::ItemCount { key } => targets.inventory.count(&key) as f32,
                    Question::Stat { stat } => read_stat(&targets.stats, stat),
                    Question::Saved { key } => targets.saved.get(&targets.project, &key),
                    Question::MobCount { rig } => {
                        mobs.iter().filter(|(mob, _)| rig.is_empty() || mob.rig == rig).count() as f32
                    }
                    Question::BlockAt { offset, block } => {
                        let found = targets.world.get(origin_cell + IVec3::from_array(offset));
                        let matches = found.is_some_and(|voxel| block.is_empty() || targets.registry.get(voxel.block).id == block);
                        if matches { 1.0 } else { 0.0 }
                    }
                    Question::NearestMob { rig } => {
                        let nearest = mobs
                            .iter()
                            .filter(|(mob, transform)| {
                                (rig.is_empty() || mob.rig == rig) && transform.translation.distance_squared(origin) > 0.001
                            })
                            .map(|(_, transform)| transform.translation)
                            .min_by(|a, b| a.distance_squared(origin).total_cmp(&b.distance_squared(origin)));
                        let position = nearest.unwrap_or(origin);
                        answer(&mut runtime, &format!("{name}_x"), position.x);
                        answer(&mut runtime, &format!("{name}_y"), position.y);
                        answer(&mut runtime, &format!("{name}_z"), position.z);
                        nearest.map_or(1_000_000.0, |position| position.distance(origin))
                    }
                };
                answer(&mut runtime, &name, value);
            }
            WorldCommand::GiveItem { key, amount } => targets.inventory.add(&key, amount),
            WorldCommand::TakeItem { key, amount, result } => {
                let paid = targets.inventory.count(&key) >= amount && targets.inventory.take(&key, amount);
                runtime.set_variable(command.owner, &command.program, &result, if paid { 1.0 } else { 0.0 });
            }
            WorldCommand::SetPlayerStat { stat, value, add } => {
                let stats = &mut *targets.stats;
                let next = if add { read_stat(stats, stat) + value } else { value };
                match stat {
                    PlayerStat::Health => stats.health = next.clamp(0.0, stats.max_health),
                    PlayerStat::MaxHealth => {
                        stats.max_health = next.max(1.0);
                        stats.health = stats.health.min(stats.max_health);
                    }
                    PlayerStat::Stamina => stats.stamina = next.clamp(0.0, stats.max_stamina),
                    PlayerStat::MaxStamina => {
                        stats.max_stamina = next.max(0.0);
                        stats.stamina = stats.stamina.min(stats.max_stamina);
                    }
                    PlayerStat::Kills => stats.kills = next.max(0.0) as u32,
                    PlayerStat::Deaths => stats.deaths = next.max(0.0) as u32,
                }
            }
            WorldCommand::DamagePlayer { amount } => {
                targets.stats.damage(amount);
            }
            WorldCommand::TeleportPlayer { position } => {
                if let Ok(mut transform) = players.single_mut() {
                    transform.translation = Vec3::from_array(position);
                }
            }
            WorldCommand::ShowMessage { text } => targets.chat.post(ChatKind::System, text),
            WorldCommand::SaveValue { key, value } => {
                if let Err(error) = targets.saved.set(&targets.project, key, value) {
                    targets.ui.warn(error);
                }
            }
            WorldCommand::StartQuest { quest } => {
                targets.quests.start(&quest, &crate::gameplay::quests::story());
            }
            WorldCommand::ShowDialogue { dialogue } => targets.quests.dialogue = Some((dialogue, 0)),
            WorldCommand::SetWeather { weather } => {
                targets.weather.force(&weather, &crate::gameplay::atmosphere::atmosphere());
            }
            WorldCommand::OpenScreen { screen } => targets.screens.open(screen),
            WorldCommand::CloseScreen { screen } => targets.screens.close(&screen),
            WorldCommand::Shoot { damage, range } => {
                let Some(goal) = player.filter(|goal| goal.distance(origin) <= range) else {
                    continue;
                };
                let mob = entity.and_then(|entity| mobs.get(entity).ok()).map(|(mob, _)| mob);
                if mob.is_some_and(|mob| mob.tamed || !mob.hostile || mob.stun > 0.0) {
                    continue;
                }
                let lift = mob.map_or(0.5, |mob| mob.half_height * 0.5);
                let damage = damage * mob.map_or(1.0, |mob| mob.damage_scale);
                targets.beams.beams.push((origin + Vec3::Y * lift, goal, 0.12));
                match entity {
                    Some(attacker) => targets.strikes.pending.push(MobStrike { attacker, origin, damage, range }),
                    None => {
                        if targets.inventory.survival && !targets.inventory.spectator {
                            targets.stats.damage(damage);
                        }
                    }
                }
            }
            WorldCommand::SpawnMob { mob } => {
                let Some(rig) = targets.library.mob_rigs().find(|rig| rig.name == mob).cloned() else {
                    targets.ui.warn(format!("[{}] unknown mob '{mob}'.", command.program));
                    continue;
                };
                let cube = {
                    let meshes = &mut targets.meshes;
                    targets
                        .mob_assets
                        .cube
                        .get_or_insert_with(|| meshes.add(Cuboid::new(1.0, 1.0, 1.0)))
                        .clone()
                };
                spawn_mob(
                    &mut targets.commands,
                    &rig,
                    origin + Vec3::new(0.0, 1.5, 0.0),
                    &cube,
                    &mut targets.materials,
                    &mut runtime,
                );
            }
            other => {
                // Entity-only commands.
                let Some(entity) = entity else {
                    continue;
                };
                let Ok((mut mob, transform)) = mobs.get_mut(entity) else {
                    continue;
                };
                match other {
                    WorldCommand::MoveToward { target, speed } => {
                        if let Some(goal) = mob.resolve_target(target, transform.translation, player) {
                            mob.move_target = Some((goal, speed));
                        }
                    }
                    WorldCommand::ApplyImpulse { impulse } => {
                        mob.knockback += Vec3::from_array(impulse);
                    }
                    WorldCommand::MoveTo { position, speed } => {
                        mob.move_target = Some((Vec3::from_array(position), speed));
                    }
                    WorldCommand::PlayAction { action, blend } => mob.play(&action, blend),
                    WorldCommand::SetHealth { amount } => {
                        mob.health = amount;
                        mob.max_health = mob.max_health.max(amount);
                    }
                    WorldCommand::Attack { damage, range } => {
                        if mob.hostile && !mob.tamed && mob.attack_cooldown <= 0.0 && mob.stun <= 0.0 {
                            mob.attack_cooldown = mob.attack_interval;
                            targets.strikes.pending.push(MobStrike {
                                attacker: entity,
                                origin: transform.translation,
                                damage: damage * mob.damage_scale,
                                range: range * mob.reach_scale,
                            });
                        }
                    }
                    WorldCommand::TameEntity => {
                        if !mob.tamed {
                            mob.tamed = true;
                            mob.hostile = false;
                            targets.ui.action(format!("{} tamed.", mob.rig));
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

fn read_stat(stats: &PlayerStats, stat: PlayerStat) -> f32 {
    match stat {
        PlayerStat::Health => stats.health,
        PlayerStat::MaxHealth => stats.max_health,
        PlayerStat::Stamina => stats.stamina,
        PlayerStat::MaxStamina => stats.max_stamina,
        PlayerStat::Kills => stats.kills as f32,
        PlayerStat::Deaths => stats.deaths as f32,
    }
}
