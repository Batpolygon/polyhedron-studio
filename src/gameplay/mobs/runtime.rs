//! Mobs in Play-In-Editor.
//!
//! When PIE starts, every rig of the library spawns `spawn_count` copies in a
//! ring around the player. A mob is a Rapier kinematic capsule with one child
//! cube per rig box; its pose is sampled from the current action clip and
//! cross-faded from the previous clip over `blend` seconds (the blend tree),
//! so `Idle -> Walk -> Attack` never pops.
//!
//! The AI is one or more bound Trame programs (rig `script` + `scripts`, "Mob Brain" by default):
//! the VM sees sensors (distance to player, health, allies) and answers with
//! commands (`MoveToward`, `Attack`, `PlayAction`...) applied by
//! [`crate::gameplay::script_bridge`].

use bevy::prelude::*;
use bevy_rapier3d::prelude::*;

use crate::core::app_state::EditorUiState;
use crate::core::pie_manager::{PieOnly, PieSet, PieState};
use crate::gameplay::mobs::library::MobLibrary;
use crate::gameplay::mobs::model::MobRig;
use crate::gameplay::mobs::pose::{drawn_bones, sample_clip, RigPose};
use crate::physics::player_controller::Player;
use crate::scripting::context::{ScriptOwner, ScriptTarget};
use crate::scripting::vm::ScriptRuntime;

const MOB_GRAVITY: f32 = 24.0;

/// Runtime state of a spawned mob.
#[derive(Component, Debug, Clone)]
pub struct MobInstance {
    pub rig: String,
    pub health: f32,
    pub max_health: f32,
    pub home: Vec3,
    pub velocity: Vec3,
    pub yaw: f32,
    pub grounded: bool,
    pub hostile: bool,
    pub tamed: bool,
    pub loot: Option<String>,
    /// Current clip and its local time.
    pub clip: String,
    pub clip_time: f32,
    /// Clip being faded out, its frozen time, and the fade progress.
    pub previous: Option<(String, f32)>,
    pub blend: f32,
    pub blend_duration: f32,
    /// Movement requested by the AI this frame.
    pub move_target: Option<(Vec3, f32)>,
    pub knockback: Vec3,
    pub stun: f32,
    pub hit_flash: f32,
    pub last_attacker: Option<Vec3>,
    /// Sound of a hit, made in the Ambience workspace.
    pub hurt_sound: Option<String>,
    pub attack_cooldown: f32,
    pub half_height: f32,
    /// Stats copied from the rig.
    pub speed_scale: f32,
    pub damage_scale: f32,
    pub attack_interval: f32,
    pub reach_scale: f32,
    pub knockback_resistance: f32,
    /// Native behaviour (`gameplay::mob_ai`).
    pub behavior: crate::gameplay::mobs::model::MobBehavior,
    /// Bound to this entity (`None` + `tamed` = the player).
    pub owner: Option<Entity>,
    /// `/aimoveto`: forced destination.
    pub forced_target: Option<Vec3>,
    /// `/aistop`: the AI does nothing.
    pub ai_stopped: bool,
    /// Resources it carries (dropped on death, Ressourcer, Grom).
    pub carried: Vec<(String, u32)>,
    /// Ethereal left to harvest (Ethereaper).
    pub ethereal: f32,
    /// Angry at the player (hit by it, territory...).
    pub angry: f32,
    /// Behaviour timers (flee, hide, call, shoot...).
    pub timer: f32,
    pub fear: f32,
    /// Mnemo clone made by a Mnemo-Cloner (cell of the cloner).
    pub clone_of: Option<IVec3>,
    /// Attracted toward a point for a while (Clanger call, Ringer).
    pub lure: Option<(Vec3, f32)>,
}

impl MobInstance {
    pub fn owner(entity: Entity) -> ScriptOwner {
        ScriptOwner::Entity(entity.to_bits())
    }

    /// Switch clip with a cross-fade.
    pub fn play(&mut self, clip: &str, blend: f32) {
        if self.clip == clip {
            return;
        }
        self.previous = Some((self.clip.clone(), self.clip_time));
        self.clip = clip.to_string();
        self.clip_time = 0.0;
        self.blend = 0.0;
        self.blend_duration = blend.max(0.0);
    }

    pub fn is_dead(&self) -> bool {
        self.health <= 0.0
    }

    /// Resolve an AI movement target to a world position.
    pub fn resolve_target(&self, target: ScriptTarget, me: Vec3, player: Option<Vec3>) -> Option<Vec3> {
        match target {
            ScriptTarget::Player => player,
            ScriptTarget::Home => Some(self.home),
            ScriptTarget::AwayFromPlayer => player.map(|p| me + (me - p).normalize_or_zero() * 6.0),
            ScriptTarget::Attacker => self.last_attacker.or(player),
        }
    }
}

/// One rig box rendered as a cube child of the mob.
#[derive(Component, Debug)]
pub struct MobBoxVisual {
    pub bone: String,
    pub base_color: Color,
}

#[derive(Resource, Default)]
pub struct MobAssets {
    pub cube: Option<Handle<Mesh>>,
}

/// Mobs to summon this frame (spawn eggs): rig name and feet position.
#[derive(Resource, Debug, Default)]
pub struct MobSpawnRequests {
    pub pending: Vec<(String, Vec3)>,
    /// Natural spawns and births (despawned when far from the player).
    pub natural: Vec<(String, Vec3)>,
    /// Mnemo clones: (rig, feet, cell of the Mnemo-Cloner).
    pub clones: Vec<(String, Vec3, IVec3)>,
}

pub struct MobRuntimePlugin;

impl Plugin for MobRuntimePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MobAssets>()
            .init_resource::<MobSpawnRequests>()
            .add_systems(
                Update,
                (spawn_mobs_on_pie_start, spawn_requested_mobs, move_mobs, animate_mobs, kill_dead_mobs)
                    .chain()
                    .after(PieSet::Transition),
            )
            .add_systems(PostUpdate, read_mob_ground.after(PhysicsSet::Writeback));
    }
}

/// Approximate standing half height of a rig, from its boxes.
pub fn rig_half_height(rig: &MobRig) -> f32 {
    let pose = RigPose::default();
    let mut min_y = f32::MAX;
    let mut max_y = f32::MIN;
    for bone in drawn_bones(rig, &pose) {
        min_y = min_y.min(bone.center.y - bone.size.y * 0.5);
        max_y = max_y.max(bone.center.y + bone.size.y * 0.5);
    }
    if min_y > max_y {
        0.5
    } else {
        ((max_y - min_y) * 0.5).clamp(0.3, 2.5)
    }
}

/// Spawn one mob entity with its visuals and its AI instance.
pub fn spawn_mob(
    commands: &mut Commands,
    rig: &MobRig,
    position: Vec3,
    cube: &Handle<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    runtime: &mut ScriptRuntime,
) -> Entity {
    let half_height = rig_half_height(rig);
    let radius = (half_height * 0.6).clamp(0.25, 0.8);
    let default_clip = if rig.default_clip.is_empty() {
        rig.clips.first().map(|clip| clip.name.clone()).unwrap_or_default()
    } else {
        rig.default_clip.clone()
    };

    let mob = commands
        .spawn((
            Name::new(format!("Mob {}", rig.name)),
            PieOnly,
            MobInstance {
                rig: rig.name.clone(),
                health: rig.health,
                max_health: rig.health,
                home: position,
                velocity: Vec3::ZERO,
                yaw: 0.0,
                grounded: false,
                hostile: rig.hostile,
                tamed: false,
                loot: rig.loot.clone(),
                clip: default_clip,
                clip_time: 0.0,
                previous: None,
                blend: 1.0,
                blend_duration: 0.0,
                move_target: None,
                knockback: Vec3::ZERO,
                stun: 0.0,
                hit_flash: 0.0,
                last_attacker: None,
                hurt_sound: rig.hurt_sound.clone(),
                attack_cooldown: 0.0,
                half_height,
                speed_scale: rig.speed.clamp(0.0, 10.0),
                damage_scale: rig.damage.clamp(0.0, 100.0),
                attack_interval: rig.attack_cooldown.clamp(0.05, 10.0),
                reach_scale: rig.reach.clamp(0.1, 10.0),
                knockback_resistance: rig.knockback_resistance.clamp(0.0, 1.0),
                behavior: rig.behavior,
                owner: None,
                forced_target: None,
                ai_stopped: false,
                carried: Vec::new(),
                ethereal: rig.ethereal.max(0.0),
                angry: 0.0,
                timer: 0.0,
                fear: 0.0,
                clone_of: None,
                lure: None,
            },
            Transform::from_translation(position),
            Visibility::default(),
            RigidBody::KinematicPositionBased,
            Collider::capsule_y((half_height - radius).max(0.05), radius),
            KinematicCharacterController {
                offset: CharacterLength::Absolute(0.03),
                autostep: Some(CharacterAutostep {
                    max_height: CharacterLength::Absolute(0.55),
                    min_width: CharacterLength::Absolute(0.15),
                    include_dynamic_bodies: false,
                }),
                snap_to_ground: Some(CharacterLength::Absolute(0.3)),
                ..default()
            },
        ))
        .id();

    for bone in &rig.boxes {
        if !bone.visible {
            continue;
        }
        let color = Color::srgb(bone.color[0], bone.color[1], bone.color[2]);
        let material = materials.add(StandardMaterial {
            base_color: color,
            emissive: color.to_linear() * bone.emissive,
            perceptual_roughness: 0.7,
            ..default()
        });
        commands.spawn((
            Mesh3d(cube.clone()),
            MeshMaterial3d(material),
            Transform::default(),
            ChildOf(mob),
            MobBoxVisual {
                bone: bone.name.clone(),
                base_color: color,
            },
        ));
    }

    for script in rig.all_scripts() {
        runtime.spawn_instance(&script, MobInstance::owner(mob));
    }
    mob
}

fn spawn_mobs_on_pie_start(
    mut commands: Commands,
    pie: Res<PieState>,
    library: Res<MobLibrary>,
    cameras: Query<&Transform, With<crate::core::editor_camera::EditorCamera>>,
    world: Res<crate::voxel::world::VoxelWorld>,
    mut assets: ResMut<MobAssets>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut runtime: ResMut<ScriptRuntime>,
    mut ui: ResMut<EditorUiState>,
) {
    if !pie.started_this_frame {
        return;
    }
    let cube = assets
        .cube
        .get_or_insert_with(|| meshes.add(Cuboid::new(1.0, 1.0, 1.0)))
        .clone();
    let center = cameras.single().map(|t| t.translation).unwrap_or(Vec3::Y * 10.0);

    let mut spawned = 0;
    let total: u32 = library.mob_rigs().map(|rig| rig.spawn_count.min(16)).sum();
    let mut index = 0u32;
    for rig in library.mob_rigs() {
        for _ in 0..rig.spawn_count.min(16) {
            let angle = index as f32 / total.max(1) as f32 * std::f32::consts::TAU;
            index += 1;
            let around = center + Vec3::new(angle.cos(), 0.0, angle.sin()) * 9.0;
            let ground = crate::voxel::picking::raycast(&world, around + Vec3::Y * 20.0, Vec3::NEG_Y, 128.0)
                .map(|hit| hit.voxel.as_vec3() + Vec3::new(0.5, 1.0, 0.5))
                .unwrap_or(around);
            let position = ground + Vec3::Y * (rig_half_height(rig) + 0.1);
            spawn_mob(&mut commands, rig, position, &cube, &mut materials, &mut runtime);
            spawned += 1;
        }
    }
    if spawned > 0 {
        ui.info(format!("{spawned} mob(s) spawned with their Weave AI."));
    }
}

/// Summon the mobs requested by spawn eggs.
#[allow(clippy::too_many_arguments)]
fn spawn_requested_mobs(
    mut commands: Commands,
    pie: Res<PieState>,
    library: Res<MobLibrary>,
    mut requests: ResMut<MobSpawnRequests>,
    mut assets: ResMut<MobAssets>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut runtime: ResMut<ScriptRuntime>,
) {
    if !pie.is_playing() {
        requests.pending.clear();
        requests.natural.clear();
        requests.clones.clear();
        return;
    }
    if requests.pending.is_empty() && requests.natural.is_empty() && requests.clones.is_empty() {
        return;
    }
    let cube = assets
        .cube
        .get_or_insert_with(|| meshes.add(Cuboid::new(1.0, 1.0, 1.0)))
        .clone();
    for (name, feet) in std::mem::take(&mut requests.pending) {
        let Some(rig) = library.mob_rigs().find(|rig| rig.name == name) else {
            continue;
        };
        let position = feet + Vec3::Y * (rig_half_height(rig) + 0.1);
        spawn_mob(&mut commands, rig, position, &cube, &mut materials, &mut runtime);
    }
    for (name, feet) in std::mem::take(&mut requests.natural) {
        let Some(rig) = library.mob_rigs().find(|rig| rig.name == name) else {
            continue;
        };
        let position = feet + Vec3::Y * (rig_half_height(rig) + 0.1);
        let entity = spawn_mob(&mut commands, rig, position, &cube, &mut materials, &mut runtime);
        commands.entity(entity).insert(crate::gameplay::mob_ai::NaturalSpawn);
    }
    for (name, feet, cloner) in std::mem::take(&mut requests.clones) {
        let Some(rig) = library.mob_rigs().find(|rig| rig.name == name) else {
            continue;
        };
        let position = feet + Vec3::Y * (rig_half_height(rig) + 0.1);
        let entity = spawn_mob(&mut commands, rig, position, &cube, &mut materials, &mut runtime);
        commands.entity(entity).insert(crate::gameplay::machines::advanced::MnemoClone { cloner, last_health: rig.health });
    }
}

fn move_mobs(
    time: Res<Time>,
    pie: Res<PieState>,
    world: Res<crate::voxel::world::VoxelWorld>,
    registry: Res<crate::voxel::blocks::BlockRegistry>,
    players: Query<&Transform, (With<Player>, Without<MobInstance>)>,
    mut mobs: Query<(&mut MobInstance, &mut KinematicCharacterController, &mut Transform)>,
) {
    if !pie.is_playing() {
        return;
    }
    let delta = time.delta_secs().min(0.05);
    let player = players.single().ok().map(|t| t.translation);

    for (mut mob, mut controller, mut transform) in &mut mobs {
        mob.stun = (mob.stun - delta).max(0.0);
        mob.hit_flash = (mob.hit_flash - delta).max(0.0);
        mob.attack_cooldown = (mob.attack_cooldown - delta).max(0.0);

        let mut horizontal = Vec3::ZERO;
        if let Some((target, speed)) = mob.move_target.take() {
            if mob.stun <= 0.0 {
                let mut to = target - transform.translation;
                to.y = 0.0;
                if to.length() > 0.4 {
                    horizontal = to.normalize() * speed * mob.speed_scale;
                    mob.yaw = (-to.x).atan2(-to.z);
                }
            }
        } else if let (Some(player), true) = (player, mob.tamed && mob.owner.is_none()) {
            // Tamed mobs follow the player.
            let mut to = player - transform.translation;
            to.y = 0.0;
            if to.length() > 3.0 {
                horizontal = to.normalize() * 3.5;
                mob.yaw = (-to.x).atan2(-to.z);
            }
        }

        // Knockback: the horizontal part is a decaying extra velocity, the
        // vertical part a one-shot hop (adding it every frame used to stack
        // into a launch of dozens of blocks).
        let resist = 1.0 - mob.knockback_resistance;
        let knock_flat = Vec3::new(mob.knockback.x, 0.0, mob.knockback.z).clamp_length_max(12.0) * resist;
        horizontal += knock_flat;
        let mut vertical = mob.velocity.y;
        if mob.knockback.y > 0.0 {
            vertical = vertical.max(mob.knockback.y.min(6.0) * resist);
            mob.grounded = false;
        }
        mob.knockback = knock_flat * (1.0 - delta * 8.0).max(0.0);
        if mob.knockback.length_squared() < 0.01 {
            mob.knockback = Vec3::ZERO;
        }
        if mob.grounded && vertical < 0.0 {
            vertical = -1.0;
        }
        // Jump over a one-block step: solid block ahead at the feet, free
        // space above it for the whole body.
        if mob.grounded && horizontal.length_squared() > 0.01 && mob.knockback.y <= 0.0 {
            let solid = |point: Vec3| {
                world
                    .get(point.floor().as_ivec3())
                    .is_some_and(|voxel| !registry.get(voxel.block).is_liquid())
            };
            let feet = transform.translation - Vec3::Y * (mob.half_height - 0.05);
            let ahead = horizontal.normalize() * 0.75;
            let step = solid(feet + ahead + Vec3::Y * 0.5);
            let height = (mob.half_height * 2.0).ceil().max(1.0) as i32;
            let clear = (1..=height).all(|level| !solid(feet + ahead + Vec3::Y * (level as f32 + 0.5)))
                && !solid(transform.translation + Vec3::Y * (mob.half_height + 0.6));
            if step && clear {
                // Enough to clear 1.25 block.
                vertical = (2.0 * MOB_GRAVITY * 1.3).sqrt();
                mob.grounded = false;
            }
        }
        vertical -= MOB_GRAVITY * delta;
        vertical = vertical.max(-40.0);
        mob.velocity = Vec3::new(horizontal.x, vertical, horizontal.z);
        controller.translation = Some(mob.velocity * delta);
        transform.rotation = Quat::from_rotation_y(mob.yaw);

        if transform.translation.y < -64.0 {
            transform.translation = mob.home;
            mob.velocity = Vec3::ZERO;
        }
    }
}

fn read_mob_ground(mut mobs: Query<(&mut MobInstance, Option<&KinematicCharacterControllerOutput>)>) {
    for (mut mob, output) in &mut mobs {
        if let Some(output) = output {
            mob.grounded = output.grounded;
            if output.grounded && mob.velocity.y < 0.0 {
                mob.velocity.y = 0.0;
            }
        }
    }
}

/// Blend-tree pose evaluation and visual update.
fn animate_mobs(
    time: Res<Time>,
    library: Res<MobLibrary>,
    mut mobs: Query<(&mut MobInstance, &Children)>,
    mut visuals: Query<(&MobBoxVisual, &mut Transform, &MeshMaterial3d<StandardMaterial>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let delta = time.delta_secs();
    for (mut mob, children) in &mut mobs {
        let Some(rig) = library.rigs.iter().find(|rig| rig.name == mob.rig) else {
            continue;
        };

        // Automatic locomotion clips when the AI did not request one.
        let speed = Vec2::new(mob.velocity.x, mob.velocity.z).length();
        let wants = if speed > 0.5 { "Walk" } else { "Idle" };
        let current_is_locomotion = mob.clip == "Walk" || mob.clip == "Idle" || mob.clip.is_empty();
        if current_is_locomotion && rig.clips.iter().any(|clip| clip.name == wants) {
            mob.play(wants, 0.2);
        }

        mob.clip_time += delta;
        mob.blend = if mob.blend_duration <= 0.0 {
            1.0
        } else {
            (mob.blend + delta / mob.blend_duration).min(1.0)
        };

        let sample = |name: &str, time: f32| -> RigPose {
            match rig.clips.iter().find(|clip| clip.name == name) {
                Some(clip) => {
                    let duration = clip.duration.max(0.01);
                    let local = if clip.looping { time % duration } else { time.min(duration) };
                    sample_clip(rig, clip, local)
                }
                None => RigPose::default(),
            }
        };

        // One-shot clips fall back to locomotion when they end.
        if let Some(clip) = rig.clips.iter().find(|clip| clip.name == mob.clip) {
            if !clip.looping && mob.clip_time > clip.duration + 0.05 {
                mob.play("Idle", 0.15);
            }
        }

        let current = sample(&mob.clip, mob.clip_time);
        let pose = match (&mob.previous, mob.blend < 1.0) {
            (Some((previous, frozen)), true) => sample(previous, *frozen).blend(&current, mob.blend),
            _ => current,
        };
        if mob.blend >= 1.0 {
            mob.previous = None;
        }

        let bones = drawn_bones(rig, &pose);
        // Lift so the rig's lowest point sits at the capsule bottom.
        let lowest = bones
            .iter()
            .map(|bone| bone.center.y - bone.size.y * 0.5)
            .fold(f32::MAX, f32::min);
        let lift = if lowest == f32::MAX { 0.0 } else { -mob.half_height - lowest };

        for child in children.iter() {
            let Ok((visual, mut transform, material)) = visuals.get_mut(child) else {
                continue;
            };
            if let Some(bone) = bones.iter().find(|bone| bone.name == visual.bone) {
                transform.translation = bone.center + Vec3::Y * lift;
                transform.rotation = bone.rotation;
                transform.scale = bone.size;
            }
            if let Some(mut material) = materials.get_mut(&material.0) {
                material.base_color = if mob.hit_flash > 0.0 {
                    Color::srgb(1.0, 0.35, 0.3)
                } else if mob.tamed {
                    visual.base_color.mix(&Color::srgb(0.5, 1.0, 0.6), 0.25)
                } else {
                    visual.base_color
                };
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn kill_dead_mobs(
    mut commands: Commands,
    library: Res<MobLibrary>,
    mut bestiary: ResMut<crate::gameplay::mob_ai::Bestiary>,
    mut deaths: ResMut<crate::gameplay::mob_ai::MobDeaths>,
    mobs: Query<(Entity, &MobInstance, &Transform)>,
    mut runtime: ResMut<ScriptRuntime>,
    mut drops: ResMut<crate::gameplay::drops::DropQueue>,
    mut stats: ResMut<crate::gameplay::player_stats::PlayerStats>,
    mut particles: ResMut<crate::renderer::particles::ParticleQueue>,
    mut sounds: ResMut<crate::audio::sound_events::SoundQueue>,
    mut ui: ResMut<EditorUiState>,
) {
    for (entity, mob, transform) in &mobs {
        if !mob.is_dead() {
            continue;
        }
        runtime.despawn_owner(MobInstance::owner(entity));
        commands.entity(entity).despawn();
        {
            use crate::scripting::context::{emit_game_event, GameEvent};
            let at = Some(transform.translation.to_array());
            emit_game_event(GameEvent::MobDied, &mob.rig, 1.0, at);
            emit_game_event(GameEvent::MobKilled, &mob.rig, 1.0, at);
        }
        particles.burst(crate::renderer::particles::ParticleBurst::new(
            transform.translation,
            Vec3::new(0.9, 0.3, 0.3),
            24,
            crate::renderer::particles::ParticleStyle::Crumble,
        ));
        let death_sound = library.rigs.iter().find(|rig| rig.name == mob.rig).and_then(|rig| rig.death_sound.as_deref());
        if !death_sound.is_some_and(|name| sounds.play_named(name)) {
            sounds.play_kind(crate::audio::dsp_engine::SoundKind::Impact);
        }
        stats.kills += 1;
        // A Mnemo clone dissolves without loot.
        if mob.clone_of.is_none() {
            let loot = mob.loot.clone().unwrap_or_else(|| "faunal".to_string());
            drops.spawn(transform.translation, &loot, 1);
            if let Some(rig) = library.rigs.iter().find(|rig| rig.name == mob.rig) {
                for (resource, count) in &rig.drops {
                    drops.spawn(transform.translation, resource, *count);
                }
            }
            for (resource, count) in &mob.carried {
                drops.spawn(transform.translation, resource, *count);
            }
            bestiary.killed.insert(mob.rig.clone());
        }
        deaths.pending.push((mob.rig.clone(), transform.translation, library.rigs.iter().any(|rig| rig.name == mob.rig && rig.explodes)));
        ui.action(format!("{} defeated.", mob.rig));
    }
}
