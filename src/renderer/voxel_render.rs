//! Voxel render bridge.
//!
//! The mesher produces CPU buffers grouped by block. This module uploads them,
//! caches one [`PolyVoxelMaterial`] per block, and keeps one render entity per
//! `(chunk, block)` pair. Every surface gets a child inverted-hull shell so the
//! outline pass can reuse the exact same mesh.
//!
//! Remeshing is incremental: the world publishes the set of chunks touched by an
//! edit (including neighbours across a chunk border), and only those chunks are
//! rebuilt. Toggling sub-voxel fusion or reloading the block library forces a
//! full rebuild.
//!
//! Shading is independent from the geometry: editing a slider bumps
//! [`ShadingSettings::revision`], and only the material uniforms and the shell
//! visibility are refreshed.

use std::collections::{HashMap, HashSet};

use bevy::asset::RenderAssetUsages;
use bevy::color::LinearRgba;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy_rapier3d::prelude::{Collider, RigidBody};

use crate::core::app_state::{EditorUiState, ShadingSettings};
use crate::renderer::poly_material::{
    PolyMaterial, PolyOutlineMaterial, PolyOutlineShell, PolyOutlineUniform, PolyShadingUniform,
    PolyVoxelMaterial,
};
use crate::voxel::blocks::{BlockId, BlockRegistry};
use crate::voxel::mesher::{self, MeshData};
use crate::voxel::picking::VoxelCursor;
use crate::voxel::world::{VoxelWorld, CHUNK_SIZE};

/// Static collision body generated for a voxel chunk.
#[derive(Component, Debug, Clone, Copy)]
pub struct VoxelChunkCollider;

/// Cached handles and remesh bookkeeping.
#[derive(Resource, Debug, Default)]
pub struct VoxelRenderState {
    entities: HashMap<IVec3, Vec<Entity>>,
    meshes: HashMap<IVec3, Vec<Handle<Mesh>>>,
    retired: Vec<Handle<Mesh>>,
    materials: HashMap<BlockId, Handle<PolyVoxelMaterial>>,
    /// Block look of each cached material (Voronoi density, normal map), for
    /// shading refreshes.
    looks: HashMap<BlockId, BlockLook>,
    /// Outline materials by raw wind value (x100): windy shells sway too.
    outline_materials: HashMap<u32, (f32, Handle<PolyOutlineMaterial>)>,
    /// Chunks waiting for a remesh (a few per frame, nearest first).
    pending: HashSet<IVec3>,
    /// Materials replaced by a registry change, freed once every chunk
    /// using them was rebuilt.
    stale_materials: Vec<Handle<PolyVoxelMaterial>>,
    last_fusion: bool,
    last_registry: u64,
    last_shading: u64,
    initialized: bool,
}

/// Remesh and respawn the visible voxel surfaces when the world changes, then
/// refresh the procedural shading uniforms when the look changes.
pub fn sync_voxel_meshes(
    mut commands: Commands,
    mut world: ResMut<VoxelWorld>,
    registry: Res<BlockRegistry>,
    ui: Res<EditorUiState>,
    mut state: ResMut<VoxelRenderState>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<PolyVoxelMaterial>>,
    mut outline_materials: ResMut<Assets<PolyOutlineMaterial>>,
    shells: Query<Entity, With<PolyOutlineShell>>,
    cameras: Query<&GlobalTransform, With<crate::core::editor_camera::EditorCamera>>,
) {
    let fusion = ui.greedy_meshing;
    let registry_revision = registry.revision;
    let mut dirty: HashSet<IVec3> = std::mem::take(&mut state.pending);
    dirty.extend(world.take_dirty());

    let full_rebuild = !state.initialized
        || state.last_fusion != fusion
        || state.last_registry != registry_revision;

    if full_rebuild {
        dirty.extend(world.chunks().map(|chunk| chunk.coord));
        dirty.extend(state.entities.keys().copied());
        if state.last_registry != registry_revision {
            let old: Vec<Handle<PolyVoxelMaterial>> = state.materials.drain().map(|(_, handle)| handle).collect();
            state.stale_materials.extend(old);
        }
        state.initialized = true;
        state.last_fusion = fusion;
        state.last_registry = registry_revision;
    }

    // Meshes retired during the previous rebuild are no longer referenced by any
    // entity: the commands that despawned those entities were flushed last frame.
    for handle in state.retired.drain(..) {
        meshes.remove(handle.id());
    }

    sync_shading(
        &mut commands,
        &mut state,
        &ui.shading,
        &mut materials,
        &mut outline_materials,
        &shells,
    );

    if dirty.is_empty() {
        for handle in state.stale_materials.drain(..) {
            materials.remove(handle.id());
        }
        return;
    }

    // Budget: removals are free, rebuilds are limited and nearest first.
    let focus = cameras.single().map(|t| t.translation()).unwrap_or(Vec3::ZERO);
    let focus_chunk = IVec3::new(
        (focus.x as i32).div_euclid(CHUNK_SIZE as i32),
        (focus.y as i32).div_euclid(CHUNK_SIZE as i32),
        (focus.z as i32).div_euclid(CHUNK_SIZE as i32),
    );
    let (mut rebuild, removals): (Vec<IVec3>, Vec<IVec3>) =
        dirty.into_iter().partition(|coord| world.chunk(*coord).is_some());
    rebuild.sort_by_key(|coord| ((*coord - focus_chunk).length_squared(), coord.x, coord.y, coord.z));
    let budget = if state.entities.is_empty() { 64 } else { 14 };
    if rebuild.len() > budget {
        state.pending.extend(rebuild.drain(budget..));
    }
    let mut coords = removals;
    coords.extend(rebuild);

    for coord in coords {
        if let Some(entities) = state.entities.remove(&coord) {
            for entity in entities {
                commands.entity(entity).despawn();
            }
        }
        if let Some(handles) = state.meshes.remove(&coord) {
            state.retired.extend(handles);
        }

        let Some(_chunk) = world.chunk(coord) else {
            continue;
        };

        let surfaces = mesher::build_chunk_mesh(&world, &registry, coord, fusion);
        let origin = chunk_origin(coord);
        let (collision_vertices, collision_triangles) = mesher::collision_soup(&surfaces, &registry);
        let mut entities = Vec::new();
        let mut handles = Vec::new();

        for (block, data) in surfaces {
            let liquid = registry.get(block).is_liquid();
            let outline_mesh = (!liquid).then(|| {
                let mut shell = data.clone();
                shell.normals = mesher::outline_directions(&data);
                shell.colors.clear();
                meshes.add(mesh_from_data(shell))
            });
            let mesh = meshes.add(mesh_from_data(data));
            let material =
                material_handle(&mut state, &mut materials, &registry, block, &ui.shading);
            let outline_material =
                outline_material_handle(&mut state, &mut outline_materials, &ui.shading, registry.get(block).wind);
            let surface = commands
                .spawn((
                    Mesh3d(mesh.clone()),
                    MeshMaterial3d(material),
                    Transform::from_translation(origin),
                    crate::editor::studio_common::WorldVisual,
                ))
                .id();
            let Some(outline_mesh) = outline_mesh else {
                // Translucent surfaces get no outline shell.
                entities.push(surface);
                handles.push(mesh);
                continue;
            };

            // The shell reuses the surface mesh and inherits its transform. The
            // outline material culls front faces, so only the expanded back hull
            // survives outside the silhouette.
            commands.spawn((
                Mesh3d(outline_mesh.clone()),
                MeshMaterial3d(outline_material.clone()),
                Transform::IDENTITY,
                ChildOf(surface),
                PolyOutlineShell,
                if ui.shading.outline {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                },
            ));

            entities.push(surface);
            handles.push(mesh);
            handles.push(outline_mesh);
        }

        // One static trimesh per chunk: ramps and stairs collide exactly as drawn.
        if !collision_triangles.is_empty() {
            match Collider::trimesh(collision_vertices, collision_triangles) {
                Ok(collider) => {
                    let body = commands
                        .spawn((
                            RigidBody::Fixed,
                            collider,
                            Transform::from_translation(origin),
                            VoxelChunkCollider,
                        ))
                        .id();
                    entities.push(body);
                }
                Err(error) => warn!("chunk {coord} collider rejected: {error:?}"),
            }
        }

        state.entities.insert(coord, entities);
        state.meshes.insert(coord, handles);
    }
}

/// Push the current [`ShadingSettings`] into every cached material and toggle
/// the outline shells. Runs only when the revision changes.
fn sync_shading(
    commands: &mut Commands,
    state: &mut VoxelRenderState,
    settings: &ShadingSettings,
    materials: &mut Assets<PolyVoxelMaterial>,
    outline_materials: &mut Assets<PolyOutlineMaterial>,
    shells: &Query<Entity, With<PolyOutlineShell>>,
) {
    if state.last_shading == settings.revision {
        return;
    }
    state.last_shading = settings.revision;

    for (block, handle) in &state.materials {
        let look = state.looks.get(block).copied().unwrap_or_default();
        if let Some(mut material) = materials.get_mut(handle) {
            material.extension.settings = PolyShadingUniform::for_look(settings, look);
        }
    }

    for (wind, handle) in state.outline_materials.values() {
        if let Some(mut material) = outline_materials.get_mut(handle) {
            material.settings = PolyOutlineUniform::for_wind(settings, *wind);
        }
    }

    let visibility = if settings.outline {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    for shell in shells {
        commands.entity(shell).insert(visibility);
    }
}

/// Wireframe cursor showing the hit voxel and the placement cell.
pub fn draw_voxel_cursor(
    editor: Res<crate::core::app_state::EditorState>,
    mut gizmos: Gizmos,
    cursor: Res<VoxelCursor>,
    ui: Res<EditorUiState>,
) {
    if !ui.show_gizmos || !editor.workspace.shows_world() || editor.mode.is_playing() {
        return;
    }

    if let Some(hit) = cursor.target {
        let center = cell_center(hit.voxel);
        gizmos.cube(
            Isometry3d::new(center, Quat::IDENTITY),
            Color::srgba(1.0, 0.82, 0.35, 0.9),
        );

        let normal = Vec3::new(
            hit.normal.x as f32,
            hit.normal.y as f32,
            hit.normal.z as f32,
        );
        if normal != Vec3::ZERO {
            gizmos.line(
                center,
                center + normal * 0.85,
                Color::srgba(1.0, 0.82, 0.35, 0.7),
            );
        }
    }

    if let Some(cell) = cursor.placement {
        gizmos.cube(
            Isometry3d::new(cell_center(cell), Quat::IDENTITY),
            Color::srgba(0.30, 0.82, 0.74, 0.72),
        );
    }
}

pub(crate) fn material_handle(
    state: &mut VoxelRenderState,
    materials: &mut Assets<PolyVoxelMaterial>,
    registry: &BlockRegistry,
    block: BlockId,
    shading: &ShadingSettings,
) -> Handle<PolyVoxelMaterial> {
    if let Some(handle) = state.materials.get(&block) {
        return handle.clone();
    }

    let def = registry.get(block);
    let handle = materials.add(block_material(def, shading));
    state.materials.insert(block, handle.clone());
    state.looks.insert(block, BlockLook::of(def));
    handle
}

/// Per-block parameters of the procedural shading.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BlockLook {
    pub voronoi: crate::voxel::blocks::VoronoiCells,
    pub normal: crate::voxel::blocks::VoronoiNormal,
    /// Raw wind sway of the block.
    pub wind: f32,
    pub liquid: bool,
    pub accent: Option<crate::voxel::blocks::BlockAccent>,
}

impl BlockLook {
    pub fn of(def: &crate::voxel::blocks::BlockDef) -> Self {
        Self {
            voronoi: def.voronoi,
            normal: def.normal,
            wind: def.wind,
            liquid: def.is_liquid(),
            accent: def.accent,
        }
    }
}

/// Full voxel material of a block: PBR base, liquid transparency, custom
/// model vertex colours and the toon / Voronoi / facet extension. Shared by
/// the world renderer and the forge / terraformer previews.
pub fn block_material(
    def: &crate::voxel::blocks::BlockDef,
    shading: &ShadingSettings,
) -> PolyVoxelMaterial {
    let base = Color::srgb_u8(def.color[0], def.color[1], def.color[2]);
    let mut standard = StandardMaterial {
        // Painted parts and elements bring their colour through the vertices.
        base_color: if def.has_vertex_colors() { Color::WHITE } else { base },
        perceptual_roughness: def.roughness.clamp(0.02, 1.0),
        metallic: def.metallic.clamp(0.0, 1.0),
        ..default()
    };

    if def.emissive > 0.0 {
        let linear = base.to_linear();
        standard.emissive = LinearRgba::rgb(
            linear.red * def.emissive,
            linear.green * def.emissive,
            linear.blue * def.emissive,
        );
    }

    if def.is_liquid() {
        standard.base_color = base.with_alpha(def.opacity.clamp(0.05, 1.0));
        standard.alpha_mode = AlphaMode::Blend;
        standard.cull_mode = None;
        standard.double_sided = true;
        standard.reflectance = 0.7;
    }

    PolyVoxelMaterial {
        base: standard,
        extension: PolyMaterial {
            settings: PolyShadingUniform::for_look(shading, BlockLook::of(def)),
        },
    }
}

fn outline_material_handle(
    state: &mut VoxelRenderState,
    materials: &mut Assets<PolyOutlineMaterial>,
    shading: &ShadingSettings,
    wind: f32,
) -> Handle<PolyOutlineMaterial> {
    let key = (wind.max(0.0) * 100.0).round() as u32;
    if let Some((_, handle)) = state.outline_materials.get(&key) {
        return handle.clone();
    }

    let handle = materials.add(PolyOutlineMaterial {
        settings: PolyOutlineUniform::for_wind(shading, wind),
    });
    state.outline_materials.insert(key, (wind, handle.clone()));
    handle
}

pub(crate) fn mesh_from_data(data: MeshData) -> Mesh {
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    // UV_1.x carries the orientation of the voxel (the vertex stage turns
    // the block-space pattern with it); UV_1 also makes the vertex stage
    // pass the block-space position.
    let count = data.positions.len();
    let colors = (count > 0 && data.colors.len() == count).then_some(data.colors);
    let orientations: Vec<[f32; 2]> = if data.orientations.len() == count {
        data.orientations.iter().map(|o| [*o, 0.0]).collect()
    } else {
        vec![[0.0_f32, 0.0]; count]
    };
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, data.positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, data.normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, data.uvs);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_1, orientations);
    if let Some(colors) = colors {
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    }
    mesh.insert_indices(Indices::U32(data.indices));
    mesh
}

fn chunk_origin(coord: IVec3) -> Vec3 {
    Vec3::new(
        (coord.x * CHUNK_SIZE as i32) as f32,
        (coord.y * CHUNK_SIZE as i32) as f32,
        (coord.z * CHUNK_SIZE as i32) as f32,
    )
}

fn cell_center(cell: IVec3) -> Vec3 {
    Vec3::new(
        cell.x as f32 + 0.5,
        cell.y as f32 + 0.5,
        cell.z as f32 + 0.5,
    )
}
