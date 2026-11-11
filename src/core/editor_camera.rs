//! Editor fly camera.
//!
//! The camera is a normal Bevy perspective camera whose viewport is synchronized
//! with the docked viewport tab. Controls are only consumed while the pointer is
//! over the 3D view, so typing in a panel never moves the editor camera.

use bevy::camera::Viewport;
use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy_egui::EguiPostUpdateSet;
use bevy_egui::input::EguiWantsInput;

use crate::core::app_state::{EditorMode, EditorState, EditorUiState};

/// Marker and tuning for the studio camera.
#[derive(Component, Debug)]
pub struct EditorCamera {
    pub yaw: f32,
    pub pitch: f32,
    pub speed: f32,
    pub min_speed: f32,
    pub max_speed: f32,
}

impl Default for EditorCamera {
    fn default() -> Self {
        Self {
            yaw: 0.66,
            pitch: -0.34,
            speed: 9.0,
            min_speed: 0.75,
            max_speed: 80.0,
        }
    }
}

pub struct EditorCameraPlugin;

impl Plugin for EditorCameraPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_editor_camera)
            .add_systems(Update, fly_editor_camera)
            .add_systems(
                PostUpdate,
                sync_editor_viewport.after(EguiPostUpdateSet::ProcessOutput),
            );
    }
}

fn spawn_editor_camera(mut commands: Commands) {
    let camera = EditorCamera::default();
    let rotation = Quat::from_euler(EulerRot::YXZ, camera.yaw, camera.pitch, 0.0);

    commands.spawn((
        Camera3d::default(),
        Camera {
            viewport: None,
            ..default()
        },
        Transform::from_xyz(16.0, 11.5, 21.0).with_rotation(rotation),
        camera,
    ));
}

fn fly_editor_camera(
    time: Res<Time>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    mouse_motion: Res<AccumulatedMouseMotion>,
    mouse_scroll: Res<AccumulatedMouseScroll>,
    editor: Res<EditorState>,
    ui: Res<EditorUiState>,
    egui_input: Res<EguiWantsInput>,
    mut cameras: Query<(&mut Transform, &mut EditorCamera)>,
) {
    if editor.mode != EditorMode::Editing || !editor.workspace.shows_world() {
        return;
    }

    let Ok((mut transform, mut camera)) = cameras.single_mut() else {
        return;
    };

    let viewport_active = ui.viewport_hovered && !egui_input.wants_keyboard_input();

    if viewport_active && mouse_buttons.pressed(MouseButton::Right) {
        const LOOK_SENSITIVITY: f32 = 0.0028;
        camera.yaw -= mouse_motion.delta.x * LOOK_SENSITIVITY;
        camera.pitch = (camera.pitch - mouse_motion.delta.y * LOOK_SENSITIVITY)
            .clamp(-1.54, 1.54);
        transform.rotation = Quat::from_euler(EulerRot::YXZ, camera.yaw, camera.pitch, 0.0);
    }

    if viewport_active && mouse_scroll.delta.y.abs() > f32::EPSILON {
        camera.speed = (camera.speed * (-mouse_scroll.delta.y * 0.12).exp())
            .clamp(camera.min_speed, camera.max_speed);
    }

    if !viewport_active {
        return;
    }

    let mut direction = Vec3::ZERO;
    if keyboard.pressed(KeyCode::KeyW) || keyboard.pressed(KeyCode::KeyZ) {
        direction += transform.rotation * Vec3::NEG_Z;
    }
    if keyboard.pressed(KeyCode::KeyS) {
        direction += transform.rotation * Vec3::Z;
    }
    if keyboard.pressed(KeyCode::KeyA) || keyboard.pressed(KeyCode::KeyQ) {
        direction += transform.rotation * Vec3::NEG_X;
    }
    if keyboard.pressed(KeyCode::KeyD) {
        direction += transform.rotation * Vec3::X;
    }
    if keyboard.pressed(KeyCode::KeyE) || keyboard.pressed(KeyCode::Space) {
        direction += Vec3::Y;
    }
    if keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ControlLeft) {
        direction -= Vec3::Y;
    }

    if direction.length_squared() > f32::EPSILON {
        transform.translation +=
            direction.normalize() * camera.speed * time.delta_secs();
    }
}

/// Convert the egui viewport rectangle into physical pixels and apply it to the
/// render camera. Runs after the egui context pass so it always uses the layout
/// produced by the current frame.
fn sync_editor_viewport(
    ui: Res<EditorUiState>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut cameras: Query<&mut Camera, With<EditorCamera>>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let Some(rect) = ui.viewport_rect else {
        return;
    };

    let physical_size = window.physical_size();
    if physical_size.x < 2 || physical_size.y < 2 {
        return;
    }

    // Expand by a couple of physical pixels so rounding can never leave an
    // uninitialized strip between the 3D viewport and the surrounding panels.
    // The panels are painted after the scene, so the overlap is invisible.
    const OVERSCAN: i32 = 2;

    let scale = ui.pixels_per_point.max(0.1);
    let x = ((rect.min.x * scale).floor() as i32 - OVERSCAN)
        .clamp(0, physical_size.x as i32 - 1) as u32;
    let y = ((rect.min.y * scale).floor() as i32 - OVERSCAN)
        .clamp(0, physical_size.y as i32 - 1) as u32;
    let right = ((rect.max.x * scale).ceil() as i32 + OVERSCAN)
        .clamp(x as i32 + 1, physical_size.x as i32);
    let bottom = ((rect.max.y * scale).ceil() as i32 + OVERSCAN)
        .clamp(y as i32 + 1, physical_size.y as i32);

    let width = (right - x as i32).max(1) as u32;
    let height = (bottom - y as i32).max(1) as u32;

    let viewport = Viewport {
        physical_position: UVec2::new(x, y),
        physical_size: UVec2::new(width, height),
        depth: 0.0..1.0,
    };

    for mut camera in &mut cameras {
        camera.viewport = Some(viewport.clone());
    }
}
