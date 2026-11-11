//! Game rules of the project: every tuning value of the player, combat, tools
//! and machines, stored in `project/game_rules.ron` and edited in the Rules
//! workspace. Gameplay code reads [`rules`], so an edit applies at once, even
//! during Play. Fields missing from the file keep their default.

use std::sync::{Arc, LazyLock, RwLock};

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::core::project_io::{load_ron, save_ron, ProjectLayout};

pub const RULES_FILE: &str = "game_rules.ron";

/// One editable row: label, value, button back to the default.
pub fn rule_row<T>(ui: &mut egui::Ui, row: RuleRow<'_, T>) -> bool
where
    T: egui::emath::Numeric + PartialEq,
{
    let RuleRow { label, help, value, default, min, max } = row;
    let mut changed = false;
    ui.label(label).on_hover_text(help);
    let span = (max.to_f64() - min.to_f64()).abs();
    let speed = if T::INTEGRAL { 0.1 } else { (span / 300.0).max(0.001) };
    changed |= ui.add(egui::DragValue::new(value).range(min..=max).speed(speed)).on_hover_text(help).changed();
    if *value != default {
        if ui.small_button("↺").on_hover_text("Back to the default value").clicked() {
            *value = default;
            changed = true;
        }
    } else {
        ui.label("");
    }
    ui.end_row();
    changed
}

pub struct RuleRow<'a, T> {
    pub label: &'a str,
    pub help: &'a str,
    pub value: &'a mut T,
    pub default: T,
    pub min: T,
    pub max: T,
}

/// A section of the rules: the struct, its defaults and its panel rows, all
/// from one table so a new rule is one line.
macro_rules! rule_section {
    ($name:ident, $title:literal, { $($field:ident : $ty:ty = $default:expr, $label:literal, $min:expr, $max:expr, $help:literal;)* }) => {
        #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
        #[serde(default)]
        pub struct $name {
            $(pub $field: $ty,)*
        }

        impl Default for $name {
            fn default() -> Self {
                Self { $($field: $default,)* }
            }
        }

        impl $name {
            pub const TITLE: &'static str = $title;

            /// The rows whose label contains `filter` (lower case).
            pub fn rows(&mut self, ui: &mut egui::Ui, filter: &str) -> bool {
                let defaults = Self::default();
                let mut changed = false;
                $(
                    if filter.is_empty() || $label.to_lowercase().contains(filter) {
                        changed |= rule_row(ui, RuleRow {
                            label: $label,
                            help: $help,
                            value: &mut self.$field,
                            default: defaults.$field,
                            min: $min,
                            max: $max,
                        });
                    }
                )*
                changed
            }

            /// True when a row of this section matches `filter`.
            pub fn matches(filter: &str) -> bool {
                filter.is_empty() || [$($label),*].iter().any(|label| label.to_lowercase().contains(filter))
            }
        }
    };
}

rule_section!(PlayerRules, "PLAYER BODY", {
    half_height: f32 = 0.55, "Half height", 0.1, 3.0, "Half height of the capsule, without its round ends.";
    radius: f32 = 0.32, "Radius", 0.1, 1.5, "Radius of the capsule.";
    eye_height: f32 = 0.62, "Eye height", 0.0, 3.0, "Camera height above the body centre.";
});

rule_section!(MovementRules, "MOVEMENT", {
    walk_speed: f32 = 5.2, "Walk speed", 0.0, 40.0, "Blocks per second.";
    sprint_speed: f32 = 8.6, "Sprint speed", 0.0, 60.0, "Blocks per second.";
    sprint_drain: f32 = 1.4, "Sprint stamina drain", 0.0, 20.0, "Stamina per second.";
    jump_speed: f32 = 8.2, "Jump speed", 0.0, 40.0, "Upward speed of a jump.";
    gravity: f32 = 24.0, "Gravity", 0.0, 100.0, "Downward acceleration.";
    ledge_hop_speed: f32 = 7.4, "Ledge hop speed", 0.0, 30.0, "Automatic hop onto a one block ledge.";
    dash_speed: f32 = 21.0, "Dash speed", 0.0, 80.0, "Blocks per second.";
    dash_time: f32 = 0.16, "Dash duration", 0.01, 2.0, "Seconds.";
    dash_cooldown: f32 = 0.55, "Dash cooldown", 0.0, 10.0, "Seconds.";
    dash_cost: f32 = 2.0, "Dash stamina cost", 0.0, 20.0, "Stamina per dash.";
    swim_speed: f32 = 3.0, "Swim speed", 0.0, 30.0, "Blocks per second.";
    swim_sprint_speed: f32 = 4.6, "Swim sprint speed", 0.0, 30.0, "Blocks per second.";
    swim_up_speed: f32 = 3.8, "Swim up speed", 0.0, 30.0, "Blocks per second.";
    sink_speed: f32 = 1.2, "Sink speed", 0.0, 30.0, "Blocks per second.";
    fly_speed: f32 = 11.0, "Fly speed", 0.0, 100.0, "Creative flight.";
    fly_fast_speed: f32 = 22.0, "Fast fly speed", 0.0, 200.0, "Creative flight, sprinting.";
    fly_vertical_speed: f32 = 8.0, "Fly vertical speed", 0.0, 100.0, "Creative flight, up and down.";
});

rule_section!(VitalRules, "HEALTH AND STAMINA", {
    max_hearts: f32 = 10.0, "Max hearts", 1.0, 200.0, "Health of a new player, before armour and boosts.";
    max_stamina: f32 = 10.0, "Max stamina", 1.0, 200.0, "Stamina of a new player, before boosts.";
    health_regen_delay: f32 = 4.0, "Health regen delay", 0.0, 120.0, "Seconds after a hit.";
    health_regen_rate: f32 = 0.35, "Health regen rate", 0.0, 20.0, "Hearts per second.";
    stamina_regen_delay: f32 = 0.9, "Stamina regen delay", 0.0, 60.0, "Seconds after the last use.";
    stamina_regen_rate: f32 = 2.4, "Stamina regen rate", 0.0, 50.0, "Stamina per second.";
    respawn_here_cost: u32 = 5, "Respawn here cost", 0, 999, "Ethereal spent to come back where you died.";
    respawn_altar_cost: u32 = 2, "Respawn altar cost", 0, 999, "Ethereal spent to come back at an altar.";
});

rule_section!(CombatRules, "COMBAT", {
    combo_window: f32 = 0.65, "Combo window", 0.05, 5.0, "Seconds to chain the next hit.";
    swing_time: f32 = 0.32, "Swing time", 0.05, 3.0, "Seconds of a sword swing.";
    combo_damage_1: f32 = 2.0, "Hit 1 damage", 0.0, 200.0, "First hit of the combo.";
    combo_damage_2: f32 = 2.0, "Hit 2 damage", 0.0, 200.0, "Second hit of the combo.";
    combo_damage_3: f32 = 4.0, "Hit 3 damage", 0.0, 200.0, "Last hit of the combo.";
    combo_knockback_1: f32 = 3.0, "Hit 1 knockback", 0.0, 100.0, "First hit of the combo.";
    combo_knockback_2: f32 = 3.5, "Hit 2 knockback", 0.0, 100.0, "Second hit of the combo.";
    combo_knockback_3: f32 = 6.0, "Hit 3 knockback", 0.0, 100.0, "Last hit of the combo.";
    sword_reach: f32 = 2.8, "Sword reach", 0.5, 30.0, "Blocks.";
    parry_window: f32 = 0.3, "Parry window", 0.0, 3.0, "Seconds at the start of a block.";
    block_drain: f32 = 1.2, "Block stamina drain", 0.0, 20.0, "Stamina per second.";
    blaster_range: f32 = 40.0, "Blaster range", 1.0, 500.0, "Blocks.";
    blaster_damage: f32 = 3.0, "Blaster damage", 0.0, 200.0, "Damage of a bolt.";
});

impl CombatRules {
    pub fn combo_damage(&self) -> [f32; 3] {
        [self.combo_damage_1, self.combo_damage_2, self.combo_damage_3]
    }

    pub fn combo_knockback(&self) -> [f32; 3] {
        [self.combo_knockback_1, self.combo_knockback_2, self.combo_knockback_3]
    }
}

rule_section!(ToolRules, "TOOLS", {
    reach: f32 = 6.0, "Block reach", 1.0, 64.0, "Blocks.";
    bind_cost: u32 = 2, "Binder cost", 0, 999, "Resources to bind an entity.";
    link_cost: u32 = 2, "Etherealink cost", 0, 999, "Resources per link.";
    blast_radius: i32 = 3, "Blast radius", 0, 32, "Blocks carved by the BlastyGun.";
    blast_range: f32 = 30.0, "Blast range", 1.0, 500.0, "Blocks.";
});

rule_section!(DropRules, "DROPPED OBJECTS", {
    gravity: f32 = 18.0, "Drop gravity", 0.0, 100.0, "Downward acceleration.";
    magnet_radius: f32 = 2.6, "Magnet radius", 0.0, 32.0, "Blocks.";
    pickup_radius: f32 = 0.7, "Pickup radius", 0.1, 16.0, "Blocks.";
    pickup_delay: f32 = 0.45, "Pickup delay", 0.0, 30.0, "Seconds after a block breaks.";
    throw_pickup_delay: f32 = 1.6, "Thrown pickup delay", 0.0, 30.0, "Seconds after a throw.";
    lifetime: f32 = 300.0, "Drop lifetime", 1.0, 7200.0, "Seconds on the ground.";
});

rule_section!(InventoryRules, "INVENTORY", {
    slots: usize = 30, "Inventory slots", 1, 300, "Slots of the survival inventory.";
    stack_size: u32 = 999, "Stack size", 1, 99999, "Resources per slot.";
});

rule_section!(EnergyRules, "ENERGY", {
    fuel_per_unit: f32 = 30.0, "Fuel per unit", 0.1, 1000.0, "Seconds of burn for one fuel.";
    fuel_batch: u32 = 10, "Fuel batch", 1, 999, "Fuel added at once to a Combustor.";
    starting_fuel: f32 = 90.0, "Starting fuel", 0.0, 10000.0, "Seconds of burn of a new Combustor.";
    energizer_capacity: f32 = 600.0, "Energizer capacity", 1.0, 100000.0, "Stored energy.";
    voltic_charge: f32 = 20.0, "Voltic charge", 0.0, 10000.0, "Energy of one Voltic.";
    energizer_charge_rate: f32 = 12.0, "Energizer charge rate", 0.0, 10000.0, "Energy per second.";
});

rule_section!(MachineRules, "MACHINES", {
    boost_heart_cost: u32 = 2, "Heart boost", 0, 999, "Astral.";
    boost_stamina_cost: u32 = 1, "Stamina boost", 0, 999, "Astral.";
    boost_damage_cost: u32 = 2, "Damage boost", 0, 999, "Astral.";
    boost_regen_cost: u32 = 1, "Regeneration boost", 0, 999, "Astral.";
    boost_armour_cost: u32 = 3, "Armour boost", 0, 999, "Astral.";
    reconstructor_period: f32 = 3.0, "Reconstructor period", 0.05, 120.0, "Seconds per cycle.";
    reconstructor_learn: u32 = 10, "Reconstructor learn cost", 0, 999, "Resources to learn a block.";
    summon_cost: u32 = 5, "Summon cost", 0, 999, "Resources per summon.";
    clone_cost: u32 = 1, "Clone cost", 0, 999, "Resources per clone.";
    clone_range: f32 = 32.0, "Clone range", 1.0, 500.0, "Blocks.";
});

rule_section!(VehicleRules, "VEHICLES", {
    max_movable_cells: usize = 4096, "Max movable blocks", 1, 65536, "Blocks of one movable.";
    max_attached_cells: usize = 2048, "Max attached blocks", 1, 65536, "Blocks attached to a movable.";
    max_rotor_cells: usize = 256, "Max rotor cells", 1, 65536, "Blocks of one rotor part.";
    max_rotor_blocks: usize = 1024, "Max rotor blocks", 1, 65536, "Blocks turned by one motor.";
    max_rpm: f32 = 120.0, "Max motor RPM", 1.0, 2000.0, "Turns per minute.";
    seat_reach: f32 = 2.6, "Seat reach", 0.5, 16.0, "Blocks.";
    seat_height: f32 = 0.92, "Seat height", 0.0, 4.0, "Blocks above the seat cell.";
    remote_range: f32 = 64.0, "Remote range", 1.0, 2000.0, "Blocks.";
    buoyancy: f32 = 12.2625, "Buoyancy", 0.0, 100.0, "Upward push in water.";
    water_drag: f32 = 0.9, "Water drag", 0.0, 20.0, "Slowdown in water.";
    current: f32 = 7.0, "Water current", 0.0, 100.0, "Push of a flowing liquid.";
});

rule_section!(SpawnRules, "NATURAL SPAWNS", {
    natural_cap: usize = 24, "Mob cap", 0, 500, "Natural mobs alive at once.";
    despawn_distance: f32 = 72.0, "Despawn distance", 8.0, 1000.0, "Blocks from the player.";
});

/// Everything stored in `project/game_rules.ron`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GameRules {
    pub player: PlayerRules,
    pub movement: MovementRules,
    pub vitals: VitalRules,
    pub combat: CombatRules,
    pub tools: ToolRules,
    pub drops: DropRules,
    pub inventory: InventoryRules,
    pub energy: EnergyRules,
    pub machines: MachineRules,
    pub vehicles: VehicleRules,
    pub spawns: SpawnRules,
    /// Items given to a new survival player.
    pub starter_tools: StarterTools,
}

/// Items of a new player.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct StarterTools(pub Vec<String>);

impl Default for StarterTools {
    fn default() -> Self {
        Self(["sword", "remote", "blaster"].map(String::from).to_vec())
    }
}

impl GameRules {
    pub fn path(project: &ProjectLayout) -> std::path::PathBuf {
        project.root.join(RULES_FILE)
    }

    pub fn load_or_install(project: &ProjectLayout) -> Result<Self, String> {
        let path = Self::path(project);
        if !path.is_file() {
            let rules = Self::default();
            save_ron(&path, &rules)?;
            return Ok(rules);
        }
        load_ron(&path)
    }

    pub fn save(&self, project: &ProjectLayout) -> Result<(), String> {
        save_ron(&Self::path(project), self)
    }
}

static RULES: LazyLock<RwLock<Arc<GameRules>>> = LazyLock::new(|| RwLock::new(Arc::new(GameRules::default())));

/// The rules in use. The handle holds no lock, so calls can nest freely.
pub fn game_rules() -> Arc<GameRules> {
    RULES.read().unwrap_or_else(|poisoned| poisoned.into_inner()).clone()
}

/// Replace the rules (project load, Rules workspace).
pub fn set_rules(rules: GameRules) {
    *RULES.write().unwrap_or_else(|poisoned| poisoned.into_inner()) = Arc::new(rules);
    RULES_REVISION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}

static RULES_REVISION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// Changes whenever the rules are edited.
pub fn rules_revision() -> u64 {
    RULES_REVISION.load(std::sync::atomic::Ordering::Relaxed)
}

pub struct GameRulesPlugin;

impl Plugin for GameRulesPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, load_game_rules.after(crate::core::project_io::initialize_project_system));
    }
}

fn load_game_rules(project: Res<ProjectLayout>, mut ui: ResMut<crate::core::app_state::EditorUiState>) {
    match GameRules::load_or_install(&project) {
        Ok(loaded) => set_rules(loaded),
        Err(error) => ui.error(format!("Game rules not loaded ({error}): defaults used.")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_fields_keep_their_default() {
        let parsed: GameRules = ron::from_str("(movement: (walk_speed: 7.0))").unwrap();
        assert_eq!(parsed.movement.walk_speed, 7.0);
        assert_eq!(parsed.movement.gravity, MovementRules::default().gravity);
        assert_eq!(parsed.starter_tools, StarterTools::default());
    }

    #[test]
    fn rules_round_trip_through_ron() {
        let mut rules = GameRules::default();
        rules.combat.combo_damage_3 = 9.0;
        rules.starter_tools.0.push("wrench".to_string());
        let text = ron::ser::to_string(&rules).unwrap();
        assert_eq!(ron::from_str::<GameRules>(&text).unwrap(), rules);
    }
}
