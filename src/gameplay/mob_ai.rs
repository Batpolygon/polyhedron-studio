//! Native behaviours of the spec entities and their natural spawning.
//!
//! A rig whose `behavior` is not `Script` is driven here (its Weave
//! programs still run): the behaviour reads the player, the other entities
//! and the time of day, then asks for a movement (`move_target`), strikes the
//! player ([`MobStrike`]) or hurts other entities directly.
//!
//! **Natural spawning**: every two seconds an entity whose `biomes` contain
//! the biome of a random spot 16 to 40 blocks from the player (and whose
//! `spawn_time` matches the daylight) may appear there; `"*"` means any
//! biome. Naturally spawned entities further than 72 blocks disappear.
//!
//! The [`Bestiary`] remembers the biomes the player discovered and the
//! entities killed (Invocateral, Mnemo-Cloner).

use crate::core::game_rules::game_rules;
use std::collections::HashSet;

use bevy::prelude::*;
use bevy_rapier3d::prelude::KinematicCharacterController;

use crate::audio::dsp_engine::SoundKind;
use crate::audio::sound_events::SoundQueue;
use crate::core::app_state::EditorUiState;
use crate::core::pie_manager::{PieSet, PieState};
use crate::core::settings::{GameAction, GameInput};
use crate::gameplay::combat::{hurt_mob, MobStrike, MobStrikes};
use crate::gameplay::mobs::library::MobLibrary;
use crate::gameplay::mobs::model::{MobBehavior, SpawnTime};
use crate::gameplay::mobs::runtime::{MobInstance, MobSpawnRequests};
use crate::gameplay::player_stats::PlayerStats;
use crate::gameplay::world_rules::WorldRules;
use crate::physics::player_controller::{Player, PlayerIntent};
use crate::renderer::particles::{ParticleBurst, ParticleQueue, ParticleStyle};
use crate::scripting::vm::ScriptRuntime;
use crate::voxel::chunk_manager::WorldStreaming;
use crate::voxel::world::{Voxel, VoxelWorld};


/// Biomes discovered and entities killed by the player.
#[derive(Resource, Debug, Default, Clone)]
pub struct Bestiary {
    pub discovered: HashSet<String>,
    pub killed: HashSet<String>,
}

impl Bestiary {
    /// Biomes the Invocateral can summon from: discovered, and every entity
    /// living there killed at least once.
    pub fn summonable(&self, library: &MobLibrary) -> Vec<String> {
        let mut list: Vec<String> = self
            .discovered
            .iter()
            .filter(|biome| {
                let living: Vec<&str> = library.mob_rigs().filter(|rig| lives_in(&rig.biomes, biome)).map(|rig| rig.name.as_str()).collect();
                !living.is_empty() && living.iter().all(|name| self.killed.contains(*name))
            })
            .cloned()
            .collect();
        list.sort();
        list
    }
}

/// Does a biome list contain `biome` (case-insensitive, `*` = any)?
pub fn lives_in(biomes: &[String], biome: &str) -> bool {
    biomes.iter().any(|entry| entry == "*" || entry.eq_ignore_ascii_case(biome) || biome.to_ascii_lowercase().contains(&entry.to_ascii_lowercase()))
}

/// Entities that died this frame: (rig, position, explodes).
#[derive(Resource, Debug, Default)]
pub struct MobDeaths {
    pub pending: Vec<(String, Vec3, bool)>,
}

/// Laser shots of the Sulfy to draw: (from, to, remaining time).
#[derive(Resource, Debug, Default)]
pub struct MobBeams {
    pub beams: Vec<(Vec3, Vec3, f32)>,
}

/// The rideable entity the player is on.
#[derive(Resource, Debug, Default)]
pub struct Mount {
    pub entity: Option<Entity>,
}

/// Marker: spawned by the natural spawner (despawned when far away).
#[derive(Component)]
pub struct NaturalSpawn;

pub struct MobAiPlugin;

impl Plugin for MobAiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Bestiary>()
            .init_resource::<MobDeaths>()
            .init_resource::<MobBeams>()
            .init_resource::<Mount>()
            .init_resource::<NestEdits>()
            .add_systems(
                Update,
                (discover_biomes, natural_spawning, mob_behaviours, explode_dead, mount_and_ride, draw_mob_beams)
                    .chain()
                    .after(PieSet::Transition)
                    .after(crate::physics::player_controller::PlayerControllerSet),
            );
    }
}

/// Small deterministic random in `[0, 1)`.
fn rand01(seed: u32) -> f32 {
    let mut x = seed.wrapping_mul(0x9E37_79B9) ^ 0x85EB_CA6B;
    x ^= x >> 15;
    x = x.wrapping_mul(0x2C1B_3C6D);
    x ^= x >> 12;
    (x % 10_000) as f32 / 10_000.0
}

fn discover_biomes(
    time: Res<Time>,
    pie: Res<PieState>,
    streaming: Res<WorldStreaming>,
    players: Query<&Transform, With<Player>>,
    mut bestiary: ResMut<Bestiary>,
    mut ui: ResMut<EditorUiState>,
    mut timer: Local<f32>,
) {
    if pie.started_this_frame && pie.persist_world.is_none() {
        *bestiary = Bestiary::default();
    }
    if !pie.is_playing() {
        return;
    }
    *timer -= time.delta_secs();
    if *timer > 0.0 {
        return;
    }
    *timer = 1.0;
    let (Some(generator), Ok(player)) = (streaming.generator.as_ref(), players.single()) else {
        return;
    };
    let p = player.translation;
    let name = generator.biome_name(generator.biome_at(p.x as i32, p.z as i32)).to_string();
    if bestiary.discovered.insert(name.clone()) {
        ui.action(format!("Biome discovered: {name}."));
    }
}

#[allow(clippy::too_many_arguments)]
fn natural_spawning(
    mut commands: Commands,
    time: Res<Time>,
    pie: Res<PieState>,
    rules: Res<WorldRules>,
    world: Res<VoxelWorld>,
    streaming: Res<WorldStreaming>,
    library: Res<MobLibrary>,
    players: Query<&Transform, With<Player>>,
    natural: Query<(Entity, &Transform), (With<NaturalSpawn>, With<MobInstance>)>,
    mut requests: ResMut<MobSpawnRequests>,
    mut timer: Local<f32>,
    mut counter: Local<u32>,
) {
    if !pie.is_playing() {
        return;
    }
    let Ok(player) = players.single() else { return };
    let center = player.translation;
    // Far entities disappear.
    for (entity, transform) in &natural {
        if transform.translation.distance(center) > game_rules().spawns.despawn_distance {
            commands.entity(entity).despawn();
        }
    }
    *timer -= time.delta_secs();
    if *timer > 0.0 {
        return;
    }
    *timer = 2.0;
    if natural.iter().count() >= game_rules().spawns.natural_cap {
        return;
    }
    let Some(generator) = streaming.generator.as_ref() else { return };
    *counter = counter.wrapping_add(1);
    let seed = counter.wrapping_mul(7919) ^ (time.elapsed_secs() * 1000.0) as u32;
    let angle = rand01(seed) * std::f32::consts::TAU;
    let distance = 16.0 + rand01(seed ^ 0xABCD) * 24.0;
    let spot = center + Vec3::new(angle.cos(), 0.0, angle.sin()) * distance;
    let biome = generator.biome_name(generator.biome_at(spot.x as i32, spot.z as i32)).to_string();
    let day = rules.daylight() > 0.3;
    let candidates: Vec<String> = library
        .mob_rigs()
        .filter(|rig| !rig.biomes.is_empty() && lives_in(&rig.biomes, &biome))
        .filter(|rig| match rig.spawn_time {
            SpawnTime::Any => true,
            SpawnTime::Day => day,
            SpawnTime::Night => !day,
        })
        .map(|rig| rig.name.clone())
        .collect();
    if candidates.is_empty() {
        return;
    }
    let pick = candidates[(rand01(seed ^ 0x1234) * candidates.len() as f32) as usize % candidates.len()].clone();
    let Some(hit) = crate::voxel::picking::raycast(&world, spot + Vec3::Y * 48.0, Vec3::NEG_Y, 128.0) else {
        return;
    };
    if world.liquid_at(hit.voxel).is_some() {
        return;
    }
    let feet = hit.voxel.as_vec3() + Vec3::new(0.5, 1.05, 0.5);
    requests.natural.push((pick, feet));
}

/// What one entity decided this frame.
#[derive(Default)]
struct Decision {
    goal: Option<(Vec3, f32)>,
    strike_player: Option<(f32, f32)>,
    strike_mob: Option<(Entity, f32, f32)>,
    repel_player: Option<Vec3>,
    lure_around: Option<Vec3>,
    place: Option<(IVec3, &'static str)>,
    breed: bool,
    beam: Option<(Vec3, Vec3)>,
    face_player: bool,
    leap: Option<Vec3>,
}

#[derive(Clone)]
struct Snap {
    entity: Entity,
    position: Vec3,
    rig: String,
    behavior: MobBehavior,
    half_height: f32,
    hit_by_player: bool,
    angry: bool,
    tamed: bool,
}

#[allow(clippy::too_many_arguments)]
fn mob_behaviours(
    time: Res<Time>,
    pie: Res<PieState>,
    rules: Res<WorldRules>,
    world: Res<VoxelWorld>,
    library: Res<MobLibrary>,
    mut players: Query<(&mut Player, &Transform), Without<MobInstance>>,
    mut mobs: Query<(Entity, &mut MobInstance, &Transform)>,
    mut strikes: ResMut<MobStrikes>,
    mut runtime: ResMut<ScriptRuntime>,
    mut beams: ResMut<MobBeams>,
    mut sounds: ResMut<SoundQueue>,
    mut requests: ResMut<MobSpawnRequests>,
    mut edits: ResMut<NestEdits>,
    mut last_revision: Local<u64>,
    inventory: Res<crate::gameplay::inventory::Inventory>,
) {
    if !pie.is_playing() {
        return;
    }
    let delta = time.delta_secs().min(0.05);
    for beam in &mut beams.beams {
        beam.2 -= delta;
    }
    beams.beams.retain(|beam| beam.2 > 0.0);
    let Ok((mut player, player_transform)) = players.single_mut() else { return };
    // In creative and spectator the player is not a target: as far as the
    // mobs can tell, the player is out of reach (they never chase, strike,
    // repel nor notice hits from them).
    let ignore_player = !inventory.survival || inventory.spectator;
    let player_pos = if ignore_player { player_transform.translation + Vec3::Y * 1.0e6 } else { player_transform.translation };
    let player_forward = player.forward();
    let player_speed = player.ground_speed;
    let day = rules.daylight() > 0.3;
    let noisy_world = world.revision != *last_revision;
    *last_revision = world.revision;

    let snaps: Vec<Snap> = mobs
        .iter()
        .map(|(entity, mob, transform)| Snap {
            entity,
            position: transform.translation,
            rig: mob.rig.clone(),
            behavior: mob.behavior,
            half_height: mob.half_height,
            hit_by_player: mob.hit_flash > 0.1 && mob.last_attacker.is_some_and(|at| at.distance(player_pos) < 3.0),
            angry: mob.angry > 0.0,
            tamed: mob.tamed,
        })
        .collect();
    let nearest = |me: Entity, position: Vec3, radius: f32, filter: &dyn Fn(&Snap) -> bool| -> Option<Snap> {
        snaps
            .iter()
            .filter(|snap| snap.entity != me && snap.position.distance(position) <= radius && filter(snap))
            .min_by(|a, b| a.position.distance(position).total_cmp(&b.position.distance(position)))
            .cloned()
    };

    let mut decisions: Vec<(Entity, Decision)> = Vec::new();
    for (entity, mut mob, transform) in &mut mobs {
        let me = transform.translation;
        mob.angry = (mob.angry - delta).max(0.0);
        mob.fear = (mob.fear - delta).max(0.0);
        mob.timer = (mob.timer - delta).max(0.0);
        if let Some((point, left)) = mob.lure {
            mob.lure = (left > delta).then_some((point, left - delta));
        }
        if mob.ai_stopped || mob.is_dead() {
            continue;
        }
        let mut d = Decision::default();
        if let Some(goal) = mob.forced_target {
            if goal.distance(me) < 0.8 {
                mob.forced_target = None;
            } else {
                d.goal = Some((goal, 3.5));
            }
            decisions.push((entity, d));
            continue;
        }
        if mob.behavior == MobBehavior::Script {
            continue;
        }
        let to_player = player_pos.distance(me);
        let hit_by_player = mob.hit_flash > 0.1 && mob.last_attacker.is_some_and(|at| at.distance(player_pos) < 3.0);
        let reach = 1.9 * mob.reach_scale + mob.half_height * 0.3;

        // Bound entities fight for their owner.
        if mob.tamed {
            if let Some(enemy) = nearest(entity, me, 10.0, &|snap: &Snap| !snap.tamed && (snap.angry || snap.behavior == MobBehavior::PlayerHunter || snap.behavior == MobBehavior::Berserk)) {
                d.goal = Some((enemy.position, 4.0));
                if enemy.position.distance(me) < reach + 0.4 {
                    d.strike_mob = Some((enemy.entity, 2.0, 4.0));
                }
            } else if let Some(owner) = mob.owner.and_then(|owner| snaps.iter().find(|snap| snap.entity == owner)) {
                if owner.position.distance(me) > 3.0 {
                    d.goal = Some((owner.position, 3.5));
                }
            }
            decisions.push((entity, d));
            continue;
        }
        if let Some((point, _)) = mob.lure {
            d.goal = Some((point, 3.5));
        }

        let chase_player = |d: &mut Decision, speed: f32, damage: f32| {
            d.goal = Some((player_pos, speed));
            if to_player < reach + 0.4 {
                d.strike_player = Some((damage, reach + 0.4));
            }
        };
        let flee = |d: &mut Decision, from: Vec3, speed: f32| {
            let away = (me - from).normalize_or_zero();
            d.goal = Some((me + away * 6.0, speed));
        };
        let wander = |d: &mut Decision, mob: &MobInstance, speed: f32| {
            if d.goal.is_some() {
                return;
            }
            if mob.home.distance(me) > 8.0 {
                d.goal = Some((mob.home, speed));
            } else {
                let seed = (entity.to_bits() as u32) ^ (time.elapsed_secs() as u32 / 4);
                if rand01(seed) < 0.5 {
                    let angle = rand01(seed ^ 77) * std::f32::consts::TAU;
                    d.goal = Some((mob.home + Vec3::new(angle.cos(), 0.0, angle.sin()) * 5.0, speed));
                }
            }
        };

        match mob.behavior {
            MobBehavior::Script => {}
            MobBehavior::Brain => {
                use crate::gameplay::mobs::brain::{BrainAction as A, BrainFacts};
                let facts = BrainFacts {
                    to_player,
                    health: mob.health / mob.max_health.max(0.01),
                    hit: hit_by_player,
                    angry: mob.angry > 0.0,
                    afraid: mob.fear > 0.0,
                    day,
                    ready: mob.timer <= 0.0,
                    roll: rand01((entity.to_bits() as u32) ^ (time.elapsed_secs() * 60.0) as u32),
                    delta,
                };
                let allies = |radius: f32| snaps.iter().filter(|snap| snap.entity != entity && snap.rig == mob.rig && snap.position.distance(me) <= radius).count() as u32;
                let rule = library.rigs.iter().find(|rig| rig.name == mob.rig).and_then(|rig| facts.choose(&rig.brain, &allies)).cloned();
                for action in rule.iter().flat_map(|rule| rule.actions.iter()) {
                    match action {
                        A::Wander(speed) => wander(&mut d, &mob, *speed),
                        A::Chase(speed) => d.goal = Some((player_pos, *speed)),
                        A::Flee(speed) => flee(&mut d, player_pos, *speed),
                        A::KeepDistance { distance, speed } => {
                            if to_player < *distance {
                                flee(&mut d, player_pos, *speed);
                            }
                        }
                        A::GoHome(speed) => d.goal = Some((mob.home, *speed)),
                        A::FacePlayer => d.face_player = true,
                        A::Melee(damage) => {
                            if to_player < reach + 0.6 {
                                d.strike_player = Some((*damage, reach + 0.6));
                            }
                        }
                        A::Shoot { damage, range, reload } => {
                            if mob.timer <= 0.0 && to_player <= *range {
                                mob.timer = *reload;
                                d.strike_player = Some((*damage, *range + 1.0));
                                d.beam = Some((me + Vec3::Y * mob.half_height * 0.5, player_pos));
                            }
                        }
                        A::Leap(strength) => {
                            if mob.timer <= 0.0 {
                                mob.timer = 1.5;
                                d.leap = Some((player_pos - me).normalize_or_zero() * *strength + Vec3::Y * 4.0);
                            }
                        }
                        A::CallAllies => d.lure_around = Some(me),
                        A::PlayClip(clip) => {
                            if mob.timer <= 0.0 {
                                mob.timer = 0.5;
                                mob.play(clip, 0.1);
                            }
                        }
                        A::Broadcast(message) => {
                            if mob.timer <= 0.0 {
                                mob.timer = 1.0;
                                crate::scripting::context::emit_game_event(crate::scripting::context::GameEvent::Message, message, 1.0, Some(me.to_array()));
                            }
                        }
                    }
                }
            }
            MobBehavior::Retaliate => {
                let ally_hit = snaps.iter().any(|snap| snap.entity != entity && snap.hit_by_player && snap.position.distance(me) < 12.0);
                if hit_by_player || ally_hit {
                    mob.angry = 10.0;
                }
                if mob.angry > 0.0 {
                    chase_player(&mut d, 3.6, 2.0);
                } else {
                    wander(&mut d, &mob, 1.2);
                }
            }
            MobBehavior::Pack => {
                let pack_on = snaps.iter().any(|snap| snap.entity != entity && snap.rig == mob.rig && snap.angry && snap.position.distance(me) < 14.0);
                if to_player < 14.0 || pack_on || hit_by_player {
                    mob.angry = 4.0;
                    chase_player(&mut d, 3.8, 1.5);
                } else {
                    wander(&mut d, &mob, 1.4);
                }
            }
            MobBehavior::Territorial => {
                if player_pos.distance(mob.home) < 10.0 || (hit_by_player && player_pos.distance(mob.home) < 16.0) {
                    mob.angry = 1.0;
                    chase_player(&mut d, 3.8, 2.0);
                } else {
                    d.goal = Some((mob.home, 2.0));
                }
            }
            MobBehavior::Fearful => {
                let touched = nearest(entity, me, 2.2, &|_: &Snap| true);
                if hit_by_player || to_player < 3.0 || touched.is_some() {
                    mob.fear = 4.0;
                }
                if mob.fear > 1.0 {
                    let from = if to_player < 8.0 { player_pos } else { touched.map(|snap| snap.position).unwrap_or(player_pos) };
                    flee(&mut d, from, 5.0);
                } else if mob.fear > 0.0 {
                    // Hiding: frozen in place.
                } else {
                    wander(&mut d, &mob, 1.0);
                }
            }
            MobBehavior::Tank => {
                if mob.health < mob.max_health * 0.3 && (hit_by_player || to_player < 5.0) {
                    flee(&mut d, player_pos, 3.0);
                } else {
                    wander(&mut d, &mob, 0.8);
                }
            }
            MobBehavior::Social => {
                if hit_by_player {
                    mob.fear = 6.0;
                }
                if mob.fear > 0.0 {
                    flee(&mut d, player_pos, 5.5);
                } else if to_player < 14.0 && to_player > 2.6 {
                    d.goal = Some((player_pos, if player_speed > 0.5 { 2.0 } else { 1.2 }));
                } else {
                    wander(&mut d, &mob, 1.0);
                }
            }
            MobBehavior::Shooter => {
                if to_player < 18.0 {
                    let away = (me - player_pos).normalize_or_zero();
                    d.goal = Some((if to_player < 7.0 { me + away * 4.0 } else { player_pos }, 2.6));
                    d.face_player = true;
                    if mob.timer <= 0.0 {
                        mob.timer = 2.2;
                        d.strike_player = Some((1.5, 19.0));
                        d.beam = Some((me + Vec3::Y * mob.half_height * 0.5, player_pos));
                    }
                } else {
                    wander(&mut d, &mob, 1.2);
                }
            }
            MobBehavior::Ambush => {
                if to_player < 6.5 && mob.timer <= 0.0 {
                    mob.timer = 3.0;
                    let flat = Vec3::new(player_pos.x - me.x, 0.0, player_pos.z - me.z).normalize_or_zero();
                    d.leap = Some(flat * 9.0 + Vec3::Y * 6.0);
                }
                if to_player < reach + 0.6 {
                    d.strike_player = Some((2.0, reach + 0.6));
                }
                d.face_player = to_player < 10.0;
            }
            MobBehavior::Bully => {
                if hit_by_player {
                    mob.angry = 8.0;
                }
                if mob.angry > 0.0 {
                    chase_player(&mut d, 3.6, 2.0);
                } else if let Some(prey) = nearest(entity, me, 12.0, &|snap: &Snap| snap.half_height < 0.45 && snap.behavior != MobBehavior::Bully && !snap.tamed) {
                    d.goal = Some((prey.position, 3.6));
                    if prey.position.distance(me) < reach + 0.3 {
                        d.strike_mob = Some((prey.entity, 2.0, 3.0));
                    }
                } else {
                    wander(&mut d, &mob, 1.2);
                }
            }
            MobBehavior::Nester => {
                if hit_by_player {
                    mob.angry = 6.0;
                }
                if mob.angry > 0.0 {
                    chase_player(&mut d, 3.2, 1.5);
                } else {
                    wander(&mut d, &mob, 1.0);
                    if mob.timer <= 0.0 {
                        mob.timer = 8.0;
                        let carried: u32 = mob.carried.iter().map(|(_, count)| *count).sum();
                        let resource = if carried % 2 == 0 { "mineral" } else { "floral" };
                        match mob.carried.iter_mut().find(|(key, _)| key.as_str() == resource) {
                            Some((_, count)) => *count += 1,
                            None => mob.carried.push((resource.to_string(), 1)),
                        }
                        // Nest: a pure resource block next to its home.
                        if carried % 3 == 2 {
                            let ring = (carried / 3) as i32 % 8;
                            let offsets = [(1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0), (-1, -1), (0, -1), (1, -1)];
                            let (ox, oz) = offsets[ring as usize];
                            let cell = (mob.home + Vec3::new(ox as f32 * 1.5, -mob.half_height, oz as f32 * 1.5)).floor().as_ivec3();
                            d.place = Some((cell, if carried % 2 == 0 { "mineral_ore" } else { "crystal_ore" }));
                        }
                        let kin = snaps.iter().filter(|snap| snap.rig == mob.rig && snap.position.distance(me) < 16.0).count();
                        if carried >= 8 && kin < 4 {
                            d.breed = true;
                            mob.carried.clear();
                        }
                    }
                }
            }
            MobBehavior::HitAndRun => {
                let fears: Vec<String> = library.rigs.iter().find(|rig| rig.name == mob.rig).map(|rig| rig.fears.clone()).unwrap_or_default();
                let scary = nearest(entity, me, 10.0, &|snap: &Snap| fears.contains(&snap.rig));
                if hit_by_player || scary.is_some() {
                    if mob.fear <= 0.0 && to_player < reach + 0.6 {
                        d.strike_player = Some((2.0, reach + 0.6));
                    }
                    mob.fear = 3.0;
                }
                if mob.fear > 0.0 {
                    let from = scary.map(|snap| snap.position).unwrap_or(player_pos);
                    flee(&mut d, from, 4.5);
                } else {
                    wander(&mut d, &mob, 1.2);
                }
            }
            MobBehavior::Noise => {
                let loud = (player_speed > 6.5 && to_player < 18.0) || (noisy_world && to_player < 16.0);
                if loud {
                    mob.angry = 4.0;
                }
                if mob.angry > 0.0 {
                    d.goal = Some((player_pos, 6.0));
                    if to_player < reach + 0.5 && mob.attack_cooldown <= 0.0 {
                        d.strike_player = Some((1.0, reach + 0.5));
                        d.repel_player = Some((player_pos - me).normalize_or_zero());
                        mob.angry = 0.0;
                    }
                } else if player_speed > 0.4 && to_player < 10.0 && to_player > 3.0 {
                    d.goal = Some((player_pos, 1.3));
                } else {
                    wander(&mut d, &mob, 1.0);
                }
            }
            MobBehavior::Repel => {
                if hit_by_player {
                    d.repel_player = Some((player_pos - me).normalize_or_zero());
                }
                wander(&mut d, &mob, 0.9);
            }
            MobBehavior::Backstab => {
                if hit_by_player {
                    mob.fear = 3.0;
                }
                if mob.fear > 0.0 {
                    flee(&mut d, player_pos, 5.0);
                } else if to_player < 16.0 {
                    let behind = player_pos - player_forward * 1.4;
                    let from_behind = (me - player_pos).normalize_or_zero().dot(player_forward) < -0.3;
                    d.goal = Some((behind, 4.2));
                    if from_behind && to_player < reach + 0.5 {
                        if mob.timer <= 0.0 {
                            mob.timer = 1.2;
                            d.leap = Some(Vec3::Y * 4.0 + (player_pos - me).normalize_or_zero() * 2.0);
                        }
                        d.strike_player = Some((2.0, reach + 0.5));
                    }
                } else {
                    wander(&mut d, &mob, 1.2);
                }
            }
            MobBehavior::Stepped => {
                let flat = Vec2::new(player_pos.x - me.x, player_pos.z - me.z).length();
                let stepped_on = flat < mob.half_height.max(0.5) + 0.3 && player_pos.y > me.y + mob.half_height * 0.5;
                if hit_by_player || stepped_on {
                    mob.angry = 8.0;
                }
                if mob.angry > 0.0 {
                    chase_player(&mut d, 3.4, 2.0);
                } else {
                    wander(&mut d, &mob, 0.9);
                }
            }
            MobBehavior::Hunter | MobBehavior::Berserk => {
                let damage = if mob.behavior == MobBehavior::Berserk { 2.5 } else { 2.0 };
                let rig = mob.rig.clone();
                let prey = nearest(entity, me, 16.0, &|snap: &Snap| snap.rig != rig && !snap.tamed);
                match prey {
                    Some(prey) if prey.position.distance(me) < to_player => {
                        d.goal = Some((prey.position, 4.0));
                        if prey.position.distance(me) < reach + 0.3 {
                            d.strike_mob = Some((prey.entity, damage, 3.0));
                        }
                    }
                    _ if to_player < 18.0 => chase_player(&mut d, 4.0, damage),
                    _ => wander(&mut d, &mob, 1.5),
                }
            }
            MobBehavior::Nocturnal => {
                if hit_by_player {
                    mob.angry = 5.0;
                }
                if !day {
                    let rig = mob.rig.clone();
                    let prey = nearest(entity, me, 16.0, &|snap: &Snap| snap.rig != rig);
                    match prey {
                        Some(prey) if prey.position.distance(me) < to_player => {
                            d.goal = Some((prey.position, 4.0));
                            if prey.position.distance(me) < reach + 0.3 {
                                d.strike_mob = Some((prey.entity, 2.5, 3.0));
                            }
                        }
                        _ if to_player < 18.0 => chase_player(&mut d, 4.0, 2.5),
                        _ => wander(&mut d, &mob, 1.5),
                    }
                } else if mob.angry > 0.0 {
                    chase_player(&mut d, 2.0, 2.0);
                    if to_player > 4.0 {
                        mob.angry = 0.0;
                    }
                } else {
                    wander(&mut d, &mob, 0.6);
                }
            }
            MobBehavior::Caller => {
                if mob.timer <= 0.0 && (to_player < 24.0) {
                    mob.timer = 8.0;
                    d.lure_around = Some(me);
                }
                let rig = mob.rig.clone();
                if to_player < reach + 0.4 {
                    d.strike_player = Some((2.0, reach + 0.4));
                } else if let Some(prey) = nearest(entity, me, reach + 0.4, &|snap: &Snap| snap.rig != rig) {
                    d.strike_mob = Some((prey.entity, 2.0, 3.0));
                }
                if to_player < 6.0 {
                    d.goal = Some((player_pos, 3.0));
                }
            }
            MobBehavior::PlayerHunter => {
                if to_player < 22.0 {
                    chase_player(&mut d, 3.8, 2.0);
                } else {
                    wander(&mut d, &mob, 1.2);
                }
            }
            MobBehavior::Watcher => {
                d.face_player = to_player < 24.0;
                mob.knockback = Vec3::ZERO;
            }
        }
        decisions.push((entity, d));
    }

    // Apply.
    for (entity, decision) in decisions {
        let Ok((_, mut mob, transform)) = mobs.get_mut(entity) else { continue };
        let me = transform.translation;
        if let Some(goal) = decision.goal {
            if mob.behavior != MobBehavior::Watcher {
                mob.move_target = Some(goal);
            }
        }
        if decision.face_player {
            let to = player_pos - me;
            mob.yaw = (-to.x).atan2(-to.z);
        }
        if let Some(leap) = decision.leap {
            mob.knockback += leap;
        }
        if let Some((damage, range)) = decision.strike_player.filter(|_| !ignore_player) {
            if mob.attack_cooldown <= 0.0 && mob.stun <= 0.0 {
                mob.attack_cooldown = mob.attack_interval.max(0.6);
                strikes.pending.push(MobStrike { attacker: entity, origin: me, damage: damage * mob.damage_scale, range: range * mob.reach_scale.max(1.0) });
                mob.play("Attack", 0.08);
            }
        }
        if let Some(direction) = decision.repel_player.filter(|_| !ignore_player) {
            player.knockback = Vec3::new(direction.x, 0.0, direction.z).normalize_or_zero() * 14.0 + Vec3::Y * 5.0;
        }
        if let Some(from_to) = decision.beam {
            beams.beams.push((from_to.0, from_to.1, 0.12));
            sounds.play_kind(SoundKind::Laser);
        }
        if let Some((cell, block)) = decision.place {
            edits.pending.push((cell, block.to_string()));
        }
        if decision.breed {
            requests.natural.push((mob.rig.clone(), me + Vec3::new(1.0, 0.2, 0.0)));
        }
        let strike_mob = decision.strike_mob.filter(|_| mob.attack_cooldown <= 0.0 && mob.stun <= 0.0);
        let damage_scale = mob.damage_scale;
        if strike_mob.is_some() {
            mob.attack_cooldown = mob.attack_interval.max(0.7);
            mob.play("Attack", 0.08);
        }
        let lure = decision.lure_around;
        drop(mob);
        if let Some((target, damage, knock)) = strike_mob {
            if let Ok((target_entity, mut target_mob, target_transform)) = mobs.get_mut(target) {
                let push = (target_transform.translation - me).normalize_or_zero() * knock + Vec3::Y * 2.0;
                hurt_mob(target_entity, &mut target_mob, damage * damage_scale, push, me, &mut runtime);
            }
        }
        if let Some(point) = lure {
            sounds.play_kind(SoundKind::UiBeep);
            for (other, mut other_mob, other_transform) in &mut mobs {
                if other != entity && other_transform.translation.distance(point) < 16.0 && !other_mob.tamed {
                    other_mob.lure = Some((point, 4.0));
                }
            }
        }
    }
}

/// Nest blocks placed by the Grom (applied with the world borrowed mutably).
#[derive(Resource, Debug, Default)]
pub struct NestEdits {
    pub pending: Vec<(IVec3, String)>,
}

/// Grom explosions and nest blocks.
#[allow(clippy::too_many_arguments)]
fn explode_dead(
    pie: Res<PieState>,
    mut deaths: ResMut<MobDeaths>,
    mut edits: ResMut<NestEdits>,
    mut world: ResMut<VoxelWorld>,
    registry: Res<crate::voxel::blocks::BlockRegistry>,
    players: Query<&Transform, With<Player>>,
    mut mobs: Query<(Entity, &mut MobInstance, &Transform), Without<Player>>,
    mut stats: ResMut<PlayerStats>,
    mut runtime: ResMut<ScriptRuntime>,
    mut particles: ResMut<ParticleQueue>,
    mut sounds: ResMut<SoundQueue>,
) {
    if !pie.is_playing() {
        deaths.pending.clear();
        edits.pending.clear();
        return;
    }
    for (cell, block) in std::mem::take(&mut edits.pending) {
        if world.get(cell).is_none() {
            if let Some(id) = registry.find(&block) {
                world.set(cell, Some(Voxel::new(id)));
            }
        }
    }
    let player = players.single().ok().map(|t| t.translation);
    for (_, position, explodes) in std::mem::take(&mut deaths.pending) {
        if !explodes {
            continue;
        }
        particles.burst(ParticleBurst::new(position, Vec3::new(1.0, 0.6, 0.2), 36, ParticleStyle::Spark));
        particles.burst(ParticleBurst::new(position, Vec3::new(0.4, 0.4, 0.42), 24, ParticleStyle::Smoke));
        sounds.play_kind(SoundKind::Impact);
        if player.is_some_and(|p| p.distance(position) < 4.0) {
            stats.damage(3.0);
        }
        for (entity, mut mob, transform) in &mut mobs {
            let to = transform.translation - position;
            if to.length() < 4.0 && !mob.is_dead() {
                hurt_mob(entity, &mut mob, 3.0, to.normalize_or_zero() * 6.0 + Vec3::Y * 3.0, position, &mut runtime);
            }
        }
    }
}

/// `E` on a bound rideable entity mounts it; Jump gets off. The player's
/// movement keys drive the mount.
#[allow(clippy::too_many_arguments)]
fn mount_and_ride(
    pie: Res<PieState>,
    input: GameInput,
    intent: Res<PlayerIntent>,
    library: Res<MobLibrary>,
    mut mount: ResMut<Mount>,
    mut players: Query<(&mut Player, &mut Transform, &mut KinematicCharacterController), Without<MobInstance>>,
    mut mobs: Query<(Entity, &mut MobInstance, &Transform), Without<Player>>,
    mut ui: ResMut<EditorUiState>,
) {
    if !pie.is_playing() {
        mount.entity = None;
        return;
    }
    let Ok((mut player, mut transform, mut controller)) = players.single_mut() else { return };
    match mount.entity {
        None => {
            if !pie.cursor_captured || !input.just_pressed(GameAction::Interact) {
                return;
            }
            let eye = transform.translation + Vec3::Y * 0.6;
            let look = player.look_direction();
            let target = mobs
                .iter()
                .filter(|(_, mob, t)| mob.tamed && t.translation.distance(eye) < 3.5 && (t.translation - eye).normalize_or_zero().dot(look) > 0.6)
                .filter(|(_, mob, _)| library.rigs.iter().any(|rig| rig.name == mob.rig && rig.rideable))
                .map(|(entity, _, _)| entity)
                .next();
            if let Some(entity) = target {
                mount.entity = Some(entity);
                ui.action("Mounted. Jump to get off.");
            }
        }
        Some(entity) => {
            let Ok((_, mut mob, mob_transform)) = mobs.get_mut(entity) else {
                mount.entity = None;
                return;
            };
            if intent.jump && pie.cursor_captured {
                mount.entity = None;
                transform.translation += Vec3::Y * 0.8;
                return;
            }
            let forward = player.forward();
            let right = Vec3::new(-forward.z, 0.0, forward.x);
            let wish = (forward * intent.axis.y + right * intent.axis.x).normalize_or_zero();
            if wish != Vec3::ZERO {
                let speed = if intent.sprint { 9.0 } else { 6.0 };
                mob.move_target = Some((mob_transform.translation + wish * 3.0, speed));
            }
            mob.lure = None;
            transform.translation = mob_transform.translation + Vec3::Y * (mob.half_height + 0.9);
            player.velocity = Vec3::ZERO;
            controller.translation = Some(Vec3::ZERO);
        }
    }
}

fn draw_mob_beams(mut gizmos: Gizmos, beams: Res<MobBeams>) {
    for (from, to, _) in &beams.beams {
        gizmos.line(*from, *to, Color::srgb(1.0, 0.85, 0.2));
        gizmos.line(*from + Vec3::Y * 0.03, *to, Color::srgb(1.0, 0.5, 0.1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn biome_lists_match_names_and_wildcards() {
        assert!(lives_in(&["Plains".to_string()], "plains"));
        assert!(lives_in(&["*".to_string()], "Crater"));
        assert!(lives_in(&["Forest".to_string()], "Great Forest"));
        assert!(!lives_in(&["Desert".to_string()], "Swamp"));
    }

    #[test]
    fn random_is_in_range() {
        for seed in 0..100 {
            let value = rand01(seed);
            assert!((0.0..1.0).contains(&value));
        }
    }
}
