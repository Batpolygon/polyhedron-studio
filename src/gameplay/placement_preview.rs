//! Ghost of the block about to be placed: a translucent copy of the real
//! block mesh (shape, model and orientation) in the target cell, in play
//! (held block, aimed face) and in the world editor (brush block).

use bevy::prelude::*;

use crate::core::app_state::{EditorState, EditorUiState, Workspace};
use crate::core::editor_camera::EditorCamera;
use crate::core::pie_manager::PieState;
use crate::gameplay::inventory::{HotbarItem, Inventory};
use crate::gameplay::mining::PlayerAim;
use crate::renderer::voxel_render::mesh_from_data;
use crate::voxel::blocks::{BlockId, BlockRegistry};
use crate::voxel::mesher;
use crate::voxel::picking::{placement_orientation, VoxelCursor};
use crate::voxel::shapes::Orientation;
use crate::voxel::world::{Voxel, VoxelWorld};

#[derive(Component)]
struct PlacementGhost;

#[derive(Resource, Default)]
struct GhostState {
    built: Option<(BlockId, Orientation, u64)>,
    material: Option<Handle<StandardMaterial>>,
    mesh: Option<Handle<Mesh>>,
}

pub struct PlacementPreviewPlugin;

impl Plugin for PlacementPreviewPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<GhostState>()
            .add_systems(Startup, spawn_ghost)
            .add_systems(PostUpdate, update_ghost);
    }
}

fn spawn_ghost(mut commands: Commands) {
    commands.spawn((
        Name::new("Placement ghost"),
        PlacementGhost,
        Transform::default(),
        Visibility::Hidden,
    ));
}

/// Orientation used by the play-mode placement (same rule as `mining`).
pub fn play_orientation(placement: crate::voxel::blocks::BlockPlacement, normal: IVec3, direction: Vec3) -> Orientation {
    crate::voxel::picking::oriented_toward_viewer(placement, normal, direction)
}

#[allow(clippy::too_many_arguments)]
fn update_ghost(
    mut commands: Commands,
    pie: Res<PieState>,
    editor: Res<EditorState>,
    ui: Res<EditorUiState>,
    inventory: Res<Inventory>,
    aim: Res<PlayerAim>,
    cursor: Res<VoxelCursor>,
    registry: Res<BlockRegistry>,
    cameras: Query<&GlobalTransform, With<EditorCamera>>,
    mut state: ResMut<GhostState>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut ghosts: Query<(Entity, &mut Transform, &mut Visibility), With<PlacementGhost>>,
) {
    let Ok((entity, mut transform, mut visibility)) = ghosts.single_mut() else {
        return;
    };
    // What would be placed, where, and how.
    let target: Option<(BlockId, IVec3, Orientation)> = if pie.is_playing() {
        match (inventory.current(), aim.hit, pie.cursor_captured) {
            (HotbarItem::Block(block), Some(hit), true) => {
                Some((*block, hit.placement(), play_orientation(registry.get(*block).placement, hit.normal, aim.direction)))
            }
            _ => None,
        }
    } else if editor.workspace == Workspace::World && ui.viewport_hovered && !ui.cable_tool {
        cursor.target.map(|hit| {
            let forward = cameras.single().map(|t| t.forward().as_vec3()).unwrap_or(Vec3::NEG_Z);
            (editor.selected_block, hit.placement(), placement_orientation(&ui, registry.get(editor.selected_block).placement, hit.normal, forward))
        })
    } else {
        None
    };

    let Some((block, cell, orientation)) = target else {
        if *visibility != Visibility::Hidden {
            *visibility = Visibility::Hidden;
        }
        return;
    };

    let key = (block, orientation, registry.revision);
    if state.built != Some(key) {
        state.built = Some(key);
        let mut world = VoxelWorld::new();
        world.set(IVec3::ZERO, Some(Voxel::oriented(block, orientation)));
        let mesh = mesher::build_chunk_mesh(&world, &registry, IVec3::ZERO, true)
            .remove(&block)
            .map(|data| meshes.add(mesh_from_data(data)));
        if let Some(old) = state.mesh.take() {
            meshes.remove(old.id());
        }
        let material = state
            .material
            .get_or_insert_with(|| {
                materials.add(StandardMaterial {
                    base_color: Color::srgba(1.0, 1.0, 1.0, 0.38),
                    alpha_mode: AlphaMode::Blend,
                    unlit: true,
                    ..default()
                })
            })
            .clone();
        match &mesh {
            Some(mesh) => {
                commands.entity(entity).insert((Mesh3d(mesh.clone()), MeshMaterial3d(material)));
            }
            None => {
                commands.entity(entity).remove::<Mesh3d>();
            }
        }
        state.mesh = mesh;
    }
    // Slightly inflated around the cell centre to avoid z-fighting.
    transform.translation = cell.as_vec3() - Vec3::splat(0.005);
    transform.scale = Vec3::splat(1.01);
    if *visibility != Visibility::Inherited {
        *visibility = Visibility::Inherited;
    }
}
