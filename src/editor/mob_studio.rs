//! 🐾 Sculpteur: the mob modelling / animation studio of the "Mobs" workspace.
//!
//! - The voxel world is hidden and the camera orbits the mob (right drag:
//!   orbit, middle drag or Shift + right drag: pan, wheel: zoom, `F`: frame).
//! - Every rig box is rendered as a real lit cube, in its rest pose (Model
//!   mode) or at the timeline playhead (Animation mode).
//! - Clicking a cube selects it. The selected cube shows a 3D gizmo:
//!   `G` move, `R` rotate, `S` size / scale, `P` pivot. Dragging a handle edits
//!   the box (Model mode) or writes a keyframe at the playhead (Animation).
//! - Snapping (1/16 block, 15°) is on by default; hold `Ctrl` to disable it.

use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::core::app_state::{EditorState, EditorUiState, Workspace};
use crate::core::editor_camera::EditorCamera;
use crate::core::pie_manager::PieState;
use crate::editor::studio_common::{effective_workspace, orbit_input, Orbit, StudioSet};
use crate::gameplay::mobs::library::MobLibrary;
use crate::gameplay::mobs::model::{Interpolation, Keyframe, MobRig};
use crate::gameplay::mobs::pose::{drawn_bones, sample_clip, world_matrices, BonePose, RigPose};

/// What the gizmo edits.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GizmoMode {
    #[default]
    Move,
    Rotate,
    Scale,
    Pivot,
}

impl GizmoMode {
    pub const ALL: [Self; 4] = [Self::Move, Self::Rotate, Self::Scale, Self::Pivot];

    pub const fn icon(self) -> &'static str {
        match self {
            Self::Move => "✛",
            Self::Rotate => "⟳",
            Self::Scale => "↗",
            Self::Pivot => "📍",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Move => "Move (G)",
            Self::Rotate => "Rotate (R)",
            Self::Scale => "Size (S)",
            Self::Pivot => "Pivot point (P)",
        }
    }
}

/// Model = edit the boxes; Animation = keyframe the selected clip.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MobEditMode {
    #[default]
    Model,
    Animate,
}

#[derive(Clone, Debug)]
struct GizmoDrag {
    axis: usize,
    bone: String,
    /// Parameter along the axis (or angle) at the start of the drag.
    start: f32,
    /// Amount already applied (after snapping).
    applied: f32,
}

#[derive(Resource, Debug)]
pub struct MobStudio {
    pub gizmo: GizmoMode,
    pub edit: MobEditMode,
    pub orbit: Orbit,
    pub snap: bool,
    pub show_grid: bool,
    pub frame_request: bool,
    pub hovered_axis: Option<usize>,
    drag: Option<GizmoDrag>,
    last_workspace: Workspace,
    preview_signature: String,
}

impl Default for MobStudio {
    fn default() -> Self {
        Self {
            gizmo: GizmoMode::Move,
            edit: MobEditMode::Model,
            orbit: Orbit::default(),
            snap: true,
            show_grid: true,
            frame_request: true,
            hovered_axis: None,
            drag: None,
            last_workspace: Workspace::World,
            preview_signature: String::new(),
        }
    }
}

impl MobStudio {
    pub fn is_dragging(&self) -> bool {
        self.drag.is_some()
    }
}

/// Root of the preview cubes.
#[derive(Component)]
struct MobPreviewRoot;

/// One preview cube.
#[derive(Component)]
struct MobPreviewBox {
    bone: String,
}

pub struct MobStudioPlugin;

impl Plugin for MobStudioPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MobStudio>().add_systems(
            Update,
            (
                switch_workspace,
                studio_shortcuts,
                gizmo_interaction,
                orbit_camera,
                sync_preview,
                draw_studio,
            )
                .chain()
                .in_set(StudioSet),
        );
    }
}

fn in_studio(editor: &EditorState, pie: &PieState) -> bool {
    editor.workspace == Workspace::Mobs && !pie.is_playing()
}

/// Pose shown in the viewport for the current mode.
pub fn studio_pose(library: &MobLibrary, mode: MobEditMode) -> Option<RigPose> {
    let rig = library.current()?;
    Some(match (mode, library.current_clip()) {
        (MobEditMode::Animate, Some(clip)) => sample_clip(rig, clip, library.preview.time),
        _ => RigPose::default(),
    })
}

fn rig_bounds(rig: &MobRig) -> (Vec3, f32) {
    let bones = drawn_bones(rig, &RigPose::default());
    if bones.is_empty() {
        return (Vec3::ZERO, 2.0);
    }
    let mut min = Vec3::splat(f32::MAX);
    let mut max = Vec3::splat(f32::MIN);
    for bone in &bones {
        min = min.min(bone.center - bone.size * 0.5);
        max = max.max(bone.center + bone.size * 0.5);
    }
    ((min + max) * 0.5, (max - min).length().max(0.5))
}

fn switch_workspace(
    editor: Res<EditorState>,
    pie: Res<PieState>,
    library: Res<MobLibrary>,
    mut studio: ResMut<MobStudio>,
) {
    let workspace = effective_workspace(&editor, &pie);
    let entering = workspace == Workspace::Mobs && studio.last_workspace != Workspace::Mobs;
    let leaving = workspace != Workspace::Mobs && studio.last_workspace == Workspace::Mobs;
    studio.last_workspace = workspace;

    if entering {
        studio.frame_request = true;
    }
    if leaving {
        studio.drag = None;
    }

    if studio.frame_request && workspace == Workspace::Mobs {
        studio.frame_request = false;
        if let Some(rig) = library.current() {
            let (center, size) = rig_bounds(rig);
            studio.orbit.target = Vec3::from_array(library.preview_origin) + center;
            studio.orbit.distance = (size * 1.9).clamp(1.5, 40.0);
        }
    }
}

fn studio_shortcuts(
    editor: Res<EditorState>,
    pie: Res<PieState>,
    ui: Res<EditorUiState>,
    keyboard: Res<ButtonInput<KeyCode>>,
    egui_input: Res<bevy_egui::input::EguiWantsInput>,
    mut studio: ResMut<MobStudio>,
) {
    if !in_studio(&editor, &pie) || !ui.viewport_hovered || egui_input.wants_keyboard_input() {
        return;
    }
    if keyboard.just_pressed(KeyCode::KeyG) {
        studio.gizmo = GizmoMode::Move;
    }
    if keyboard.just_pressed(KeyCode::KeyR) {
        studio.gizmo = GizmoMode::Rotate;
    }
    if keyboard.just_pressed(KeyCode::KeyS) {
        studio.gizmo = GizmoMode::Scale;
    }
    if keyboard.just_pressed(KeyCode::KeyP) {
        studio.gizmo = GizmoMode::Pivot;
    }
    if keyboard.just_pressed(KeyCode::KeyF) {
        studio.frame_request = true;
    }
    if keyboard.just_pressed(KeyCode::Tab) {
        studio.edit = match studio.edit {
            MobEditMode::Model => MobEditMode::Animate,
            MobEditMode::Animate => MobEditMode::Model,
        };
    }
}

#[allow(clippy::too_many_arguments)]
fn orbit_camera(
    editor: Res<EditorState>,
    pie: Res<PieState>,
    ui: Res<EditorUiState>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    mut studio: ResMut<MobStudio>,
    mut cameras: Query<&mut Transform, With<EditorCamera>>,
) {
    if !in_studio(&editor, &pie) {
        return;
    }
    if ui.viewport_hovered && !studio.is_dragging() {
        orbit_input(&mut studio.orbit, &keyboard, &mouse, &motion, &scroll, 80.0);
    }
    if let Ok(mut transform) = cameras.single_mut() {
        *transform = studio.orbit.transform();
    }
}

#[allow(clippy::too_many_arguments)]
fn sync_preview(
    mut commands: Commands,
    editor: Res<EditorState>,
    pie: Res<PieState>,
    library: Res<MobLibrary>,
    mut studio: ResMut<MobStudio>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    roots: Query<Entity, With<MobPreviewRoot>>,
    mut boxes: Query<(&MobPreviewBox, &mut Transform, &mut Visibility, &MeshMaterial3d<StandardMaterial>)>,
) {
    let active = in_studio(&editor, &pie);
    let rig = library.current();
    let signature = match (active, rig) {
        (true, Some(rig)) => format!(
            "{}|{}",
            rig.name,
            rig.boxes.iter().map(|b| b.name.as_str()).collect::<Vec<_>>().join(",")
        ),
        _ => String::new(),
    };

    // Rebuild when the box list changes (or leave the studio).
    if signature != studio.preview_signature {
        studio.preview_signature = signature.clone();
        for root in &roots {
            commands.entity(root).despawn();
        }
        if let (false, Some(rig)) = (signature.is_empty(), rig) {
            let cube = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
            let root = commands
                .spawn((
                    Name::new("Mob preview"),
                    MobPreviewRoot,
                    Transform::from_translation(Vec3::from_array(library.preview_origin)),
                    Visibility::default(),
                ))
                .id();
            for bone in &rig.boxes {
                let material = materials.add(StandardMaterial {
                    base_color: Color::srgb(bone.color[0], bone.color[1], bone.color[2]),
                    perceptual_roughness: 0.75,
                    ..default()
                });
                // The glow follows the draft below.
                commands.spawn((
                    Mesh3d(cube.clone()),
                    MeshMaterial3d(material),
                    Transform::default(),
                    Visibility::default(),
                    ChildOf(root),
                    MobPreviewBox {
                        bone: bone.name.clone(),
                    },
                ));
            }
        }
        return;
    }
    let (Some(rig), true) = (rig, active) else {
        return;
    };
    let Some(pose) = studio_pose(&library, studio.edit) else {
        return;
    };
    let drawn = drawn_bones(rig, &pose);
    for (preview, mut transform, mut visibility, material) in &mut boxes {
        match drawn.iter().find(|d| d.name == preview.bone) {
            Some(drawn) => {
                transform.translation = drawn.center;
                transform.rotation = drawn.rotation;
                transform.scale = drawn.size;
                *visibility = Visibility::Inherited;
                if let Some(mut material) = materials.get_mut(&material.0) {
                    let color = Color::srgb(drawn.color[0], drawn.color[1], drawn.color[2]);
                    if material.base_color != color {
                        material.base_color = color;
                    }
                    let glow = rig.bone(&preview.bone).map_or(0.0, |bone| bone.emissive);
                    let emissive = color.to_linear() * glow;
                    if material.emissive != emissive {
                        material.emissive = emissive;
                    }
                }
            }
            None => *visibility = Visibility::Hidden,
        }
    }
}

// --- Gizmo math -----------------------------------------------------------------

/// Frame of the gizmo for the selected bone.
struct GizmoFrame {
    origin: Vec3,
    axes: [Vec3; 3],
    size: f32,
}

fn linear(matrix: Mat4) -> Mat3 {
    Mat3::from_mat4(matrix)
}

fn rest_matrix(rig: &MobRig, bone: &str) -> Mat4 {
    rig.bone(bone)
        .map(|b| {
            Mat4::from_translation(Vec3::from_array(b.offset))
                * Mat4::from_euler(
                    EulerRot::XYZ,
                    b.rotation[0].to_radians(),
                    b.rotation[1].to_radians(),
                    b.rotation[2].to_radians(),
                )
        })
        .unwrap_or(Mat4::IDENTITY)
}

/// Parent frame (rig scale included) of a bone.
fn parent_frame(rig: &MobRig, pose: &RigPose, bone: &str) -> Mat4 {
    let matrices = world_matrices(rig, pose);
    let scale = Mat4::from_scale(Vec3::from_array(rig.scale).max(Vec3::splat(1e-4)));
    let parent = rig
        .bone(bone)
        .and_then(|b| b.parent.as_deref())
        .and_then(|p| matrices.get(p).copied())
        .unwrap_or(Mat4::IDENTITY);
    scale * parent
}

fn gizmo_frame(library: &MobLibrary, studio: &MobStudio, camera: Vec3) -> Option<(GizmoFrame, String)> {
    let rig = library.current()?;
    let bone = library.bone_selection.clone()?;
    rig.bone(&bone)?;
    let pose = studio_pose(library, studio.edit)?;
    let matrices = world_matrices(rig, &pose);
    let scale = Mat4::from_scale(Vec3::from_array(rig.scale).max(Vec3::splat(1e-4)));
    let frame = scale * *matrices.get(&bone)?;
    let origin_local = frame.transform_point3(Vec3::ZERO);
    let anchor = Vec3::from_array(library.preview_origin);

    // Pivot mode shows the pivot point; other modes too, at the bone pivot.
    let axes = match studio.gizmo {
        GizmoMode::Move | GizmoMode::Pivot => [Vec3::X, Vec3::Y, Vec3::Z],
        GizmoMode::Rotate => {
            let parent = linear(parent_frame(rig, &pose, &bone));
            [
                (parent * Vec3::X).normalize_or(Vec3::X),
                (parent * Vec3::Y).normalize_or(Vec3::Y),
                (parent * Vec3::Z).normalize_or(Vec3::Z),
            ]
        }
        GizmoMode::Scale => {
            let l = linear(frame);
            [
                (l * Vec3::X).normalize_or(Vec3::X),
                (l * Vec3::Y).normalize_or(Vec3::Y),
                (l * Vec3::Z).normalize_or(Vec3::Z),
            ]
        }
    };
    let origin = anchor + origin_local;
    let size = (camera.distance(origin) * 0.16).clamp(0.15, 6.0);
    Some((GizmoFrame { origin, axes, size }, bone))
}

/// Closest parameter on the axis line to the mouse ray.
pub(crate) fn axis_param(origin: Vec3, axis: Vec3, ray_origin: Vec3, ray_dir: Vec3) -> Option<f32> {
    let w0 = origin - ray_origin;
    let b = axis.dot(ray_dir);
    let d = axis.dot(w0);
    let e = ray_dir.dot(w0);
    let denom = 1.0 - b * b;
    if denom.abs() < 1e-5 {
        return None;
    }
    Some((b * e - d) / denom)
}

/// Angle of the mouse ray around a ring axis.
pub(crate) fn ring_angle(origin: Vec3, axis: Vec3, ray_origin: Vec3, ray_dir: Vec3) -> Option<f32> {
    let denom = axis.dot(ray_dir);
    if denom.abs() < 1e-4 {
        return None;
    }
    let t = axis.dot(origin - ray_origin) / denom;
    let hit = ray_origin + ray_dir * t;
    let reference = if axis.dot(Vec3::Y).abs() > 0.9 { Vec3::X } else { Vec3::Y };
    let u = axis.cross(reference).normalize();
    let v = axis.cross(u);
    let d = hit - origin;
    Some(d.dot(v).atan2(d.dot(u)))
}

pub(crate) fn ring_point(origin: Vec3, axis: Vec3, radius: f32, angle: f32) -> Vec3 {
    let reference = if axis.dot(Vec3::Y).abs() > 0.9 { Vec3::X } else { Vec3::Y };
    let u = axis.cross(reference).normalize();
    let v = axis.cross(u);
    origin + (u * angle.cos() + v * angle.sin()) * radius
}

pub(crate) fn segment_distance(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_squared().max(1e-6)).clamp(0.0, 1.0);
    p.distance(a + ab * t)
}

/// Ray / oriented box intersection distance.
pub(crate) fn ray_obb(origin: Vec3, dir: Vec3, center: Vec3, rotation: Quat, size: Vec3) -> Option<f32> {
    let inv = rotation.inverse();
    let o = inv * (origin - center);
    let d = inv * dir;
    let half = size * 0.5;
    let mut t_min = f32::MIN;
    let mut t_max = f32::MAX;
    for i in 0..3 {
        if d[i].abs() < 1e-6 {
            if o[i].abs() > half[i] {
                return None;
            }
        } else {
            let t1 = (-half[i] - o[i]) / d[i];
            let t2 = (half[i] - o[i]) / d[i];
            t_min = t_min.max(t1.min(t2));
            t_max = t_max.min(t1.max(t2));
        }
    }
    (t_max >= t_min.max(0.0)).then_some(t_min.max(0.0))
}

/// Ensure a keyframe exists at the playhead and return its pose fields.
fn keyframe_at<'a>(library: &'a mut MobLibrary, bone: &str) -> Option<&'a mut Keyframe> {
    let time = library.preview.time;
    let sampled: BonePose = {
        let rig = library.current()?;
        let clip = library.current_clip()?;
        sample_clip(rig, clip, time).get(bone)
    };
    let clip = library.current_clip_mut()?;
    let track = clip.ensure_track(bone);
    if track.key_at(time).is_none() {
        track.upsert(Keyframe {
            time,
            translation: sampled.translation.to_array(),
            rotation: sampled.rotation.to_array(),
            scale: sampled.scale.to_array(),
            interpolation: Interpolation::Auto,
        });
    }
    track.key_at_mut(time)
}

/// Apply a gizmo delta (world units or radians) to the library.
fn apply_delta(library: &mut MobLibrary, studio: &MobStudio, bone: &str, axis: usize, amount: f32, axis_dir: Vec3) {
    let Some(rig) = library.current() else {
        return;
    };
    let pose = studio_pose(library, studio.edit).unwrap_or_default();
    let parent = parent_frame(rig, &pose, bone);
    let rest = rest_matrix(rig, bone);
    let rig_scale = Vec3::from_array(rig.scale).max(Vec3::splat(1e-4));
    let world_delta = axis_dir * amount;

    match (studio.edit, studio.gizmo) {
        (MobEditMode::Model, GizmoMode::Move) => {
            let local = linear(parent).inverse() * world_delta;
            if let Some(b) = library.current_mut().and_then(|r| r.bone_mut(bone)) {
                for i in 0..3 {
                    b.offset[i] += local[i];
                }
            }
        }
        (MobEditMode::Model, GizmoMode::Rotate) => {
            if let Some(b) = library.current_mut().and_then(|r| r.bone_mut(bone)) {
                b.rotation[axis] = (b.rotation[axis] + amount.to_degrees() + 180.0).rem_euclid(360.0) - 180.0;
            }
        }
        (MobEditMode::Model, GizmoMode::Scale) => {
            if let Some(b) = library.current_mut().and_then(|r| r.bone_mut(bone)) {
                b.size[axis] = (b.size[axis] + amount / rig_scale[axis]).max(0.02);
            }
        }
        (MobEditMode::Model, GizmoMode::Pivot) => {
            // Move the pivot without moving the box: shift the offset too.
            let rest_rotation = linear(rest);
            let local = (linear(parent) * rest_rotation).inverse() * world_delta;
            if let Some(b) = library.current_mut().and_then(|r| r.bone_mut(bone)) {
                let shift = rest_rotation * local;
                for i in 0..3 {
                    b.pivot[i] += local[i];
                    b.offset[i] += shift[i];
                }
            }
        }
        (MobEditMode::Animate, gizmo) => {
            let frame = linear(parent * rest);
            let Some(key) = keyframe_at(library, bone) else {
                return;
            };
            match gizmo {
                GizmoMode::Move | GizmoMode::Pivot => {
                    let local = frame.inverse() * world_delta;
                    for i in 0..3 {
                        key.translation[i] += local[i];
                    }
                }
                GizmoMode::Rotate => key.rotation[axis] += amount.to_degrees(),
                GizmoMode::Scale => key.scale[axis] = (key.scale[axis] + amount).max(0.01),
            }
        }
    }
    library.touch();
}

#[allow(clippy::too_many_arguments)]
fn gizmo_interaction(
    editor: Res<EditorState>,
    pie: Res<PieState>,
    ui: Res<EditorUiState>,
    mouse: Res<ButtonInput<MouseButton>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform), With<EditorCamera>>,
    mut library: ResMut<MobLibrary>,
    mut studio: ResMut<MobStudio>,
) {
    if !in_studio(&editor, &pie) {
        studio.hovered_axis = None;
        studio.drag = None;
        return;
    }
    if !mouse.pressed(MouseButton::Left) {
        studio.drag = None;
    }
    let (Ok(window), Ok((camera, camera_transform))) = (windows.single(), cameras.single()) else {
        return;
    };
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    let Ok(ray) = camera.viewport_to_world(camera_transform, cursor) else {
        return;
    };
    let ray_dir = *ray.direction;
    let camera_pos = camera_transform.translation();
    let to_screen = |p: Vec3| camera.world_to_viewport(camera_transform, p).ok();

    let frame = gizmo_frame(&library, &studio, camera_pos);

    // Hover detection.
    if studio.drag.is_none() {
        studio.hovered_axis = None;
        if let (Some((frame, _)), true) = (&frame, ui.viewport_hovered) {
            let mut best = (f32::MAX, None);
            for (index, axis) in frame.axes.iter().enumerate() {
                let distance = if studio.gizmo == GizmoMode::Rotate {
                    (0..48)
                        .filter_map(|step| {
                            let a = step as f32 / 48.0 * std::f32::consts::TAU;
                            let b = (step + 1) as f32 / 48.0 * std::f32::consts::TAU;
                            let pa = to_screen(ring_point(frame.origin, *axis, frame.size, a))?;
                            let pb = to_screen(ring_point(frame.origin, *axis, frame.size, b))?;
                            Some(segment_distance(cursor, pa, pb))
                        })
                        .fold(f32::MAX, f32::min)
                } else {
                    match (to_screen(frame.origin), to_screen(frame.origin + *axis * frame.size)) {
                        (Some(a), Some(b)) => segment_distance(cursor, a, b),
                        _ => f32::MAX,
                    }
                };
                if distance < best.0 {
                    best = (distance, Some(index));
                }
            }
            if best.0 < 9.0 {
                studio.hovered_axis = best.1;
            }
        }
    }

    // Start a drag or select a box.
    if mouse.just_pressed(MouseButton::Left) && ui.viewport_hovered {
        if let (Some(axis), Some((frame, bone))) = (studio.hovered_axis, &frame) {
            let start = if studio.gizmo == GizmoMode::Rotate {
                ring_angle(frame.origin, frame.axes[axis], ray.origin, ray_dir)
            } else {
                axis_param(frame.origin, frame.axes[axis], ray.origin, ray_dir)
            };
            if let Some(start) = start {
                studio.drag = Some(GizmoDrag {
                    axis,
                    bone: bone.clone(),
                    start,
                    applied: 0.0,
                });
            }
        } else if let Some(rig) = library.current() {
            let pose = studio_pose(&library, studio.edit).unwrap_or_default();
            let anchor = Vec3::from_array(library.preview_origin);
            let hit = drawn_bones(rig, &pose)
                .into_iter()
                .filter_map(|bone| {
                    ray_obb(ray.origin, ray_dir, anchor + bone.center, bone.rotation, bone.size)
                        .map(|t| (t, bone.name))
                })
                .min_by(|a, b| a.0.total_cmp(&b.0));
            let add = keyboard.pressed(KeyCode::ShiftLeft)
                || keyboard.pressed(KeyCode::ShiftRight)
                || keyboard.pressed(KeyCode::ControlLeft)
                || keyboard.pressed(KeyCode::ControlRight);
            library.select_bone(hit.map(|(_, name)| name), add);
        }
    }

    // Continue the drag.
    let Some(drag) = studio.drag.clone() else {
        return;
    };
    let Some((frame, _)) = frame else {
        return;
    };
    let axis_dir = frame.axes[drag.axis];
    let snapping = studio.snap && !(keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight));
    let (current, step) = if studio.gizmo == GizmoMode::Rotate {
        let Some(angle) = ring_angle(frame.origin, axis_dir, ray.origin, ray_dir) else {
            return;
        };
        // Unwrap around ±π so long drags keep accumulating.
        let mut total = angle - drag.start;
        while total - drag.applied > std::f32::consts::PI {
            total -= std::f32::consts::TAU;
        }
        while total - drag.applied < -std::f32::consts::PI {
            total += std::f32::consts::TAU;
        }
        (total, 15f32.to_radians())
    } else {
        let Some(param) = axis_param(frame.origin, axis_dir, ray.origin, ray_dir) else {
            return;
        };
        // Move / Pivot drag the gizmo origin along: add back what was applied.
        let moving = matches!(studio.gizmo, GizmoMode::Move | GizmoMode::Pivot);
        (param - drag.start + if moving { drag.applied } else { 0.0 }, 1.0 / 16.0)
    };
    let target = if snapping { (current / step).round() * step } else { current };
    let amount = target - drag.applied;
    if amount.abs() > 1e-6 {
        apply_delta(&mut library, &studio, &drag.bone, drag.axis, amount, axis_dir);
        // The other selected boxes follow the same delta.
        for other in library.selected_bones() {
            if other != drag.bone {
                apply_delta(&mut library, &studio, &other, drag.axis, amount, axis_dir);
            }
        }
        if let Some(active) = studio.drag.as_mut() {
            active.applied = target;
        }
    }
}

pub(crate) const AXIS_COLORS: [Color; 3] = [
    Color::srgb(0.95, 0.32, 0.36),
    Color::srgb(0.45, 0.88, 0.40),
    Color::srgb(0.36, 0.56, 0.98),
];

fn draw_studio(
    mut gizmos: Gizmos,
    editor: Res<EditorState>,
    pie: Res<PieState>,
    library: Res<MobLibrary>,
    studio: Res<MobStudio>,
    cameras: Query<&GlobalTransform, With<EditorCamera>>,
) {
    if !in_studio(&editor, &pie) {
        return;
    }
    let anchor = Vec3::from_array(library.preview_origin);

    // Studio floor: 1 block = 16 pixels.
    if studio.show_grid {
        let extent = 3;
        for i in -extent * 4..=extent * 4 {
            let f = i as f32 * 0.25;
            let major = i % 4 == 0;
            let color = if i == 0 {
                Color::srgba(0.8, 0.85, 0.9, 0.55)
            } else if major {
                Color::srgba(0.5, 0.6, 0.7, 0.35)
            } else {
                Color::srgba(0.4, 0.5, 0.6, 0.12)
            };
            let e = extent as f32;
            gizmos.line(anchor + Vec3::new(f, 0.0, -e), anchor + Vec3::new(f, 0.0, e), color);
            gizmos.line(anchor + Vec3::new(-e, 0.0, f), anchor + Vec3::new(e, 0.0, f), color);
        }
        // North marker.
        gizmos.arrow(anchor + Vec3::new(0.0, 0.0, -2.6), anchor + Vec3::new(0.0, 0.0, -3.2), Color::srgba(0.9, 0.9, 0.9, 0.5));
    }

    let Some(rig) = library.current() else {
        return;
    };
    let Some(pose) = studio_pose(&library, studio.edit) else {
        return;
    };
    let selected = library.bone_selection.as_deref();
    for bone in drawn_bones(rig, &pose) {
        let extra = library.bone_extra.contains(&bone.name);
        if Some(bone.name.as_str()) == selected || extra {
            gizmos.cube(
                Transform::from_translation(anchor + bone.center)
                    .with_rotation(bone.rotation)
                    .with_scale(bone.size * 1.02),
                if extra { Color::srgb(1.0, 0.62, 0.2) } else { Color::srgb(1.0, 0.82, 0.3) },
            );
        }
    }
    // Sockets.
    for (name, position) in crate::gameplay::mobs::pose::socket_positions(rig, &pose) {
        let chosen = library.socket_selection.as_deref() == Some(name.as_str());
        gizmos.sphere(
            Isometry3d::from_translation(anchor + position),
            if chosen { 0.07 } else { 0.045 },
            Color::srgb(0.95, 0.75, 0.35),
        );
    }

    // Gizmo.
    let Ok(camera) = cameras.single() else {
        return;
    };
    let Some((frame, _)) = gizmo_frame(&library, &studio, camera.translation()) else {
        return;
    };
    let active = studio.drag.as_ref().map(|d| d.axis).or(studio.hovered_axis);
    gizmos.sphere(Isometry3d::from_translation(frame.origin), frame.size * 0.06, Color::WHITE);
    for (index, axis) in frame.axes.iter().enumerate() {
        let color = if active == Some(index) { Color::srgb(1.0, 0.95, 0.4) } else { AXIS_COLORS[index] };
        match studio.gizmo {
            GizmoMode::Move | GizmoMode::Pivot => {
                gizmos.arrow(frame.origin, frame.origin + *axis * frame.size, color);
            }
            GizmoMode::Scale => {
                let tip = frame.origin + *axis * frame.size;
                gizmos.line(frame.origin, tip, color);
                gizmos.cube(Transform::from_translation(tip).with_scale(Vec3::splat(frame.size * 0.1)), color);
            }
            GizmoMode::Rotate => {
                let mut previous = ring_point(frame.origin, *axis, frame.size, 0.0);
                for step in 1..=48 {
                    let angle = step as f32 / 48.0 * std::f32::consts::TAU;
                    let point = ring_point(frame.origin, *axis, frame.size, angle);
                    gizmos.line(previous, point, color);
                    previous = point;
                }
            }
        }
    }
    if studio.gizmo == GizmoMode::Pivot {
        gizmos.sphere(Isometry3d::from_translation(frame.origin), frame.size * 0.12, Color::srgb(1.0, 0.4, 0.85));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn axis_param_finds_the_closest_point() {
        // Ray looking down -Z, crossing x = 2 on the X axis.
        let s = axis_param(Vec3::ZERO, Vec3::X, Vec3::new(2.0, 0.0, 5.0), Vec3::NEG_Z).unwrap();
        assert!((s - 2.0).abs() < 1e-4);
    }

    #[test]
    fn ray_hits_an_oriented_box() {
        let t = ray_obb(Vec3::new(0.0, 0.0, 5.0), Vec3::NEG_Z, Vec3::ZERO, Quat::from_rotation_y(0.7), Vec3::ONE);
        assert!(t.is_some());
        assert!(ray_obb(Vec3::new(3.0, 0.0, 5.0), Vec3::NEG_Z, Vec3::ZERO, Quat::IDENTITY, Vec3::ONE).is_none());
    }

    #[test]
    fn moving_the_pivot_keeps_the_box_in_place() {
        let mut library = MobLibrary::default();
        library.rigs.push(crate::gameplay::mobs::model::starter_rig());
        library.selection = 0;
        let rig = library.current().unwrap();
        let bone = rig.boxes[1].name.clone();
        let before = drawn_bones(rig, &RigPose::default())
            .into_iter()
            .find(|b| b.name == bone)
            .unwrap()
            .center;
        let studio = MobStudio {
            gizmo: GizmoMode::Pivot,
            ..Default::default()
        };
        apply_delta(&mut library, &studio, &bone, 0, 0.25, Vec3::X);
        let rig = library.current().unwrap();
        let after = drawn_bones(rig, &RigPose::default())
            .into_iter()
            .find(|b| b.name == bone)
            .unwrap()
            .center;
        assert!((before - after).length() < 1e-4);
    }
}
