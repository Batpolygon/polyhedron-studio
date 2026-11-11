//! Scene catalogue synchronization.
//!
//! The hierarchy is derived from the voxel world rather than authored by hand.
//! It is rebuilt only when the world revision changes, and the current selection
//! is preserved by chunk coordinate.

use bevy::prelude::*;
use egui::Color32;

use crate::core::app_state::{EditorState, SceneEntry};
use crate::voxel::world::VoxelWorld;

/// Rebuild the hierarchy entries from the current chunks.
pub fn refresh_scene_entries(world: Res<VoxelWorld>, mut editor: ResMut<EditorState>) {
    if editor.scene_revision == world.revision && !editor.scene_entries.is_empty() {
        return;
    }

    let previous = editor.selected_coord();
    let stats = world.stats();
    let mut entries = Vec::with_capacity(stats.chunks + 1);

    entries.push(SceneEntry {
        name: "Voxel World".to_string(),
        kind: "Voxel World",
        color: Color32::from_rgb(78, 210, 190),
        coord: None,
        voxels: stats.voxels,
    });

    let mut chunks: Vec<_> = world.chunks().collect();
    chunks.sort_by_key(|chunk| (chunk.coord.x, chunk.coord.y, chunk.coord.z));

    for chunk in chunks {
        entries.push(SceneEntry {
            name: format!(
                "Chunk [{}, {}, {}]",
                chunk.coord.x, chunk.coord.y, chunk.coord.z
            ),
            kind: "Voxel Chunk",
            color: chunk_tint(chunk.coord.x * 31 + chunk.coord.y * 17 + chunk.coord.z * 47),
            coord: Some(chunk.coord),
            voxels: chunk.filled(),
        });
    }

    editor.scene_entries = entries;
    editor.scene_revision = world.revision;

    let selected = previous
        .and_then(|coord| {
            editor
                .scene_entries
                .iter()
                .position(|entry| entry.coord == Some(coord))
        })
        .unwrap_or(0);
    editor.selected_scene = selected.min(editor.scene_entries.len().saturating_sub(1));
}

fn chunk_tint(seed: i32) -> Color32 {
    match seed.rem_euclid(5) {
        0 => Color32::from_rgb(78, 210, 190),
        1 => Color32::from_rgb(102, 178, 255),
        2 => Color32::from_rgb(148, 140, 255),
        3 => Color32::from_rgb(255, 160, 112),
        _ => Color32::from_rgb(164, 224, 104),
    }
}
