//! Mob and action editor backend.
//!
//! - [`model`] is the serializable rig, socket, clip and keyframe vocabulary;
//! - [`library`] owns the editable rigs, their validation and `.ron` files;
//! - [`pose`] samples clips and composes the box hierarchy for the viewport.
//!
//! The plugin loads the library after the project bootstrap, advances the
//! preview clock. The 🐾 Mobs workspace ([`crate::editor::mob_studio`])
//! renders the rig and its gizmos in the viewport.

pub mod bestiary;
pub mod brain;
pub mod library;
pub mod model;
pub mod pose;
pub mod runtime;

use bevy::prelude::*;

use crate::core::app_state::EditorUiState;
use crate::core::project_io::{initialize_project_system, ProjectLayout};
use crate::gameplay::mobs::library::{MobLibrary, MobPreview};

pub struct MobPlugin;

impl Plugin for MobPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(runtime::MobRuntimePlugin);
        app.init_resource::<MobLibrary>().add_systems(
            Startup,
            load_mob_library_system.after(initialize_project_system),
        );
        app.add_systems(
            Update,
            tick_mob_preview_system,
        );
    }
}

fn load_mob_library_system(
    mut library: ResMut<MobLibrary>,
    project: Res<ProjectLayout>,
    mut ui: ResMut<EditorUiState>,
) {
    match library.load_from_project(&project) {
        Ok(report) => {
            ui.info(format!("Mob library loaded: {} rig(s).", report.rigs));
            for warning in report.warnings {
                ui.warn(warning);
            }
            if report.created_defaults {
                ui.action("Created the starter mob in project/entities/mobs.");
            }
        }
        Err(error) => ui.error(format!("Mob library load failed: {error}")),
    }
}

fn tick_mob_preview_system(time: Res<Time>, mut library: ResMut<MobLibrary>) {
    if !library.loaded {
        return;
    }
    let duration = library.clip_duration();
    let looping = library.clip_looping();
    let preview = &mut library.preview;
    preview.advance(time.delta_secs(), duration, looping);
}

/// Rewind the preview to the first frame and pause it.
pub fn rewind_preview(preview: &mut MobPreview) {
    preview.time = 0.0;
    preview.playing = false;
}
