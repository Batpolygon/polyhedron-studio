//! Shared plumbing of the three preview studios (🐾 Sculpteur, 🔨 Forge,
//! 🌲 Terraformeur):
//!
//! - the voxel world ([`WorldVisual`]) is hidden while a studio is open and
//!   the world camera is saved / restored around it;
//! - [`Orbit`] is the turntable camera used by every studio preview;
//! - [`orbit_input`] applies the common mouse controls (right drag: orbit,
//!   middle drag or Shift + right drag: pan, wheel: zoom).

use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll};
use bevy::prelude::*;

use crate::core::app_state::{EditorState, Workspace};
use crate::core::editor_camera::EditorCamera;
use crate::core::pie_manager::PieState;

/// World-only visuals hidden in the studios.
#[derive(Component)]
pub struct WorldVisual;

#[derive(Clone, Copy, Debug)]
pub struct Orbit {
    pub target: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
}

impl Default for Orbit {
    fn default() -> Self {
        Self {
            target: Vec3::ZERO,
            yaw: 0.6,
            pitch: -0.35,
            distance: 4.5,
        }
    }
}

impl Orbit {
    pub fn transform(&self) -> Transform {
        let rotation = Quat::from_euler(EulerRot::YXZ, self.yaw, self.pitch, 0.0);
        Transform::from_translation(self.target + rotation * Vec3::new(0.0, 0.0, self.distance))
            .with_rotation(rotation)
    }
}

/// Apply the shared turntable controls to an orbit.
pub fn orbit_input(
    orbit: &mut Orbit,
    keyboard: &ButtonInput<KeyCode>,
    mouse: &ButtonInput<MouseButton>,
    motion: &AccumulatedMouseMotion,
    scroll: &AccumulatedMouseScroll,
    max_distance: f32,
) {
    let shift = keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight);
    let pan = mouse.pressed(MouseButton::Middle) || (shift && mouse.pressed(MouseButton::Right));
    if pan {
        let rotation = Quat::from_euler(EulerRot::YXZ, orbit.yaw, orbit.pitch, 0.0);
        let scale = orbit.distance * 0.0016;
        orbit.target += rotation * Vec3::new(-motion.delta.x, motion.delta.y, 0.0) * scale;
    } else if mouse.pressed(MouseButton::Right) {
        orbit.yaw -= motion.delta.x * 0.006;
        orbit.pitch = (orbit.pitch - motion.delta.y * 0.006).clamp(-1.5, 1.5);
    }
    if scroll.delta.y.abs() > f32::EPSILON {
        orbit.distance = (orbit.distance * (-scroll.delta.y * 0.12).exp()).clamp(0.4, max_distance);
    }
}

/// Workspace actually shown (PIE always shows the world).
pub fn effective_workspace(editor: &EditorState, pie: &PieState) -> Workspace {
    if pie.is_playing() { Workspace::World } else { editor.workspace }
}

#[derive(Resource, Debug, Default)]
pub struct StudioCamera {
    saved_world_camera: Option<Transform>,
    in_studio: bool,
}

pub struct StudioCommonPlugin;

impl Plugin for StudioCommonPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<StudioCamera>()
            .add_systems(Update, manage_studio_world.before(StudioSet));
    }
}

/// Studio systems run after the world was hidden and the camera saved.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StudioSet;

fn manage_studio_world(
    editor: Res<EditorState>,
    pie: Res<PieState>,
    mut state: ResMut<StudioCamera>,
    mut cameras: Query<(&mut Transform, &mut EditorCamera)>,
    mut visuals: Query<&mut Visibility, With<WorldVisual>>,
) {
    let studio = !effective_workspace(&editor, &pie).shows_world();
    if studio && !state.in_studio {
        if let Ok((transform, _)) = cameras.single() {
            state.saved_world_camera = Some(*transform);
        }
    }
    if !studio && state.in_studio {
        if let (Some(saved), Ok((mut transform, mut camera))) = (state.saved_world_camera.take(), cameras.single_mut()) {
            *transform = saved;
            let (yaw, pitch, _) = saved.rotation.to_euler(EulerRot::YXZ);
            camera.yaw = yaw;
            camera.pitch = pitch;
        }
    }
    state.in_studio = studio;

    // Newly streamed chunks also follow the current visibility.
    let wanted = if studio { Visibility::Hidden } else { Visibility::Inherited };
    for mut visibility in &mut visuals {
        if *visibility != wanted {
            *visibility = wanted;
        }
    }
}
