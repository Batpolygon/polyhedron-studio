//! Special tools and armours (spec "Tools").
//!
//! | Tool        | Use                                                                 |
//! |-------------|---------------------------------------------------------------------|
//! | Brush       | `Use` recolours the aimed synthetic block with the colour set with `C`. |
//! | Wrench      | `Use` rotates the aimed block one step.                              |
//! | Binder      | `Use` on an entity binds it for 2 Faunal: it follows and fights with you. |
//! | EtheraGun   | Hold `Use`: grabs an entity and carries it (1 Ethereal per grab).    |
//! | BlastyGun   | `Attack`: explosion that breaks blocks (resources collected) and throws the entities (1 Conflux). |
//! | Etherealink | `Use`: teleports to the next Etheraltar (2 Ethereal).                |
//! | Armours     | `Use` wears / removes the armour (flat damage reduction, extra jump, flight, damage, hearts). |
//!
//! The Gun / VoltiGun ammo and the Atomixer are handled by `combat` and
//! `mining`. In creative nothing is consumed.

use crate::core::game_rules::game_rules;
use std::sync::{LazyLock, Mutex};

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;

use crate::audio::dsp_engine::SoundKind;
use crate::audio::sound_events::SoundQueue;
use crate::core::app_state::EditorUiState;
use crate::core::pie_manager::{PieSet, PieState};
use crate::core::settings::{GameAction, GameInput};
use crate::gameplay::energy_grid::EnergyGrid;
use crate::gameplay::inventory::Inventory;
use crate::gameplay::items::{HeldTool, ItemKind, ItemLibrary};
use crate::gameplay::mining::PlayerAim;
use crate::gameplay::mobs::runtime::MobInstance;
use crate::gameplay::player_stats::PlayerStats;
use crate::physics::player_controller::Player;
use crate::renderer::particles::{ParticleBurst, ParticleQueue, ParticleStyle};
use crate::scripting::vm::ScriptRuntime;
use crate::voxel::blocks::{BlockFunction, BlockRegistry, PartColors, SyntheticLook, VoronoiCells, VoronoiNormal};
use crate::voxel::world::{Voxel, VoxelWorld};

/// Look painted by the Brush, chosen beforehand with the Customize key.
pub static BRUSH: LazyLock<Mutex<SyntheticLook>> = LazyLock::new(|| {
    Mutex::new(SyntheticLook { color: [230, 90, 160], voronoi: VoronoiCells::Cells7, normal: VoronoiNormal::default(), parts: PartColors::default() })
});

/// Current Brush look.
pub fn brush_look() -> SyntheticLook {
    *BRUSH.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub fn set_brush_look(look: SyntheticLook) {
    *BRUSH.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = look;
}


/// Permanent stat boosts bought at an Astral-Modifier.
#[derive(Resource, Debug, Default, Clone, Copy)]
pub struct StatBoosts {
    pub health: f32,
    pub stamina: f32,
    pub damage: f32,
    pub regen: f32,
    pub armor: f32,
}

/// Entity held by the EtheraGun.
#[derive(Resource, Debug, Default)]
pub struct EtheraGrab {
    pub entity: Option<Entity>,
    pub distance: f32,
}

/// Next Etheraltar used by the Etherealink (cycles through them).
#[derive(Resource, Debug, Default)]
pub struct LinkCursor {
    pub next: usize,
}

pub struct ToolsPlugin;

impl Plugin for ToolsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<StatBoosts>()
            .init_resource::<EtheraGrab>()
            .init_resource::<LinkCursor>()
            .add_systems(
                Update,
                (use_tools, carry_grabbed, apply_armor)
                    .chain()
                    .after(PieSet::Transition)
                    .after(crate::gameplay::mining::MiningSet),
            );
    }
}

/// Spend `amount` of `resource` (free in creative). Warns when missing.
pub fn pay(inventory: &mut Inventory, resource: &str, amount: u32, ui: &mut EditorUiState) -> bool {
    if !inventory.survival || amount == 0 {
        return true;
    }
    if inventory.take(resource, amount) {
        return true;
    }
    ui.warn(format!("Needs {amount} {}.", crate::gameplay::crafting::resource_label(resource)));
    false
}

#[derive(SystemParam)]
struct ToolContext<'w> {
    pie: Res<'w, PieState>,
    input: GameInput<'w>,
    aim: Res<'w, PlayerAim>,
    held: Res<'w, HeldTool>,
    inventory: ResMut<'w, Inventory>,
    registry: ResMut<'w, BlockRegistry>,
    world: ResMut<'w, VoxelWorld>,
    grid: Res<'w, EnergyGrid>,
    particles: ResMut<'w, ParticleQueue>,
    sounds: ResMut<'w, SoundQueue>,
    ui: ResMut<'w, EditorUiState>,
    runtime: ResMut<'w, ScriptRuntime>,
    grab: ResMut<'w, EtheraGrab>,
    link: ResMut<'w, LinkCursor>,
    time: Res<'w, Time>,
    items: Res<'w, crate::gameplay::items::ItemLibrary>,
    stats: ResMut<'w, crate::gameplay::player_stats::PlayerStats>,
    beams: ResMut<'w, crate::gameplay::mob_ai::MobBeams>,
}

/// Closest mob along the look ray within `range` (sphere test).
fn aimed_mob<'a>(origin: Vec3, direction: Vec3, range: f32, mobs: impl Iterator<Item = (Entity, &'a MobInstance, Vec3)>) -> Option<(Entity, f32)> {
    let mut best: Option<(Entity, f32)> = None;
    for (entity, mob, position) in mobs {
        let to = position - origin;
        let along = to.dot(direction);
        if along < 0.0 || along > range {
            continue;
        }
        if (origin + direction * along).distance(position) <= mob.half_height.max(0.6) && best.is_none_or(|(_, d)| along < d) {
            best = Some((entity, along));
        }
    }
    best
}

fn use_tools(
    mut ctx: ToolContext,
    mut players: Query<(&mut Player, &mut Transform), Without<MobInstance>>,
    mut mobs: Query<(Entity, &mut MobInstance, &Transform), Without<Player>>,
    mut effect_cooldown: Local<f32>,
) {
    *effect_cooldown = (*effect_cooldown - ctx.time.delta_secs()).max(0.0);
    if !ctx.pie.is_playing() || !ctx.pie.cursor_captured || ctx.inventory.spectator {
        ctx.grab.entity = None;
        return;
    }
    let Some(kind) = ctx.held.kind else {
        ctx.grab.entity = None;
        return;
    };
    let use_pressed = ctx.input.just_pressed(GameAction::Use);
    let attack_pressed = ctx.input.just_pressed(GameAction::Attack);
    if use_pressed {
        crate::scripting::context::emit_game_event(crate::scripting::context::GameEvent::ItemUsed, ctx.held.item.as_deref().unwrap_or(""), 1.0, None);
    }
    let origin = ctx.aim.origin;
    let direction = if ctx.aim.direction == Vec3::ZERO { Vec3::NEG_Z } else { ctx.aim.direction };
    if (use_pressed || attack_pressed) && *effect_cooldown <= 0.0 {
        if let Some(def) = ctx.held.item.as_deref().and_then(|id| ctx.items.find(id)).cloned() {
            let effects = if use_pressed { &def.on_use } else { &def.on_attack };
            if !effects.is_empty() {
                *effect_cooldown = def.cooldown;
                run_item_effects(&mut ctx, &def.id, effects, origin, direction, &mut players, &mut mobs);
            }
        }
    }

    match kind {
        ItemKind::Brush if use_pressed => {
            let Some(hit) = ctx.aim.hit else { return };
            let Some(voxel) = ctx.world.get(hit.voxel) else { return };
            let def = ctx.registry.get(voxel.block).clone();
            if !def.synthetic {
                ctx.ui.warn("The Brush paints synthetic blocks only.");
                return;
            }
            let base = def.variant_of.clone().unwrap_or(def.id.clone());
            let key = BlockRegistry::variant_key(&base, &brush_look());
            if let Some(id) = ctx.registry.ensure_variant(&key) {
                ctx.world.set(hit.voxel, Some(Voxel { block: id, ..voxel }));
                ctx.particles.burst(ParticleBurst::new(hit.voxel.as_vec3() + Vec3::splat(0.5), Vec3::new(1.0, 0.5, 0.8), 8, ParticleStyle::Spark));
                ctx.sounds.play_kind(SoundKind::BlockPlace);
            }
        }
        ItemKind::Wrench if use_pressed => {
            let Some(hit) = ctx.aim.hit else { return };
            let Some(voxel) = ctx.world.get(hit.voxel) else { return };
            let rotated = Voxel { orientation: voxel.orientation.next_roll(), ..voxel };
            ctx.world.set(hit.voxel, Some(rotated));
            ctx.sounds.play_kind(SoundKind::UiClick);
        }
        ItemKind::Binder if use_pressed => {
            let target = aimed_mob(origin, direction, 5.0, mobs.iter().map(|(e, m, t)| (e, m, t.translation)));
            let Some((entity, _)) = target else {
                ctx.ui.info("Aim at an entity to bind it.");
                return;
            };
            let ammo = ctx.held.ammo.clone().unwrap_or_else(|| "faunal".to_string());
            if let Ok((_, mut mob, transform)) = mobs.get_mut(entity) {
                if mob.tamed {
                    ctx.ui.info(format!("{} is already bound.", mob.rig));
                    return;
                }
                if !pay(&mut ctx.inventory, &ammo, game_rules().tools.bind_cost, &mut ctx.ui) {
                    return;
                }
                mob.tamed = true;
                mob.hostile = false;
                ctx.particles.burst(ParticleBurst::new(transform.translation, Vec3::new(0.5, 1.0, 0.6), 16, ParticleStyle::Spark));
                ctx.sounds.play_kind(SoundKind::PowerOn);
                ctx.ui.action(format!("{} is bound to you: it follows and fights with you.", mob.rig));
            }
        }
        ItemKind::EtheraGun => {
            if ctx.input.pressed(GameAction::Use) {
                if ctx.grab.entity.is_none() && use_pressed {
                    let target = aimed_mob(origin, direction, 14.0, mobs.iter().map(|(e, m, t)| (e, m, t.translation)));
                    if let Some((entity, distance)) = target {
                        let ammo = ctx.held.ammo.clone().unwrap_or_else(|| "ethereal".to_string());
                        if pay(&mut ctx.inventory, &ammo, 1, &mut ctx.ui) {
                            ctx.grab.entity = Some(entity);
                            ctx.grab.distance = distance.clamp(2.5, 8.0);
                            ctx.sounds.play_kind(SoundKind::Laser);
                        }
                    }
                }
            } else if let Some(entity) = ctx.grab.entity.take() {
                // Release: a small throw along the view.
                if let Ok((_, mut mob, _)) = mobs.get_mut(entity) {
                    mob.knockback += direction * 6.0;
                }
            }
        }
        ItemKind::BlastyGun if attack_pressed => {
            let ammo = ctx.held.ammo.clone().unwrap_or_else(|| "conflux".to_string());
            let end = crate::voxel::picking::raycast(&ctx.world, origin, direction, game_rules().tools.blast_range)
                .map(|hit| hit.voxel.as_vec3() + Vec3::splat(0.5))
                .unwrap_or(origin + direction * game_rules().tools.blast_range);
            if !pay(&mut ctx.inventory, &ammo, 1, &mut ctx.ui) {
                return;
            }
            explode(&mut ctx, end, &mut mobs);
        }
        ItemKind::Etherealink if use_pressed => {
            let mut altars: Vec<IVec3> = ctx.grid.machines.iter().filter(|(_, m)| m.function == BlockFunction::Etheraltar).map(|(c, _)| *c).collect();
            altars.sort_by_key(|c| (c.x, c.y, c.z));
            if altars.is_empty() {
                ctx.ui.warn("No Etheraltar placed.");
                return;
            }
            let ammo = ctx.held.ammo.clone().unwrap_or_else(|| "ethereal".to_string());
            if !pay(&mut ctx.inventory, &ammo, game_rules().tools.link_cost, &mut ctx.ui) {
                return;
            }
            let altar = altars[ctx.link.next % altars.len()];
            ctx.link.next = ctx.link.next.wrapping_add(1);
            if let Ok((mut player, mut transform)) = players.single_mut() {
                transform.translation = altar.as_vec3() + Vec3::new(0.5, 2.2, 0.5);
                player.velocity = Vec3::ZERO;
            }
            ctx.sounds.play_kind(SoundKind::PowerOn);
            ctx.ui.action(format!("Teleported to the Etheraltar at {} {} {}.", altar.x, altar.y, altar.z));
        }
        ItemKind::Armor if use_pressed => {
            let Some(id) = ctx.held.item.clone() else { return };
            if ctx.inventory.armor.as_deref() == Some(id.as_str()) {
                ctx.inventory.armor = None;
                ctx.ui.action("Armour removed.");
            } else {
                ctx.inventory.armor = Some(id);
                ctx.ui.action("Armour worn.");
            }
            ctx.inventory.revision = ctx.inventory.revision.wrapping_add(1);
            ctx.sounds.play_kind(SoundKind::Pickup);
        }
        _ => {}
    }
}

/// BlastyGun explosion: blocks in a sphere break (their drops go straight to
/// the inventory), entities nearby take heavy damage and fly.
/// Runs the effects of an item in order. A failed payment stops them.
fn run_item_effects(
    ctx: &mut ToolContext,
    item: &str,
    effects: &[crate::gameplay::items::ItemEffect],
    origin: Vec3,
    direction: Vec3,
    players: &mut Query<(&mut Player, &mut Transform), Without<MobInstance>>,
    mobs: &mut Query<(Entity, &mut MobInstance, &Transform), Without<Player>>,
) {
    use crate::gameplay::items::ItemEffect as E;
    for effect in effects {
        match effect {
            E::Spend { resource, amount } => {
                if !pay(&mut ctx.inventory, resource, *amount, &mut ctx.ui) {
                    return;
                }
            }
            E::SpendHeld => {
                if !pay(&mut ctx.inventory, item, 1, &mut ctx.ui) {
                    return;
                }
            }
            E::Heal(amount) => ctx.stats.heal(*amount),
            E::Stamina(amount) => ctx.stats.stamina = (ctx.stats.stamina + amount).clamp(0.0, ctx.stats.max_stamina),
            E::Beam { damage, range, knockback } => {
                let wall = crate::voxel::picking::raycast(&ctx.world, origin, direction, *range).map_or(*range, |hit| hit.voxel.as_vec3().distance(origin));
                let target = aimed_mob(origin, direction, wall, mobs.iter().map(|(e, m, t)| (e, &*m, t.translation)));
                let mut end = origin + direction * wall;
                if let Some((entity, distance)) = target {
                    end = origin + direction * distance;
                    if let Ok((entity, mut mob, _)) = mobs.get_mut(entity) {
                        crate::gameplay::combat::hurt_mob(entity, &mut mob, *damage, direction * *knockback, origin, &mut ctx.runtime);
                    }
                }
                ctx.beams.beams.push((origin - Vec3::Y * 0.2, end, 0.12));
            }
            E::Zone { radius, damage, knockback } => {
                let Ok((_, transform)) = players.single() else { continue };
                let center = transform.translation;
                for (entity, mut mob, transform) in mobs.iter_mut() {
                    let to = transform.translation - center;
                    if to.length() <= *radius {
                        crate::gameplay::combat::hurt_mob(entity, &mut mob, *damage, to.normalize_or_zero() * *knockback, center, &mut ctx.runtime);
                    }
                }
            }
            E::Blast => {
                let range = game_rules().tools.blast_range;
                let end = crate::voxel::picking::raycast(&ctx.world, origin, direction, range)
                    .map(|hit| hit.voxel.as_vec3() + Vec3::splat(0.5))
                    .unwrap_or(origin + direction * range);
                explode(ctx, end, mobs);
            }
            E::Dash(strength) => {
                if let Ok((mut player, _)) = players.single_mut() {
                    player.knockback += direction * *strength;
                }
            }
            E::Sparks(count) => {
                ctx.particles.burst(ParticleBurst::new(origin + direction, Vec3::new(1.0, 0.9, 0.6), *count, ParticleStyle::Spark));
            }
            E::Sound(name) => {
                ctx.sounds.play_named(name);
            }
            E::Broadcast(message) => {
                crate::scripting::context::emit_game_event(crate::scripting::context::GameEvent::Message, message, 1.0, Some(origin.to_array()));
            }
        }
    }
}

fn explode(ctx: &mut ToolContext, center: Vec3, mobs: &mut Query<(Entity, &mut MobInstance, &Transform), Without<Player>>) {
    let cell = center.floor().as_ivec3();
    let mut collected: Vec<String> = Vec::new();
    for dz in -game_rules().tools.blast_radius..=game_rules().tools.blast_radius {
        for dy in -game_rules().tools.blast_radius..=game_rules().tools.blast_radius {
            for dx in -game_rules().tools.blast_radius..=game_rules().tools.blast_radius {
                let offset = IVec3::new(dx, dy, dz);
                if offset.as_vec3().length() > game_rules().tools.blast_radius as f32 + 0.3 {
                    continue;
                }
                let probe = cell + offset;
                let Some(voxel) = ctx.world.get(probe) else { continue };
                let def = ctx.registry.get(voxel.block);
                if def.is_liquid() || def.hardness >= 1000.0 || def.function != BlockFunction::None {
                    continue;
                }
                if let Some(drop) = crate::gameplay::inventory::drop_for(&ctx.registry, voxel.block) {
                    collected.push(drop);
                }
                ctx.world.set(probe, None);
            }
        }
    }
    if ctx.inventory.survival {
        for drop in &collected {
            ctx.inventory.add(drop, 1);
        }
    }
    let damage = ctx.held.damage.max(1.0);
    let knockback = ctx.held.knockback.max(4.0);
    for (entity, mut mob, transform) in mobs.iter_mut() {
        let to = transform.translation - center;
        let distance = to.length();
        if distance > game_rules().tools.blast_radius as f32 + 2.5 {
            continue;
        }
        let falloff = 1.0 - (distance / (game_rules().tools.blast_radius as f32 + 2.5)).clamp(0.0, 1.0);
        let push = to.normalize_or_zero() * knockback * falloff + Vec3::Y * 4.0;
        crate::gameplay::combat::hurt_mob(entity, &mut mob, damage * (0.4 + 0.6 * falloff), push, center, &mut ctx.runtime);
    }
    ctx.particles.burst(ParticleBurst::new(center, Vec3::new(1.0, 0.55, 0.2), 40, ParticleStyle::Spark));
    ctx.particles.burst(ParticleBurst::new(center, Vec3::new(0.4, 0.4, 0.42), 30, ParticleStyle::Smoke));
    ctx.sounds.play_kind(SoundKind::Impact);
    if !collected.is_empty() {
        ctx.ui.action(format!("Explosion: {} block(s) collected.", collected.len()));
    }
}

/// The EtheraGun keeps the grabbed entity in front of the player.
fn carry_grabbed(
    pie: Res<PieState>,
    aim: Res<PlayerAim>,
    mut grab: ResMut<EtheraGrab>,
    mut mobs: Query<(&mut MobInstance, &mut Transform)>,
) {
    if !pie.is_playing() {
        grab.entity = None;
        return;
    }
    let Some(entity) = grab.entity else { return };
    let Ok((mut mob, mut transform)) = mobs.get_mut(entity) else {
        grab.entity = None;
        return;
    };
    let goal = aim.origin + aim.direction * grab.distance;
    let blend = 0.25;
    transform.translation = transform.translation.lerp(goal, blend);
    mob.velocity = Vec3::ZERO;
    mob.knockback = Vec3::ZERO;
    mob.stun = mob.stun.max(0.2);
}

/// Worn armour and Astral-Modifier boosts onto the player stats.
fn apply_armor(pie: Res<PieState>, inventory: Res<Inventory>, items: Res<ItemLibrary>, boosts: Res<StatBoosts>, mut stats: ResMut<PlayerStats>) {
    if !pie.is_playing() {
        return;
    }
    let armor = inventory.armor.as_deref().and_then(|id| items.find(id)).filter(|def| def.kind == ItemKind::Armor);
    let owned = inventory.armor.as_deref().is_some_and(|id| !inventory.survival || inventory.count(id) > 0);
    let armor = armor.filter(|_| owned);
    stats.flat_armor = armor.map(|def| def.armor).unwrap_or(0.0) + boosts.armor;
    stats.extra_jumps = armor.map(|def| def.extra_jumps).unwrap_or(0);
    stats.armor_flight = armor.is_some_and(|def| def.flight);
    stats.bonus_damage = armor.map(|def| def.bonus_damage).unwrap_or(0.0) + boosts.damage;
    stats.regen_bonus = boosts.regen;
    stats.max_health = game_rules().vitals.max_hearts + armor.map(|def| def.bonus_health).unwrap_or(0.0) + boosts.health;
    stats.max_stamina = game_rules().vitals.max_stamina + boosts.stamina;
    stats.health = stats.health.min(stats.max_health);
    stats.stamina = stats.stamina.min(stats.max_stamina);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paying_is_free_in_creative_and_checked_in_survival() {
        let mut ui = EditorUiState::default();
        let mut inventory = Inventory::default();
        assert!(pay(&mut inventory, "conflux", 1, &mut ui));
        inventory.survival = true;
        assert!(!pay(&mut inventory, "conflux", 1, &mut ui));
        inventory.add("conflux", 2);
        assert!(pay(&mut inventory, "conflux", 1, &mut ui));
        assert_eq!(inventory.count("conflux"), 1);
    }
}
