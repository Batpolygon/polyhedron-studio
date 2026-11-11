//! Voxel prefabs (`project/structures/*.ron`) and the isolated structure room.
//!
//! A structure is a list of cells relative to its minimum corner plus
//! optional anchor cells: every anchor must sit directly on solid ground for
//! the world generator to accept a placement.
//!
//! The structure editor works in an isolated room high above the world
//! ([`ROOM_ORIGIN`]): opening a prefab clears the room box, lays a temporary
//! floor, stamps the prefab and moves the editor camera there. The normal
//! brush edits it; capturing writes the box content back to the prefab file.
//! Closing the room erases it, so it never leaks into saved worlds.

use std::fs;
use std::path::PathBuf;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::core::project_io::{load_ron, save_ron, ProjectLayout};
use crate::voxel::biomes::{file_stem, ron_files};
use crate::voxel::blocks::BlockRegistry;
use crate::voxel::shapes::Orientation;
use crate::voxel::world::{SubVoxel, Voxel, VoxelWorld};

/// Minimum corner of the structure room.
pub const ROOM_ORIGIN: IVec3 = IVec3::new(4096, 256, 4096);
/// Block used for the temporary floor (never captured).
pub const ROOM_FLOOR_BLOCK: &str = "deepslate";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StructureCell {
    pub pos: [i32; 3],
    pub block: String,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub rot: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sub: Option<(String, u8)>,
}

fn is_zero(value: &u8) -> bool {
    *value == 0
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StructureDef {
    pub name: String,
    /// Bounding box size, in voxels.
    pub size: [i32; 3],
    pub cells: Vec<StructureCell>,
    /// Cells (relative) that must rest on solid ground.
    #[serde(default)]
    pub anchors: Vec<[i32; 3]>,
}

impl StructureDef {
    pub fn new(name: &str, size: [i32; 3]) -> Self {
        Self {
            name: name.to_string(),
            size,
            cells: Vec::new(),
            anchors: Vec::new(),
        }
    }

    pub fn put(&mut self, x: i32, y: i32, z: i32, block: &str) {
        self.cells.retain(|cell| cell.pos != [x, y, z]);
        self.cells.push(StructureCell {
            pos: [x, y, z],
            block: block.to_string(),
            rot: 0,
            sub: None,
        });
    }

    /// Resolve a cell into a voxel with the registry.
    pub fn voxel(cell: &StructureCell, registry: &BlockRegistry) -> Option<Voxel> {
        let block = registry.find(&cell.block)?;
        let mut voxel = Voxel::oriented(block, Orientation(cell.rot % 24));
        if let Some((key, rot)) = &cell.sub {
            if let Some(sub) = registry.find(key) {
                voxel = voxel.with_sub(SubVoxel {
                    block: sub,
                    orientation: Orientation(*rot % 24),
                });
            }
        }
        Some(voxel)
    }

    /// Anchors, or the bottom layer's occupied columns when none are set.
    pub fn effective_anchors(&self) -> Vec<IVec3> {
        if !self.anchors.is_empty() {
            return self.anchors.iter().map(|a| IVec3::from_array(*a)).collect();
        }
        self.cells
            .iter()
            .filter(|cell| cell.pos[1] == 0)
            .map(|cell| IVec3::from_array(cell.pos))
            .collect()
    }
}

#[derive(Resource, Debug, Clone, Default)]
pub struct StructureLibrary {
    pub structures: Vec<StructureDef>,
    pub selection: usize,
    pub revision: u64,
}

impl StructureLibrary {
    pub fn dir(project: &ProjectLayout) -> PathBuf {
        project.module_path("structures")
    }

    pub fn find(&self, name: &str) -> Option<&StructureDef> {
        self.structures.iter().find(|s| s.name == name)
    }

    pub fn names(&self) -> Vec<String> {
        self.structures.iter().map(|s| s.name.clone()).collect()
    }

    pub fn load_from_project(&mut self, project: &ProjectLayout) -> Result<Vec<String>, String> {
        let dir = Self::dir(project);
        fs::create_dir_all(&dir).map_err(|error| format!("cannot create {}: {error}", dir.display()))?;
        let mut warnings = Vec::new();
        let mut structures = Vec::new();
        for path in ron_files(&dir)? {
            match load_ron::<StructureDef>(&path) {
                Ok(structure) => structures.push(structure),
                Err(error) => warnings.push(error),
            }
        }
        // Make sure every default prefab referenced by the default biomes exists.
        let mut created = Vec::new();
        for default in default_structures() {
            if !structures.iter().any(|s: &StructureDef| s.name == default.name) {
                created.push(default.name.clone());
                structures.push(default);
            }
        }
        structures.sort_by(|a, b| a.name.cmp(&b.name));
        self.structures = structures;
        self.revision = self.revision.wrapping_add(1);
        for name in created {
            if let Some(structure) = self.find(&name) {
                save_ron(&dir.join(format!("{}.ron", file_stem(&name))), structure)?;
            }
        }
        Ok(warnings)
    }

    pub fn save_one(&self, project: &ProjectLayout, index: usize) -> Result<PathBuf, String> {
        let structure = self.structures.get(index).ok_or("no structure selected")?;
        let path = Self::dir(project).join(format!("{}.ron", file_stem(&structure.name)));
        save_ron(&path, structure)?;
        Ok(path)
    }
}

/// State of the isolated structure room.
#[derive(Resource, Debug, Default)]
pub struct StructureRoom {
    /// Index of the prefab being edited.
    pub editing: Option<usize>,
    /// Editable box size (can differ from the prefab until captured).
    pub size: [i32; 3],
    /// Anchor cells (relative) being edited.
    pub anchors: Vec<[i32; 3]>,
    /// LMB toggles anchors instead of placing blocks.
    pub anchor_mode: bool,
    /// Editor camera transform before entering the room.
    pub saved_camera: Option<Transform>,
    /// Camera move requested by the editor panel.
    pub camera_request: Option<Transform>,
}

impl StructureRoom {
    pub fn contains(&self, cell: IVec3) -> bool {
        if self.editing.is_none() {
            return false;
        }
        let local = cell - ROOM_ORIGIN;
        local.x >= 0
            && local.y >= 0
            && local.z >= 0
            && local.x < self.size[0]
            && local.y < self.size[1]
            && local.z < self.size[2]
    }

    /// Clear the room box and its floor.
    pub fn clear(&self, world: &mut VoxelWorld, size: [i32; 3]) {
        for x in -1..=size[0] {
            for z in -1..=size[2] {
                for y in -1..size[1] {
                    world.set(ROOM_ORIGIN + IVec3::new(x, y, z), None);
                }
            }
        }
    }

    /// Open a prefab in the room.
    pub fn open(&mut self, world: &mut VoxelWorld, registry: &BlockRegistry, structure: &StructureDef, index: usize) {
        if let Some(previous) = self.editing {
            let _ = previous;
            self.clear(world, self.size);
        }
        self.editing = Some(index);
        self.size = [
            structure.size[0].clamp(1, 48),
            structure.size[1].clamp(1, 48),
            structure.size[2].clamp(1, 48),
        ];
        self.anchors = structure.anchors.clone();
        self.clear(world, self.size);
        self.lay_floor(world, registry);
        for cell in &structure.cells {
            if let Some(voxel) = StructureDef::voxel(cell, registry) {
                world.set(ROOM_ORIGIN + IVec3::from_array(cell.pos), Some(voxel));
            }
        }
    }

    pub fn lay_floor(&self, world: &mut VoxelWorld, registry: &BlockRegistry) {
        let floor = registry.id_or_default(ROOM_FLOOR_BLOCK);
        for x in 0..self.size[0] {
            for z in 0..self.size[2] {
                let cell = ROOM_ORIGIN + IVec3::new(x, -1, z);
                world.set(cell, Some(Voxel::new(floor)));
            }
        }
    }

    /// Read the room box back into a prefab.
    pub fn capture(&self, world: &VoxelWorld, registry: &BlockRegistry, structure: &mut StructureDef) -> usize {
        structure.size = self.size;
        structure.cells.clear();
        for y in 0..self.size[1] {
            for z in 0..self.size[2] {
                for x in 0..self.size[0] {
                    let Some(voxel) = world.get(ROOM_ORIGIN + IVec3::new(x, y, z)) else {
                        continue;
                    };
                    structure.cells.push(StructureCell {
                        pos: [x, y, z],
                        block: registry.get(voxel.block).id.clone(),
                        rot: voxel.orientation.0,
                        sub: voxel.sub.map(|sub| (registry.get(sub.block).id.clone(), sub.orientation.0)),
                    });
                }
            }
        }
        structure.anchors = self
            .anchors
            .iter()
            .copied()
            .filter(|a| a.iter().zip(self.size).all(|(v, s)| *v >= 0 && *v < s))
            .collect();
        structure.cells.len()
    }

    pub fn toggle_anchor(&mut self, cell: IVec3) {
        let local = (cell - ROOM_ORIGIN).to_array();
        if let Some(index) = self.anchors.iter().position(|a| *a == local) {
            self.anchors.remove(index);
        } else {
            self.anchors.push(local);
        }
    }
}

// --- Default prefabs ----------------------------------------------------------

fn tree(name: &str, trunk: i32, radius: i32, leaves: &str) -> StructureDef {
    let size = radius * 2 + 1;
    let mut s = StructureDef::new(name, [size, trunk + radius + 2, size]);
    for y in 0..trunk {
        s.put(radius, y, radius, "log");
    }
    for y in (trunk - 1)..(trunk + radius + 1) {
        for z in 0..size {
            for x in 0..size {
                let d = IVec3::new(x - radius, y - trunk, z - radius).as_vec3();
                if d.length() <= radius as f32 + 0.3 && !(x == radius && z == radius && y < trunk) {
                    s.put(x, y, z, leaves);
                }
            }
        }
    }
    s.anchors = vec![[radius, 0, radius]];
    s
}

fn pine(name: &str, height: i32) -> StructureDef {
    let mut s = StructureDef::new(name, [5, height + 2, 5]);
    for y in 0..height {
        s.put(2, y, 2, "log");
    }
    for y in 2..=height {
        let r = ((height - y) / 3).min(2);
        for z in (2 - r)..=(2 + r) {
            for x in (2 - r)..=(2 + r) {
                if (x - 2).abs() + (z - 2).abs() <= r + 1 && !(x == 2 && z == 2 && y < height) {
                    s.put(x, y, z, "pine_leaves");
                }
            }
        }
    }
    s.put(2, height + 1, 2, "pine_leaves");
    s.anchors = vec![[2, 0, 2]];
    s
}

fn ruin() -> StructureDef {
    let mut s = StructureDef::new("Ruin", [7, 4, 7]);
    for x in 0..7 {
        for z in 0..7 {
            if x == 0 || z == 0 || x == 6 || z == 6 {
                let height = 1 + ((x * 7 + z * 3) % 3);
                for y in 0..height {
                    s.put(x, y, z, if y == 0 { "temple_brick" } else { "stone" });
                }
            } else {
                s.put(x, 0, z, "slab_stone");
            }
        }
    }
    s.put(3, 1, 3, "crystal_ore");
    s.anchors = vec![[0, 0, 0], [6, 0, 0], [0, 0, 6], [6, 0, 6]];
    s
}

fn hut() -> StructureDef {
    let mut s = StructureDef::new("Hut", [5, 5, 5]);
    for x in 0..5 {
        for z in 0..5 {
            s.put(x, 0, z, "log");
            let wall = x == 0 || z == 0 || x == 4 || z == 4;
            let door = z == 0 && x == 2;
            if wall && !door {
                s.put(x, 1, z, "log");
                s.put(x, 2, z, "log");
            }
            s.put(x, 3, z, "wedge_blue");
        }
    }
    s.put(2, 4, 2, "leaves");
    s.anchors = vec![[0, 0, 0], [4, 0, 0], [0, 0, 4], [4, 0, 4]];
    s
}

fn spire() -> StructureDef {
    let mut s = StructureDef::new("Crystal Spire", [3, 9, 3]);
    for y in 0..9 {
        s.put(1, y, 1, if y > 5 { "ice" } else { "stone" });
        if y < 3 {
            s.put(0, y, 1, "stone");
            s.put(2, y, 1, "stone");
            s.put(1, y, 0, "stone");
            s.put(1, y, 2, "stone");
        }
    }
    s.put(1, 8, 1, "crystal_ore");
    s.anchors = vec![[1, 0, 1]];
    s
}

fn alien_pod() -> StructureDef {
    let mut s = StructureDef::new("Alien Pod", [5, 5, 5]);
    for y in 0..5 {
        for z in 0..5 {
            for x in 0..5 {
                let d = Vec3::new(x as f32 - 2.0, y as f32 - 1.5, z as f32 - 2.0);
                if d.length() <= 2.3 && d.length() >= 1.3 {
                    s.put(x, y, z, "alien_crust");
                }
            }
        }
    }
    s.put(2, 1, 2, "mnemo_rot");
    s.anchors = vec![[2, 0, 2]];
    s
}

/// Palm of the oases: a bent trunk with a crown of fronds.
fn palm() -> StructureDef {
    let mut s = StructureDef::new("Palm Tree", [7, 8, 7]);
    for y in 0..6 {
        s.put(3 + i32::from(y > 3), y, 3, "log");
    }
    let top = 6;
    for (dx, dz) in [(0, 0), (1, 0), (-1, 0), (0, 1), (0, -1), (2, 0), (-2, 0), (0, 2), (0, -2), (2, -1), (-1, 2)] {
        s.put(4 + dx, top, 3 + dz, "palm_leaves");
    }
    for (dx, dz) in [(3, 0), (-3, 0), (0, 3), (0, -3)] {
        s.put((4 + dx).clamp(0, 6), top - 1, (3 + dz).clamp(0, 6), "palm_leaves");
    }
    s.anchors = vec![[3, 0, 3]];
    s
}

/// Dead tree of the burnt forests.
fn burnt_tree() -> StructureDef {
    let mut s = StructureDef::new("Burnt Tree", [5, 7, 5]);
    for y in 0..6 {
        s.put(2, y, 2, "burnt_log");
    }
    for (x, y, z) in [(1, 3, 2), (0, 4, 2), (3, 4, 2), (4, 5, 2), (2, 4, 1), (2, 5, 0), (2, 3, 3)] {
        s.put(x, y, z, "burnt_log");
    }
    s.anchors = vec![[2, 0, 2]];
    s
}

/// Stepped sandstone pyramid with a dark entrance.
fn pyramid() -> StructureDef {
    let base = 13;
    let mut s = StructureDef::new("Pyramid", [base, base / 2 + 2, base]);
    for level in 0..=(base / 2) {
        for z in level..(base - level) {
            for x in level..(base - level) {
                let edge = x == level || z == level || x == base - level - 1 || z == base - level - 1;
                if edge || level == 0 {
                    s.put(x, level, z, if level == base / 2 { "temple_brick" } else { "sandstone" });
                }
            }
        }
    }
    // Entrance corridor.
    for z in 0..4 {
        for y in 1..3 {
            s.cells.retain(|cell| cell.pos != [base / 2, y, z]);
        }
    }
    s.anchors = vec![[0, 0, 0], [base - 1, 0, 0], [0, 0, base - 1], [base - 1, 0, base - 1]];
    s
}

/// Open temple: a brick platform with pillars and a lintel.
fn temple() -> StructureDef {
    let mut s = StructureDef::new("Temple", [9, 7, 11]);
    for z in 0..11 {
        for x in 0..9 {
            s.put(x, 0, z, "temple_brick");
        }
    }
    for z in [1, 4, 7, 9] {
        for x in [1, 7] {
            for y in 1..5 {
                s.put(x, y, z, "sandstone");
            }
        }
    }
    for z in 0..11 {
        for x in 0..9 {
            if x == 0 || x == 8 || z == 0 || z == 10 {
                s.put(x, 5, z, "temple_brick");
            }
        }
    }
    s.put(4, 1, 8, "temple_brick");
    s.put(4, 2, 8, "crystal_ore");
    s.anchors = vec![[0, 0, 0], [8, 0, 0], [0, 0, 10], [8, 0, 10]];
    s
}

pub fn default_structures() -> Vec<StructureDef> {
    vec![
        tree("Oak Tree", 4, 2, "leaves"),
        tree("Giant Tree", 8, 4, "leaves"),
        pine("Pine Tree", 8),
        ruin(),
        hut(),
        spire(),
        alien_pod(),
        palm(),
        burnt_tree(),
        pyramid(),
        temple(),
    ]
}

pub struct StructurePlugin;

impl Plugin for StructurePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<StructureLibrary>()
            .init_resource::<StructureRoom>()
            .add_systems(
                Startup,
                load_structures.after(crate::core::project_io::initialize_project_system),
            )
            .add_systems(Update, (apply_room_camera, draw_room));
    }
}

fn apply_room_camera(
    mut room: ResMut<StructureRoom>,
    mut cameras: Query<(&mut Transform, &mut crate::core::editor_camera::EditorCamera)>,
) {
    let Some(target) = room.camera_request.take() else {
        return;
    };
    let Ok((mut transform, mut camera)) = cameras.single_mut() else {
        return;
    };
    if room.editing.is_some() && room.saved_camera.is_none() {
        room.saved_camera = Some(*transform);
    }
    *transform = target;
    let (yaw, pitch, _) = target.rotation.to_euler(EulerRot::YXZ);
    camera.yaw = yaw;
    camera.pitch = pitch;
}

/// Bounding box, floor grid and anchors of the structure room.
fn draw_room(mut gizmos: Gizmos, room: Res<StructureRoom>) {
    if room.editing.is_none() {
        return;
    }
    let min = ROOM_ORIGIN.as_vec3();
    let size = Vec3::new(room.size[0] as f32, room.size[1] as f32, room.size[2] as f32);
    gizmos.cube(
        Transform::from_translation(min + size * 0.5).with_scale(size),
        Color::srgba(1.0, 0.75, 0.3, 0.9),
    );
    let grid = Color::srgba(0.4, 0.7, 0.8, 0.35);
    for x in 0..=room.size[0] {
        let a = min + Vec3::new(x as f32, 0.01, 0.0);
        gizmos.line(a, a + Vec3::Z * size.z, grid);
    }
    for z in 0..=room.size[2] {
        let a = min + Vec3::new(0.0, 0.01, z as f32);
        gizmos.line(a, a + Vec3::X * size.x, grid);
    }
    for anchor in &room.anchors {
        let center = min + IVec3::from_array(*anchor).as_vec3() + Vec3::splat(0.5);
        gizmos.cube(
            Transform::from_translation(center).with_scale(Vec3::splat(1.08)),
            Color::srgb(1.0, 0.25, 0.85),
        );
        gizmos.line(center, center - Vec3::Y * 0.9, Color::srgb(1.0, 0.25, 0.85));
    }
}

fn load_structures(
    mut library: ResMut<StructureLibrary>,
    project: Res<ProjectLayout>,
    mut ui: ResMut<crate::core::app_state::EditorUiState>,
) {
    match library.load_from_project(&project) {
        Ok(warnings) => {
            ui.info(format!("Structure library: {} prefab(s).", library.structures.len()));
            for warning in warnings {
                ui.warn(warning);
            }
        }
        Err(error) => ui.error(format!("Structure library failed: {error}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_round_trips_the_room() {
        let registry = BlockRegistry::builtin();
        let mut world = VoxelWorld::new();
        let mut room = StructureRoom::default();
        let tree = default_structures().remove(0);
        room.open(&mut world, &registry, &tree, 0);

        let mut captured = StructureDef::new("copy", [1, 1, 1]);
        let count = room.capture(&world, &registry, &mut captured);
        assert_eq!(count, tree.cells.len());
        assert_eq!(captured.anchors, tree.anchors);

        room.clear(&mut world, room.size);
        assert_eq!(world.stats().voxels, 0);
    }
}
