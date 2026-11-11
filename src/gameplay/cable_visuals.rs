//! Cables and ports drawn as real meshes.
//!
//! - **Ports** are small pegs on top of the blocks that declare them
//!   (`BlockDef::ports`): their *shape* gives the kind (Energy = square,
//!   Resource = triangle, Signal = hexagon), their *colour* the direction
//!   (input = green, output = red). A cable always goes from a red output to a
//!   green input of the same shape.
//! - **Cables** are sagging tubes tinted by their kind, with a glowing pulse
//!   running from the output to the input while playing.
//!
//! Everything comes from the block data and the cable network: no block is
//! special-cased here.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use bevy::asset::RenderAssetUsages;
use bevy::light::NotShadowCaster;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

use crate::core::editor_camera::EditorCamera;
use crate::core::pie_manager::PieState;
use crate::gameplay::data_cables::{port_position, ports_at, CableNetwork};
use crate::voxel::blocks::{BlockRegistry, PortDirection, PortKind};
use crate::voxel::world::VoxelWorld;

/// Ports are drawn on the blocks within this distance of the camera.
const PORT_DRAW_RADIUS: i32 = 12;
const CABLE_RADIUS: f32 = 0.035;
const CABLE_SIDES: usize = 7;
/// Radius and height of a port peg.
const PEG_RADIUS: f32 = 0.11;
const PEG_HEIGHT: f32 = 0.07;

/// Colour of an input port (green) / output port (red).
pub const INPUT_RGB: [u8; 3] = [70, 220, 110];
pub const OUTPUT_RGB: [u8; 3] = [235, 70, 70];

/// Number of sides of the peg of a port kind.
pub const fn kind_sides(kind: PortKind) -> usize {
    match kind {
        PortKind::Energy => 4,
        PortKind::Resource => 3,
        PortKind::Signal => 6,
    }
}

/// Shape name of a port kind (UI).
pub const fn kind_shape(kind: PortKind) -> &'static str {
    match kind {
        PortKind::Energy => "square",
        PortKind::Resource => "triangle",
        PortKind::Signal => "hexagon",
    }
}

#[derive(Component)]
pub struct CableMesh;

#[derive(Component)]
pub struct PortPeg;

/// Pulse running along a cable.
#[derive(Component)]
pub struct CablePulse {
    points: Vec<Vec3>,
    phase: f32,
}

#[derive(Resource, Default)]
pub struct CableVisuals {
    cables_signature: Option<u64>,
    ports_signature: Option<u64>,
    cables: Vec<Entity>,
    ports: Vec<Entity>,
    pegs: HashMap<usize, Handle<Mesh>>,
    pulse: Option<Handle<Mesh>>,
    materials: HashMap<([u8; 3], bool), Handle<StandardMaterial>>,
}

impl CableVisuals {
    fn material(&mut self, materials: &mut Assets<StandardMaterial>, rgb: [u8; 3], glow: bool) -> Handle<StandardMaterial> {
        self.materials
            .entry((rgb, glow))
            .or_insert_with(|| {
                let color = Color::srgb_u8(rgb[0], rgb[1], rgb[2]);
                materials.add(StandardMaterial {
                    base_color: color,
                    emissive: color.to_linear() * if glow { 3.0 } else { 0.25 },
                    perceptual_roughness: 0.35,
                    metallic: if glow { 0.0 } else { 0.3 },
                    ..default()
                })
            })
            .clone()
    }
}

pub struct CableVisualsPlugin;

impl Plugin for CableVisualsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CableVisuals>().add_systems(Update, (sync_cable_visuals, animate_pulses).chain());
    }
}

/// Points of the sagging curve of a cable.
pub fn cable_curve(a: Vec3, b: Vec3) -> Vec<Vec3> {
    let span = a.distance(b);
    let sag = (span * 0.12).min(3.0);
    let segments = ((span * 3.0) as usize).clamp(8, 64);
    (0..=segments)
        .map(|step| {
            let t = step as f32 / segments as f32;
            let mut point = a.lerp(b, t);
            point.y -= 4.0 * t * (1.0 - t) * sag;
            point
        })
        .collect()
}

/// A round tube following `points`.
pub fn tube_mesh(points: &[Vec3], radius: f32, sides: usize) -> Mesh {
    let mut positions = Vec::with_capacity(points.len() * sides);
    let mut normals = Vec::with_capacity(points.len() * sides);
    let mut uvs = Vec::with_capacity(points.len() * sides);
    let mut indices = Vec::new();
    let mut previous_normal: Option<Vec3> = None;
    for (index, point) in points.iter().enumerate() {
        let tangent = if index + 1 < points.len() {
            points[index + 1] - *point
        } else if index > 0 {
            *point - points[index - 1]
        } else {
            Vec3::X
        }
        .normalize_or(Vec3::X);
        // Parallel transport of the ring frame (no twisting).
        let normal = match previous_normal {
            Some(previous) => (previous - tangent * previous.dot(tangent)).normalize_or(tangent.any_orthonormal_vector()),
            None => tangent.any_orthonormal_vector(),
        };
        previous_normal = Some(normal);
        let binormal = tangent.cross(normal);
        for side in 0..sides {
            let angle = side as f32 / sides as f32 * std::f32::consts::TAU;
            let direction = normal * angle.cos() + binormal * angle.sin();
            positions.push((*point + direction * radius).to_array());
            normals.push(direction.to_array());
            uvs.push([side as f32 / sides as f32, index as f32 / points.len().max(1) as f32]);
        }
    }
    for ring in 0..points.len().saturating_sub(1) {
        for side in 0..sides {
            let a = (ring * sides + side) as u32;
            let b = (ring * sides + (side + 1) % sides) as u32;
            let c = a + sides as u32;
            let d = b + sides as u32;
            indices.extend_from_slice(&[a, c, b, b, c, d]);
        }
    }
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

/// A flat-shaded prism of `sides` sides (axis Y, base at y = 0).
pub fn peg_mesh(sides: usize, radius: f32, height: f32) -> Mesh {
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut normals: Vec<[f32; 3]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();
    // Squares sit square to the block edges.
    let offset = if sides == 4 { std::f32::consts::FRAC_PI_4 } else { std::f32::consts::FRAC_PI_2 };
    let corner = |i: usize| {
        let angle = offset + i as f32 / sides as f32 * std::f32::consts::TAU;
        Vec2::new(angle.cos(), angle.sin()) * radius
    };
    // Top cap.
    let center = positions.len() as u32;
    positions.push([0.0, height, 0.0]);
    normals.push([0.0, 1.0, 0.0]);
    for i in 0..sides {
        let p = corner(i);
        positions.push([p.x, height, p.y]);
        normals.push([0.0, 1.0, 0.0]);
    }
    for i in 0..sides as u32 {
        let next = (i + 1) % sides as u32;
        indices.extend_from_slice(&[center, center + 1 + next, center + 1 + i]);
    }
    // Sides.
    for i in 0..sides {
        let a = corner(i);
        let b = corner((i + 1) % sides);
        let normal = Vec3::new((a.x + b.x) * 0.5, 0.0, (a.y + b.y) * 0.5).normalize_or(Vec3::X).to_array();
        let base = positions.len() as u32;
        positions.extend_from_slice(&[[a.x, 0.0, a.y], [b.x, 0.0, b.y], [a.x, height, a.y], [b.x, height, b.y]]);
        normals.extend_from_slice(&[normal; 4]);
        indices.extend_from_slice(&[base, base + 2, base + 1, base + 1, base + 2, base + 3]);
    }
    let uvs = vec![[0.0, 0.0]; positions.len()];
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

fn darker(rgb: [u8; 3]) -> [u8; 3] {
    rgb.map(|channel| (channel as f32 * 0.55) as u8)
}

#[allow(clippy::too_many_arguments)]
fn sync_cable_visuals(
    mut commands: Commands,
    mut state: ResMut<CableVisuals>,
    network: Res<CableNetwork>,
    world: Res<VoxelWorld>,
    registry: Res<BlockRegistry>,
    held: Res<crate::gameplay::items::HeldTool>,
    pie: Res<PieState>,
    cameras: Query<&Transform, With<EditorCamera>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let revisions = (world.revision, registry.revision);

    // --- Cables: rebuilt when the network or the blocks change -------------
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    revisions.hash(&mut hasher);
    for cable in &network.cables {
        (cable.from, cable.from_port, cable.to, cable.to_port, cable.kind as u8).hash(&mut hasher);
    }
    let signature = hasher.finish();
    if state.cables_signature != Some(signature) {
        state.cables_signature = Some(signature);
        for entity in state.cables.drain(..) {
            commands.entity(entity).despawn();
        }
        let pulse = state.pulse.get_or_insert_with(|| meshes.add(Sphere::new(CABLE_RADIUS * 2.2).mesh().ico(2).unwrap())).clone();
        for (index, cable) in network.cables.iter().enumerate() {
            let from = IVec3::from_array(cable.from);
            let to = IVec3::from_array(cable.to);
            let a = port_position(from, &ports_at(&world, &registry, from), cable.from_port as usize);
            let b = port_position(to, &ports_at(&world, &registry, to), cable.to_port as usize);
            let points = cable_curve(a, b);
            let rgb = cable.kind.rgb();
            let sheath = state.material(&mut materials, darker(rgb), false);
            let glow = state.material(&mut materials, rgb, true);
            let tube = commands
                .spawn((Name::new("Cable"), CableMesh, Mesh3d(meshes.add(tube_mesh(&points, CABLE_RADIUS, CABLE_SIDES))), MeshMaterial3d(sheath), Transform::IDENTITY, NotShadowCaster))
                .id();
            let pulse = commands
                .spawn((
                    Name::new("Cable pulse"),
                    CableMesh,
                    CablePulse { points, phase: index as f32 * 0.37 },
                    Mesh3d(pulse.clone()),
                    MeshMaterial3d(glow),
                    Transform::from_translation(a),
                    Visibility::Hidden,
                    NotShadowCaster,
                ))
                .id();
            state.cables.extend([tube, pulse]);
        }
    }

    // --- Ports of the blocks around the camera ----------------------------------
    let Some(camera) = cameras.iter().next() else {
        return;
    };
    let center = camera.translation.floor().as_ivec3();
    let big = pie.is_playing() && held.cable;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    (revisions, center.div_euclid(IVec3::splat(4)), big).hash(&mut hasher);
    let signature = hasher.finish();
    if state.ports_signature == Some(signature) {
        return;
    }
    state.ports_signature = Some(signature);
    for entity in state.ports.drain(..) {
        commands.entity(entity).despawn();
    }
    let with_ports: Vec<bool> = {
        let mut flags = Vec::new();
        for (id, def) in registry.blocks() {
            let index = id.0 as usize;
            if flags.len() <= index {
                flags.resize(index + 1, false);
            }
            flags[index] = !def.ports().is_empty();
        }
        flags
    };
    if !with_ports.iter().any(|flag| *flag) {
        return;
    }
    let scale = if big { 1.35 } else { 1.0 };
    let r = PORT_DRAW_RADIUS;
    for dy in -r..=r {
        for dz in -r..=r {
            for dx in -r..=r {
                let cell = center + IVec3::new(dx, dy, dz);
                let Some(voxel) = world.get(cell) else {
                    continue;
                };
                if !with_ports.get(voxel.block.0 as usize).copied().unwrap_or(false) {
                    continue;
                }
                let ports = registry.get(voxel.block).ports();
                for (index, port) in ports.iter().enumerate() {
                    let sides = kind_sides(port.kind);
                    let peg = state.pegs.entry(sides).or_insert_with(|| meshes.add(peg_mesh(sides, PEG_RADIUS, PEG_HEIGHT))).clone();
                    let rgb = if port.direction == PortDirection::Input { INPUT_RGB } else { OUTPUT_RGB };
                    let material = state.material(&mut materials, rgb, false);
                    // The peg stands out of its side face.
                    let normal = crate::gameplay::data_cables::port_normal(&ports, index);
                    let base = port_position(cell, &ports, index) - normal * (PEG_HEIGHT * 0.5);
                    let entity = commands
                        .spawn((
                            Name::new("Port"),
                            PortPeg,
                            Mesh3d(peg),
                            MeshMaterial3d(material),
                            Transform::from_translation(base)
                                .with_rotation(Quat::from_rotation_arc(Vec3::Y, normal))
                                .with_scale(Vec3::splat(scale)),
                            NotShadowCaster,
                        ))
                        .id();
                    state.ports.push(entity);
                }
            }
        }
    }
}

fn animate_pulses(time: Res<Time>, pie: Res<PieState>, mut pulses: Query<(&CablePulse, &mut Transform, &mut Visibility)>) {
    let playing = pie.is_playing();
    for (pulse, mut transform, mut visibility) in &mut pulses {
        let wanted = if playing { Visibility::Inherited } else { Visibility::Hidden };
        if *visibility != wanted {
            *visibility = wanted;
        }
        if !playing || pulse.points.len() < 2 {
            continue;
        }
        let t = (time.elapsed_secs() * 0.8 + pulse.phase).fract() * (pulse.points.len() - 1) as f32;
        let index = (t as usize).min(pulse.points.len() - 2);
        transform.translation = pulse.points[index].lerp(pulse.points[index + 1], t - index as f32);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn port_shapes_differ_by_kind() {
        let sides: Vec<usize> = PortKind::ALL.iter().map(|kind| kind_sides(*kind)).collect();
        assert_eq!(sides, vec![4, 3, 6]);
        let mesh = peg_mesh(6, 0.1, 0.05);
        assert_eq!(mesh.count_vertices(), 7 + 6 * 4);
    }

    #[test]
    fn tube_follows_the_curve() {
        let points = cable_curve(Vec3::ZERO, Vec3::new(4.0, 0.0, 0.0));
        assert!(points.iter().any(|point| point.y < -0.1), "cables sag");
        let mesh = tube_mesh(&points, 0.05, 6);
        assert_eq!(mesh.count_vertices(), points.len() * 6);
    }
}
