//! Voxel data model, generation, meshing, persistence and editing.
//!
//! Stage 3 turns the world into authored project data:
//!
//! - [`blocks`] loads the data-driven block library from `project/blocks/`.
//! - [`world`] owns sparse 16^3 chunks of [`world::Voxel`] cells and tracks the
//!   chunks that need a remesh.
//! - [`generator`] builds the procedural demo island and showcase.
//! - [`mesher`] emits greedy, block-grouped surface meshes for the nine shapes.
//! - [`persistence`] reads and writes `project/worlds/*.ron`.
//! - [`picking`] projects the viewport cursor with a DDA and drives the brush.
//! - [`biomes`], [`world_gen`] and [`chunk_manager`] stream an infinite,
//!   biome-driven world on worker threads.
//! - [`structures`] holds the voxel prefabs and the isolated structure room.
//! - [`fluids`] makes liquid blocks flow.

pub mod biomes;
pub mod blocks;
pub mod chunk_manager;
pub mod fluids;
pub mod generator;
pub mod mesher;
pub mod persistence;
pub mod picking;
pub mod shapes;
pub mod structures;
pub mod world;
pub mod world_gen;

use bevy::prelude::*;

use crate::core::app_state::EditorUiState;
use crate::core::project_io::{initialize_project_system, ProjectLayout};
use crate::voxel::blocks::BlockRegistry;
use crate::voxel::picking::{VoxelBrushTimer, VoxelCursor};
use crate::voxel::world::VoxelWorld;

/// Ordered voxel phases. Input runs before the renderer and the editor sync.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VoxelSet {
    Input,
    Sync,
}

pub struct VoxelPlugin;

impl Plugin for VoxelPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            biomes::BiomePlugin,
            structures::StructurePlugin,
            chunk_manager::ChunkManagerPlugin,
        ))
        .init_resource::<BlockRegistry>()
            .insert_resource(VoxelWorld::new())
            .init_resource::<VoxelCursor>()
            .init_resource::<VoxelBrushTimer>()
            .init_resource::<fluids::FluidSim>()
            .configure_sets(Update, (VoxelSet::Input, VoxelSet::Sync).chain())
            .add_systems(
                Startup,
                (blocks::load_block_registry, startup_world)
                    .chain()
                    .after(initialize_project_system),
            )
            .add_systems(
                Update,
                (
                    fluids::sync_liquid_mask,
                    picking::update_voxel_cursor,
                    picking::apply_voxel_brush,
                )
                    .chain()
                    .in_set(VoxelSet::Input),
            )
            .add_systems(
                Update,
                fluids::simulate_fluids
                    .after(VoxelSet::Input)
                    .before(VoxelSet::Sync),
            );
    }
}

/// Load the last saved world when one exists, otherwise build the demo scene.
fn startup_world(
    mut world: ResMut<VoxelWorld>,
    registry: Res<BlockRegistry>,
    project: Res<ProjectLayout>,
    mut ui: ResMut<EditorUiState>,
) {
    let path = project.worlds_path().join("demo_world.ron");

    if path.is_file() {
        match persistence::load_from_path(&mut world, &registry, &path) {
            Ok(info) => {
                ui.world_name = info.name.clone();
                ui.last_saved_revision = world.revision;
                ui.info(format!(
                    "Loaded world '{}' ({} chunks, {} voxels) from {}.",
                    info.name,
                    info.chunks,
                    info.voxels,
                    path.display()
                ));
                if info.skipped > 0 {
                    ui.warn(format!(
                        "{} cell(s) referenced unknown blocks and were skipped.",
                        info.skipped
                    ));
                }
                return;
            }
            Err(error) => ui.warn(format!(
                "Could not load {}: {error}. Generating the demo world instead.",
                path.display()
            )),
        }
    }

    generator::generate_demo_world(&mut world, &registry);
    ui.info("Procedural demo world generated. Press Ctrl+S to save it.");
}
