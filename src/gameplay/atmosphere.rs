//! Atmosphere data: particle effects, sky, clouds and weather, all made in the
//! editor (`project/atmosphere.ron`).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, RwLock};

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::core::pie_manager::PieState;
use crate::core::project_io::{load_ron, save_ron, ProjectLayout};
use crate::gameplay::world_rules::WorldRules;
use crate::physics::player_controller::Player;
use crate::renderer::particles::{ParticleBurst, ParticleQueue, ParticleShape, ParticleStyle};
use crate::scripting::context::{emit_game_event, GameEvent};
use crate::voxel::chunk_manager::WorldStreaming;

const ATMOSPHERE_FILE: &str = "atmosphere.ron";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ParticleDef {
    pub name: String,
    pub color: [u8; 3],
    pub count: u32,
    pub speed: f32,
    /// Upward speed added to every particle.
    pub lift: f32,
    pub gravity: f32,
    pub life: f32,
    pub size: f32,
    pub glow: bool,
}

impl Default for ParticleDef {
    fn default() -> Self {
        Self { name: "Effect".to_string(), color: [255, 220, 120], count: 16, speed: 4.0, lift: 1.0, gravity: 6.0, life: 0.6, size: 0.06, glow: true }
    }
}

impl ParticleDef {
    pub fn shape(&self) -> ParticleShape {
        ParticleShape { speed: self.speed, lift: self.lift, gravity: self.gravity, life: self.life, size: self.size, glow: self.glow, fragile: false }
    }

    pub fn burst(&self, position: Vec3) -> ParticleBurst {
        let color = Vec3::new(self.color[0] as f32, self.color[1] as f32, self.color[2] as f32) / 255.0;
        ParticleBurst { shape: Some(self.shape()), ..ParticleBurst::new(position, color, self.count, ParticleStyle::Spark) }
    }
}

/// Sky colours at a time of day (0 = midnight, 0.5 = noon).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SkyKey {
    pub time: f32,
    pub zenith: [f32; 3],
    pub horizon: [f32; 3],
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SkyConfig {
    /// Empty = the sky follows the biome.
    pub keys: Vec<SkyKey>,
    pub sun_color: [f32; 3],
    pub sun_size: f32,
    pub moon_color: [f32; 3],
    pub moon_size: f32,
    /// Brightness of the stars (0 hides them).
    pub stars: f32,
    pub clouds: bool,
    pub cloud_height: f32,
    pub cloud_speed: f32,
}

impl Default for SkyConfig {
    fn default() -> Self {
        Self {
            keys: Vec::new(),
            sun_color: [1.0, 0.97, 0.82],
            sun_size: 1.0,
            moon_color: [0.9, 0.93, 1.0],
            moon_size: 1.0,
            stars: 1.0,
            clouds: true,
            cloud_height: 78.0,
            cloud_speed: 1.6,
        }
    }
}

impl SkyConfig {
    /// Zenith and horizon colours at `time`, when the sky has keys.
    pub fn sample(&self, time: f32) -> Option<(Vec3, Vec3)> {
        let first = self.keys.first()?;
        let mut before = first;
        let mut after = first;
        for key in &self.keys {
            if key.time <= time && (before.time > time || key.time >= before.time) {
                before = key;
            }
        }
        for key in &self.keys {
            if key.time > time && (after.time <= time || key.time < after.time) {
                after = key;
            }
        }
        if before.time > time {
            before = self.keys.iter().fold(first, |best, key| if key.time > best.time { key } else { best });
        }
        if after.time <= time {
            after = self.keys.iter().fold(first, |best, key| if key.time < best.time { key } else { best });
        }
        let span = (after.time - before.time).rem_euclid(1.0);
        let progress = if span > 1e-4 { (time - before.time).rem_euclid(1.0) / span } else { 0.0 };
        let mix = |a: [f32; 3], b: [f32; 3]| Vec3::from_array(a).lerp(Vec3::from_array(b), progress);
        Some((mix(before.zenith, after.zenith), mix(before.horizon, after.horizon)))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WeatherDef {
    pub name: String,
    /// Empty = every biome.
    pub biomes: Vec<String>,
    /// Chance it starts at every check.
    pub chance: f32,
    pub duration: f32,
    pub color: [u8; 3],
    /// Particles per second.
    pub rate: f32,
    pub fall_speed: f32,
    pub size: f32,
    pub glow: bool,
    pub sky_tint: [f32; 3],
    /// How far the sky turns to the tint (0 to 1).
    pub tint: f32,
    /// Sunlight kept (0 to 1).
    pub light: f32,
}

impl Default for WeatherDef {
    fn default() -> Self {
        Self {
            name: "Rain".to_string(),
            biomes: Vec::new(),
            chance: 0.15,
            duration: 90.0,
            color: [130, 160, 220],
            rate: 90.0,
            fall_speed: 16.0,
            size: 0.05,
            glow: false,
            sky_tint: [0.42, 0.46, 0.52],
            tint: 0.65,
            light: 0.55,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Atmosphere {
    #[serde(default)]
    pub particles: Vec<ParticleDef>,
    #[serde(default)]
    pub sky: SkyConfig,
    #[serde(default)]
    pub weather: Vec<WeatherDef>,
}

impl Atmosphere {
    pub fn starter() -> Self {
        Self {
            particles: vec![ParticleDef::default()],
            sky: SkyConfig::default(),
            weather: vec![
                WeatherDef::default(),
                WeatherDef {
                    name: "Snow".to_string(),
                    biomes: vec!["Frozen Lake".to_string(), "Ice Floe".to_string(), "Mountain".to_string()],
                    color: [240, 244, 255],
                    rate: 60.0,
                    fall_speed: 2.5,
                    size: 0.07,
                    sky_tint: [0.75, 0.8, 0.86],
                    tint: 0.5,
                    light: 0.8,
                    ..WeatherDef::default()
                },
                WeatherDef {
                    name: "Storm".to_string(),
                    chance: 0.05,
                    duration: 60.0,
                    color: [110, 130, 190],
                    rate: 160.0,
                    fall_speed: 24.0,
                    sky_tint: [0.2, 0.22, 0.3],
                    tint: 0.85,
                    light: 0.3,
                    ..WeatherDef::default()
                },
            ],
        }
    }

    pub fn path(project: &ProjectLayout) -> std::path::PathBuf {
        project.root.join(ATMOSPHERE_FILE)
    }

    pub fn save(&self, project: &ProjectLayout) -> Result<(), String> {
        save_ron(&Self::path(project), self)
    }
}

static DATA: LazyLock<RwLock<Arc<Atmosphere>>> = LazyLock::new(|| RwLock::new(Arc::new(Atmosphere::starter())));
static REVISION: AtomicU64 = AtomicU64::new(1);

pub fn atmosphere() -> Arc<Atmosphere> {
    DATA.read().unwrap_or_else(|poisoned| poisoned.into_inner()).clone()
}

pub fn set_atmosphere(data: Atmosphere) {
    *DATA.write().unwrap_or_else(|poisoned| poisoned.into_inner()) = Arc::new(data);
    REVISION.fetch_add(1, Ordering::Relaxed);
}

/// Changes whenever the atmosphere is edited.
pub fn atmosphere_revision() -> u64 {
    REVISION.load(Ordering::Relaxed)
}

/// The weather of the moment.
#[derive(Resource, Debug, Default)]
pub struct WeatherState {
    pub current: Option<String>,
    left: f32,
    check: f32,
    pending: f32,
    roll: u32,
}

impl WeatherState {
    /// Force a weather (empty = clear sky), from Weave.
    pub fn force(&mut self, name: &str, data: &Atmosphere) {
        let found = data.weather.iter().find(|weather| weather.name == name);
        self.left = found.map_or(120.0, |weather| weather.duration);
        self.check = self.left;
        self.current = found.map(|weather| weather.name.clone());
        emit_game_event(GameEvent::WeatherChanged, name, 0.0, None);
    }

    fn random(&mut self) -> f32 {
        self.roll = self.roll.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        ((self.roll >> 8) & 0xFFFF) as f32 / 65_535.0
    }
}

pub struct AtmospherePlugin;

impl Plugin for AtmospherePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<WeatherState>()
            .add_systems(Startup, load_atmosphere.after(crate::core::project_io::initialize_project_system))
            .add_systems(Update, run_weather);
    }
}

fn load_atmosphere(project: Res<ProjectLayout>, mut ui: ResMut<crate::core::app_state::EditorUiState>) {
    let path = Atmosphere::path(&project);
    if !path.is_file() {
        if let Err(error) = Atmosphere::starter().save(&project) {
            ui.warn(error);
        }
        return;
    }
    match load_ron::<Atmosphere>(&path) {
        Ok(loaded) => set_atmosphere(loaded),
        Err(error) => ui.warn(format!("Atmosphere: {error}")),
    }
}

fn run_weather(
    time: Res<Time>,
    pie: Res<PieState>,
    streaming: Res<WorldStreaming>,
    players: Query<&Transform, With<Player>>,
    mut rules: ResMut<WorldRules>,
    mut particles: ResMut<ParticleQueue>,
    mut state: ResMut<WeatherState>,
) {
    if !pie.is_playing() {
        *state = WeatherState::default();
        rules.weather_tint = 0.0;
        rules.weather_light = 1.0;
        return;
    }
    let Ok(transform) = players.single() else { return };
    let at = transform.translation;
    let delta = time.delta_secs();
    let data = atmosphere();

    state.check -= delta;
    state.left -= delta;
    if state.current.is_some() && state.left <= 0.0 {
        state.current = None;
        emit_game_event(GameEvent::WeatherChanged, "", 0.0, None);
    }
    if state.current.is_none() && state.check <= 0.0 {
        state.check = 45.0;
        let biome = streaming.generator.as_ref().map(|generator| generator.biome_name(generator.biome_at(at.x as i32, at.z as i32)).to_ascii_lowercase()).unwrap_or_default();
        for weather in &data.weather {
            let here = weather.biomes.is_empty() || weather.biomes.iter().any(|name| name == "*" || biome.contains(&name.to_ascii_lowercase()));
            if here && state.random() < weather.chance {
                state.current = Some(weather.name.clone());
                state.left = weather.duration;
                emit_game_event(GameEvent::WeatherChanged, &weather.name, 0.0, None);
                break;
            }
        }
    }

    let active = state.current.as_ref().and_then(|name| data.weather.iter().find(|weather| weather.name == *name));
    let (tint, light, color) = active.map_or((0.0, 1.0, Vec3::ZERO), |weather| (weather.tint, weather.light, Vec3::from_array(weather.sky_tint)));
    let blend = (delta * 0.6).min(1.0);
    rules.weather_tint += (tint - rules.weather_tint) * blend;
    rules.weather_light += (light - rules.weather_light) * blend;
    if active.is_some() {
        rules.weather_color = color;
    }

    if let Some(weather) = active {
        state.pending += weather.rate * delta;
        let count = state.pending as u32;
        if count > 0 {
            state.pending -= count as f32;
            let color = Vec3::new(weather.color[0] as f32, weather.color[1] as f32, weather.color[2] as f32) / 255.0;
            let shape = ParticleShape {
                speed: 0.2,
                lift: -weather.fall_speed,
                gravity: 0.0,
                life: 3.0,
                size: weather.size,
                glow: weather.glow,
                fragile: true,
            };
            particles.burst(ParticleBurst {
                shape: Some(shape),
                scatter: 44.0,
                ..ParticleBurst::new(at + Vec3::Y * 16.0, color, count, ParticleStyle::Smoke)
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(time: f32, value: f32) -> SkyKey {
        SkyKey { time, zenith: [value; 3], horizon: [value; 3] }
    }

    #[test]
    fn sky_keys_blend_and_wrap_around_midnight() {
        let sky = SkyConfig { keys: vec![key(0.25, 0.0), key(0.75, 1.0)], ..SkyConfig::default() };
        assert!((sky.sample(0.5).unwrap().0.x - 0.5).abs() < 1e-3);
        assert!((sky.sample(0.0).unwrap().0.x - 0.5).abs() < 1e-3);
        assert!((sky.sample(0.25).unwrap().0.x).abs() < 1e-3);
        assert!(SkyConfig::default().sample(0.5).is_none());
    }

    #[test]
    fn the_starter_round_trips() {
        let data = Atmosphere::starter();
        let text = ron::ser::to_string_pretty(&data, ron::ser::PrettyConfig::default()).unwrap();
        assert_eq!(data, ron::from_str::<Atmosphere>(&text).unwrap());
    }
}
