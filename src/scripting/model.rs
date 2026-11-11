//! Serializable block vocabulary shared by the editor, the VM and the project files.
//!
//! A program is split into two event handlers (`on_start` and `on_tick`) instead
//! of a flat list: the Blockly hats are structural, so they cannot be nested or
//! reordered by mistake. Every block is a plain RON value and the editor can
//! therefore round-trip a program without any engine state.

use serde::{Deserialize, Serialize};

pub use crate::scripting::context::{GameEvent, PlayerStat, Question, ScriptTarget, Sensor};

/// Comparison used by `If` and `While`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompareOp {
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
}

impl CompareOp {
    pub const ALL: [Self; 6] = [
        Self::Lt,
        Self::Le,
        Self::Gt,
        Self::Ge,
        Self::Eq,
        Self::Ne,
    ];

    pub const fn symbol(self) -> &'static str {
        match self {
            Self::Lt => "<",
            Self::Le => "<=",
            Self::Gt => ">",
            Self::Ge => ">=",
            Self::Eq => "==",
            Self::Ne => "!=",
        }
    }

    pub fn evaluate(self, left: f32, right: f32) -> bool {
        match self {
            Self::Lt => left < right,
            Self::Le => left <= right,
            Self::Gt => left > right,
            Self::Ge => left >= right,
            Self::Eq => (left - right).abs() <= f32::EPSILON,
            Self::Ne => (left - right).abs() > f32::EPSILON,
        }
    }
}

/// Arithmetic used by the `Math` block: `name = name <op> value`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MathOp {
    Add,
    Sub,
    Mul,
    Div,
    Pow,
    Mod,
}

impl MathOp {
    pub const ALL: [Self; 6] = [Self::Add, Self::Sub, Self::Mul, Self::Div, Self::Pow, Self::Mod];

    pub const fn symbol(self) -> &'static str {
        match self {
            Self::Add => "+",
            Self::Sub => "-",
            Self::Mul => "*",
            Self::Div => "/",
            Self::Pow => "^",
            Self::Mod => "%",
        }
    }

    /// Division and modulo by zero yield zero instead of NaN / infinity.
    pub fn apply(self, left: f32, right: f32) -> f32 {
        let result = match self {
            Self::Add => left + right,
            Self::Sub => left - right,
            Self::Mul => left * right,
            Self::Div => {
                if right.abs() <= f32::EPSILON {
                    0.0
                } else {
                    left / right
                }
            }
            Self::Pow => left.powf(right),
            Self::Mod => {
                if right.abs() <= f32::EPSILON {
                    0.0
                } else {
                    left.rem_euclid(right)
                }
            }
        };
        if result.is_finite() { result } else { 0.0 }
    }
}

/// A number written in the block, or the name of a variable.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Operand {
    Number(f32),
    Var(String),
}

impl Default for Operand {
    fn default() -> Self {
        Self::Number(0.0)
    }
}

impl From<f32> for Operand {
    fn from(value: f32) -> Self {
        Self::Number(value)
    }
}

impl Operand {
    /// False for a number that is not finite or an empty variable name.
    pub fn is_valid(&self) -> bool {
        match self {
            Self::Number(value) => value.is_finite(),
            Self::Var(name) => !name.trim().is_empty(),
        }
    }

    pub fn text(&self) -> String {
        match self {
            Self::Number(value) => format!("{value:.2}"),
            Self::Var(name) => name.clone(),
        }
    }
}

/// A handler for a [`GameEvent`], limited to one key when `filter` is set.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct EventHandler {
    pub event: GameEvent,
    #[serde(default)]
    pub filter: String,
    #[serde(default)]
    pub blocks: Vec<ScriptBlock>,
}

/// One visual block.
///
/// Container variants own their nested body so the RON file stays a direct
/// reflection of the block tree shown in the editor.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ScriptBlock {
    Move {
        distance: f32,
    },
    Rotate {
        degrees: f32,
    },
    Wait {
        seconds: Operand,
    },
    Emit {
        signal: String,
    },
    Log {
        message: String,
    },
    SetVar {
        name: String,
        value: Operand,
    },
    AddVar {
        name: String,
        value: Operand,
    },
    AddEnergy {
        amount: f32,
    },
    Repeat {
        times: u32,
        body: Vec<ScriptBlock>,
    },
    If {
        variable: String,
        op: CompareOp,
        value: Operand,
        then_body: Vec<ScriptBlock>,
        else_body: Vec<ScriptBlock>,
    },
    While {
        variable: String,
        op: CompareOp,
        value: Operand,
        max_iterations: u32,
        body: Vec<ScriptBlock>,
    },
    CallAction {
        action: String,
        /// Variables set right before the call: the action's parameters.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        args: Vec<(String, Operand)>,
    },
    // --- Data -------------------------------------------------------------
    Math {
        name: String,
        op: MathOp,
        value: Operand,
    },
    /// `name = left <op> right`.
    Compute {
        name: String,
        left: Operand,
        op: MathOp,
        right: Operand,
    },
    /// `name` gets a random number between `min` and `max`.
    RandomRange {
        name: String,
        min: Operand,
        max: Operand,
    },
    /// Ask the game something; `name` gets the answer.
    Ask {
        name: String,
        question: Question,
    },
    // --- Lists ------------------------------------------------------------
    ListPush {
        list: String,
        value: Operand,
    },
    ListGet {
        name: String,
        list: String,
        index: Operand,
    },
    ListSet {
        list: String,
        index: Operand,
        value: Operand,
    },
    ListLength {
        name: String,
        list: String,
    },
    ListClear {
        list: String,
    },
    Sense {
        name: String,
        sensor: Sensor,
    },
    // --- Voxel world ------------------------------------------------------
    SetBlock {
        offset: [i32; 3],
        block: String,
        #[serde(default)]
        rot: u8,
    },
    BreakBlock {
        offset: [i32; 3],
        drop: bool,
    },
    SpawnParticle {
        kind: String,
        count: u32,
    },
    DropResource {
        resource: String,
        amount: u32,
    },
    PlaySound {
        sound: String,
    },
    SetTimeOfDay {
        value: f32,
    },
    SpawnMob {
        mob: String,
    },
    // --- Entities & movement ----------------------------------------------
    MoveToward {
        target: ScriptTarget,
        speed: f32,
    },
    ApplyImpulse {
        impulse: [f32; 3],
    },
    PlayAction {
        action: String,
        blend: f32,
    },
    SetHealth {
        amount: Operand,
    },
    /// Fire a beam at the player when in range.
    Shoot {
        damage: Operand,
        range: Operand,
    },
    /// Walk to a world position.
    MoveTo {
        x: Operand,
        y: Operand,
        z: Operand,
        speed: f32,
    },
    Attack {
        damage: f32,
        range: f32,
    },
    TameEntity,
    // --- Logic signals ----------------------------------------------------
    SetSignal {
        port: String,
        value: Operand,
    },
    /// Start a weather (empty = clear sky).
    SetWeather {
        weather: String,
    },
    /// Start a quest of the Story workspace.
    StartQuest {
        quest: String,
    },
    /// Open a dialogue of the Story workspace.
    ShowDialogue {
        dialogue: String,
    },
    /// Send a message to every program (`Message received` event).
    Broadcast {
        message: String,
        value: Operand,
    },
    // --- Player, items and interface --------------------------------------
    GiveItem {
        key: String,
        amount: Operand,
    },
    /// Takes the items when the player has them; `result` gets 1, else 0.
    TakeItem {
        key: String,
        amount: Operand,
        result: String,
    },
    /// Set a stat of the player, or add to it.
    SetPlayerStat {
        stat: PlayerStat,
        value: Operand,
        add: bool,
    },
    DamagePlayer {
        amount: Operand,
    },
    TeleportPlayer {
        x: Operand,
        y: Operand,
        z: Operand,
    },
    /// Chat message. `{name}` is replaced by a variable.
    ShowMessage {
        text: String,
    },
    OpenScreen {
        screen: String,
    },
    CloseScreen {
        screen: String,
    },
    /// Keep a value across runs (`Ask` / Saved value reads it back).
    SaveValue {
        key: String,
        value: Operand,
    },
    /// Pause the VM here (debugger).
    Breakpoint,
}

impl ScriptBlock {
    /// One sentence for the block's tooltip.
    pub const fn help(&self) -> &'static str {
        match self {
            Self::Move { .. } => "Moves the debug avatar forward.",
            Self::Rotate { .. } => "Turns the debug avatar.",
            Self::Wait { .. } => "Pauses this handler for some seconds.",
            Self::Emit { .. } => "Records a named signal in the log.",
            Self::Log { .. } => "Writes a line in the console.",
            Self::SetVar { .. } => "Sets a variable. Prefix with g. to share it between programs.",
            Self::AddVar { .. } => "Adds to a variable.",
            Self::AddEnergy { .. } => "Adds to the energy of the VM.",
            Self::Repeat { .. } => "Runs the body several times.",
            Self::If { .. } => "Runs one branch or the other.",
            Self::While { .. } => "Runs the body as long as the condition holds.",
            Self::CallAction { .. } => "Runs a reusable action, setting its parameters first.",
            Self::Math { .. } => "Applies an operation to a variable.",
            Self::Compute { .. } => "Stores the result of an operation between two values.",
            Self::RandomRange { .. } => "Stores a random number.",
            Self::Sense { .. } => "Reads a value of the game at once.",
            Self::Ask { .. } => "Asks the game something. The answer arrives on the next tick; the handler waits for it.",
            Self::ListPush { .. } => "Adds a number at the end of a list.",
            Self::ListGet { .. } => "Reads a number of a list (0 is the first).",
            Self::ListSet { .. } => "Changes a number of a list.",
            Self::ListLength { .. } => "Stores how many numbers a list holds.",
            Self::ListClear { .. } => "Empties a list.",
            Self::SetBlock { .. } => "Places a block, relative to the owner.",
            Self::BreakBlock { .. } => "Breaks a block, relative to the owner.",
            Self::SpawnParticle { .. } => "Spawns an effect at the owner.",
            Self::DropResource { .. } => "Gives a resource to the player.",
            Self::PlaySound { .. } => "Plays a sound.",
            Self::SetTimeOfDay { .. } => "Sets the time of day (0 to 1).",
            Self::SpawnMob { .. } => "Spawns a mob above the owner.",
            Self::MoveToward { .. } => "Walks toward a target this tick.",
            Self::MoveTo { .. } => "Walks toward a world position this tick.",
            Self::Shoot { .. } => "Fires a beam at the player when in range, from a mob or a block.",
            Self::ApplyImpulse { .. } => "Pushes the owner.",
            Self::PlayAction { .. } => "Plays an animation clip.",
            Self::SetHealth { .. } => "Sets the health of the owner.",
            Self::Attack { .. } => "Strikes the player when in range.",
            Self::TameEntity => "The owner stops being hostile and follows the player.",
            Self::SetSignal { .. } => "Writes on an output port.",
            Self::SetWeather { .. } => "Starts a weather made in the Ambience workspace.",
            Self::StartQuest { .. } => "Starts a quest of the Story workspace, unless it was already done.",
            Self::ShowDialogue { .. } => "Opens a dialogue of the Story workspace and frees the mouse.",
            Self::Broadcast { .. } => "Reaches every program listening to Message received.",
            Self::GiveItem { .. } => "Adds items or resources to the player.",
            Self::TakeItem { .. } => "Removes items from the player when they have enough.",
            Self::SetPlayerStat { .. } => "Changes a stat of the player.",
            Self::DamagePlayer { .. } => "Hurts the player, armour applies.",
            Self::TeleportPlayer { .. } => "Moves the player to a world position.",
            Self::ShowMessage { .. } => "Shows a chat message. {name} is replaced by a variable.",
            Self::OpenScreen { .. } => "Opens a Showcase layout.",
            Self::CloseScreen { .. } => "Closes a Showcase layout.",
            Self::SaveValue { .. } => "Keeps a value in project/saved_values.ron, across runs.",
            Self::Breakpoint => "Pauses the whole VM. Resume or step from the Weave panel.",
        }
    }

    pub const fn kind_label(&self) -> &'static str {
        match self {
            Self::Move { .. } => "Move",
            Self::Rotate { .. } => "Rotate",
            Self::Wait { .. } => "Wait",
            Self::Emit { .. } => "Emit",
            Self::Log { .. } => "Log",
            Self::SetVar { .. } => "Set var",
            Self::AddVar { .. } => "Add var",
            Self::AddEnergy { .. } => "Energy",
            Self::Repeat { .. } => "Repeat",
            Self::If { .. } => "If",
            Self::While { .. } => "While",
            Self::CallAction { .. } => "Call action",
            Self::Math { .. } => "Math",
            Self::Compute { .. } => "Compute",
            Self::RandomRange { .. } => "Random",
            Self::Ask { .. } => "Ask",
            Self::ListPush { .. } => "List add",
            Self::ListGet { .. } => "List get",
            Self::ListSet { .. } => "List set",
            Self::ListLength { .. } => "List length",
            Self::ListClear { .. } => "List clear",
            Self::Sense { .. } => "Sense",
            Self::SetBlock { .. } => "Set block",
            Self::BreakBlock { .. } => "Break block",
            Self::SpawnParticle { .. } => "Particles",
            Self::DropResource { .. } => "Drop",
            Self::PlaySound { .. } => "Sound",
            Self::SetTimeOfDay { .. } => "Time of day",
            Self::SpawnMob { .. } => "Spawn mob",
            Self::MoveToward { .. } => "Move toward",
            Self::ApplyImpulse { .. } => "Impulse",
            Self::PlayAction { .. } => "Play action",
            Self::SetHealth { .. } => "Set health",
            Self::Attack { .. } => "Attack",
            Self::TameEntity => "Tame",
            Self::SetSignal { .. } => "Set signal",
            Self::MoveTo { .. } => "Move to",
            Self::Shoot { .. } => "Shoot",
            Self::Broadcast { .. } => "Broadcast",
            Self::SetWeather { .. } => "Set weather",
            Self::StartQuest { .. } => "Start quest",
            Self::ShowDialogue { .. } => "Show dialogue",
            Self::GiveItem { .. } => "Give item",
            Self::TakeItem { .. } => "Take item",
            Self::SetPlayerStat { .. } => "Player stat",
            Self::DamagePlayer { .. } => "Hurt player",
            Self::TeleportPlayer { .. } => "Teleport player",
            Self::ShowMessage { .. } => "Message",
            Self::OpenScreen { .. } => "Open screen",
            Self::CloseScreen { .. } => "Close screen",
            Self::SaveValue { .. } => "Save value",
            Self::Breakpoint => "Breakpoint",
        }
    }

    /// Palette category, used for the card colour in the editor.
    pub const fn category(&self) -> BlockCategory {
        match self {
            Self::Wait { .. }
            | Self::Repeat { .. }
            | Self::If { .. }
            | Self::While { .. }
            | Self::CallAction { .. }
            | Self::Breakpoint => BlockCategory::Flow,
            Self::SetVar { .. }
            | Self::AddVar { .. }
            | Self::Math { .. }
            | Self::Sense { .. }
            | Self::Compute { .. }
            | Self::RandomRange { .. }
            | Self::Ask { .. }
            | Self::ListPush { .. }
            | Self::ListGet { .. }
            | Self::ListSet { .. }
            | Self::ListLength { .. }
            | Self::ListClear { .. }
            | Self::SaveValue { .. }
            | Self::Log { .. } => BlockCategory::Data,
            Self::SetBlock { .. }
            | Self::BreakBlock { .. }
            | Self::SpawnParticle { .. }
            | Self::DropResource { .. }
            | Self::PlaySound { .. }
            | Self::SetWeather { .. }
            | Self::StartQuest { .. }
            | Self::ShowDialogue { .. }
            | Self::SetTimeOfDay { .. }
            | Self::SpawnMob { .. } => BlockCategory::World,
            Self::Move { .. }
            | Self::Rotate { .. }
            | Self::MoveToward { .. }
            | Self::ApplyImpulse { .. }
            | Self::PlayAction { .. }
            | Self::SetHealth { .. }
            | Self::Attack { .. }
            | Self::MoveTo { .. }
            | Self::Shoot { .. }
            | Self::TameEntity => BlockCategory::Entity,
            Self::Emit { .. } | Self::AddEnergy { .. } | Self::SetSignal { .. } | Self::Broadcast { .. } => {
                BlockCategory::Signal
            }
            Self::GiveItem { .. }
            | Self::TakeItem { .. }
            | Self::SetPlayerStat { .. }
            | Self::DamagePlayer { .. }
            | Self::TeleportPlayer { .. }
            | Self::ShowMessage { .. }
            | Self::OpenScreen { .. }
            | Self::CloseScreen { .. } => BlockCategory::Player,
        }
    }

    /// Short one-line description used by the block card header.
    pub fn summary(&self) -> String {
        match self {
            Self::Move { distance } => format!("{distance:.2} m"),
            Self::Rotate { degrees } => format!("{degrees:.1} deg"),
            Self::Wait { seconds } => format!("{} s", seconds.text()),
            Self::Emit { signal } => format!("'{signal}'"),
            Self::Log { message } => truncate(message, 34),
            Self::SetVar { name, value } => format!("{name} = {}", value.text()),
            Self::AddVar { name, value } => format!("{name} += {}", value.text()),
            Self::AddEnergy { amount } => format!("{amount:+.1}"),
            Self::Repeat { times, .. } => format!("x {times}"),
            Self::If {
                variable,
                op,
                value,
                ..
            } => format!("{variable} {} {}", op.symbol(), value.text()),
            Self::While {
                variable,
                op,
                value,
                max_iterations,
                ..
            } => format!("{variable} {} {} (max {max_iterations})", op.symbol(), value.text()),
            Self::CallAction { action, .. } => format!("'{action}'"),
            Self::Math { name, op, value } => format!("{name} = {name} {} {}", op.symbol(), value.text()),
            Self::Compute { name, left, op, right } => format!("{name} = {} {} {}", left.text(), op.symbol(), right.text()),
            Self::RandomRange { name, min, max } => format!("{name} in {} .. {}", min.text(), max.text()),
            Self::Ask { name, question } => format!("{name} <- {}", question.summary()),
            Self::ListPush { list, value } => format!("{list} + {}", value.text()),
            Self::ListGet { name, list, index } => format!("{name} = {list}[{}]", index.text()),
            Self::ListSet { list, index, value } => format!("{list}[{}] = {}", index.text(), value.text()),
            Self::ListLength { name, list } => format!("{name} = length of {list}"),
            Self::ListClear { list } => list.clone(),
            Self::Sense { name, sensor } => format!("{name} <- {}", sensor.label()),
            Self::SetBlock { offset, block, rot } => format!(
                "{block} at ({}, {}, {}) r{rot}",
                offset[0], offset[1], offset[2]
            ),
            Self::BreakBlock { offset, drop } => format!(
                "({}, {}, {}){}",
                offset[0],
                offset[1],
                offset[2],
                if *drop { " + drop" } else { "" }
            ),
            Self::SpawnParticle { kind, count } => format!("{count} x {kind}"),
            Self::DropResource { resource, amount } => format!("{amount} x {resource}"),
            Self::PlaySound { sound } => format!("'{sound}'"),
            Self::SetTimeOfDay { value } => format!("{value:.2}"),
            Self::SpawnMob { mob } => format!("'{mob}'"),
            Self::MoveToward { target, speed } => format!("{} @ {speed:.1} m/s", target.label()),
            Self::ApplyImpulse { impulse } => {
                format!("({:.1}, {:.1}, {:.1})", impulse[0], impulse[1], impulse[2])
            }
            Self::PlayAction { action, blend } => format!("'{action}' blend {blend:.2}s"),
            Self::SetHealth { amount } => amount.text(),
            Self::Attack { damage, range } => format!("{damage:.1} dmg, {range:.1} m"),
            Self::TameEntity => "by player".to_string(),
            Self::SetSignal { port, value } => format!("{port} = {}", value.text()),
            Self::Shoot { damage, range } => format!("{} dmg, {} m", damage.text(), range.text()),
            Self::MoveTo { x, y, z, speed } => format!("({}, {}, {}) @ {speed:.1} m/s", x.text(), y.text(), z.text()),
            Self::StartQuest { quest } => format!("'{quest}'"),
            Self::ShowDialogue { dialogue } => format!("'{dialogue}'"),
            Self::SetWeather { weather } => if weather.is_empty() { "clear sky".to_string() } else { format!("'{weather}'") },
            Self::Broadcast { message, value } => format!("'{message}' = {}", value.text()),
            Self::GiveItem { key, amount } => format!("{} x {key}", amount.text()),
            Self::TakeItem { key, amount, result } => format!("{} x {key} -> {result}", amount.text()),
            Self::SetPlayerStat { stat, value, add } => format!("{} {} {}", stat.label(), if *add { "+=" } else { "=" }, value.text()),
            Self::DamagePlayer { amount } => amount.text(),
            Self::TeleportPlayer { x, y, z } => format!("({}, {}, {})", x.text(), y.text(), z.text()),
            Self::ShowMessage { text } => truncate(text, 34),
            Self::OpenScreen { screen } | Self::CloseScreen { screen } => format!("'{screen}'"),
            Self::SaveValue { key, value } => format!("{key} = {}", value.text()),
            Self::Breakpoint => "pause the VM".to_string(),
        }
    }

    pub fn move_default() -> Self {
        Self::Move { distance: 1.0 }
    }

    pub fn rotate_default() -> Self {
        Self::Rotate { degrees: 90.0 }
    }

    pub fn wait_default() -> Self {
        Self::Wait { seconds: 0.5.into() }
    }

    pub fn emit_default() -> Self {
        Self::Emit {
            signal: "signal".to_string(),
        }
    }

    pub fn log_default() -> Self {
        Self::Log {
            message: "Hello from Weave".to_string(),
        }
    }

    pub fn set_var_default() -> Self {
        Self::SetVar {
            name: "counter".to_string(),
            value: 0.0.into(),
        }
    }

    pub fn add_var_default() -> Self {
        Self::AddVar {
            name: "counter".to_string(),
            value: 1.0.into(),
        }
    }

    pub fn add_energy_default() -> Self {
        Self::AddEnergy { amount: 10.0 }
    }

    pub fn repeat_default() -> Self {
        Self::Repeat {
            times: 3,
            body: Vec::new(),
        }
    }

    pub fn if_default() -> Self {
        Self::If {
            variable: "counter".to_string(),
            op: CompareOp::Ge,
            value: 1.0.into(),
            then_body: Vec::new(),
            else_body: Vec::new(),
        }
    }

    pub fn while_default() -> Self {
        Self::While {
            variable: "counter".to_string(),
            op: CompareOp::Lt,
            value: 10.0.into(),
            max_iterations: 100,
            body: Vec::new(),
        }
    }

    #[cfg(test)]
    pub fn call_action_default(action: impl Into<String>) -> Self {
        Self::CallAction {
            action: action.into(),
            args: Vec::new(),
        }
    }
}

/// Palette families shown in the toolbox.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockCategory {
    Flow,
    Data,
    World,
    Entity,
    Signal,
    Player,
}

impl ScriptBlock {
    /// Default instance of every palette entry, grouped by category.
    pub fn palette() -> Vec<(BlockCategory, ScriptBlock)> {
        use BlockCategory::*;
        vec![
            (Flow, Self::wait_default()),
            (Flow, Self::repeat_default()),
            (Flow, Self::if_default()),
            (Flow, Self::while_default()),
            (Data, Self::set_var_default()),
            (Data, Self::add_var_default()),
            (Data, Self::Math { name: "counter".into(), op: MathOp::Mul, value: 2.0.into() }),
            (Data, Self::Compute { name: "result".into(), left: Operand::Var("a".into()), op: MathOp::Add, right: Operand::Var("b".into()) }),
            (Data, Self::RandomRange { name: "dice".into(), min: 0.0.into(), max: 1.0.into() }),
            (Data, Self::Ask { name: "answer".into(), question: Question::ItemCount { key: "synthetic".into() } }),
            (Data, Self::SaveValue { key: "score".into(), value: Operand::Var("score".into()) }),
            (Data, Self::ListPush { list: "list".into(), value: 1.0.into() }),
            (Data, Self::ListGet { name: "item".into(), list: "list".into(), index: 0.0.into() }),
            (Data, Self::ListSet { list: "list".into(), index: 0.0.into(), value: 1.0.into() }),
            (Data, Self::ListLength { name: "length".into(), list: "list".into() }),
            (Data, Self::ListClear { list: "list".into() }),
            (Data, Self::Sense { name: "dist".into(), sensor: Sensor::DistanceToPlayer }),
            (Data, Self::log_default()),
            (World, Self::SetBlock { offset: [0, 1, 0], block: "stone".into(), rot: 0 }),
            (World, Self::BreakBlock { offset: [0, 0, 0], drop: true }),
            (World, Self::SpawnParticle { kind: "spark".into(), count: 12 }),
            (World, Self::DropResource { resource: "crystalline".into(), amount: 1 }),
            (World, Self::PlaySound { sound: "ui_beep".into() }),
            (World, Self::SetTimeOfDay { value: 0.5 }),
            (World, Self::SpawnMob { mob: "Starter".into() }),
            (Entity, Self::move_default()),
            (Entity, Self::rotate_default()),
            (Entity, Self::MoveToward { target: ScriptTarget::Player, speed: 3.0 }),
            (Entity, Self::ApplyImpulse { impulse: [0.0, 6.0, 0.0] }),
            (Entity, Self::PlayAction { action: "Walk".into(), blend: 0.2 }),
            (Entity, Self::SetHealth { amount: 10.0.into() }),
            (Entity, Self::Shoot { damage: 1.5.into(), range: 19.0.into() }),
            (Entity, Self::MoveTo { x: Operand::Var("target_x".into()), y: Operand::Var("target_y".into()), z: Operand::Var("target_z".into()), speed: 3.0 }),
            (Entity, Self::Attack { damage: 1.0, range: 1.8 }),
            (Entity, Self::TameEntity),
            (Signal, Self::emit_default()),
            (Signal, Self::add_energy_default()),
            (Signal, Self::SetSignal { port: "out".into(), value: 1.0.into() }),
            (World, Self::SetWeather { weather: "Rain".into() }),
            (World, Self::StartQuest { quest: "quest".into() }),
            (World, Self::ShowDialogue { dialogue: "dialogue".into() }),
            (Signal, Self::Broadcast { message: "message".into(), value: 1.0.into() }),
            (Player, Self::GiveItem { key: "synthetic".into(), amount: 1.0.into() }),
            (Player, Self::TakeItem { key: "synthetic".into(), amount: 1.0.into(), result: "paid".into() }),
            (Player, Self::SetPlayerStat { stat: PlayerStat::Health, value: 1.0.into(), add: true }),
            (Player, Self::DamagePlayer { amount: 1.0.into() }),
            (Player, Self::TeleportPlayer { x: 0.0.into(), y: 40.0.into(), z: 0.0.into() }),
            (Player, Self::ShowMessage { text: "Hello {counter}".into() }),
            (Player, Self::OpenScreen { screen: "HUD".into() }),
            (Player, Self::CloseScreen { screen: "HUD".into() }),
            (Flow, Self::Breakpoint),
        ]
    }

}

/// A complete Blockly program.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScriptProgram {
    pub name: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub on_start: Vec<ScriptBlock>,
    #[serde(default)]
    pub on_tick: Vec<ScriptBlock>,
    /// Player pressed interact on the owner.
    #[serde(default)]
    pub on_interact: Vec<ScriptBlock>,
    /// Owner took damage (`damage` local is set).
    #[serde(default)]
    pub on_hit: Vec<ScriptBlock>,
    /// A signal arrived (`signal` local is set). Empty port = any port.
    #[serde(default)]
    pub on_signal: Vec<ScriptBlock>,
    #[serde(default)]
    pub signal_port: String,
    /// Wireless power changed (`powered` local is 0 or 1).
    #[serde(default)]
    pub on_energy: Vec<ScriptBlock>,
    /// Bound programs are instantiated per block / entity instead of once.
    #[serde(default)]
    pub bound: bool,
    /// Handlers of game events (see [`GameEvent`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<EventHandler>,
}

impl ScriptProgram {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            enabled: true,
            description: String::new(),
            on_start: Vec::new(),
            on_tick: Vec::new(),
            on_interact: Vec::new(),
            on_hit: Vec::new(),
            on_signal: Vec::new(),
            signal_port: String::new(),
            on_energy: Vec::new(),
            bound: false,
            events: Vec::new(),
        }
    }

    /// Every handler with its display name.
    pub fn handlers(&self) -> [(&'static str, &Vec<ScriptBlock>); 6] {
        [
            ("On Start", &self.on_start),
            ("On Tick", &self.on_tick),
            ("On Interact", &self.on_interact),
            ("On Hit", &self.on_hit),
            ("On Signal", &self.on_signal),
            ("On Energy", &self.on_energy),
        ]
    }

    pub fn is_empty(&self) -> bool {
        self.handlers().iter().all(|(_, blocks)| blocks.is_empty()) && self.events.is_empty()
    }
}

/// A reusable action called by programs through [`ScriptBlock::CallAction`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ActionDef {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub blocks: Vec<ScriptBlock>,
}

impl ActionDef {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: String::new(),
            blocks: Vec::new(),
        }
    }
}

/// Severity attached to a validation finding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IssueSeverity {
    Warning,
    Error,
}

impl IssueSeverity {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Warning => "WARN",
            Self::Error => "ERROR",
        }
    }
}

/// One validation finding produced by [`crate::scripting::ScriptLibrary::validate`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidationIssue {
    pub severity: IssueSeverity,
    pub location: String,
    pub message: String,
}

impl ValidationIssue {
    pub fn error(location: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            severity: IssueSeverity::Error,
            location: location.into(),
            message: message.into(),
        }
    }

    pub fn warning(location: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            severity: IssueSeverity::Warning,
            location: location.into(),
            message: message.into(),
        }
    }
}

fn default_true() -> bool {
    true
}

fn truncate(text: &str, max_chars: usize) -> String {
    let mut chars = text.chars();
    let mut result: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        result.push_str("...");
    }
    result
}
