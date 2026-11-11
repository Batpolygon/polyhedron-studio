//! 🗡 Armurerie: the item studio of the "Items" workspace.
//!
//! The selected item is shown in the viewport on a turntable, built from its
//! coloured boxes (1/16 block grid). Click a box to select it, then drag the
//! gizmo (`G` move, `S` size, 1/16 snapping, `Ctrl` to disable it) or edit
//! it in the details panel. Items are edited in place in the library and
//! saved with the panel's *Enregistrer* button.

use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::core::app_state::{EditorState, EditorUiState, Workspace};
use crate::core::editor_camera::EditorCamera;
use crate::core::pie_manager::PieState;
use crate::editor::block_studio::ForgeGizmo;
use crate::editor::mob_studio::{axis_param, ray_obb, segment_distance, AXIS_COLORS};
use crate::editor::studio_common::{effective_workspace, orbit_input, Orbit, StudioSet};
use crate::gameplay::items::{item_mesh, ItemLibrary, ItemPart};

/// Where the item preview is built.
pub const ITEM_ORIGIN: Vec3 = Vec3::new(0.0, 900.0, 0.0);

#[derive(Clone, Debug)]
struct PartDrag {
    axis: usize,
    part: usize,
    start: f32,
    applied: f32,
}

#[derive(Resource, Debug)]
pub struct ItemStudio {
    pub part: Option<usize>,
    /// Other selected boxes (Shift / Ctrl + click).
    pub extra: Vec<usize>,
    pub gizmo: ForgeGizmo,
    pub orbit: Orbit,
    pub snap: bool,
    pub frame_request: bool,
    pub hovered_axis: Option<usize>,
    drag: Option<PartDrag>,
    signature: String,
    last_workspace: Workspace,
    mesh: Option<Handle<Mesh>>,
    material: Option<Handle<StandardMaterial>>,
}

impl Default for ItemStudio {
    fn default() -> Self {
        Self {
            part: None,
            extra: Vec::new(),
            gizmo: ForgeGizmo::Move,
            orbit: Orbit {
                target: ITEM_ORIGIN + Vec3::new(0.5, 0.5, 0.5),
                distance: 2.6,
                ..Default::default()
            },
            snap: true,
            frame_request: true,
            hovered_axis: None,
            drag: None,
            signature: String::new(),
            last_workspace: Workspace::World,
            mesh: None,
            material: None,
        }
    }
}

impl ItemStudio {
    pub fn is_dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// Every selected box (the primary one first).
    pub fn selected(&self) -> Vec<usize> {
        let mut list: Vec<usize> = self.part.into_iter().chain(self.extra.iter().copied()).collect();
        list.dedup();
        list
    }

    /// Click on a box: replace the selection, or toggle it with `add`.
    pub fn select(&mut self, index: Option<usize>, add: bool) {
        if !add {
            self.part = index;
            self.extra.clear();
            return;
        }
        let Some(index) = index else {
            return;
        };
        if self.part == Some(index) {
            self.part = if self.extra.is_empty() { None } else { Some(self.extra.remove(0)) };
        } else if let Some(position) = self.extra.iter().position(|i| *i == index) {
            self.extra.remove(position);
        } else if self.part.is_none() {
            self.part = Some(index);
        } else {
            self.extra.push(index);
        }
    }
}

#[derive(Component)]
struct ItemPreview;

pub struct ItemStudioPlugin;

impl Plugin for ItemStudioPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ItemStudio>().add_systems(
            Update,
            (item_shortcuts, item_gizmo, item_camera, sync_item_preview, draw_item_studio)
                .chain()
                .in_set(StudioSet),
        );
    }
}

fn in_armoury(editor: &EditorState, pie: &PieState) -> bool {
    effective_workspace(editor, pie) == Workspace::Items
}

fn part_box(part: &ItemPart) -> (Vec3, Vec3) {
    let (min, max) = part.bounds();
    (ITEM_ORIGIN + min, ITEM_ORIGIN + max)
}

fn item_shortcuts(
    editor: Res<EditorState>,
    pie: Res<PieState>,
    ui: Res<EditorUiState>,
    keyboard: Res<ButtonInput<KeyCode>>,
    egui_input: Res<bevy_egui::input::EguiWantsInput>,
    mut library: ResMut<ItemLibrary>,
    mut studio: ResMut<ItemStudio>,
) {
    let workspace = effective_workspace(&editor, &pie);
    if workspace == Workspace::Items && studio.last_workspace != Workspace::Items {
        studio.frame_request = true;
    }
    studio.last_workspace = workspace;
    if !in_armoury(&editor, &pie) || !ui.viewport_hovered || egui_input.wants_keyboard_input() {
        return;
    }
    if keyboard.just_pressed(KeyCode::KeyG) {
        studio.gizmo = ForgeGizmo::Move;
    }
    if keyboard.just_pressed(KeyCode::KeyS) {
        studio.gizmo = ForgeGizmo::Size;
    }
    if keyboard.just_pressed(KeyCode::KeyF) {
        studio.frame_request = true;
    }
    if keyboard.just_pressed(KeyCode::Delete) {
        let selection = library.selection;
        let mut selected = studio.selected();
        if let (false, Some(item)) = (selected.is_empty(), library.items.get_mut(selection)) {
            selected.sort_unstable_by(|a, b| b.cmp(a));
            for index in selected {
                if index < item.parts.len() {
                    item.parts.remove(index);
                }
            }
            studio.select(None, false);
            library.touch();
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn item_camera(
    editor: Res<EditorState>,
    pie: Res<PieState>,
    ui: Res<EditorUiState>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    library: Res<ItemLibrary>,
    mut studio: ResMut<ItemStudio>,
    mut cameras: Query<&mut Transform, With<EditorCamera>>,
) {
    if !in_armoury(&editor, &pie) {
        return;
    }
    if ui.viewport_hovered && !studio.is_dragging() {
        orbit_input(&mut studio.orbit, &keyboard, &mouse, &motion, &scroll, 30.0);
    }
    if std::mem::take(&mut studio.frame_request) {
        let (min, max) = library.items.get(library.selection).map(|i| i.bounds()).unwrap_or((Vec3::ZERO, Vec3::ONE));
        studio.orbit.target = ITEM_ORIGIN + (min + max) * 0.5;
        studio.orbit.distance = ((max - min).length() * 2.4).clamp(1.2, 20.0);
    }
    if let Ok(mut transform) = cameras.single_mut() {
        *transform = studio.orbit.transform();
    }
}

#[allow(clippy::too_many_arguments)]
fn item_gizmo(
    editor: Res<EditorState>,
    pie: Res<PieState>,
    ui: Res<EditorUiState>,
    mouse: Res<ButtonInput<MouseButton>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform), With<EditorCamera>>,
    mut library: ResMut<ItemLibrary>,
    mut studio: ResMut<ItemStudio>,
) {
    if !in_armoury(&editor, &pie) {
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
    let to_screen = |p: Vec3| camera.world_to_viewport(camera_transform, p).ok();
    let axes = [Vec3::X, Vec3::Y, Vec3::Z];
    let selection = library.selection;
    let frame = studio.part.and_then(|index| {
        let part = library.items.get(selection)?.parts.get(index)?;
        let (min, max) = part_box(part);
        Some((index, (min + max) * 0.5, ((max - min).max_element() * 0.8).clamp(0.2, 0.6)))
    });

    if studio.drag.is_none() {
        studio.hovered_axis = None;
        if let (Some((_, origin, size)), true) = (frame, ui.viewport_hovered) {
            let mut best = (f32::MAX, None);
            for (index, axis) in axes.iter().enumerate() {
                if let (Some(a), Some(b)) = (to_screen(origin), to_screen(origin + *axis * size)) {
                    let distance = segment_distance(cursor, a, b);
                    if distance < best.0 {
                        best = (distance, Some(index));
                    }
                }
            }
            if best.0 < 9.0 {
                studio.hovered_axis = best.1;
            }
        }
    }

    if mouse.just_pressed(MouseButton::Left) && ui.viewport_hovered {
        if let (Some(axis), Some((part, origin, _))) = (studio.hovered_axis, frame) {
            if let Some(start) = axis_param(origin, axes[axis], ray.origin, ray_dir) {
                studio.drag = Some(PartDrag { axis, part, start, applied: 0.0 });
            }
        } else if let Some(item) = library.items.get(selection) {
            let hit = item
                .parts
                .iter()
                .enumerate()
                .filter_map(|(index, part)| {
                    let (min, max) = part_box(part);
                    ray_obb(ray.origin, ray_dir, (min + max) * 0.5, Quat::IDENTITY, max - min).map(|t| (t, index))
                })
                .min_by(|a, b| a.0.total_cmp(&b.0));
            let add = keyboard.pressed(KeyCode::ShiftLeft)
                || keyboard.pressed(KeyCode::ShiftRight)
                || keyboard.pressed(KeyCode::ControlLeft)
                || keyboard.pressed(KeyCode::ControlRight);
            studio.select(hit.map(|(_, index)| index), add);
        }
    }

    let Some(drag) = studio.drag.clone() else {
        return;
    };
    let Some((_, origin, _)) = frame else {
        return;
    };
    let Some(param) = axis_param(origin, axes[drag.axis], ray.origin, ray_dir) else {
        return;
    };
    let moving = studio.gizmo == ForgeGizmo::Move;
    let current = param - drag.start + if moving { drag.applied } else { 0.0 };
    let snapping = studio.snap && !(keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight));
    let step = if moving { 1.0 / 16.0 } else { 1.0 / 32.0 };
    let target = if snapping { (current / step).round() * step } else { current };
    let amount = target - drag.applied;
    if amount.abs() < 1e-6 {
        return;
    }
    let pixels = amount * 16.0;
    let axis = drag.axis;
    // The dragged box and every other selected one move together.
    let mut targets = studio.selected();
    if !targets.contains(&drag.part) {
        targets.insert(0, drag.part);
    }
    let Some(item) = library.items.get_mut(selection) else {
        return;
    };
    for index in targets {
        let Some(part) = item.parts.get_mut(index) else {
            continue;
        };
        let low = part.from[axis].min(part.to[axis]);
        let high = part.from[axis].max(part.to[axis]);
        if moving {
            part.from[axis] = low + pixels;
            part.to[axis] = high + pixels;
        } else {
            let center = (low + high) * 0.5;
            let half = ((high - low) * 0.5 + pixels).clamp(0.25, 16.0);
            part.from[axis] = center - half;
            part.to[axis] = center + half;
        }
    }
    if let Some(active) = studio.drag.as_mut() {
        active.applied = target;
    }
    library.touch();
}

#[allow(clippy::too_many_arguments)]
fn sync_item_preview(
    mut commands: Commands,
    editor: Res<EditorState>,
    pie: Res<PieState>,
    library: Res<ItemLibrary>,
    mut studio: ResMut<ItemStudio>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    previews: Query<Entity, With<ItemPreview>>,
) {
    let active = in_armoury(&editor, &pie);
    let item = library.items.get(library.selection);
    let signature = match (active, item) {
        (true, Some(item)) => ron::to_string(item).unwrap_or_default(),
        _ => String::new(),
    };
    if signature == studio.signature {
        return;
    }
    studio.signature = signature;
    for entity in &previews {
        commands.entity(entity).despawn();
    }
    if let Some(old) = studio.mesh.take() {
        meshes.remove(old.id());
    }
    let (Some(item), true) = (item, active) else {
        return;
    };
    let material = studio
        .material
        .get_or_insert_with(|| {
            materials.add(StandardMaterial {
                base_color: Color::WHITE,
                perceptual_roughness: 0.6,
                ..default()
            })
        })
        .clone();
    // `item_mesh` is centred on the grip: shift it back to the 16-grid.
    let (min, max) = item.bounds();
    let pivot = Vec3::new((min.x + max.x) * 0.5, min.y, (min.z + max.z) * 0.5);
    let mesh = meshes.add(item_mesh(item));
    studio.mesh = Some(mesh.clone());
    commands.spawn((
        Name::new("Item preview"),
        ItemPreview,
        Mesh3d(mesh),
        MeshMaterial3d(material),
        Transform::from_translation(ITEM_ORIGIN + pivot),
    ));
}

fn draw_item_studio(
    mut gizmos: Gizmos,
    editor: Res<EditorState>,
    pie: Res<PieState>,
    library: Res<ItemLibrary>,
    studio: Res<ItemStudio>,
) {
    if !in_armoury(&editor, &pie) {
        return;
    }
    for i in 0..=16 {
        let f = i as f32 / 16.0;
        let color = if i % 4 == 0 { Color::srgba(0.55, 0.65, 0.75, 0.4) } else { Color::srgba(0.4, 0.5, 0.6, 0.12) };
        gizmos.line(ITEM_ORIGIN + Vec3::new(f, 0.0, 0.0), ITEM_ORIGIN + Vec3::new(f, 0.0, 1.0), color);
        gizmos.line(ITEM_ORIGIN + Vec3::new(0.0, 0.0, f), ITEM_ORIGIN + Vec3::new(1.0, 0.0, f), color);
    }
    let Some(item) = library.items.get(library.selection) else {
        return;
    };
    for (index, part) in item.parts.iter().enumerate() {
        let (min, max) = part_box(part);
        let selected = studio.part == Some(index);
        let color = if selected {
            Color::srgb(1.0, 0.82, 0.3)
        } else if studio.extra.contains(&index) {
            Color::srgb(1.0, 0.62, 0.2)
        } else {
            Color::srgba(0.8, 0.9, 1.0, 0.15)
        };
        gizmos.cube(Transform::from_translation((min + max) * 0.5).with_scale((max - min) * 1.01 + Vec3::splat(0.002)), color);
    }
    let Some((min, max)) = studio.part.and_then(|index| item.parts.get(index)).map(part_box) else {
        return;
    };
    let origin = (min + max) * 0.5;
    let size = ((max - min).max_element() * 0.8).clamp(0.2, 0.6);
    let active = studio.drag.as_ref().map(|d| d.axis).or(studio.hovered_axis);
    for (index, axis) in [Vec3::X, Vec3::Y, Vec3::Z].iter().enumerate() {
        let color = if active == Some(index) { Color::srgb(1.0, 0.95, 0.4) } else { AXIS_COLORS[index] };
        match studio.gizmo {
            ForgeGizmo::Move => {
                gizmos.arrow(origin, origin + *axis * size, color);
            }
            ForgeGizmo::Size => {
                let tip = origin + *axis * size;
                gizmos.line(origin, tip, color);
                gizmos.cube(Transform::from_translation(tip).with_scale(Vec3::splat(size * 0.1)), color);
            }
        }
    }
}
