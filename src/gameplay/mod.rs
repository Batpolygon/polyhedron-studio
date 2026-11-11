//! Gameplay layer, active during Play-In-Editor (F7).
//!
//! - [`player_stats`]: 10 hearts, 10 stamina, regeneration;
//! - [`inventory`]: resources and the 9-slot hotbar;
//! - [`mining`]: aim, progressive mining, placing, interaction, footsteps;
//! - [`combat`]: sword combos, parry / block, blaster, mob strikes;
//! - [`tools`]: brush, wrench, binder, EtheraGun, BlastyGun, Etherealink, armours;
//! - [`mob_ai`]: native behaviours of the spec entities and natural spawning;
//! - [`logic`]: logic machines (Transmitter, Emitter, Receiver, Screen, Laser...);
//! - [`commands`]: the command language (chat, Terminal, Comandor);
//! - [`death`]: death screen and respawn choices;
//! - [`energy_grid`]: wireless power spheres around generators;
//! - [`data_cables`]: catenary cables carrying signals and resources;
//! - [`cable_visuals`]: the cables (tubes) and the ports (shaped pegs) as meshes;
//! - [`machines`]: harvester, synthesizer, vacuum, infuser, combustor;
//! - [`mobs`]: rigs, clips, the animator backend and the PIE runtime;
//! - [`world_rules`]: time of day, sun and sky;
//! - [`script_bridge`]: Trame commands and sensors;
//! - [`avatar`]: first-person arm, third-person body and the held item;
//! - [`items`]: armoury items (tools, weapons, food);
//! - [`drops`]: items dropped on the ground and picked up;
//! - [`chat`]: in-game chat and commands;
//! - [`player_preview`]: 3D replica of the player for the HUD;
//! - [`crafting`]: recipes, crafting stations and the Omnicore network.

pub mod atmosphere;
pub mod avatar;
pub mod cable_visuals;
pub mod chat;
pub mod commands;
pub mod death;
pub mod logic;
pub mod mob_ai;
pub mod combat;
pub mod crafting;
pub mod data_cables;
pub mod drops;
pub mod energy_grid;
pub mod inventory;
pub mod items;
pub mod quests;
pub mod machines;
pub mod mining;
pub mod mobs;
pub mod placement_preview;
pub mod player_preview;
pub mod player_stats;
pub mod script_bridge;
pub mod tools;
pub mod world_rules;

use bevy::prelude::*;

pub struct GameplayPlugin;

impl Plugin for GameplayPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            player_stats::PlayerStatsPlugin,
            inventory::InventoryPlugin,
            mining::MiningPlugin,
            combat::CombatPlugin,
            tools::ToolsPlugin,
            energy_grid::EnergyGridPlugin,
            data_cables::DataCablePlugin,
            machines::MachinePlugin,
            mobs::MobPlugin,
            world_rules::WorldRulesPlugin,
            script_bridge::ScriptBridgePlugin,
            avatar::AvatarPlugin,
        ))
        .add_plugins((
            items::ItemPlugin,
            drops::DropPlugin,
            placement_preview::PlacementPreviewPlugin,
            chat::ChatPlugin,
            player_preview::PlayerPreviewPlugin,
            crafting::CraftingPlugin,
            cable_visuals::CableVisualsPlugin,
            mob_ai::MobAiPlugin,
            logic::LogicPlugin,
            commands::CommandPlugin,
            death::DeathPlugin,
        ));
    }
}
