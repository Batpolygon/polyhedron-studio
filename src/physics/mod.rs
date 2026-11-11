//! Physics boundary.
//!
//! Rapier is registered here so gameplay and voxel colliders share a single
//! physics world:
//! - [`player_controller`]: kinematic capsule for Play-In-Editor;
//! - [`movable_compound`]: flood-fill of connected blocks into dynamic bodies.

pub mod movable_compound;
pub mod player_controller;

use bevy::prelude::*;
use bevy_rapier3d::prelude::*;

use crate::physics::movable_compound::MovablePlugin;
use crate::physics::player_controller::PlayerControllerPlugin;

pub struct PhysicsPlugin;

impl Plugin for PhysicsPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(RapierPhysicsPlugin::<NoUserData>::default())
            .add_plugins((PlayerControllerPlugin, MovablePlugin));
    }
}
