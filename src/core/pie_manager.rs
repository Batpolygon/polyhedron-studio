//! Play-In-Editor (PIE) state machine with snapshot isolation.
//!
//! Pressing `F7` while authoring:
//! 1. serializes an in-memory snapshot of the voxel world and the editor camera;
//! 2. hides the dock (the editor plugin switches to a full-window viewport);
//! 3. hands the camera to the player controller and activates gameplay systems.
//!
//! Pressing `F7` again stops the simulation, despawns every entity tagged
//! [`PieOnly`] (player, mobs, projectiles, movable bodies, particles), restores
//! the voxel world bit-for-bit from the snapshot and gives the fly camera back.
//!
//! Mode changes requested by the UI are applied by [`apply_pie_transition_system`]
//! because only an ECS system can touch the world, the camera and the entities.
//! Gameplay plugins read [`PieState::started_this_frame`] /
//! [`PieState::stopped_this_frame`] (ordered after [`PieSet::Transition`]) to
//! reset or tear down their own runtime state.

use bevy::prelude::*;

use crate::core::app_state::{EditorMode, EditorState, EditorUiState};
use crate::core::editor_camera::EditorCamera;
use crate::voxel::world::VoxelWorld;

/// Marker for entities that only exist while PIE runs.
#[derive(Component, Debug, Default, Clone, Copy)]
pub struct PieOnly;

/// Ordering anchor for PIE transitions.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PieSet {
    /// Input (F7) and snapshot / rollback.
    Transition,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PieTransition {
    Start,
    Stop,
}

/// Everything restored when PIE stops.
#[derive(Clone, Debug)]
pub struct PieSnapshot {
    pub world: VoxelWorld,
    pub camera: Transform,
    pub world_revision: u64,
    pub saved_revision: u64,
    pub voxels: usize,
}

/// Runtime state for Play-In-Editor.
#[derive(Resource, Debug, Default)]
pub struct PieState {
    pub elapsed: f32,
    pub was_playing: bool,
    pub pending: Option<PieTransition>,
    pub snapshot: Option<PieSnapshot>,
    /// True during the frame the simulation began (after the snapshot).
    pub started_this_frame: bool,
    /// True during the frame the world was rolled back.
    pub stopped_this_frame: bool,
    /// Cursor captured by the player controller (Esc releases it).
    pub cursor_captured: bool,
    /// Game started from the main menu: the played world is saved under
    /// this name when the game stops (before the editor rollback).
    pub persist_world: Option<String>,
}

impl PieState {
    pub fn is_playing(&self) -> bool {
        self.was_playing
    }

    pub fn toggle(&mut self, editor: &mut EditorState, ui: &mut EditorUiState) {
        if self.was_playing {
            self.stop(editor, ui);
        } else {
            self.start(editor, ui);
        }
    }

    pub fn start(&mut self, editor: &mut EditorState, ui: &mut EditorUiState) {
        if self.was_playing {
            return;
        }
        self.was_playing = true;
        self.elapsed = 0.0;
        self.pending = Some(PieTransition::Start);
        editor.mode = EditorMode::Playing;
        ui.action("PIE started: world snapshot taken, F7 to stop and roll back.");
    }

    pub fn stop(&mut self, editor: &mut EditorState, ui: &mut EditorUiState) {
        if !self.was_playing {
            return;
        }
        self.was_playing = false;
        self.pending = Some(PieTransition::Stop);
        editor.mode = EditorMode::Editing;
        ui.action(format!(
            "PIE stopped after {:.1}s. Restoring the snapshot.",
            self.elapsed
        ));
        self.elapsed = 0.0;
    }
}

/// F7 toggles PIE from anywhere in the window.
pub fn toggle_pie_system(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut pie: ResMut<PieState>,
    mut editor: ResMut<EditorState>,
    mut ui: ResMut<EditorUiState>,
) {
    if keyboard.just_pressed(KeyCode::F7) && !crate::core::projects::game_only() {
        pie.toggle(&mut editor, &mut ui);
    }
}

/// Take the snapshot on start, roll everything back on stop.
pub fn apply_pie_transition_system(
    mut commands: Commands,
    mut pie: ResMut<PieState>,
    mut world: ResMut<VoxelWorld>,
    mut ui: ResMut<EditorUiState>,
    mut cameras: Query<&mut Transform, With<EditorCamera>>,
    pie_entities: Query<Entity, With<PieOnly>>,
    registry: Res<crate::voxel::blocks::BlockRegistry>,
    project: Res<crate::core::project_io::ProjectLayout>,
    streaming: Res<crate::voxel::chunk_manager::WorldStreaming>,
) {
    pie.started_this_frame = false;
    pie.stopped_this_frame = false;

    let Some(transition) = pie.pending.take() else {
        return;
    };

    match transition {
        PieTransition::Start => {
            let camera = cameras
                .single()
                .map(|transform| *transform)
                .unwrap_or_default();
            let stats = world.stats();
            pie.snapshot = Some(PieSnapshot {
                world: world.clone(),
                camera,
                world_revision: world.revision,
                saved_revision: ui.last_saved_revision,
                voxels: stats.voxels,
            });
            pie.started_this_frame = true;
            pie.cursor_captured = true;
            ui.info(format!(
                "Snapshot: {} chunks / {} voxels held in RAM.",
                stats.chunks, stats.voxels
            ));
        }
        PieTransition::Stop => {
            if let Some(name) = pie.persist_world.take() {
                let path = project.worlds_path().join(format!("{name}.ron"));
                // An endless world keeps its edits, near or far; the seed gives the rest.
                let mut saved = if streaming.enabled { world.edited_columns() } else { world.clone() };
                for (column, chunks) in streaming.stored_columns() {
                    saved.restore_column(column, chunks.clone());
                }
                match crate::voxel::persistence::save_to_path(&saved, &registry, &path, &name) {
                    Ok(saved) => ui.action(format!("World '{name}' saved ({} chunks).", saved.chunks)),
                    Err(error) => ui.error(format!("Could not save the world: {error}")),
                }
            }
            let mut despawned = 0;
            for entity in &pie_entities {
                commands.entity(entity).despawn();
                despawned += 1;
            }

            if let Some(snapshot) = pie.snapshot.take() {
                let changed = world.revision != snapshot.world_revision;
                world.restore_from(&snapshot.world);
                // The restored content equals what was on disk if it was saved.
                if snapshot.saved_revision == snapshot.world_revision {
                    ui.last_saved_revision = world.revision;
                }
                if let Ok(mut transform) = cameras.single_mut() {
                    *transform = snapshot.camera;
                }
                ui.info(format!(
                    "Rollback complete: {} voxels restored{}, {despawned} runtime entities removed.",
                    snapshot.voxels,
                    if changed { " (world was modified in play)" } else { "" }
                ));
            }
            pie.stopped_this_frame = true;
            pie.cursor_captured = false;
        }
    }
}

/// Advance the simulation clock while PIE is active.
pub fn tick_pie_system(time: Res<Time>, mut pie: ResMut<PieState>, editor: Res<EditorState>) {
    if editor.mode.is_playing() {
        pie.elapsed += time.delta_secs();
    }
}
