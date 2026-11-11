//! Animated parts of the blocks during play.
//!
//! Every block may carry extra parts (`BlockDef::parts`, edited in the Forge
//! like any other block data): coloured boxes that spin, float, follow a
//! motor or light up while the machine is powered. This module draws them on
//! the blocks of the world and on the blocks of launched movables. Nothing
//! here depends on a particular block: the engine defaults of the machines are
//! plain data saved in the block files.

use std::collections::{HashMap, HashSet};

use bevy::light::NotShadowCaster;
use bevy::prelude::*;

use crate::core::pie_manager::{PieOnly, PieState};
use crate::gameplay::energy_grid::EnergyGrid;
use crate::gameplay::machines::motors::Motors;
use crate::physics::movable_compound::MovableBody;
use crate::voxel::blocks::{BlockId, BlockPart, BlockRegistry, PartMotion};
use crate::voxel::world::{VoxelWorld, CHUNK_SIZE};

/// Root of the parts of one world cell.
#[derive(Component)]
pub struct MachineVisual;

/// Parts of a launched movable were spawned.
#[derive(Component)]
pub struct BodyPartsSpawned;

/// Where an animated part reads its state.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PartSource {
    Cell(IVec3),
    /// A cell of a movable body; `local` is its centre in body space.
    Body { body: Entity, local: Vec3 },
}

/// Pivot of an animated part.
#[derive(Component)]
pub struct AnimatedPart {
    source: PartSource,
    spin: f32,
    axis: Vec3,
    bob: f32,
    bob_axis: Vec3,
    swing: f32,
    pulse: f32,
    speed: f32,
    motion: PartMotion,
    base: Vec3,
    angle: f32,
    phase: f32,
}

/// Part whose colour changes while powered.
#[derive(Component)]
pub struct PoweredLook {
    source: PartSource,
    off: Handle<StandardMaterial>,
    on: Handle<StandardMaterial>,
}

#[derive(Resource, Default)]
pub struct MachineVisuals {
    spawned: HashMap<IVec3, (Entity, BlockId)>,
    scanned: Option<(u64, u64)>,
    cube: Option<Handle<Mesh>>,
    materials: HashMap<(u32, u32, u32, u32, u32), Handle<StandardMaterial>>,
    clock: f32,
}

impl MachineVisuals {
    fn material(&mut self, materials: &mut Assets<StandardMaterial>, color: [u8; 3], emissive: f32, metallic: f32) -> Handle<StandardMaterial> {
        let key = (color[0] as u32, color[1] as u32, color[2] as u32, (emissive * 100.0) as u32, (metallic * 100.0) as u32);
        self.materials
            .entry(key)
            .or_insert_with(|| {
                let base = Color::srgb_u8(color[0], color[1], color[2]);
                materials.add(StandardMaterial {
                    base_color: base,
                    emissive: base.to_linear() * emissive,
                    metallic,
                    perceptual_roughness: 0.45,
                    ..default()
                })
            })
            .clone()
    }
}

/// Spawn the parts of one block under `parent`, centred at `center` with the
/// block `rotation`.
#[allow(clippy::too_many_arguments)]
fn spawn_parts(
    commands: &mut Commands,
    state: &mut MachineVisuals,
    materials: &mut Assets<StandardMaterial>,
    cube: &Handle<Mesh>,
    parent: Option<Entity>,
    center: Vec3,
    rotation: Quat,
    parts: &[BlockPart],
    source: PartSource,
) -> Entity {
    let mut root = commands.spawn((
        Name::new("Block parts"),
        MachineVisual,
        PieOnly,
        Transform::from_translation(center).with_rotation(rotation),
        Visibility::Inherited,
    ));
    if let Some(parent) = parent {
        root.insert(ChildOf(parent));
    }
    let root = root.id();
    for part in parts {
        let (part_center, size) = part.center_size();
        let pivot = part.pivot_point();
        let animated = part.is_animated();
        let holder = if animated {
            commands
                .spawn((
                    Transform::from_translation(pivot),
                    Visibility::Inherited,
                    AnimatedPart {
                        source,
                        spin: part.spin,
                        axis: Vec3::from_array(part.spin_axis).normalize_or(Vec3::Y),
                        bob: part.bob,
                        bob_axis: Vec3::from_array(part.bob_axis).normalize_or(Vec3::Y),
                        swing: part.swing.to_radians(),
                        pulse: part.pulse,
                        speed: part.speed,
                        motion: part.motion,
                        base: pivot,
                        angle: 0.0,
                        phase: part.phase + center.x * 0.3 + center.z * 0.17,
                    },
                    ChildOf(root),
                ))
                .id()
        } else {
            root
        };
        let offset = if animated { part_center - pivot } else { part_center };
        let material = state.material(materials, part.color, part.emissive, part.metallic);
        let mut entity = commands.spawn((
            Mesh3d(cube.clone()),
            MeshMaterial3d(material.clone()),
            Transform::from_translation(offset).with_rotation(part.rotation_quat()).with_scale(size),
            NotShadowCaster,
            ChildOf(holder),
        ));
        if let Some(on) = part.powered_color {
            let lit = state.material(materials, on, part.powered_emissive, part.metallic);
            entity.insert(PoweredLook { source, off: material, on: lit });
        }
    }
    root
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn sync_machine_visuals(
    mut commands: Commands,
    time: Res<Time>,
    pie: Res<PieState>,
    grid: Res<EnergyGrid>,
    world: Res<VoxelWorld>,
    registry: Res<BlockRegistry>,
    motors: Res<Motors>,
    mut state: ResMut<MachineVisuals>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    bodies: Query<(Entity, &MovableBody, Has<BodyPartsSpawned>)>,
    mut animated: Query<(&mut AnimatedPart, &mut Transform)>,
    mut looks: Query<(&PoweredLook, &mut MeshMaterial3d<StandardMaterial>)>,
) {
    if pie.started_this_frame || pie.stopped_this_frame {
        state.spawned.clear();
        state.scanned = None;
    }
    if !pie.is_playing() {
        return;
    }
    let delta = time.delta_secs();
    state.clock += delta;
    let cube = state.cube.get_or_insert_with(|| meshes.add(Cuboid::new(1.0, 1.0, 1.0))).clone();

    // --- World cells: rescan when the world or the blocks change ---------------
    let signature = (world.revision, registry.revision);
    if state.scanned != Some(signature) {
        state.scanned = Some(signature);
        let with_parts: HashSet<BlockId> = registry
            .blocks()
            .filter(|(_, def)| def.parts.as_ref().is_some_and(|parts| !parts.is_empty()))
            .map(|(id, _)| id)
            .collect();
        let mut wanted: HashMap<IVec3, (BlockId, Quat)> = HashMap::new();
        if !with_parts.is_empty() {
            for chunk in world.chunks() {
                let origin = chunk.origin();
                for index in 0..CHUNK_SIZE * CHUNK_SIZE * CHUNK_SIZE {
                    let local = crate::voxel::world::VoxelChunk::local_from_index(index);
                    let Some(voxel) = chunk.get(local) else {
                        continue;
                    };
                    if voxel.sub.is_none() && with_parts.contains(&voxel.block) {
                        wanted.insert(origin + local.as_ivec3(), (voxel.block, voxel.orientation.rotation()));
                    }
                }
            }
        }
        let stale: Vec<IVec3> = state
            .spawned
            .iter()
            .filter(|(cell, (_, block))| wanted.get(cell).is_none_or(|(id, _)| id != block))
            .map(|(cell, _)| *cell)
            .collect();
        for cell in stale {
            if let Some((entity, _)) = state.spawned.remove(&cell) {
                commands.entity(entity).despawn();
            }
        }
        for (cell, (block, rotation)) in wanted {
            if state.spawned.contains_key(&cell) {
                continue;
            }
            let Some(parts) = registry.get(block).parts.clone() else {
                continue;
            };
            let root = spawn_parts(
                &mut commands,
                &mut state,
                &mut materials,
                &cube,
                None,
                cell.as_vec3() + Vec3::splat(0.5),
                rotation,
                &parts,
                PartSource::Cell(cell),
            );
            state.spawned.insert(cell, (root, block));
        }
    }

    // --- Launched movables ---------------------------------------------------------
    for (entity, body, spawned) in &bodies {
        if spawned {
            continue;
        }
        commands.entity(entity).insert(BodyPartsSpawned);
        for (cell, voxel) in &body.voxels {
            let Some(parts) = registry.get(voxel.block).parts.clone().filter(|parts| !parts.is_empty()) else {
                continue;
            };
            let local = body.grid_offset + cell.as_vec3() + Vec3::splat(0.5);
            spawn_parts(
                &mut commands,
                &mut state,
                &mut materials,
                &cube,
                Some(entity),
                local,
                voxel.orientation.rotation(),
                &parts,
                PartSource::Body { body: entity, local },
            );
        }
    }

    // --- Animation ---------------------------------------------------------------------
    let powered = |source: PartSource| match source {
        PartSource::Cell(cell) => grid.machine(cell).is_some_and(|machine| machine.powered || machine.fuel > 0.0),
        PartSource::Body { .. } => true,
    };
    // Motor speed (rad/s) of a source.
    let motor_speed = |source: PartSource| -> Option<f32> {
        match source {
            PartSource::Cell(cell) => motors.motors.get(&cell).map(|config| if config.enabled { config.angular_speed() } else { 0.0 }),
            PartSource::Body { body, local } => bodies.get(body).ok().and_then(|(_, body, _)| {
                body.motors.iter().find(|mount| mount.local.distance(local) < 0.1).map(|mount| {
                    if mount.enabled && mount.rpm > 0.0 {
                        let sign = if mount.clockwise { -1.0 } else { 1.0 };
                        sign * mount.rpm / 60.0 * std::f32::consts::TAU
                    } else {
                        0.0
                    }
                })
            }),
        }
    };
    let clock = state.clock;
    for (mut part, mut transform) in &mut animated {
        let (active, speed) = match part.motion {
            PartMotion::Always => (true, part.spin),
            PartMotion::Powered => (powered(part.source), part.spin),
            PartMotion::Motor => {
                let speed = motor_speed(part.source).unwrap_or(0.0) * part.spin;
                (speed != 0.0, speed)
            }
        };
        // A world motor turns with its rotor: use its angle.
        if let (PartMotion::Motor, PartSource::Cell(cell)) = (part.motion, part.source) {
            part.angle = motors.motors.get(&cell).map(|config| config.angle * part.spin).unwrap_or(part.angle);
        } else if active {
            part.angle = (part.angle + speed * delta).rem_euclid(std::f32::consts::TAU);
        }
        let wave = if active { (clock * part.speed + part.phase).sin() } else { 0.0 };
        transform.rotation = Quat::from_axis_angle(part.axis, part.angle + wave * part.swing);
        transform.translation = part.base + part.bob_axis * (wave * part.bob);
        transform.scale = Vec3::splat(1.0 + wave * part.pulse);
    }
    for (look, mut material) in &mut looks {
        let wanted = if powered(look.source) { &look.on } else { &look.off };
        if material.0 != *wanted {
            material.0 = wanted.clone();
        }
    }
}
