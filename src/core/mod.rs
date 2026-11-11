//! Engine core: shared state, editor camera, project I/O and PIE.

pub mod app_state;
pub mod editor_camera;
pub mod game_rules;
pub mod gamepad;
pub mod pie_manager;
pub mod project_io;
pub mod projects;
pub mod settings;

use bevy::prelude::*;

use crate::core::app_state::{EditorState, EditorUiState};
use crate::core::editor_camera::EditorCameraPlugin;
use crate::core::pie_manager::{PieSet, PieState};
use crate::core::project_io::ProjectLayout;

pub struct CorePlugin;

impl Plugin for CorePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<EditorState>()
            .init_resource::<EditorUiState>()
            .init_resource::<PieState>()
            .init_resource::<ProjectLayout>()
            .init_resource::<settings::GameSettings>()
            .init_resource::<settings::RebindRequest>()
            .add_plugins(EditorCameraPlugin)
            .add_systems(
                Startup,
                (project_io::initialize_project_system, settings::load_settings_system).chain(),
            )
            .add_systems(
                Update,
                (settings::capture_rebind_system, settings::apply_audio_settings_system, gamepad::gamepad_cursor),
            )
            .add_systems(
                Update,
                (
                    pie_manager::toggle_pie_system,
                    pie_manager::apply_pie_transition_system,
                    pie_manager::tick_pie_system,
                )
                    .chain()
                    .in_set(PieSet::Transition),
            );
    }
}
