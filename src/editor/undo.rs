//! Studio undo / redo: `Ctrl+Z` reverts the last change anywhere in the
//! studio (world edits, mobs, items, blocks in the forge, biomes,
//! structures, scripts, interface layouts), `Ctrl+Y` / `Ctrl+Shift+Z` redo.
//!
//! Each editable library is watched through its revision counter: when it
//! changes (and no mouse button is held, so a whole gizmo drag is one step)
//! the previous state is pushed on one shared history. The voxel world logs
//! its single edits instead of being copied (`VoxelWorld::undo_log`): a
//! brush stroke becomes one step.

use std::any::Any;

use bevy::prelude::*;
use bevy_egui::input::EguiWantsInput;

use crate::core::app_state::{EditorMode, EditorState};
use crate::core::pie_manager::PieState;
use crate::voxel::world::{Voxel, VoxelWorld};

/// Steps kept in the history.
const MAX_STEPS: usize = 200;

/// One undoable step.
enum Step {
    /// A copy of a library before the change.
    Snapshot { domain: &'static str, data: Box<dyn Any + Send + Sync> },
    /// World cells: (cell, before, after).
    World(Vec<(IVec3, Option<Voxel>, Option<Voxel>)>),
}

impl Step {
    fn domain(&self) -> &'static str {
        match self {
            Self::Snapshot { domain, .. } => domain,
            Self::World(_) => "world",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Request {
    Undo,
    Redo,
}

/// Shared undo / redo history of the studio.
#[derive(Resource, Default)]
pub struct UndoHistory {
    undo: Vec<Step>,
    redo: Vec<Step>,
    /// Requested this frame, handled by the system owning that domain.
    request: Option<(Request, &'static str)>,
}

impl UndoHistory {
    fn push(&mut self, step: Step) {
        self.undo.push(step);
        if self.undo.len() > MAX_STEPS {
            self.undo.remove(0);
        }
        self.redo.clear();
    }
}

/// A library the history can copy and restore.
pub trait Undoable: Resource + bevy::ecs::component::Component<Mutability = bevy::ecs::component::Mutable> {
    const DOMAIN: &'static str;
    type Snapshot: Clone + Send + Sync + 'static;
    /// Changes whenever the edited data changes.
    fn version(&self) -> u64;
    fn snapshot(&self) -> Self::Snapshot;
    /// Put a copy back (and bump the revision so everything refreshes).
    fn restore(&mut self, snapshot: Self::Snapshot);
}

/// Last committed state of one library.
struct Baseline<S> {
    version: u64,
    snapshot: S,
}

pub struct UndoPlugin;

impl Plugin for UndoPlugin {
    fn build(&self, app: &mut App) {
        use crate::editor::block_studio::BlockStudio;
        use crate::gameplay::items::ItemLibrary;
        use crate::gameplay::mobs::library::MobLibrary;
        use crate::scripting::library::ScriptLibrary;
        use crate::ui::UiLibrary;
        use crate::voxel::biomes::BiomeLibrary;
        use crate::voxel::structures::StructureLibrary;
        app.init_resource::<UndoHistory>()
            .init_resource::<RulesDoc>()
            .init_resource::<CraftingDoc>()
            .init_resource::<AudioDoc>()
            .init_resource::<AtmosphereDoc>()
            .init_resource::<StoryDoc>()
            .add_systems(
            Update,
            (
                undo_shortcuts,
                track_world,
                track::<MobLibrary>,
                track::<ItemLibrary>,
                track::<BlockStudio>,
                track::<BiomeLibrary>,
                track::<StructureLibrary>,
                track::<ScriptLibrary>,
                track::<UiLibrary>,
                track::<RulesDoc>,
                track::<CraftingDoc>,
                track::<AudioDoc>,
                track::<AtmosphereDoc>,
                track::<StoryDoc>,
                clear_request,
            )
                .chain(),
        );
    }
}

fn editing(editor: &EditorState, pie: &PieState) -> bool {
    editor.mode == EditorMode::Editing && !pie.is_playing()
}

/// `Ctrl+Z` / `Ctrl+Y` / `Ctrl+Shift+Z` (text fields keep their own undo).
fn undo_shortcuts(
    keyboard: Res<ButtonInput<KeyCode>>,
    egui_input: Res<EguiWantsInput>,
    editor: Res<EditorState>,
    pie: Res<PieState>,
    mut history: ResMut<UndoHistory>,
    mut ui: ResMut<crate::core::app_state::EditorUiState>,
) {
    let button = ui.undo_request.take();
    if !editing(&editor, &pie) {
        return;
    }
    let request = match button {
        Some(true) => Request::Undo,
        Some(false) => Request::Redo,
        None => {
            if egui_input.wants_keyboard_input() {
                return;
            }
            let ctrl = keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight);
            if !ctrl {
                return;
            }
            let shift = keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight);
            // `KeyW` is where AZERTY keyboards have their Z.
            let z = keyboard.just_pressed(KeyCode::KeyZ) || keyboard.just_pressed(KeyCode::KeyW);
            let y = keyboard.just_pressed(KeyCode::KeyY);
            if (z && shift) || y {
                Request::Redo
            } else if z {
                Request::Undo
            } else {
                return;
            }
        }
    };
    let target = match request {
        Request::Undo => history.undo.last(),
        Request::Redo => history.redo.last(),
    };
    match target {
        Some(step) => {
            let domain = step.domain();
            history.request = Some((request, domain));
            ui.info(format!("{} ({domain}).", if request == Request::Undo { "Undo" } else { "Redo" }));
        }
        None => ui.info(if request == Request::Undo { "Nothing to undo." } else { "Nothing to redo." }),
    }
}

fn clear_request(mut history: ResMut<UndoHistory>) {
    history.request = None;
}

/// Watch one library: commit its changes, apply undo / redo requests.
fn track<T: Undoable>(
    mut library: ResMut<T>,
    mut history: ResMut<UndoHistory>,
    mut baseline: Local<Option<Baseline<T::Snapshot>>>,
    mouse: Res<ButtonInput<MouseButton>>,
    editor: Res<EditorState>,
    pie: Res<PieState>,
) {
    let version = library.version();
    let Some(current) = baseline.as_mut() else {
        *baseline = Some(Baseline { version, snapshot: library.snapshot() });
        return;
    };
    if !editing(&editor, &pie) {
        // Play changes nothing in the studio data; follow silently.
        if current.version != version {
            *current = Baseline { version, snapshot: library.snapshot() };
        }
        return;
    }
    // Commit a finished change (a drag is one step: wait for the release).
    let held = mouse.get_pressed().next().is_some();
    if current.version != version && !held {
        let previous = std::mem::replace(&mut current.snapshot, library.snapshot());
        current.version = version;
        history.push(Step::Snapshot { domain: T::DOMAIN, data: Box::new(previous) });
    }
    let Some((request, domain)) = history.request else {
        return;
    };
    if domain != T::DOMAIN || current.version != version {
        return;
    }
    let step = match request {
        Request::Undo => history.undo.pop(),
        Request::Redo => history.redo.pop(),
    };
    let Some(Step::Snapshot { data, .. }) = step else {
        return;
    };
    let Ok(snapshot) = data.downcast::<T::Snapshot>() else {
        return;
    };
    let now = library.snapshot();
    let back = Step::Snapshot { domain: T::DOMAIN, data: Box::new(now) };
    match request {
        Request::Undo => history.redo.push(back),
        Request::Redo => history.undo.push(back),
    }
    library.restore(*snapshot);
    *current = Baseline { version: library.version(), snapshot: library.snapshot() };
    history.request = None;
}

/// World edits: recorded by `VoxelWorld::set` while the studio edits.
fn track_world(
    mut world: ResMut<VoxelWorld>,
    mut history: ResMut<UndoHistory>,
    mouse: Res<ButtonInput<MouseButton>>,
    editor: Res<EditorState>,
    pie: Res<PieState>,
) {
    let record = editing(&editor, &pie);
    if world.recording != record {
        world.recording = record;
        if !record {
            world.undo_log.clear();
        }
    }
    if !record {
        return;
    }
    let held = mouse.get_pressed().next().is_some();
    if !world.undo_log.is_empty() && !held {
        let log = std::mem::take(&mut world.undo_log);
        history.push(Step::World(log));
    }
    let Some((request, "world")) = history.request else {
        return;
    };
    let step = match request {
        Request::Undo => history.undo.pop(),
        Request::Redo => history.redo.pop(),
    };
    let Some(Step::World(changes)) = step else {
        return;
    };
    world.recording = false;
    match request {
        Request::Undo => {
            for (cell, before, _) in changes.iter().rev() {
                world.set(*cell, *before);
            }
            history.redo.push(Step::World(changes));
        }
        Request::Redo => {
            for (cell, _, after) in &changes {
                world.set(*cell, *after);
            }
            history.undo.push(Step::World(changes));
        }
    }
    world.recording = true;
    history.request = None;
}

// --- Project-wide documents ---------------------------------------------------

macro_rules! document {
    ($name:ident, $domain:literal, $snapshot:ty, $revision:path, $get:expr, $set:path) => {
        #[derive(Resource, Default)]
        pub struct $name;

        impl Undoable for $name {
            const DOMAIN: &'static str = $domain;
            type Snapshot = $snapshot;
            fn version(&self) -> u64 {
                $revision()
            }
            fn snapshot(&self) -> Self::Snapshot {
                $get
            }
            fn restore(&mut self, snapshot: Self::Snapshot) {
                $set(snapshot);
            }
        }
    };
}

document!(RulesDoc, "rules", crate::core::game_rules::GameRules, crate::core::game_rules::rules_revision, (*crate::core::game_rules::game_rules()).clone(), crate::core::game_rules::set_rules);
document!(CraftingDoc, "crafting", crate::gameplay::crafting::CraftingBook, crate::gameplay::crafting::book_revision, crate::gameplay::crafting::book().clone(), crate::gameplay::crafting::set_book);
document!(AudioDoc, "sounds", crate::audio::synth::SoundBank, crate::audio::synth::bank_revision, (*crate::audio::synth::bank()).clone(), crate::audio::synth::set_bank);
document!(AtmosphereDoc, "ambience", crate::gameplay::atmosphere::Atmosphere, crate::gameplay::atmosphere::atmosphere_revision, (*crate::gameplay::atmosphere::atmosphere()).clone(), crate::gameplay::atmosphere::set_atmosphere);
document!(StoryDoc, "story", crate::gameplay::quests::StoryData, crate::gameplay::quests::story_revision, (*crate::gameplay::quests::story()).clone(), crate::gameplay::quests::set_story);

// --- Libraries ------------------------------------------------------------

impl Undoable for crate::gameplay::mobs::library::MobLibrary {
    const DOMAIN: &'static str = "mobs";
    type Snapshot = Vec<crate::gameplay::mobs::model::MobRig>;
    fn version(&self) -> u64 {
        self.revision
    }
    fn snapshot(&self) -> Self::Snapshot {
        self.rigs.clone()
    }
    fn restore(&mut self, snapshot: Self::Snapshot) {
        self.rigs = snapshot;
        self.clamp_selection();
        self.touch();
    }
}

impl Undoable for crate::gameplay::items::ItemLibrary {
    const DOMAIN: &'static str = "items";
    type Snapshot = Vec<crate::gameplay::items::ItemDef>;
    fn version(&self) -> u64 {
        self.revision
    }
    fn snapshot(&self) -> Self::Snapshot {
        self.items.clone()
    }
    fn restore(&mut self, snapshot: Self::Snapshot) {
        self.items = snapshot;
        self.selection = self.selection.min(self.items.len().saturating_sub(1));
        self.touch();
    }
}

impl Undoable for crate::editor::block_studio::BlockStudio {
    const DOMAIN: &'static str = "forge";
    type Snapshot = crate::voxel::blocks::BlockDef;
    fn version(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        format!("{:?}", self.draft).hash(&mut hasher);
        hasher.finish()
    }
    fn snapshot(&self) -> Self::Snapshot {
        self.draft.clone()
    }
    fn restore(&mut self, snapshot: Self::Snapshot) {
        self.draft = snapshot;
        let count = self.elements().len();
        if self.element.is_some_and(|index| index >= count) {
            self.select(None, false);
        }
        self.extra.retain(|index| *index < count);
        self.touch();
    }
}

impl Undoable for crate::voxel::biomes::BiomeLibrary {
    const DOMAIN: &'static str = "biomes";
    type Snapshot = (Vec<crate::voxel::biomes::BiomeDef>, crate::voxel::biomes::WorldGenSettings);
    fn version(&self) -> u64 {
        self.revision
    }
    fn snapshot(&self) -> Self::Snapshot {
        (self.biomes.clone(), self.settings.clone())
    }
    fn restore(&mut self, snapshot: Self::Snapshot) {
        self.biomes = snapshot.0;
        self.settings = snapshot.1;
        self.selection = self.selection.min(self.biomes.len().saturating_sub(1));
        self.touch();
    }
}

impl Undoable for crate::voxel::structures::StructureLibrary {
    const DOMAIN: &'static str = "structures";
    type Snapshot = Vec<crate::voxel::structures::StructureDef>;
    fn version(&self) -> u64 {
        self.revision
    }
    fn snapshot(&self) -> Self::Snapshot {
        self.structures.clone()
    }
    fn restore(&mut self, snapshot: Self::Snapshot) {
        self.structures = snapshot;
        self.selection = self.selection.min(self.structures.len().saturating_sub(1));
        self.revision = self.revision.wrapping_add(1);
    }
}

impl Undoable for crate::scripting::library::ScriptLibrary {
    const DOMAIN: &'static str = "scripts";
    type Snapshot = (Vec<crate::scripting::model::ScriptProgram>, Vec<crate::scripting::model::ActionDef>);
    fn version(&self) -> u64 {
        self.revision
    }
    fn snapshot(&self) -> Self::Snapshot {
        (self.programs.clone(), self.actions.clone())
    }
    fn restore(&mut self, snapshot: Self::Snapshot) {
        self.programs = snapshot.0;
        self.actions = snapshot.1;
        self.touch();
    }
}

impl Undoable for crate::ui::UiLibrary {
    const DOMAIN: &'static str = "interface";
    type Snapshot = Vec<crate::ui::UiLayout>;
    fn version(&self) -> u64 {
        self.revision
    }
    fn snapshot(&self) -> Self::Snapshot {
        self.layouts.clone()
    }
    fn restore(&mut self, snapshot: Self::Snapshot) {
        self.layouts = snapshot;
        self.selection = self.selection.min(self.layouts.len().saturating_sub(1));
        self.widget_path.clear();
        self.touch();
    }
}
