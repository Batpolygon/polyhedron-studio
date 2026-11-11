//! Bridge between the Blockly VM and the game world.
//!
//! The VM never touches the ECS directly. A program instance is bound to an
//! owner ([`ScriptOwner`]): the global world rules, a block of the voxel world
//! or a runtime entity (mob, movable). World-facing blocks push
//! [`ScriptCommand`]s that the gameplay layer drains every frame and applies
//! to the voxel world, entities, inventory, audio and logic signals. The game
//! feeds back [`SensorValues`] and fires [`ScriptEvent`]s into the instances.

use serde::{Deserialize, Serialize};

/// Who a program instance belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ScriptOwner {
    /// World rules: day/night, weather, waves. Positions resolve to the origin.
    Global,
    /// A scripted block at a voxel coordinate.
    Block([i32; 3]),
    /// A runtime entity, by its Bevy entity bits.
    Entity(u64),
}

/// Events delivered to the matching handler of an instance.
#[derive(Clone, Debug, PartialEq)]
pub enum ScriptEvent {
    /// The player pressed the interact key on the owner.
    Interact,
    /// The owner took damage. Sets local `damage`.
    Hit { damage: f32 },
    /// A cable / logic signal arrived. Sets locals `signal` and `port_<name>`.
    Signal { port: String, value: f32 },
    /// The owner gained or lost wireless power. Sets local `powered`.
    Energy { powered: bool },
}

/// Something that happened in the game. Any program can listen to it, whoever
/// owns the program, optionally only for one key (a rig, block, item, button).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum GameEvent {
    /// A `Broadcast` block. Key: the message.
    #[default]
    Message,
    /// A mob died. Key: the rig.
    MobDied,
    /// The player killed a mob. Key: the rig.
    MobKilled,
    /// The player took damage. Value: the damage.
    PlayerHurt,
    /// The player died.
    PlayerDied,
    /// The player placed a block. Key: the block.
    BlockPlaced,
    /// The player broke a block. Key: the block.
    BlockBroken,
    /// The player used the held item. Key: the item.
    ItemUsed,
    /// The player picked up something. Key: the resource. Value: the amount.
    ItemCollected,
    /// The player crafted something. Key: the output. Value: the amount.
    Crafted,
    /// The sun rose.
    DayStarted,
    /// The sun set.
    NightStarted,
    /// A button of an interface was pressed. Key: its action.
    UiAction,
    /// A quest was completed. Key: the quest.
    QuestDone,
    /// The weather changed. Key: the new weather (empty = clear sky).
    WeatherChanged,
    /// An achievement was unlocked. Key: the achievement.
    AchievementUnlocked,
}

impl GameEvent {
    pub const ALL: [Self; 16] = [
        Self::Message,
        Self::MobDied,
        Self::MobKilled,
        Self::PlayerHurt,
        Self::PlayerDied,
        Self::BlockPlaced,
        Self::BlockBroken,
        Self::ItemUsed,
        Self::ItemCollected,
        Self::Crafted,
        Self::DayStarted,
        Self::NightStarted,
        Self::UiAction,
        Self::QuestDone,
        Self::WeatherChanged,
        Self::AchievementUnlocked,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Message => "Message received",
            Self::MobDied => "Mob died",
            Self::MobKilled => "Mob killed by the player",
            Self::PlayerHurt => "Player hurt",
            Self::PlayerDied => "Player died",
            Self::BlockPlaced => "Block placed",
            Self::BlockBroken => "Block broken",
            Self::ItemUsed => "Item used",
            Self::ItemCollected => "Item collected",
            Self::Crafted => "Crafted",
            Self::DayStarted => "Day started",
            Self::NightStarted => "Night started",
            Self::UiAction => "Interface button",
            Self::QuestDone => "Quest completed",
            Self::WeatherChanged => "Weather changed",
            Self::AchievementUnlocked => "Achievement unlocked",
        }
    }

    /// What the event's key means, empty when it has none.
    pub const fn key_hint(self) -> &'static str {
        match self {
            Self::Message => "message",
            Self::MobDied | Self::MobKilled => "rig",
            Self::BlockPlaced | Self::BlockBroken => "block",
            Self::ItemUsed | Self::Crafted | Self::ItemCollected => "item or resource",
            Self::UiAction => "button action",
            Self::QuestDone => "quest",
            Self::WeatherChanged => "weather",
            Self::AchievementUnlocked => "achievement",
            Self::PlayerHurt | Self::PlayerDied | Self::DayStarted | Self::NightStarted => "",
        }
    }
}

/// One occurrence of a [`GameEvent`], waiting for the VM.
#[derive(Clone, Debug, PartialEq)]
pub struct GameEventRecord {
    pub event: GameEvent,
    pub key: String,
    pub value: f32,
    pub position: Option<[f32; 3]>,
}

static PENDING_EVENTS: std::sync::Mutex<Vec<GameEventRecord>> = std::sync::Mutex::new(Vec::new());

/// Report a game event from anywhere in the gameplay code.
pub fn emit_game_event(event: GameEvent, key: &str, value: f32, position: Option<[f32; 3]>) {
    let mut pending = PENDING_EVENTS.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if pending.len() < 512 {
        pending.push(GameEventRecord { event, key: key.to_string(), value, position });
    }
}

/// The events reported since the last call.
pub fn drain_game_events() -> Vec<GameEventRecord> {
    std::mem::take(&mut *PENDING_EVENTS.lock().unwrap_or_else(|poisoned| poisoned.into_inner()))
}

/// Movement targets understood by `MoveToward`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScriptTarget {
    #[default]
    Player,
    /// Where the owner spawned.
    Home,
    /// Away from the player (flee).
    AwayFromPlayer,
    /// The last attacker.
    Attacker,
}

impl ScriptTarget {
    pub const ALL: [Self; 4] = [Self::Player, Self::Home, Self::AwayFromPlayer, Self::Attacker];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Player => "Player",
            Self::Home => "Home",
            Self::AwayFromPlayer => "Away from player",
            Self::Attacker => "Attacker",
        }
    }
}

/// Values the game exposes to a program through the `Sense` block.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Sensor {
    #[default]
    DistanceToPlayer,
    Health,
    Powered,
    TimeOfDay,
    Random,
    PlayerHealth,
    NearbyAllies,
    PlayerStamina,
    PlayerX,
    PlayerY,
    PlayerZ,
    OwnerX,
    OwnerY,
    OwnerZ,
}

impl Sensor {
    pub const ALL: [Self; 14] = [
        Self::DistanceToPlayer,
        Self::Health,
        Self::Powered,
        Self::TimeOfDay,
        Self::Random,
        Self::PlayerHealth,
        Self::NearbyAllies,
        Self::PlayerStamina,
        Self::PlayerX,
        Self::PlayerY,
        Self::PlayerZ,
        Self::OwnerX,
        Self::OwnerY,
        Self::OwnerZ,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::DistanceToPlayer => "Distance to player",
            Self::Health => "Health",
            Self::Powered => "Powered (0/1)",
            Self::TimeOfDay => "Time of day (0-1)",
            Self::Random => "Random (0-1)",
            Self::PlayerHealth => "Player health",
            Self::NearbyAllies => "Nearby allies",
            Self::PlayerStamina => "Player stamina",
            Self::PlayerX => "Player X",
            Self::PlayerY => "Player Y",
            Self::PlayerZ => "Player Z",
            Self::OwnerX => "Own X",
            Self::OwnerY => "Own Y",
            Self::OwnerZ => "Own Z",
        }
    }
}

/// Sensor snapshot pushed by the game before each VM tick.
#[derive(Clone, Copy, Debug, Default)]
pub struct SensorValues {
    pub distance_to_player: f32,
    pub health: f32,
    pub powered: bool,
    pub time_of_day: f32,
    pub player_health: f32,
    pub nearby_allies: f32,
    pub player_stamina: f32,
    pub player_position: [f32; 3],
    pub position: [f32; 3],
}

impl SensorValues {
    pub fn read(&self, sensor: Sensor, random: f32) -> f32 {
        match sensor {
            Sensor::DistanceToPlayer => self.distance_to_player,
            Sensor::Health => self.health,
            Sensor::Powered => {
                if self.powered {
                    1.0
                } else {
                    0.0
                }
            }
            Sensor::TimeOfDay => self.time_of_day,
            Sensor::Random => random,
            Sensor::PlayerHealth => self.player_health,
            Sensor::NearbyAllies => self.nearby_allies,
            Sensor::PlayerStamina => self.player_stamina,
            Sensor::PlayerX => self.player_position[0],
            Sensor::PlayerY => self.player_position[1],
            Sensor::PlayerZ => self.player_position[2],
            Sensor::OwnerX => self.position[0],
            Sensor::OwnerY => self.position[1],
            Sensor::OwnerZ => self.position[2],
        }
    }
}

/// A stat of the player a program can read or change.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlayerStat {
    #[default]
    Health,
    MaxHealth,
    Stamina,
    MaxStamina,
    Kills,
    Deaths,
}

impl PlayerStat {
    pub const ALL: [Self; 6] = [Self::Health, Self::MaxHealth, Self::Stamina, Self::MaxStamina, Self::Kills, Self::Deaths];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Health => "Health",
            Self::MaxHealth => "Max health",
            Self::Stamina => "Stamina",
            Self::MaxStamina => "Max stamina",
            Self::Kills => "Kills",
            Self::Deaths => "Deaths",
        }
    }
}

/// A question to the game. The answer lands in a variable on the next tick:
/// the program waits for it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Question {
    /// How many of a resource the player holds.
    ItemCount { key: String },
    /// Distance to the nearest mob of a rig (empty = any). Also sets
    /// `<name>_x`, `<name>_y` and `<name>_z`.
    NearestMob { rig: String },
    /// How many mobs of a rig (empty = any) are alive.
    MobCount { rig: String },
    /// 1 when the block at an offset is `block` (empty = any solid block).
    BlockAt { offset: [i32; 3], block: String },
    /// A stat of the player.
    Stat { stat: PlayerStat },
    /// A value kept by `Save value`, across runs.
    Saved { key: String },
}

impl Question {
    pub fn summary(&self) -> String {
        match self {
            Self::ItemCount { key } => format!("count of {key}"),
            Self::NearestMob { rig } => format!("nearest {}", if rig.is_empty() { "mob" } else { rig }),
            Self::MobCount { rig } => format!("number of {}", if rig.is_empty() { "mobs" } else { rig }),
            Self::BlockAt { offset, block } => format!("{block} at ({}, {}, {})", offset[0], offset[1], offset[2]),
            Self::Stat { stat } => stat.label().to_string(),
            Self::Saved { key } => format!("saved {key}"),
        }
    }
}

/// A world mutation requested by a program.
#[derive(Clone, Debug, PartialEq)]
pub enum WorldCommand {
    SetBlock { offset: [i32; 3], block: String, rot: u8 },
    BreakBlock { offset: [i32; 3], drop: bool },
    SpawnParticle { kind: String, count: u32 },
    DropResource { resource: String, amount: u32 },
    PlaySound { sound: String },
    MoveToward { target: ScriptTarget, speed: f32 },
    ApplyImpulse { impulse: [f32; 3] },
    PlayAction { action: String, blend: f32 },
    SetHealth { amount: f32 },
    TameEntity,
    SetSignal { port: String, value: f32 },
    Attack { damage: f32, range: f32 },
    SpawnMob { mob: String },
    SetTimeOfDay { value: f32 },
    Ask { name: String, question: Question },
    GiveItem { key: String, amount: u32 },
    TakeItem { key: String, amount: u32, result: String },
    SetPlayerStat { stat: PlayerStat, value: f32, add: bool },
    Shoot { damage: f32, range: f32 },
    MoveTo { position: [f32; 3], speed: f32 },
    TeleportPlayer { position: [f32; 3] },
    DamagePlayer { amount: f32 },
    ShowMessage { text: String },
    SaveValue { key: String, value: f32 },
    SetWeather { weather: String },
    StartQuest { quest: String },
    ShowDialogue { dialogue: String },
    OpenScreen { screen: String },
    CloseScreen { screen: String },
}

/// A command tagged with the owner that produced it.
#[derive(Clone, Debug, PartialEq)]
pub struct ScriptCommand {
    pub owner: ScriptOwner,
    pub program: String,
    pub command: WorldCommand,
}
