//! Dockable studio layout, one dock per workspace.
//!
//! Each [`Workspace`] owns its own `DockState`, so switching from "🌍 World"
//! to "🐾 Mobs" swaps the whole arrangement (viewport in the middle, the
//! relevant tools around it) instead of piling every panel into one dock.

use std::collections::HashMap;

use bevy::prelude::Resource;
use egui_dock::{DockState, NodeIndex};
use serde::{Deserialize, Serialize};

use crate::core::app_state::Workspace;

/// Every panel that can be hosted by the studio dock.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EditorTab {
    Viewport,
    Hierarchy,
    Project,
    Toolbox,
    Inspector,
    Console,
    Timeline,
    Scripting,
    #[serde(alias = "Mob")]
    MobOutliner,
    MobDetails,
    Blocks,
    BlockLibrary,
    Biomes,
    BiomeList,
    Structures,
    StructureList,
    ItemList,
    ItemDetails,
    UiHierarchy,
    UiProperties,
    World,
    UiDesigner,
    Shader,
    GameRules,
    Crafting,
    Audio,
    Particles,
    Sky,
    Story,
}

impl EditorTab {
    pub const fn icon(self) -> &'static str {
        match self {
            Self::Viewport => "🎥",
            Self::Hierarchy => "🗄",
            Self::Project => "📁",
            Self::Toolbox => "🔧",
            Self::Inspector => "🔍",
            Self::Console => "📜",
            Self::Timeline => "🎬",
            Self::Scripting => "🕸",
            Self::MobOutliner => "💀",
            Self::MobDetails => "🔬",
            Self::Blocks => "🔨",
            Self::BlockLibrary => "📚",
            Self::Biomes => "🌲",
            Self::BiomeList => "🗺",
            Self::StructureList => "📑",
            Self::ItemList => "🗡",
            Self::ItemDetails => "🔩",
            Self::UiHierarchy => "🌳",
            Self::UiProperties => "📋",
            Self::Structures => "🏰",
            Self::World => "⚡",
            Self::UiDesigner => "🖼",
            Self::Shader => "🎨",
            Self::GameRules => "⚖",
            Self::Crafting => "🔥",
            Self::Audio => "🎵",
            Self::Particles => "✨",
            Self::Sky => "☁",
            Self::Story => "📖",
        }
    }

    pub const fn title(self) -> &'static str {
        match self {
            Self::Viewport => "3D View",
            Self::Hierarchy => "Scene",
            Self::Project => "Project",
            Self::Toolbox => "Tools",
            Self::Inspector => "Inspector",
            Self::Console => "Console",
            Self::Timeline => "Timeline",
            Self::Scripting => "Weave",
            Self::MobOutliner => "Skeleton",
            Self::MobDetails => "Details",
            Self::Blocks => "Block",
            Self::BlockLibrary => "Forge",
            Self::Biomes => "Biome",
            Self::BiomeList => "Terraformer",
            Self::StructureList => "Atelier",
            Self::ItemList => "Armoury",
            Self::ItemDetails => "Item",
            Self::UiHierarchy => "Hierarchy",
            Self::UiProperties => "Properties",
            Self::Structures => "Structure",
            Self::World => "Gameplay",
            Self::UiDesigner => "Showcase",
            Self::Shader => "Shader",
            Self::GameRules => "Rules",
            Self::Crafting => "Crafting",
            Self::Audio => "Sounds and music",
            Self::Particles => "Particles",
            Self::Sky => "Sky and weather",
            Self::Story => "Story",
        }
    }

    pub const fn is_viewport(self) -> bool {
        matches!(self, Self::Viewport)
    }
}

/// Resource wrapper so Bevy can own the dock states.
#[derive(Resource, Debug)]
pub struct EditorLayout {
    pub docks: HashMap<Workspace, DockState<EditorTab>>,
}

impl Default for EditorLayout {
    fn default() -> Self {
        Self {
            docks: Workspace::ALL
                .iter()
                .map(|workspace| (*workspace, default_dock_state(*workspace)))
                .collect(),
        }
    }
}

impl EditorLayout {
    pub fn dock_mut(&mut self, workspace: Workspace) -> &mut DockState<EditorTab> {
        self.docks
            .entry(workspace)
            .or_insert_with(|| default_dock_state(workspace))
    }

    pub fn reset(&mut self, workspace: Workspace) {
        self.docks.insert(workspace, default_dock_state(workspace));
    }
}

/// Viewport in the middle, `left` / `right` / `bottom` groups around it.
fn arrange(
    left: Vec<EditorTab>,
    left_split: Option<Vec<EditorTab>>,
    right: Vec<EditorTab>,
    bottom: Vec<EditorTab>,
    center: Vec<EditorTab>,
) -> DockState<EditorTab> {
    let mut state = DockState::new(center);
    let tree = state.main_surface_mut();
    let mut middle = NodeIndex::root();

    if !left.is_empty() {
        let [rest, left_node] = tree.split_left(middle, 0.19, left);
        middle = rest;
        if let Some(lower) = left_split {
            let _ = tree.split_below(left_node, 0.55, lower);
        }
    }
    if !right.is_empty() {
        let [rest, _right] = tree.split_right(middle, 0.74, right);
        middle = rest;
    }
    if !bottom.is_empty() {
        let [_rest, _bottom] = tree.split_below(middle, 0.70, bottom);
    }
    state
}

/// Default arrangement of every workspace.
pub fn default_dock_state(workspace: Workspace) -> DockState<EditorTab> {
    use EditorTab::*;
    match workspace {
        Workspace::World => arrange(
            vec![Toolbox],
            Some(vec![Hierarchy, Project, Console]),
            vec![Inspector, World],
            Vec::new(),
            vec![Viewport],
        ),
        Workspace::Blocks => arrange(vec![BlockLibrary], None, vec![Blocks], Vec::new(), vec![Viewport]),
        Workspace::Mobs => arrange(vec![MobOutliner], None, vec![MobDetails], vec![Timeline], vec![Viewport]),
        Workspace::Scripts => arrange(Vec::new(), None, vec![Console], Vec::new(), vec![Scripting]),
        Workspace::Biomes => arrange(vec![BiomeList], None, vec![Biomes], Vec::new(), vec![Viewport]),
        Workspace::Structures => arrange(vec![StructureList], None, vec![Structures], Vec::new(), vec![Viewport]),
        Workspace::Items => arrange(vec![ItemList], None, vec![ItemDetails], Vec::new(), vec![Viewport]),
        Workspace::Interface => arrange(vec![UiHierarchy], None, vec![UiProperties], Vec::new(), vec![UiDesigner]),
        Workspace::Shader => arrange(Vec::new(), None, vec![Shader], Vec::new(), vec![Viewport]),
        Workspace::Rules => arrange(Vec::new(), None, vec![Crafting], Vec::new(), vec![GameRules]),
        Workspace::Ambience => arrange(Vec::new(), None, vec![Particles, Sky], Vec::new(), vec![Audio]),
        Workspace::Story => arrange(Vec::new(), None, Vec::new(), Vec::new(), vec![Story]),
    }
}
