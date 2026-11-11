//! Player settings and remappable key bindings.
//!
//! Saved as `project/ui/settings.ron`. Keys are stored by name (`"KeyW"`,
//! `"Space"`, `"MouseLeft"`…) so the file stays readable and editable.

use bevy::ecs::system::SystemParam;
use bevy::input::keyboard::KeyCode;
use bevy::input::mouse::MouseButton;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::core::project_io::{load_ron, save_ron, ProjectLayout};

pub const SETTINGS_FILE: &str = "settings.ron";

/// Every remappable gameplay action.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GameAction {
    Forward,
    Back,
    Left,
    Right,
    Jump,
    Sprint,
    Dash,
    Attack,
    Use,
    Interact,
    Inventory,
    ToggleView,
    Pause,
    Chat,
    Command,
    /// Customise the held synthetic block (colour, Voronoi relief).
    Customize,
    /// Throw the held entry on the ground (one unit, the stack with Sprint).
    Drop,
}

impl GameAction {
    pub const ALL: [Self; 17] = [
        Self::Forward,
        Self::Back,
        Self::Left,
        Self::Right,
        Self::Jump,
        Self::Sprint,
        Self::Dash,
        Self::Attack,
        Self::Use,
        Self::Interact,
        Self::Inventory,
        Self::ToggleView,
        Self::Pause,
        Self::Chat,
        Self::Command,
        Self::Customize,
        Self::Drop,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Forward => "Forward",
            Self::Back => "Back",
            Self::Left => "Left",
            Self::Right => "Right",
            Self::Jump => "Jump / swim up / fly",
            Self::Sprint => "Sprint / fly down",
            Self::Dash => "Dash / fast flight",
            Self::Attack => "Attack / mine",
            Self::Use => "Place / block",
            Self::Interact => "Interact",
            Self::Inventory => "Inventory",
            Self::ToggleView => "First / third person",
            Self::Pause => "Pause",
            Self::Chat => "Open chat",
            Self::Command => "Open command",
            Self::Customize => "Customize synthetic block",
            Self::Drop => "Drop the held item",
        }
    }
}

/// A physical input: a keyboard key or a mouse button.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Binding {
    Key(KeyCode),
    Mouse(MouseButton),
}

impl Binding {
    pub fn name(self) -> String {
        match self {
            Self::Key(key) => KEY_NAMES
                .iter()
                .find(|(_, code)| *code == key)
                .map(|(name, _)| (*name).to_string())
                .unwrap_or_else(|| format!("{key:?}")),
            Self::Mouse(MouseButton::Left) => "MouseLeft".to_string(),
            Self::Mouse(MouseButton::Right) => "MouseRight".to_string(),
            Self::Mouse(MouseButton::Middle) => "MouseMiddle".to_string(),
            Self::Mouse(MouseButton::Back) => "MouseBack".to_string(),
            Self::Mouse(MouseButton::Forward) => "MouseForward".to_string(),
            Self::Mouse(MouseButton::Other(index)) => format!("Mouse{index}"),
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "MouseLeft" => return Some(Self::Mouse(MouseButton::Left)),
            "MouseRight" => return Some(Self::Mouse(MouseButton::Right)),
            "MouseMiddle" => return Some(Self::Mouse(MouseButton::Middle)),
            "MouseBack" => return Some(Self::Mouse(MouseButton::Back)),
            "MouseForward" => return Some(Self::Mouse(MouseButton::Forward)),
            _ => {}
        }
        KEY_NAMES
            .iter()
            .find(|(key_name, _)| *key_name == name)
            .map(|(_, code)| Self::Key(*code))
    }

    /// Short label shown on the binding buttons.
    pub fn display(self) -> String {
        match self {
            Self::Mouse(MouseButton::Left) => "LMB".to_string(),
            Self::Mouse(MouseButton::Right) => "RMB".to_string(),
            Self::Mouse(MouseButton::Middle) => "MMB".to_string(),
            Self::Mouse(_) => self.name(),
            Self::Key(_) => {
                let name = self.name();
                name.strip_prefix("Key")
                    .or_else(|| name.strip_prefix("Digit"))
                    .map(str::to_string)
                    .unwrap_or(name)
            }
        }
    }
}

/// Keys that can be bound and saved by name.
const KEY_NAMES: &[(&str, KeyCode)] = &[
    ("KeyA", KeyCode::KeyA),
    ("KeyB", KeyCode::KeyB),
    ("KeyC", KeyCode::KeyC),
    ("KeyD", KeyCode::KeyD),
    ("KeyE", KeyCode::KeyE),
    ("KeyF", KeyCode::KeyF),
    ("KeyG", KeyCode::KeyG),
    ("KeyH", KeyCode::KeyH),
    ("KeyI", KeyCode::KeyI),
    ("KeyJ", KeyCode::KeyJ),
    ("KeyK", KeyCode::KeyK),
    ("KeyL", KeyCode::KeyL),
    ("KeyM", KeyCode::KeyM),
    ("KeyN", KeyCode::KeyN),
    ("KeyO", KeyCode::KeyO),
    ("KeyP", KeyCode::KeyP),
    ("KeyQ", KeyCode::KeyQ),
    ("KeyR", KeyCode::KeyR),
    ("KeyS", KeyCode::KeyS),
    ("KeyT", KeyCode::KeyT),
    ("KeyU", KeyCode::KeyU),
    ("KeyV", KeyCode::KeyV),
    ("KeyW", KeyCode::KeyW),
    ("KeyX", KeyCode::KeyX),
    ("KeyY", KeyCode::KeyY),
    ("KeyZ", KeyCode::KeyZ),
    ("Digit0", KeyCode::Digit0),
    ("Digit1", KeyCode::Digit1),
    ("Digit2", KeyCode::Digit2),
    ("Digit3", KeyCode::Digit3),
    ("Digit4", KeyCode::Digit4),
    ("Digit5", KeyCode::Digit5),
    ("Digit6", KeyCode::Digit6),
    ("Digit7", KeyCode::Digit7),
    ("Digit8", KeyCode::Digit8),
    ("Digit9", KeyCode::Digit9),
    ("Space", KeyCode::Space),
    ("ShiftLeft", KeyCode::ShiftLeft),
    ("ShiftRight", KeyCode::ShiftRight),
    ("ControlLeft", KeyCode::ControlLeft),
    ("ControlRight", KeyCode::ControlRight),
    ("AltLeft", KeyCode::AltLeft),
    ("AltRight", KeyCode::AltRight),
    ("Tab", KeyCode::Tab),
    ("CapsLock", KeyCode::CapsLock),
    ("Enter", KeyCode::Enter),
    ("Backspace", KeyCode::Backspace),
    ("Escape", KeyCode::Escape),
    ("ArrowUp", KeyCode::ArrowUp),
    ("ArrowDown", KeyCode::ArrowDown),
    ("ArrowLeft", KeyCode::ArrowLeft),
    ("ArrowRight", KeyCode::ArrowRight),
    ("F1", KeyCode::F1),
    ("F2", KeyCode::F2),
    ("F3", KeyCode::F3),
    ("F4", KeyCode::F4),
    ("F5", KeyCode::F5),
    ("F6", KeyCode::F6),
    ("F8", KeyCode::F8),
    ("F9", KeyCode::F9),
    ("F10", KeyCode::F10),
    ("F11", KeyCode::F11),
    ("F12", KeyCode::F12),
    ("Insert", KeyCode::Insert),
    ("Delete", KeyCode::Delete),
    ("Home", KeyCode::Home),
    ("End", KeyCode::End),
    ("PageUp", KeyCode::PageUp),
    ("PageDown", KeyCode::PageDown),
    ("Numpad0", KeyCode::Numpad0),
    ("Numpad1", KeyCode::Numpad1),
    ("Numpad2", KeyCode::Numpad2),
    ("Numpad3", KeyCode::Numpad3),
    ("Numpad4", KeyCode::Numpad4),
    ("Numpad5", KeyCode::Numpad5),
    ("Numpad6", KeyCode::Numpad6),
    ("Numpad7", KeyCode::Numpad7),
    ("Numpad8", KeyCode::Numpad8),
    ("Numpad9", KeyCode::Numpad9),
    ("Comma", KeyCode::Comma),
    ("Period", KeyCode::Period),
    ("Semicolon", KeyCode::Semicolon),
    ("Quote", KeyCode::Quote),
    ("Slash", KeyCode::Slash),
    ("Backslash", KeyCode::Backslash),
    ("BracketLeft", KeyCode::BracketLeft),
    ("BracketRight", KeyCode::BracketRight),
    ("Minus", KeyCode::Minus),
    ("Equal", KeyCode::Equal),
    ("Backquote", KeyCode::Backquote),
    ("IntlBackslash", KeyCode::IntlBackslash),
];

/// Up to two inputs per action.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ActionBinding {
    pub action: GameAction,
    pub primary: Option<Binding>,
    pub secondary: Option<Binding>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct KeyBindings {
    pub actions: Vec<ActionBinding>,
}

impl Default for KeyBindings {
    fn default() -> Self {
        use Binding::{Key, Mouse};
        let bind = |action, primary, secondary| ActionBinding {
            action,
            primary: Some(primary),
            secondary,
        };
        Self {
            actions: vec![
                // ZQSD and WASD both work out of the box.
                bind(GameAction::Forward, Key(KeyCode::KeyW), Some(Key(KeyCode::KeyZ))),
                bind(GameAction::Back, Key(KeyCode::KeyS), Some(Key(KeyCode::ArrowDown))),
                bind(GameAction::Left, Key(KeyCode::KeyA), Some(Key(KeyCode::KeyQ))),
                bind(GameAction::Right, Key(KeyCode::KeyD), Some(Key(KeyCode::ArrowRight))),
                bind(GameAction::Jump, Key(KeyCode::Space), None),
                bind(GameAction::Sprint, Key(KeyCode::ShiftLeft), Some(Key(KeyCode::ShiftRight))),
                bind(GameAction::Dash, Key(KeyCode::ControlLeft), Some(Key(KeyCode::AltLeft))),
                bind(GameAction::Attack, Mouse(MouseButton::Left), None),
                bind(GameAction::Use, Mouse(MouseButton::Right), None),
                bind(GameAction::Interact, Key(KeyCode::KeyE), None),
                bind(GameAction::Inventory, Key(KeyCode::Tab), Some(Key(KeyCode::KeyI))),
                bind(GameAction::ToggleView, Key(KeyCode::F5), None),
                bind(GameAction::Pause, Key(KeyCode::Escape), None),
                bind(GameAction::Chat, Key(KeyCode::KeyT), Some(Key(KeyCode::Enter))),
                bind(GameAction::Command, Key(KeyCode::Slash), None),
                bind(GameAction::Customize, Key(KeyCode::KeyC), None),
                bind(GameAction::Drop, Key(KeyCode::KeyG), None),
            ],
        }
    }
}

impl KeyBindings {
    pub fn get(&self, action: GameAction) -> Option<&ActionBinding> {
        self.actions.iter().find(|binding| binding.action == action)
    }

    pub fn get_mut(&mut self, action: GameAction) -> &mut ActionBinding {
        if let Some(index) = self.actions.iter().position(|binding| binding.action == action) {
            return &mut self.actions[index];
        }
        self.actions.push(ActionBinding {
            action,
            primary: None,
            secondary: None,
        });
        self.actions.last_mut().expect("just pushed")
    }

    fn inputs(&self, action: GameAction) -> impl Iterator<Item = Binding> + '_ {
        self.get(action)
            .into_iter()
            .flat_map(|binding| binding.primary.into_iter().chain(binding.secondary))
    }

    pub fn pressed(&self, action: GameAction, keys: &ButtonInput<KeyCode>, mouse: &ButtonInput<MouseButton>) -> bool {
        self.inputs(action).any(|input| match input {
            Binding::Key(key) => keys.pressed(key),
            Binding::Mouse(button) => mouse.pressed(button),
        })
    }

    pub fn just_pressed(
        &self,
        action: GameAction,
        keys: &ButtonInput<KeyCode>,
        mouse: &ButtonInput<MouseButton>,
    ) -> bool {
        self.inputs(action).any(|input| match input {
            Binding::Key(key) => keys.just_pressed(key),
            Binding::Mouse(button) => mouse.just_pressed(button),
        })
    }

    /// Display text of an action's inputs ("W / Z").
    pub fn describe(&self, action: GameAction) -> String {
        let names: Vec<String> = self.inputs(action).map(Binding::display).collect();
        if names.is_empty() { "-".to_string() } else { names.join(" / ") }
    }
}

/// Serialized form of [`KeyBindings`].
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct BindingDocument {
    action: Option<GameAction>,
    primary: Option<String>,
    secondary: Option<String>,
}

/// Player preferences.
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct GameSettings {
    pub mouse_sensitivity: f32,
    pub invert_y: bool,
    pub fov_degrees: f32,
    /// Streaming radius in chunk columns.
    pub view_distance: u32,
    pub show_fps: bool,
    pub head_bob: bool,
    /// Open the main menu when the engine starts.
    pub show_main_menu: bool,
    pub master_volume: f32,
    pub sfx_volume: f32,
    /// Colour of each box of the player model, by box name (main menu ->
    /// Customize). A box without an entry keeps the colour of the model
    /// (edited in the studio, Mobs workspace, rig "Player").
    pub part_colors: BoxColors,
    /// 3D replica of the player in the top-right HUD panel.
    pub show_player_preview: bool,
    /// Minimap in the top-right HUD panel.
    pub show_minimap: bool,
    pub bindings: KeyBindings,
    /// Bumped on every change (apply systems compare it).
    pub revision: u64,
}

impl Default for GameSettings {
    fn default() -> Self {
        Self {
            mouse_sensitivity: 1.0,
            invert_y: false,
            fov_degrees: 70.0,
            view_distance: 6,
            show_fps: true,
            head_bob: true,
            show_main_menu: true,
            master_volume: 0.8,
            sfx_volume: 0.9,
            part_colors: BoxColors::new(),
            show_player_preview: true,
            show_minimap: true,
            bindings: KeyBindings::default(),
            revision: 0,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct SettingsDocument {
    #[serde(default = "one")]
    mouse_sensitivity: f32,
    #[serde(default)]
    invert_y: bool,
    #[serde(default = "default_fov")]
    fov_degrees: f32,
    #[serde(default = "default_view")]
    view_distance: u32,
    #[serde(default = "yes")]
    show_fps: bool,
    #[serde(default = "yes")]
    head_bob: bool,
    #[serde(default = "yes")]
    show_main_menu: bool,
    #[serde(default = "default_master")]
    master_volume: f32,
    #[serde(default = "default_sfx")]
    sfx_volume: f32,
    /// Old per-part colours (head, torso, arms, legs); read once, converted.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    part_colors: Vec<Option<[u8; 3]>>,
    #[serde(default)]
    box_colors: BoxColors,
    #[serde(default = "yes")]
    show_player_preview: bool,
    #[serde(default = "yes")]
    show_minimap: bool,
    #[serde(default)]
    bindings: Vec<BindingDocument>,
}

fn one() -> f32 {
    1.0
}
fn yes() -> bool {
    true
}
fn default_fov() -> f32 {
    70.0
}
fn default_view() -> u32 {
    6
}
fn default_master() -> f32 {
    0.8
}
fn default_sfx() -> f32 {
    0.9
}
/// Custom colour of each box of the player model, by box name.
pub type BoxColors = std::collections::BTreeMap<String, [u8; 3]>;

/// Boxes coloured by each entry of the old 6-part customisation.
const LEGACY_PART_BOXES: [&[&str]; 6] = [&["Head"], &["Torso"], &["Arm L", "Hand L"], &["Arm R", "Hand R"], &["Leg L", "Foot L"], &["Leg R", "Foot R"]];

/// Named colours offered by the character customisation.
pub const PART_PALETTE: [(&str, [u8; 3]); 14] = [
    ("Yellow", [246, 226, 32]),
    ("Orange", [240, 140, 40]),
    ("Red", [230, 70, 64]),
    ("Pink", [240, 120, 180]),
    ("Purple", [170, 90, 230]),
    ("Blue", [70, 140, 240]),
    ("Cyan", [60, 200, 220]),
    ("Teal", [60, 180, 160]),
    ("Green", [70, 190, 80]),
    ("Lime", [170, 225, 70]),
    ("Brown", [130, 86, 56]),
    ("White", [238, 238, 238]),
    ("Grey", [130, 134, 142]),
    ("Black", [40, 40, 48]),
];

/// Name of a part colour ("Default" for the model colours, "Custom" otherwise).
pub fn part_color_name(color: Option<[u8; 3]>) -> &'static str {
    match color {
        None => "Default",
        Some(rgb) => PART_PALETTE.iter().find(|(_, c)| *c == rgb).map(|(name, _)| *name).unwrap_or("Custom"),
    }
}

impl GameSettings {
    pub fn touch(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    /// Radians of yaw / pitch per pixel of mouse motion.
    pub fn look_sensitivity(&self) -> f32 {
        0.0022 * self.mouse_sensitivity.clamp(0.05, 5.0)
    }

    pub fn path(project: &ProjectLayout) -> std::path::PathBuf {
        project.root.join("ui").join(SETTINGS_FILE)
    }

    pub fn save(&self, project: &ProjectLayout) -> Result<(), String> {
        let document = SettingsDocument {
            mouse_sensitivity: self.mouse_sensitivity,
            invert_y: self.invert_y,
            fov_degrees: self.fov_degrees,
            view_distance: self.view_distance,
            show_fps: self.show_fps,
            head_bob: self.head_bob,
            show_main_menu: self.show_main_menu,
            master_volume: self.master_volume,
            sfx_volume: self.sfx_volume,
            part_colors: Vec::new(),
            box_colors: self.part_colors.clone(),
            show_player_preview: self.show_player_preview,
            show_minimap: self.show_minimap,
            bindings: self
                .bindings
                .actions
                .iter()
                .map(|binding| BindingDocument {
                    action: Some(binding.action),
                    primary: binding.primary.map(Binding::name),
                    secondary: binding.secondary.map(Binding::name),
                })
                .collect(),
        };
        save_ron(&Self::path(project), &document)
    }

    pub fn load(project: &ProjectLayout) -> Result<Self, String> {
        let document: SettingsDocument = load_ron(&Self::path(project))?;
        let mut settings = Self {
            mouse_sensitivity: document.mouse_sensitivity,
            invert_y: document.invert_y,
            fov_degrees: document.fov_degrees,
            view_distance: document.view_distance,
            show_fps: document.show_fps,
            head_bob: document.head_bob,
            show_main_menu: document.show_main_menu,
            master_volume: document.master_volume,
            sfx_volume: document.sfx_volume,
            part_colors: {
                let mut colors = document.box_colors.clone();
                if colors.is_empty() {
                    for (boxes, color) in LEGACY_PART_BOXES.iter().zip(document.part_colors.iter()) {
                        if let Some(rgb) = color {
                            for name in boxes.iter() {
                                colors.insert(name.to_string(), *rgb);
                            }
                        }
                    }
                }
                colors
            },
            show_player_preview: document.show_player_preview,
            show_minimap: document.show_minimap,
            ..Self::default()
        };
        for entry in document.bindings {
            let Some(action) = entry.action else {
                continue;
            };
            let binding = settings.bindings.get_mut(action);
            binding.primary = entry.primary.as_deref().and_then(Binding::parse);
            binding.secondary = entry.secondary.as_deref().and_then(Binding::parse);
        }
        Ok(settings)
    }
}

/// Keyboard + mouse read through the player's bindings.
#[derive(SystemParam)]
pub struct GameInput<'w> {
    keys: Res<'w, ButtonInput<KeyCode>>,
    mouse: Res<'w, ButtonInput<MouseButton>>,
    settings: Res<'w, GameSettings>,
}

impl GameInput<'_> {
    pub fn pressed(&self, action: GameAction) -> bool {
        self.settings.bindings.pressed(action, &self.keys, &self.mouse)
    }

    pub fn just_pressed(&self, action: GameAction) -> bool {
        self.settings.bindings.just_pressed(action, &self.keys, &self.mouse)
    }
}

/// Action currently waiting for a new input in the settings screen.
#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct RebindRequest {
    /// `(action, slot)` with slot 0 = primary, 1 = secondary.
    pub pending: Option<(GameAction, usize)>,
}

/// Startup: read `project/ui/settings.ron` if present.
pub fn load_settings_system(project: Res<ProjectLayout>, mut settings: ResMut<GameSettings>) {
    if let Ok(loaded) = GameSettings::load(&project) {
        *settings = loaded;
        settings.touch();
    }
}

/// Capture the next key / mouse button for a pending rebind. `Escape`
/// cancels; `Backspace` clears the slot.
pub fn capture_rebind_system(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut request: ResMut<RebindRequest>,
    mut settings: ResMut<GameSettings>,
) {
    let Some((action, slot)) = request.pending else {
        return;
    };
    let captured = if keys.just_pressed(KeyCode::Escape) {
        request.pending = None;
        return;
    } else if keys.just_pressed(KeyCode::Backspace) {
        Some(None)
    } else if let Some(key) = keys.get_just_pressed().next() {
        Some(Some(Binding::Key(*key)))
    } else {
        mouse
            .get_just_pressed()
            .next()
            .map(|button| Some(Binding::Mouse(*button)))
    };
    let Some(input) = captured else {
        return;
    };
    let binding = settings.bindings.get_mut(action);
    if slot == 0 {
        binding.primary = input;
    } else {
        binding.secondary = input;
    }
    request.pending = None;
    settings.touch();
}

/// Push the audio volumes of the settings into the mixer.
pub fn apply_audio_settings_system(
    settings: Res<GameSettings>,
    mut audio: ResMut<crate::audio::AudioSettings>,
    mut last: Local<Option<u64>>,
) {
    if *last == Some(settings.revision) {
        return;
    }
    *last = Some(settings.revision);
    audio.master_volume = settings.master_volume.clamp(0.0, 1.0);
    audio.sfx_volume = settings.sfx_volume.clamp(0.0, 1.0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bindings_round_trip_by_name() {
        for binding in [
            Binding::Key(KeyCode::KeyW),
            Binding::Key(KeyCode::Space),
            Binding::Key(KeyCode::F5),
            Binding::Mouse(MouseButton::Left),
        ] {
            assert_eq!(Binding::parse(&binding.name()), Some(binding));
        }
    }

    #[test]
    fn default_bindings_cover_every_action() {
        let bindings = KeyBindings::default();
        for action in GameAction::ALL {
            assert!(bindings.get(action).and_then(|b| b.primary).is_some(), "{action:?}");
        }
    }
}
