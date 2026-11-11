//! Motors: a motor block that is not part of a movable is a rotary bearing.
//!
//! Switched on from its panel (`E`), it lifts every block attached to its
//! output face (the top of the block, following its orientation) into a
//! kinematic rotor and spins it around the face normal at the chosen speed
//! and direction. Players and mobs collide with the spinning blocks. Stopped,
//! the blocks go back into the world aligned on the grid. The world snapshot
//! restores the blocks when play stops.

use crate::core::game_rules::game_rules;
use std::collections::HashMap;
use std::f32::consts::TAU;

use bevy::prelude::*;

use crate::core::app_state::EditorUiState;
use crate::core::pie_manager::PieState;
use crate::physics::movable_compound::{assemble_rotor, attached_blocks};
use crate::renderer::poly_material::PolyVoxelMaterial;
use crate::renderer::voxel_render::VoxelRenderState;
use crate::voxel::blocks::{BlockFunction, BlockRegistry};
use crate::voxel::world::VoxelWorld;


#[derive(Clone, Debug)]
pub struct MotorConfig {
    pub enabled: bool,
    /// Revolutions per minute.
    pub rpm: f32,
    pub clockwise: bool,
    /// Current rotor angle, radians.
    pub angle: f32,
    /// The attached blocks were lifted (or there were none).
    pub assembled: bool,
    pub rotor: Option<Entity>,
    pub attached: usize,
    /// Last status line shown in the panel.
    pub status: String,
    /// Rotation axis and pivot (world space), set when assembled.
    pub axis: Vec3,
    pub pivot: Vec3,
}

impl Default for MotorConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            rpm: 20.0,
            clockwise: true,
            angle: 0.0,
            assembled: false,
            rotor: None,
            attached: 0,
            status: "Off".to_string(),
            axis: Vec3::Y,
            pivot: Vec3::ZERO,
        }
    }
}

impl MotorConfig {
    /// Signed angular speed in radians per second.
    pub fn angular_speed(&self) -> f32 {
        let sign = if self.clockwise { -1.0 } else { 1.0 };
        sign * self.rpm.clamp(0.0, game_rules().vehicles.max_rpm) / 60.0 * TAU
    }
}

#[derive(Resource, Debug, Default)]
pub struct Motors {
    pub motors: HashMap<IVec3, MotorConfig>,
}

impl Motors {
    pub fn config_mut(&mut self, cell: IVec3) -> &mut MotorConfig {
        self.motors.entry(cell).or_default()
    }
}

#[allow(clippy::too_many_arguments)]
pub fn run_motors(
    time: Res<Time>,
    pie: Res<PieState>,
    mut motors: ResMut<Motors>,
    mut commands: Commands,
    mut world: ResMut<VoxelWorld>,
    registry: Res<BlockRegistry>,
    mut render: ResMut<VoxelRenderState>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<PolyVoxelMaterial>>,
    ui: Res<EditorUiState>,
    mut rotors: Query<&mut Transform>,
    rotor_blocks: Query<&crate::physics::movable_compound::RotorBody>,
) {
    if pie.started_this_frame || pie.stopped_this_frame {
        motors.motors.clear();
    }
    if !pie.is_playing() {
        return;
    }
    let delta = time.delta_secs();
    let shading = ui.shading;
    for (cell, config) in motors.motors.iter_mut() {
        if !config.enabled {
            continue;
        }
        if !config.assembled {
            let Some(voxel) = world.get(*cell) else {
                config.enabled = false;
                config.status = "The motor is gone".to_string();
                continue;
            };
            if registry.get(voxel.block).function != BlockFunction::Motor {
                config.enabled = false;
                continue;
            }
            // Part of a movable: it will push the body once launched.
            let mounted = crate::physics::movable_compound::flood_fill(&world, &registry, *cell)
                .iter()
                .any(|part| world.get(*part).is_some_and(|v| registry.get(v.block).function == BlockFunction::Movable));
            if mounted {
                config.status = "Mounted on a movable: launch it (E on a frame) and this motor pushes it".to_string();
                continue;
            }
            let axis = voxel.orientation.apply_direction(Vec3::Y).round().normalize_or(Vec3::Y);
            let output = *cell + axis.as_ivec3();
            config.axis = axis;
            config.pivot = cell.as_vec3() + Vec3::splat(0.5) + axis * 0.5;
            match attached_blocks(&world, &registry, output, *cell, game_rules().vehicles.max_rotor_blocks) {
                None => {
                    config.enabled = false;
                    config.status = format!("Too many attached blocks (over {}): is it touching the ground?", game_rules().vehicles.max_rotor_blocks);
                    continue;
                }
                Some(cells) => {
                    config.attached = cells.len();
                    config.rotor = assemble_rotor(
                        &mut commands,
                        &mut world,
                        &registry,
                        &cells,
                        config.pivot,
                        *cell,
                        &mut render,
                        &mut meshes,
                        &mut materials,
                        &shading,
                    );
                    config.assembled = true;
                }
            }
        }
        config.angle = (config.angle + config.angular_speed() * delta).rem_euclid(TAU);
        config.status = if config.attached == 0 {
            "Spinning (nothing attached on the top face)".to_string()
        } else {
            format!("Spinning {} block(s)", config.attached)
        };
        if let Some(rotor) = config.rotor {
            if let Ok(mut transform) = rotors.get_mut(rotor) {
                transform.translation = config.pivot;
                transform.rotation = Quat::from_axis_angle(config.axis, config.angle);
            }
        }
    }
    // Stopped motors put their blocks back in the world, aligned on the grid
    // (the angle snaps to the nearest quarter turn), like a settled movable.
    for config in motors.motors.values_mut() {
        if config.enabled || !config.assembled {
            continue;
        }
        if let Some(rotor) = config.rotor.take() {
            if let Ok(body) = rotor_blocks.get(rotor) {
                let (_, blocked) = crate::physics::movable_compound::settle_rotor(&mut world, &registry, body, config.pivot, config.axis, config.angle);
                commands.entity(rotor).despawn();
                config.status = if blocked > 0 { format!("Stopped ({blocked} block(s) blocked by the terrain)") } else { "Stopped".to_string() };
            }
        } else if !config.status.starts_with("Too many") && !config.status.starts_with("Mounted") {
            config.status = "Stopped".to_string();
        }
        // Started again, it lifts the blocks from where they now are.
        config.assembled = false;
        config.angle = 0.0;
        config.attached = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direction_flips_the_angular_speed() {
        let mut config = MotorConfig { rpm: 60.0, ..Default::default() };
        let cw = config.angular_speed();
        config.clockwise = false;
        assert!((cw + config.angular_speed()).abs() < 1e-5);
        assert!((config.angular_speed() - TAU).abs() < 1e-4);
    }
}
