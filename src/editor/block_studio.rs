//! 🔨 Forge: the block studio of the "Blocks" workspace.
//!
//! Works like the mob sculptor: the world is hidden, the block being edited
//! sits in the middle of the viewport on a turntable camera and every change
//! of the draft (colour, material, facets, liquid, model elements) is visible
//! immediately, rendered with the real voxel mesher and shader.
//!
//! A block can be a plain shape or a **model**: several boxes ("elements")
//! placed on a 16×16×16 grid. Elements are selected by clicking them and
//! edited with the gizmo (`G` move, `S` size, 1/16 snapping, `Ctrl` to
//! disable it) or from the details panel. The draft only reaches the world
//! when it is applied.

use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::core::app_state::{EditorState, EditorUiState, Workspace};
use crate::core::editor_camera::EditorCamera;
use crate::core::pie_manager::PieState;
use crate::editor::mob_studio::{axis_param, ray_obb, segment_distance, AXIS_COLORS};
use crate::editor::studio_common::{effective_workspace, orbit_input, Orbit, StudioSet};
use crate::renderer::poly_material::PolyVoxelMaterial;
use crate::renderer::voxel_render::{block_material, mesh_from_data};
use crate::voxel::blocks::{BlockDef, BlockId, BlockModel, BlockPart, BlockRegistry, ModelElement};
use crate::voxel::mesher;
use crate::voxel::world::{Voxel, VoxelWorld};

/// Where the forge preview is built (far from the studio lights).
pub const FORGE_ORIGIN: Vec3 = Vec3::new(0.0, 300.0, 0.0);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ForgeGizmo {
    #[default]
    Move,
    Size,
}

impl ForgeGizmo {
    pub const ALL: [Self; 2] = [Self::Move, Self::Size];

    pub const fn icon(self) -> &'static str {
        match self {
            Self::Move => "✛",
            Self::Size => "↗",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Move => "Move the element (G)",
            Self::Size => "Resize the element (S)",
        }
    }
}

/// How the draft is shown in the preview.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ForgePreview {
    /// One block.
    #[default]
    Single,
    /// A 3×2 wall: tiling of the facets and the Voronoi pattern.
    Wall,
    /// On a stone platform (liquids flow on it).
    Platform,
}

impl ForgePreview {
    pub const ALL: [Self; 3] = [Self::Single, Self::Wall, Self::Platform];

    pub const fn icon(self) -> &'static str {
        match self {
            Self::Single => "📦",
            Self::Wall => "🏢",
            Self::Platform => "👣",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Single => "Single block",
            Self::Wall => "Wall 3x2",
            Self::Platform => "On a platform (liquids flow)",
        }
    }
}

#[derive(Clone, Debug)]
struct ForgeDrag {
    axis: usize,
    element: usize,
    start: f32,
    applied: f32,
}

#[derive(Resource, Debug)]
pub struct BlockStudio {
    /// Block being edited (not yet applied to the registry).
    pub draft: BlockDef,
    /// Registry entry the draft came from.
    pub source: Option<BlockId>,
    /// The draft differs from the registry.
    pub dirty: bool,
    pub element: Option<usize>,
    /// Other selected elements (Shift / Ctrl + click): the gizmo and Delete
    /// apply to all of them.
    pub extra: Vec<usize>,
    pub gizmo: ForgeGizmo,
    pub preview: ForgePreview,
    pub orbit: Orbit,
    pub snap: bool,
    pub show_grid: bool,
    pub frame_request: bool,
    pub hovered_axis: Option<usize>,
    pub filter: String,
    drag: Option<ForgeDrag>,
    signature: String,
    last_workspace: Workspace,
    meshes: Vec<Handle<Mesh>>,
    materials: Vec<Handle<PolyVoxelMaterial>>,
    part_materials: Vec<Handle<StandardMaterial>>,
    part_cube: Option<Handle<Mesh>>,
}

impl Default for BlockStudio {
    fn default() -> Self {
        Self {
            draft: BlockRegistry::builtin().get(BlockId(0)).clone(),
            source: None,
            dirty: false,
            element: None,
            extra: Vec::new(),
            gizmo: ForgeGizmo::Move,
            preview: ForgePreview::Single,
            orbit: Orbit {
                target: FORGE_ORIGIN + Vec3::splat(0.5),
                distance: 3.2,
                ..Default::default()
            },
            snap: true,
            show_grid: true,
            frame_request: true,
            hovered_axis: None,
            filter: String::new(),
            drag: None,
            signature: String::new(),
            last_workspace: Workspace::World,
            meshes: Vec::new(),
            materials: Vec::new(),
            part_materials: Vec::new(),
            part_cube: None,
        }
    }
}

impl BlockStudio {
    pub fn is_dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// Load a registry block into the draft.
    pub fn edit(&mut self, registry: &BlockRegistry, id: BlockId) {
        self.draft = registry.get(id).clone();
        self.source = Some(id);
        self.dirty = false;
        self.element = None;
        self.extra.clear();
        self.drag = None;
        self.frame_request = true;
    }

    pub fn elements(&self) -> &[ModelElement] {
        self.draft.model.as_ref().map(|m| m.elements.as_slice()).unwrap_or(&[])
    }

    pub fn elements_mut(&mut self) -> &mut Vec<ModelElement> {
        &mut self.draft.model.get_or_insert_with(BlockModel::default).elements
    }

    pub fn touch(&mut self) {
        self.dirty = true;
    }

    /// Every selected element (the primary one first).
    pub fn selected(&self) -> Vec<usize> {
        let count = self.elements().len();
        let mut list: Vec<usize> = self.element.into_iter().chain(self.extra.iter().copied()).filter(|i| *i < count).collect();
        list.dedup();
        list
    }

    /// Click on an element: replace the selection, or toggle it with `add`.
    pub fn select(&mut self, index: Option<usize>, add: bool) {
        if !add {
            self.element = index;
            self.extra.clear();
            return;
        }
        let Some(index) = index else {
            return;
        };
        if self.element == Some(index) {
            // Toggle off the primary: the next selected becomes primary.
            self.element = if self.extra.is_empty() { None } else { Some(self.extra.remove(0)) };
        } else if let Some(position) = self.extra.iter().position(|i| *i == index) {
            self.extra.remove(position);
        } else if self.element.is_none() {
            self.element = Some(index);
        } else {
            self.extra.push(index);
        }
    }

    /// Add an element (a full cube for the first one, a half-size box after).
    pub fn add_element(&mut self) {
        let index = self.elements().len();
        let element = if index == 0 {
            ModelElement::new("Element 1", [0.0, 0.0, 0.0], [16.0, 16.0, 16.0])
        } else {
            ModelElement::new(&format!("Element {}", index + 1), [4.0, 4.0, 4.0], [12.0, 12.0, 12.0])
        };
        self.elements_mut().push(element);
        self.element = Some(index);
        self.touch();
    }
}

/// Root of the forge preview meshes.
#[derive(Component)]
struct ForgePreviewRoot;

/// Animated part of the draft, moved like in game (powered, motor running).
#[derive(Component)]
struct ForgePart {
    part: BlockPart,
    base: Vec3,
}

pub struct BlockStudioPlugin;

impl Plugin for BlockStudioPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BlockStudio>().add_systems(
            Update,
            (follow_selection, forge_shortcuts, forge_gizmo, forge_camera, sync_forge_preview, animate_forge_parts, draw_forge)
                .chain()
                .in_set(StudioSet),
        );
    }
}

fn in_forge(editor: &EditorState, pie: &PieState) -> bool {
    effective_workspace(editor, pie) == Workspace::Blocks
}

/// The toolbox / library selection drives the draft; entering the
/// workspace frames the block.
fn follow_selection(
    editor: Res<EditorState>,
    pie: Res<PieState>,
    registry: Res<BlockRegistry>,
    mut studio: ResMut<BlockStudio>,
) {
    let workspace = effective_workspace(&editor, &pie);
    if workspace == Workspace::Blocks && studio.last_workspace != Workspace::Blocks {
        studio.frame_request = true;
    }
    if workspace != Workspace::Blocks {
        studio.drag = None;
    }
    studio.last_workspace = workspace;
    if studio.source.is_none() || (studio.source != Some(editor.selected_block) && !studio.dirty) {
        studio.edit(&registry, editor.selected_block);
    }
    if studio.frame_request && workspace == Workspace::Blocks {
        studio.frame_request = false;
        let (center, size): (Vec3, f32) = match studio.preview {
            ForgePreview::Single => (Vec3::splat(0.5), 1.0),
            ForgePreview::Wall => (Vec3::new(1.5, 1.0, 0.5), 3.4),
            ForgePreview::Platform => (Vec3::new(0.5, 0.5, 0.5), 6.5),
        };
        studio.orbit.target = FORGE_ORIGIN + center;
        studio.orbit.distance = (size * 2.6).clamp(2.0, 30.0);
    }
}

fn forge_shortcuts(
    editor: Res<EditorState>,
    pie: Res<PieState>,
    ui: Res<EditorUiState>,
    keyboard: Res<ButtonInput<KeyCode>>,
    egui_input: Res<bevy_egui::input::EguiWantsInput>,
    mut studio: ResMut<BlockStudio>,
) {
    if !in_forge(&editor, &pie) || !ui.viewport_hovered || egui_input.wants_keyboard_input() {
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
        let mut selected = studio.selected();
        if !selected.is_empty() {
            selected.sort_unstable_by(|a, b| b.cmp(a));
            for index in selected {
                studio.elements_mut().remove(index);
            }
            studio.select(None, false);
            studio.touch();
        }
    }
}

fn forge_camera(
    editor: Res<EditorState>,
    pie: Res<PieState>,
    ui: Res<EditorUiState>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    mut studio: ResMut<BlockStudio>,
    mut cameras: Query<&mut Transform, With<EditorCamera>>,
) {
    if !in_forge(&editor, &pie) {
        return;
    }
    if ui.viewport_hovered && !studio.is_dragging() {
        orbit_input(&mut studio.orbit, &keyboard, &mouse, &motion, &scroll, 40.0);
    }
    if let Ok(mut transform) = cameras.single_mut() {
        *transform = studio.orbit.transform();
    }
}

/// World-space bounds of an element in the preview.
fn element_box(element: &ModelElement) -> (Vec3, Vec3) {
    let (min, max) = element.bounds();
    (FORGE_ORIGIN + min, FORGE_ORIGIN + max)
}

/// Gizmo origin of the selected element: centre (move) or +axis faces (size).
fn gizmo_origin(studio: &BlockStudio) -> Option<(usize, Vec3, f32)> {
    let index = studio.element?;
    let element = studio.elements().get(index)?;
    let (min, max) = element_box(element);
    let size = ((max - min).max_element() * 0.75).clamp(0.3, 0.8);
    Some((index, (min + max) * 0.5, size))
}

#[allow(clippy::too_many_arguments)]
fn forge_gizmo(
    editor: Res<EditorState>,
    pie: Res<PieState>,
    ui: Res<EditorUiState>,
    mouse: Res<ButtonInput<MouseButton>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform), With<EditorCamera>>,
    mut studio: ResMut<BlockStudio>,
) {
    if !in_forge(&editor, &pie) {
        studio.hovered_axis = None;
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
    let frame = gizmo_origin(&studio);

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
        if let (Some(axis), Some((element, origin, _))) = (studio.hovered_axis, frame) {
            if let Some(start) = axis_param(origin, axes[axis], ray.origin, ray_dir) {
                studio.drag = Some(ForgeDrag {
                    axis,
                    element,
                    start,
                    applied: 0.0,
                });
            }
        } else {
            let hit = studio
                .elements()
                .iter()
                .enumerate()
                .filter_map(|(index, element)| {
                    let (min, max) = element_box(element);
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
    let axis_dir = axes[drag.axis];
    let Some(param) = axis_param(origin, axis_dir, ray.origin, ray_dir) else {
        return;
    };
    // Move drags the gizmo along (add back what was applied); Size grows the
    // box symmetrically around its centre, so the gizmo stays in place.
    let moving = studio.gizmo == ForgeGizmo::Move;
    let current = param - drag.start + if moving { drag.applied } else { 0.0 };
    let snapping = studio.snap && !(keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight));
    let step = if moving { 1.0 / 16.0 } else { 1.0 / 32.0 };
    let target = if snapping { (current / step).round() * step } else { current };
    let amount = target - drag.applied;
    if amount.abs() < 1e-6 {
        return;
    }
    let gizmo = studio.gizmo;
    let pixels = amount * 16.0;
    let axis = drag.axis;
    // The dragged element and every other selected one move together.
    let mut targets = studio.selected();
    if !targets.contains(&drag.element) {
        targets.insert(0, drag.element);
    }
    for index in targets {
        let Some(element) = studio.elements_mut().get_mut(index) else {
            continue;
        };
        let low = element.from[axis].min(element.to[axis]);
        let high = element.from[axis].max(element.to[axis]);
        match gizmo {
            ForgeGizmo::Move => {
                let shift = pixels.clamp(-low, 16.0 - high);
                element.from[axis] = low + shift;
                element.to[axis] = high + shift;
            }
            ForgeGizmo::Size => {
                let center = (low + high) * 0.5;
                let half = ((high - low) * 0.5 + pixels).clamp(0.25, 8.0);
                element.from[axis] = (center - half).max(0.0);
                element.to[axis] = (center + half).min(16.0);
            }
        }
    }
    if let Some(active) = studio.drag.as_mut() {
        active.applied = target;
    }
    studio.touch();
}

/// Rebuild the preview meshes whenever the draft (or the look) changes.
#[allow(clippy::too_many_arguments)]
fn sync_forge_preview(
    mut commands: Commands,
    editor: Res<EditorState>,
    pie: Res<PieState>,
    ui: Res<EditorUiState>,
    registry: Res<BlockRegistry>,
    mut studio: ResMut<BlockStudio>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<PolyVoxelMaterial>>,
    mut part_materials: ResMut<Assets<StandardMaterial>>,
    roots: Query<Entity, With<ForgePreviewRoot>>,
) {
    let active = in_forge(&editor, &pie);
    let signature = if active {
        format!(
            "{}|{:?}|{}|{}",
            ron::to_string(&studio.draft).unwrap_or_default(),
            studio.preview,
            ui.shading.revision,
            registry.revision
        )
    } else {
        String::new()
    };
    if signature == studio.signature {
        return;
    }
    studio.signature = signature;
    for root in &roots {
        commands.entity(root).despawn();
    }
    for handle in studio.meshes.drain(..) {
        meshes.remove(handle.id());
    }
    for handle in studio.materials.drain(..) {
        materials.remove(handle.id());
    }
    for handle in studio.part_materials.drain(..) {
        part_materials.remove(handle.id());
    }
    if !active {
        return;
    }

    // Temporary registry holding the draft, and a tiny world around it.
    let mut preview_registry = registry.clone();
    let mut draft = studio.draft.clone();
    if draft.id.trim().is_empty() {
        draft.id = "__forge_draft".to_string();
    }
    let block = preview_registry.upsert(draft);
    let stone = preview_registry.id_or_default("stone");
    let mut world = VoxelWorld::new();
    let def = preview_registry.get(block).clone();
    match studio.preview {
        ForgePreview::Single => {
            world.set(IVec3::ZERO, Some(Voxel::new(block)));
        }
        ForgePreview::Wall => {
            for x in 0..3 {
                for y in 0..2 {
                    world.set(IVec3::new(x, y, 0), Some(Voxel::new(block)));
                }
            }
        }
        ForgePreview::Platform => {
            for x in -3..=4 {
                for z in -3..=4 {
                    world.set(IVec3::new(x, -1, z), Some(Voxel::new(stone)));
                }
            }
            if def.is_liquid() {
                world.set(IVec3::new(0, 1, 0), Some(Voxel::new(block)));
                crate::voxel::fluids::settle(&preview_registry, &mut world, 600);
            } else {
                world.set(IVec3::ZERO, Some(Voxel::new(block)));
            }
        }
    }

    let root = commands
        .spawn((
            Name::new("Forge preview"),
            ForgePreviewRoot,
            Transform::from_translation(FORGE_ORIGIN),
            Visibility::default(),
        ))
        .id();
    let coords: Vec<IVec3> = world.chunks().map(|chunk| chunk.coord).collect();
    let mut cache: std::collections::HashMap<BlockId, Handle<PolyVoxelMaterial>> = Default::default();
    for coord in coords {
        let origin = coord.as_vec3() * crate::voxel::world::CHUNK_SIZE as f32;
        for (id, data) in mesher::build_chunk_mesh(&world, &preview_registry, coord, ui.greedy_meshing) {
            let mesh = meshes.add(mesh_from_data(data));
            let material = cache
                .entry(id)
                .or_insert_with(|| {
                    let handle = materials.add(block_material(preview_registry.get(id), &ui.shading));
                    studio.materials.push(handle.clone());
                    handle
                })
                .clone();
            studio.meshes.push(mesh.clone());
            commands.spawn((
                Mesh3d(mesh),
                MeshMaterial3d(material),
                Transform::from_translation(origin),
                ChildOf(root),
            ));
        }
    }

    // The animated parts, on every placed copy of the block.
    let parts = def.parts.clone().unwrap_or_default();
    if parts.is_empty() {
        return;
    }
    let cube = studio.part_cube.get_or_insert_with(|| meshes.add(Cuboid::new(1.0, 1.0, 1.0))).clone();
    let mut looks = Vec::new();
    for part in &parts {
        let color = part.powered_color.unwrap_or(part.color);
        let glow = if part.powered_color.is_some() { part.powered_emissive } else { part.emissive };
        let base = Color::srgb_u8(color[0], color[1], color[2]);
        looks.push(part_materials.add(StandardMaterial {
            base_color: base,
            emissive: base.to_linear() * glow,
            metallic: part.metallic,
            perceptual_roughness: 0.45,
            ..default()
        }));
    }
    studio.part_materials.extend(looks.iter().cloned());
    let cells: Vec<IVec3> = world
        .chunks()
        .flat_map(|chunk| {
            let origin = chunk.origin();
            (0..crate::voxel::world::CHUNK_SIZE.pow(3)).filter_map(move |index| {
                let local = crate::voxel::world::VoxelChunk::local_from_index(index);
                chunk.get(local).filter(|voxel| voxel.block == block).map(|_| origin + local.as_ivec3())
            })
        })
        .collect();
    for cell in cells {
        let center = cell.as_vec3() + Vec3::splat(0.5);
        for (part, look) in parts.iter().zip(&looks) {
            let (part_center, size) = part.center_size();
            let pivot = part.pivot_point();
            let holder = commands
                .spawn((
                    Transform::from_translation(center + pivot),
                    Visibility::default(),
                    ForgePart { part: part.clone(), base: center + pivot },
                    ChildOf(root),
                ))
                .id();
            commands.spawn((
                Mesh3d(cube.clone()),
                MeshMaterial3d(look.clone()),
                Transform::from_translation(part_center - pivot).with_rotation(part.rotation_quat()).with_scale(size),
                ChildOf(holder),
            ));
        }
    }
}

/// Same motion as `machines::visuals`, with everything running.
fn animate_forge_parts(time: Res<Time>, mut parts: Query<(&ForgePart, &mut Transform)>) {
    let clock = time.elapsed_secs();
    for (forge, mut transform) in &mut parts {
        let part = &forge.part;
        let wave = (clock * part.speed + part.phase).sin();
        let axis = Vec3::from_array(part.spin_axis).normalize_or(Vec3::Y);
        let spin = if part.motion == crate::voxel::blocks::PartMotion::Motor { part.spin * 2.0 } else { part.spin };
        transform.rotation = Quat::from_axis_angle(axis, clock * spin + wave * part.swing.to_radians());
        transform.translation = forge.base + Vec3::from_array(part.bob_axis).normalize_or(Vec3::Y) * (wave * part.bob);
        transform.scale = Vec3::splat(1.0 + wave * part.pulse);
    }
}

fn draw_forge(
    mut gizmos: Gizmos,
    editor: Res<EditorState>,
    pie: Res<PieState>,
    studio: Res<BlockStudio>,
) {
    if !in_forge(&editor, &pie) {
        return;
    }
    // 16×16 pixel grid under the block.
    if studio.show_grid {
        for i in 0..=16 {
            let f = i as f32 / 16.0;
            let color = if i % 4 == 0 {
                Color::srgba(0.55, 0.65, 0.75, 0.45)
            } else {
                Color::srgba(0.4, 0.5, 0.6, 0.15)
            };
            gizmos.line(FORGE_ORIGIN + Vec3::new(f, 0.0, 0.0), FORGE_ORIGIN + Vec3::new(f, 0.0, 1.0), color);
            gizmos.line(FORGE_ORIGIN + Vec3::new(0.0, 0.0, f), FORGE_ORIGIN + Vec3::new(1.0, 0.0, f), color);
        }
        gizmos.cube(
            Transform::from_translation(FORGE_ORIGIN + Vec3::splat(0.5)),
            Color::srgba(0.5, 0.6, 0.7, 0.25),
        );
        gizmos.arrow(
            FORGE_ORIGIN + Vec3::new(0.5, 0.0, -0.2),
            FORGE_ORIGIN + Vec3::new(0.5, 0.0, -0.6),
            Color::srgba(0.9, 0.9, 0.9, 0.5),
        );
    }

    for (index, element) in studio.elements().iter().enumerate() {
        let (min, max) = element_box(element);
        let selected = studio.element == Some(index);
        let color = if selected {
            Color::srgb(1.0, 0.82, 0.3)
        } else if studio.extra.contains(&index) {
            Color::srgb(1.0, 0.62, 0.2)
        } else {
            Color::srgba(0.8, 0.9, 1.0, 0.18)
        };
        gizmos.cube(
            Transform::from_translation((min + max) * 0.5).with_scale((max - min) * 1.01 + Vec3::splat(0.002)),
            color,
        );
    }

    let Some((_, origin, size)) = gizmo_origin(&studio) else {
        return;
    };
    let active = studio.drag.as_ref().map(|d| d.axis).or(studio.hovered_axis);
    gizmos.sphere(Isometry3d::from_translation(origin), size * 0.06, Color::WHITE);
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
