//! Movables: a structure built on Movable frames becomes one dynamic body.
//!
//! `E` on a Movable frame launches it: a 6-connected flood fill collects the
//! functional blocks connected to the frame (frames, motors, controllers,
//! seats, lamps, generators...) and every other block attached to them at or
//! above the frame level (decorations, walls, wheels...). Terrain under the
//! structure never joins; when too many blocks are attached (the structure
//! touches a hill) only the functional blocks are taken.
//!
//! The collected cells are removed from the voxel world and re-created as a
//! Rapier `Dynamic` body with a compound collider (one cuboid per cell, half
//! height for slabs) and the same toon materials. `E` on one of its frames
//! settles it back: the body snaps to the nearest grid rotation and its
//! blocks return to the world as solid blocks. The world snapshot restores
//! everything when PIE stops.
//!
//! Motors on board: configured with `E` (before launch on the block, after
//! launch on the body), a running motor pushes the body along its hub axis
//! (forward or backward), so "frame + motor + start" drives by physics.
//!
//! Driving: sit in a `Wagon` seat with `E`, or hold `Shift` with the Remote
//! selected to pilot the nearest movable that has a `Controller` (40 m).
//! `ZQSD/WASD` thrust and steer, `Space` lifts. Motors need power: a
//! `Combustor` on board, or the wireless sphere of a nearby generator.

use crate::core::game_rules::game_rules;
use std::collections::{HashSet, VecDeque};

use bevy::prelude::*;
use bevy_rapier3d::prelude::*;

use crate::audio::dsp_engine::SoundKind;
use crate::audio::sound_events::{SoundQueue, SoundRequest};
use crate::core::app_state::EditorUiState;
use crate::core::pie_manager::{PieOnly, PieSet, PieState};
use crate::core::settings::{GameAction, GameInput};
use crate::gameplay::energy_grid::EnergyGrid;
use crate::gameplay::inventory::Inventory;
use crate::physics::player_controller::{Player, PlayerControllerSet, PlayerIntent};
use crate::renderer::poly_material::PolyVoxelMaterial;
use crate::renderer::voxel_render::{material_handle, mesh_from_data, VoxelRenderState};
use crate::voxel::blocks::{BlockFunction, BlockRegistry};
use crate::voxel::mesher;
use crate::voxel::world::VoxelWorld;


/// The controller the Remote drives: `E` on a controller with the Remote in
/// hand connects it, `E` again disconnects it. Not connected, the Remote
/// drives the nearest controlled movable.
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq)]
pub enum RemoteLink {
    #[default]
    None,
    /// A controller of the static world (follows its structure at launch).
    Cell(IVec3),
    /// The controller of a launched movable.
    Body(Entity),
}


/// A motor mounted on a movable body.
#[derive(Debug, Clone)]
pub struct MotorMount {
    /// Local centre of the motor cell.
    pub local: Vec3,
    /// Local hub axis (the thrust direction when running forward).
    pub axis: Vec3,
    pub enabled: bool,
    pub rpm: f32,
    /// Clockwise = forward along the hub axis.
    pub clockwise: bool,
}

/// A dynamic body assembled from voxels.
#[derive(Component, Debug, Clone)]
pub struct MovableBody {
    /// Local centres of every cell (for hit and push tests).
    pub cells: Vec<Vec3>,
    /// The original voxels, by integer cell relative to `grid_offset`.
    pub voxels: Vec<(IVec3, crate::voxel::world::Voxel)>,
    /// Local position of the min corner of cell `(0, 0, 0)`.
    pub grid_offset: Vec3,
    pub motors: Vec<MotorMount>,
    pub seats: Vec<Vec3>,
    pub has_controller: bool,
    pub has_engine: bool,
    /// Local forward axis (from the controller or seat orientation).
    pub forward: Vec3,
    pub mass: f32,
    pub engine_timer: f32,
    pub powered_motors: usize,
}

impl MovableBody {
}

/// A wheel spun by a motor of a movable: the blocks attached to the motor
/// hub, assembled as their own body and held by a revolute joint whose
/// motor follows the mount settings.
#[derive(Component, Debug, Clone, Copy)]
pub struct RotorOf {
    /// The movable carrying the motor.
    pub body: Entity,
    /// Local centre of the motor cell in that body.
    pub motor_local: Vec3,
}


/// Blocks attached to the hub of the motor at `motor` (facing `axis`) that
/// belong to it alone: a flood fill through `cells` from the hub cell that
/// never reaches another functional block (such a link means the blocks are
/// part of the chassis). `None` when there is no such wheel.
pub fn rotor_cells(
    world: &VoxelWorld,
    registry: &BlockRegistry,
    cells: &HashSet<IVec3>,
    motor: IVec3,
    axis: IVec3,
) -> Option<Vec<IVec3>> {
    let functional = |cell: IVec3| world.get(cell).is_some_and(|voxel| registry.get(voxel.block).function != BlockFunction::None);
    let hub = motor + axis;
    if !cells.contains(&hub) || functional(hub) {
        return None;
    }
    let mut seen = HashSet::from([hub, motor]);
    let mut queue = VecDeque::from([hub]);
    let mut wheel = Vec::new();
    while let Some(cell) = queue.pop_front() {
        wheel.push(cell);
        if wheel.len() > game_rules().vehicles.max_rotor_cells {
            return None;
        }
        for offset in [IVec3::X, IVec3::NEG_X, IVec3::Y, IVec3::NEG_Y, IVec3::Z, IVec3::NEG_Z] {
            let next = cell + offset;
            if !cells.contains(&next) || !seen.insert(next) {
                continue;
            }
            if functional(next) {
                return None;
            }
            queue.push_back(next);
        }
    }
    Some(wheel)
}

/// Mean of the cell centres (the origin `assemble` gives a body).
fn cells_center(cells: &[IVec3]) -> Vec3 {
    cells.iter().map(|cell| cell.as_vec3() + Vec3::splat(0.5)).sum::<Vec3>() / cells.len().max(1) as f32
}

pub struct MovablePlugin;

impl Plugin for MovablePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RemoteLink>().add_systems(
            Update,
            (
                remote_link,
                assemble_on_interact,
                seat_interaction,
                movable_interactions,
                sync_body_motor_panel,
                drive_movables,
                run_movable_motors,
                liquid_push_movables,
                push_movables,
            )
                .chain()
                .after(PieSet::Transition)
                .after(PlayerControllerSet)
                .after(crate::gameplay::mining::MiningSet),
        )
        .add_systems(
            PostUpdate,
            stick_to_seat
                .after(PhysicsSet::Writeback)
                .before(crate::physics::player_controller::follow_player_camera),
        );
    }
}


/// Keep a seated player on the seat *after* the physics step, so the rider
/// follows the moving body in the same frame (no lag, no jitter) and the
/// character controller never fights the seat.
fn stick_to_seat(
    pie: Res<PieState>,
    bodies: Query<(&MovableBody, &Transform), Without<Player>>,
    mut players: Query<(&mut Player, &mut Transform, &mut KinematicCharacterController)>,
) {
    if !pie.is_playing() {
        return;
    }
    for (mut player, mut transform, mut controller) in &mut players {
        let Some(seated) = player.seated else {
            continue;
        };
        controller.translation = None;
        player.velocity = Vec3::ZERO;
        if let Some(block) = player.seat_block {
            transform.translation = block.as_vec3() + Vec3::splat(0.5) + Vec3::Y * game_rules().vehicles.seat_height;
        } else if let Ok((body, body_transform)) = bodies.get(seated) {
            let seat = body.seats.get(player.seat_index).or(body.seats.first()).copied().unwrap_or(Vec3::ZERO);
            transform.translation = body_transform.transform_point(seat) + body_transform.rotation * Vec3::Y * game_rules().vehicles.seat_height;
        }
    }
}

/// Flood fill from `start` through functional blocks.
pub fn flood_fill(world: &VoxelWorld, registry: &BlockRegistry, start: IVec3) -> Vec<IVec3> {
    let mut seen = HashSet::from([start]);
    let mut queue = VecDeque::from([start]);
    let mut cells = Vec::new();
    while let Some(cell) = queue.pop_front() {
        let Some(voxel) = world.get(cell) else {
            continue;
        };
        if registry.get(voxel.block).function == BlockFunction::None {
            continue;
        }
        cells.push(cell);
        if cells.len() >= game_rules().vehicles.max_movable_cells {
            break;
        }
        for offset in [IVec3::X, IVec3::NEG_X, IVec3::Y, IVec3::NEG_Y, IVec3::Z, IVec3::NEG_Z] {
            let next = cell + offset;
            if seen.insert(next) {
                queue.push_back(next);
            }
        }
    }
    cells
}

/// Functional blocks connected to `start` plus every solid block attached
/// to them at or above the lowest functional level. Returns the cells and
/// whether the attached blocks had to be left out (too many: terrain).
pub fn structure_cells(world: &VoxelWorld, registry: &BlockRegistry, start: IVec3) -> (Vec<IVec3>, bool) {
    let functional = flood_fill(world, registry, start);
    if functional.is_empty() {
        return (functional, false);
    }
    let base = functional.iter().map(|cell| cell.y).min().unwrap_or(start.y);
    let mut seen: HashSet<IVec3> = functional.iter().copied().collect();
    let mut queue: VecDeque<IVec3> = functional.iter().copied().collect();
    let mut attached = Vec::new();
    let attachable = |cell: IVec3| {
        cell.y >= base
            && world
                .get(cell)
                .is_some_and(|voxel| !registry.get(voxel.block).is_liquid() && registry.get(voxel.block).hardness < 1000.0)
    };
    let mut truncated = false;
    'flood: while let Some(cell) = queue.pop_front() {
        for offset in [IVec3::X, IVec3::NEG_X, IVec3::Y, IVec3::NEG_Y, IVec3::Z, IVec3::NEG_Z] {
            let next = cell + offset;
            if !seen.insert(next) || !attachable(next) {
                continue;
            }
            attached.push(next);
            if attached.len() > game_rules().vehicles.max_attached_cells || functional.len() + attached.len() > game_rules().vehicles.max_movable_cells {
                truncated = true;
                break 'flood;
            }
            queue.push_back(next);
        }
    }
    let mut cells = functional;
    if !truncated {
        cells.extend(attached);
    }
    // Blocks built by the player and connected to the structure always come
    // along (on top, on the sides, below: wheels...), even when the rest
    // touches the terrain.
    let mut seen: HashSet<IVec3> = cells.iter().copied().collect();
    let mut queue: VecDeque<IVec3> = cells.iter().copied().collect();
    let built = |cell: IVec3| {
        world.is_placed(cell) && world.get(cell).is_some_and(|voxel| !registry.get(voxel.block).is_liquid())
    };
    while let Some(cell) = queue.pop_front() {
        for offset in [IVec3::X, IVec3::NEG_X, IVec3::Y, IVec3::NEG_Y, IVec3::Z, IVec3::NEG_Z] {
            let next = cell + offset;
            if cells.len() >= game_rules().vehicles.max_movable_cells {
                break;
            }
            if seen.insert(next) && built(next) {
                cells.push(next);
                queue.push_back(next);
            }
        }
    }
    (cells, truncated)
}

/// Nearest of the 24 axis-aligned rotations.
fn snap_rotation(rotation: Quat) -> crate::voxel::shapes::Orientation {
    use crate::voxel::shapes::Orientation;
    (0..Orientation::COUNT as u8)
        .map(Orientation)
        .max_by(|a, b| {
            a.rotation()
                .dot(rotation)
                .abs()
                .total_cmp(&b.rotation().dot(rotation).abs())
        })
        .unwrap_or(Orientation::IDENTITY)
}

/// Orientation equal to `rotation * orientation` (both axis-aligned).
fn compose_orientation(rotation: Quat, orientation: crate::voxel::shapes::Orientation) -> crate::voxel::shapes::Orientation {
    snap_rotation(rotation * orientation.rotation())
}

#[allow(clippy::too_many_arguments)]
fn assemble(
    commands: &mut Commands,
    world: &mut VoxelWorld,
    registry: &BlockRegistry,
    cells: &[IVec3],
    render: &mut VoxelRenderState,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<PolyVoxelMaterial>,
    shading: &crate::core::app_state::ShadingSettings,
    motor_configs: &mut crate::gameplay::machines::motors::Motors,
) -> Option<Entity> {
    if cells.is_empty() {
        return None;
    }
    let center = cells
        .iter()
        .map(|cell| cell.as_vec3() + Vec3::splat(0.5))
        .sum::<Vec3>()
        / cells.len() as f32;

    // Temporary world in body-local integer space (min corner at the origin).
    let min = cells
        .iter()
        .copied()
        .reduce(|a, b| a.min(b))
        .unwrap_or(IVec3::ZERO);
    let mut local_world = VoxelWorld::new();
    let mut body_cells = Vec::with_capacity(cells.len());
    let mut voxels = Vec::with_capacity(cells.len());
    let mut shapes = Vec::with_capacity(cells.len());
    let mut motors = Vec::new();
    let mut seats = Vec::new();
    let mut has_controller = false;
    let mut has_engine = false;
    let mut forward = Vec3::NEG_Z;
    let mut forward_locked = false;

    for cell in cells {
        let Some(voxel) = world.get(*cell) else {
            continue;
        };
        let def = registry.get(voxel.block);
        let local_center = cell.as_vec3() + Vec3::splat(0.5) - center;
        local_world.set(*cell - min, Some(voxel));
        body_cells.push((local_center, voxel));
        voxels.push((*cell - min, voxel));

        let half = if def.shape.is_half() && voxel.sub.is_none() {
            // Slab collider aligned with the voxel facing.
            let up = voxel.orientation.apply_direction(Vec3::Y);
            let offset = -up * 0.25;
            let half_extents = Vec3::splat(0.5) - up.abs() * 0.25;
            (offset, half_extents)
        } else {
            (Vec3::ZERO, Vec3::splat(0.5))
        };
        shapes.push((
            local_center + half.0,
            Quat::IDENTITY,
            Collider::cuboid(half.1.x, half.1.y, half.1.z),
        ));

        match def.function {
            BlockFunction::Motor => {
                // Keep the settings made on the block before the launch.
                let config = motor_configs.motors.remove(cell).unwrap_or_default();
                motors.push(MotorMount {
                    local: local_center,
                    axis: voxel.orientation.apply_direction(Vec3::Y).round().normalize_or(Vec3::Y),
                    enabled: config.enabled,
                    rpm: config.rpm,
                    clockwise: config.clockwise,
                });
            }
            BlockFunction::Wagon => {
                seats.push(local_center);
                if !forward_locked {
                    forward = voxel.orientation.apply_direction(Vec3::Z);
                }
            }
            BlockFunction::Controller => {
                has_controller = true;
                forward = voxel.orientation.apply_direction(Vec3::Z);
                forward_locked = true;
            }
            BlockFunction::Combustor => has_engine = true,
            _ => {}
        }
        world.set(*cell, None);
    }
    if body_cells.is_empty() {
        return None;
    }

    let mass = body_cells.len() as f32;
    let body = commands
        .spawn((
            Name::new(format!("Movable ({} cells)", body_cells.len())),
            PieOnly,
            Transform::from_translation(center),
            Visibility::default(),
            RigidBody::Dynamic,
            Collider::compound(shapes),
            ColliderMassProperties::Density(1.0),
            Velocity::default(),
            ExternalForce::default(),
            ExternalImpulse::default(),
            Damping {
                linear_damping: 0.4,
                angular_damping: 1.2,
            },
            // Low friction: the motors and the controller slide it on the ground.
            Friction::coefficient(0.4),
            MovableBody {
                cells: body_cells.iter().map(|(center, _)| *center).collect(),
                voxels,
                grid_offset: min.as_vec3() - center,
                motors,
                seats,
                has_controller,
                has_engine,
                forward: Vec3::new(forward.x, 0.0, forward.z).normalize_or(Vec3::NEG_Z),
                mass,
                engine_timer: 0.0,
                powered_motors: 0,
            },
        ))
        .id();

    // Same toon materials as the static world.
    let chunk_coords: HashSet<IVec3> = local_world.chunks().map(|chunk| chunk.coord).collect();
    for coord in chunk_coords {
        let surfaces = mesher::build_chunk_mesh(&local_world, registry, coord, true);
        let chunk_origin = coord.as_vec3() * crate::voxel::world::CHUNK_SIZE as f32;
        for (block, data) in surfaces {
            let mesh = meshes.add(mesh_from_data(data));
            let material = material_handle(render, materials, registry, block, shading);
            commands.spawn((
                Mesh3d(mesh),
                MeshMaterial3d(material),
                Transform::from_translation(chunk_origin + min.as_vec3() - center),
                ChildOf(body),
            ));
        }
    }
    Some(body)
}

/// A kinematic body spun by a motor (see `gameplay::machines::motors`):
/// its voxels by centre relative to the pivot (to put them back in the world
/// when the motor stops).
#[derive(Component, Debug, Clone)]
pub struct RotorBody {
    pub voxels: Vec<(Vec3, crate::voxel::world::Voxel)>,
}

/// Lift `cells` out of the world into a kinematic body whose origin is
/// `pivot` (the motor's output face). Returns the body entity.
#[allow(clippy::too_many_arguments)]
pub fn assemble_rotor(
    commands: &mut Commands,
    world: &mut VoxelWorld,
    registry: &BlockRegistry,
    cells: &[IVec3],
    pivot: Vec3,
    motor: IVec3,
    render: &mut VoxelRenderState,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<PolyVoxelMaterial>,
    shading: &crate::core::app_state::ShadingSettings,
) -> Option<Entity> {
    if cells.is_empty() {
        return None;
    }
    let min = cells.iter().copied().reduce(|a, b| a.min(b)).unwrap_or(IVec3::ZERO);
    let mut local_world = VoxelWorld::new();
    let mut shapes = Vec::with_capacity(cells.len());
    let mut voxels = Vec::with_capacity(cells.len());
    for cell in cells {
        let Some(voxel) = world.get(*cell) else {
            continue;
        };
        let def = registry.get(voxel.block);
        local_world.set(*cell - min, Some(voxel));
        voxels.push((cell.as_vec3() + Vec3::splat(0.5) - pivot, voxel));
        if !def.is_liquid() {
            let local_center = cell.as_vec3() + Vec3::splat(0.5) - pivot;
            shapes.push((local_center, Quat::IDENTITY, Collider::cuboid(0.5, 0.5, 0.5)));
        }
        world.set(*cell, None);
    }
    if shapes.is_empty() {
        return None;
    }
    let body = commands
        .spawn((
            Name::new(format!("Rotor of motor ({}, {}, {}) - {} blocks", motor.x, motor.y, motor.z, cells.len())),
            PieOnly,
            RotorBody { voxels },
            Transform::from_translation(pivot),
            Visibility::default(),
            RigidBody::KinematicPositionBased,
            Collider::compound(shapes),
        ))
        .id();
    let chunk_coords: HashSet<IVec3> = local_world.chunks().map(|chunk| chunk.coord).collect();
    for coord in chunk_coords {
        let surfaces = mesher::build_chunk_mesh(&local_world, registry, coord, true);
        let chunk_origin = coord.as_vec3() * crate::voxel::world::CHUNK_SIZE as f32;
        for (block, data) in surfaces {
            let mesh = meshes.add(mesh_from_data(data));
            let material = material_handle(render, materials, registry, block, shading);
            commands.spawn((
                Mesh3d(mesh),
                MeshMaterial3d(material),
                Transform::from_translation(chunk_origin + min.as_vec3() - pivot),
                ChildOf(body),
            ));
        }
    }
    Some(body)
}

/// Put the blocks of a stopped motor rotor back into the world: the rotor
/// angle snaps to the nearest quarter turn around `axis` and every block lands
/// on its grid cell (blocked cells drop nothing: the block is kept only if
/// the cell is free). Returns (placed, blocked).
pub fn settle_rotor(world: &mut VoxelWorld, registry: &BlockRegistry, rotor: &RotorBody, pivot: Vec3, axis: Vec3, angle: f32) -> (usize, usize) {
    let quarter = std::f32::consts::FRAC_PI_2;
    let snapped = (angle / quarter).round() * quarter;
    let rotation = snap_rotation(Quat::from_axis_angle(axis, snapped)).rotation();
    let mut placed = 0;
    let mut blocked = 0;
    for (local, voxel) in &rotor.voxels {
        let target = (pivot + rotation * *local).floor().as_ivec3();
        let mut voxel = *voxel;
        voxel.orientation = compose_orientation(rotation, voxel.orientation);
        if let Some(mut sub) = voxel.sub {
            sub.orientation = compose_orientation(rotation, sub.orientation);
            voxel.sub = Some(sub);
        }
        if world.get(target).is_none_or(|existing| registry.get(existing.block).is_liquid()) {
            world.set(target, Some(voxel));
            world.mark_placed(target);
            placed += 1;
        } else {
            blocked += 1;
        }
    }
    (placed, blocked)
}

/// Every block attached to `start` (6-connected, liquids excluded), or
/// `None` when more than `limit` blocks are connected (touching the ground).
pub fn attached_blocks(world: &VoxelWorld, registry: &BlockRegistry, start: IVec3, exclude: IVec3, limit: usize) -> Option<Vec<IVec3>> {
    let solid = |cell: IVec3| world.get(cell).is_some_and(|voxel| !registry.get(voxel.block).is_liquid());
    if start == exclude || !solid(start) {
        return Some(Vec::new());
    }
    let mut seen = HashSet::from([start, exclude]);
    let mut queue = VecDeque::from([start]);
    let mut cells = Vec::new();
    while let Some(cell) = queue.pop_front() {
        cells.push(cell);
        if cells.len() > limit {
            return None;
        }
        for offset in [IVec3::X, IVec3::NEG_X, IVec3::Y, IVec3::NEG_Y, IVec3::Z, IVec3::NEG_Z] {
            let next = cell + offset;
            if seen.insert(next) && solid(next) {
                queue.push_back(next);
            }
        }
    }
    Some(cells)
}

#[allow(clippy::too_many_arguments)]
fn assemble_on_interact(
    mut commands: Commands,
    pie: Res<PieState>,
    requests: Res<crate::gameplay::mining::InteractRequests>,
    mut world: ResMut<VoxelWorld>,
    registry: Res<BlockRegistry>,
    mut render: ResMut<VoxelRenderState>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<PolyVoxelMaterial>>,
    mut ui: ResMut<EditorUiState>,
    mut motor_configs: ResMut<crate::gameplay::machines::motors::Motors>,
    mut link: ResMut<RemoteLink>,
) {
    if !pie.is_playing() {
        *link = RemoteLink::None;
        return;
    }
    for cell in requests.cells.clone() {
        let is_frame = world
            .get(cell)
            .map(|voxel| registry.get(voxel.block).function == BlockFunction::Movable)
            .unwrap_or(false);
        if !is_frame {
            continue;
        }
        let (cells, truncated) = structure_cells(&world, &registry, cell);
        let shading = ui.shading;
        // The blocks on each motor hub become a wheel of their own.
        let all: HashSet<IVec3> = cells.iter().copied().collect();
        let mut claimed: HashSet<IVec3> = HashSet::new();
        let mut rotors: Vec<(IVec3, IVec3, Vec<IVec3>)> = Vec::new();
        for &motor in &cells {
            let Some(voxel) = world.get(motor) else { continue };
            if registry.get(voxel.block).function != BlockFunction::Motor {
                continue;
            }
            let axis = voxel.orientation.apply_direction(Vec3::Y).round().as_ivec3();
            if let Some(wheel) = rotor_cells(&world, &registry, &all, motor, axis) {
                if wheel.iter().all(|cell| !claimed.contains(cell)) {
                    claimed.extend(wheel.iter().copied());
                    rotors.push((motor, axis, wheel));
                }
            }
        }
        let chassis: Vec<IVec3> = cells.iter().copied().filter(|cell| !claimed.contains(cell)).collect();
        let chassis_center = cells_center(&chassis);
        if let Some(body) = assemble(
            &mut commands,
            &mut world,
            &registry,
            &chassis,
            &mut render,
            &mut meshes,
            &mut materials,
            &shading,
            &mut motor_configs,
        ) {
            for (motor, axis, wheel) in &rotors {
                let wheel_center = cells_center(wheel);
                let Some(rotor) = assemble(
                    &mut commands,
                    &mut world,
                    &registry,
                    wheel,
                    &mut render,
                    &mut meshes,
                    &mut materials,
                    &shading,
                    &mut motor_configs,
                ) else {
                    continue;
                };
                let axis = axis.as_vec3();
                let motor_center = motor.as_vec3() + Vec3::splat(0.5);
                let pivot = motor_center + axis * 0.5;
                let mut joint = RevoluteJointBuilder::new(axis)
                    .local_anchor1(pivot - chassis_center)
                    .local_anchor2(pivot - wheel_center)
                    .motor_velocity(0.0, 0.0)
                    .build();
                joint.set_contacts_enabled(false);
                commands.entity(rotor).insert((
                    ImpulseJoint::new(body, joint),
                    RotorOf { body, motor_local: motor_center - chassis_center },
                    // Wheels grip the ground and spin freely.
                    Friction::coefficient(1.2),
                    Damping { linear_damping: 0.2, angular_damping: 0.05 },
                ));
            }
            if let RemoteLink::Cell(linked) = *link {
                if cells.contains(&linked) {
                    *link = RemoteLink::Body(body);
                }
            }
            if truncated {
                ui.warn(format!(
                    "Launched {} block(s): the structure touches the terrain, only its machines and the blocks you built came along.",
                    cells.len()
                ));
            } else {
                ui.action(format!("Launched a movable of {} block(s). E on a frame settles it back.", cells.len()));
            }
        }
    }
}

/// `E` on a controller (world or movable) with the Remote in hand connects
/// the Remote to it; `E` on the connected one disconnects it.
#[allow(clippy::too_many_arguments)]
fn remote_link(
    pie: Res<PieState>,
    input: GameInput,
    aim: Res<crate::gameplay::mining::PlayerAim>,
    held: Res<crate::gameplay::items::HeldTool>,
    world: Res<VoxelWorld>,
    registry: Res<BlockRegistry>,
    bodies: Query<(Entity, &MovableBody, &Transform)>,
    mut link: ResMut<RemoteLink>,
    mut ui: ResMut<EditorUiState>,
    mut sounds: ResMut<SoundQueue>,
) {
    if !pie.is_playing() {
        return;
    }
    // A connected body that no longer exists.
    if let RemoteLink::Body(entity) = *link {
        if bodies.get(entity).is_err() {
            *link = RemoteLink::None;
        }
    }
    if !held.remote || !pie.cursor_captured || !input.just_pressed(GameAction::Interact) {
        return;
    }
    let target = match aimed_cell(&aim, bodies.iter()) {
        Some((entity, index)) => bodies.get(entity).ok().and_then(|(_, body, _)| {
            let (_, voxel) = body.voxels.get(index)?;
            (registry.get(voxel.block).function == BlockFunction::Controller).then_some(RemoteLink::Body(entity))
        }),
        None => aim.hit.and_then(|hit| {
            world
                .get(hit.voxel)
                .is_some_and(|voxel| registry.get(voxel.block).function == BlockFunction::Controller)
                .then_some(RemoteLink::Cell(hit.voxel))
        }),
    };
    let Some(target) = target else {
        return;
    };
    if *link == target {
        *link = RemoteLink::None;
        ui.info("Remote disconnected from the controller.");
    } else {
        let replaced = *link != RemoteLink::None;
        *link = target;
        ui.action(if replaced {
            "Remote connected to this controller (the previous one is disconnected). Sprint + ZQSD drives it."
        } else {
            "Remote connected to this controller. Sprint + ZQSD drives it, E again disconnects."
        });
    }
    sounds.play_kind(SoundKind::UiClick);
}

/// Sit in a wagon seat with `E`: a seat of a launched movable (aimed, or the
/// nearest within reach) drives it; a seat of the static world just seats the
/// player. `E` again leaves.
#[allow(clippy::too_many_arguments)]
fn seat_interaction(
    mut commands: Commands,
    pie: Res<PieState>,
    input: GameInput,
    aim: Res<crate::gameplay::mining::PlayerAim>,
    world: Res<VoxelWorld>,
    registry: Res<BlockRegistry>,
    bodies: Query<(Entity, &MovableBody, &Transform), Without<Player>>,
    mut players: Query<(Entity, &mut Player, &mut Transform)>,
    mut ui: ResMut<EditorUiState>,
) {
    if !pie.is_playing() {
        return;
    }
    let Ok((player_entity, mut player, mut player_transform)) = players.single_mut() else {
        return;
    };
    let pressed = pie.cursor_captured && input.just_pressed(GameAction::Interact);

    if let Some(seated) = player.seated {
        let mut leave = pressed;
        if let Some(block) = player.seat_block {
            // Static seat: stay on it while it exists.
            let exists = world.get(block).is_some_and(|voxel| registry.get(voxel.block).function == BlockFunction::Wagon);
            if exists {
                player.velocity = Vec3::ZERO;
            } else {
                leave = true;
            }
        } else if bodies.get(seated).is_err() {
            leave = true;
        }
        // (The position on the seat is set after the physics step: `stick_to_seat`.)
        if leave {
            player.seated = None;
            player.seat_block = None;
            player.seat_index = 0;
            if pressed {
                player_transform.translation += Vec3::Y * 1.2;
                ui.info("Left the seat.");
            }
            commands.entity(player_entity).remove::<ColliderDisabled>();
        }
        return;
    }

    if !pressed {
        return;
    }
    // 1. The seat of a movable under the crosshair, else the nearest one.
    let mut target: Option<(Entity, usize)> = aimed_cell(&aim, bodies.iter()).and_then(|(entity, index)| {
        let (_, body, _) = bodies.get(entity).ok()?;
        let (_, voxel) = body.voxels.get(index)?;
        if registry.get(voxel.block).function != BlockFunction::Wagon {
            return None;
        }
        let local = body.cells[index];
        body.seats.iter().position(|seat| seat.distance(local) < 0.1).map(|seat| (entity, seat))
    });
    if target.is_none() && aim.hit.is_none() {
        'bodies: for (entity, body, transform) in &bodies {
            for (index, seat) in body.seats.iter().enumerate() {
                if transform.transform_point(*seat).distance(player_transform.translation) <= game_rules().vehicles.seat_reach {
                    target = Some((entity, index));
                    break 'bodies;
                }
            }
        }
    }
    if let Some((entity, index)) = target {
        player.seated = Some(entity);
        player.seat_index = index;
        player.seat_block = None;
        player.velocity = Vec3::ZERO;
        commands.entity(player_entity).insert(ColliderDisabled);
        ui.action("Seated: ZQSD/WASD drive, Space lifts, E to leave.");
        return;
    }
    // 2. A wagon seat of the static world.
    if let Some(hit) = aim.hit {
        let is_seat = world.get(hit.voxel).is_some_and(|voxel| registry.get(voxel.block).function == BlockFunction::Wagon);
        if is_seat {
            player.seated = Some(player_entity);
            player.seat_block = Some(hit.voxel);
            player.seat_index = 0;
            player.velocity = Vec3::ZERO;
            commands.entity(player_entity).insert(ColliderDisabled);
            ui.action("Seated. Launch the movable (E on a frame) to drive it. E to leave.");
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn drive_movables(
    time: Res<Time>,
    pie: Res<PieState>,
    intent: Res<PlayerIntent>,
    input: GameInput,
    grid: Res<EnergyGrid>,
    players: Query<(&Player, &Transform), Without<MovableBody>>,
    mut bodies: Query<(Entity, &mut MovableBody, &Transform, &Velocity, &mut ExternalForce)>,
    mut sounds: ResMut<SoundQueue>,
    held: Res<crate::gameplay::items::HeldTool>,
    link: Res<RemoteLink>,
) {
    if !pie.is_playing() {
        return;
    }
    let player = players.single().ok();
    let sneaking = input.pressed(GameAction::Sprint);
    let remote = held.remote && sneaking;

    // Remote target: the connected controller, else the nearest controlled body.
    let remote_target = if !remote {
        None
    } else if let RemoteLink::Body(entity) = *link {
        Some(entity)
    } else if let RemoteLink::Cell(_) = *link {
        // Connected to a controller that was not launched yet.
        None
    } else {
        player.and_then(|(_, player_transform)| {
            bodies
                .iter()
                .filter(|(_, body, ..)| body.has_controller)
                .map(|(entity, _, transform, ..)| {
                    (entity, transform.translation.distance(player_transform.translation))
                })
                .filter(|(_, distance)| *distance <= game_rules().vehicles.remote_range)
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(entity, _)| entity)
        })
    };

    for (entity, mut body, transform, velocity, mut force) in &mut bodies {
        force.force = Vec3::ZERO;
        force.torque = Vec3::ZERO;

        let driven = player.map(|(p, _)| p.seated == Some(entity)).unwrap_or(false)
            || remote_target == Some(entity);
        let powered = body
            .motors
            .iter()
            .filter(|motor| body.has_engine || grid.is_powered_at(transform.transform_point(motor.local)))
            .count();
        body.powered_motors = powered;
        if !driven {
            continue;
        }
        // Motors give the thrust; a controller alone drives at full ratio
        // (its own small thrusters), a seat alone needs motors.
        let ratio = if body.motors.is_empty() {
            if body.has_controller {
                1.0
            } else {
                continue;
            }
        } else {
            (powered as f32 / body.motors.len() as f32).max(if body.has_controller { 0.35 } else { 0.0 })
        };
        let forward = transform.rotation * body.forward;
        let up = transform.rotation * Vec3::Y;
        let throttle = intent.axis.y;
        let steer = -intent.axis.x;

        // Strong enough to beat the ground friction (0.4 * g * mass).
        force.force = forward * throttle * body.mass * 22.0 * ratio;
        force.torque = up * steer * body.mass * 9.0 * ratio;
        if intent.jump {
            force.force += Vec3::Y * body.mass * 14.0 * ratio;
        }

        // Engine hum slaved to speed.
        body.engine_timer -= time.delta_secs();
        if ratio > 0.0 && body.engine_timer <= 0.0 {
            body.engine_timer = 0.17;
            let speed = velocity.linear.length();
            sounds.play(
                SoundRequest::sfx(SoundKind::Engine {
                    rpm: 900.0 + speed * 260.0 + throttle.abs() * 600.0,
                })
                .with_volume(0.35),
            );
        }
    }
}

/// The body cell under the aiming ray (closer than any world block).
fn aimed_cell<'a>(
    aim: &crate::gameplay::mining::PlayerAim,
    bodies: impl Iterator<Item = (Entity, &'a MovableBody, &'a Transform)>,
) -> Option<(Entity, usize)> {
    if aim.ray_direction == Vec3::ZERO {
        return None;
    }
    let block_distance = aim.hit.map(|hit| hit.distance).unwrap_or(f32::MAX);
    let mut best: Option<(f32, Entity, usize)> = None;
    for (entity, body, transform) in bodies {
        let inverse = transform.compute_affine().inverse();
        let origin = inverse.transform_point3(aim.origin);
        let direction = inverse.transform_vector3(aim.ray_direction).normalize_or_zero();
        for (index, cell) in body.cells.iter().enumerate() {
            if let Some(distance) = ray_cell(origin, direction, *cell) {
                if distance <= aim.reach && distance < block_distance && best.is_none_or(|(d, ..)| distance < d) {
                    best = Some((distance, entity, index));
                }
            }
        }
    }
    best.map(|(_, entity, index)| (entity, index))
}

/// `E` on a launched movable: a motor opens its panel, a seat is left to
/// [`seat_interaction`], any other block settles the body back into the world.
#[allow(clippy::too_many_arguments)]
fn movable_interactions(
    mut commands: Commands,
    pie: Res<PieState>,
    input: GameInput,
    aim: Res<crate::gameplay::mining::PlayerAim>,
    mut world: ResMut<VoxelWorld>,
    registry: Res<BlockRegistry>,
    inventory: Res<Inventory>,
    players: Query<&Player>,
    bodies: Query<(Entity, &MovableBody, &Transform)>,
    mut panel: ResMut<crate::gameplay::machines::MachinePanel>,
    mut motor_configs: ResMut<crate::gameplay::machines::motors::Motors>,
    mut drops: ResMut<crate::gameplay::drops::DropQueue>,
    mut ui: ResMut<EditorUiState>,
    mut sounds: ResMut<SoundQueue>,
    (held, mut link, rotors): (Res<crate::gameplay::items::HeldTool>, ResMut<RemoteLink>, Query<(Entity, &RotorOf)>),
) {
    if !pie.is_playing() || !pie.cursor_captured || inventory.spectator || !input.just_pressed(GameAction::Interact) {
        return;
    }
    if players.single().is_ok_and(|player| player.seated.is_some()) {
        return;
    }
    let Some((entity, index)) = aimed_cell(&aim, bodies.iter()) else {
        return;
    };
    let Ok((_, body, _)) = bodies.get(entity) else {
        return;
    };
    let Some((_, voxel)) = body.voxels.get(index) else {
        return;
    };
    let local = body.cells[index];
    match registry.get(voxel.block).function {
        BlockFunction::Wagon => {}
        // The Remote connects to it (`remote_link`).
        BlockFunction::Controller if held.remote => {}
        BlockFunction::Motor => {
            if let Some(mount_index) = body.motors.iter().position(|mount| mount.local.distance(local) < 0.1) {
                let mount = &body.motors[mount_index];
                panel.open = None;
                panel.body_motor = Some(crate::gameplay::machines::BodyMotorEdit {
                    body: entity,
                    index: mount_index,
                    enabled: mount.enabled,
                    rpm: mount.rpm,
                    clockwise: mount.clockwise,
                    dirty: false,
                });
                panel.just_opened = true;
                sounds.play_kind(SoundKind::UiClick);
            }
        }
        _ => {
            // Settle the chassis and its wheels together: snap each to the
            // grid and give the blocks back to the world.
            let root = rotors.get(entity).map(|(_, rotor)| rotor.body).unwrap_or(entity);
            let mut group = vec![root];
            group.extend(rotors.iter().filter(|(_, rotor)| rotor.body == root).map(|(wheel, _)| wheel));
            let mut placed = 0;
            let mut blocked = 0;
            for member in group {
                let Ok((_, body, transform)) = bodies.get(member) else {
                    continue;
                };
                let snap = snap_rotation(transform.rotation);
                let rotation = snap.rotation();
                for (cell, voxel) in &body.voxels {
                    let local_center = body.grid_offset + cell.as_vec3() + Vec3::splat(0.5);
                    let world_center = transform.translation + rotation * local_center;
                    let target = world_center.floor().as_ivec3();
                    let mut voxel = *voxel;
                    voxel.orientation = compose_orientation(rotation, voxel.orientation);
                    if let Some(mut sub) = voxel.sub {
                        sub.orientation = compose_orientation(rotation, sub.orientation);
                        voxel.sub = Some(sub);
                    }
                    let free = world.get(target).is_none_or(|existing| registry.get(existing.block).is_liquid());
                    if free {
                        world.set(target, Some(voxel));
                        world.mark_placed(target);
                        placed += 1;
                        if *link == RemoteLink::Body(member) && registry.get(voxel.block).function == BlockFunction::Controller {
                            *link = RemoteLink::Cell(target);
                        }
                        if registry.get(voxel.block).function == BlockFunction::Motor {
                            if let Some(mount) = body.motors.iter().find(|mount| mount.local.distance(local_center) < 0.1) {
                                let config = motor_configs.config_mut(target);
                                config.enabled = mount.enabled;
                                config.rpm = mount.rpm;
                                config.clockwise = mount.clockwise;
                            }
                        }
                    } else {
                        blocked += 1;
                        if inventory.survival {
                            drops.spawn(world_center, registry.get(voxel.block).resource_id(), 1);
                        }
                    }
                }
                if panel.body_motor.as_ref().is_some_and(|edit| edit.body == member) {
                    panel.body_motor = None;
                }
                commands.entity(member).despawn();
            }
            sounds.play_kind(SoundKind::BlockPlace);
            if blocked > 0 {
                ui.warn(format!("Movable settled: {placed} block(s) placed, {blocked} blocked by the terrain."));
            } else {
                ui.action(format!("Movable settled back into {placed} solid block(s)."));
            }
        }
    }
}

/// Apply the edits of the on-board motor panel and close it when the body
/// is gone or far away.
fn sync_body_motor_panel(
    pie: Res<PieState>,
    mut panel: ResMut<crate::gameplay::machines::MachinePanel>,
    mut bodies: Query<(&mut MovableBody, &Transform), Without<Player>>,
    players: Query<&Transform, With<Player>>,
) {
    if !pie.is_playing() {
        panel.body_motor = None;
        return;
    }
    let Some(edit) = panel.body_motor.as_mut() else {
        return;
    };
    let Ok((mut body, transform)) = bodies.get_mut(edit.body) else {
        panel.body_motor = None;
        return;
    };
    let far = players.single().is_ok_and(|player| player.translation.distance(transform.translation) > 14.0);
    let Some(mount) = body.motors.get_mut(edit.index) else {
        panel.body_motor = None;
        return;
    };
    if edit.dirty {
        mount.enabled = edit.enabled;
        mount.rpm = edit.rpm;
        mount.clockwise = edit.clockwise;
        edit.dirty = false;
    }
    if far {
        panel.body_motor = None;
    }
}

/// Running motors spin their wheel (the blocks on the hub); a motor with
/// nothing on its hub pushes its body along the hub axis instead.
fn run_movable_motors(
    mut commands: Commands,
    pie: Res<PieState>,
    inventory: Res<Inventory>,
    grid: Res<EnergyGrid>,
    mut bodies: Query<(Entity, &MovableBody, &Transform, &Velocity, &mut ExternalForce), Without<RotorOf>>,
    mut rotors: Query<(Entity, &RotorOf, &mut ImpulseJoint, &Transform)>,
) {
    if !pie.is_playing() {
        return;
    }
    let mut spinning: HashSet<(Entity, IVec3)> = HashSet::new();
    let key = |local: Vec3| (local * 2.0).round().as_ivec3();
    for (entity, rotor, mut joint, wheel_transform) in &mut rotors {
        let Ok((parent, body, transform, _, _)) = bodies.get(rotor.body) else {
            // The chassis is gone: the wheel becomes a loose movable.
            commands.entity(entity).remove::<(ImpulseJoint, RotorOf)>();
            continue;
        };
        spinning.insert((parent, key(rotor.motor_local)));
        let Some(mount) = body.motors.iter().find(|mount| mount.local.distance(rotor.motor_local) < 0.1) else {
            continue;
        };
        let at = transform.transform_point(mount.local);
        let powered = !inventory.survival || body.has_engine || grid.is_powered_at(at);
        let running = mount.enabled && mount.rpm > 0.0 && powered;
        let sign = if mount.clockwise { -1.0 } else { 1.0 };
        let target = if running { sign * mount.rpm * std::f32::consts::TAU / 60.0 } else { 0.0 };
        // Strong enough to turn the wheel and roll the vehicle; free when off.
        // (acceleration-based motor model: the factor does not depend on the mass)
        let factor = if running { 8.0 } else { 0.0 };
        if let TypedJoint::RevoluteJoint(revolute) = &mut joint.data {
            if running {
                revolute.set_motor_velocity(target, factor);
            } else {
                // Stopped: the wheel turns to the nearest quarter turn, so
                // its blocks line up with the chassis (and the world grid
                // once the movable settles).
                let relative = transform.rotation.inverse() * wheel_transform.rotation;
                let along = Vec3::new(relative.x, relative.y, relative.z).dot(mount.axis);
                let angle = 2.0 * along.atan2(relative.w);
                let angle = (angle + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
                let quarter = std::f32::consts::FRAC_PI_2;
                let aligned = (angle / quarter).round() * quarter;
                revolute.set_motor_position(aligned, 40.0, 8.0);
            }
        }
    }
    for (entity, body, transform, velocity, mut force) in &mut bodies {
        for mount in body.motors.iter().filter(|mount| mount.enabled && mount.rpm > 0.0) {
            if spinning.contains(&(entity, key(mount.local))) {
                continue;
            }
            let at = transform.transform_point(mount.local);
            let powered = !inventory.survival || body.has_engine || grid.is_powered_at(at);
            if !powered {
                continue;
            }
            let sign = if mount.clockwise { 1.0 } else { -1.0 };
            let axis = (transform.rotation * mount.axis).normalize_or_zero() * sign;
            let throttle = (mount.rpm / game_rules().vehicles.max_rpm).clamp(0.0, 1.0);
            // Top speed grows with the motor speed.
            let top = 4.0 + 14.0 * throttle;
            let along = velocity.linear.dot(axis);
            let factor = (1.0 - along / top).clamp(0.0, 1.0);
            // Even a slow motor beats the ground friction (0.4 * g * mass).
            let share = body.mass / body.motors.len().max(1) as f32;
            let push = if axis.y.abs() > 0.7 {
                // Hub pointing up or down: a lift rotor (hovers near half speed).
                9.81 * (0.5 + 0.9 * throttle)
            } else {
                8.0 + 22.0 * throttle
            };
            force.force += axis * share * push * factor;
        }
    }
}

/// Horizontal flow of the liquid in `cell` (towards the less full
/// neighbours; falling liquid pushes down). Zero in still water.
pub fn liquid_flow(world: &VoxelWorld, cell: IVec3) -> Vec3 {
    let Some(block) = world.liquid_at(cell) else {
        return Vec3::ZERO;
    };
    let level = world.fluid_level(cell);
    if level == crate::voxel::world::FLUID_FALLING {
        return Vec3::NEG_Y;
    }
    let mut flow = Vec3::ZERO;
    for offset in [IVec3::X, IVec3::NEG_X, IVec3::Z, IVec3::NEG_Z] {
        let next = cell + offset;
        let direction = offset.as_vec3();
        match world.liquid_at(next) {
            Some(other) if other == block => {
                let other_level = world.fluid_level(next);
                let other_level = if other_level == crate::voxel::world::FLUID_FALLING { 0 } else { other_level };
                flow += direction * (other_level as f32 - level as f32);
            }
            Some(_) => {}
            // Liquid pouring over an edge.
            None if world.get(next).is_none() && level > 0 => flow += direction * 2.0,
            None => {}
        }
    }
    flow.normalize_or_zero()
}

/// Liquids carry movables: every submerged cell floats (buoyancy), is slowed
/// by the liquid (drag) and pushed along the flow of the current.
fn liquid_push_movables(
    pie: Res<PieState>,
    world: Res<VoxelWorld>,
    mut bodies: Query<(&MovableBody, &Transform, &Velocity, &mut ExternalForce)>,
) {
    if !pie.is_playing() {
        return;
    }
    for (body, transform, velocity, mut force) in &mut bodies {
        let mut submerged = 0usize;
        let mut push = Vec3::ZERO;
        let mut torque = Vec3::ZERO;
        for local in &body.cells {
            let at = transform.transform_point(*local);
            let cell = at.floor().as_ivec3();
            if world.liquid_at(cell).is_none() {
                continue;
            }
            submerged += 1;
            let cell_velocity = velocity.linear + velocity.angular.cross(at - transform.translation);
            let flow = liquid_flow(&world, cell);
            let cell_force = Vec3::Y * game_rules().vehicles.buoyancy + flow * game_rules().vehicles.current - cell_velocity * game_rules().vehicles.water_drag;
            push += cell_force;
            torque += (at - transform.translation).cross(cell_force);
        }
        if submerged == 0 {
            continue;
        }
        force.force += push;
        force.torque += torque * 0.5 - velocity.angular * (submerged as f32 * 0.3);
    }
}

/// Ray / unit-cube intersection in body-local space; returns the distance.
fn ray_cell(origin: Vec3, direction: Vec3, center: Vec3) -> Option<f32> {
    let min = center - Vec3::splat(0.5);
    let max = center + Vec3::splat(0.5);
    let inv = direction.recip();
    let t1 = (min - origin) * inv;
    let t2 = (max - origin) * inv;
    let near = t1.min(t2).max_element();
    let far = t1.max(t2).min_element();
    (far >= near.max(0.0)).then_some(near.max(0.0))
}

/// Distance from a point to the nearest cell of a body (local space).
fn distance_to_cells(point: Vec3, cells: &[Vec3]) -> f32 {
    cells
        .iter()
        .map(|center| {
            let d = ((point - *center).abs() - Vec3::splat(0.5)).max(Vec3::ZERO);
            d.length()
        })
        .fold(f32::MAX, f32::min)
}

/// Push movables: hit them (Attack) to shove them, or walk into them.
#[allow(clippy::too_many_arguments)]
fn push_movables(
    time: Res<Time>,
    pie: Res<PieState>,
    input: GameInput,
    intent: Res<PlayerIntent>,
    aim: Res<crate::gameplay::mining::PlayerAim>,
    players: Query<(&Player, &Transform), Without<MovableBody>>,
    mut bodies: Query<(&MovableBody, &Transform, &Velocity, &mut ExternalImpulse)>,
    mut sounds: ResMut<SoundQueue>,
) {
    if !pie.is_playing() || !pie.cursor_captured {
        return;
    }
    let Ok((player, player_transform)) = players.single() else {
        return;
    };
    if player.seated.is_some() {
        return;
    }
    let delta = time.delta_secs().min(0.05);

    // --- Hit ---------------------------------------------------------------
    if input.just_pressed(GameAction::Attack) && aim.ray_direction != Vec3::ZERO {
        let block_distance = aim.hit.map(|hit| hit.distance).unwrap_or(f32::MAX);
        let mut best: Option<(f32, usize)> = None;
        let candidates: Vec<_> = bodies.iter().map(|(body, transform, ..)| (body.cells.clone(), *transform)).collect();
        for (index, (cells, transform)) in candidates.iter().enumerate() {
            let inverse = transform.compute_affine().inverse();
            let origin = inverse.transform_point3(aim.origin);
            let direction = inverse.transform_vector3(aim.ray_direction).normalize_or_zero();
            for cell in cells {
                if let Some(distance) = ray_cell(origin, direction, *cell) {
                    if distance <= aim.reach && distance < block_distance && best.is_none_or(|(d, _)| distance < d) {
                        best = Some((distance, index));
                    }
                }
            }
        }
        if let Some((distance, index)) = best {
            if let Some((body, transform, _, mut impulse)) = bodies.iter_mut().nth(index) {
                let mass = body.mass.max(1.0);
                let flat = Vec3::new(aim.ray_direction.x, 0.0, aim.ray_direction.z).normalize_or_zero();
                let strength = mass * 3.2 / (1.0 + mass / 40.0);
                let j = (flat + Vec3::Y * 0.18) * strength;
                let point = aim.origin + aim.ray_direction * distance;
                impulse.impulse += j;
                impulse.torque_impulse += (point - transform.translation).cross(j) * 0.25;
                sounds.play(SoundRequest::sfx(SoundKind::Impact).with_volume(0.6));
            }
        }
    }

    // --- Walk into it ------------------------------------------------------
    if intent.axis == Vec2::ZERO || intent.remote {
        return;
    }
    let forward = player.forward();
    let right = Vec3::new(-forward.z, 0.0, forward.x);
    let wish = (forward * intent.axis.y + right * intent.axis.x).normalize_or_zero();
    let center = player_transform.translation;
    for (body, transform, velocity, mut impulse) in &mut bodies {
        if transform.translation.distance(center) > 64.0 {
            continue;
        }
        let inverse = transform.compute_affine().inverse();
        let touching = [-0.8_f32, 0.0, 0.8].iter().any(|dy| {
            let probe = inverse.transform_point3(center + Vec3::Y * *dy + wish * 0.35);
            distance_to_cells(probe, &body.cells) < 0.5
        });
        if !touching {
            continue;
        }
        // Only push while the body is slower than a walk in that direction.
        if velocity.linear.dot(wish) > 4.0 {
            continue;
        }
        let mass = body.mass.max(1.0);
        let accel = 22.0 / (1.0 + mass / 120.0);
        impulse.impulse += wish * accel * mass * delta;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::voxel::world::Voxel;

    #[test]
    fn flood_fill_ignores_terrain() {
        let registry = BlockRegistry::builtin();
        let frame = registry.find("frame").expect("frame");
        let motor = registry.find("motor").expect("motor");
        let stone = registry.find("stone").expect("stone");
        let mut world = VoxelWorld::new();
        world.set(IVec3::ZERO, Some(Voxel::new(frame)));
        world.set(IVec3::X, Some(Voxel::new(motor)));
        world.set(IVec3::NEG_Y, Some(Voxel::new(stone)));
        world.set(IVec3::new(0, -1, 1), Some(Voxel::new(stone)));

        let cells = flood_fill(&world, &registry, IVec3::ZERO);
        assert_eq!(cells.len(), 2);
        assert!(!cells.contains(&IVec3::NEG_Y));
    }

    #[test]
    fn built_blocks_follow_a_launch_even_against_terrain() {
        let registry = BlockRegistry::builtin();
        let frame = registry.find("frame").expect("frame");
        let stone = registry.find("stone").expect("stone");
        let mut world = VoxelWorld::new();
        world.set(IVec3::ZERO, Some(Voxel::new(frame)));
        // A hill at the frame level: too big to be carried.
        for x in 2..40 {
            for z in -20..20 {
                for y in 0..4 {
                    world.set(IVec3::new(x, y, z), Some(Voxel::new(stone)));
                }
            }
        }
        // Built by the player: on top, on the side and under the frame.
        for cell in [IVec3::Y, IVec3::new(0, 1, 1), IVec3::NEG_X, IVec3::new(-1, -1, 0)] {
            world.set(cell, Some(Voxel::new(stone)));
            world.mark_placed(cell);
        }
        // The side block touches the hill.
        world.set(IVec3::X, Some(Voxel::new(stone)));
        world.mark_placed(IVec3::X);
        let (cells, truncated) = structure_cells(&world, &registry, IVec3::ZERO);
        assert!(truncated);
        for cell in [IVec3::ZERO, IVec3::Y, IVec3::new(0, 1, 1), IVec3::NEG_X, IVec3::new(-1, -1, 0), IVec3::X] {
            assert!(cells.contains(&cell), "missing {cell}");
        }
        assert!(!cells.contains(&IVec3::new(2, 0, 0)));
    }
}
