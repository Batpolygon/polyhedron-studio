//! Gamepad cursor: whenever the mouse is free (menus, inventory, machine
//! panels, death screen), the left stick of a gamepad moves the cursor and
//! the South button (A / Cross) clicks.

use bevy::input::mouse::MouseButtonInput;
use bevy::input::ButtonState;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};

/// Cursor speed at full stick, in logical pixels per second.
const CURSOR_SPEED: f32 = 900.0;
const DEAD_ZONE: f32 = 0.15;

pub fn gamepad_cursor(
    time: Res<Time<Real>>,
    gamepads: Query<&Gamepad>,
    mut windows: Query<(Entity, &mut Window, &CursorOptions), With<PrimaryWindow>>,
    mut clicks: MessageWriter<MouseButtonInput>,
) {
    let Ok((entity, mut window, cursor)) = windows.single_mut() else {
        return;
    };
    if cursor.grab_mode != CursorGrabMode::None {
        return;
    }
    for gamepad in &gamepads {
        let stick = gamepad.left_stick();
        if stick.length() > DEAD_ZONE {
            let size = Vec2::new(window.width(), window.height());
            let current = window.cursor_position().unwrap_or(size * 0.5);
            // Screen y grows downward, the stick's upward.
            let delta = Vec2::new(stick.x, -stick.y) * CURSOR_SPEED * time.delta_secs();
            let next = (current + delta).clamp(Vec2::ZERO, size - Vec2::ONE);
            window.set_cursor_position(Some(next));
        }
        for (pressed, state) in [(gamepad.just_pressed(GamepadButton::South), ButtonState::Pressed), (gamepad.just_released(GamepadButton::South), ButtonState::Released)] {
            if pressed {
                clicks.write(MouseButtonInput { button: MouseButton::Left, state, window: entity });
            }
        }
    }
}
