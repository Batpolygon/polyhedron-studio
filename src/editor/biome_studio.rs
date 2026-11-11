//! 🌲 Terraformeur: the biome studio of the "Biomes" workspace.
//!
//! The selected biome is generated alone (every column belongs to it) on a
//! patch of a few chunk columns, with its relief, palette, landform feature,
//! water and every structure its rules let spawn naturally, then meshed with
//! the real voxel renderer and shown on a turntable camera under the biome's
//! sky colour. Edits are previewed after a short pause (debounce).

use std::collections::HashMap;
use std::time::Instant;

use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll};
use bevy::prelude::*;

use crate::core::app_state::{EditorState, EditorUiState, Workspace};
use crate::core::editor_camera::EditorCamera;
use crate::core::pie_manager::PieState;
use crate::editor::studio_common::{effective_workspace, orbit_input, Orbit, StudioSet};
use crate::renderer::poly_material::PolyVoxelMaterial;
use crate::renderer::voxel_render::{block_material, mesh_from_data};
use crate::voxel::biomes::BiomeLibrary;
use crate::voxel::blocks::{BlockId, BlockRegistry};
use crate::voxel::mesher;
use crate::voxel::structures::StructureLibrary;
use crate::voxel::world::{VoxelWorld, CHUNK_SIZE};
use crate::voxel::world_gen::TerrainGenerator;

/// Where the biome preview is built.
pub const TERRAFORM_ORIGIN: Vec3 = Vec3::new(4096.0, 0.0, 4096.0);
/// Seconds without edits before the preview is regenerated.
const DEBOUNCE: f32 = 0.35;

#[derive(Resource, Debug)]
pub struct BiomeStudio {
    pub orbit: Orbit,
    /// Side of the preview patch, in chunk columns.
    pub size: i32,
    /// Sample position of the patch in the generator (chunk columns).
    pub offset: IVec2,
    pub frame_request: bool,
    pub rebuild_request: bool,
    /// Stats of the last generation.
    pub last_voxels: usize,
    pub last_millis: f32,
    pub last_structures: usize,
    /// Surface height at the centre of the last patch.
    surface: f32,
    frame_after_build: bool,
    signature: String,
    pending_since: Option<f32>,
    last_workspace: Workspace,
    saved_clear: Option<Color>,
    meshes: Vec<Handle<Mesh>>,
    materials: Vec<Handle<PolyVoxelMaterial>>,
}

impl Default for BiomeStudio {
    fn default() -> Self {
        Self {
            orbit: Orbit {
                target: TERRAFORM_ORIGIN,
                yaw: 0.7,
                pitch: -0.55,
                distance: 70.0,
            },
            size: 4,
            offset: IVec2::ZERO,
            frame_request: true,
            rebuild_request: false,
            last_voxels: 0,
            last_millis: 0.0,
            last_structures: 0,
            surface: 10.0,
            frame_after_build: true,
            signature: String::new(),
            pending_since: None,
            last_workspace: Workspace::World,
            saved_clear: None,
            meshes: Vec::new(),
            materials: Vec::new(),
        }
    }
}

impl BiomeStudio {
    /// Jump to another sample spot of the generator.
    pub fn reroll(&mut self) {
        let seed = (self.offset.x * 73_856_093) ^ (self.offset.y * 19_349_663) ^ 0x5bd1;
        self.offset = IVec2::new(seed.rem_euclid(97) - 48, (seed / 97).rem_euclid(97) - 48);
        self.rebuild_request = true;
    }
}

#[derive(Component)]
struct TerraformPreviewRoot;

pub struct BiomeStudioPlugin;

impl Plugin for BiomeStudioPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BiomeStudio>().add_systems(
            Update,
            (enter_leave, terraform_camera, sync_terraform_preview).chain().in_set(StudioSet),
        );
    }
}

fn in_terraformer(editor: &EditorState, pie: &PieState) -> bool {
    effective_workspace(editor, pie) == Workspace::Biomes
}

/// Sky colour of the selected biome while the studio is open.
fn enter_leave(
    editor: Res<EditorState>,
    pie: Res<PieState>,
    library: Res<BiomeLibrary>,
    mut studio: ResMut<BiomeStudio>,
    mut clear: ResMut<ClearColor>,
) {
    let workspace = effective_workspace(&editor, &pie);
    let inside = workspace == Workspace::Biomes;
    if inside && studio.last_workspace != Workspace::Biomes {
        studio.saved_clear = Some(clear.0);
        studio.frame_after_build = true;
    }
    if !inside && studio.last_workspace == Workspace::Biomes {
        if let Some(saved) = studio.saved_clear.take() {
            clear.0 = saved;
        }
    }
    studio.last_workspace = workspace;
    if inside {
        if let Some(biome) = library.biomes.get(library.selection) {
            let sky = Color::srgb(biome.sky_color[0], biome.sky_color[1], biome.sky_color[2]);
            if clear.0 != sky {
                clear.0 = sky;
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn terraform_camera(
    editor: Res<EditorState>,
    pie: Res<PieState>,
    ui: Res<EditorUiState>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    egui_input: Res<bevy_egui::input::EguiWantsInput>,
    mut studio: ResMut<BiomeStudio>,
    mut cameras: Query<&mut Transform, With<EditorCamera>>,
) {
    if !in_terraformer(&editor, &pie) {
        return;
    }
    if ui.viewport_hovered {
        orbit_input(&mut studio.orbit, &keyboard, &mouse, &motion, &scroll, 400.0);
        if !egui_input.wants_keyboard_input() && keyboard.just_pressed(KeyCode::KeyF) {
            studio.frame_request = true;
        }
    }
    if std::mem::take(&mut studio.frame_request) {
        let patch = (studio.size.clamp(1, 8) * CHUNK_SIZE as i32) as f32;
        studio.orbit.target = TERRAFORM_ORIGIN + Vec3::Y * studio.surface;
        studio.orbit.distance = patch * 1.35;
    }
    if let Ok(mut transform) = cameras.single_mut() {
        *transform = studio.orbit.transform();
    }
}

#[allow(clippy::too_many_arguments)]
fn sync_terraform_preview(
    mut commands: Commands,
    time: Res<Time<Real>>,
    editor: Res<EditorState>,
    pie: Res<PieState>,
    ui: Res<EditorUiState>,
    registry: Res<BlockRegistry>,
    library: Res<BiomeLibrary>,
    structures: Res<StructureLibrary>,
    mut studio: ResMut<BiomeStudio>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<PolyVoxelMaterial>>,
    roots: Query<Entity, With<TerraformPreviewRoot>>,
) {
    let active = in_terraformer(&editor, &pie);
    let signature = if active {
        format!(
            "{}|{}|{}|{}|{}|{}|{:?}",
            library.selection,
            library.revision,
            structures.revision,
            registry.revision,
            ui.shading.revision,
            studio.size,
            studio.offset
        )
    } else {
        String::new()
    };
    let now = time.elapsed_secs();
    if signature != studio.signature {
        // Leaving the studio clears immediately; edits wait for a pause.
        if !active || studio.signature.is_empty() || studio.rebuild_request {
            studio.pending_since = Some(now - DEBOUNCE);
        } else if studio.pending_since.is_none() {
            studio.pending_since = Some(now);
        }
        studio.signature = signature.clone();
    }
    let Some(since) = studio.pending_since else {
        return;
    };
    if now - since < DEBOUNCE && !studio.rebuild_request {
        return;
    }
    studio.pending_since = None;
    studio.rebuild_request = false;

    for root in &roots {
        commands.entity(root).despawn();
    }
    for handle in studio.meshes.drain(..) {
        meshes.remove(handle.id());
    }
    for handle in studio.materials.drain(..) {
        materials.remove(handle.id());
    }
    if !active {
        return;
    }
    let Some(biome) = library.biomes.get(library.selection) else {
        return;
    };

    // Generator restricted to the selected biome.
    let started = Instant::now();
    let solo = BiomeLibrary {
        biomes: vec![biome.clone()],
        settings: library.settings.clone(),
        ..Default::default()
    };
    let generator = TerrainGenerator::new(&solo, &structures, &registry);
    let size = studio.size.clamp(1, 8);
    let base = studio.offset;
    let chunk = CHUNK_SIZE as i32;

    // Lowest surface of the patch: deeper layers are never visible.
    let mut floor = i32::MAX;
    for z in (0..size * chunk).step_by(3) {
        for x in (0..size * chunk).step_by(3) {
            let info = generator.column(base.x * chunk + x, base.y * chunk + z);
            floor = floor.min(info.height.min(generator.settings.sea_level));
        }
    }
    let floor = floor - 3;

    let mut world = VoxelWorld::new();
    let mut voxels = 0;
    let mut placed = 0;
    for cz in 0..size {
        for cx in 0..size {
            placed += generator.structure_count(base + IVec2::new(cx, cz));
            for (cell, voxel) in generator.generate_column(base + IVec2::new(cx, cz)) {
                if cell.y < floor {
                    continue;
                }
                // Back to patch-local coordinates.
                let local = cell - IVec3::new(base.x * chunk, 0, base.y * chunk);
                if local.x < 0 || local.z < 0 || local.x >= size * chunk || local.z >= size * chunk {
                    continue;
                }
                world.set(local, Some(voxel));
                voxels += 1;
            }
        }
    }
    world.take_edits();
    studio.last_voxels = voxels;
    studio.last_structures = placed;

    let half = (size * chunk) as f32 * 0.5;
    let root = commands
        .spawn((
            Name::new("Terraform preview"),
            TerraformPreviewRoot,
            Transform::from_translation(TERRAFORM_ORIGIN - Vec3::new(half, 0.0, half)),
            Visibility::default(),
        ))
        .id();
    let mut cache: HashMap<BlockId, Handle<PolyVoxelMaterial>> = HashMap::new();
    let coords: Vec<IVec3> = world.chunks().map(|c| c.coord).collect();
    for coord in coords {
        let origin = coord.as_vec3() * chunk as f32;
        for (id, data) in mesher::build_chunk_mesh(&world, &registry, coord, true) {
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
            commands.spawn((
                Mesh3d(mesh),
                MeshMaterial3d(material),
                Transform::from_translation(origin),
                ChildOf(root),
            ));
        }
    }
    studio.last_millis = started.elapsed().as_secs_f32() * 1000.0;

    studio.surface = generator
        .column(base.x * chunk + (half as i32), base.y * chunk + (half as i32))
        .height
        .max(generator.settings.sea_level) as f32;
    if std::mem::take(&mut studio.frame_after_build) {
        studio.frame_request = true;
    }
}
