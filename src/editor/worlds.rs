//! Game worlds: metadata, creation, the main-menu panorama and launching.
//!
//! - A world is `project/worlds/<name>.ron` (voxels) plus `<name>.meta`
//!   (type, seed, game mode, optional single biome, sea level).
//! - The main menu shows a panorama generated live with the parameters being
//!   chosen (the editor world is kept aside and restored for the studio).
//! - *Jouer* loads or creates the world, waits for the terrain under the
//!   spawn point, then starts the game; the world is saved when you quit.

use std::fs;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::core::app_state::{EditorState, EditorUiState, Workspace};
use crate::core::editor_camera::EditorCamera;
use crate::core::pie_manager::PieState;
use crate::core::project_io::{load_ron, save_ron, ProjectLayout};
use crate::core::settings::GameSettings;
use crate::editor::menus::{MenuScreen, MenuState};
use crate::editor::world_actions::{sanitize_name, world_path};
use crate::gameplay::inventory::Inventory;
use crate::gameplay::world_rules::WorldRules;
use crate::voxel::biomes::BiomeLibrary;
use crate::voxel::blocks::BlockRegistry;
use crate::voxel::chunk_manager::WorldStreaming;
use crate::voxel::persistence;
use crate::voxel::picking::raycast;
use crate::voxel::structures::StructureLibrary;
use crate::voxel::world::{Voxel, VoxelWorld};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WorldKind {
    /// Infinite biome terrain, streamed around the player.
    #[default]
    Infinite,
    /// Flat grass plain.
    Flat,
    /// The hand-built demo island.
    Island,
}

impl WorldKind {
    pub const ALL: [Self; 3] = [Self::Infinite, Self::Flat, Self::Island];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Infinite => "Infinite (biomes)",
            Self::Flat => "Flat",
            Self::Island => "Demo island",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GameMode {
    #[default]
    Creative,
    Survival,
}

impl GameMode {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Creative => "Creative",
            Self::Survival => "Survival",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WorldMeta {
    pub name: String,
    #[serde(default)]
    pub kind: WorldKind,
    #[serde(default)]
    pub seed: u32,
    #[serde(default)]
    pub mode: GameMode,
    /// Generate a single biome instead of the whole climate map.
    #[serde(default)]
    pub biome: Option<String>,
    #[serde(default = "default_sea")]
    pub sea_level: i32,
    /// Carve caves (caverns, tunnels, ores, lava lakes).
    #[serde(default = "default_true")]
    pub caves: bool,
    /// Omnicore stored in the world's Omnicore network.
    #[serde(default)]
    pub omnicore: u32,
    /// Cables between machine ports.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cables: Vec<crate::gameplay::data_cables::Cable>,
    /// Settings of the configurable machines.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub machines: Vec<crate::gameplay::machines::config::SavedConfig>,
}

fn default_sea() -> i32 {
    6
}

fn default_true() -> bool {
    true
}

impl Default for WorldMeta {
    fn default() -> Self {
        Self {
            name: "New world".to_string(),
            kind: WorldKind::Infinite,
            seed: 1337,
            mode: GameMode::Creative,
            biome: None,
            sea_level: default_sea(),
            caves: true,
            omnicore: 0,
            machines: Vec::new(),
            cables: Vec::new(),
        }
    }
}

impl WorldMeta {
    pub fn stem(&self) -> String {
        sanitize_name(&self.name)
    }

    fn meta_path(project: &ProjectLayout, stem: &str) -> std::path::PathBuf {
        project.worlds_path().join(format!("{stem}.meta"))
    }

    pub fn save(&self, project: &ProjectLayout) -> Result<(), String> {
        save_ron(&Self::meta_path(project, &self.stem()), self)
    }

    /// Every world of the project (a `.ron` without `.meta` gets defaults).
    pub fn list(project: &ProjectLayout) -> Vec<WorldMeta> {
        let dir = project.worlds_path();
        let mut worlds = Vec::new();
        let Ok(entries) = fs::read_dir(&dir) else {
            return worlds;
        };
        let mut stems: Vec<String> = entries
            .flatten()
            .filter_map(|entry| {
                let path = entry.path();
                let ext = path.extension()?.to_string_lossy().to_lowercase();
                if ext != "ron" && ext != "meta" {
                    return None;
                }
                Some(path.file_stem()?.to_string_lossy().to_string())
            })
            .collect();
        stems.sort();
        stems.dedup();
        for stem in stems {
            let meta = load_ron::<WorldMeta>(&Self::meta_path(project, &stem)).unwrap_or_else(|_| WorldMeta {
                name: stem.clone(),
                kind: WorldKind::Island,
                ..WorldMeta::default()
            });
            worlds.push(meta);
        }
        worlds
    }

    pub fn delete(&self, project: &ProjectLayout) {
        let stem = self.stem();
        let _ = fs::remove_file(Self::meta_path(project, &stem));
        let _ = fs::remove_file(world_path(project, &stem));
    }

    /// Random-looking seed from a text (numbers are used as-is).
    pub fn seed_from_text(text: &str) -> u32 {
        text.trim().parse::<u32>().unwrap_or_else(|_| {
            text.bytes().fold(2166136261u32, |h, b| (h ^ b as u32).wrapping_mul(16777619))
        })
    }
}

/// Flat grass plain of `radius` chunks around the origin.
pub fn generate_flat(world: &mut VoxelWorld, registry: &BlockRegistry, radius: i32) {
    world.clear();
    let grass = registry.id_or_default("grass");
    let dirt = registry.id_or_default("dirt");
    let stone = registry.id_or_default("stone");
    let extent = radius * 16;
    let mut cells = Vec::with_capacity((extent * extent * 4 * 8) as usize);
    for x in -extent..extent {
        for z in -extent..extent {
            for y in -6..=3 {
                let block = match y {
                    3 => grass,
                    0..=2 => dirt,
                    _ => stone,
                };
                cells.push((IVec3::new(x, y, z), Voxel::new(block)));
            }
        }
    }
    world.set_bulk(cells);
}

/// Generate a world for `meta`; returns the surface height at the spawn.
pub fn generate(
    meta: &WorldMeta,
    world: &mut VoxelWorld,
    streaming: &mut WorldStreaming,
    biomes: &BiomeLibrary,
    structures: &StructureLibrary,
    registry: &BlockRegistry,
) -> f32 {
    match meta.kind {
        WorldKind::Infinite => {
            let library = biome_library_for(meta, biomes);
            streaming.start_new_world(world, &library, structures, registry);
            streaming
                .generator
                .as_ref()
                .map(|g| g.column(8, 8).height.max(g.settings.sea_level) as f32)
                .unwrap_or(12.0)
        }
        WorldKind::Flat => {
            streaming.enabled = false;
            generate_flat(world, registry, 5);
            4.0
        }
        WorldKind::Island => {
            streaming.enabled = false;
            crate::voxel::generator::generate_demo_world(world, registry);
            8.0
        }
    }
}

/// Biome library with the world's seed, sea level and optional single biome.
pub fn biome_library_for(meta: &WorldMeta, biomes: &BiomeLibrary) -> BiomeLibrary {
    let mut library = biomes.clone();
    library.settings.seed = meta.seed;
    library.settings.sea_level = meta.sea_level;
    library.settings.caves = meta.caves;
    if meta.caves {
        library.settings.bedrock = library.settings.bedrock.min(-48);
    }
    if let Some(name) = &meta.biome {
        let only: Vec<_> = library.biomes.iter().filter(|b| &b.name == name).cloned().collect();
        if !only.is_empty() {
            library.biomes = only;
        }
    }
    library
}

/// The world list / creation screens of the main menu.
#[derive(Resource, Debug, Default)]
pub struct WorldBrowser {
    pub worlds: Vec<WorldMeta>,
    pub selected: Option<usize>,
    pub draft: WorldMeta,
    pub seed_text: String,
    pub confirm_delete: bool,
    /// World to start (`true` = create it first).
    pub launch: Option<(WorldMeta, bool)>,
    pub launch_timer: f32,
    /// World being played (its meta stores the Omnicore network).
    pub playing: Option<WorldMeta>,
    launch_prepared: bool,
    panorama_signature: String,
    backup: Option<EditorBackup>,
}

#[derive(Debug)]
struct EditorBackup {
    world: VoxelWorld,
    streaming: bool,
    radius: i32,
    name: String,
    camera: Transform,
    clear: Color,
}

impl WorldBrowser {
    pub fn refresh(&mut self, project: &ProjectLayout) {
        self.worlds = WorldMeta::list(project);
        if self.selected.map(|s| s >= self.worlds.len()).unwrap_or(true) {
            self.selected = (!self.worlds.is_empty()).then_some(0);
        }
    }

    pub fn is_launching(&self) -> bool {
        self.launch.is_some()
    }

    /// Parameters the panorama shows for the current menu screen.
    fn panorama_meta(&self, menu: &MenuState) -> WorldMeta {
        match menu.screen {
            Some(MenuScreen::CreateWorld) => self.draft.clone(),
            Some(MenuScreen::Worlds) => self
                .selected
                .and_then(|index| self.worlds.get(index))
                .cloned()
                .unwrap_or_default(),
            _ => WorldMeta::default(),
        }
    }
}

pub struct WorldsPlugin;

impl Plugin for WorldsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<WorldBrowser>()
            // After the transition: a stopping game is saved before the panorama
            // or the studio world takes its place.
            .add_systems(Update, (panorama, launch_world, persist_omnicore).chain().after(crate::core::pie_manager::PieSet::Transition));
    }
}

/// Write the Omnicore network and the cables into the played world's meta
/// when they change.
fn persist_omnicore(
    pie: Res<PieState>,
    project: Res<ProjectLayout>,
    mut browser: ResMut<WorldBrowser>,
    mut network: ResMut<crate::gameplay::crafting::OmnicoreNetwork>,
    mut cables: ResMut<crate::gameplay::data_cables::CableNetwork>,
    mut configs: ResMut<crate::gameplay::machines::config::MachineConfigs>,
    time: Res<Time<Real>>,
    mut last_write: Local<f64>,
    mut ui: ResMut<EditorUiState>,
) {
    if !pie.is_playing() {
        if browser.launch.is_none() {
            browser.playing = None;
        }
        return;
    }
    if !network.dirty && !cables.dirty && !configs.dirty {
        return;
    }
    // At most one write every two seconds (machine panels edit continuously).
    let now = time.elapsed_secs_f64();
    if now - *last_write < 2.0 {
        return;
    }
    *last_write = now;
    network.dirty = false;
    cables.dirty = false;
    configs.dirty = false;
    if let Some(meta) = browser.playing.as_mut() {
        meta.omnicore = network.stored;
        meta.cables = cables.cables.clone();
        meta.machines = configs.saved();
        if let Err(error) = meta.save(&project) {
            ui.error(error);
        }
    }
}

/// Is the menu showing something that wants the panorama behind it?
fn wants_panorama(menu: &MenuState) -> bool {
    menu.covers_studio()
        || matches!(menu.screen, Some(MenuScreen::Worlds) | Some(MenuScreen::CreateWorld))
}

#[allow(clippy::too_many_arguments)]
fn panorama(
    pie: Res<PieState>,
    menu: Res<MenuState>,
    mut browser: ResMut<WorldBrowser>,
    mut world: ResMut<VoxelWorld>,
    mut streaming: ResMut<WorldStreaming>,
    biomes: Res<BiomeLibrary>,
    structures: Res<StructureLibrary>,
    registry: Res<BlockRegistry>,
    rules: Res<WorldRules>,
    mut ui: ResMut<EditorUiState>,
    mut clear: ResMut<ClearColor>,
    mut cameras: Query<(&mut Transform, &mut EditorCamera)>,
) {
    if pie.pending.is_some() {
        return; // A stopping game still owns the world until it is saved.
    }
    let active = wants_panorama(&menu) && !pie.is_playing() && !browser.is_launching();
    if active {
        if browser.backup.is_none() {
            let camera = cameras.single().map(|(t, _)| *t).unwrap_or_default();
            browser.backup = Some(EditorBackup {
                world: world.clone(),
                streaming: streaming.enabled,
                radius: streaming.radius,
                name: ui.world_name.clone(),
                camera,
                clear: clear.0,
            });
        }
        let meta = browser.panorama_meta(&menu);
        let signature = format!("{:?}|{}|{:?}|{}", meta.kind, meta.seed, meta.biome, meta.sea_level);
        if signature != browser.panorama_signature {
            browser.panorama_signature = signature;
            streaming.radius = 4;
            let surface = generate(&meta, &mut world, &mut streaming, &biomes, &structures, &registry);
            if let Ok((mut transform, mut camera)) = cameras.single_mut() {
                transform.translation = Vec3::new(8.0, surface + 20.0, 8.0);
                camera.pitch = -0.32;
                transform.rotation = Quat::from_euler(EulerRot::YXZ, camera.yaw, camera.pitch, 0.0);
            }
        }
        let sky = rules.biome_sky;
        clear.0 = Color::srgb(sky.x, sky.y, sky.z);
        return;
    }
    if browser.is_launching() || pie.is_playing() {
        return;
    }
    // Back to the studio: put the editor world back.
    if let Some(backup) = browser.backup.take() {
        browser.panorama_signature.clear();
        world.restore_from(&backup.world);
        streaming.radius = backup.radius;
        streaming.enabled = backup.streaming;
        if backup.streaming {
            streaming.rebuild(&biomes, &structures, &registry);
            streaming.adopt_existing(&world);
        }
        ui.world_name = backup.name;
        clear.0 = backup.clear;
        if let Ok((mut transform, mut camera)) = cameras.single_mut() {
            *transform = backup.camera;
            let (yaw, pitch, _) = backup.camera.rotation.to_euler(EulerRot::YXZ);
            camera.yaw = yaw;
            camera.pitch = pitch;
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn launch_world(
    time: Res<Time<Real>>,
    project: Res<ProjectLayout>,
    settings: Res<GameSettings>,
    mut browser: ResMut<WorldBrowser>,
    mut menu: ResMut<MenuState>,
    mut pie: ResMut<PieState>,
    mut editor: ResMut<EditorState>,
    mut ui: ResMut<EditorUiState>,
    mut world: ResMut<VoxelWorld>,
    mut streaming: ResMut<WorldStreaming>,
    mut inventory: ResMut<Inventory>,
    biomes: Res<BiomeLibrary>,
    structures: Res<StructureLibrary>,
    mut registry: ResMut<BlockRegistry>,
    mut cameras: Query<(&mut Transform, &mut EditorCamera)>,
    (mut network, mut cables, mut configs): (
        ResMut<crate::gameplay::crafting::OmnicoreNetwork>,
        ResMut<crate::gameplay::data_cables::CableNetwork>,
        ResMut<crate::gameplay::machines::config::MachineConfigs>,
    ),
) {
    let Some((meta, create)) = browser.launch.clone() else {
        return;
    };
    let stem = meta.stem();
    if !browser.launch_prepared {
        browser.launch_prepared = true;
        browser.launch_timer = 0.0;
        browser.panorama_signature.clear();
        let path = world_path(&project, &stem);
        if !create && path.is_file() {
            // Customised synthetic blocks are rebuilt from their keys.
            persistence::preload_variants(&mut registry, &path);
        }
        let loaded = !create && path.is_file() && persistence::load_from_path(&mut world, &registry, &path).is_ok();
        network.stored = meta.omnicore;
        network.dirty = false;
        cables.cables = meta.cables.clone();
        cables.pending = None;
        cables.dirty = false;
        configs.restore(&meta.machines);
        browser.playing = Some(meta.clone());
        if loaded {
            if meta.kind == WorldKind::Infinite {
                let library = biome_library_for(&meta, &biomes);
                streaming.rebuild(&library, &structures, &registry);
                streaming.adopt_existing(&world);
                streaming.enabled = true;
            } else {
                streaming.enabled = false;
            }
        } else {
            generate(&meta, &mut world, &mut streaming, &biomes, &structures, &registry);
        }
        if create {
            if let Err(error) = meta.save(&project) {
                ui.error(error);
            }
        }
        streaming.radius = settings.view_distance.clamp(2, 16) as i32;
        inventory.survival = meta.mode == GameMode::Survival;
        ui.world_name = stem.clone();
        ui.info(format!("World '{}': generating the terrain...", meta.name));
        return;
    }

    // Wait until there is ground under the spawn point.
    browser.launch_timer += time.delta_secs();
    let top = streaming.generator.as_ref().map_or(220, |generator| generator.top()) as f32;
    let spawn = Vec3::new(8.5, top, 8.5);
    let ground = raycast(&world, spawn, Vec3::NEG_Y, top + 200.0);
    if ground.is_none() && browser.launch_timer < 15.0 {
        return;
    }
    let height = ground.map(|hit| hit.voxel.y as f32 + 1.0).unwrap_or(40.0);
    if let Ok((mut transform, mut camera)) = cameras.single_mut() {
        transform.translation = Vec3::new(spawn.x, height + 2.0, spawn.z);
        camera.pitch = 0.0;
        transform.rotation = Quat::from_rotation_y(camera.yaw);
    }
    browser.launch = None;
    browser.launch_prepared = false;
    menu.screen = None;
    editor.workspace = Workspace::World;
    pie.persist_world = Some(stem);
    pie.start(&mut editor, &mut ui);
}
