//! Save/load orchestration shared by the menu, the project panel and the console.
//!
//! The voxel layer only knows how to serialize a world; this module turns that
//! into editor actions with paths, logging and dirty-state bookkeeping.

use std::fs;
use std::path::PathBuf;

use crate::core::app_state::EditorUiState;
use crate::core::project_io::ProjectLayout;
use crate::voxel::blocks::BlockRegistry;
use crate::voxel::persistence;
use crate::voxel::world::VoxelWorld;

/// Turn a user-provided name into a safe file stem.
pub fn sanitize_name(name: &str) -> String {
    let filtered: String = name
        .trim()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '-' || character == '_' {
                character
            } else {
                '_'
            }
        })
        .collect();

    if filtered.is_empty() {
        "untitled".to_string()
    } else {
        filtered
    }
}

pub fn world_path(project: &ProjectLayout, name: &str) -> PathBuf {
    project
        .worlds_path()
        .join(format!("{}.ron", sanitize_name(name)))
}

/// Save the current world under the name stored in the UI state.
pub fn save_current(
    world: &VoxelWorld,
    registry: &BlockRegistry,
    project: &ProjectLayout,
    state: &mut EditorUiState,
) -> bool {
    let name = sanitize_name(&state.world_name);
    state.world_name = name.clone();

    let path = world_path(project, &name);
    match persistence::save_to_path(world, registry, &path, &name) {
        Ok(saved) => {
            state.last_saved_revision = world.revision;
            state.action(format!(
                "Saved world '{}' - {} chunks, {} voxels -> {}",
                name,
                saved.chunks,
                saved.voxels,
                saved.path.display()
            ));
            true
        }
        Err(error) => {
            state.error(format!("World save failed: {error}"));
            false
        }
    }
}

/// Load `name` from `project/worlds/`, replacing the current world.
pub fn load_named(
    world: &mut VoxelWorld,
    registry: &BlockRegistry,
    project: &ProjectLayout,
    state: &mut EditorUiState,
    name: &str,
) -> bool {
    let name = sanitize_name(name);
    let path = world_path(project, &name);

    match persistence::load_from_path(world, registry, &path) {
        Ok(loaded) => {
            state.world_name = loaded.name.clone();
            state.last_saved_revision = world.revision;
            state.action(format!(
                "Loaded world '{}' - {} chunks, {} voxels.",
                loaded.name, loaded.chunks, loaded.voxels
            ));
            if loaded.skipped > 0 {
                state.warn(format!(
                    "{} cell(s) referenced unknown blocks and were skipped.",
                    loaded.skipped
                ));
            }
            true
        }
        Err(error) => {
            state.error(format!("World load failed: {error}"));
            false
        }
    }
}

/// World names available in the project folder, sorted alphabetically.
pub fn list_worlds(project: &ProjectLayout) -> Vec<String> {
    let Ok(entries) = fs::read_dir(project.worlds_path()) else {
        return Vec::new();
    };

    let mut names: Vec<String> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .map(|extension| extension.eq_ignore_ascii_case("ron"))
                .unwrap_or(false)
        })
        .filter_map(|path| {
            path.file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
        })
        .collect();
    names.sort();
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_sanitized_for_the_filesystem() {
        assert_eq!(sanitize_name("demo_world"), "demo_world");
        assert_eq!(sanitize_name("My World!"), "My_World_");
        assert_eq!(sanitize_name("   "), "untitled");
    }
}
