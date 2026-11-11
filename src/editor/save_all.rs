//! Save all: one action that writes every change of the studio to the project.

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy_egui::EguiContexts;

use crate::audio::synth::{bank, bank_revision};
use crate::core::app_state::{EditorState, EditorUiState};
use crate::core::game_rules::{game_rules, rules_revision};
use crate::core::pie_manager::PieState;
use crate::core::project_io::{save_ron, ProjectLayout};
use crate::core::settings::GameSettings;
use crate::editor::block_studio::BlockStudio;
use crate::editor::dock_layout::EditorLayout;
use crate::editor::structure_studio::StructureStudio;
use crate::editor::{views, world_actions};
use crate::gameplay::atmosphere::{atmosphere, atmosphere_revision};
use crate::gameplay::crafting::{book, book_revision};
use crate::gameplay::data_cables::CableNetwork;
use crate::gameplay::items::ItemLibrary;
use crate::gameplay::mobs::library::MobLibrary;
use crate::gameplay::quests::{story, story_revision};
use crate::scripting::library::ScriptLibrary;
use crate::ui::UiLibrary;
use crate::voxel::biomes::BiomeLibrary;
use crate::voxel::blocks::BlockRegistry;
use crate::voxel::structures::StructureLibrary;
use crate::voxel::world::VoxelWorld;

/// File keeping the arrangement of the panels.
pub fn layout_path(project: &ProjectLayout) -> std::path::PathBuf {
    project.root.join("ui").join("dock_layouts.ron")
}

/// Revisions of the data that keeps no saved revision itself.
#[derive(Resource, Debug, Default)]
pub struct SavedRevisions {
    ready: bool,
    blocks: u64,
    documents: [u64; 5],
}

fn documents() -> [u64; 5] {
    [rules_revision(), book_revision(), bank_revision(), atmosphere_revision(), story_revision()]
}

const DOCUMENT_NAMES: [&str; 5] = ["Rules", "Crafting", "Ambience", "Ambience", "Story"];

#[derive(SystemParam)]
pub struct Libraries<'w> {
    scripts: ResMut<'w, ScriptLibrary>,
    mobs: ResMut<'w, MobLibrary>,
    items: ResMut<'w, ItemLibrary>,
    biomes: ResMut<'w, BiomeLibrary>,
    structures: Res<'w, StructureLibrary>,
    umg: ResMut<'w, UiLibrary>,
    cables: Res<'w, CableNetwork>,
    settings: Res<'w, GameSettings>,
}

#[derive(SystemParam)]
pub struct Studio<'w> {
    world: Res<'w, VoxelWorld>,
    registry: ResMut<'w, BlockRegistry>,
    project: Res<'w, ProjectLayout>,
    editor: ResMut<'w, EditorState>,
    ui: ResMut<'w, EditorUiState>,
    pie: Res<'w, PieState>,
    layout: Res<'w, EditorLayout>,
    forge: ResMut<'w, BlockStudio>,
    structure_studio: ResMut<'w, StructureStudio>,
}

/// Names of what differs from the disk, for the Save all button.
fn unsaved(studio: &Studio, libraries: &Libraries, saved: &SavedRevisions) -> Vec<&'static str> {
    let mut areas = Vec::new();
    let mut add = |dirty: bool, name: &'static str| {
        if dirty && !areas.contains(&name) {
            areas.push(name);
        }
    };
    add(studio.world.revision != studio.ui.last_saved_revision, "World");
    add(studio.registry.revision != saved.blocks || studio.forge.dirty, "Blocks");
    add(libraries.items.is_dirty(), "Items");
    add(libraries.mobs.is_dirty(), "Mobs");
    add(studio.structure_studio.dirty, "Structures");
    add(libraries.biomes.is_dirty(), "Biomes");
    add(libraries.scripts.is_dirty(), "Scripts");
    add(libraries.umg.revision != libraries.umg.last_saved_revision, "Interface");
    for (index, revision) in documents().into_iter().enumerate() {
        add(revision != saved.documents[index], DOCUMENT_NAMES[index]);
    }
    areas
}

fn save_everything(ctx: &egui::Context, studio: &mut Studio, libraries: &mut Libraries) -> (Vec<String>, Vec<String>) {
    let (mut done, mut errors) = (Vec::new(), Vec::new());
    let project = studio.project.clone();

    // A Forge draft that was never applied is a change too.
    if studio.forge.dirty {
        let key = studio.forge.draft.id.trim().to_string();
        if key.is_empty() {
            errors.push("Forge: the block has no id".to_string());
        } else {
            studio.forge.draft.id = key;
            let draft = studio.forge.draft.clone();
            let id = studio.registry.upsert(draft);
            studio.editor.selected_block = id;
            studio.forge.source = Some(id);
            studio.forge.dirty = false;
        }
    }

    if world_actions::save_current(&studio.world, &studio.registry, &project, &mut studio.ui) {
        done.push("world".to_string());
    }
    let directory = project.module_path("blocks");
    let ids: Vec<_> = studio.registry.blocks().map(|(id, _)| id).collect();
    match ids.iter().try_for_each(|id| studio.registry.save_block(&directory, *id).map(|_| ())) {
        Ok(()) => done.push(format!("{} blocks", ids.len())),
        Err(error) => errors.push(format!("Blocks: {error}")),
    }

    let mut count = |name: &str, result: Result<usize, String>| match result {
        Ok(count) => done.push(format!("{count} {name}")),
        Err(error) => errors.push(format!("{name}: {error}")),
    };
    count("items", libraries.items.save_all(&project));
    count("mobs", libraries.mobs.save_all(&project).map(|report| report.rigs));
    count("biomes", libraries.biomes.save_all(&project));
    count("programs", libraries.scripts.save_all(&project).map(|report| report.programs));
    count("screens", libraries.umg.save_all(&project));
    let structures = (0..libraries.structures.structures.len()).try_for_each(|index| libraries.structures.save_one(&project, index).map(|_| ()));
    count("structures", structures.map(|()| libraries.structures.structures.len()));
    studio.structure_studio.dirty = false;

    let mut one = |name: &str, result: Result<(), String>| {
        if let Err(error) = result {
            errors.push(format!("{name}: {error}"));
        }
    };
    one("Rules", game_rules().save(&project));
    one("Crafting", book().save(&project));
    one("Sounds", bank().save(&project));
    one("Atmosphere", atmosphere().save(&project));
    one("Story", story().save(&project));
    one("Cables", libraries.cables.save(&project));
    one("Settings", libraries.settings.save(&project));
    one("Layout", save_ron(&layout_path(&project), &studio.layout.docks));
    one("Shader", views::shader_editor::save_open_work(ctx, &project, &mut studio.ui));
    (done, errors)
}

/// Runs the request of the button or Ctrl+S, and keeps the list of changes.
pub fn save_all_system(mut contexts: EguiContexts, mut studio: Studio, mut libraries: Libraries, mut saved: ResMut<SavedRevisions>) -> Result {
    if !saved.ready {
        saved.ready = true;
        saved.blocks = studio.registry.revision;
        saved.documents = documents();
    }
    if std::mem::take(&mut studio.ui.save_all_request) {
        if studio.pie.is_playing() {
            studio.ui.warn("Stop the game before saving: Play restores everything when it stops.");
        } else {
            let ctx = contexts.ctx_mut()?.clone();
            let (done, errors) = save_everything(&ctx, &mut studio, &mut libraries);
            saved.blocks = studio.registry.revision;
            saved.documents = documents();
            if errors.is_empty() {
                studio.ui.action(format!("Everything saved: {}.", done.join(", ")));
            } else {
                studio.ui.error(format!("Save all: {}", errors.join(" / ")));
            }
        }
    }
    let areas = unsaved(&studio, &libraries, &saved);
    studio.ui.unsaved = areas;
    Ok(())
}

/// Startup: panels come back the way they were saved.
pub fn load_layout(project: Res<ProjectLayout>, mut layout: ResMut<EditorLayout>) {
    let path = layout_path(&project);
    if path.exists() {
        if let Ok(docks) = crate::core::project_io::load_ron(&path) {
            layout.docks = docks;
        }
    }
}
