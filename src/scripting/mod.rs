//! Blockly-style visual scripting.
//!
//! The module is split in three layers:
//! - [`model`] is the serializable block vocabulary and the project file format;
//! - [`library`] owns the editable programs/actions and their validation;
//! - [`vm`] snapshots the library and executes it with an explicit frame stack.
//!
//! The plugin loads the library after the project bootstrap, starts the VM with
//! Play-In-Editor, ticks it every frame, mirrors its log into the console and
//! draws the script avatar as a viewport gizmo.

pub mod context;
pub mod library;
pub mod model;
pub mod vm;

use bevy::prelude::*;

use crate::core::app_state::EditorUiState;
use crate::core::pie_manager::PieState;
use crate::core::project_io::{initialize_project_system, ProjectLayout};
use crate::scripting::library::ScriptLibrary;
use crate::scripting::vm::{ScriptLogLevel, ScriptRuntime};

/// The VM tick; the script bridge runs right after it.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ScriptVmSet;

pub struct ScriptingPlugin;

impl Plugin for ScriptingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ScriptLibrary>()
            .init_resource::<ScriptRuntime>()
            .add_systems(
                Startup,
                load_script_library_system.after(initialize_project_system),
            )
            .add_systems(
                Update,
                (
                    tick_scripts_system
                        .after(crate::core::pie_manager::PieSet::Transition)
                        .in_set(ScriptVmSet),
                    draw_script_avatar,
                )
                    .chain(),
            );
    }
}

fn load_script_library_system(
    mut library: ResMut<ScriptLibrary>,
    project: Res<ProjectLayout>,
    mut ui: ResMut<EditorUiState>,
) {
    match library.load_from_project(&project) {
        Ok(report) => {
            ui.info(format!(
                "Script library loaded: {} program(s), {} action(s).",
                report.programs, report.actions
            ));
            for warning in report.warnings {
                ui.warn(warning);
            }
            if report.created_defaults {
                ui.action(
                    "Created the starter Weave library in project/scripts and project/actions.",
                );
            }
        }
        Err(error) => ui.error(format!("Script library load failed: {error}")),
    }
}

fn tick_scripts_system(
    time: Res<Time>,
    pie: Res<PieState>,
    library: Res<ScriptLibrary>,
    mut runtime: ResMut<ScriptRuntime>,
    mut ui: ResMut<EditorUiState>,
) {
    let playing = pie.is_playing();

    if playing && !runtime.is_running() {
        runtime.start(&library, true);
        ui.action(format!(
            "PIE script VM started with {} program(s).",
            runtime.program_count()
        ));
    } else if playing && runtime.is_running() && !runtime.is_pie_owned() {
        runtime.start(&library, true);
        ui.action("PIE adopted the running script VM.");
    }

    if !playing && runtime.is_running() && runtime.is_pie_owned() {
        runtime.stop();
        ui.action("PIE script VM stopped.");
    }

    if runtime.is_running() {
        if runtime.last_revision() != library.revision {
            let pie_owned = runtime.is_pie_owned();
            runtime.start(&library, pie_owned);
            ui.warn("Script library changed; the VM restarted.");
        }
        runtime.tick(time.delta_secs());
    }

    for entry in runtime.drain_log() {
        match entry.level {
            ScriptLogLevel::Info => ui.info(format!("[{}] {}", entry.program, entry.message)),
            ScriptLogLevel::Action => ui.action(format!("[{}] {}", entry.program, entry.message)),
            ScriptLogLevel::Warn => ui.warn(format!("[{}] {}", entry.program, entry.message)),
            ScriptLogLevel::Error => ui.error(format!("[{}] {}", entry.program, entry.message)),
        }
    }
}

fn draw_script_avatar(mut gizmos: Gizmos, runtime: Res<ScriptRuntime>) {
    if !runtime.is_running() {
        return;
    }

    let (position, yaw) = runtime.avatar();
    let center = Vec3::new(position[0], position[1], position[2]);
    let forward = Vec3::new(yaw.sin(), 0.0, yaw.cos());

    gizmos.cube(
        Isometry3d::new(center, Quat::IDENTITY),
        Color::srgba(0.31, 0.82, 0.74, 0.75),
    );
    gizmos.line(
        center,
        center + forward * 1.4,
        Color::srgba(1.0, 0.72, 0.36, 0.9),
    );
}
