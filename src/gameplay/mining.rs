//! Player ↔ voxel interaction during PIE: aiming, continuous mining with
//! progressive crumbling, harvesting by destruction, placing, interacting and
//! footsteps.
//!
//! - Hold LMB with a block (or empty hand) selected: the targeted block cracks
//!   for `hardness` seconds (crack particles and sounds), then breaks and its
//!   drop falls on the ground as an item to pick up.
//! - RMB with a block selected places it (orientation follows the view).
//! - `E` interacts: sits in a wagon seat, refuels a combustor, or fires the
//!   `On Interact` handler of the block's Blockly program.

use crate::core::game_rules::game_rules;
use bevy::prelude::*;

use crate::audio::dsp_engine::SoundKind;
use crate::audio::sound_events::{surface_tone, SoundQueue, SoundRequest};
use crate::audio::AudioSettings;
use crate::core::app_state::EditorUiState;
use crate::core::editor_camera::EditorCamera;
use crate::core::pie_manager::{PieSet, PieState};
use crate::core::settings::{GameAction, GameInput};
use crate::gameplay::inventory::{drop_for, HotbarItem, Inventory};
use crate::physics::player_controller::Player;
use crate::renderer::particles::{ParticleBurst, ParticleQueue, ParticleStyle};
use crate::scripting::context::{ScriptEvent, ScriptOwner};
use crate::scripting::vm::ScriptRuntime;
use crate::voxel::blocks::{BlockFunction, BlockRegistry};
use crate::voxel::picking::{raycast, try_fuse, VoxelHit};
use crate::voxel::world::{Voxel, VoxelWorld};


/// What the player is looking at this frame.
#[derive(Resource, Debug, Default, Clone, Copy)]
pub struct PlayerAim {
    pub origin: Vec3,
    pub direction: Vec3,
    /// Direction of the aiming ray (camera forward in third person).
    pub ray_direction: Vec3,
    /// Reach of the aiming ray.
    pub reach: f32,
    pub hit: Option<VoxelHit>,
}

/// Progress of the block being mined.
#[derive(Resource, Debug, Default)]
pub struct MiningState {
    pub target: Option<IVec3>,
    pub progress: f32,
    pub crack_timer: f32,
    pub place_cooldown: f32,
    pub step_distance: f32,
}

impl MiningState {
    /// Fraction in `[0, 1]` for the HUD.
    pub fn fraction(&self, registry: &BlockRegistry, world: &VoxelWorld) -> f32 {
        let Some(target) = self.target else {
            return 0.0;
        };
        let Some(voxel) = world.get(target) else {
            return 0.0;
        };
        (self.progress / registry.get(voxel.block).hardness.max(0.05)).clamp(0.0, 1.0)
    }
}


/// Interactions resolved by other systems (wagons, combustors).
#[derive(Resource, Debug, Default)]
pub struct InteractRequests {
    pub cells: Vec<IVec3>,
}

/// Aim, mining, placing and interaction run in this set.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MiningSet;

pub struct MiningPlugin;

impl Plugin for MiningPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PlayerAim>()
            .init_resource::<AimedEntity>()
            .init_resource::<MiningState>()
            .init_resource::<InteractRequests>()
            .add_systems(
                Update,
                (update_aim, aim_entity, mine_and_place, interact, footsteps)
                    .chain()
                    .in_set(MiningSet)
                    .after(PieSet::Transition)
                    .after(crate::physics::player_controller::PlayerControllerSet),
            );
    }
}

fn update_aim(
    pie: Res<PieState>,
    world: Res<VoxelWorld>,
    players: Query<&Player>,
    cameras: Query<&Transform, With<EditorCamera>>,
    mut aim: ResMut<PlayerAim>,
) {
    *aim = PlayerAim::default();
    if !pie.is_playing() {
        return;
    }
    let (Ok(player), Ok(camera)) = (players.single(), cameras.single()) else {
        return;
    };
    let direction = player.look_direction();
    // In third person aim from the camera so the crosshair stays truthful.
    let origin = camera.translation;
    let reach = if player.third_person { game_rules().tools.reach + 4.5 } else { game_rules().tools.reach };
    aim.origin = origin;
    aim.direction = direction;
    let ray_direction = if player.third_person { camera.forward().as_vec3() } else { direction };
    aim.ray_direction = ray_direction;
    aim.reach = reach;
    aim.hit = raycast(&world, origin, ray_direction, reach);
}

/// The creature under the crosshair (closer than the aimed block), for the
/// crosshair info.
#[derive(Resource, Debug, Default, Clone)]
pub struct AimedEntity {
    pub target: Option<AimedMob>,
}

#[derive(Debug, Clone)]
pub struct AimedMob {
    pub name: String,
    pub health: f32,
    pub max_health: f32,
    pub tamed: bool,
    pub hostile: bool,
}

fn aim_entity(
    aim: Res<PlayerAim>,
    mobs: Query<(Entity, &crate::gameplay::mobs::runtime::MobInstance, &Transform)>,
    mut aimed: ResMut<AimedEntity>,
) {
    aimed.target = None;
    if aim.reach <= 0.0 || aim.ray_direction == Vec3::ZERO {
        return;
    }
    // Mobs are seen a bit further than blocks are reached.
    let limit = aim.hit.map(|hit| hit.distance).unwrap_or(aim.reach * 2.0);
    let mut best: Option<(f32, Entity)> = None;
    for (entity, mob, transform) in &mobs {
        let to = transform.translation - aim.origin;
        let along = to.dot(aim.ray_direction);
        if along < 0.0 || along > limit {
            continue;
        }
        let closest = aim.origin + aim.ray_direction * along;
        let radius = mob.half_height.max(0.5);
        if closest.distance(transform.translation) <= radius && best.is_none_or(|(distance, _)| along < distance) {
            best = Some((along, entity));
        }
    }
    if let Some((_, entity)) = best {
        if let Ok((_, mob, _)) = mobs.get(entity) {
            aimed.target = Some(AimedMob {
                name: mob.rig.clone(),
                health: mob.health,
                max_health: mob.max_health,
                tamed: mob.tamed,
                hostile: mob.hostile,
            });
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn mine_and_place(
    time: Res<Time>,
    pie: Res<PieState>,
    input: GameInput,
    aim: Res<PlayerAim>,
    registry: Res<BlockRegistry>,
    mut world: ResMut<VoxelWorld>,
    mut mining: ResMut<MiningState>,
    mut inventory: ResMut<Inventory>,
    mut particles: ResMut<ParticleQueue>,
    mut sounds: ResMut<SoundQueue>,
    mut runtime: ResMut<ScriptRuntime>,
    players: Query<(&Player, &Transform)>,
    mut ui: ResMut<EditorUiState>,
    held: Res<crate::gameplay::items::HeldTool>,
    mut drops: ResMut<crate::gameplay::drops::DropQueue>,
    mut eggs: ResMut<crate::gameplay::mobs::runtime::MobSpawnRequests>,
) {
    let delta = time.delta_secs();
    mining.place_cooldown = (mining.place_cooldown - delta).max(0.0);
    if !pie.is_playing() || !pie.cursor_captured || inventory.spectator {
        mining.target = None;
        mining.progress = 0.0;
        return;
    }
    let item = inventory.current().clone();
    // The cable tool works on machine ports (`data_cables`), not on blocks.
    if held.cable {
        mining.target = None;
        mining.progress = 0.0;
        return;
    }
    // Spawn eggs: RMB on a block summons the mob on top of it.
    if let HotbarItem::SpawnEgg(name) = &item {
        mining.target = None;
        mining.progress = 0.0;
        if input.just_pressed(GameAction::Use) {
            if let Some(hit) = aim.hit {
                let key = format!("egg:{name}");
                if inventory.survival && !inventory.take(&key, 1) {
                    ui.warn(format!("No {name} egg left."));
                    return;
                }
                let at = hit.placement().as_vec3() + Vec3::new(0.5, 0.05, 0.5);
                eggs.pending.push((name.clone(), at));
                sounds.play_kind(SoundKind::BlockPlace);
            }
        }
        return;
    }
    if held.mining_speed <= 0.0 {
        mining.target = None;
        mining.progress = 0.0;
        return;
    }
    let Some(hit) = aim.hit else {
        mining.target = None;
        mining.progress = 0.0;
        return;
    };

    // --- Mining -------------------------------------------------------------
    if input.pressed(GameAction::Attack) {
        if mining.target != Some(hit.voxel) {
            mining.target = Some(hit.voxel);
            mining.progress = 0.0;
            mining.crack_timer = 0.0;
        }
        let Some(voxel) = world.get(hit.voxel) else {
            return;
        };
        let def = registry.get(voxel.block).clone();
        if !inventory.survival {
            // Creative: blocks break instantly, with a short repeat delay.
            if mining.place_cooldown > 0.0 {
                return;
            }
            mining.progress = def.hardness.max(0.05);
            mining.place_cooldown = 0.18;
        }
        // Pickaxe: rock, Axe: floral; other blocks mine at hand speed.
        let speed = match held.mining_target.as_deref() {
            Some(target) if !crate::gameplay::crafting::accepts(target, def.resource_id()) => 1.0,
            _ => held.mining_speed,
        };
        mining.progress += delta * speed;
        mining.crack_timer -= delta;
        let center = hit.voxel.as_vec3() + Vec3::splat(0.5) + hit.normal.as_vec3() * 0.5;
        if mining.crack_timer <= 0.0 {
            mining.crack_timer = 0.16;
            particles.burst(ParticleBurst::new(center, def.color_vec(), 3, ParticleStyle::Crumble));
            sounds.play(
                SoundRequest::sfx(SoundKind::MineCrack(surface_tone(def.material))).with_volume(0.6),
            );
        }
        if mining.progress >= def.hardness.max(0.05) {
            // Remove the fused half first, then the main part.
            let remaining = match voxel.sub {
                Some(_) => Some(Voxel {
                    sub: None,
                    ..voxel
                }),
                None => None,
            };
            let broken = voxel.sub.map(|sub| sub.block).unwrap_or(voxel.block);
            world.set(hit.voxel, remaining);
            let broken_def = registry.get(broken);
            crate::scripting::context::emit_game_event(
                crate::scripting::context::GameEvent::BlockBroken,
                &broken_def.id,
                1.0,
                Some(hit.voxel.as_vec3().to_array()),
            );
            // Creative breaks blocks without dropping anything.
            if let Some(drop) = drop_for(&registry, broken).filter(|_| inventory.survival) {
                drops.spawn(hit.voxel.as_vec3() + Vec3::splat(0.5), &drop, 1);
            }
            // The Atomixer sometimes extracts an Atomic from what it mines.
            if inventory.survival && held.kind == Some(crate::gameplay::items::ItemKind::Atomixer) {
                let roll = (hit.voxel.x.wrapping_mul(73_856_093) ^ hit.voxel.y.wrapping_mul(19_349_663) ^ hit.voxel.z.wrapping_mul(83_492_791))
                    .unsigned_abs()
                    % 100;
                if roll < 12 {
                    drops.spawn(hit.voxel.as_vec3() + Vec3::splat(0.5), "atomic", 1);
                }
            }
            particles.burst(ParticleBurst::new(
                hit.voxel.as_vec3() + Vec3::splat(0.5),
                broken_def.color_vec(),
                18,
                ParticleStyle::Crumble,
            ));
            if !broken_def.break_sound.as_deref().is_some_and(|name| sounds.play_named(name)) {
                sounds.play_kind(SoundKind::BlockBreak(surface_tone(broken_def.material)));
            }
            runtime.despawn_owner(ScriptOwner::Block(hit.voxel.to_array()));
            mining.target = None;
            mining.progress = 0.0;
        }
        return;
    }
    mining.target = None;
    mining.progress = 0.0;

    // --- Placing ------------------------------------------------------------
    if input.pressed(GameAction::Use) && mining.place_cooldown <= 0.0 {
        let HotbarItem::Block(block) = item else {
            return;
        };
        let def = registry.get(block);
        if inventory.survival && !inventory.take(def.resource_id(), 1) {
            ui.warn(format!("No {} left in the inventory.", def.label));
            mining.place_cooldown = 0.4;
            return;
        }
        // Front toward the player (see `BlockPlacement`).
        let orientation = crate::voxel::picking::oriented_toward_viewer(def.placement, hit.normal, aim.direction);

        // Fuse halves first.
        if def.shape.is_half() {
            if let Some(fused) = try_fuse(&world, &registry, hit.voxel, block, orientation) {
                world.set(hit.voxel, Some(fused));
                sounds.play_kind(SoundKind::BlockPlace);
                mining.place_cooldown = 0.2;
                return;
            }
        }

        let target = hit.placement();
        // Never place inside the player capsule.
        if let Ok((_, transform)) = players.single() {
            let feet = transform.translation - Vec3::Y * (game_rules().player.half_height + game_rules().player.radius);
            let head = transform.translation + Vec3::Y * (game_rules().player.half_height + game_rules().player.radius);
            let cell_min = target.as_vec3();
            let cell_max = cell_min + Vec3::ONE;
            let overlap_xz = (transform.translation.x + game_rules().player.radius > cell_min.x)
                && (transform.translation.x - game_rules().player.radius < cell_max.x)
                && (transform.translation.z + game_rules().player.radius > cell_min.z)
                && (transform.translation.z - game_rules().player.radius < cell_max.z);
            if overlap_xz && head.y > cell_min.y && feet.y < cell_max.y && !def.is_liquid() {
                if inventory.survival {
                    inventory.add(def.resource_id(), 1);
                }
                return;
            }
        }
        if world.set(target, Some(Voxel::oriented(block, orientation))) {
            world.mark_placed(target);
            if !def.place_sound.as_deref().is_some_and(|name| sounds.play_named(name)) {
                sounds.play_kind(SoundKind::BlockPlace);
            }
            crate::scripting::context::emit_game_event(
                crate::scripting::context::GameEvent::BlockPlaced,
                &def.id,
                1.0,
                Some(target.as_vec3().to_array()),
            );
            if let Some(script) = &def.script {
                runtime.spawn_instance(script, ScriptOwner::Block(target.to_array()));
            }
        }
        mining.place_cooldown = 0.2;
    }
}

#[allow(clippy::too_many_arguments)]
fn interact(
    pie: Res<PieState>,
    input: GameInput,
    aim: Res<PlayerAim>,
    world: Res<VoxelWorld>,
    registry: Res<BlockRegistry>,
    mut runtime: ResMut<ScriptRuntime>,
    mut requests: ResMut<InteractRequests>,
    mut sounds: ResMut<SoundQueue>,
    inventory: Res<Inventory>,
) {
    requests.cells.clear();
    if !pie.is_playing() || !pie.cursor_captured || !input.just_pressed(GameAction::Interact) || inventory.spectator {
        return;
    }
    let Some(hit) = aim.hit else {
        return;
    };
    let Some(voxel) = world.get(hit.voxel) else {
        return;
    };
    let def = registry.get(voxel.block);
    let owner = ScriptOwner::Block(hit.voxel.to_array());
    let fired = runtime.fire_event(owner, &ScriptEvent::Interact);
    if fired == 0 && def.function == BlockFunction::None {
        sounds.play(SoundRequest::ui(SoundKind::UiClick));
    }
    requests.cells.push(hit.voxel);
}

fn footsteps(
    time: Res<Time>,
    pie: Res<PieState>,
    settings: Res<AudioSettings>,
    world: Res<VoxelWorld>,
    registry: Res<BlockRegistry>,
    players: Query<(&Player, &Transform)>,
    mut mining: ResMut<MiningState>,
    mut sounds: ResMut<SoundQueue>,
) {
    if !pie.is_playing() || !settings.footsteps {
        return;
    }
    let Ok((player, transform)) = players.single() else {
        return;
    };
    if !player.grounded || player.seated.is_some() {
        mining.step_distance = 0.0;
        return;
    }
    mining.step_distance += player.ground_speed * time.delta_secs();
    let stride = if player.ground_speed > 7.0 { 2.4 } else { 1.9 };
    if mining.step_distance < stride {
        return;
    }
    mining.step_distance = 0.0;
    let below = (transform.translation - Vec3::Y * (game_rules().player.half_height + game_rules().player.radius + 0.2))
        .floor()
        .as_ivec3();
    if let Some(voxel) = world.get(below) {
        let material = registry.get(voxel.block).material;
        sounds.play(
            SoundRequest::sfx(SoundKind::Footstep(surface_tone(material)))
                .with_volume(0.45)
                .with_pitch(0.9 + (time.elapsed_secs() * 7.3).sin().abs() * 0.2),
        );
    }
}
