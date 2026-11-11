//! Blocky clouds: flat white boxes on a layer above the terrain, drifting
//! with the wind, generated around the camera from a stable hash so they
//! never pop when you come back.

use std::collections::HashMap;

use bevy::light::NotShadowCaster;
use bevy::prelude::*;

use crate::core::editor_camera::EditorCamera;
use crate::editor::studio_common::WorldVisual;

const CELL: f32 = 36.0;
const RADIUS: i32 = 8;

#[derive(Component)]
struct Cloud {
    cell: IVec2,
}

#[derive(Resource, Default)]
struct CloudState {
    clouds: HashMap<IVec2, Entity>,
    mesh: Option<Handle<Mesh>>,
    material: Option<Handle<StandardMaterial>>,
    time: f32,
}

pub struct CloudPlugin;

impl Plugin for CloudPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CloudState>().add_systems(Update, update_clouds);
    }
}

fn hash(cell: IVec2, salt: u32) -> f32 {
    let mut h = (cell.x as u32).wrapping_mul(0x8DA6_B343) ^ (cell.y as u32).wrapping_mul(0xD816_3841) ^ salt.wrapping_mul(0xCB1A_B31F);
    h ^= h >> 13;
    h = h.wrapping_mul(0x5bd1_e995);
    h ^= h >> 15;
    (h & 0xFFFF) as f32 / 65535.0
}

/// Cloud shape of a cell, if any: (centre offset, size).
fn cloud_of(cell: IVec2) -> Option<(Vec3, Vec3)> {
    if hash(cell, 1) < 0.58 {
        return None;
    }
    let size = Vec3::new(10.0 + hash(cell, 2) * 22.0, 3.0 + hash(cell, 3) * 2.0, 8.0 + hash(cell, 4) * 18.0);
    let offset = Vec3::new(hash(cell, 5) * (CELL - size.x), hash(cell, 6) * 6.0, hash(cell, 7) * (CELL - size.z));
    Some((offset + size * 0.5, size))
}

fn update_clouds(
    mut commands: Commands,
    time: Res<Time>,
    mut state: ResMut<CloudState>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    cameras: Query<&GlobalTransform, With<EditorCamera>>,
    mut clouds: Query<(&Cloud, &mut Transform)>,
    rules: Res<crate::gameplay::world_rules::WorldRules>,
) {
    let Ok(camera) = cameras.single() else {
        return;
    };
    if !crate::gameplay::atmosphere::atmosphere().sky.clouds {
        for (_, entity) in state.clouds.drain() {
            commands.entity(entity).despawn();
        }
        return;
    }
    state.time += time.delta_secs();
    let drift = state.time * crate::gameplay::atmosphere::atmosphere().sky.cloud_speed;
    let focus = camera.translation();
    let center = IVec2::new(((focus.x - drift) / CELL).floor() as i32, (focus.z / CELL).floor() as i32);

    let mesh = state.mesh.get_or_insert_with(|| meshes.add(Cuboid::new(1.0, 1.0, 1.0))).clone();
    let material = state
        .material
        .get_or_insert_with(|| {
            materials.add(StandardMaterial {
                base_color: Color::srgb(0.97, 0.98, 1.0),
                emissive: LinearRgba::rgb(0.35, 0.36, 0.4),
                perceptual_roughness: 1.0,
                ..default()
            })
        })
        .clone();
    if let Some(mut cloud_material) = materials.get_mut(&material) {
        let light = 0.18 + 0.82 * rules.daylight();
        cloud_material.base_color = Color::srgb(0.97 * light, 0.98 * light, light);
        cloud_material.emissive = LinearRgba::rgb(0.35, 0.36, 0.4) * light;
    }

    // Drop far cells, add missing ones.
    let keep: Vec<IVec2> = state.clouds.keys().copied().collect();
    for cell in keep {
        if (cell - center).abs().max_element() > RADIUS {
            if let Some(entity) = state.clouds.remove(&cell) {
                commands.entity(entity).despawn();
            }
        }
    }
    for dz in -RADIUS..=RADIUS {
        for dx in -RADIUS..=RADIUS {
            let cell = center + IVec2::new(dx, dz);
            if state.clouds.contains_key(&cell) {
                continue;
            }
            let Some((_, size)) = cloud_of(cell) else {
                continue;
            };
            let entity = commands
                .spawn((
                    Name::new("Cloud"),
                    Cloud { cell },
                    WorldVisual,
                    NotShadowCaster,
                    Mesh3d(mesh.clone()),
                    MeshMaterial3d(material.clone()),
                    Transform::from_scale(size),
                ))
                .id();
            state.clouds.insert(cell, entity);
        }
    }
    for (cloud, mut transform) in &mut clouds {
        if let Some((offset, _)) = cloud_of(cloud.cell) {
            transform.translation =
                Vec3::new(cloud.cell.x as f32 * CELL + drift, crate::gameplay::atmosphere::atmosphere().sky.cloud_height, cloud.cell.y as f32 * CELL) + offset;
        }
    }
}
