//! UMG-Lite: data-driven game interfaces (`project/ui/*.ron`).
//!
//! A layout is a tree of widgets (`CanvasPanel`, `GridSlotContainer`,
//! `ProgressBar`, `Label`, `Button`, `ItemSlot`). Every widget is positioned
//! by an anchor on its parent rectangle plus an offset and a size, so layouts
//! adapt to any window. Widgets read live values through [`Binding`]s (player
//! vitals, inventory counts, mining progress, time of day...).
//!
//! The layout named `HUD` is rendered on top of the game during PIE by
//! [`render::draw_layout`]; the designer lives in
//! [`crate::editor::views::umg_designer`].

pub mod render;

use std::fs;
use std::path::PathBuf;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::core::project_io::{load_ron, save_ron, ProjectLayout};
use crate::voxel::biomes::{file_stem, ron_files};

pub const HUD_LAYOUT: &str = "HUD";

/// Nine-point anchor on the parent rectangle.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Anchor {
    #[default]
    TopLeft,
    Top,
    TopRight,
    Left,
    Center,
    Right,
    BottomLeft,
    Bottom,
    BottomRight,
}

impl Anchor {
    pub const ALL: [Self; 9] = [
        Self::TopLeft,
        Self::Top,
        Self::TopRight,
        Self::Left,
        Self::Center,
        Self::Right,
        Self::BottomLeft,
        Self::Bottom,
        Self::BottomRight,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::TopLeft => "Top left",
            Self::Top => "Top",
            Self::TopRight => "Top right",
            Self::Left => "Left",
            Self::Center => "Center",
            Self::Right => "Right",
            Self::BottomLeft => "Bottom left",
            Self::Bottom => "Bottom",
            Self::BottomRight => "Bottom right",
        }
    }

    /// Normalised position of the anchor (0..1 on both axes).
    pub fn factor(self) -> [f32; 2] {
        match self {
            Self::TopLeft => [0.0, 0.0],
            Self::Top => [0.5, 0.0],
            Self::TopRight => [1.0, 0.0],
            Self::Left => [0.0, 0.5],
            Self::Center => [0.5, 0.5],
            Self::Right => [1.0, 0.5],
            Self::BottomLeft => [0.0, 1.0],
            Self::Bottom => [0.5, 1.0],
            Self::BottomRight => [1.0, 1.0],
        }
    }
}

/// Live value a widget can display.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum Binding {
    #[default]
    None,
    PlayerHealth,
    PlayerStamina,
    MiningProgress,
    TimeOfDay,
    /// Count of a named resource in the inventory.
    Resource(String),
    Kills,
    Deaths,
    ComboStage,
    PoweredMachines,
    Fixed(f32),
    MaxHealth,
    /// A Weave variable (prefix `g.` for the shared ones).
    Variable(String),
}

impl Binding {
    pub fn label(&self) -> String {
        match self {
            Self::None => "None".to_string(),
            Self::PlayerHealth => "Player health".to_string(),
            Self::PlayerStamina => "Player stamina".to_string(),
            Self::MiningProgress => "Mining progress".to_string(),
            Self::TimeOfDay => "Time of day".to_string(),
            Self::Resource(name) => format!("Resource '{name}'"),
            Self::Kills => "Kills".to_string(),
            Self::Deaths => "Deaths".to_string(),
            Self::ComboStage => "Combo stage".to_string(),
            Self::PoweredMachines => "Powered machines".to_string(),
            Self::Fixed(value) => format!("Fixed {value:.2}"),
            Self::MaxHealth => "Max health".to_string(),
            Self::Variable(name) => format!("Weave '{name}'"),
        }
    }

    pub fn presets() -> Vec<Binding> {
        vec![
            Self::None,
            Self::PlayerHealth,
            Self::PlayerStamina,
            Self::MiningProgress,
            Self::TimeOfDay,
            Self::Resource("crystalline".to_string()),
            Self::Kills,
            Self::Deaths,
            Self::ComboStage,
            Self::PoweredMachines,
            Self::Fixed(1.0),
            Self::MaxHealth,
            Self::Variable("score".to_string()),
        ]
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProgressStyle {
    #[default]
    Bar,
    /// Discrete pips (hearts, stamina) with this many segments.
    Pips(u32),
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum ButtonAction {
    #[default]
    None,
    StopPie,
    ToggleSurvival,
    /// Fire a logic signal to world-rule programs.
    Signal(String),
    OpenScreen(String),
    CloseScreen(String),
    ToggleScreen(String),
    /// Fires the `Interface button` Weave event with this key.
    Event(String),
    Resume,
    OpenSettings,
    OpenStudio,
    QuitToMenu,
    QuitGame,
    RespawnAtSpawn,
    RespawnWhereDied,
    /// Row of an Altars list.
    RespawnAtAltar,
}

impl ButtonAction {
    pub fn presets() -> Vec<Self> {
        vec![
            Self::None,
            Self::StopPie,
            Self::ToggleSurvival,
            Self::Signal("button".to_string()),
            Self::OpenScreen(String::new()),
            Self::CloseScreen(String::new()),
            Self::ToggleScreen(String::new()),
            Self::Event("button".to_string()),
            Self::Resume,
            Self::OpenSettings,
            Self::OpenStudio,
            Self::QuitToMenu,
            Self::QuitGame,
            Self::RespawnAtSpawn,
            Self::RespawnWhereDied,
            Self::RespawnAtAltar,
        ]
    }

    pub const fn label(&self) -> &'static str {
        match self {
            Self::None => "None",
            Self::StopPie => "Stop PIE",
            Self::ToggleSurvival => "Toggle survival",
            Self::Signal(_) => "Signal",
            Self::OpenScreen(_) => "Open screen",
            Self::CloseScreen(_) => "Close screen",
            Self::ToggleScreen(_) => "Toggle screen",
            Self::Event(_) => "Weave event",
            Self::Resume => "Resume",
            Self::OpenSettings => "Settings",
            Self::OpenStudio => "Open the studio",
            Self::QuitToMenu => "Quit to menu",
            Self::QuitGame => "Quit the game",
            Self::RespawnAtSpawn => "Respawn at spawn",
            Self::RespawnWhereDied => "Respawn where died",
            Self::RespawnAtAltar => "Respawn at altar",
        }
    }
}

/// What a list widget shows.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum ListSource {
    /// The resources of the player, with their counts.
    #[default]
    Inventory,
    /// The numbers of a Weave list.
    WeaveList(String),
    /// The Etheraltars placed in the world.
    Altars,
}

impl ListSource {
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Inventory => "Inventory",
            Self::WeaveList(_) => "Weave list",
            Self::Altars => "Etheraltars",
        }
    }
}

/// When a layout is shown.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScreenRole {
    /// Opened by a button or by Weave; the game goes on, the mouse stays
    /// captured.
    #[default]
    Overlay,
    /// Opened by a button or by Weave; frees the mouse.
    Modal,
    /// Replaces the death screen.
    Death,
    /// Replaces the pause menu.
    Pause,
}

impl ScreenRole {
    pub const ALL: [Self; 4] = [Self::Overlay, Self::Modal, Self::Death, Self::Pause];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Overlay => "Overlay",
            Self::Modal => "Modal (frees the mouse)",
            Self::Death => "Death screen",
            Self::Pause => "Pause menu",
        }
    }
}

/// Parts of the built-in HUD a project can switch off to rebuild them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HudParts {
    pub hotbar: bool,
    pub status: bool,
    pub crosshair: bool,
    pub chat: bool,
    /// Quest tracker.
    #[serde(default = "quests_on")]
    pub quests: bool,
}

fn quests_on() -> bool {
    true
}

impl Default for HudParts {
    fn default() -> Self {
        Self { hotbar: true, status: true, crosshair: true, chat: true, quests: true }
    }
}

/// Screens open during play, and the state of their widgets.
#[derive(Resource, Debug, Default)]
pub struct OpenScreens {
    pub names: std::collections::BTreeSet<String>,
    /// A modal screen is open: the mouse is free.
    pub modal: bool,
    pub widgets: render::WidgetState,
}

impl OpenScreens {
    pub fn open(&mut self, name: String) {
        self.names.insert(name);
    }

    pub fn close(&mut self, name: &str) {
        self.names.remove(name);
    }

    pub fn toggle(&mut self, name: String) {
        if !self.names.remove(&name) {
            self.names.insert(name);
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum WidgetKind {
    CanvasPanel {
        #[serde(default)]
        background: [u8; 4],
    },
    GridSlotContainer {
        columns: u32,
        slots: u32,
        slot_size: f32,
        spacing: f32,
    },
    ProgressBar {
        binding: Binding,
        max: f32,
        style: ProgressStyle,
        fill: [u8; 4],
        background: [u8; 4],
    },
    Label {
        /// Text with `{value}` replaced by the binding.
        text: String,
        binding: Binding,
        font_size: f32,
        color: [u8; 4],
        #[serde(default)]
        centered: bool,
    },
    Button {
        text: String,
        action: ButtonAction,
    },
    ItemSlot {
        /// Hotbar index (0..9).
        index: usize,
    },
    /// A glyph or emoji.
    Icon { glyph: String, color: [u8; 4] },
    /// One resource with its count.
    ResourceSlot { key: String },
    /// Rows of live data; a click runs `on_row`.
    ListView {
        source: ListSource,
        row_height: f32,
        color: [u8; 4],
        #[serde(default)]
        on_row: ButtonAction,
    },
    /// Its children are pages, named after them.
    Tabs { tab_height: f32 },
    /// Its children scroll inside it.
    ScrollPanel { content_height: f32 },
    /// Several buttons, stacked.
    ButtonBar {
        buttons: Vec<(String, ButtonAction)>,
        vertical: bool,
        spacing: f32,
    },
    Minimap,
}

impl WidgetKind {
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::CanvasPanel { .. } => "CanvasPanel",
            Self::GridSlotContainer { .. } => "GridSlotContainer",
            Self::ProgressBar { .. } => "ProgressBar",
            Self::Label { .. } => "Label",
            Self::Button { .. } => "Button",
            Self::ItemSlot { .. } => "ItemSlot",
            Self::Icon { .. } => "Icon",
            Self::ResourceSlot { .. } => "ResourceSlot",
            Self::ListView { .. } => "ListView",
            Self::Tabs { .. } => "Tabs",
            Self::ScrollPanel { .. } => "ScrollPanel",
            Self::ButtonBar { .. } => "ButtonBar",
            Self::Minimap => "Minimap",
        }
    }

    /// Default instance of every widget type, for the palette.
    pub fn palette() -> Vec<WidgetKind> {
        vec![
            Self::CanvasPanel { background: [0, 0, 0, 0] },
            Self::GridSlotContainer { columns: 9, slots: 9, slot_size: 44.0, spacing: 4.0 },
            Self::ProgressBar {
                binding: Binding::PlayerHealth,
                max: 10.0,
                style: ProgressStyle::Bar,
                fill: [220, 70, 70, 255],
                background: [0, 0, 0, 140],
            },
            Self::Label {
                text: "{value}".to_string(),
                binding: Binding::None,
                font_size: 13.0,
                color: [230, 236, 242, 255],
                centered: false,
            },
            Self::Button { text: "Button".to_string(), action: ButtonAction::None },
            Self::ItemSlot { index: 0 },
            Self::Icon { glyph: "★".to_string(), color: [255, 220, 120, 255] },
            Self::ResourceSlot { key: "voltic".to_string() },
            Self::ListView { source: ListSource::Inventory, row_height: 22.0, color: [230, 236, 242, 255], on_row: ButtonAction::None },
            Self::Tabs { tab_height: 26.0 },
            Self::ScrollPanel { content_height: 600.0 },
            Self::ButtonBar { buttons: vec![("Button".to_string(), ButtonAction::None)], vertical: true, spacing: 6.0 },
            Self::Minimap,
        ]
    }

    pub fn accepts_children(&self) -> bool {
        matches!(self, Self::CanvasPanel { .. } | Self::Tabs { .. } | Self::ScrollPanel { .. })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Widget {
    pub name: String,
    pub kind: WidgetKind,
    pub anchor: Anchor,
    pub offset: [f32; 2],
    pub size: [f32; 2],
    #[serde(default = "visible")]
    pub visible: bool,
    /// Text shown while the pointer is over the widget.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub tooltip: String,
    #[serde(default)]
    pub children: Vec<Widget>,
}

fn visible() -> bool {
    true
}

impl Widget {
    pub fn new(name: &str, kind: WidgetKind, anchor: Anchor, offset: [f32; 2], size: [f32; 2]) -> Self {
        Self {
            name: name.to_string(),
            kind,
            anchor,
            offset,
            size,
            visible: true,
            tooltip: String::new(),
            children: Vec::new(),
        }
    }

    /// Widget at a child-index path (empty = self).
    pub fn at_path(&self, path: &[usize]) -> Option<&Widget> {
        match path.split_first() {
            None => Some(self),
            Some((first, rest)) => self.children.get(*first)?.at_path(rest),
        }
    }

    pub fn at_path_mut(&mut self, path: &[usize]) -> Option<&mut Widget> {
        match path.split_first() {
            None => Some(self),
            Some((first, rest)) => self.children.get_mut(*first)?.at_path_mut(rest),
        }
    }

}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UiLayout {
    pub name: String,
    /// Reference resolution the offsets were authored at.
    pub reference: [f32; 2],
    pub root: Widget,
    #[serde(default)]
    pub role: ScreenRole,
    /// HUD layout: parts of the built-in HUD that stay.
    #[serde(default)]
    pub parts: HudParts,
}

#[derive(Resource, Debug, Default)]
pub struct UiLibrary {
    pub layouts: Vec<UiLayout>,
    pub selection: usize,
    /// Selected widget, as a child-index path from the root.
    pub widget_path: Vec<usize>,
    pub revision: u64,
    pub last_saved_revision: u64,
}

impl UiLibrary {
    pub fn dir(project: &ProjectLayout) -> PathBuf {
        project.module_path("ui")
    }

    pub fn touch(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    pub fn hud(&self) -> Option<&UiLayout> {
        self.layouts.iter().find(|layout| layout.name == HUD_LAYOUT)
    }

    pub fn find(&self, name: &str) -> Option<&UiLayout> {
        self.layouts.iter().find(|layout| layout.name == name)
    }

    /// The layout that replaces a built-in screen.
    pub fn with_role(&self, role: ScreenRole) -> Option<&UiLayout> {
        self.layouts.iter().find(|layout| layout.role == role)
    }

    pub fn load_from_project(&mut self, project: &ProjectLayout) -> Result<Vec<String>, String> {
        let dir = Self::dir(project);
        fs::create_dir_all(&dir).map_err(|error| format!("cannot create {}: {error}", dir.display()))?;
        let mut warnings = Vec::new();
        let mut layouts = Vec::new();
        for path in ron_files(&dir)? {
            // Other UI files (workspace, dock layout) share the folder.
            let Ok(text) = fs::read_to_string(&path) else {
                continue;
            };
            if !text.contains("reference") || !text.contains("root") {
                continue;
            }
            match load_ron::<UiLayout>(&path) {
                Ok(layout) => layouts.push(layout),
                Err(error) => warnings.push(error),
            }
        }
        for layout in [default_hud(), default_pause(), default_death()] {
            if !layouts.iter().any(|existing| existing.name == layout.name) {
                save_ron(&dir.join(format!("{}.ron", file_stem(&layout.name))), &layout)?;
                layouts.push(layout);
            }
        }
        layouts.sort_by(|a, b| a.name.cmp(&b.name));
        self.layouts = layouts;
        self.selection = self.selection.min(self.layouts.len().saturating_sub(1));
        self.widget_path.clear();
        self.touch();
        self.last_saved_revision = self.revision;
        Ok(warnings)
    }

    pub fn save_all(&mut self, project: &ProjectLayout) -> Result<usize, String> {
        let dir = Self::dir(project);
        for layout in &self.layouts {
            save_ron(&dir.join(format!("{}.ron", file_stem(&layout.name))), layout)?;
        }
        self.last_saved_revision = self.revision;
        Ok(self.layouts.len())
    }
}

/// The default heads-up display authored with UMG-Lite.
pub fn default_hud() -> UiLayout {
    let mut root = Widget::new(
        "Root",
        WidgetKind::CanvasPanel { background: [0, 0, 0, 0] },
        Anchor::TopLeft,
        [0.0, 0.0],
        [1280.0, 720.0],
    );
    // The built-in HUD (hotbar, vitals, crosshair, status card, chat) is drawn
    // by the game UI; this layout holds the project's own widgets and starts
    // empty (add labels / bars in the Showcase).
    root.children = Vec::new();
    UiLayout {
        name: HUD_LAYOUT.to_string(),
        reference: [1280.0, 720.0],
        root,
        role: ScreenRole::Overlay,
        parts: HudParts::default(),
    }
}

fn full_canvas(background: [u8; 4]) -> Widget {
    Widget::new("Root", WidgetKind::CanvasPanel { background }, Anchor::TopLeft, [0.0, 0.0], [1280.0, 720.0])
}

fn title(text: &str, color: [u8; 4], offset_y: f32) -> Widget {
    Widget::new(
        "Title",
        WidgetKind::Label { text: text.to_string(), binding: Binding::None, font_size: 30.0, color, centered: true },
        Anchor::Top,
        [-200.0, offset_y],
        [400.0, 44.0],
    )
}

fn bar(buttons: Vec<(&str, ButtonAction)>, offset_y: f32, height: f32) -> Widget {
    Widget::new(
        "Buttons",
        WidgetKind::ButtonBar {
            buttons: buttons.into_iter().map(|(text, action)| (text.to_string(), action)).collect(),
            vertical: true,
            spacing: 10.0,
        },
        Anchor::Top,
        [-160.0, offset_y],
        [320.0, height],
    )
}

/// The pause menu, as a layout a project can change.
pub fn default_pause() -> UiLayout {
    let mut root = full_canvas([0, 0, 0, 150]);
    root.children = vec![
        title("Paused", [255, 255, 255, 255], 170.0),
        bar(
            vec![
                ("Resume", ButtonAction::Resume),
                ("Settings", ButtonAction::OpenSettings),
                ("Open the studio", ButtonAction::OpenStudio),
                ("Quit to menu", ButtonAction::QuitToMenu),
                ("Quit the game", ButtonAction::QuitGame),
            ],
            230.0,
            260.0,
        ),
    ];
    UiLayout { name: "Pause".to_string(), reference: [1280.0, 720.0], root, role: ScreenRole::Pause, parts: HudParts::default() }
}

/// The death screen, as a layout a project can change.
pub fn default_death() -> UiLayout {
    let mut root = full_canvas([70, 8, 12, 170]);
    root.children = vec![
        title("YOU DIED", [240, 90, 90, 255], 130.0),
        Widget::new(
            "Ethereal",
            WidgetKind::Label {
                text: "Ethereal: {value}".to_string(),
                binding: Binding::Resource("ethereal".to_string()),
                font_size: 14.0,
                color: [220, 200, 200, 255],
                centered: true,
            },
            Anchor::Top,
            [-200.0, 176.0],
            [400.0, 24.0],
        ),
        bar(
            vec![
                ("Respawn at the world spawn", ButtonAction::RespawnAtSpawn),
                ("Respawn where I died ({respawn_cost} Ethereal)", ButtonAction::RespawnWhereDied),
            ],
            214.0,
            94.0,
        ),
        Widget::new(
            "Altars",
            WidgetKind::ListView { source: ListSource::Altars, row_height: 34.0, color: [235, 225, 225, 255], on_row: ButtonAction::RespawnAtAltar },
            Anchor::Top,
            [-160.0, 320.0],
            [320.0, 170.0],
        ),
    ];
    UiLayout { name: "Death".to_string(), reference: [1280.0, 720.0], root, role: ScreenRole::Death, parts: HudParts::default() }
}

pub struct UmgPlugin;

impl Plugin for UmgPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<UiLibrary>().init_resource::<OpenScreens>().add_systems(
            Startup,
            load_ui_library.after(crate::core::project_io::initialize_project_system),
        );
    }
}

fn load_ui_library(
    mut library: ResMut<UiLibrary>,
    project: Res<ProjectLayout>,
    mut ui: ResMut<crate::core::app_state::EditorUiState>,
) {
    match library.load_from_project(&project) {
        Ok(warnings) => {
            ui.info(format!("Showcase: {} layout(s).", library.layouts.len()));
            for warning in warnings {
                ui.warn(warning);
            }
        }
        Err(error) => ui.error(format!("Showcase load failed: {error}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hud_round_trips_and_paths_resolve() {
        let hud = default_hud();
        let text = ron::ser::to_string_pretty(&hud, ron::ser::PrettyConfig::default()).unwrap();
        let back: UiLayout = ron::from_str(&text).unwrap();
        assert_eq!(hud, back);
        // The default HUD starts empty: the project adds its own widgets.
        assert!(back.root.children.is_empty());
        assert_eq!(back.root.at_path(&[]).map(|w| w.name.as_str()), Some("Root"));
        assert!(back.root.at_path(&[99]).is_none());
    }

    #[test]
    fn old_layouts_still_load_and_new_widgets_round_trip() {
        let old = "(name: \"HUD\", reference: (1280.0, 720.0), root: (name: \"Root\", kind: CanvasPanel(background: (0, 0, 0, 0)), anchor: TopLeft, offset: (0.0, 0.0), size: (1280.0, 720.0), visible: true, children: []))";
        let layout: UiLayout = ron::from_str(old).expect("old file");
        assert_eq!(layout.role, ScreenRole::Overlay);
        assert!(layout.parts.hotbar);
        for layout in [default_pause(), default_death()] {
            let text = ron::ser::to_string_pretty(&layout, ron::ser::PrettyConfig::default()).unwrap();
            assert_eq!(layout, ron::from_str::<UiLayout>(&text).unwrap());
        }
    }
}
