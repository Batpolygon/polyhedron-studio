//! POLYHEDRON STUDIO
//!
//! Entry point for the engine and its in-engine studio. The binary is deliberately
//! small: every subsystem is a Bevy plugin so the editor and the runtime can share
//! the same world, renderer and serialization layer.

// Release builds open no console window next to the studio.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio;
mod core;
mod editor;
mod gameplay;
mod physics;
mod renderer;
mod scripting;
mod ui;
mod voxel;

use bevy::prelude::*;
use bevy::window::{PresentMode, PrimaryWindow, WindowResolution};
use bevy_egui::EguiPlugin;

use crate::audio::AudioPlugin;
use crate::core::CorePlugin;
use crate::editor::EditorPlugin;
use crate::gameplay::GameplayPlugin;
use crate::physics::PhysicsPlugin;
use crate::renderer::RendererPlugin;
use crate::scripting::ScriptingPlugin;
use crate::ui::UmgPlugin;
use crate::voxel::VoxelPlugin;

fn main() -> AppExit {
    let game = crate::core::projects::game_info();
    if game.is_some() {
        // An exported game runs from its own folder.
        if let Some(folder) = std::env::current_exe().ok().and_then(|exe| exe.parent().map(std::path::Path::to_path_buf)) {
            let _ = std::env::set_current_dir(folder);
        }
    }
    let title = game.map_or_else(|| "Polyhedron Studio".to_string(), |game| game.title.clone());
    App::new()
        .insert_resource(ClearColor(Color::srgb(0.018, 0.023, 0.031)))
        .add_plugins(
            DefaultPlugins
                .set(ImagePlugin::default_nearest())
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title,
                        resolution: WindowResolution::new(1600, 900),
                        present_mode: PresentMode::AutoVsync,
                        ..default()
                    }),
                    ..default()
                }),
        )
        .add_plugins(EguiPlugin::default())
        .add_plugins((
            CorePlugin,
            crate::core::game_rules::GameRulesPlugin,
            crate::gameplay::atmosphere::AtmospherePlugin,
            crate::gameplay::quests::QuestPlugin,
            VoxelPlugin,
            RendererPlugin,
            PhysicsPlugin,
            ScriptingPlugin,
            AudioPlugin,
            GameplayPlugin,
            EditorPlugin,
            UmgPlugin,
        ))
        .add_systems(Startup, maximize_primary_window)
        .add_systems(Update, show_window_icon)
        .run()
}

/// Cube drawn by `tools/design_icon.py`.
const WINDOW_ICON: &[u8] = include_bytes!("../assets/icon/window_64.rgba");

/// The window exists a few frames after startup; its icon is set once.
fn show_window_icon(
    windows: Query<Entity, With<PrimaryWindow>>,
    mut done: Local<bool>,
    _main_thread: bevy::ecs::system::NonSendMarker,
) {
    if *done {
        return;
    }
    let Ok(entity) = windows.single() else {
        return;
    };
    bevy::winit::WINIT_WINDOWS.with_borrow(|winit_windows| {
        if let Some(window) = winit_windows.get_window(entity) {
            *done = true;
            window.set_window_icon(winit::window::Icon::from_rgba(WINDOW_ICON.to_vec(), 64, 64).ok());
        }
    });
}

/// The studio is designed to be used maximized: at high DPI the requested
/// 1600x900 surface can be smaller than the monitor work area, and the dock
/// layout needs the room for its five panels.
fn maximize_primary_window(mut windows: Query<&mut Window, With<PrimaryWindow>>) {
    if let Ok(mut window) = windows.single_mut() {
        window.set_maximized(true);
    }
}
