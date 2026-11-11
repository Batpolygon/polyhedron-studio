//! Viewport voxel picking and brush input.
//!
//! The picker walks the voxel grid with the Amanatides & Woo DDA algorithm. It
//! returns the hit cell, the face normal and the neighbouring placement cell, so
//! the brush never needs to know about the render meshes.

use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy_egui::input::EguiWantsInput;

use crate::core::app_state::{EditorMode, EditorState, EditorUiState};
use crate::core::editor_camera::EditorCamera;
use crate::voxel::blocks::{BlockId, BlockRegistry};
use crate::voxel::shapes::{shapes_overlap, Facing, Orientation};
use crate::voxel::world::{SubVoxel, Voxel, VoxelWorld};

/// Maximum picking distance, in metres.
pub const MAX_REACH: f32 = 96.0;

/// Result of a successful viewport raycast.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VoxelHit {
    pub voxel: IVec3,
    pub normal: IVec3,
    pub distance: f32,
    pub block: BlockId,
}

impl VoxelHit {
    /// Cell that would receive a new voxel on the face the ray hit.
    pub fn placement(&self) -> IVec3 {
        self.voxel + self.normal
    }
}

/// Hover state published by the picker and consumed by the viewport overlays.
#[derive(Resource, Debug, Default)]
pub struct VoxelCursor {
    pub target: Option<VoxelHit>,
    pub placement: Option<IVec3>,
}

/// Throttle for click-and-drag painting.
#[derive(Resource, Debug)]
pub struct VoxelBrushTimer {
    pub cooldown: f32,
}

impl Default for VoxelBrushTimer {
    fn default() -> Self {
        Self { cooldown: 0.0 }
    }
}

/// Amanatides & Woo voxel traversal. Liquids are transparent to the ray
/// (gameplay: you mine through water, place into it, see through it).
pub fn raycast(
    world: &VoxelWorld,
    origin: Vec3,
    direction: Vec3,
    max_distance: f32,
) -> Option<VoxelHit> {
    raycast_filtered(world, origin, direction, max_distance, false)
}

/// Same traversal, optionally stopping on liquid cells (editor brush, which
/// must be able to select and erase water).
pub fn raycast_filtered(
    world: &VoxelWorld,
    origin: Vec3,
    direction: Vec3,
    max_distance: f32,
    hit_liquids: bool,
) -> Option<VoxelHit> {
    let direction = direction.normalize_or_zero();
    if direction == Vec3::ZERO {
        return None;
    }

    let mut cell = IVec3::new(
        origin.x.floor() as i32,
        origin.y.floor() as i32,
        origin.z.floor() as i32,
    );

    let mut step = [0i32; 3];
    let mut t_max = [f32::INFINITY; 3];
    let mut t_delta = [f32::INFINITY; 3];

    for axis in 0..3 {
        let component = direction[axis];
        if component > 1e-6 {
            step[axis] = 1;
            t_max[axis] = ((cell[axis] + 1) as f32 - origin[axis]) / component;
            t_delta[axis] = 1.0 / component;
        } else if component < -1e-6 {
            step[axis] = -1;
            t_max[axis] = (cell[axis] as f32 - origin[axis]) / component;
            t_delta[axis] = -1.0 / component;
        }
    }

    let mut normal = IVec3::ZERO;
    let mut distance = 0.0;

    for _ in 0..4096 {
        if let Some(voxel) = world
            .get(cell)
            .filter(|voxel| hit_liquids || voxel.sub.is_some() || !world.is_liquid_block(voxel.block))
        {
            return Some(VoxelHit {
                voxel: cell,
                normal,
                distance,
                block: voxel.block,
            });
        }

        let axis = if t_max[0] <= t_max[1] && t_max[0] <= t_max[2] {
            0
        } else if t_max[1] <= t_max[2] {
            1
        } else {
            2
        };

        distance = t_max[axis];
        if !distance.is_finite() || distance > max_distance {
            return None;
        }

        cell = IVec3::new(
            cell.x + if axis == 0 { step[0] } else { 0 },
            cell.y + if axis == 1 { step[1] } else { 0 },
            cell.z + if axis == 2 { step[2] } else { 0 },
        );

        normal = match axis {
            0 => IVec3::new(-step[0], 0, 0),
            1 => IVec3::new(0, -step[1], 0),
            _ => IVec3::new(0, 0, -step[2]),
        };

        t_max[axis] += t_delta[axis];
    }

    None
}

/// Project the window cursor through the editor camera and publish the hit.
pub fn update_voxel_cursor(
    world: Res<VoxelWorld>,
    ui: Res<EditorUiState>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform), With<EditorCamera>>,
    mut cursor: ResMut<VoxelCursor>,
) {
    cursor.target = None;
    cursor.placement = None;

    if !ui.viewport_hovered {
        return;
    }

    let Ok(window) = windows.single() else {
        return;
    };
    let Some(position) = window.cursor_position() else {
        return;
    };
    let Ok((camera, transform)) = cameras.single() else {
        return;
    };
    let Ok(ray) = camera.viewport_to_world(transform, position) else {
        return;
    };

    if let Some(hit) = raycast_filtered(&world, ray.origin, *ray.direction, MAX_REACH, true) {
        cursor.placement = Some(hit.placement());
        cursor.target = Some(hit);
    }
}

/// Place the selected block with LMB, erase with Shift+LMB or `X`.
pub fn apply_voxel_brush(
    time: Res<Time>,
    mut world: ResMut<VoxelWorld>,
    registry: Res<BlockRegistry>,
    cursor: Res<VoxelCursor>,
    mouse: Res<ButtonInput<MouseButton>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    editor: Res<EditorState>,
    mut ui: ResMut<EditorUiState>,
    egui_input: Res<EguiWantsInput>,
    mut brush: ResMut<VoxelBrushTimer>,
    cameras: Query<&GlobalTransform, With<EditorCamera>>,
    mut room: ResMut<crate::voxel::structures::StructureRoom>,
) {
    brush.cooldown = (brush.cooldown - time.delta_secs()).max(0.0);

    if editor.mode == EditorMode::Playing
        || !editor.workspace.shows_world()
        || !ui.viewport_hovered
        || ui.cable_tool
        || egui_input.wants_keyboard_input()
    {
        return;
    }

    let Some(hit) = cursor.target else {
        return;
    };

    // Structure room: anchor mode toggles anchors instead of painting.
    if room.anchor_mode && room.contains(hit.voxel) {
        if mouse.just_pressed(MouseButton::Left) {
            room.toggle_anchor(hit.voxel);
            ui.action(format!("Anchor toggled ({} anchor(s)).", room.anchors.len()));
        }
        return;
    }

    let shift =
        keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight);
    let erase_key = keyboard.just_pressed(KeyCode::KeyX);
    let left = mouse.pressed(MouseButton::Left);
    let first_press = mouse.just_pressed(MouseButton::Left);
    let ready = brush.cooldown <= 0.0;
    let should_log = first_press || erase_key;

    if erase_key || (left && shift && ready) {
        if world.set(hit.voxel, None) {
            if should_log {
                ui.action(format!(
                    "Erased voxel at ({}, {}, {}).",
                    hit.voxel.x, hit.voxel.y, hit.voxel.z
                ));
            }
            brush.cooldown = 0.12;
        }
        return;
    }

    // Orientation shortcuts while authoring.
    if keyboard.just_pressed(KeyCode::KeyR) {
        ui.manual_orientation = ui.manual_orientation.next_roll();
    }
    if keyboard.just_pressed(KeyCode::KeyT) {
        ui.manual_orientation = ui.manual_orientation.next_facing();
    }

    if left && !shift && ready {
        let block = editor.selected_block;
        let shape = registry.get(block).shape;
        let forward = cameras
            .single()
            .map(|transform| transform.forward().as_vec3())
            .unwrap_or(Vec3::NEG_Z);
        let orientation = placement_orientation(&ui, registry.get(block).placement, hit.normal, forward);

        // Sub-voxel fusion: drop a half cube into the hit cell when it holds a
        // single complementary half.
        if ui.sub_voxel_fusion && shape.is_half() {
            if let Some(fused) = try_fuse(&world, &registry, hit.voxel, block, orientation) {
                if world.set(hit.voxel, Some(fused)) {
                    if should_log {
                        ui.action(format!(
                            "Fused {} into ({}, {}, {}).",
                            registry.get(block).label,
                            hit.voxel.x,
                            hit.voxel.y,
                            hit.voxel.z
                        ));
                    }
                    brush.cooldown = 0.12;
                    return;
                }
            }
        }

        let target = hit.placement();
        let voxel = Voxel::oriented(block, orientation);

        if world.set(target, Some(voxel)) {
            world.mark_placed(target);
            if should_log {
                ui.action(format!(
                    "Placed {} at ({}, {}, {}).",
                    registry.get(block).label,
                    target.x,
                    target.y,
                    target.z
                ));
            }
            brush.cooldown = 0.12;
        } else if first_press {
            ui.warn("Placement blocked: target cell already contains a voxel.");
        }
    }
}

/// Orientation of a block placed against the face `normal` while looking along
/// `forward`: its front faces the viewer; its up axis follows the block's
/// placement rule (upright, or along the clicked face).
pub fn oriented_toward_viewer(placement: crate::voxel::blocks::BlockPlacement, normal: IVec3, forward: Vec3) -> Orientation {
    let up = match placement {
        crate::voxel::blocks::BlockPlacement::AlignToSurface => Facing::from_vector(normal.as_vec3()),
        crate::voxel::blocks::BlockPlacement::FacePlayer => {
            if normal.y < 0 {
                Facing::NegY
            } else {
                Facing::PosY
            }
        }
    };
    // Remove the component along `up` so the back side follows the view.
    let up_vec = up.vector().as_vec3();
    let mut back = forward - up_vec * forward.dot(up_vec);
    if back.length_squared() < 1e-4 {
        back = Vec3::Z;
    }
    Orientation::facing_toward(up, back)
}

/// Orientation for a new voxel in the editor: the block's placement rule,
/// then the manual roll offset (or the manual orientation when auto-orient
/// is off).
pub fn placement_orientation(
    ui: &EditorUiState,
    placement: crate::voxel::blocks::BlockPlacement,
    normal: IVec3,
    forward: Vec3,
) -> Orientation {
    if !ui.auto_orient {
        return ui.manual_orientation;
    }
    let base = oriented_toward_viewer(placement, normal, forward);
    let mut result = base;
    for _ in 0..ui.manual_orientation.roll() {
        result = result.next_roll();
    }
    result
}

/// Fuse a half cube into a cell that holds one complementary half.
pub fn try_fuse(
    world: &VoxelWorld,
    registry: &BlockRegistry,
    cell: IVec3,
    block: BlockId,
    orientation: Orientation,
) -> Option<Voxel> {
    let existing = world.get(cell)?;
    if existing.sub.is_some() {
        return None;
    }
    let existing_shape = registry.get(existing.block).shape;
    let shape = registry.get(block).shape;
    if !existing_shape.is_half() || !shape.is_half() {
        return None;
    }

    let existing_part = (existing_shape, existing.orientation);
    let flipped = Orientation::new(
        existing.orientation.facing().opposite(),
        existing.orientation.roll(),
    );
    let mut candidates = vec![orientation, flipped];
    candidates.extend((0..Orientation::COUNT).map(|index| Orientation(index as u8)));

    candidates
        .into_iter()
        .find(|candidate| !shapes_overlap(existing_part, (shape, *candidate)))
        .map(|candidate| {
            existing.with_sub(SubVoxel {
                block,
                orientation: candidate,
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dda_reports_hit_normal_and_placement_cell() {
        let mut world = VoxelWorld::new();
        world.set(IVec3::new(0, 0, 0), Some(Voxel::new(BlockId(0))));

        let hit = raycast(&world, Vec3::new(0.5, 0.5, 5.0), Vec3::NEG_Z, 20.0)
            .expect("ray should hit the voxel");
        assert_eq!(hit.voxel, IVec3::ZERO);
        assert_eq!(hit.normal, IVec3::Z);
        assert_eq!(hit.placement(), IVec3::new(0, 0, 1));
        assert_eq!(hit.block, BlockId(0));
        assert!((hit.distance - 4.0).abs() < 1e-4);
    }

    #[test]
    fn dda_misses_when_the_ray_points_away() {
        let mut world = VoxelWorld::new();
        world.set(IVec3::new(0, 0, 0), Some(Voxel::new(BlockId(0))));

        assert!(raycast(&world, Vec3::new(0.5, 0.5, 5.0), Vec3::Z, 20.0).is_none());
    }
}
