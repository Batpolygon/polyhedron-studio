//! Player avatar visuals, built from two rigs of the mob library (edited in
//! the studio like any mob, Mobs workspace):
//!
//! - **Player** (third person body): every bone is animated by the clips of
//!   the rig (walk, run, jump, swim, attack...), `Hips` and `Torso` turn
//!   toward a sideways walk, `Head` follows the camera; the held item hangs
//!   on the `Hand` socket. The same body
//!   is used by the HUD replica ([`crate::gameplay::player_preview`]) and by
//!   the 3D character of the main menu, whose head follows the mouse.
//! - **Player Hand** (first person, camera space): the `Arm` bone is swung
//!   by the animations; the held item hangs on its `Hand` socket.
//!
//! Body part colours come from the settings (main menu -> Customize):
//! each part (head, torso, arms, legs) is either the model colour or a
//! named colour; eyes and pupils keep theirs.
//!
//! Blocks are shown with their real mesh (shape or forge model) and their
//! voxel material, so the hand always holds exactly what will be placed.

use crate::core::game_rules::game_rules;
use std::f32::consts::{FRAC_PI_2, PI};

use bevy::camera::visibility::RenderLayers;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;

use crate::core::app_state::EditorUiState;
use crate::core::editor_camera::EditorCamera;
use crate::core::pie_manager::{PieOnly, PieState};
use crate::core::settings::{BoxColors, GameAction, GameInput, GameSettings};
use crate::gameplay::combat::CombatState;
use crate::gameplay::inventory::{HotbarItem, Inventory};
use crate::gameplay::mobs::library::MobLibrary;
use crate::gameplay::mobs::model::MobRig;
use crate::gameplay::mobs::pose::{drawn_bones, world_matrices, BonePose, RigPose};
use crate::physics::player_controller::{Player, PlayerControllerSet};
use crate::renderer::poly_material::PolyVoxelMaterial;
use crate::renderer::voxel_render::{material_handle, mesh_from_data, VoxelRenderState};
use crate::voxel::blocks::BlockRegistry;
use crate::voxel::mesher;
use crate::voxel::world::{Voxel, VoxelWorld};

/// Rest position of the first-person arm in camera space.
const ARM_REST: Vec3 = Vec3::new(0.30, -0.27, -0.40);
/// Render layer and position of the main menu character.
pub const MENU_LAYER: usize = 4;
pub const MENU_ORIGIN: Vec3 = Vec3::new(0.0, -4300.0, 0.0);

/// Which copy of the body this is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BodyKind {
    /// Child of the player (third person).
    World,
    /// HUD replica.
    Preview,
    /// Main menu character.
    Menu,
}

/// Root of a copy of the player body.
#[derive(Component)]
struct RigBody(BodyKind);

/// One box of a rig copy (its transform is posed every frame).
#[derive(Component)]
struct RigBox {
    bone: String,
}

/// Root of the first-person arm (child of the camera).
#[derive(Component)]
struct ViewModel;

/// Pivot of the first-person arm (swung by the animations).
#[derive(Component)]
struct ViewArm;

/// Where the held item is attached (`true` = first person).
#[derive(Component)]
struct HeldItemSlot(bool);

/// One spawned piece of the held item.
#[derive(Component)]
struct HeldItemPiece;

/// Head direction of the menu character (set by the main menu).
#[derive(Resource, Debug, Default)]
pub struct MenuCharacter {
    /// Pointer relative to the character card: `(-1..1, -1..1)`.
    pub look: Option<Vec2>,
}

#[derive(Resource, Default)]
struct AvatarAssets {
    cube: Handle<Mesh>,
    ready: bool,
}

#[derive(Resource, Default)]
struct AvatarState {
    /// Item currently built in the slots (debug string + registry revision).
    item_key: String,
    /// Rigs and colours the bodies were built with.
    look_key: String,
    use_pulse: f32,
    time: f32,
    /// Smoothed head direction of the menu character.
    menu_look: Vec2,
    /// Walking cycle, 0..1, shared by the Walk and Run clips.
    gait: f32,
    /// Seconds off the ground.
    air_time: f32,
    /// Seconds since landing from a fall; `None` once the clip is over.
    landed: Option<f32>,
    /// Seconds the mining swing has been going.
    mining_time: f32,
    /// What is on screen: follows the clips, so nothing snaps between them.
    body_pose: RigPose,
    hand_pose: RigPose,
}

pub struct AvatarPlugin;

impl Plugin for AvatarPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AvatarAssets>()
            .init_resource::<AvatarState>()
            .init_resource::<MenuCharacter>()
            .add_systems(
                Update,
                (spawn_avatar, rebuild_bodies, sync_held_item, animate_avatar)
                    .chain()
                    .after(PlayerControllerSet),
            );
    }
}

fn ensure_assets(assets: &mut AvatarAssets, meshes: &mut Assets<Mesh>) {
    if assets.ready {
        return;
    }
    assets.cube = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
    assets.ready = true;
}

/// Colour of a box with the player's customisation applied (by box name; the
/// first-person arm uses the right-hand boxes of the body: "Arm" -> "Arm R").
fn bone_color(bone: &crate::gameplay::mobs::model::MobBox, colors: &BoxColors, hand: bool) -> Color {
    let custom = if hand {
        colors.get(&format!("{} R", bone.name)).or_else(|| colors.get(&bone.name))
    } else {
        colors.get(&bone.name)
    };
    match custom {
        Some(rgb) => Color::srgb_u8(rgb[0], rgb[1], rgb[2]),
        None => Color::srgb(bone.color[0], bone.color[1], bone.color[2]),
    }
}

/// Spawn the boxes of `rig` under `parent` (posed later by [`pose_boxes`]).
fn spawn_rig_boxes(
    commands: &mut Commands,
    parent: Entity,
    rig: &MobRig,
    cube: &Handle<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    colors: &BoxColors,
    hand: bool,
    layer: Option<&RenderLayers>,
) {
    let pose = RigPose::default();
    let drawn = drawn_bones(rig, &pose);
    for bone in rig.boxes.iter().filter(|bone| bone.visible) {
        let material = materials.add(StandardMaterial {
            base_color: bone_color(bone, colors, hand),
            perceptual_roughness: 0.8,
            ..default()
        });
        let transform = drawn
            .iter()
            .find(|d| d.name == bone.name)
            .map(|d| Transform::from_translation(d.center).with_rotation(d.rotation).with_scale(d.size))
            .unwrap_or_default();
        let mut entity = commands.spawn((
            RigBox { bone: bone.name.clone() },
            Mesh3d(cube.clone()),
            MeshMaterial3d(material),
            transform,
            ChildOf(parent),
        ));
        if hand {
            entity.insert(NotShadowCaster);
        }
        if let Some(layer) = layer {
            entity.insert((layer.clone(), NotShadowCaster));
        }
    }
}

/// Local transform of a socket under its rig root.
fn socket_transform(rig: &MobRig, pose: &RigPose, socket: &str) -> Option<Transform> {
    let socket = rig.sockets.iter().find(|s| s.name == socket)?;
    let matrices = world_matrices(rig, pose);
    let bone = matrices.get(&socket.bone)?;
    let scale = Vec3::from_array(rig.scale).max(Vec3::splat(1e-4));
    let local = Mat4::from_translation(Vec3::from_array(socket.offset))
        * Mat4::from_euler(
            EulerRot::XYZ,
            socket.rotation[0].to_radians(),
            socket.rotation[1].to_radians(),
            socket.rotation[2].to_radians(),
        );
    let (_, rotation, translation) = (Mat4::from_scale(scale) * *bone * local).to_scale_rotation_translation();
    Some(Transform::from_translation(translation).with_rotation(rotation))
}

/// Build one copy of the player body.
#[allow(clippy::too_many_arguments)]
fn spawn_body(
    commands: &mut Commands,
    kind: BodyKind,
    parent: Option<Entity>,
    transform: Transform,
    rig: &MobRig,
    assets: &AvatarAssets,
    materials: &mut Assets<StandardMaterial>,
    colors: &BoxColors,
    layer: Option<&RenderLayers>,
    visibility: Visibility,
) -> Entity {
    let name = match kind {
        BodyKind::World => "Avatar body",
        BodyKind::Preview => "Player preview body",
        BodyKind::Menu => "Menu character",
    };
    let mut root = commands.spawn((Name::new(name), RigBody(kind), transform, visibility));
    if let Some(parent) = parent {
        root.insert(ChildOf(parent));
    }
    if kind == BodyKind::Preview {
        root.insert(PieOnly);
    }
    if let Some(layer) = layer {
        root.insert(layer.clone());
    }
    let root = root.id();
    spawn_rig_boxes(commands, root, rig, &assets.cube, materials, colors, false, layer);
    if kind != BodyKind::Menu {
        let at = socket_transform(rig, &RigPose::default(), "Hand")
            .unwrap_or(Transform::from_xyz(0.36, -0.14, -0.06).with_rotation(Quat::from_rotation_x(-FRAC_PI_2)));
        let mut slot = commands.spawn((HeldItemSlot(false), at, Visibility::Inherited, ChildOf(root)));
        if let Some(layer) = layer {
            slot.insert(layer.clone());
        }
    }
    root
}

#[allow(clippy::too_many_arguments)]
fn spawn_avatar(
    mut commands: Commands,
    players: Query<Entity, Added<Player>>,
    cameras: Query<Entity, With<EditorCamera>>,
    mut assets: ResMut<AvatarAssets>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut state: ResMut<AvatarState>,
    settings: Res<GameSettings>,
    library: Res<MobLibrary>,
    menu_bodies: Query<(), With<RigBody>>,
) {
    ensure_assets(&mut assets, &mut meshes);
    let colors = settings.part_colors.clone();
    let rig = library.player_rig();

    // The menu character always exists (shown by its own camera).
    if menu_bodies.is_empty() {
        let layer = RenderLayers::layer(MENU_LAYER);
        spawn_body(
            &mut commands,
            BodyKind::Menu,
            None,
            Transform::from_translation(MENU_ORIGIN),
            &rig,
            &assets,
            &mut materials,
            &colors,
            Some(&layer),
            Visibility::Inherited,
        );
    }

    for player in &players {
        state.item_key.clear();
        spawn_body(&mut commands, BodyKind::World, Some(player), Transform::default(), &rig, &assets, &mut materials, &colors, None, Visibility::Hidden);
        let layer = RenderLayers::layer(crate::gameplay::player_preview::PREVIEW_LAYER);
        spawn_body(
            &mut commands,
            BodyKind::Preview,
            None,
            Transform::from_translation(crate::gameplay::player_preview::PREVIEW_ORIGIN).with_rotation(Quat::from_rotation_y(-0.35)),
            &rig,
            &assets,
            &mut materials,
            &colors,
            Some(&layer),
            Visibility::Inherited,
        );

        // --- First person arm -------------------------------------------------
        let Ok(camera) = cameras.single() else {
            continue;
        };
        let root = commands
            .spawn((Name::new("View model"), ViewModel, PieOnly, Transform::default(), Visibility::Inherited, ChildOf(camera)))
            .id();
        spawn_view_arm(&mut commands, root, &library.hand_rig(), &assets, &mut materials, &colors);
    }
}

/// The first-person arm boxes and item slot under `root`.
fn spawn_view_arm(
    commands: &mut Commands,
    root: Entity,
    rig: &MobRig,
    assets: &AvatarAssets,
    materials: &mut Assets<StandardMaterial>,
    colors: &BoxColors,
) {
    let arm = commands.spawn((ViewArm, Transform::from_translation(ARM_REST), Visibility::Inherited, ChildOf(root))).id();
    spawn_rig_boxes(commands, arm, rig, &assets.cube, materials, colors, true, None);
    let at = socket_transform(rig, &RigPose::default(), "Hand").unwrap_or(
        Transform::from_xyz(-0.01, 0.06, -0.13).with_rotation(Quat::from_rotation_x(-FRAC_PI_2 * 0.85) * Quat::from_rotation_z(0.15)),
    );
    commands.spawn((HeldItemSlot(true), at.with_scale(Vec3::splat(0.55)), Visibility::Inherited, ChildOf(arm)));
}

/// Rebuild every body when the rigs (studio edits) or the colours change.
#[allow(clippy::too_many_arguments)]
fn rebuild_bodies(
    mut commands: Commands,
    library: Res<MobLibrary>,
    settings: Res<GameSettings>,
    assets: Res<AvatarAssets>,
    mut state: ResMut<AvatarState>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    bodies: Query<(Entity, &RigBody, Option<&ChildOf>, &Transform, &Visibility)>,
    arms: Query<(Entity, &ChildOf), With<ViewArm>>,
) {
    let key = format!("{}|{:?}", library.revision, settings.part_colors);
    if key == state.look_key || !assets.ready {
        return;
    }
    let player = library.player_rig();
    let hand = library.hand_rig();
    let first = state.look_key.is_empty();
    state.look_key = key;
    if first {
        return;
    }
    state.item_key.clear();
    let colors = settings.part_colors.clone();
    for (entity, body, parent, transform, visibility) in &bodies {
        let kind = body.0;
        let layer = match kind {
            BodyKind::World => None,
            BodyKind::Preview => Some(RenderLayers::layer(crate::gameplay::player_preview::PREVIEW_LAYER)),
            BodyKind::Menu => Some(RenderLayers::layer(MENU_LAYER)),
        };
        let parent = parent.map(|p| p.parent());
        let transform = *transform;
        let visibility = *visibility;
        commands.entity(entity).despawn();
        spawn_body(&mut commands, kind, parent, transform, &player, &assets, &mut materials, &colors, layer.as_ref(), visibility);
    }
    for (arm, parent) in &arms {
        let root = parent.parent();
        commands.entity(arm).despawn();
        spawn_view_arm(&mut commands, root, &hand, &assets, &mut materials, &colors);
    }
}

/// Rebuild the held item in every slot when the hotbar selection changes.
#[allow(clippy::too_many_arguments)]
fn sync_held_item(
    mut commands: Commands,
    inventory: Res<Inventory>,
    registry: Res<BlockRegistry>,
    ui: Res<EditorUiState>,
    assets: Res<AvatarAssets>,
    mut state: ResMut<AvatarState>,
    mut render: ResMut<VoxelRenderState>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut voxel_materials: ResMut<Assets<PolyVoxelMaterial>>,
    slots: Query<(Entity, &HeldItemSlot, Option<&RenderLayers>)>,
    pieces: Query<(Entity, &ChildOf), With<HeldItemPiece>>,
    items: Res<crate::gameplay::items::ItemLibrary>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mobs: Res<crate::gameplay::mobs::library::MobLibrary>,
) {
    if slots.is_empty() || !assets.ready {
        return;
    }
    let item = inventory.current().clone();
    let key = format!("{item:?}|{}|{}|{}|{}", registry.revision, items.revision, mobs.revision, slots.iter().count());
    if key == state.item_key {
        return;
    }
    state.item_key = key;
    let slot_entities: Vec<Entity> = slots.iter().map(|(entity, ..)| entity).collect();
    for (piece, parent) in &pieces {
        if slot_entities.contains(&parent.parent()) {
            commands.entity(piece).despawn();
        }
    }

    // Block items: the real voxel mesh of the block.
    let block_mesh = match item {
        HotbarItem::Block(block) => {
            let mut world = VoxelWorld::new();
            world.set(IVec3::ZERO, Some(Voxel::new(block)));
            mesher::build_chunk_mesh(&world, &registry, IVec3::ZERO, true).remove(&block).map(|data| {
                (
                    meshes.add(mesh_from_data(data)),
                    material_handle(&mut render, &mut voxel_materials, &registry, block, &ui.shading),
                )
            })
        }
        _ => None,
    };
    // Armoury items: their coloured box model.
    let item_model = match &item {
        HotbarItem::Item(id) => Some(items.find_or_placeholder(id)).map(|def| {
            let (min, max) = def.bounds();
            let size = (max - min).max_element().max(0.1);
            (
                meshes.add(crate::gameplay::items::item_mesh(&def)),
                materials.add(StandardMaterial {
                    base_color: Color::WHITE,
                    perceptual_roughness: 0.6,
                    ..default()
                }),
                0.8 * def.hold_scale.clamp(0.2, 4.0) / size,
            )
        }),
        _ => None,
    };
    // Spawn egg: the same cubic egg as its icon (`items::egg_boxes`).
    let egg_parts: Vec<(Handle<StandardMaterial>, Vec3, Vec3)> = match &item {
        HotbarItem::SpawnEgg(name) => crate::gameplay::items::egg_boxes(mobs.rigs.iter().find(|rig| rig.name == *name))
            .into_iter()
            .map(|(center, size, color)| {
                let material = materials.add(StandardMaterial {
                    base_color: Color::srgb_u8(color[0], color[1], color[2]),
                    perceptual_roughness: 0.5,
                    ..default()
                });
                (material, center, size)
            })
            .collect(),
        _ => Vec::new(),
    };

    for (slot, HeldItemSlot(first_person), layer) in &slots {
        let first_person = *first_person;
        let layer = layer.cloned();
        let decorate = |entity: &mut EntityCommands| {
            if first_person {
                entity.insert(NotShadowCaster);
            }
            if let Some(layer) = &layer {
                entity.insert((layer.clone(), NotShadowCaster));
            }
        };
        let part = |commands: &mut Commands, material: &Handle<StandardMaterial>, center: Vec3, size: Vec3| {
            let mut entity = commands.spawn((
                HeldItemPiece,
                Mesh3d(assets.cube.clone()),
                MeshMaterial3d(material.clone()),
                Transform::from_translation(center).with_scale(size),
                ChildOf(slot),
            ));
            decorate(&mut entity);
        };
        match &item {
            HotbarItem::Empty => {}
            HotbarItem::SpawnEgg(_) => {
                const EGG_SCALE: f32 = 0.22;
                for (material, center, size) in &egg_parts {
                    part(&mut commands, material, Vec3::new(0.0, 0.1, 0.0) + *center * EGG_SCALE, *size * EGG_SCALE);
                }
            }
            HotbarItem::Item(_) => {
                if let Some((mesh, material, scale)) = &item_model {
                    let mut entity = commands.spawn((
                        HeldItemPiece,
                        Mesh3d(mesh.clone()),
                        MeshMaterial3d(material.clone()),
                        Transform::from_scale(Vec3::splat(*scale)),
                        ChildOf(slot),
                    ));
                    decorate(&mut entity);
                }
            }
            HotbarItem::Block(_) => {
                if let Some((mesh, material)) = &block_mesh {
                    let scale = 0.24;
                    let mut entity = commands.spawn((
                        HeldItemPiece,
                        Mesh3d(mesh.clone()),
                        MeshMaterial3d(material.clone()),
                        Transform::from_translation(Vec3::new(-0.5, 0.0, -0.5) * scale + Vec3::Y * 0.02).with_scale(Vec3::splat(scale)),
                        ChildOf(slot),
                    ));
                    decorate(&mut entity);
                }
            }
            HotbarItem::Resource(id) => {
                // A small glowing chunk of the resource.
                let color = crate::gameplay::crafting::resource_info(id).map(|info| info.color).unwrap_or([200, 200, 210]);
                let material = materials.add(StandardMaterial {
                    base_color: Color::srgb_u8(color[0], color[1], color[2]),
                    perceptual_roughness: 0.5,
                    ..default()
                });
                part(&mut commands, &material, Vec3::new(0.0, 0.08, 0.0), Vec3::splat(0.14));
            }
        }
    }
}

/// Names of the clips the player rigs ("Player", "Player Hand") play in game.
/// They are ordinary clips of those rigs (Mobs workspace); the defaults are
/// installed by `mobs::model::install_player_clips`.
pub mod clips {
    pub const IDLE: &str = "Idle";
    pub const WALK: &str = "Walk";
    pub const RUN: &str = "Run";
    pub const JUMP: &str = "Jump";
    pub const FALL: &str = "Fall";
    pub const LAND: &str = "Land";
    pub const SWIM: &str = "Swim";
    pub const FLY: &str = "Fly";
    pub const SIT: &str = "Sit";
    pub const ATTACK: &str = "Attack";
    pub const USE: &str = "Use";
    pub const BLOCK: &str = "Block";
    pub const HOLD: &str = "Hold";
    pub const MENU: &str = "Menu";
}

/// Overlay the bones animated by clip `name` on `pose`: sampled at `time`
/// (seconds, looping) and blended with `weight`. Returns whether it exists.
fn overlay_clip(rig: &MobRig, pose: &mut RigPose, name: &str, time: f32, weight: f32) -> bool {
    let Some(clip) = rig.clip(name).filter(|clip| clip.key_count() > 0) else {
        return false;
    };
    let weight = weight.clamp(0.0, 1.0);
    if weight <= 0.001 {
        return true;
    }
    let duration = clip.duration_or(1.0).max(0.01);
    let local = if clip.looping { time.rem_euclid(duration) } else { time.clamp(0.0, duration) };
    for track in clip.tracks.iter().filter(|track| !track.keys.is_empty()) {
        let sampled = crate::gameplay::mobs::pose::sample_track(track, local);
        let current = pose.get(&track.bone);
        pose.bones.insert(
            track.bone.clone(),
            BonePose {
                translation: current.translation.lerp(sampled.translation, weight),
                rotation: current.rotation.lerp(sampled.rotation, weight),
                scale: current.scale.lerp(sampled.scale, weight),
            },
        );
    }
    true
}

/// Clip time of a one-shot action from its 0..1 progress.
fn action_time(rig: &MobRig, name: &str, progress: f32) -> f32 {
    rig.clip(name).map(|clip| clip.duration_or(1.0) * progress.clamp(0.0, 1.0)).unwrap_or(0.0)
}

/// Blocks walked per second of the Walk and Run clips.
const STRIDE_BLOCKS: f32 = 2.5;
/// Speed at which the pose on screen reaches the clips.
const POSE_FOLLOW: f32 = 16.0;
/// Below this time in the air, a step down is not a jump.
const AIR_DELAY: f32 = 0.09;
/// Falls shorter than this end without the Land clip.
const LAND_AFTER: f32 = 0.3;
/// Turn of the body when walking sideways, degrees.
const STRAFE_TURN: f32 = 42.0;

fn clip_length(rig: &MobRig, name: &str, fallback: f32) -> f32 {
    rig.clip(name).map_or(fallback, |clip| clip.duration_or(fallback).max(0.01))
}

/// What the legs are doing, for both rigs.
#[derive(Clone, Copy)]
struct Motion {
    walk: f32,
    run: f32,
    gait: f32,
    air: Option<f32>,
    rising: bool,
    landed: Option<f32>,
}

/// Ground and air clips of `rig`.
fn overlay_motion(rig: &MobRig, pose: &mut RigPose, motion: Motion, time: f32, bob: bool) {
    match motion.air {
        Some(air) => {
            if motion.rising {
                overlay_clip(rig, pose, clips::JUMP, air, 1.0);
            } else {
                overlay_clip(rig, pose, clips::FALL, air, 1.0);
            }
        }
        None => {
            overlay_clip(rig, pose, clips::IDLE, time, 1.0);
            if bob {
                overlay_clip(rig, pose, clips::WALK, motion.gait * clip_length(rig, clips::WALK, 1.0), motion.walk);
                overlay_clip(rig, pose, clips::RUN, motion.gait * clip_length(rig, clips::RUN, 1.0), motion.run);
            }
            if let Some(since) = motion.landed {
                overlay_clip(rig, pose, clips::LAND, since, 1.0);
            }
        }
    }
}

/// Move the pose on screen toward the pose of the clips.
fn follow_pose(shown: &mut RigPose, rig: &MobRig, target: &RigPose, blend: f32) {
    for bone in &rig.boxes {
        let (from, to) = (shown.get(&bone.name), target.get(&bone.name));
        shown.bones.insert(
            bone.name.clone(),
            BonePose {
                translation: from.translation.lerp(to.translation, blend),
                rotation: from.rotation.lerp(to.rotation, blend),
                scale: from.scale.lerp(to.scale, blend),
            },
        );
    }
}

/// Pose every box of a body (and its held item slot) from `pose`.
fn pose_boxes(
    rig: &MobRig,
    pose: &RigPose,
    children: &Children,
    boxes: &mut Query<(&RigBox, &mut Transform), (Without<RigBody>, Without<HeldItemSlot>, Without<ViewArm>)>,
    slots: &mut Query<(&HeldItemSlot, &mut Transform), (Without<RigBox>, Without<RigBody>, Without<ViewArm>)>,
    first_person: bool,
) {
    let drawn = drawn_bones(rig, pose);
    let hand = socket_transform(rig, pose, "Hand");
    for child in children.iter() {
        if let Ok((rig_box, mut transform)) = boxes.get_mut(child) {
            if let Some(bone) = drawn.iter().find(|d| d.name == rig_box.bone) {
                transform.translation = bone.center;
                transform.rotation = bone.rotation;
                transform.scale = bone.size;
            }
        } else if let Ok((slot, mut transform)) = slots.get_mut(child) {
            if slot.0 == first_person {
                if let Some(hand) = hand {
                    transform.translation = hand.translation;
                    transform.rotation = hand.rotation;
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn animate_avatar(
    time: Res<Time>,
    pie: Res<PieState>,
    input: GameInput,
    settings: Res<GameSettings>,
    combat: Res<CombatState>,
    inventory: Res<Inventory>,
    library: Res<MobLibrary>,
    menu: Res<MenuCharacter>,
    mut state: ResMut<AvatarState>,
    players: Query<&Player>,
    mut bodies: Query<(&RigBody, &mut Visibility, &mut Transform, &Children), (Without<ViewModel>, Without<RigBox>, Without<HeldItemSlot>, Without<ViewArm>)>,
    mut views: Query<&mut Visibility, (With<ViewModel>, Without<RigBody>)>,
    mut boxes: Query<(&RigBox, &mut Transform), (Without<RigBody>, Without<HeldItemSlot>, Without<ViewArm>)>,
    mut slots: Query<(&HeldItemSlot, &mut Transform), (Without<RigBox>, Without<RigBody>, Without<ViewArm>)>,
    mut arms: Query<(&mut Transform, &Children), (With<ViewArm>, Without<RigBox>, Without<RigBody>, Without<HeldItemSlot>)>,
    held: Res<crate::gameplay::items::HeldTool>,
) {
    let delta = time.delta_secs();
    state.time += delta;
    let t = state.time;
    let rig = library.player_rig();

    // --- Main menu character: idle breathing, head toward the mouse --------
    let wanted = menu.look.unwrap_or(Vec2::ZERO);
    let blend = 1.0 - (-8.0 * time.delta_secs().max(0.001)).exp();
    state.menu_look = state.menu_look.lerp(wanted, blend);
    let look = state.menu_look;
    for (body, _, mut transform, children) in &mut bodies {
        if body.0 != BodyKind::Menu {
            continue;
        }
        transform.translation = MENU_ORIGIN;
        transform.rotation = Quat::from_rotation_y(PI + look.x * 0.35);
        let mut pose = RigPose::default();
        // The rig's "Menu" clip, else its "Idle" clip.
        if !overlay_clip(&rig, &mut pose, clips::MENU, t, 1.0) {
            overlay_clip(&rig, &mut pose, clips::IDLE, t, 1.0);
        }
        // The head follows the mouse on top of the clip.
        let mut head = pose.get("Head");
        head.rotation += Vec3::new((-look.y * 0.5).to_degrees(), (look.x * 0.75).to_degrees(), 0.0);
        pose.bones.insert("Head".to_string(), head);
        pose_boxes(&rig, &pose, children, &mut boxes, &mut slots, false);
    }

    let Ok(player) = players.single() else {
        return;
    };
    let capturing = pie.is_playing() && pie.cursor_captured;
    if capturing && input.just_pressed(GameAction::Use) {
        state.use_pulse = 1.0;
    }
    state.use_pulse = (state.use_pulse - delta * 5.0).max(0.0);

    // Swing: sword combo timer, or continuous mining / punching.
    let item = inventory.current();
    let mining = capturing
        && input.pressed(GameAction::Attack)
        && held.mining_speed > 0.0
        && matches!(item, HotbarItem::Block(_) | HotbarItem::Empty | HotbarItem::Item(_) | HotbarItem::Resource(_));
    state.mining_time = if mining { state.mining_time + delta } else { 0.0 };
    let swing = if combat.swing_timer > 0.0 {
        Some(1.0 - combat.swing_timer / game_rules().combat.swing_time)
    } else if mining {
        Some((state.mining_time * 1.75).fract())
    } else {
        None
    };
    let blocking = combat.blocking;
    let hidden = inventory.spectator;

    // --- Walking, running, jumping -------------------------------------------
    let hand = library.hand_rig();
    let rules = game_rules();
    let movement = &rules.movement;
    let speed = player.ground_speed;
    let run = ((speed - movement.walk_speed) / (movement.sprint_speed - movement.walk_speed).max(0.1)).clamp(0.0, 1.0);
    let walk = (speed / movement.walk_speed.max(0.1)).clamp(0.0, 1.0);
    let cycle = clip_length(&rig, clips::WALK, 1.3).lerp(clip_length(&rig, clips::RUN, 1.6), run) * STRIDE_BLOCKS;
    let facing = Vec3::new(-player.yaw.sin(), 0.0, -player.yaw.cos());
    let side = Vec3::new(facing.z, 0.0, -facing.x);
    let flat = Vec3::new(player.velocity.x, 0.0, player.velocity.z);
    let (ahead, across) = (flat.dot(facing), flat.dot(side));
    let backward = ahead < -0.1 * speed;
    state.gait = (state.gait + if backward { -1.0 } else { 1.0 } * speed * delta / cycle.max(0.1)).rem_euclid(1.0);

    let swimming_or_flying = player.swimming || player.flying || player.seated.is_some();
    let was_in_air = state.air_time;
    state.air_time = if player.grounded || swimming_or_flying { 0.0 } else { state.air_time + delta };
    if state.air_time == 0.0 && was_in_air > LAND_AFTER && player.grounded {
        state.landed = Some(0.0);
    }
    state.landed = state.landed.map(|since| since + delta).filter(|since| *since < clip_length(&rig, clips::LAND, 0.3));
    let motion = Motion {
        walk,
        run,
        gait: state.gait,
        air: (state.air_time > AIR_DELAY).then_some(state.air_time),
        rising: player.velocity.y > 0.4,
        landed: state.landed,
    };

    // --- Third person pose (body and HUD replica) ---------------------------
    // Every motion comes from the clips of the "Player" rig (Mobs workspace).
    // On top of them, the body turns toward a sideways walk and the head
    // follows the camera.
    let mut target = RigPose::default();
    if player.seated.is_some() {
        overlay_clip(&rig, &mut target, clips::SIT, t, 1.0);
    } else if player.swimming {
        overlay_clip(&rig, &mut target, clips::SWIM, t, 1.0);
    } else if player.flying {
        overlay_clip(&rig, &mut target, clips::FLY, t, 1.0);
    } else {
        overlay_motion(&rig, &mut target, motion, t, true);
        let turn = (across / ahead.abs().max(0.5)).atan().to_degrees().clamp(-STRAFE_TURN, STRAFE_TURN) * walk;
        let mut hips = target.get("Hips");
        hips.rotation.y -= turn;
        target.bones.insert("Hips".to_string(), hips);
        let mut torso = target.get("Torso");
        torso.rotation.y += turn * 0.7;
        target.bones.insert("Torso".to_string(), torso);
    }
    if !matches!(item, HotbarItem::Empty) && !player.swimming {
        overlay_clip(&rig, &mut target, clips::HOLD, t, 1.0);
    }
    if blocking {
        overlay_clip(&rig, &mut target, clips::BLOCK, t, 1.0);
    }
    if let Some(progress) = swing {
        overlay_clip(&rig, &mut target, clips::ATTACK, action_time(&rig, clips::ATTACK, progress), 1.0);
    }
    if state.use_pulse > 0.0 {
        overlay_clip(&rig, &mut target, clips::USE, action_time(&rig, clips::USE, 1.0 - state.use_pulse), 1.0);
    }
    let follow = 1.0 - (-POSE_FOLLOW * delta.max(0.001)).exp();
    follow_pose(&mut state.body_pose, &rig, &target, follow);
    let mut pose = state.body_pose.clone();
    let mut head = pose.get("Head");
    head.rotation.x += player.pitch.to_degrees() * 0.6;
    pose.bones.insert("Head".to_string(), head);

    let third = player.third_person;
    for (body, mut visibility, _, children) in &mut bodies {
        match body.0 {
            BodyKind::World => {
                let wanted = if third && !hidden { Visibility::Inherited } else { Visibility::Hidden };
                if *visibility != wanted {
                    *visibility = wanted;
                }
            }
            BodyKind::Preview => {}
            BodyKind::Menu => continue,
        }
        pose_boxes(&rig, &pose, children, &mut boxes, &mut slots, false);
    }
    for mut visibility in &mut views {
        let wanted = if third || player.seated.is_some() || hidden { Visibility::Hidden } else { Visibility::Inherited };
        if *visibility != wanted {
            *visibility = wanted;
        }
    }

    // --- First person ---------------------------------------------------------------
    // The arm rests at ARM_REST; its motion is the clips of the "Player Hand"
    // rig (Mobs workspace).
    let mut hand_target = RigPose::default();
    if player.swimming {
        overlay_clip(&hand, &mut hand_target, clips::SWIM, t, 1.0);
    } else {
        overlay_motion(&hand, &mut hand_target, motion, t, settings.head_bob);
    }
    if blocking {
        overlay_clip(&hand, &mut hand_target, clips::BLOCK, t, 1.0);
    }
    if let Some(progress) = swing {
        overlay_clip(&hand, &mut hand_target, clips::ATTACK, action_time(&hand, clips::ATTACK, progress), 1.0);
    }
    if state.use_pulse > 0.0 {
        overlay_clip(&hand, &mut hand_target, clips::USE, action_time(&hand, clips::USE, 1.0 - state.use_pulse), 1.0);
    }
    follow_pose(&mut state.hand_pose, &hand, &hand_target, follow);
    let hand_pose = state.hand_pose.clone();
    for (mut transform, children) in &mut arms {
        transform.translation = ARM_REST;
        transform.rotation = Quat::from_rotation_y(0.12) * Quat::from_rotation_x(0.08);
        pose_boxes(&hand, &hand_pose, children, &mut boxes, &mut slots, true);
    }
}
