//! Dynamic first / third person combat (Hytale-style).
//!
//! - Sword, LMB: a 3-hit combo. Only a swing that touches an entity counts:
//!   each hit opens a short chain window and hitting again inside it advances
//!   the combo (damage 2, 2, then a heavy 4 with extra knockback). A swing
//!   that touches nothing, or a late swing, resets the combo. Every hit shows
//!   a hitmarker on the crosshair.
//! - Sword, RMB: tap for a parry window (0.3 s) that fully negates the next
//!   mob strike, stuns the attacker and refunds stamina; hold to block (half
//!   damage, drains stamina).
//! - Gun / VoltiGun, LMB: hitscan bolt (1 stamina) that spends one `ammo`
//!   resource in survival (Gun: Mineral, VoltiGun: Voltic).
//! - Dash (`Ctrl`) grants invulnerability frames.
//!
//! Mob strikes come from the Blockly `Attack` block and are resolved here
//! through [`MobStrike`] requests.

use crate::core::game_rules::game_rules;
use bevy::prelude::*;

use crate::audio::dsp_engine::SoundKind;
use crate::audio::sound_events::{SoundQueue, SoundRequest};
use crate::core::app_state::EditorUiState;
use crate::core::pie_manager::{PieSet, PieState};
use crate::core::settings::{GameAction, GameInput};
use crate::gameplay::inventory::{HotbarItem, Inventory};
use crate::gameplay::mining::PlayerAim;
use crate::gameplay::mobs::runtime::MobInstance;
use crate::gameplay::player_stats::PlayerStats;
use crate::physics::player_controller::{Player, PlayerControllerSet};
use crate::renderer::particles::{ParticleBurst, ParticleQueue, ParticleStyle};
use crate::scripting::context::ScriptEvent;
use crate::scripting::vm::ScriptRuntime;
use crate::voxel::picking::raycast;
use crate::voxel::world::VoxelWorld;

/// Seconds the hitmarker stays on the crosshair.
pub const HITMARKER_TIME: f32 = 0.28;

#[derive(Resource, Debug, Default)]
pub struct CombatState {
    /// 0 = idle, 1..=3 = last combo hit landed.
    pub combo: usize,
    pub combo_timer: f32,
    pub swing_timer: f32,
    pub parry_timer: f32,
    pub blocking: bool,
    pub blaster_cooldown: f32,
    /// Laser bolt to draw: (from, to, remaining time).
    pub bolt: Option<(Vec3, Vec3, f32)>,
    pub parries: u32,
    /// Remaining hitmarker time (crosshair feedback).
    pub hitmarker: f32,
    /// The last hit was the heavy third one (bigger hitmarker).
    pub heavy_hit: bool,
}

/// A mob strike waiting to be resolved against the player.
#[derive(Clone, Copy, Debug)]
pub struct MobStrike {
    pub attacker: Entity,
    pub origin: Vec3,
    pub damage: f32,
    pub range: f32,
}

#[derive(Resource, Debug, Default)]
pub struct MobStrikes {
    pub pending: Vec<MobStrike>,
}

pub struct CombatPlugin;

impl Plugin for CombatPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CombatState>()
            .init_resource::<MobStrikes>()
            .add_systems(
                Update,
                (player_attacks, resolve_mob_strikes, draw_combat_feedback)
                    .chain()
                    .after(PieSet::Transition)
                    .after(PlayerControllerSet),
            );
    }
}

/// Damage a mob: health, flash, knockback, `On Hit` handler.
pub fn hurt_mob(
    entity: Entity,
    mob: &mut MobInstance,
    damage: f32,
    knockback: Vec3,
    source: Vec3,
    runtime: &mut ScriptRuntime,
) {
    mob.health -= damage;
    mob.hit_flash = 0.15;
    mob.knockback += knockback;
    mob.last_attacker = Some(source);
    if let Some(sound) = &mob.hurt_sound {
        crate::audio::sound_events::queue_named_sound(sound);
    }
    runtime.fire_event(MobInstance::owner(entity), &ScriptEvent::Hit { damage });
}

#[allow(clippy::too_many_arguments)]
fn player_attacks(
    time: Res<Time>,
    pie: Res<PieState>,
    input: GameInput,
    mut inventory: ResMut<Inventory>,
    aim: Res<PlayerAim>,
    world: Res<VoxelWorld>,
    mut combat: ResMut<CombatState>,
    mut stats: ResMut<PlayerStats>,
    players: Query<(&Player, &Transform), Without<MobInstance>>,
    mut mobs: Query<(Entity, &mut MobInstance, &Transform)>,
    mut runtime: ResMut<ScriptRuntime>,
    mut particles: ResMut<ParticleQueue>,
    mut sounds: ResMut<SoundQueue>,
    held: Res<crate::gameplay::items::HeldTool>,
) {
    let delta = time.delta_secs();
    combat.combo_timer = (combat.combo_timer - delta).max(0.0);
    combat.swing_timer = (combat.swing_timer - delta).max(0.0);
    combat.parry_timer = (combat.parry_timer - delta).max(0.0);
    combat.blaster_cooldown = (combat.blaster_cooldown - delta).max(0.0);
    combat.hitmarker = (combat.hitmarker - delta).max(0.0);
    if let Some((_, _, remaining)) = combat.bolt.as_mut() {
        *remaining -= delta;
    }
    if combat.bolt.map(|(_, _, remaining)| remaining <= 0.0).unwrap_or(false) {
        combat.bolt = None;
    }
    if pie.started_this_frame {
        *combat = CombatState::default();
    }
    if combat.combo_timer <= 0.0 {
        combat.combo = 0;
    }
    combat.blocking = false;

    if !pie.is_playing() || !pie.cursor_captured || inventory.spectator {
        return;
    }
    let Ok((player, body)) = players.single() else {
        return;
    };
    if player.seated.is_some() {
        return;
    }
    let eye = body.translation + Vec3::Y * 0.6;
    let look = player.look_direction();

    // Melee stats: the sword, or a weapon / tool from the armoury.
    let melee = match inventory.current() {
        HotbarItem::Item(_) => held.weapon.map(|(damage, knockback)| ((damage + stats.bonus_damage) / 2.0, knockback / 3.0)),
        _ => None,
    };
    match inventory.current().clone() {
        HotbarItem::Item(_) if melee.is_some() => {
            let (damage_scale, knockback_scale) = melee.unwrap_or((1.0, 1.0));
            // Parry / block.
            if input.just_pressed(GameAction::Use) {
                combat.parry_timer = game_rules().combat.parry_window;
                sounds.play(SoundRequest::sfx(SoundKind::SwordSwing).with_pitch(0.7).with_volume(0.4));
            }
            if input.pressed(GameAction::Use) {
                combat.blocking = stats.drain(game_rules().combat.block_drain, delta);
            }

            // Combo swing.
            if input.just_pressed(GameAction::Attack) && combat.swing_timer <= 0.0 && !combat.blocking {
                // The chain only continues from a hit that landed in time.
                let steps: Vec<(f32, f32)> = if held.combo.is_empty() {
                    let rules = game_rules();
                    rules.combat.combo_damage().iter().copied().zip(rules.combat.combo_knockback().iter().copied()).collect()
                } else {
                    held.combo.iter().map(|step| (step.damage, step.knockback)).collect()
                };
                let last = steps.len().max(1);
                let stage = if combat.combo_timer > 0.0 && combat.combo > 0 { (combat.combo % last) + 1 } else { 1 };
                if !stats.try_spend(0.4) {
                    return;
                }
                combat.swing_timer = game_rules().combat.swing_time * if stage == last { 1.4 } else { 1.0 };
                sounds.play(
                    SoundRequest::sfx(SoundKind::SwordSwing).with_pitch(0.9 + stage as f32 * 0.12),
                );

                let mut landed = false;
                for (entity, mut mob, transform) in &mut mobs {
                    let to = transform.translation - eye;
                    let distance = to.length();
                    if distance > game_rules().combat.sword_reach + mob.half_height * 0.5 {
                        continue;
                    }
                    let flat_look = Vec3::new(look.x, 0.0, look.z).normalize_or_zero();
                    let flat_to = Vec3::new(to.x, 0.0, to.z).normalize_or_zero();
                    if flat_look.dot(flat_to) < 0.45 && distance > 1.0 {
                        continue;
                    }
                    let index = stage - 1;
                    let (step_damage, step_knockback) = steps[index];
                    let push = flat_to * step_knockback * knockback_scale + Vec3::Y * (2.0 + index as f32 * 0.8);
                    hurt_mob(entity, &mut mob, step_damage * damage_scale, push, body.translation, &mut runtime);
                    particles.burst(
                        ParticleBurst {
                            bias: flat_to * 2.0,
                            ..ParticleBurst::new(
                                transform.translation,
                                Vec3::new(1.0, 0.85, 0.5),
                                if stage == last { 16 } else { 8 },
                                ParticleStyle::Spark,
                            )
                        },
                    );
                    landed = true;
                }
                if landed {
                    combat.combo = stage;
                    combat.combo_timer = game_rules().combat.combo_window;
                    combat.hitmarker = HITMARKER_TIME;
                    combat.heavy_hit = stage == last;
                    sounds.play(SoundRequest::sfx(SoundKind::Impact).with_pitch(1.0 + stage as f32 * 0.1));
                } else {
                    // A miss breaks the chain.
                    combat.combo = 0;
                    combat.combo_timer = 0.0;
                }
            }
        }
        HotbarItem::Item(_) if held.blaster.is_some() => {
            let bolt_damage = held.blaster.unwrap_or(game_rules().combat.blaster_damage) + stats.bonus_damage;
            if input.just_pressed(GameAction::Attack) && combat.blaster_cooldown <= 0.0 {
                if let Some(ammo) = held.ammo.as_deref() {
                    if inventory.survival && !inventory.take(ammo, 1) {
                        combat.blaster_cooldown = 0.4;
                        sounds.play(SoundRequest::sfx(SoundKind::UiClick).with_volume(0.5));
                        return;
                    }
                }
                if !stats.try_spend(1.0) {
                    return;
                }
                combat.blaster_cooldown = 0.25;
                sounds.play_kind(SoundKind::Laser);
                let origin = aim.origin;
                let direction = if aim.direction == Vec3::ZERO { look } else { aim.direction };
                let mut end_distance = raycast(&world, origin, direction, game_rules().combat.blaster_range)
                    .map(|hit| hit.distance)
                    .unwrap_or(game_rules().combat.blaster_range);

                // Closest mob intersecting the bolt (sphere test).
                let mut best: Option<(Entity, f32)> = None;
                for (entity, mob, transform) in &mobs {
                    let to = transform.translation - origin;
                    let along = to.dot(direction);
                    if along < 0.0 || along > end_distance {
                        continue;
                    }
                    let closest = origin + direction * along;
                    let radius = mob.half_height.max(0.5);
                    if closest.distance(transform.translation) <= radius
                        && best.map(|(_, d)| along < d).unwrap_or(true)
                    {
                        best = Some((entity, along));
                    }
                }
                if let Some((entity, along)) = best {
                    end_distance = along;
                    if let Ok((entity, mut mob, transform)) = mobs.get_mut(entity) {
                        combat.hitmarker = HITMARKER_TIME;
                        combat.heavy_hit = false;
                        hurt_mob(entity, &mut mob, bolt_damage, direction * 3.0, origin, &mut runtime);
                        particles.burst(ParticleBurst::new(
                            transform.translation,
                            Vec3::new(1.0, 0.3, 0.9),
                            10,
                            ParticleStyle::Spark,
                        ));
                    }
                }
                let end = origin + direction * end_distance;
                particles.burst(ParticleBurst::new(end, Vec3::new(1.0, 0.4, 0.9), 6, ParticleStyle::Spark));
                combat.bolt = Some((eye + look * 0.4 - Vec3::Y * 0.15, end, 0.08));
            }
        }
        _ => {}
    }
}

#[allow(clippy::too_many_arguments)]
fn resolve_mob_strikes(
    pie: Res<PieState>,
    mut strikes: ResMut<MobStrikes>,
    mut combat: ResMut<CombatState>,
    mut stats: ResMut<PlayerStats>,
    mut players: Query<(&mut Player, &Transform), Without<MobInstance>>,
    mut mobs: Query<&mut MobInstance>,
    mut sounds: ResMut<SoundQueue>,
    mut particles: ResMut<ParticleQueue>,
    mut ui: ResMut<EditorUiState>,
    inventory: Res<crate::gameplay::inventory::Inventory>,
) {
    let pending: Vec<MobStrike> = strikes.pending.drain(..).collect();
    // Mobs never hurt the player in creative or spectator.
    if !pie.is_playing() || !inventory.survival || inventory.spectator {
        return;
    }
    let Ok((mut player, transform)) = players.single_mut() else {
        return;
    };

    for strike in pending {
        if strike.origin.distance(transform.translation) > strike.range + 0.6 {
            continue;
        }
        let mut away = transform.translation - strike.origin;
        away.y = 0.0;
        let away = away.normalize_or_zero();

        // Dash i-frames.
        if player.dash_timer > 0.0 {
            continue;
        }
        // Perfect parry.
        if combat.parry_timer > 0.0 {
            combat.parry_timer = 0.0;
            combat.parries += 1;
            stats.stamina = (stats.stamina + 1.5).min(stats.max_stamina);
            if let Ok(mut mob) = mobs.get_mut(strike.attacker) {
                mob.stun = 1.2;
                mob.knockback += -away * 6.0 + Vec3::Y * 2.0;
            }
            sounds.play_kind(SoundKind::Parry);
            particles.burst(ParticleBurst::new(
                transform.translation + Vec3::Y * 0.5 - away * 0.6,
                Vec3::new(1.0, 1.0, 0.7),
                14,
                ParticleStyle::Spark,
            ));
            ui.action("Parry!");
            continue;
        }
        let damage = if combat.blocking { strike.damage * 0.5 } else { strike.damage };
        let taken = stats.damage(damage);
        if taken > 0.0 {
            player.knockback = away * 4.5 + Vec3::Y * 3.0;
            sounds.play_kind(SoundKind::Hurt);
            particles.burst(ParticleBurst::new(
                transform.translation,
                Vec3::new(0.9, 0.2, 0.2),
                8,
                ParticleStyle::Spark,
            ));
        }
    }
}

fn draw_combat_feedback(mut gizmos: Gizmos, combat: Res<CombatState>) {
    if let Some((from, to, _)) = combat.bolt {
        gizmos.line(from, to, Color::srgb(1.0, 0.35, 0.95));
        gizmos.line(from + Vec3::Y * 0.02, to, Color::srgb(1.0, 0.8, 1.0));
    }
}
