//! 🏰 Atelier de structures: the prefab studio of the "Structures" workspace.
//!
//! The selected prefab is shown directly in the viewport (the world is
//! hidden, turntable camera) and edited in place:
//!
//! - **Poser** (`B`): left click places the brush block on the aimed face (or
//!   on the floor grid);
//! - **Effacer** (`X`, or `Shift` + click): removes the aimed block;
//! - **Ancre** (`A`): toggles an anchor block (anchors must rest on solid
//!   ground for the generator to place the prefab);
//! - **Pipette** (`I`): picks the aimed block as the brush.
//!
//! The prefab box follows its content; cells are kept relative to the
//! minimum corner. Changes are saved with the panel's *Enregistrer* button.

use std::collections::HashMap;

use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::core::app_state::{EditorState, EditorUiState, Workspace};
use crate::core::editor_camera::EditorCamera;
use crate::core::pie_manager::PieState;
use crate::editor::studio_common::{effective_workspace, orbit_input, Orbit, StudioSet};
use crate::renderer::poly_material::PolyVoxelMaterial;
use crate::renderer::voxel_render::{block_material, mesh_from_data};
use crate::voxel::blocks::{BlockId, BlockRegistry};
use crate::voxel::mesher;
use crate::voxel::picking::{placement_orientation, raycast_filtered};
use crate::voxel::structures::{StructureCell, StructureDef, StructureLibrary};
use crate::voxel::world::VoxelWorld;

/// Where the prefab preview is built.
pub const STRUCTURE_ORIGIN: Vec3 = Vec3::new(-4096.0, 0.0, -4096.0);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StructureTool {
    #[default]
    Place,
    Erase,
    Anchor,
    Pick,
}

impl StructureTool {
    pub const ALL: [Self; 4] = [Self::Place, Self::Erase, Self::Anchor, Self::Pick];

    pub const fn icon(self) -> &'static str {
        match self {
            Self::Place => "📌",
            Self::Erase => "🗙",
            Self::Anchor => "⚓",
            Self::Pick => "💧",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Place => "Place the brush block (B)",
            Self::Erase => "Erase (X or Shift + click)",
            Self::Anchor => "Anchor: must rest on the ground (A)",
            Self::Pick => "Picker: take the aimed block (I)",
        }
    }
}

#[derive(Resource, Debug)]
pub struct StructureStudio {
    pub tool: StructureTool,
    pub orbit: Orbit,
    pub frame_request: bool,
    /// Unsaved edits of the selected prefab.
    pub dirty: bool,
    /// Aimed cell and face normal (for the gizmo).
    hover: Option<(IVec3, IVec3, bool)>,
    world: VoxelWorld,
    signature: String,
    last_workspace: Workspace,
    meshes: Vec<Handle<Mesh>>,
    materials: Vec<Handle<PolyVoxelMaterial>>,
}

impl Default for StructureStudio {
    fn default() -> Self {
        Self {
            tool: StructureTool::Place,
            orbit: Orbit {
                target: STRUCTURE_ORIGIN,
                yaw: 0.7,
                pitch: -0.5,
                distance: 18.0,
            },
            frame_request: true,
            dirty: false,
            hover: None,
            world: VoxelWorld::new(),
            signature: String::new(),
            last_workspace: Workspace::World,
            meshes: Vec::new(),
            materials: Vec::new(),
        }
    }
}

#[derive(Component)]
struct StructurePreviewRoot;

pub struct StructureStudioPlugin;

impl Plugin for StructureStudioPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<StructureStudio>().add_systems(
            Update,
            (enter, structure_camera, sync_structure_preview, edit_structure, draw_structure)
                .chain()
                .in_set(StudioSet),
        );
    }
}

fn in_atelier(editor: &EditorState, pie: &PieState) -> bool {
    effective_workspace(editor, pie) == Workspace::Structures
}

/// Move cells so the minimum corner is at the origin and fit the box.
pub fn normalize(structure: &mut StructureDef) {
    if structure.cells.is_empty() {
        return;
    }
    let mut min = IVec3::splat(i32::MAX);
    let mut max = IVec3::splat(i32::MIN);
    for cell in &structure.cells {
        let p = IVec3::from_array(cell.pos);
        min = min.min(p);
        max = max.max(p);
    }
    for cell in &mut structure.cells {
        cell.pos = (IVec3::from_array(cell.pos) - min).to_array();
    }
    for anchor in &mut structure.anchors {
        *anchor = (IVec3::from_array(*anchor) - min).to_array();
    }
    structure.anchors.retain(|anchor| structure.cells.iter().any(|cell| cell.pos == *anchor));
    structure.size = (max - min + IVec3::ONE).to_array();
}

fn enter(editor: Res<EditorState>, pie: Res<PieState>, mut studio: ResMut<StructureStudio>, library: Res<StructureLibrary>, mut last_selection: Local<usize>) {
    let workspace = effective_workspace(&editor, &pie);
    if workspace == Workspace::Structures && studio.last_workspace != Workspace::Structures {
        studio.frame_request = true;
    }
    if *last_selection != library.selection {
        *last_selection = library.selection;
        studio.frame_request = true;
    }
    studio.last_workspace = workspace;
}

#[allow(clippy::too_many_arguments)]
fn structure_camera(
    editor: Res<EditorState>,
    pie: Res<PieState>,
    ui: Res<EditorUiState>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    egui_input: Res<bevy_egui::input::EguiWantsInput>,
    library: Res<StructureLibrary>,
    mut studio: ResMut<StructureStudio>,
    mut cameras: Query<&mut Transform, With<EditorCamera>>,
) {
    if !in_atelier(&editor, &pie) {
        return;
    }
    if ui.viewport_hovered {
        orbit_input(&mut studio.orbit, &keyboard, &mouse, &motion, &scroll, 200.0);
        if !egui_input.wants_keyboard_input() {
            for (key, tool) in [
                (KeyCode::KeyB, StructureTool::Place),
                (KeyCode::KeyX, StructureTool::Erase),
                (KeyCode::KeyA, StructureTool::Anchor),
                (KeyCode::KeyI, StructureTool::Pick),
            ] {
                if keyboard.just_pressed(key) {
                    studio.tool = tool;
                }
            }
            if keyboard.just_pressed(KeyCode::KeyF) {
                studio.frame_request = true;
            }
        }
    }
    if std::mem::take(&mut studio.frame_request) {
        let size = library
            .structures
            .get(library.selection)
            .map(|s| IVec3::from_array(s.size).max(IVec3::splat(4)).as_vec3())
            .unwrap_or(Vec3::splat(8.0));
        studio.orbit.target = STRUCTURE_ORIGIN + size * Vec3::new(0.5, 0.35, 0.5);
        studio.orbit.distance = (size.length() * 1.5).clamp(6.0, 160.0);
    }
    if let Ok(mut transform) = cameras.single_mut() {
        *transform = studio.orbit.transform();
    }
}

#[allow(clippy::too_many_arguments)]
fn sync_structure_preview(
    mut commands: Commands,
    editor: Res<EditorState>,
    pie: Res<PieState>,
    ui: Res<EditorUiState>,
    registry: Res<BlockRegistry>,
    library: Res<StructureLibrary>,
    mut studio: ResMut<StructureStudio>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<PolyVoxelMaterial>>,
    roots: Query<Entity, With<StructurePreviewRoot>>,
) {
    let active = in_atelier(&editor, &pie);
    let signature = if active {
        format!("{}|{}|{}|{}", library.selection, library.revision, registry.revision, ui.shading.revision)
    } else {
        String::new()
    };
    if signature == studio.signature {
        return;
    }
    studio.signature = signature;
    for root in &roots {
        commands.entity(root).despawn();
    }
    for handle in studio.meshes.drain(..) {
        meshes.remove(handle.id());
    }
    for handle in studio.materials.drain(..) {
        materials.remove(handle.id());
    }
    let mut world = VoxelWorld::new();
    if active {
        if let Some(structure) = library.structures.get(library.selection) {
            for cell in &structure.cells {
                if let Some(voxel) = StructureDef::voxel(cell, &registry) {
                    world.set(IVec3::from_array(cell.pos), Some(voxel));
                }
            }
        }
    }
    world.take_edits();
    world.set_liquid_mask(registry.blocks().map(|(_, def)| def.is_liquid()).collect());
    if !active {
        studio.world = world;
        return;
    }
    let root = commands
        .spawn((
            Name::new("Structure preview"),
            StructurePreviewRoot,
            Transform::from_translation(STRUCTURE_ORIGIN),
            Visibility::default(),
        ))
        .id();
    let mut cache: HashMap<BlockId, Handle<PolyVoxelMaterial>> = HashMap::new();
    let coords: Vec<IVec3> = world.chunks().map(|c| c.coord).collect();
    for coord in coords {
        let origin = coord.as_vec3() * crate::voxel::world::CHUNK_SIZE as f32;
        for (id, data) in mesher::build_chunk_mesh(&world, &registry, coord, ui.greedy_meshing) {
            let mesh = meshes.add(mesh_from_data(data));
            let material = cache
                .entry(id)
                .or_insert_with(|| {
                    let handle = materials.add(block_material(registry.get(id), &ui.shading));
                    studio.materials.push(handle.clone());
                    handle
                })
                .clone();
            studio.meshes.push(mesh.clone());
            commands.spawn((Mesh3d(mesh), MeshMaterial3d(material), Transform::from_translation(origin), ChildOf(root)));
        }
    }
    studio.world = world;
}

#[allow(clippy::too_many_arguments)]
fn edit_structure(
    mut editor: ResMut<EditorState>,
    pie: Res<PieState>,
    mut ui: ResMut<EditorUiState>,
    mouse: Res<ButtonInput<MouseButton>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    registry: Res<BlockRegistry>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform), With<EditorCamera>>,
    mut library: ResMut<StructureLibrary>,
    mut studio: ResMut<StructureStudio>,
) {
    studio.hover = None;
    if !in_atelier(&editor, &pie) || !ui.viewport_hovered {
        return;
    }
    let (Ok(window), Ok((camera, camera_transform))) = (windows.single(), cameras.single()) else {
        return;
    };
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    let Ok(ray) = camera.viewport_to_world(camera_transform, cursor) else {
        return;
    };
    let origin = ray.origin - STRUCTURE_ORIGIN;
    let direction = *ray.direction;

    // Aimed block, or the floor plane (y = 0) when nothing is hit.
    let hit = raycast_filtered(&studio.world, origin, direction, 300.0, true);
    let (cell, normal, on_block) = match hit {
        Some(hit) => (hit.voxel, hit.normal, true),
        None => {
            if direction.y.abs() < 1e-4 {
                return;
            }
            let t = -origin.y / direction.y;
            if t <= 0.0 {
                return;
            }
            let point = origin + direction * t;
            (IVec3::new(point.x.floor() as i32, -1, point.z.floor() as i32), IVec3::Y, false)
        }
    };
    studio.hover = Some((cell, normal, on_block));

    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    let shift = keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight);
    let tool = if shift { StructureTool::Erase } else { studio.tool };
    let selection = library.selection;
    let Some(structure) = library.structures.get_mut(selection) else {
        return;
    };
    let mut changed = false;
    match tool {
        StructureTool::Place => {
            let target = cell + normal;
            if target.abs().max_element() > 96 {
                return;
            }
            let block = editor.selected_block;
            let orientation = placement_orientation(&ui, registry.get(block).placement, normal, direction);
            let pos = target.to_array();
            structure.cells.retain(|c| c.pos != pos);
            structure.cells.push(StructureCell {
                pos,
                block: registry.get(block).id.clone(),
                rot: orientation.0,
                sub: None,
            });
            changed = true;
        }
        StructureTool::Erase if on_block => {
            let pos = cell.to_array();
            let before = structure.cells.len();
            structure.cells.retain(|c| c.pos != pos);
            structure.anchors.retain(|a| *a != pos);
            changed = structure.cells.len() != before;
        }
        StructureTool::Anchor if on_block => {
            let pos = cell.to_array();
            if let Some(index) = structure.anchors.iter().position(|a| *a == pos) {
                structure.anchors.remove(index);
            } else {
                structure.anchors.push(pos);
            }
            changed = true;
        }
        StructureTool::Pick if on_block => {
            if let Some(found) = structure.cells.iter().find(|c| c.pos == cell.to_array()) {
                if let Some(block) = registry.find(&found.block) {
                    editor.selected_block = block;
                    ui.info(format!("Brush: {}", registry.get(block).label));
                }
            }
        }
        _ => {}
    }
    if changed {
        // Keep the origin at the minimum corner (and follow the shift).
        let before = structure.cells.iter().map(|c| IVec3::from_array(c.pos)).fold(IVec3::splat(i32::MAX), IVec3::min);
        normalize(structure);
        library.revision = library.revision.wrapping_add(1);
        studio.dirty = true;
        if before.cmplt(IVec3::ZERO).any() {
            // The content moved: shift the camera target along so the view stays.
            let shift = (-before.min(IVec3::ZERO)).as_vec3();
            studio.orbit.target += shift;
        }
    }
}

fn draw_structure(
    mut gizmos: Gizmos,
    editor: Res<EditorState>,
    pie: Res<PieState>,
    library: Res<StructureLibrary>,
    studio: Res<StructureStudio>,
) {
    if !in_atelier(&editor, &pie) {
        return;
    }
    let size = library
        .structures
        .get(library.selection)
        .map(|s| IVec3::from_array(s.size))
        .unwrap_or(IVec3::splat(8));
    // Floor grid around the box.
    let margin = 4;
    for x in -margin..=size.x + margin {
        let color = if x == 0 || x == size.x { Color::srgba(0.8, 0.85, 0.9, 0.5) } else { Color::srgba(0.4, 0.5, 0.6, 0.18) };
        gizmos.line(
            STRUCTURE_ORIGIN + Vec3::new(x as f32, 0.0, -margin as f32),
            STRUCTURE_ORIGIN + Vec3::new(x as f32, 0.0, (size.z + margin) as f32),
            color,
        );
    }
    for z in -margin..=size.z + margin {
        let color = if z == 0 || z == size.z { Color::srgba(0.8, 0.85, 0.9, 0.5) } else { Color::srgba(0.4, 0.5, 0.6, 0.18) };
        gizmos.line(
            STRUCTURE_ORIGIN + Vec3::new(-margin as f32, 0.0, z as f32),
            STRUCTURE_ORIGIN + Vec3::new((size.x + margin) as f32, 0.0, z as f32),
            color,
        );
    }
    // Bounding box.
    let half = size.as_vec3() * 0.5;
    gizmos.cube(
        Transform::from_translation(STRUCTURE_ORIGIN + half).with_scale(size.as_vec3()),
        Color::srgba(0.3, 0.82, 0.74, 0.5),
    );
    // Anchors.
    if let Some(structure) = library.structures.get(library.selection) {
        for anchor in &structure.anchors {
            let center = STRUCTURE_ORIGIN + IVec3::from_array(*anchor).as_vec3() + Vec3::splat(0.5);
            gizmos.cube(Transform::from_translation(center).with_scale(Vec3::splat(1.04)), Color::srgb(1.0, 0.5, 0.2));
            gizmos.line(center, center - Vec3::Y * 0.9, Color::srgb(1.0, 0.5, 0.2));
        }
    }
    // Hover.
    if let Some((cell, normal, on_block)) = studio.hover {
        let target = match studio.tool {
            StructureTool::Place => cell + normal,
            _ => cell,
        };
        let color = match studio.tool {
            StructureTool::Place => Color::srgba(0.3, 0.9, 0.8, 0.9),
            StructureTool::Erase => Color::srgba(1.0, 0.35, 0.35, 0.9),
            StructureTool::Anchor => Color::srgba(1.0, 0.6, 0.2, 0.9),
            StructureTool::Pick => Color::srgba(1.0, 0.9, 0.4, 0.9),
        };
        if on_block || studio.tool == StructureTool::Place {
            gizmos.cube(
                Transform::from_translation(STRUCTURE_ORIGIN + target.as_vec3() + Vec3::splat(0.5)).with_scale(Vec3::splat(1.02)),
                color,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_moves_the_minimum_corner_to_the_origin() {
        let mut structure = StructureDef::new("Test", [1, 1, 1]);
        for pos in [[-2, 0, 1], [3, 2, 1]] {
            structure.cells.push(StructureCell { pos, block: "stone".into(), rot: 0, sub: None });
        }
        structure.anchors.push([-2, 0, 1]);
        normalize(&mut structure);
        assert_eq!(structure.size, [6, 3, 1]);
        assert!(structure.cells.iter().any(|c| c.pos == [0, 0, 0]));
        assert_eq!(structure.anchors, vec![[0, 0, 0]]);
    }
}
