//! The command language of Polyhedron, shared by the chat (`/`), the
//! Terminal (one line after another, in a loop) and the Comandor.
//!
//! Entities are named by a selector: `@p` / `player` (the player), `@s`
//! (the entity running the command, see `/referenceto`), `@n` (the nearest
//! entity) or a rig name (`Gim-Gim`, spaces as `_`): the nearest one.
//! Coordinates accept `~` (relative to whoever runs the command).
//!
//! In survival every command typed in the chat spends 1 Omnicore from the
//! world network; machines need a non-empty network but spend nothing.

use std::collections::HashMap;

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;

use crate::core::app_state::EditorUiState;
use crate::core::pie_manager::{PieOnly, PieSet, PieState};
use crate::gameplay::chat::{Chat, ChatKind};
use crate::gameplay::crafting::OmnicoreNetwork;
use crate::gameplay::data_cables::{CableNetwork, PortRef};
use crate::gameplay::energy_grid::EnergyGrid;
use crate::gameplay::inventory::{HotbarItem, Inventory, PlayMode};
use crate::gameplay::items::ItemLibrary;
use crate::gameplay::logic::{LogicState, SignalValue};
use crate::gameplay::machines::config::MachineConfigs;
use crate::gameplay::machines::motors::Motors;
use crate::gameplay::mobs::library::MobLibrary;
use crate::gameplay::mobs::model::{MobBehavior, MobBox, MobRig};
use crate::gameplay::mobs::runtime::{rig_half_height, spawn_mob, MobAssets, MobInstance};
use crate::gameplay::player_stats::PlayerStats;
use crate::gameplay::world_rules::WorldRules;
use crate::physics::player_controller::Player;
use crate::scripting::context::{ScriptEvent, ScriptOwner};
use crate::scripting::vm::ScriptRuntime;
use crate::voxel::blocks::{BlockRegistry, PortDirection, PortKind};
use crate::voxel::shapes::Orientation;
use crate::voxel::world::{Voxel, VoxelWorld};

/// Largest volume edited by one fill / break / clone.
const MAX_VOLUME: i64 = 32_768;

/// Who runs a command.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CommandSource {
    Player,
    Block(IVec3),
}

#[derive(Clone, Debug)]
pub struct CommandRequest {
    pub text: String,
    pub source: CommandSource,
}

/// Commands waiting to run this frame.
#[derive(Resource, Debug, Default)]
pub struct CommandQueue {
    pub pending: Vec<CommandRequest>,
}

impl CommandQueue {
    pub fn push_player(&mut self, text: &str) {
        self.pending.push(CommandRequest { text: text.trim_start_matches('/').to_string(), source: CommandSource::Player });
    }

    pub fn push_block(&mut self, text: &str, cell: IVec3) {
        let text = text.trim().trim_start_matches('/').to_string();
        if !text.is_empty() {
            self.pending.push(CommandRequest { text, source: CommandSource::Block(cell) });
        }
    }
}

/// Kinds of argument, for the help and the `Tab` completion.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arg {
    Coord,
    Block,
    Entity,
    Mob,
    Resource,
    Stat,
    Time,
    Var,
    Value,
    Text,
    Number,
    Kind,
    Command,
    Mode,
}

/// One command of the language.
pub struct CommandSpec {
    pub name: &'static str,
    pub format: &'static str,
    pub help: &'static str,
    pub args: &'static [Arg],
}

/// Every command, in the order of `/helpcommand`.
pub const SPECS: &[CommandSpec] = &[
    CommandSpec { name: "helpcommand", format: "/helpcommand", help: "details the commands", args: &[] },
    CommandSpec { name: "helpformat", format: "/helpformat [command]", help: "details the arguments of the commands", args: &[Arg::Command] },
    CommandSpec { name: "setstat", format: "/setstat <entity> <health|maxhealth|stamina|speed|damage|armor|hostile> <value>", help: "changes a stat of an entity", args: &[Arg::Entity, Arg::Stat, Arg::Value] },
    CommandSpec { name: "setrespawnpoint", format: "/setrespawnpoint [x y z]", help: "changes the respawn point", args: &[Arg::Coord, Arg::Coord, Arg::Coord] },
    CommandSpec { name: "settime", format: "/settime <day|noon|sunset|night|midnight|sunrise|0..1>", help: "changes the time of the world", args: &[Arg::Time] },
    CommandSpec { name: "setpolyhedron", format: "/setpolyhedron <x> <y> <z> <block> [rotation 0-23]", help: "places a block", args: &[Arg::Coord, Arg::Coord, Arg::Coord, Arg::Block, Arg::Number] },
    CommandSpec { name: "fillpolyhedron", format: "/fillpolyhedron <x1> <y1> <z1> <x2> <y2> <z2> <block>", help: "places blocks between two corners", args: &[Arg::Coord, Arg::Coord, Arg::Coord, Arg::Coord, Arg::Coord, Arg::Coord, Arg::Block] },
    CommandSpec { name: "breakpolyhedron", format: "/breakpolyhedron <x1> <y1> <z1> <x2> <y2> <z2>", help: "breaks the blocks between two corners", args: &[Arg::Coord, Arg::Coord, Arg::Coord, Arg::Coord, Arg::Coord, Arg::Coord] },
    CommandSpec { name: "clonepolyhedron", format: "/clonepolyhedron <x1> <y1> <z1> <x2> <y2> <z2> <x> <y> <z>", help: "copies the blocks between two corners to a destination", args: &[Arg::Coord, Arg::Coord, Arg::Coord, Arg::Coord, Arg::Coord, Arg::Coord, Arg::Coord, Arg::Coord, Arg::Coord] },
    CommandSpec { name: "teleportentitytocoord", format: "/teleportentitytocoord <entity> <x> <y> <z>", help: "teleports an entity to coordinates", args: &[Arg::Entity, Arg::Coord, Arg::Coord, Arg::Coord] },
    CommandSpec { name: "teleportentitytoentity", format: "/teleportentitytoentity <entity> <target>", help: "teleports an entity onto another entity", args: &[Arg::Entity, Arg::Entity] },
    CommandSpec { name: "spawn", format: "/spawn <entity name> [x y z]", help: "spawns an entity", args: &[Arg::Mob, Arg::Coord, Arg::Coord, Arg::Coord] },
    CommandSpec { name: "kill", format: "/kill <entity>", help: "kills an entity instantly", args: &[Arg::Entity] },
    CommandSpec { name: "findpolyhedron", format: "/findpolyhedron <x> <y> <z> [block]", help: "tests if there is a block (of that kind) at a coordinate", args: &[Arg::Coord, Arg::Coord, Arg::Coord, Arg::Block] },
    CommandSpec { name: "findentity", format: "/findentity <entity name> [radius]", help: "finds the nearest entity", args: &[Arg::Mob, Arg::Number] },
    CommandSpec { name: "findchat", format: "/findchat <word>", help: "finds a word in the chat", args: &[Arg::Text] },
    CommandSpec { name: "findvar", format: "/findvar <name>", help: "finds a created variable", args: &[Arg::Var] },
    CommandSpec { name: "sendchat", format: "/sendchat <message>", help: "sends a message in the chat", args: &[Arg::Text] },
    CommandSpec { name: "sendsignal", format: "/sendsignal <x> <y> <z> <value>", help: "sends a signal to the machine at the coordinates", args: &[Arg::Coord, Arg::Coord, Arg::Coord, Arg::Value] },
    CommandSpec { name: "sendtoomnicorenetwork", format: "/sendtoomnicorenetwork <amount|all>", help: "sends Omnicore from the inventory into the network", args: &[Arg::Number] },
    CommandSpec { name: "assignvar", format: "/assignvar <name> <value>", help: "assigns a value to a variable (creates it when missing)", args: &[Arg::Var, Arg::Value] },
    CommandSpec { name: "deletevar", format: "/deletevar <name>", help: "deletes a variable", args: &[Arg::Var] },
    CommandSpec { name: "listvar", format: "/listvar", help: "lists the variables", args: &[] },
    CommandSpec { name: "stopmachine", format: "/stopmachine", help: "stops every machine (OFF)", args: &[] },
    CommandSpec { name: "wire", format: "/wire <x1> <y1> <z1> <x2> <y2> <z2> [energy|resource|signal]", help: "connects two machines", args: &[Arg::Coord, Arg::Coord, Arg::Coord, Arg::Coord, Arg::Coord, Arg::Coord, Arg::Kind] },
    CommandSpec { name: "gettime", format: "/gettime", help: "returns the time of the world", args: &[] },
    CommandSpec { name: "getstat", format: "/getstat <entity> <stat>", help: "returns a stat of an entity", args: &[Arg::Entity, Arg::Stat] },
    CommandSpec { name: "createentity", format: "/createentity <name> <health> <hostile true|false> [size] [r g b]", help: "creates an entity from scratch", args: &[Arg::Text, Arg::Number, Arg::Value, Arg::Number, Arg::Number, Arg::Number, Arg::Number] },
    CommandSpec { name: "rideentity", format: "/rideentity <rider> <mount|none>", help: "makes an entity sit on another entity", args: &[Arg::Entity, Arg::Entity] },
    CommandSpec { name: "ridepolyhedron", format: "/ridepolyhedron <entity> <x> <y> <z>", help: "makes an entity sit on a block", args: &[Arg::Entity, Arg::Coord, Arg::Coord, Arg::Coord] },
    CommandSpec { name: "addressourcetoentity", format: "/addressourcetoentity <entity> <resource> <count>", help: "adds resources to an entity", args: &[Arg::Entity, Arg::Resource, Arg::Number] },
    CommandSpec { name: "addressourcetopolyhedron", format: "/addressourcetopolyhedron <x> <y> <z> <resource> <count>", help: "adds resources to a block (machine storage)", args: &[Arg::Coord, Arg::Coord, Arg::Coord, Arg::Resource, Arg::Number] },
    CommandSpec { name: "damage", format: "/damage <entity> <amount>", help: "deals damage to an entity", args: &[Arg::Entity, Arg::Number] },
    CommandSpec { name: "slideentity", format: "/slideentity <entity> <x> <y> <z> [seconds]", help: "moves an entity smoothly to coordinates", args: &[Arg::Entity, Arg::Coord, Arg::Coord, Arg::Coord, Arg::Number] },
    CommandSpec { name: "bind", format: "/bind <entity> <master>", help: "forces a bind between two entities", args: &[Arg::Entity, Arg::Entity] },
    CommandSpec { name: "referenceto", format: "/referenceto <entity> <command...>", help: "runs a command as if it were this entity", args: &[Arg::Entity, Arg::Command] },
    CommandSpec { name: "aimoveto", format: "/aimoveto <entity> <x> <y> <z>", help: "forces an AI to walk to the coordinates", args: &[Arg::Entity, Arg::Coord, Arg::Coord, Arg::Coord] },
    CommandSpec { name: "aistop", format: "/aistop <entity>", help: "forces an AI to stop (again to resume)", args: &[Arg::Entity] },
    CommandSpec { name: "laser", format: "/laser <x> <y> <z> <dx> <dy> <dz> [length]", help: "simulates a Laser block (line trace)", args: &[Arg::Coord, Arg::Coord, Arg::Coord, Arg::Number, Arg::Number, Arg::Number, Arg::Number] },
    CommandSpec { name: "slideblock", format: "/slideblock <x1> <y1> <z1> <x2> <y2> <z2> [seconds]", help: "slides a block to a coordinate", args: &[Arg::Coord, Arg::Coord, Arg::Coord, Arg::Coord, Arg::Coord, Arg::Coord, Arg::Number] },
    CommandSpec { name: "gamemode", format: "/gamemode <creative|survival|spectator>", help: "changes the game mode", args: &[Arg::Mode] },
    CommandSpec { name: "give", format: "/give <block|item|resource|egg:entity> [count]", help: "gives something to the player", args: &[Arg::Resource, Arg::Number] },
];

/// The spec of a command name.
pub fn spec(name: &str) -> Option<&'static CommandSpec> {
    SPECS.iter().find(|spec| spec.name.eq_ignore_ascii_case(name))
}

/// Commands that never cost Omnicore.
fn is_free(name: &str) -> bool {
    matches!(name, "helpcommand" | "helpformat" | "help" | "listvar" | "findvar" | "gettime")
}

/// An entity named by a selector.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Target {
    Player(Entity),
    Mob(Entity),
}

/// What a rider sits on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RideAnchor {
    Entity(Target),
    Block(IVec3),
}

/// Rides (`/rideentity`, `/ridepolyhedron`) and slides (`/slideentity`,
/// `/slideblock`) in progress.
#[derive(Resource, Debug, Default)]
pub struct Motions {
    pub rides: Vec<(Target, RideAnchor)>,
    /// (entity, from, to, elapsed, duration)
    pub slides: Vec<(Target, Vec3, Vec3, f32, f32)>,
    /// (voxel, from cell, to cell, elapsed, duration, visual)
    pub blocks: Vec<(Voxel, IVec3, IVec3, f32, f32, Option<Entity>)>,
}

#[derive(SystemParam)]
pub struct CommandWorld<'w, 's> {
    commands: Commands<'w, 's>,
    rules: ResMut<'w, WorldRules>,
    inventory: ResMut<'w, Inventory>,
    stats: ResMut<'w, PlayerStats>,
    world: ResMut<'w, VoxelWorld>,
    registry: Res<'w, BlockRegistry>,
    items: Res<'w, ItemLibrary>,
    library: ResMut<'w, MobLibrary>,
    mob_assets: ResMut<'w, MobAssets>,
    meshes: ResMut<'w, Assets<Mesh>>,
    materials: ResMut<'w, Assets<StandardMaterial>>,
    runtime: ResMut<'w, ScriptRuntime>,
    network: ResMut<'w, OmnicoreNetwork>,
    grid: ResMut<'w, EnergyGrid>,
    cables: ResMut<'w, CableNetwork>,
}

#[derive(SystemParam)]
pub struct CommandExtras<'w, 's> {
    logic: ResMut<'w, LogicState>,
    configs: ResMut<'w, MachineConfigs>,
    motors: ResMut<'w, Motors>,
    motions: ResMut<'w, Motions>,
    chat: ResMut<'w, Chat>,
    players: Query<'w, 's, (Entity, &'static mut Player, &'static mut Transform), Without<MobInstance>>,
    mobs: Query<'w, 's, (Entity, &'static mut MobInstance, &'static mut Transform), Without<Player>>,
}

#[derive(SystemParam)]
pub struct CommandCtx<'w, 's> {
    w: CommandWorld<'w, 's>,
    x: CommandExtras<'w, 's>,
}

pub struct CommandPlugin;

impl Plugin for CommandPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CommandQueue>()
            .init_resource::<Motions>()
            .add_systems(Update, (run_commands, run_motions).chain().after(PieSet::Transition).after(crate::gameplay::logic::LogicSet));
    }
}

fn run_commands(pie: Res<PieState>, mut queue: ResMut<CommandQueue>, mut ctx: CommandCtx, mut ui: ResMut<EditorUiState>) {
    let pending = std::mem::take(&mut queue.pending);
    if !pie.is_playing() {
        if pie.stopped_this_frame {
            *ctx.x.motions = Motions::default();
        }
        return;
    }
    for request in pending {
        let name = request.text.split_whitespace().next().unwrap_or("").to_ascii_lowercase();
        let origin = match request.source {
            CommandSource::Player => ctx.player().map(|(_, p)| p).unwrap_or(Vec3::ZERO),
            CommandSource::Block(cell) => cell.as_vec3() + Vec3::splat(0.5),
        };
        let survival = ctx.w.inventory.survival;
        let charged = survival && request.source == CommandSource::Player && !is_free(&name);
        if survival && !is_free(&name) && ctx.w.network.stored == 0 {
            let message = "The Omnicore network is empty: craft Omnicore at an Assembler and send it with an Omnichorus.";
            match request.source {
                CommandSource::Player => ctx.x.chat.post(ChatKind::Error, message),
                CommandSource::Block(cell) => {
                    ctx.x.logic.outputs.insert(cell, SignalValue::Bool(false));
                }
            }
            continue;
        }
        let me = match request.source {
            CommandSource::Player => ctx.player().map(|(entity, _)| Target::Player(entity)),
            CommandSource::Block(_) => None,
        };
        let result = execute(&mut ctx, &request.text, origin, me, 0);
        if charged && result.is_ok() {
            ctx.w.network.spend(1);
        }
        match request.source {
            CommandSource::Player => match result {
                Ok(message) => {
                    let left = ctx.w.network.stored;
                    let text = if charged { format!("{message}  (-1 Omnicore, {left} left)") } else { message };
                    ctx.x.chat.post(ChatKind::Success, text);
                }
                Err(message) => ctx.x.chat.post(ChatKind::Error, message),
            },
            CommandSource::Block(cell) => {
                let value = match &result {
                    Ok(text) => match SignalValue::parse(text) {
                        SignalValue::Text(_) => SignalValue::Bool(true),
                        parsed => parsed,
                    },
                    Err(_) => SignalValue::Bool(false),
                };
                if let Err(error) = &result {
                    ui.warn(format!("[Machine {} {} {}] {error}", cell.x, cell.y, cell.z));
                }
                ctx.x.logic.outputs.insert(cell, value);
            }
        }
    }
}

fn parse_coord(text: &str, current: f32) -> Result<f32, String> {
    if let Some(rest) = text.strip_prefix('~') {
        let offset = if rest.is_empty() { 0.0 } else { rest.parse::<f32>().map_err(|_| format!("Bad coordinate '{text}'"))? };
        Ok(current + offset)
    } else {
        text.parse::<f32>().map_err(|_| format!("Bad coordinate '{text}'"))
    }
}

fn parse_vec(args: &[&str], at: usize, origin: Vec3) -> Result<Vec3, String> {
    if args.len() < at + 3 {
        return Err("Missing coordinates (x y z).".to_string());
    }
    Ok(Vec3::new(parse_coord(args[at], origin.x)?, parse_coord(args[at + 1], origin.y)?, parse_coord(args[at + 2], origin.z)?))
}

fn parse_cell(args: &[&str], at: usize, origin: Vec3) -> Result<IVec3, String> {
    parse_vec(args, at, origin).map(|v| v.floor().as_ivec3())
}

fn parse_number(text: Option<&&str>, what: &str) -> Result<f32, String> {
    text.ok_or_else(|| format!("Missing {what}."))?.parse::<f32>().map_err(|_| format!("Bad {what}."))
}

fn usage(name: &str) -> String {
    spec(name).map(|spec| format!("Usage: {}", spec.format)).unwrap_or_else(|| "Bad arguments.".to_string())
}

/// Classic day ticks (0 = sunrise, 6000 = noon) to a time of day (0.25 = sunrise).
pub fn ticks_to_time(ticks: f32) -> f32 {
    (ticks / 24_000.0 + 0.25).rem_euclid(1.0)
}

/// Clock text of a time of day (0.5 = 12:00).
pub fn clock(time: f32) -> String {
    let minutes = (time.rem_euclid(1.0) * 24.0 * 60.0) as u32;
    format!("{:02}:{:02}", minutes / 60, minutes % 60)
}

impl CommandCtx<'_, '_> {
    fn player(&self) -> Option<(Entity, Vec3)> {
        self.x.players.iter().next().map(|(entity, _, transform)| (entity, transform.translation))
    }

    fn resolve(&self, selector: &str, origin: Vec3, me: Option<Target>) -> Result<Target, String> {
        let lower = selector.to_ascii_lowercase();
        let player = self.player().map(|(entity, _)| Target::Player(entity));
        match lower.as_str() {
            "@p" | "player" | "me" => return player.ok_or_else(|| "No player.".to_string()),
            "@s" | "self" => return me.or(player).ok_or_else(|| "No entity runs this command.".to_string()),
            _ => {}
        }
        let wanted = selector.replace('_', " ");
        let any = lower == "@n";
        self.x
            .mobs
            .iter()
            .filter(|(_, mob, _)| any || mob.rig.eq_ignore_ascii_case(&wanted) || mob.rig.replace(' ', "_").eq_ignore_ascii_case(selector))
            .min_by(|a, b| a.2.translation.distance(origin).total_cmp(&b.2.translation.distance(origin)))
            .map(|(entity, _, _)| Target::Mob(entity))
            .ok_or_else(|| format!("No entity '{selector}' found."))
    }

    fn position(&self, target: Target) -> Option<Vec3> {
        match target {
            Target::Player(entity) => self.x.players.get(entity).ok().map(|(_, _, t)| t.translation),
            Target::Mob(entity) => self.x.mobs.get(entity).ok().map(|(_, _, t)| t.translation),
        }
    }

    fn set_position(&mut self, target: Target, position: Vec3) {
        match target {
            Target::Player(entity) => {
                if let Ok((_, mut player, mut transform)) = self.x.players.get_mut(entity) {
                    transform.translation = position;
                    player.velocity = Vec3::ZERO;
                }
            }
            Target::Mob(entity) => {
                if let Ok((_, mut mob, mut transform)) = self.x.mobs.get_mut(entity) {
                    transform.translation = position;
                    mob.velocity = Vec3::ZERO;
                }
            }
        }
    }

    fn name_of(&self, target: Target) -> String {
        match target {
            Target::Player(_) => "Player".to_string(),
            Target::Mob(entity) => self.x.mobs.get(entity).map(|(_, mob, _)| mob.rig.clone()).unwrap_or_else(|_| "entity".to_string()),
        }
    }

    fn block_id(&self, key: &str) -> Result<crate::voxel::blocks::BlockId, String> {
        self.w.registry.find(key).ok_or_else(|| format!("Unknown block '{key}'."))
    }

    fn spawn(&mut self, rig: &MobRig, position: Vec3) -> Entity {
        let meshes = &mut self.w.meshes;
        let cube = self.w.mob_assets.cube.get_or_insert_with(|| meshes.add(Cuboid::new(1.0, 1.0, 1.0))).clone();
        spawn_mob(&mut self.w.commands, rig, position, &cube, &mut self.w.materials, &mut self.w.runtime)
    }
}

fn region(args: &[&str], origin: Vec3) -> Result<(IVec3, IVec3), String> {
    let a = parse_cell(args, 0, origin)?;
    let b = parse_cell(args, 3, origin)?;
    let (min, max) = (a.min(b), a.max(b));
    let size = (max - min + IVec3::ONE).as_i64vec3();
    if size.x * size.y * size.z > MAX_VOLUME {
        return Err(format!("Too many blocks (more than {MAX_VOLUME})."));
    }
    Ok((min, max))
}

/// Run one command (without the leading `/`).
pub fn execute(ctx: &mut CommandCtx, text: &str, origin: Vec3, me: Option<Target>, depth: u32) -> Result<String, String> {
    let words: Vec<&str> = text.trim().trim_start_matches('/').split_whitespace().collect();
    let Some(name) = words.first().map(|w| w.to_ascii_lowercase()) else {
        return Err("Empty command.".to_string());
    };
    let args = &words[1..];
    match name.as_str() {
        "helpcommand" | "help" => {
            let list: Vec<String> = SPECS.iter().map(|spec| format!("/{} ({})", spec.name, spec.help)).collect();
            Ok(format!("Commands: {}. /helpformat <command> shows its arguments.", list.join(", ")))
        }
        "helpformat" => match args.first() {
            Some(command) => spec(command.trim_start_matches('/')).map(|spec| format!("{} - {}", spec.format, spec.help)).ok_or_else(|| format!("Unknown command '{command}'.")),
            None => Ok("Entities: @p (player), @s (self), @n (nearest), or an entity name (spaces as _). Coordinates: numbers or ~ (relative). Values: true / false, numbers or text. /helpformat <command> for one command.".to_string()),
        },
        "setstat" => {
            let target = ctx.resolve(args.first().ok_or_else(|| usage("setstat"))?, origin, me)?;
            let stat = args.get(1).ok_or_else(|| usage("setstat"))?.to_ascii_lowercase();
            let raw = args.get(2).ok_or_else(|| usage("setstat"))?;
            let value = SignalValue::parse(raw);
            match target {
                Target::Player(_) => {
                    let stats = &mut ctx.w.stats;
                    let number = value.as_f32();
                    match stat.as_str() {
                        "health" => stats.health = number.clamp(0.0, stats.max_health),
                        "maxhealth" => stats.max_health = number.max(1.0),
                        "stamina" => stats.stamina = number.clamp(0.0, stats.max_stamina),
                        "maxstamina" => stats.max_stamina = number.max(1.0),
                        "damage" => stats.bonus_damage = number,
                        "armor" => stats.flat_armor = number.max(0.0),
                        other => return Err(format!("Unknown player stat '{other}'.")),
                    }
                }
                Target::Mob(entity) => {
                    let Ok((_, mut mob, _)) = ctx.x.mobs.get_mut(entity) else { return Err("Entity gone.".to_string()) };
                    let number = value.as_f32();
                    match stat.as_str() {
                        "health" => mob.health = number,
                        "maxhealth" => mob.max_health = number.max(1.0),
                        "speed" => mob.speed_scale = number.clamp(0.0, 10.0),
                        "damage" => mob.damage_scale = number.clamp(0.0, 100.0),
                        "hostile" => mob.hostile = value.as_bool(),
                        other => return Err(format!("Unknown entity stat '{other}'.")),
                    }
                }
            }
            Ok(format!("{} {stat} set to {}", ctx.name_of(target), value.as_text()))
        }
        "getstat" => {
            let target = ctx.resolve(args.first().ok_or_else(|| usage("getstat"))?, origin, me)?;
            let stat = args.get(1).ok_or_else(|| usage("getstat"))?.to_ascii_lowercase();
            let value: f32 = match target {
                Target::Player(_) => {
                    let stats = &ctx.w.stats;
                    match stat.as_str() {
                        "health" => stats.health,
                        "maxhealth" => stats.max_health,
                        "stamina" => stats.stamina,
                        "maxstamina" => stats.max_stamina,
                        "damage" => stats.bonus_damage,
                        "armor" => stats.flat_armor,
                        other => return Err(format!("Unknown player stat '{other}'.")),
                    }
                }
                Target::Mob(entity) => {
                    let Ok((_, mob, _)) = ctx.x.mobs.get(entity) else { return Err("Entity gone.".to_string()) };
                    match stat.as_str() {
                        "health" => mob.health,
                        "maxhealth" => mob.max_health,
                        "speed" => mob.speed_scale,
                        "damage" => mob.damage_scale,
                        "hostile" => f32::from(u8::from(mob.hostile)),
                        other => return Err(format!("Unknown entity stat '{other}'.")),
                    }
                }
            };
            Ok(format!("{value}"))
        }
        "setrespawnpoint" => {
            let point = if args.len() >= 3 { parse_vec(args, 0, origin)? } else { origin };
            let Some((entity, _)) = ctx.player() else { return Err("No player.".to_string()) };
            if let Ok((_, mut player, _)) = ctx.x.players.get_mut(entity) {
                player.spawn = point;
            }
            Ok(format!("Respawn point set to {:.0} {:.0} {:.0}", point.x, point.y, point.z))
        }
        "settime" => {
            let value = args.first().ok_or_else(|| usage("settime"))?.to_ascii_lowercase();
            let time = match value.as_str() {
                "day" => ticks_to_time(1_000.0),
                "noon" => 0.5,
                "sunset" => ticks_to_time(12_000.0),
                "night" => ticks_to_time(13_000.0),
                "midnight" => 0.0,
                "sunrise" => 0.25,
                other => {
                    let number: f32 = other.parse().map_err(|_| format!("Unknown time '{other}'."))?;
                    if number > 1.0 { ticks_to_time(number) } else { number.rem_euclid(1.0) }
                }
            };
            ctx.w.rules.time_of_day = time;
            ctx.w.rules.scripted = true;
            Ok(format!("Time set to {}", clock(time)))
        }
        "gettime" => Ok(format!("{:.3} ({})", ctx.w.rules.time_of_day, clock(ctx.w.rules.time_of_day))),
        "setpolyhedron" => {
            let cell = parse_cell(args, 0, origin)?;
            let block = ctx.block_id(args.get(3).ok_or_else(|| usage("setpolyhedron"))?)?;
            let rotation = args.get(4).and_then(|r| r.parse::<u8>().ok()).unwrap_or(0) % 24;
            ctx.w.world.set(cell, Some(Voxel::oriented(block, Orientation(rotation))));
            ctx.w.world.mark_placed(cell);
            Ok(format!("Block placed at {} {} {}", cell.x, cell.y, cell.z))
        }
        "fillpolyhedron" => {
            let (min, max) = region(args, origin)?;
            let block = ctx.block_id(args.get(6).ok_or_else(|| usage("fillpolyhedron"))?)?;
            let mut count = 0;
            for z in min.z..=max.z {
                for y in min.y..=max.y {
                    for x in min.x..=max.x {
                        let cell = IVec3::new(x, y, z);
                        ctx.w.world.set(cell, Some(Voxel::new(block)));
                        ctx.w.world.mark_placed(cell);
                        count += 1;
                    }
                }
            }
            Ok(format!("{count} block(s) placed"))
        }
        "breakpolyhedron" => {
            let (min, max) = region(args, origin)?;
            let mut count = 0;
            for z in min.z..=max.z {
                for y in min.y..=max.y {
                    for x in min.x..=max.x {
                        let cell = IVec3::new(x, y, z);
                        if ctx.w.world.get(cell).is_some() {
                            ctx.w.world.set(cell, None);
                            count += 1;
                        }
                    }
                }
            }
            Ok(format!("{count} block(s) broken"))
        }
        "clonepolyhedron" => {
            let (min, max) = region(args, origin)?;
            let destination = parse_cell(args, 6, origin)?;
            let mut copied = Vec::new();
            for z in min.z..=max.z {
                for y in min.y..=max.y {
                    for x in min.x..=max.x {
                        let cell = IVec3::new(x, y, z);
                        copied.push((cell - min, ctx.w.world.get(cell)));
                    }
                }
            }
            let count = copied.iter().filter(|(_, voxel)| voxel.is_some()).count();
            for (offset, voxel) in copied {
                ctx.w.world.set(destination + offset, voxel);
            }
            Ok(format!("{count} block(s) cloned"))
        }
        "teleportentitytocoord" => {
            let target = ctx.resolve(args.first().ok_or_else(|| usage("teleportentitytocoord"))?, origin, me)?;
            let point = parse_vec(args, 1, origin)?;
            ctx.set_position(target, point);
            Ok(format!("{} teleported to {:.1} {:.1} {:.1}", ctx.name_of(target), point.x, point.y, point.z))
        }
        "teleportentitytoentity" => {
            let target = ctx.resolve(args.first().ok_or_else(|| usage("teleportentitytoentity"))?, origin, me)?;
            let destination = ctx.resolve(args.get(1).ok_or_else(|| usage("teleportentitytoentity"))?, origin, me)?;
            let point = ctx.position(destination).ok_or("Target gone.")? + Vec3::Y * 1.2;
            ctx.set_position(target, point);
            Ok(format!("{} teleported onto {}", ctx.name_of(target), ctx.name_of(destination)))
        }
        "spawn" => {
            let wanted = args.first().ok_or_else(|| usage("spawn"))?.replace('_', " ");
            let rig = ctx
                .w
                .library
                .mob_rigs()
                .find(|rig| rig.name.eq_ignore_ascii_case(&wanted))
                .cloned()
                .ok_or_else(|| format!("Unknown entity '{wanted}'."))?;
            let feet = if args.len() >= 4 { parse_vec(args, 1, origin)? } else { origin + Vec3::new(2.0, 0.0, 0.0) };
            let position = feet + Vec3::Y * (rig_half_height(&rig) + 0.2);
            ctx.spawn(&rig, position);
            Ok(format!("{} spawned", rig.name))
        }
        "kill" => {
            let target = ctx.resolve(args.first().ok_or_else(|| usage("kill"))?, origin, me)?;
            match target {
                Target::Player(_) => {
                    if !ctx.w.inventory.survival {
                        return Err("The player cannot die outside survival.".to_string());
                    }
                    ctx.w.stats.health = 0.0;
                }
                Target::Mob(entity) => {
                    if let Ok((_, mut mob, _)) = ctx.x.mobs.get_mut(entity) {
                        mob.health = 0.0;
                    }
                }
            }
            Ok(format!("{} killed", ctx.name_of(target)))
        }
        "damage" => {
            let target = ctx.resolve(args.first().ok_or_else(|| usage("damage"))?, origin, me)?;
            let amount = parse_number(args.get(1), "amount")?;
            match target {
                Target::Player(_) => {
                    ctx.w.stats.invulnerable = 0.0;
                    ctx.w.stats.damage(amount);
                }
                Target::Mob(entity) => {
                    if let Ok((entity, mut mob, _)) = ctx.x.mobs.get_mut(entity) {
                        crate::gameplay::combat::hurt_mob(entity, &mut mob, amount, Vec3::ZERO, origin, &mut ctx.w.runtime);
                    }
                }
            }
            Ok(format!("{amount} damage dealt to {}", ctx.name_of(target)))
        }
        "findpolyhedron" => {
            let cell = parse_cell(args, 0, origin)?;
            let found = match (ctx.w.world.get(cell), args.get(3)) {
                (Some(voxel), Some(key)) => ctx.w.registry.get(voxel.block).id.eq_ignore_ascii_case(key) || ctx.w.registry.get(voxel.block).resource_id().eq_ignore_ascii_case(key),
                (Some(_), None) => true,
                (None, _) => false,
            };
            Ok(found.to_string())
        }
        "findentity" => {
            let wanted = args.first().ok_or_else(|| usage("findentity"))?;
            let radius = args.get(1).and_then(|r| r.parse::<f32>().ok()).unwrap_or(f32::MAX);
            let target = ctx.resolve(wanted, origin, me)?;
            let point = ctx.position(target).ok_or("Entity gone.")?;
            if point.distance(origin) > radius {
                return Err(format!("No '{wanted}' within {radius} blocks."));
            }
            Ok(format!("{} at {:.0} {:.0} {:.0} ({:.0} blocks)", ctx.name_of(target), point.x, point.y, point.z, point.distance(origin)))
        }
        "findchat" => {
            let word = args.join(" ");
            if word.is_empty() {
                return Err(usage("findchat"));
            }
            let lower = word.to_ascii_lowercase();
            let count = ctx.x.chat.lines.iter().filter(|line| line.text.to_ascii_lowercase().contains(&lower)).count();
            if count == 0 { Ok("false".to_string()) } else { Ok(format!("{count}")) }
        }
        "findvar" => {
            let name = args.first().ok_or_else(|| usage("findvar"))?;
            ctx.x.logic.variables.get(*name).map(SignalValue::as_text).ok_or_else(|| format!("No variable '{name}'."))
        }
        "assignvar" => {
            let name = args.first().ok_or_else(|| usage("assignvar"))?.to_string();
            let raw = args.get(1..).filter(|rest| !rest.is_empty()).map(|rest| rest.join(" ")).ok_or_else(|| usage("assignvar"))?;
            let value = SignalValue::parse(&raw);
            let shown = value.as_text();
            ctx.x.logic.variables.insert(name.clone(), value);
            Ok(format!("{name} = {shown}"))
        }
        "deletevar" => {
            let name = args.first().ok_or_else(|| usage("deletevar"))?;
            ctx.x.logic.variables.remove(*name).map(|_| format!("{name} deleted")).ok_or_else(|| format!("No variable '{name}'."))
        }
        "listvar" => {
            let mut list: Vec<String> = ctx.x.logic.variables.iter().map(|(name, value)| format!("{name} = {} ({})", value.as_text(), value.kind())).collect();
            list.sort();
            Ok(if list.is_empty() { "No variable.".to_string() } else { list.join(", ") })
        }
        "sendchat" => {
            let message = args.join(" ");
            if message.is_empty() {
                return Err(usage("sendchat"));
            }
            if me.is_none() {
                ctx.x.chat.post(ChatKind::System, format!("[Machine] {message}"));
            }
            Ok(message)
        }
        "sendsignal" => {
            let cell = parse_cell(args, 0, origin)?;
            let raw = args.get(3..).filter(|rest| !rest.is_empty()).map(|rest| rest.join(" ")).ok_or_else(|| usage("sendsignal"))?;
            let value = SignalValue::parse(&raw);
            ctx.x.logic.inputs.insert(cell, value.clone());
            ctx.x.logic.outputs.insert(cell, value.clone());
            if let Some(config) = ctx.x.configs.map.get_mut(&cell) {
                config.trigger = value.as_bool();
            }
            ctx.w.cables.values.insert(cell, value.as_f32());
            ctx.w.runtime.fire_event(ScriptOwner::Block(cell.to_array()), &ScriptEvent::Signal { port: "command".to_string(), value: value.as_f32() });
            Ok(format!("Signal {} sent to {} {} {}", value.as_text(), cell.x, cell.y, cell.z))
        }
        "sendtoomnicorenetwork" => {
            let owned = if ctx.w.inventory.survival { ctx.w.inventory.count("omnicore") } else { u32::MAX };
            let amount = match args.first() {
                Some(text) if text.eq_ignore_ascii_case("all") => owned.min(9_999),
                Some(text) => text.parse::<u32>().map_err(|_| usage("sendtoomnicorenetwork"))?,
                None => 1,
            };
            if amount == 0 || amount > owned {
                return Err("Not enough Omnicore in the inventory.".to_string());
            }
            if ctx.w.inventory.survival {
                ctx.w.inventory.take("omnicore", amount);
            }
            ctx.w.network.deposit(amount);
            Ok(format!("{amount} Omnicore sent: the network holds {}", ctx.w.network.stored))
        }
        "stopmachine" => {
            let cells: Vec<(IVec3, crate::voxel::blocks::BlockFunction)> = ctx.w.grid.machines.iter().map(|(cell, machine)| (*cell, machine.function)).collect();
            for (cell, function) in &cells {
                ctx.x.configs.get_or_default(*cell, *function).enabled = false;
            }
            for config in ctx.x.motors.motors.values_mut() {
                config.enabled = false;
            }
            ctx.x.configs.dirty = true;
            Ok(format!("{} machine(s) stopped", cells.len()))
        }
        "wire" => {
            let a = parse_cell(args, 0, origin)?;
            let b = parse_cell(args, 3, origin)?;
            let kind = match args.get(6).map(|k| k.to_ascii_lowercase()) {
                Some(k) if k == "energy" => Some(PortKind::Energy),
                Some(k) if k == "resource" => Some(PortKind::Resource),
                Some(k) if k == "signal" => Some(PortKind::Signal),
                Some(other) => return Err(format!("Unknown cable kind '{other}'.")),
                None => None,
            };
            let ports_a = crate::gameplay::data_cables::ports_at(&ctx.w.world, &ctx.w.registry, a);
            let ports_b = crate::gameplay::data_cables::ports_at(&ctx.w.world, &ctx.w.registry, b);
            for (index_a, port_a) in ports_a.iter().enumerate() {
                if port_a.direction != PortDirection::Output || kind.is_some_and(|k| k != port_a.kind) {
                    continue;
                }
                if let Some((index_b, port_b)) = ports_b.iter().enumerate().find(|(_, p)| p.kind == port_a.kind && p.direction == PortDirection::Input) {
                    let from = PortRef { cell: a, index: index_a as u8, kind: port_a.kind, direction: port_a.direction };
                    let to = PortRef { cell: b, index: index_b as u8, kind: port_b.kind, direction: port_b.direction };
                    let connected = ctx.w.cables.connect(from, to)?;
                    ctx.w.cables.dirty = true;
                    return Ok(format!("{} cable connected", connected.label()));
                }
            }
            Err("No matching output / input ports between these blocks.".to_string())
        }
        "createentity" => {
            let name = args.first().ok_or_else(|| usage("createentity"))?.replace('_', " ");
            let health = parse_number(args.get(1), "health")?;
            let hostile = SignalValue::parse(args.get(2).ok_or_else(|| usage("createentity"))?).as_bool();
            let size = args.get(3).and_then(|s| s.parse::<f32>().ok()).unwrap_or(0.8).clamp(0.2, 4.0);
            let channel = |i: usize, fallback: f32| args.get(i).and_then(|v| v.parse::<f32>().ok()).map(|v| if v > 1.0 { v / 255.0 } else { v }).unwrap_or(fallback);
            let color = [channel(4, 0.8), channel(5, 0.5), channel(6, 0.3)];
            let mut rig = MobRig::new(name.clone());
            rig.health = health.max(1.0);
            rig.hostile = hostile;
            rig.script = None;
            rig.spawn_count = 0;
            rig.behavior = if hostile { MobBehavior::PlayerHunter } else { MobBehavior::Tank };
            let mut body = MobBox::new("Body", None);
            body.size = [size, size * 0.8, size];
            body.color = color;
            rig.boxes.push(body);
            let mut head = MobBox::new("Head", Some("Body".to_string()));
            head.size = [size * 0.6; 3];
            head.offset = [0.0, size * 0.65, -size * 0.2];
            head.color = [color[0] * 0.8, color[1] * 0.8, color[2] * 0.8];
            rig.boxes.push(head);
            if !ctx.w.library.rigs.iter().any(|existing| existing.name == rig.name) {
                ctx.w.library.rigs.push(rig.clone());
            }
            let position = origin + Vec3::new(2.0, rig_half_height(&rig) + 0.3, 0.0);
            ctx.spawn(&rig, position);
            Ok(format!("Entity {name} created"))
        }
        "rideentity" => {
            let rider = ctx.resolve(args.first().ok_or_else(|| usage("rideentity"))?, origin, me)?;
            let mount = args.get(1).ok_or_else(|| usage("rideentity"))?;
            ctx.x.motions.rides.retain(|(who, _)| *who != rider);
            if mount.eq_ignore_ascii_case("none") {
                return Ok(format!("{} got off", ctx.name_of(rider)));
            }
            let anchor = ctx.resolve(mount, origin, me)?;
            if anchor == rider {
                return Err("An entity cannot ride itself.".to_string());
            }
            ctx.x.motions.rides.push((rider, RideAnchor::Entity(anchor)));
            Ok(format!("{} rides {}", ctx.name_of(rider), ctx.name_of(anchor)))
        }
        "ridepolyhedron" => {
            let rider = ctx.resolve(args.first().ok_or_else(|| usage("ridepolyhedron"))?, origin, me)?;
            let cell = parse_cell(args, 1, origin)?;
            if ctx.w.world.get(cell).is_none() {
                return Err("No block there.".to_string());
            }
            ctx.x.motions.rides.retain(|(who, _)| *who != rider);
            ctx.x.motions.rides.push((rider, RideAnchor::Block(cell)));
            Ok(format!("{} sits on the block", ctx.name_of(rider)))
        }
        "addressourcetoentity" => {
            let target = ctx.resolve(args.first().ok_or_else(|| usage("addressourcetoentity"))?, origin, me)?;
            let resource = args.get(1).ok_or_else(|| usage("addressourcetoentity"))?.to_string();
            let count = args.get(2).and_then(|c| c.parse::<u32>().ok()).unwrap_or(1);
            match target {
                Target::Player(_) => ctx.w.inventory.add(&resource, count),
                Target::Mob(entity) => {
                    if let Ok((_, mut mob, _)) = ctx.x.mobs.get_mut(entity) {
                        match mob.carried.iter_mut().find(|(key, _)| *key == resource) {
                            Some((_, stored)) => *stored += count,
                            None => mob.carried.push((resource.clone(), count)),
                        }
                    }
                }
            }
            Ok(format!("{count} {resource} added to {}", ctx.name_of(target)))
        }
        "addressourcetopolyhedron" => {
            let cell = parse_cell(args, 0, origin)?;
            let resource = args.get(3).ok_or_else(|| usage("addressourcetopolyhedron"))?.to_string();
            let count = args.get(4).and_then(|c| c.parse::<u32>().ok()).unwrap_or(1);
            let machine = ctx.w.grid.machine_mut(cell).ok_or("This block keeps no resources.")?;
            machine.add(&resource, count);
            Ok(format!("{count} {resource} added to the block"))
        }
        "slideentity" => {
            let target = ctx.resolve(args.first().ok_or_else(|| usage("slideentity"))?, origin, me)?;
            let to = parse_vec(args, 1, origin)?;
            let seconds = args.get(4).and_then(|s| s.parse::<f32>().ok()).unwrap_or(1.0).max(0.05);
            let from = ctx.position(target).ok_or("Entity gone.")?;
            ctx.x.motions.slides.retain(|(who, ..)| *who != target);
            ctx.x.motions.slides.push((target, from, to, 0.0, seconds));
            Ok(format!("{} slides for {seconds:.1} s", ctx.name_of(target)))
        }
        "slideblock" => {
            let from = parse_cell(args, 0, origin)?;
            let to = parse_cell(args, 3, origin)?;
            let seconds = args.get(6).and_then(|s| s.parse::<f32>().ok()).unwrap_or(1.0).max(0.05);
            let voxel = ctx.w.world.get(from).ok_or("No block to slide.")?;
            ctx.w.world.set(from, None);
            ctx.x.motions.blocks.push((voxel, from, to, 0.0, seconds, None));
            Ok(format!("Block slides for {seconds:.1} s"))
        }
        "bind" => {
            let target = ctx.resolve(args.first().ok_or_else(|| usage("bind"))?, origin, me)?;
            let master = ctx.resolve(args.get(1).ok_or_else(|| usage("bind"))?, origin, me)?;
            let Target::Mob(entity) = target else { return Err("Only an entity (not the player) can be bound.".to_string()) };
            if let Ok((_, mut mob, _)) = ctx.x.mobs.get_mut(entity) {
                mob.tamed = true;
                mob.hostile = false;
                mob.owner = match master {
                    Target::Mob(owner) if owner != entity => Some(owner),
                    _ => None,
                };
            }
            Ok(format!("{} is bound to {}", ctx.name_of(target), ctx.name_of(master)))
        }
        "referenceto" => {
            if depth > 4 {
                return Err("Too many nested /referenceto.".to_string());
            }
            let target = ctx.resolve(args.first().ok_or_else(|| usage("referenceto"))?, origin, me)?;
            let rest = args.get(1..).filter(|rest| !rest.is_empty()).map(|rest| rest.join(" ")).ok_or_else(|| usage("referenceto"))?;
            let position = ctx.position(target).ok_or("Entity gone.")?;
            execute(ctx, &rest, position, Some(target), depth + 1)
        }
        "aimoveto" => {
            let target = ctx.resolve(args.first().ok_or_else(|| usage("aimoveto"))?, origin, me)?;
            let goal = parse_vec(args, 1, origin)?;
            let Target::Mob(entity) = target else { return Err("The player has no AI.".to_string()) };
            if let Ok((_, mut mob, _)) = ctx.x.mobs.get_mut(entity) {
                mob.forced_target = Some(goal);
                mob.ai_stopped = false;
            }
            Ok(format!("{} walks to {:.0} {:.0} {:.0}", ctx.name_of(target), goal.x, goal.y, goal.z))
        }
        "aistop" => {
            let target = ctx.resolve(args.first().ok_or_else(|| usage("aistop"))?, origin, me)?;
            let Target::Mob(entity) = target else { return Err("The player has no AI.".to_string()) };
            let stopped = match ctx.x.mobs.get_mut(entity) {
                Ok((_, mut mob, _)) => {
                    mob.ai_stopped = !mob.ai_stopped;
                    mob.move_target = None;
                    mob.forced_target = None;
                    mob.ai_stopped
                }
                Err(_) => return Err("Entity gone.".to_string()),
            };
            Ok(format!("{} AI {}", ctx.name_of(target), if stopped { "stopped" } else { "resumed" }))
        }
        "laser" => {
            let start = parse_vec(args, 0, origin)?;
            let direction = Vec3::new(parse_number(args.get(3), "dx")?, parse_number(args.get(4), "dy")?, parse_number(args.get(5), "dz")?).normalize_or_zero();
            if direction == Vec3::ZERO {
                return Err("The direction cannot be zero.".to_string());
            }
            let length = args.get(6).and_then(|l| l.parse::<f32>().ok()).unwrap_or(32.0).clamp(0.5, 256.0);
            let hit = crate::voxel::picking::raycast(&ctx.w.world, start, direction, length);
            let mut best = hit.map(|hit| (hit.distance, ctx.w.registry.get(hit.block).label.clone()));
            for (_, mob, transform) in ctx.x.mobs.iter() {
                let along = (transform.translation - start).dot(direction);
                if along > 0.0 && along < best.as_ref().map(|b| b.0).unwrap_or(length) && (start + direction * along).distance(transform.translation) < mob.half_height.max(0.5) {
                    best = Some((along, mob.rig.clone()));
                }
            }
            ctx.x.logic.beams.push((start, start + direction * best.as_ref().map(|b| b.0).unwrap_or(length), best.is_some()));
            match best {
                Some((distance, name)) => Ok(format!("Hit {name} at {distance:.2}")),
                None => Ok("false".to_string()),
            }
        }
        "gamemode" | "gm" => {
            let mode = args.first().map(|a| a.to_ascii_lowercase()).ok_or_else(|| usage("gamemode"))?;
            let mode = match mode.as_str() {
                "survival" | "s" | "0" => PlayMode::Survival,
                "creative" | "c" | "1" => PlayMode::Creative,
                "spectator" | "sp" | "3" => PlayMode::Spectator,
                other => return Err(format!("Unknown game mode '{other}'.")),
            };
            ctx.w.inventory.set_mode(mode);
            if let Some((entity, _)) = ctx.player() {
                if let Ok((_, mut player, _)) = ctx.x.players.get_mut(entity) {
                    player.flying = mode == PlayMode::Spectator;
                    player.velocity = Vec3::ZERO;
                }
            }
            Ok(format!("Game mode set to {}", mode.label()))
        }
        "give" => {
            let id = args.first().ok_or_else(|| usage("give"))?.to_string();
            let count: u32 = args.get(1).and_then(|c| c.parse().ok()).unwrap_or(1).clamp(1, 9_999);
            let id = match id.strip_prefix("egg:") {
                Some(mob) => {
                    let wanted = mob.replace('_', " ");
                    let name = ctx.w.library.mob_rigs().find(|rig| rig.name.eq_ignore_ascii_case(&wanted)).map(|rig| rig.name.clone()).ok_or_else(|| format!("Unknown entity '{mob}'."))?;
                    format!("egg:{name}")
                }
                None => id,
            };
            match HotbarItem::from_resource(&id, &ctx.w.registry, &ctx.w.items) {
                Some(item) => {
                    let label = item.label(&ctx.w.registry, &ctx.w.items);
                    let key = item.resource_key(&ctx.w.registry).unwrap_or_else(|| id.clone());
                    let inventory = &mut ctx.w.inventory;
                    if inventory.survival {
                        inventory.add(&key, count);
                        inventory.auto_assign(item);
                    } else {
                        let slot = inventory.selected;
                        inventory.hotbar[slot] = item;
                    }
                    inventory.revision = inventory.revision.wrapping_add(1);
                    Ok(format!("Gave {count} [{label}]"))
                }
                None if crate::gameplay::crafting::is_resource(&id) => {
                    ctx.w.inventory.add(&id, count);
                    Ok(format!("Gave {count} [{}]", crate::gameplay::crafting::resource_label(&id)))
                }
                None => Err(format!("Unknown block, item or resource '{id}'.")),
            }
        }
        other => Err(format!("Unknown command '/{other}'. Type /helpcommand")),
    }
}

/// Rides follow their anchor; slides move entities and blocks smoothly.
#[allow(clippy::too_many_arguments)]
fn run_motions(
    time: Res<Time>,
    pie: Res<PieState>,
    mut motions: ResMut<Motions>,
    mut world: ResMut<VoxelWorld>,
    registry: Res<BlockRegistry>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut players: Query<(Entity, &mut Player, &mut Transform), Without<MobInstance>>,
    mut mobs: Query<(Entity, &mut MobInstance, &mut Transform), Without<Player>>,
    mut visuals: Query<&mut Transform, (Without<Player>, Without<MobInstance>, With<PieOnly>, With<SlidingBlockVisual>)>,
) {
    if !pie.is_playing() {
        return;
    }
    let delta = time.delta_secs();
    let mut positions: HashMap<Target, Vec3> = HashMap::new();
    for (entity, _, transform) in &players {
        positions.insert(Target::Player(entity), transform.translation);
    }
    for (entity, _, transform) in &mobs {
        positions.insert(Target::Mob(entity), transform.translation);
    }
    let mut place = |target: Target, point: Vec3| match target {
        Target::Player(entity) => {
            if let Ok((_, mut player, mut transform)) = players.get_mut(entity) {
                transform.translation = point;
                player.velocity = Vec3::ZERO;
            }
        }
        Target::Mob(entity) => {
            if let Ok((_, mut mob, mut transform)) = mobs.get_mut(entity) {
                transform.translation = point;
                mob.velocity = Vec3::ZERO;
                mob.move_target = None;
            }
        }
    };
    // Slides.
    let mut finished = Vec::new();
    for (index, slide) in motions.slides.iter_mut().enumerate() {
        slide.3 += delta;
        let t = (slide.3 / slide.4).clamp(0.0, 1.0);
        let eased = t * t * (3.0 - 2.0 * t);
        place(slide.0, slide.1.lerp(slide.2, eased));
        if t >= 1.0 {
            finished.push(index);
        }
    }
    for index in finished.into_iter().rev() {
        motions.slides.remove(index);
    }
    // Rides.
    motions.rides.retain(|(rider, _)| positions.contains_key(rider));
    for (rider, anchor) in motions.rides.clone() {
        let point = match anchor {
            RideAnchor::Entity(target) => positions.get(&target).map(|p| *p + Vec3::Y * 1.2),
            RideAnchor::Block(cell) => world.get(cell).map(|_| cell.as_vec3() + Vec3::new(0.5, 1.6, 0.5)),
        };
        match point {
            Some(point) => place(rider, point),
            None => motions.rides.retain(|(who, _)| *who != rider),
        }
    }
    // Sliding blocks.
    let mut done = Vec::new();
    for (index, (voxel, from, to, elapsed, duration, visual)) in motions.blocks.iter_mut().enumerate() {
        *elapsed += delta;
        let t = (*elapsed / *duration).clamp(0.0, 1.0);
        let point = from.as_vec3().lerp(to.as_vec3(), t) + Vec3::splat(0.5);
        match visual {
            Some(entity) => {
                if let Ok(mut transform) = visuals.get_mut(*entity) {
                    transform.translation = point;
                }
            }
            None => {
                let color = registry.get(voxel.block).color_vec();
                let entity = commands
                    .spawn((
                        Mesh3d(meshes.add(Cuboid::new(1.0, 1.0, 1.0))),
                        MeshMaterial3d(materials.add(StandardMaterial { base_color: Color::srgb(color.x, color.y, color.z), ..default() })),
                        Transform::from_translation(point),
                        PieOnly,
                        SlidingBlockVisual,
                    ))
                    .id();
                *visual = Some(entity);
            }
        }
        if t >= 1.0 {
            done.push(index);
        }
    }
    for index in done.into_iter().rev() {
        let (voxel, _, to, _, _, visual) = motions.blocks.remove(index);
        world.set(to, Some(voxel));
        if let Some(entity) = visual {
            commands.entity(entity).despawn();
        }
    }
}

/// Visual of a block moved by `/slideblock`.
#[derive(Component)]
pub struct SlidingBlockVisual;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_spec_command_is_listed_once() {
        for name in [
            "helpcommand", "helpformat", "setstat", "setrespawnpoint", "settime", "setpolyhedron", "fillpolyhedron", "breakpolyhedron",
            "clonepolyhedron", "teleportentitytocoord", "teleportentitytoentity", "spawn", "kill", "findpolyhedron", "findentity", "findchat",
            "findvar", "sendchat", "sendsignal", "sendtoomnicorenetwork", "assignvar", "deletevar", "listvar", "stopmachine", "wire",
            "gettime", "getstat", "createentity", "rideentity", "ridepolyhedron", "addressourcetoentity", "addressourcetopolyhedron",
            "damage", "slideentity", "bind", "referenceto", "aimoveto", "aistop", "laser", "slideblock",
        ] {
            assert_eq!(SPECS.iter().filter(|spec| spec.name == name).count(), 1, "{name}");
        }
    }

    #[test]
    fn coordinates_accept_relative_values() {
        let origin = Vec3::new(10.0, 5.0, -2.0);
        assert_eq!(parse_vec(&["~", "~1", "3"], 0, origin).unwrap(), Vec3::new(10.0, 6.0, 3.0));
        assert!(parse_vec(&["1", "2"], 0, origin).is_err());
        assert_eq!(clock(0.5), "12:00");
    }
}
