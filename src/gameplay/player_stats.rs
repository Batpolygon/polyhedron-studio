//! Player vitals: 10 hearts, 10 stamina pips, regeneration and damage rules.
//! Worn armour (`Inventory::armor`) removes a flat number of damage points
//! from every hit and may add jumps, flight, damage and hearts.
//!
//! The stats are reset every time PIE starts, so a play session never leaks
//! into the next one (part of the snapshot isolation contract).

use crate::core::game_rules::game_rules;
use bevy::prelude::*;

use crate::core::pie_manager::{PieSet, PieState};



#[derive(Resource, Debug, Clone)]
pub struct PlayerStats {
    pub health: f32,
    pub max_health: f32,
    pub stamina: f32,
    pub max_stamina: f32,
    pub since_damage: f32,
    pub since_stamina_use: f32,
    /// Remaining invulnerability after a hit, in seconds.
    pub invulnerable: f32,
    pub deaths: u32,
    pub kills: u32,
    /// Extra damage reduction ratio, in `[0, 0.9]` (Astral-Modifier).
    pub armor: f32,
    /// Flat health regeneration bonus per second (Astral-Modifier).
    pub regen_bonus: f32,
    /// Damage points removed from every hit (worn armour).
    pub flat_armor: f32,
    /// Added to every player attack (Mnemo-Symbiote, Astral-Modifier).
    pub bonus_damage: f32,
    /// Extra jumps in the air (TechnicArmor).
    pub extra_jumps: u32,
    /// The worn armour lets the player fly (RefreatechArmor).
    pub armor_flight: bool,
    /// Creative mode: no damage, no stamina cost.
    pub creative: bool,
}

impl Default for PlayerStats {
    fn default() -> Self {
        Self {
            health: game_rules().vitals.max_hearts,
            max_health: game_rules().vitals.max_hearts,
            stamina: game_rules().vitals.max_stamina,
            max_stamina: game_rules().vitals.max_stamina,
            since_damage: 999.0,
            since_stamina_use: 999.0,
            invulnerable: 0.0,
            deaths: 0,
            kills: 0,
            armor: 0.0,
            regen_bonus: 0.0,
            flat_armor: 0.0,
            bonus_damage: 0.0,
            extra_jumps: 0,
            armor_flight: false,
            creative: false,
        }
    }
}

impl PlayerStats {
    /// Spend stamina if enough is available.
    pub fn try_spend(&mut self, amount: f32) -> bool {
        if self.creative {
            return true;
        }
        if self.stamina + 1e-4 < amount {
            return false;
        }
        self.stamina = (self.stamina - amount).max(0.0);
        self.since_stamina_use = 0.0;
        true
    }

    /// Continuous drain (sprint). Returns false when exhausted.
    pub fn drain(&mut self, rate: f32, delta: f32) -> bool {
        if self.creative {
            return true;
        }
        if self.stamina <= 0.0 {
            return false;
        }
        self.stamina = (self.stamina - rate * delta).max(0.0);
        self.since_stamina_use = 0.0;
        true
    }

    /// Apply damage; returns the amount actually taken.
    pub fn damage(&mut self, amount: f32) -> f32 {
        if self.creative || self.invulnerable > 0.0 || self.health <= 0.0 {
            return 0.0;
        }
        let taken = (amount - self.flat_armor).max(0.0) * (1.0 - self.armor.clamp(0.0, 0.9));
        if taken <= 0.0 {
            return 0.0;
        }
        self.health = (self.health - taken).max(0.0);
        self.since_damage = 0.0;
        self.invulnerable = 0.45;
        crate::scripting::context::emit_game_event(crate::scripting::context::GameEvent::PlayerHurt, "", taken, None);
        taken
    }

    pub fn heal(&mut self, amount: f32) {
        self.health = (self.health + amount).min(self.max_health);
    }

    pub fn is_dead(&self) -> bool {
        self.health <= 0.0
    }

    pub fn tick(&mut self, delta: f32) {
        self.since_damage += delta;
        self.since_stamina_use += delta;
        self.invulnerable = (self.invulnerable - delta).max(0.0);

        if self.since_damage > game_rules().vitals.health_regen_delay && self.health > 0.0 {
            self.heal((game_rules().vitals.health_regen_rate + self.regen_bonus) * delta);
        } else if self.regen_bonus > 0.0 && self.health > 0.0 {
            self.heal(self.regen_bonus * delta);
        }
        if self.since_stamina_use > game_rules().vitals.stamina_regen_delay {
            self.stamina = (self.stamina + game_rules().vitals.stamina_regen_rate * delta).min(self.max_stamina);
        }
    }
}

pub struct PlayerStatsPlugin;

impl Plugin for PlayerStatsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PlayerStats>()
            .add_systems(Update, tick_player_stats.after(PieSet::Transition));
    }
}

fn tick_player_stats(
    time: Res<Time>,
    pie: Res<PieState>,
    inventory: Res<crate::gameplay::inventory::Inventory>,
    mut stats: ResMut<PlayerStats>,
) {
    if pie.started_this_frame {
        *stats = PlayerStats::default();
    }
    stats.creative = !inventory.survival;
    if stats.creative {
        stats.health = stats.max_health;
        stats.stamina = stats.max_stamina;
    }
    if pie.is_playing() {
        stats.tick(time.delta_secs());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stamina_refills_after_the_delay() {
        let mut stats = PlayerStats::default();
        assert!(stats.try_spend(4.0));
        stats.tick(0.5);
        assert!((stats.stamina - 6.0).abs() < 1e-4);
        stats.tick(1.0);
        assert!(stats.stamina > 6.0);
    }

    #[test]
    fn flat_armor_removes_damage_points() {
        let mut stats = PlayerStats { flat_armor: 1.0, ..Default::default() };
        assert_eq!(stats.damage(1.0), 0.0);
        assert!((stats.damage(3.0) - 2.0).abs() < 1e-4);
    }

    #[test]
    fn damage_respects_invulnerability_and_armor() {
        let mut stats = PlayerStats {
            armor: 0.5,
            ..Default::default()
        };
        assert!((stats.damage(2.0) - 1.0).abs() < 1e-4);
        assert_eq!(stats.damage(2.0), 0.0);
        stats.tick(0.5);
        assert!(stats.damage(2.0) > 0.0);
    }
}
