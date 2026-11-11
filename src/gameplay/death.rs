//! Death screen and respawn choices.
//!
//! When the player dies (survival) the body stops and the death screen
//! offers three choices:
//! - **World spawn**: free, at the respawn point (`/setrespawnpoint`);
//! - **Where I died**: costs [`RESPAWN_HERE_COST`] Ethereal;
//! - **At an Etheraltar**: costs [`RESPAWN_ALTAR_COST`] Ethereal.

use crate::core::game_rules::game_rules;
use bevy::prelude::*;

use crate::core::app_state::EditorUiState;
use crate::core::pie_manager::{PieSet, PieState};
use crate::gameplay::energy_grid::EnergyGrid;
use crate::gameplay::inventory::Inventory;
use crate::gameplay::player_stats::PlayerStats;
use crate::physics::player_controller::Player;
use crate::voxel::blocks::BlockFunction;


/// Respawn option picked on the death screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RespawnChoice {
    WorldSpawn,
    WhereDied,
    Etheraltar(IVec3),
}

#[derive(Resource, Debug, Default)]
pub struct DeathState {
    pub open: bool,
    pub died_at: Vec3,
    /// Etheraltars of the world (filled when the screen opens).
    pub altars: Vec<IVec3>,
    pub choice: Option<RespawnChoice>,
    /// Message shown when a choice cannot be paid.
    pub error: Option<String>,
}

pub struct DeathPlugin;

impl Plugin for DeathPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DeathState>().add_systems(
            Update,
            handle_death.after(PieSet::Transition).after(crate::physics::player_controller::PlayerControllerSet),
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_death(
    pie: Res<PieState>,
    grid: Res<EnergyGrid>,
    mut death: ResMut<DeathState>,
    mut stats: ResMut<PlayerStats>,
    mut inventory: ResMut<Inventory>,
    mut players: Query<(&mut Player, &mut Transform)>,
    mut mount: ResMut<crate::gameplay::mob_ai::Mount>,
    mut ui: ResMut<EditorUiState>,
) {
    if !pie.is_playing() {
        *death = DeathState::default();
        return;
    }
    let Ok((mut player, mut transform)) = players.single_mut() else { return };
    if stats.is_dead() && !death.open && !inventory.spectator {
        death.open = true;
        death.died_at = transform.translation;
        death.error = None;
        death.choice = None;
        stats.deaths += 1;
        crate::scripting::context::emit_game_event(crate::scripting::context::GameEvent::PlayerDied, "", 1.0, Some(transform.translation.to_array()));
        mount.entity = None;
        let mut altars: Vec<IVec3> = grid.machines.iter().filter(|(_, m)| m.function == BlockFunction::Etheraltar).map(|(c, _)| *c).collect();
        altars.sort_by_key(|c| (c.x, c.y, c.z));
        death.altars = altars;
        ui.warn(format!("You died ({}).", stats.deaths));
    }
    if !death.open {
        return;
    }
    let Some(choice) = death.choice.take() else { return };
    let (target, cost) = match choice {
        RespawnChoice::WorldSpawn => (player.spawn, 0),
        RespawnChoice::WhereDied => (death.died_at + Vec3::Y * 0.5, game_rules().vitals.respawn_here_cost),
        RespawnChoice::Etheraltar(cell) => (cell.as_vec3() + Vec3::new(0.5, 2.2, 0.5), game_rules().vitals.respawn_altar_cost),
    };
    if inventory.survival && cost > 0 && !inventory.take("ethereal", cost) {
        death.error = Some(format!("Needs {cost} Ethereal."));
        return;
    }
    transform.translation = target;
    player.velocity = Vec3::ZERO;
    player.knockback = Vec3::ZERO;
    stats.health = stats.max_health;
    stats.stamina = stats.max_stamina;
    stats.invulnerable = 2.0;
    death.open = false;
    death.error = None;
}
