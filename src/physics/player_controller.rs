//! Kinematic player controller used during Play-In-Editor.
//!
//! The player is a Rapier capsule driven by a [`KinematicCharacterController`]:
//! autostep climbs half-blocks and stairs, snap-to-ground keeps it glued to
//! ramps, and the chunk trimeshes give exact collisions on every shape.
//!
//! Controls are remappable in Settings → Touches (defaults, QWERTY and AZERTY):
//! - `W`/`Z` `S` `A`/`Q` `D`: move, mouse: look, `Space`: jump / swim up;
//! - `Shift`: sprint (drains stamina), `Ctrl`: dash (2 stamina);
//! - creative: double-tap `Space` to fly (`Space` up, `Shift` down, `Ctrl` fast);
//! - `F5`: first / third person, `Esc`: pause menu.
//!
//! Liquids have no collision: inside one the avatar swims (slow sinking,
//! `Space` to rise, drag on every axis) and can hop out onto a ledge.
//!
//! The camera is the editor camera entity: its transform is saved in the PIE
//! snapshot and restored when PIE stops.

use crate::core::game_rules::game_rules;
use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};
use bevy_rapier3d::prelude::*;

use crate::core::app_state::EditorUiState;
use crate::core::editor_camera::EditorCamera;
use crate::core::pie_manager::{PieOnly, PieSet, PieState};
use crate::core::settings::{GameAction, GameSettings};
use crate::gameplay::player_stats::PlayerStats;
use crate::voxel::picking::raycast;
use crate::voxel::world::VoxelWorld;



/// Runtime state of the PIE avatar.
#[derive(Component, Debug)]
pub struct Player {
    pub yaw: f32,
    pub pitch: f32,
    pub velocity: Vec3,
    pub grounded: bool,
    pub third_person: bool,
    pub dash_timer: f32,
    pub dash_cooldown: f32,
    pub dash_direction: Vec3,
    pub spawn: Vec3,
    /// Horizontal speed of the last frame, used by footsteps.
    pub ground_speed: f32,
    /// Seated in a wagon: movement input drives the vehicle instead. For a
    /// seat of the static world it holds the player entity itself.
    pub seated: Option<Entity>,
    /// Seat of a movable the player sits on (index in `MovableBody::seats`).
    pub seat_index: usize,
    /// Wagon seat of the static world the player sits on.
    pub seat_block: Option<IVec3>,
    /// Remaining time of an external knockback impulse.
    pub knockback: Vec3,
    /// Body (feet or chest) inside a liquid: swimming physics.
    pub swimming: bool,
    /// Eyes inside a liquid: underwater tint.
    pub head_under: Option<[u8; 3]>,
    /// The last move was stopped horizontally (wall, ledge).
    pub blocked: bool,
    /// Distance walked, drives the head bob and limb swing.
    pub stride: f32,
    /// Creative flight (double-tap Jump to toggle).
    pub flying: bool,
    /// Seconds since the last Jump press, to detect the double tap.
    pub since_jump_tap: f32,
    /// Jumps made in the air since the last landing (armour extra jumps).
    pub air_jumps: u32,
}

impl Player {
    pub fn forward(&self) -> Vec3 {
        Vec3::new(-self.yaw.sin(), 0.0, -self.yaw.cos())
    }

    pub fn look_rotation(&self) -> Quat {
        Quat::from_euler(EulerRot::YXZ, self.yaw, self.pitch, 0.0)
    }

    pub fn look_direction(&self) -> Vec3 {
        self.look_rotation() * Vec3::NEG_Z
    }
}

/// Movement intent shared with vehicles (a seated player drives the wagon).
#[derive(Resource, Debug, Default, Clone, Copy)]
pub struct PlayerIntent {
    /// x = strafe, y = forward, in `[-1, 1]`.
    pub axis: Vec2,
    pub jump: bool,
    pub sprint: bool,
    /// Sneaking with the Remote: input drives a movable, not the avatar.
    pub remote: bool,
}

/// Input and movement of the PIE avatar; gameplay systems run after it.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PlayerControllerSet;

pub struct PlayerControllerPlugin;

impl Plugin for PlayerControllerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PlayerIntent>()
            .add_systems(
                Update,
                (
                    spawn_player_on_pie_start,
                    capture_cursor,
                    read_player_input,
                    move_player,
                )
                    .chain()
                    .in_set(PlayerControllerSet)
                    .after(PieSet::Transition),
            )
            .add_systems(
                PostUpdate,
                (read_controller_output, follow_player_camera)
                    .chain()
                    .after(PhysicsSet::Writeback),
            );
    }
}

fn spawn_player_on_pie_start(
    mut commands: Commands,
    pie: Res<PieState>,
    world: Res<VoxelWorld>,
    cameras: Query<&Transform, With<EditorCamera>>,
    mut ui: ResMut<EditorUiState>,
) {
    if !pie.started_this_frame {
        return;
    }

    let camera = cameras.single().copied().unwrap_or_default();
    // Drop the player straight below the editor camera onto the first surface.
    let mut spawn = camera.translation;
    if let Some(hit) = raycast(&world, camera.translation, Vec3::NEG_Y, 256.0) {
        spawn = hit.voxel.as_vec3() + Vec3::new(0.5, 1.0 + game_rules().player.half_height + game_rules().player.radius + 0.05, 0.5);
    }
    let (yaw, _, _) = camera.rotation.to_euler(EulerRot::YXZ);

    commands.spawn((
        Name::new("PIE Player"),
        PieOnly,
        Player {
            yaw,
            pitch: 0.0,
            velocity: Vec3::ZERO,
            grounded: false,
            third_person: false,
            dash_timer: 0.0,
            dash_cooldown: 0.0,
            dash_direction: Vec3::ZERO,
            spawn,
            ground_speed: 0.0,
            seated: None,
            seat_index: 0,
            seat_block: None,
            knockback: Vec3::ZERO,
            swimming: false,
            flying: false,
            since_jump_tap: 99.0,
            air_jumps: 0,
            head_under: None,
            blocked: false,
            stride: 0.0,
        },
        Transform::from_translation(spawn),
        RigidBody::KinematicPositionBased,
        Collider::capsule_y(game_rules().player.half_height, game_rules().player.radius),
        KinematicCharacterController {
            offset: CharacterLength::Absolute(0.03),
            up: Vec3::Y,
            max_slope_climb_angle: 50.0_f32.to_radians(),
            min_slope_slide_angle: 40.0_f32.to_radians(),
            autostep: Some(CharacterAutostep {
                max_height: CharacterLength::Absolute(0.6),
                min_width: CharacterLength::Absolute(0.15),
                include_dynamic_bodies: false,
            }),
            snap_to_ground: Some(CharacterLength::Absolute(0.35)),
            ..default()
        },
    ));

    ui.info(format!(
        "Player spawned at ({:.1}, {:.1}, {:.1}). F5 toggles third person, Esc opens the pause menu.",
        spawn.x, spawn.y, spawn.z
    ));
}

fn capture_cursor(
    mut pie: ResMut<PieState>,
    menu: Res<crate::editor::menus::MenuState>,
    chat: Res<crate::gameplay::chat::Chat>,
    panel: Res<crate::gameplay::machines::MachinePanel>,
    death: Res<crate::gameplay::death::DeathState>,
    screens: Res<crate::ui::OpenScreens>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut cursors: Query<&mut CursorOptions, With<PrimaryWindow>>,
    mut was_free: Local<bool>,
) {
    let Ok(mut cursor) = cursors.single_mut() else {
        return;
    };

    if pie.is_playing() {
        let free = menu.wants_cursor() || chat.open || panel.open.is_some() || panel.body_motor.is_some() || death.open || screens.modal;
        if free {
            pie.cursor_captured = false;
        } else if *was_free || (!pie.cursor_captured && mouse.just_pressed(MouseButton::Left)) {
            // Closing the pause menu / inventory recaptures the mouse.
            pie.cursor_captured = true;
        }
        *was_free = free;
    }

    let capture = pie.is_playing() && pie.cursor_captured;
    let mode = if capture {
        CursorGrabMode::Locked
    } else {
        CursorGrabMode::None
    };
    if cursor.grab_mode != mode {
        cursor.grab_mode = mode;
        cursor.visible = !capture;
    }
}

fn read_player_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    settings: Res<GameSettings>,
    pie: Res<PieState>,
    held_tool: Res<crate::gameplay::items::HeldTool>,
    mut intent: ResMut<PlayerIntent>,
) {
    *intent = PlayerIntent::default();
    if !pie.is_playing() || !pie.cursor_captured {
        return;
    }

    let bindings = &settings.bindings;
    let held = |action| bindings.pressed(action, &keyboard, &mouse);
    let mut axis = Vec2::ZERO;
    if held(GameAction::Forward) {
        axis.y += 1.0;
    }
    if held(GameAction::Back) {
        axis.y -= 1.0;
    }
    if held(GameAction::Left) {
        axis.x -= 1.0;
    }
    if held(GameAction::Right) {
        axis.x += 1.0;
    }
    intent.axis = axis.clamp_length_max(1.0);
    intent.jump = held(GameAction::Jump);
    intent.sprint = held(GameAction::Sprint);
    intent.remote = intent.sprint && held_tool.remote;
    if intent.remote {
        intent.sprint = false;
    }
}

#[allow(clippy::too_many_arguments)]
fn move_player(
    time: Res<Time>,
    pie: Res<PieState>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    settings: Res<GameSettings>,
    mouse_motion: Res<AccumulatedMouseMotion>,
    intent: Res<PlayerIntent>,
    world: Res<VoxelWorld>,
    registry: Res<crate::voxel::blocks::BlockRegistry>,
    mut stats: ResMut<PlayerStats>,
    mut players: Query<(&mut Player, &mut KinematicCharacterController, &mut Transform)>,
    mut ui: ResMut<EditorUiState>,
    inventory: Res<crate::gameplay::inventory::Inventory>,
) {
    if !pie.is_playing() {
        return;
    }
    let delta = time.delta_secs().min(0.05);
    if delta <= 0.0 {
        // Paused.
        for (_, mut controller, _) in &mut players {
            controller.translation = Some(Vec3::ZERO);
        }
        return;
    }
    let sensitivity = settings.look_sensitivity();
    let invert = if settings.invert_y { -1.0 } else { 1.0 };

    for (mut player, mut controller, mut transform) in &mut players {
        if pie.cursor_captured {
            player.yaw -= mouse_motion.delta.x * sensitivity;
            player.pitch = (player.pitch - mouse_motion.delta.y * sensitivity * invert).clamp(-1.52, 1.52);
            if settings.bindings.just_pressed(GameAction::ToggleView, &keyboard, &mouse) {
                player.third_person = !player.third_person;
            }
        }
        transform.rotation = Quat::from_rotation_y(player.yaw);

        // Liquids around the body.
        let liquid = |point: Vec3| {
            world.liquid_at(IVec3::new(point.x.floor() as i32, point.y.floor() as i32, point.z.floor() as i32))
        };
        let center = transform.translation;
        let feet = center - Vec3::Y * (game_rules().player.half_height + game_rules().player.radius - 0.15);
        let eye = center + Vec3::Y * game_rules().player.eye_height;
        player.swimming = liquid(center).is_some() || liquid(feet).is_some();
        player.head_under = liquid(eye).map(|block| registry.get(block).color);

        // Death: the body stops; the death screen offers the respawn choices
        // (`gameplay::death`).
        if stats.is_dead() && !inventory.spectator {
            player.velocity = Vec3::ZERO;
            controller.translation = Some(Vec3::ZERO);
            let _ = &mut ui;
            continue;
        }

        // Seated players do not walk: the wagon reads the intent instead.
        if player.seated.is_some() {
            controller.translation = None;
            player.velocity = Vec3::ZERO;
            continue;
        }

        let forward = player.forward();
        let right = Vec3::new(-forward.z, 0.0, forward.x);
        let wish = if intent.remote {
            Vec3::ZERO
        } else {
            (forward * intent.axis.y + right * intent.axis.x).normalize_or_zero()
        };

        // Creative flight (or a flying armour): double-tap Jump toggles it,
        // landing ends it.
        let creative = !inventory.survival || stats.armor_flight;
        player.since_jump_tap += delta;
        let jump_tap = pie.cursor_captured && settings.bindings.just_pressed(GameAction::Jump, &keyboard, &mouse);
        if jump_tap {
            if creative && player.since_jump_tap < 0.3 {
                player.flying = !player.flying;
                player.since_jump_tap = 99.0;
            } else {
                player.since_jump_tap = 0.0;
            }
        }
        if !creative || (player.swimming && !inventory.spectator) {
            player.flying = false;
        }
        if inventory.spectator {
            player.flying = true;
        }
        if player.flying {
            // Space: up, Shift: down, Ctrl: fast.
            let fast = pie.cursor_captured && settings.bindings.pressed(GameAction::Dash, &keyboard, &mouse);
            let speed = if fast { game_rules().movement.fly_fast_speed } else { game_rules().movement.fly_speed };
            let mut target = wish * speed;
            if intent.jump {
                target.y += game_rules().movement.fly_vertical_speed;
            }
            if intent.sprint {
                target.y -= game_rules().movement.fly_vertical_speed;
            }
            let blend = 1.0 - (-10.0 * delta).exp();
            let current = player.velocity;
            player.velocity += (target - current) * blend;
            if player.grounded && intent.sprint && !intent.jump && !inventory.spectator {
                player.flying = false;
            }
            player.ground_speed = 0.0;
            player.knockback = Vec3::ZERO;
            if inventory.spectator {
                // No clipping: move the body directly, through every block.
                transform.translation += player.velocity * delta;
                controller.translation = Some(Vec3::ZERO);
            } else {
                controller.translation = Some(player.velocity * delta);
            }
            continue;
        }

        let sprinting = intent.sprint && wish != Vec3::ZERO && stats.drain(game_rules().movement.sprint_drain, delta);
        let speed = match (player.swimming, sprinting) {
            (true, true) => game_rules().movement.swim_sprint_speed,
            (true, false) => game_rules().movement.swim_speed,
            (false, true) => game_rules().movement.sprint_speed,
            (false, false) => game_rules().movement.walk_speed,
        };

        player.dash_cooldown = (player.dash_cooldown - delta).max(0.0);
        let dash_pressed = pie.cursor_captured
            && settings.bindings.just_pressed(GameAction::Dash, &keyboard, &mouse);
        if dash_pressed && player.dash_cooldown <= 0.0 && stats.try_spend(game_rules().movement.dash_cost) {
            let direction = if wish == Vec3::ZERO { forward } else { wish };
            player.dash_direction = direction;
            player.dash_timer = game_rules().movement.dash_time;
            player.dash_cooldown = game_rules().movement.dash_cooldown;
        }

        let mut horizontal = wish * speed;
        if player.dash_timer > 0.0 {
            player.dash_timer -= delta;
            horizontal = player.dash_direction * game_rules().movement.dash_speed;
        }

        // Knockback: horizontal part decays quickly and adds to the input; the
        // vertical part is a single small hop, consumed on the frame it lands
        // (it used to be re-added every frame and launched the player).
        let knock_flat = Vec3::new(player.knockback.x, 0.0, player.knockback.z).clamp_length_max(10.0);
        horizontal += knock_flat;
        let knock_y = player.knockback.y.clamp(0.0, 5.0);
        player.knockback = knock_flat * (1.0 - delta * 8.0).max(0.0);
        if player.knockback.length_squared() < 0.01 {
            player.knockback = Vec3::ZERO;
        }

        let mut vertical = player.velocity.y;
        if player.swimming {
            // Buoyancy and drag: rise with Jump, sink slowly otherwise.
            let target = if intent.jump && !intent.remote { game_rules().movement.swim_up_speed } else { -game_rules().movement.sink_speed };
            let blend = 1.0 - (-4.0 * delta).exp();
            vertical += (target - vertical) * blend;
            // Hop out of the water onto a ledge (or off the bottom).
            if intent.jump && player.head_under.is_none() && (player.blocked || player.grounded) {
                vertical = game_rules().movement.ledge_hop_speed;
                player.grounded = false;
            }
        } else {
            if player.grounded {
                player.air_jumps = 0;
                vertical = vertical.max(-2.0);
                if intent.jump && !intent.remote {
                    vertical = game_rules().movement.jump_speed;
                    player.grounded = false;
                }
            } else if jump_tap && !intent.remote && player.air_jumps < stats.extra_jumps {
                // Armour extra jump in the air.
                player.air_jumps += 1;
                vertical = game_rules().movement.jump_speed;
            }
            if player.dash_timer <= 0.0 {
                vertical -= game_rules().movement.gravity * delta;
            } else {
                vertical = vertical.max(0.0);
            }
        }
        if knock_y > 0.0 {
            vertical = vertical.max(knock_y);
        }
        vertical = vertical.max(-45.0);

        player.velocity = Vec3::new(horizontal.x, vertical, horizontal.z);
        player.ground_speed = if player.grounded || player.swimming { horizontal.length() } else { 0.0 };
        player.stride += player.ground_speed * delta;
        controller.translation = Some(player.velocity * delta);

        // Fell out of the world.
        if transform.translation.y < -64.0 {
            transform.translation = player.spawn;
            player.velocity = Vec3::ZERO;
        }
    }
}

fn read_controller_output(
    mut players: Query<(&mut Player, Option<&KinematicCharacterControllerOutput>)>,
) {
    for (mut player, output) in &mut players {
        if let Some(output) = output {
            player.grounded = output.grounded;
            let desired = Vec2::new(output.desired_translation.x, output.desired_translation.z);
            let effective = Vec2::new(output.effective_translation.x, output.effective_translation.z);
            player.blocked = desired.length() > 1e-4 && effective.length() < desired.length() * 0.5;
            if output.grounded && player.velocity.y < 0.0 {
                player.velocity.y = 0.0;
            }
            // Bumping a ceiling cancels the upward motion.
            if player.velocity.y > 0.0
                && output.effective_translation.y + 1e-3 < output.desired_translation.y
            {
                player.velocity.y = 0.0;
            }
        }
    }
}

pub fn follow_player_camera(
    pie: Res<PieState>,
    world: Res<VoxelWorld>,
    players: Query<(&Player, &Transform), Without<EditorCamera>>,
    mut cameras: Query<&mut Transform, With<EditorCamera>>,
) {
    if !pie.is_playing() {
        return;
    }
    let Ok((player, body)) = players.single() else {
        return;
    };
    let Ok(mut camera) = cameras.single_mut() else {
        return;
    };

    let eye = body.translation + Vec3::Y * game_rules().player.eye_height;
    let rotation = player.look_rotation();
    if player.third_person {
        let back = rotation * Vec3::new(0.0, 0.0, 1.0);
        let mut distance = 4.5;
        // Pull the camera in front of walls.
        if let Some(hit) = raycast(&world, eye, back, distance) {
            distance = (hit.distance - 0.3).max(0.6);
        }
        camera.translation = eye + back * distance + Vec3::Y * 0.4;
        camera.look_at(eye, Vec3::Y);
    } else {
        camera.translation = eye;
        camera.rotation = rotation;
    }
}
