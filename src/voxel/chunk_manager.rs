//! Infinite world streaming with asynchronous generation.
//!
//! When streaming is on, chunk columns within `radius` of the focus (the
//! player in PIE, the editor camera otherwise) are requested from a pool of
//! worker threads that evaluate the [`TerrainGenerator`]. Finished columns
//! are inserted on the main thread with a per-frame budget; the mesher and
//! the Rapier trimeshes pick them up through the regular dirty set.
//!
//! A column that already holds chunks (loaded from disk, edited, or restored
//! by the PIE snapshot) is never overwritten. Columns generated during PIE
//! are forgotten on rollback so they regenerate identically.
//!
//! Far columns are unloaded (on by default): untouched ones are simply
//! dropped and regenerated later, edited ones are kept in memory and put
//! back exactly as they were when the player comes back.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::thread;

use bevy::prelude::*;
use crossbeam_channel::{Receiver, Sender};

use crate::core::app_state::EditorUiState;
use crate::core::editor_camera::EditorCamera;
use crate::core::pie_manager::{PieSet, PieState};
use crate::physics::player_controller::Player;
use crate::voxel::biomes::BiomeLibrary;
use crate::voxel::blocks::BlockRegistry;
use crate::voxel::structures::StructureLibrary;
use crate::voxel::world::{Voxel, VoxelChunk, VoxelWorld, CHUNK_SIZE};
use crate::voxel::world_gen::TerrainGenerator;

const MAX_IN_FLIGHT: usize = 16;
const MAX_INSERTS_PER_FRAME: usize = 4;

struct Job {
    generation: u64,
    coord: IVec2,
    generator: Arc<TerrainGenerator>,
}

struct JobResult {
    generation: u64,
    coord: IVec2,
    cells: Vec<(IVec3, Voxel)>,
}

#[derive(Resource)]
pub struct WorldStreaming {
    pub enabled: bool,
    /// Radius in chunk columns.
    pub radius: i32,
    /// Unload columns beyond `radius + 2` (edited ones stay in memory).
    pub unload: bool,
    pub generator: Option<Arc<TerrainGenerator>>,
    generation: u64,
    generated: HashSet<IVec2>,
    in_flight: HashSet<IVec2>,
    ready: Vec<JobResult>,
    jobs: Sender<Job>,
    results: Receiver<JobResult>,
    /// Biome under the focus, for the status bar and the sky.
    pub focus_biome: Option<String>,
    pub columns_generated: u64,
    /// Edited columns unloaded from the world, restored when needed.
    stored: HashMap<IVec2, Vec<VoxelChunk>>,
    /// `stored` as it was when PIE started (restored on rollback).
    stored_snapshot: Option<HashMap<IVec2, Vec<VoxelChunk>>>,
}

impl WorldStreaming {
    fn new() -> Self {
        let (jobs, job_rx) = crossbeam_channel::unbounded::<Job>();
        let (result_tx, results) = crossbeam_channel::unbounded::<JobResult>();
        let workers = thread::available_parallelism()
            .map(|n| n.get().saturating_sub(2))
            .unwrap_or(2)
            .clamp(2, 6);
        for index in 0..workers {
            let job_rx = job_rx.clone();
            let result_tx = result_tx.clone();
            let spawned = thread::Builder::new()
                .name(format!("polyhedron-worldgen-{index}"))
                .spawn(move || {
                    while let Ok(job) = job_rx.recv() {
                        let cells = job.generator.generate_column(job.coord);
                        if result_tx
                            .send(JobResult {
                                generation: job.generation,
                                coord: job.coord,
                                cells,
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                });
            if let Err(error) = spawned {
                warn!("world generation worker {index} failed to start: {error}");
            }
        }
        Self {
            enabled: false,
            radius: 5,
            unload: true,
            generator: None,
            generation: 0,
            generated: HashSet::new(),
            in_flight: HashSet::new(),
            ready: Vec::new(),
            jobs,
            results,
            focus_biome: None,
            columns_generated: 0,
            stored: HashMap::new(),
            stored_snapshot: None,
        }
    }

    /// Edited columns kept out of the world.
    pub fn stored_columns(&self) -> impl Iterator<Item = (IVec2, &Vec<VoxelChunk>)> {
        self.stored.iter().map(|(column, chunks)| (*column, chunks))
    }

    /// Edited columns currently kept out of the world.
    pub fn stored_count(&self) -> usize {
        self.stored.len()
    }

    /// Rebuild the generator from the libraries and invalidate pending work.
    pub fn rebuild(&mut self, biomes: &BiomeLibrary, structures: &StructureLibrary, registry: &BlockRegistry) {
        self.generator = Some(Arc::new(TerrainGenerator::new(biomes, structures, registry)));
        self.generation = self.generation.wrapping_add(1);
        self.in_flight.clear();
        self.ready.clear();
    }

    /// Start a fresh infinite world: clears the voxel world.
    pub fn start_new_world(
        &mut self,
        world: &mut VoxelWorld,
        biomes: &BiomeLibrary,
        structures: &StructureLibrary,
        registry: &BlockRegistry,
    ) {
        world.clear();
        self.generated.clear();
        self.stored.clear();
        self.rebuild(biomes, structures, registry);
        self.enabled = true;
    }

    /// Mark the columns that already exist in the world as generated.
    pub fn adopt_existing(&mut self, world: &VoxelWorld) {
        self.generated = world.chunks().map(|chunk| IVec2::new(chunk.coord.x, chunk.coord.z)).collect();
    }

    pub fn pending(&self) -> usize {
        self.in_flight.len() + self.ready.len()
    }

    pub fn generated_count(&self) -> usize {
        self.generated.len()
    }

    fn column_exists(world: &VoxelWorld, coord: IVec2, generator: &TerrainGenerator) -> bool {
        let size = CHUNK_SIZE as i32;
        let low = generator.settings.bedrock.div_euclid(size);
        let high = generator.top().div_euclid(size);
        (low..=high).any(|cy| world.has_chunk(IVec3::new(coord.x, cy, coord.y)))
    }
}

pub struct ChunkManagerPlugin;

impl Plugin for ChunkManagerPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(WorldStreaming::new()).add_systems(
            Update,
            stream_chunks
                .after(PieSet::Transition)
                .before(crate::voxel::VoxelSet::Sync),
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn stream_chunks(
    pie: Res<PieState>,
    mut streaming: ResMut<WorldStreaming>,
    mut world: ResMut<VoxelWorld>,
    mut rules: ResMut<crate::gameplay::world_rules::WorldRules>,
    players: Query<&Transform, With<Player>>,
    cameras: Query<&Transform, (With<EditorCamera>, Without<Player>)>,
    room: Res<crate::voxel::structures::StructureRoom>,
    mut ui: ResMut<EditorUiState>,
) {
    let streaming = &mut *streaming;
    if pie.started_this_frame {
        streaming.stored_snapshot = Some(streaming.stored.clone());
    }
    // Rollback: forget columns that only existed during play.
    if pie.stopped_this_frame {
        if let Some(stored) = streaming.stored_snapshot.take() {
            streaming.stored = stored;
        }
        let generated: Vec<IVec2> = streaming.generated.iter().copied().collect();
        if let Some(generator) = streaming.generator.clone() {
            for coord in generated {
                if !WorldStreaming::column_exists(&world, coord, &generator) {
                    streaming.generated.remove(&coord);
                }
            }
        }
        streaming.in_flight.clear();
        streaming.ready.clear();
        streaming.generation = streaming.generation.wrapping_add(1);
    }

    // Collect finished jobs.
    while let Ok(result) = streaming.results.try_recv() {
        if result.generation == streaming.generation {
            streaming.ready.push(result);
        }
    }
    if !streaming.enabled || room.editing.is_some() {
        return;
    }
    let Some(generator) = streaming.generator.clone() else {
        return;
    };

    let focus = players
        .single()
        .ok()
        .filter(|_| pie.is_playing())
        .or_else(|| cameras.single().ok())
        .map(|t| t.translation)
        .unwrap_or(Vec3::ZERO);
    let size = CHUNK_SIZE as i32;
    let center = IVec2::new((focus.x as i32).div_euclid(size), (focus.z as i32).div_euclid(size));

    // Biome under the focus drives the sky colour.
    let info = generator.column(focus.x as i32, focus.z as i32);
    let name = generator.biome_name(info.biome).to_string();
    rules.biome_sky = generator.biome_sky(info.biome);
    if streaming.focus_biome.as_deref() != Some(name.as_str()) {
        if pie.is_playing() {
            ui.info(format!("Entering {name}."));
        }
        streaming.focus_biome = Some(name);
    }

    // Insert finished columns, nearest first.
    streaming
        .ready
        .sort_by_key(|result| -(result.coord - center).length_squared());
    for _ in 0..MAX_INSERTS_PER_FRAME {
        let Some(result) = streaming.ready.pop() else {
            break;
        };
        streaming.in_flight.remove(&result.coord);
        if streaming.generated.contains(&result.coord)
            || WorldStreaming::column_exists(&world, result.coord, &generator)
        {
            streaming.generated.insert(result.coord);
            continue;
        }
        world.set_bulk(result.cells);
        streaming.generated.insert(result.coord);
        streaming.columns_generated += 1;
    }

    // Request missing columns in a spiral around the focus.
    let radius = streaming.radius.clamp(1, 16);
    let mut wanted: Vec<IVec2> = Vec::new();
    for dz in -radius..=radius {
        for dx in -radius..=radius {
            if dx * dx + dz * dz > radius * radius {
                continue;
            }
            let coord = center + IVec2::new(dx, dz);
            if streaming.generated.contains(&coord) || streaming.in_flight.contains(&coord) {
                continue;
            }
            // Edited columns come back from memory, exactly as left.
            if let Some(chunks) = streaming.stored.remove(&coord) {
                world.restore_column(coord, chunks);
                streaming.generated.insert(coord);
                continue;
            }
            wanted.push(coord);
        }
    }
    wanted.sort_by_key(|coord| (*coord - center).length_squared());
    for coord in wanted {
        if streaming.in_flight.len() >= MAX_IN_FLIGHT {
            break;
        }
        let job = Job {
            generation: streaming.generation,
            coord,
            generator: generator.clone(),
        };
        if streaming.jobs.send(job).is_ok() {
            streaming.in_flight.insert(coord);
        }
    }

    // Optional unloading of far columns.
    if streaming.unload {
        let far = radius + 2;
        let to_drop: Vec<IVec2> = streaming
            .generated
            .iter()
            .copied()
            .filter(|coord| (*coord - center).abs().max_element() > far)
            .collect();
        let low = generator.settings.bedrock.div_euclid(size);
        let high = generator.top().div_euclid(size);
        for coord in to_drop {
            let modified = world.column_modified(coord);
            let chunks = world.take_column(coord, low, high);
            if modified && !chunks.is_empty() {
                streaming.stored.insert(coord, chunks);
            }
            streaming.generated.remove(&coord);
        }
    }
}
