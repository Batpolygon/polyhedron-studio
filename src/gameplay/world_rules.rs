//! World rules: time of day, sun, sky colour and fog.
//!
//! The clock is driven by Blockly (`SetTimeOfDay`, see the "Day Night"
//! starter program) and read back by sensors, solar panels and mob spawning.
//! While authoring, the sun stays at the studio angle; during PIE it follows
//! the clock and the clear colour fades between day and night. Biomes tint
//! the sky through [`WorldRules::biome_sky`].

use std::f32::consts::{PI, TAU};

use bevy::prelude::*;

use crate::core::editor_camera::EditorCamera;
use crate::core::pie_manager::{PieOnly, PieSet, PieState};
use crate::voxel::chunk_manager::WorldStreaming;

#[derive(Resource, Debug, Clone)]
pub struct WorldRules {
    /// 0 = midnight, 0.25 = sunrise, 0.5 = noon, 0.75 = sunset.
    pub time_of_day: f32,
    /// Advance the clock automatically (when no script drives it).
    pub auto_cycle: bool,
    /// Seconds for a full day when `auto_cycle` is on.
    pub day_length: f32,
    /// Sky colour of the biome under the player.
    pub biome_sky: Vec3,
    /// Set when a script wrote the clock this frame.
    pub scripted: bool,
    /// Weather: how far the sky turns to `weather_color` (0 to 1).
    pub weather_tint: f32,
    pub weather_color: Vec3,
    /// Weather: sunlight kept (0 to 1).
    pub weather_light: f32,
}

impl Default for WorldRules {
    fn default() -> Self {
        Self {
            time_of_day: 0.35,
            auto_cycle: true,
            day_length: 240.0,
            biome_sky: Vec3::new(0.46, 0.66, 0.86),
            scripted: false,
            weather_tint: 0.0,
            weather_color: Vec3::ONE,
            weather_light: 1.0,
        }
    }
}

impl WorldRules {
    /// Daylight factor in `[0, 1]`.
    pub fn daylight(&self) -> f32 {
        let sun = (self.time_of_day * TAU - PI * 0.5).sin();
        (sun * 1.6 + 0.25).clamp(0.0, 1.0)
    }
}

/// Marker for the main directional light driven by the clock.
#[derive(Component)]
pub struct SunLight;

#[derive(Resource, Debug, Default)]
struct StudioLook {
    saved: Option<(Transform, f32, Color)>,
}

pub struct WorldRulesPlugin;

impl Plugin for WorldRulesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<WorldRules>()
            .init_resource::<StudioLook>()
            .add_systems(Update, (tag_sun, advance_clock, cave_light).chain().after(PieSet::Transition));
    }
}

/// The light the player carries: it fades in under the surface, so the great
/// halls can be seen. Set in the Biomes panel (Caves).
#[derive(Component)]
struct CaveLight;

fn cave_light(
    mut commands: Commands,
    pie: Res<PieState>,
    streaming: Res<WorldStreaming>,
    cameras: Query<&Transform, (With<EditorCamera>, Without<CaveLight>)>,
    mut lights: Query<(&mut Transform, &mut PointLight), With<CaveLight>>,
) {
    let (Some(generator), Ok(camera)) = (streaming.generator.as_ref().filter(|_| pie.is_playing()), cameras.single()) else {
        return;
    };
    let cave = &generator.settings.cave;
    let position = camera.translation;
    let surface = generator.column(position.x.floor() as i32, position.z.floor() as i32).height as f32;
    let depth = ((surface - 4.0 - position.y) / 10.0).clamp(0.0, 1.0);
    let [red, green, blue] = cave.explorer_color;
    let color = Color::srgb(red, green, blue);
    let Ok((mut transform, mut light)) = lights.single_mut() else {
        commands.spawn((
            PointLight { color, intensity: 0.0, range: cave.explorer_range, shadow_maps_enabled: false, ..default() },
            Transform::from_translation(position),
            CaveLight,
            PieOnly,
        ));
        return;
    };
    // Overhead, so the hands and the nearby walls do not burn out.
    transform.translation = position + Vec3::Y * 6.0;
    light.color = color;
    light.range = cave.explorer_range;
    light.intensity = cave.explorer_light * depth;
}

fn tag_sun(
    mut commands: Commands,
    lights: Query<Entity, (With<DirectionalLight>, Without<SunLight>)>,
    suns: Query<(), With<SunLight>>,
) {
    if !suns.is_empty() {
        return;
    }
    if let Some(entity) = lights.iter().next() {
        commands.entity(entity).insert(SunLight);
    }
}

fn advance_clock(
    time: Res<Time>,
    pie: Res<PieState>,
    mut rules: ResMut<WorldRules>,
    mut studio: ResMut<StudioLook>,
    mut clear: ResMut<ClearColor>,
    mut suns: Query<(&mut Transform, &mut DirectionalLight), With<SunLight>>,
    mut was_day: Local<Option<bool>>,
) {
    let is_day = rules.daylight() > 0.5;
    if pie.is_playing() && was_day.is_some_and(|before| before != is_day) {
        use crate::scripting::context::{emit_game_event, GameEvent};
        emit_game_event(if is_day { GameEvent::DayStarted } else { GameEvent::NightStarted }, "", rules.time_of_day, None);
    }
    *was_day = Some(is_day);
    let Ok((mut transform, mut light)) = suns.single_mut() else {
        return;
    };

    if pie.started_this_frame {
        studio.saved = Some((*transform, light.illuminance, clear.0));
        rules.time_of_day = WorldRules::default().time_of_day;
    }
    if pie.stopped_this_frame {
        if let Some((saved, illuminance, color)) = studio.saved.take() {
            *transform = saved;
            light.illuminance = illuminance;
            clear.0 = color;
        }
        return;
    }
    if !pie.is_playing() {
        return;
    }

    if rules.auto_cycle && !rules.scripted {
        rules.time_of_day =
            (rules.time_of_day + time.delta_secs() / rules.day_length.max(1.0)).rem_euclid(1.0);
    }
    rules.scripted = false;

    // Sun orbit around the X axis; below the horizon it becomes moonlight.
    let angle = rules.time_of_day * TAU - PI * 0.5;
    let direction = Vec3::new(0.35, angle.sin(), angle.cos()).normalize();
    let daylight = rules.daylight();
    let up = if direction.y > 0.0 { direction } else { -direction };
    *transform = Transform::from_translation(up * 40.0).looking_at(Vec3::ZERO, Vec3::Y);
    light.illuminance = (800.0 + 13_000.0 * daylight) * rules.weather_light;
    light.color = Color::srgb(
        0.75 + 0.25 * daylight,
        0.78 + 0.2 * daylight,
        0.95 + 0.05 * daylight,
    );

    let night = Vec3::new(0.02, 0.03, 0.07);
    let sky = night.lerp(rules.biome_sky, daylight).lerp(rules.weather_color * (0.25 + 0.75 * daylight), rules.weather_tint);
    clear.0 = Color::srgb(sky.x, sky.y, sky.z);
}
