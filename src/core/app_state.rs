//! Shared editor state.
//!
//! These resources are intentionally independent from the renderer and from the
//! project loader. Panels mutate editor state, while runtime systems read it to
//! decide whether the scene is being authored or simulated.

use bevy::prelude::*;
use egui::Color32;

use crate::voxel::blocks::BlockId;

/// The two fundamental modes of the studio.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditorMode {
    Editing,
    Playing,
}

impl EditorMode {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Editing => "EDITING",
            Self::Playing => "PLAY IN EDITOR",
        }
    }

    pub const fn is_playing(self) -> bool {
        matches!(self, Self::Playing)
    }
}

/// Task-focused studio layouts, switched from the top bar.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Workspace {
    #[default]
    World,
    Blocks,
    Mobs,
    Scripts,
    Biomes,
    Structures,
    Items,
    Interface,
    Shader,
    Rules,
    Ambience,
    Story,
}

impl Workspace {
    pub const ALL: [Self; 12] = [
        Self::World,
        Self::Blocks,
        Self::Items,
        Self::Mobs,
        Self::Scripts,
        Self::Biomes,
        Self::Structures,
        Self::Interface,
        Self::Shader,
        Self::Rules,
        Self::Ambience,
        Self::Story,
    ];

    pub const fn icon(self) -> &'static str {
        match self {
            Self::World => "🌍",
            Self::Blocks => "🔨",
            Self::Mobs => "🐾",
            Self::Scripts => "🕸",
            Self::Biomes => "🌲",
            Self::Structures => "🏰",
            Self::Items => "🗡",
            Self::Interface => "🖼",
            Self::Shader => "🎨",
            Self::Rules => "⚖",
            Self::Ambience => "🎵",
            Self::Story => "📖",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::World => "World",
            Self::Blocks => "Blocks",
            Self::Mobs => "Mobs",
            Self::Scripts => "Scripts",
            Self::Biomes => "Biomes",
            Self::Structures => "Structures",
            Self::Items => "Items",
            Self::Interface => "Interface",
            Self::Shader => "Shader",
            Self::Rules => "Rules",
            Self::Ambience => "Ambience",
            Self::Story => "Story",
        }
    }

    /// The left rail, by family.
    pub const RAIL: [&'static [Workspace]; 4] = [
        &[Self::World],
        &[Self::Blocks, Self::Items, Self::Mobs, Self::Structures, Self::Biomes],
        &[Self::Scripts, Self::Interface, Self::Shader],
        &[Self::Rules, Self::Ambience, Self::Story],
    ];

    pub const fn hint(self) -> &'static str {
        match self {
            Self::World => "Build the level block by block",
            Self::Blocks => "Forge: shape blocks, materials, facets and liquids",
            Self::Mobs => "Sculptor: model and animate creatures",
            Self::Scripts => "Weave: visual programming",
            Self::Biomes => "Terraformer: create biomes with a 3D preview",
            Self::Structures => "Workshop: build trees, ruins, houses... directly",
            Self::Items => "Armoury: create tools, weapons, food and items",
            Self::Interface => "Showcase: design the HUD and the game menus",
            Self::Shader => "Render look: toon, Voronoi, outlines",
            Self::Rules => "Game rules and crafting: every value of the game",
            Self::Ambience => "Sounds, music, particles, sky and weather, made here",
            Self::Story => "Quests, dialogues and achievements",
        }
    }

    /// Workspaces whose viewport shows the voxel world (the others show a
    /// studio preview: mob sculptor, block forge, biome terraformer).
    pub const fn shows_world(self) -> bool {
        !matches!(self, Self::Mobs | Self::Blocks | Self::Biomes | Self::Structures | Self::Items)
    }
}

/// A UI-facing description of an object in the voxel scene.
///
/// The authoritative voxel data lives in [`crate::voxel::world::VoxelWorld`].
/// This catalogue is rebuilt from the world revision so the hierarchy, the
/// inspector and the status bar always describe the current chunks.
#[derive(Clone, Debug)]
pub struct SceneEntry {
    pub name: String,
    pub kind: &'static str,
    pub color: Color32,
    /// `None` for the world root, `Some` for a chunk.
    pub coord: Option<IVec3>,
    pub voxels: usize,
}

impl SceneEntry {
    pub fn is_world(&self) -> bool {
        self.coord.is_none()
    }
}

/// Persistent editor selection and project metadata.
#[derive(Resource, Debug)]
pub struct EditorState {
    pub mode: EditorMode,
    pub workspace: Workspace,
    pub selected_scene: usize,
    pub selected_block: BlockId,
    pub project_name: String,
    pub scene_entries: Vec<SceneEntry>,
    pub scene_revision: u64,
    pub frame: u64,
}

impl Default for EditorState {
    fn default() -> Self {
        Self {
            mode: EditorMode::Editing,
            workspace: Workspace::World,
            selected_scene: 0,
            selected_block: BlockId::default(),
            project_name: "Polyhedron Sample".to_string(),
            scene_entries: Vec::new(),
            // Forces the first scene sync even before the world is generated.
            scene_revision: u64::MAX,
            frame: 0,
        }
    }
}

impl EditorState {
    pub fn selected_entry(&self) -> Option<&SceneEntry> {
        self.scene_entries.get(self.selected_scene)
    }

    pub fn selected_name(&self) -> &str {
        self.selected_entry()
            .map(|entry| entry.name.as_str())
            .unwrap_or("Nothing selected")
    }

    pub fn selected_coord(&self) -> Option<IVec3> {
        self.selected_entry().and_then(|entry| entry.coord)
    }
}

/// Severity used by the in-editor console.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogLevel {
    Info,
    Action,
    Warn,
    Error,
}

impl LogLevel {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Info => "INFO",
            Self::Action => "ACTION",
            Self::Warn => "WARN",
            Self::Error => "ERROR",
        }
    }

    pub const fn color(self) -> Color32 {
        match self {
            Self::Info => Color32::from_rgb(132, 190, 230),
            Self::Action => Color32::from_rgb(78, 210, 190),
            Self::Warn => Color32::from_rgb(240, 184, 92),
            Self::Error => Color32::from_rgb(240, 104, 104),
        }
    }
}

/// One line in the console panel.
#[derive(Clone, Debug)]
pub struct ConsoleLine {
    pub level: LogLevel,
    pub message: String,
}

/// Studio look parameters consumed by the procedural shaders.
///
/// The renderer only re-uploads the material uniforms when [`Self::revision`]
/// changes, so every panel that edits a field must call [`Self::touch`].
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct ShadingSettings {
    pub toon: bool,
    pub voronoi: bool,
    pub rim: bool,
    pub outline: bool,
    pub bands: u32,
    pub band_softness: f32,
    pub voronoi_strength: f32,
    pub voronoi_scale: f32,
    pub rim_strength: f32,
    pub outline_width: f32,
    pub rim_color: [f32; 3],
    pub voronoi_color: [f32; 3],
    pub outline_color: [f32; 3],
    /// Global switch of the per-block low-poly facet normal maps.
    pub facets: bool,
    /// Global multiplier of the facet strength.
    pub facet_strength: f32,
    /// Blocks with a wind value (leaves) sway.
    pub wind: bool,
    /// Global multiplier of the wind sway.
    pub wind_strength: f32,
    /// Liquid surfaces wave.
    pub waves: bool,
    /// Height of the liquid waves, in blocks.
    pub wave_height: f32,
    #[serde(skip)]
    pub revision: u64,
}

impl Default for ShadingSettings {
    fn default() -> Self {
        Self {
            toon: true,
            voronoi: true,
            rim: true,
            outline: true,
            bands: 4,
            band_softness: 0.35,
            voronoi_strength: 0.35,
            voronoi_scale: 0.22,
            rim_strength: 0.55,
            outline_width: 0.012,
            rim_color: [0.42, 0.86, 0.92],
            voronoi_color: [0.05, 0.10, 0.16],
            outline_color: [0.86, 0.96, 0.9],
            facets: true,
            facet_strength: 1.0,
            wind: true,
            wind_strength: 1.0,
            waves: true,
            wave_height: 0.12,
            revision: 0,
        }
    }
}

impl ShadingSettings {
    /// Wind sway applied to a block whose raw wind value is `raw`.
    pub fn effective_wind(&self, raw: f32) -> f32 {
        if self.wind {
            (raw * self.wind_strength).clamp(0.0, 3.0)
        } else {
            0.0
        }
    }

    /// Mark the uniforms as dirty without changing any value.
    pub fn touch(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    /// Short summary used by the viewport overlay and the console.
    pub fn summary(&self) -> String {
        format!(
            "toon {} ({} bands)  voronoi {}  facets {}  rim {}  outline {} ({:.3})",
            if self.toon { "ON" } else { "OFF" },
            self.bands,
            if self.voronoi { "ON" } else { "OFF" },
            if self.facets { "ON" } else { "OFF" },
            if self.rim { "ON" } else { "OFF" },
            if self.outline { "ON" } else { "OFF" },
            self.outline_width,
        )
    }
}

/// Transient UI state shared by the dock panels and the renderer.
#[derive(Resource, Debug)]
pub struct EditorUiState {
    /// Screen-space rectangle of the active 3D viewport, in egui points.
    pub viewport_rect: Option<egui::Rect>,
    pub viewport_hovered: bool,
    pub pixels_per_point: f32,
    pub show_grid: bool,
    pub show_gizmos: bool,
    pub show_stats: bool,
    pub theme_applied: bool,
    pub snap_to_grid: bool,
    pub grid_size: f32,
    pub greedy_meshing: bool,
    /// Allow two complementary half cubes to share one cell.
    pub sub_voxel_fusion: bool,
    /// Derive the placement orientation from the clicked face and camera.
    pub auto_orient: bool,
    /// Manual orientation (or roll/facing offset when `auto_orient` is on).
    pub manual_orientation: crate::voxel::shapes::Orientation,
    /// World panel: cable authoring mode (LMB connects machine ports, X removes).
    pub cable_tool: bool,
    /// Undo (`Some(true)`) or redo (`Some(false)`) asked by a button.
    pub undo_request: Option<bool>,
    /// The help window is open.
    pub help_open: bool,
    /// Save all was asked; served after the panels are drawn.
    pub save_all_request: bool,
    /// What differs from the disk.
    pub unsaved: Vec<&'static str>,
    pub hierarchy_filter: String,
    pub world_name: String,
    /// World revision captured by the last successful save or load.
    pub last_saved_revision: u64,
    pub fps: f32,
    pub shading: ShadingSettings,
    /// Console/toolbox request: re-read `project/shaders/` and rebuild pipelines.
    pub reload_shader_sources: bool,
    /// Console/toolbox request: restore the embedded shader sources first.
    pub reset_shader_sources: bool,
    pub console: Vec<ConsoleLine>,
    pub console_input: String,
    /// Total number of lines ever logged (the chat mirrors new ones).
    pub log_total: u64,
}

impl Default for EditorUiState {
    fn default() -> Self {
        Self {
            viewport_rect: None,
            viewport_hovered: false,
            pixels_per_point: 1.0,
            show_grid: true,
            show_gizmos: true,
            show_stats: true,
            theme_applied: false,
            snap_to_grid: true,
            grid_size: 1.0,
            greedy_meshing: true,
            sub_voxel_fusion: true,
            auto_orient: true,
            manual_orientation: crate::voxel::shapes::Orientation::IDENTITY,
            cable_tool: false,
            undo_request: None,
            help_open: false,
            save_all_request: false,
            unsaved: Vec::new(),
            hierarchy_filter: String::new(),
            world_name: "demo_world".to_string(),
            last_saved_revision: 0,
            fps: 60.0,
            shading: ShadingSettings::default(),
            reload_shader_sources: false,
            reset_shader_sources: false,
            console: vec![ConsoleLine {
                level: LogLevel::Info,
                message: "Polyhedron Studio ready. Press F7 to play in editor.".to_string(),
            }],
            console_input: String::new(),
            log_total: 1,
        }
    }
}

impl EditorUiState {
    pub fn log(&mut self, level: LogLevel, message: impl Into<String>) {
        self.log_total = self.log_total.wrapping_add(1);
        self.console.push(ConsoleLine {
            level,
            message: message.into(),
        });

        const MAX_LINES: usize = 512;
        if self.console.len() > MAX_LINES {
            let overflow = self.console.len() - MAX_LINES;
            self.console.drain(0..overflow);
        }
    }

    pub fn info(&mut self, message: impl Into<String>) {
        self.log(LogLevel::Info, message);
    }

    pub fn action(&mut self, message: impl Into<String>) {
        self.log(LogLevel::Action, message);
    }

    pub fn warn(&mut self, message: impl Into<String>) {
        self.log(LogLevel::Warn, message);
    }

    pub fn error(&mut self, message: impl Into<String>) {
        self.log(LogLevel::Error, message);
    }
}
