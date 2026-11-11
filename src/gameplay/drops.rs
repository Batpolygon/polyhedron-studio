//! Dropped items: broken blocks and mob loot fall on the ground as small
//! spinning items, then fly to the player when they get close and land in
//! the inventory (survival also puts new blocks / items in the hotbar).

use crate::core::game_rules::game_rules;
use bevy::prelude::*;

use crate::audio::dsp_engine::SoundKind;
use crate::audio::sound_events::{SoundQueue, SoundRequest};
use crate::core::app_state::EditorUiState;
use crate::core::pie_manager::{PieOnly, PieSet, PieState};
use crate::gameplay::inventory::{HotbarItem, Inventory};
use crate::gameplay::items::{item_mesh, ItemLibrary};
use crate::physics::player_controller::Player;
use crate::renderer::poly_material::PolyVoxelMaterial;
use crate::renderer::voxel_render::{material_handle, mesh_from_data, VoxelRenderState};
use crate::voxel::blocks::BlockRegistry;
use crate::voxel::mesher;
use crate::voxel::world::{Voxel, VoxelWorld};


/// Drops requested by gameplay this frame.
#[derive(Resource, Debug, Default)]
pub struct DropQueue {
    pending: Vec<(Vec3, String, u32, Option<Vec3>)>,
}

impl DropQueue {
    pub fn spawn(&mut self, position: Vec3, resource: &str, count: u32) {
        if count > 0 && !resource.is_empty() {
            self.pending.push((position, resource.to_string(), count, None));
        }
    }

    /// Throw an item from `position` with `velocity` (the player's drop key):
    /// it is not picked up again before [`THROW_PICKUP_DELAY`].
    pub fn throw(&mut self, position: Vec3, velocity: Vec3, resource: &str, count: u32) {
        if count > 0 && !resource.is_empty() {
            self.pending.push((position, resource.to_string(), count, Some(velocity)));
        }
    }
}


#[derive(Component, Debug)]
pub struct ItemDrop {
    pub resource: String,
    pub count: u32,
    velocity: Vec3,
    age: f32,
    resting: bool,
}

pub struct DropPlugin;

impl Plugin for DropPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DropQueue>().add_systems(
            Update,
            (throw_items, spawn_drops, simulate_drops).chain().after(PieSet::Transition),
        );
    }
}

/// The drop key throws one unit of the held entry (the whole stack with
/// Sprint); the inventory window queues throws of dragged entries.
fn throw_items(
    pie: Res<PieState>,
    input: crate::core::settings::GameInput,
    registry: Res<BlockRegistry>,
    mut inventory: ResMut<Inventory>,
    mut queue: ResMut<DropQueue>,
    players: Query<&Player>,
    cameras: Query<&GlobalTransform, With<crate::core::editor_camera::EditorCamera>>,
    mut ui: ResMut<EditorUiState>,
) {
    if !pie.is_playing() || inventory.spectator {
        inventory.throw_requests.clear();
        return;
    }
    let mut requests = std::mem::take(&mut inventory.throw_requests);
    if pie.cursor_captured && input.just_pressed(crate::core::settings::GameAction::Drop) {
        let all = input.pressed(crate::core::settings::GameAction::Sprint);
        if let Some(key) = inventory.current().resource_key(&registry) {
            let owned = inventory.count(&key);
            if inventory.survival {
                if owned > 0 {
                    requests.push((key, if all { owned } else { 1 }));
                }
            } else {
                // Creative: an infinite supply.
                requests.push((key, if all { 16 } else { 1 }));
            }
        }
    }
    if requests.is_empty() {
        return;
    }
    let Ok(camera) = cameras.single() else {
        return;
    };
    let forward = camera.forward().as_vec3();
    let origin = camera.translation() + forward * 0.6 - Vec3::Y * 0.25;
    let _ = players.single();
    for (key, count) in requests {
        if count == 0 {
            continue;
        }
        if inventory.survival {
            let owned = inventory.count(&key);
            let count = count.min(owned);
            if count == 0 || !inventory.take(&key, count) {
                continue;
            }
            queue.throw(origin, forward * 6.0 + Vec3::Y * 2.0, &key, count);
        } else {
            queue.throw(origin, forward * 6.0 + Vec3::Y * 2.0, &key, count);
        }
        ui.info(format!("Dropped {count} {}.", crate::gameplay::crafting::resource_label(&key)));
    }
}

/// Colour of a raw material (no block, no item), stable per name.
fn material_color(name: &str) -> Color {
    let hash = name.bytes().fold(2166136261u32, |h, b| (h ^ b as u32).wrapping_mul(16777619));
    Color::hsl((hash % 360) as f32, 0.65, 0.6)
}

#[allow(clippy::too_many_arguments)]
fn spawn_drops(
    mut commands: Commands,
    pie: Res<PieState>,
    mut queue: ResMut<DropQueue>,
    registry: Res<BlockRegistry>,
    items: Res<ItemLibrary>,
    ui: Res<EditorUiState>,
    mut render: ResMut<VoxelRenderState>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut voxel_materials: ResMut<Assets<PolyVoxelMaterial>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if !pie.is_playing() {
        queue.pending.clear();
        return;
    }
    for (index, (position, resource, count, thrown)) in queue.pending.drain(..).enumerate() {
        let angle = index as f32 * 2.4 + position.x * 3.1 + position.z * 1.7;
        let velocity = thrown.unwrap_or(Vec3::new(angle.cos() * 1.6, 4.5, angle.sin() * 1.6));
        let drop = ItemDrop {
            resource: resource.clone(),
            count,
            velocity,
            // A thrown item waits longer before it can be picked up again.
            age: if thrown.is_some() { game_rules().drops.pickup_delay - game_rules().drops.throw_pickup_delay } else { 0.0 },
            resting: false,
        };
        let root = commands
            .spawn((
                Name::new(format!("Drop {resource}")),
                PieOnly,
                drop,
                Transform::from_translation(position),
                Visibility::default(),
            ))
            .id();

        if let Some(block) = registry.find(&resource) {
            let mut world = VoxelWorld::new();
            world.set(IVec3::ZERO, Some(Voxel::new(block)));
            if let Some(data) = mesher::build_chunk_mesh(&world, &registry, IVec3::ZERO, true).remove(&block) {
                let scale = 0.28;
                commands.spawn((
                    Mesh3d(meshes.add(mesh_from_data(data))),
                    MeshMaterial3d(material_handle(&mut render, &mut voxel_materials, &registry, block, &ui.shading)),
                    Transform::from_translation(Vec3::new(-0.5, 0.0, -0.5) * scale).with_scale(Vec3::splat(scale)),
                    ChildOf(root),
                ));
            }
        } else if let Some(item) = items.find(&resource) {
            let (min, max) = item.bounds();
            let size = (max - min).max_element().max(0.1);
            commands.spawn((
                Mesh3d(meshes.add(item_mesh(item))),
                MeshMaterial3d(materials.add(StandardMaterial {
                    base_color: Color::WHITE,
                    perceptual_roughness: 0.6,
                    ..default()
                })),
                Transform::from_scale(Vec3::splat(0.45 / size)),
                ChildOf(root),
            ));
        } else {
            commands.spawn((
                Mesh3d(meshes.add(Cuboid::new(0.18, 0.18, 0.18))),
                MeshMaterial3d(materials.add(StandardMaterial {
                    base_color: material_color(&resource),
                    emissive: material_color(&resource).to_linear() * 0.4,
                    ..default()
                })),
                Transform::from_xyz(0.0, 0.09, 0.0).with_rotation(Quat::from_rotation_z(0.6)),
                ChildOf(root),
            ));
        }
    }
}

fn solid_at(world: &VoxelWorld, point: Vec3) -> bool {
    let cell = IVec3::new(point.x.floor() as i32, point.y.floor() as i32, point.z.floor() as i32);
    world
        .get(cell)
        .map(|voxel| voxel.sub.is_some() || !world.is_liquid_block(voxel.block))
        .unwrap_or(false)
}

#[allow(clippy::too_many_arguments)]
fn simulate_drops(
    mut commands: Commands,
    time: Res<Time>,
    pie: Res<PieState>,
    world: Res<VoxelWorld>,
    registry: Res<BlockRegistry>,
    items: Res<ItemLibrary>,
    mut inventory: ResMut<Inventory>,
    mut sounds: ResMut<SoundQueue>,
    players: Query<&Transform, (With<Player>, Without<ItemDrop>)>,
    mut drops: Query<(Entity, &mut ItemDrop, &mut Transform)>,
) {
    if !pie.is_playing() {
        return;
    }
    let delta = time.delta_secs().min(0.05);
    // Spectators do not pick anything up.
    let player = players.single().ok().map(|t| t.translation).filter(|_| !inventory.spectator);
    for (entity, mut drop, mut transform) in &mut drops {
        drop.age += delta;
        if drop.age > game_rules().drops.lifetime {
            commands.entity(entity).despawn();
            continue;
        }
        // Magnet and pickup.
        // A full inventory leaves the item on the ground.
        let has_room = inventory.room_for(&drop.resource, 1) > 0;
        if let (Some(player), true) = (player, drop.age > game_rules().drops.pickup_delay && has_room) {
            let to = player - transform.translation;
            let distance = to.length();
            if distance < game_rules().drops.pickup_radius {
                let taken = inventory.add_capped(&drop.resource, drop.count);
                if taken > 0 {
                    crate::scripting::context::emit_game_event(crate::scripting::context::GameEvent::ItemCollected, &drop.resource, taken as f32, None);
                }
                if taken < drop.count {
                    drop.count -= taken;
                    continue;
                }
                if inventory.survival {
                    if let Some(slot) = HotbarItem::from_resource(&drop.resource, &registry, &items) {
                        inventory.auto_assign(slot);
                    }
                }
                sounds.play(SoundRequest::sfx(SoundKind::Pickup).with_volume(0.5));
                commands.entity(entity).despawn();
                continue;
            }
            if distance < game_rules().drops.magnet_radius {
                let pull = to.normalize_or_zero() * (9.0 * (1.0 - distance / game_rules().drops.magnet_radius) + 2.0);
                transform.translation += pull * delta;
                drop.resting = false;
                drop.velocity = Vec3::ZERO;
                transform.rotate_y(delta * 2.0);
                continue;
            }
        }
        // Fall and rest on the ground (liquids slow it down).
        if !drop.resting {
            drop.velocity.y -= game_rules().drops.gravity * delta;
            if world.liquid_at(transform.translation.floor().as_ivec3()).is_some() {
                drop.velocity *= 1.0 - (3.0 * delta).min(0.9);
            }
            let next = transform.translation + drop.velocity * delta;
            if drop.velocity.y < 0.0 && solid_at(&world, next) {
                transform.translation.y = next.y.floor() + 1.0;
                transform.translation.x = next.x;
                transform.translation.z = next.z;
                drop.resting = true;
                drop.velocity = Vec3::ZERO;
            } else if solid_at(&world, Vec3::new(next.x, transform.translation.y, next.z)) {
                drop.velocity.x = 0.0;
                drop.velocity.z = 0.0;
                transform.translation.y = next.y;
            } else {
                transform.translation = next;
            }
            if transform.translation.y < -80.0 {
                commands.entity(entity).despawn();
                continue;
            }
        } else if !solid_at(&world, transform.translation - Vec3::Y * 0.05) {
            drop.resting = false;
        }
        transform.rotate_y(delta * 1.5);
    }
}
