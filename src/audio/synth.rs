//! Sound designer data: every sound and song is made in the editor from
//! oscillators; nothing is imported.
//!
//! A [`SynthSound`] stacks layers (wave, pitch sweep, envelope, filter). A
//! [`Song`] is a step sequencer whose tracks play sounds at a pitch; it starts
//! by itself in its biomes and at its time of day. `project/audio.ron`.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex, RwLock};

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::audio::sound_events::{SoundBus, SoundQueue};
use crate::core::pie_manager::PieState;
use crate::core::project_io::{load_ron, save_ron, ProjectLayout};
use crate::gameplay::world_rules::WorldRules;
use crate::physics::player_controller::Player;
use crate::voxel::chunk_manager::WorldStreaming;

pub const SAMPLE_RATE: f32 = 44_100.0;
const AUDIO_FILE: &str = "audio.ron";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Wave {
    #[default]
    Sine,
    Square,
    Saw,
    Triangle,
    Noise,
}

impl Wave {
    pub const ALL: [Self; 5] = [Self::Sine, Self::Square, Self::Saw, Self::Triangle, Self::Noise];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Sine => "Sine",
            Self::Square => "Square",
            Self::Saw => "Saw",
            Self::Triangle => "Triangle",
            Self::Noise => "Noise",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SynthLayer {
    pub wave: Wave,
    /// Pitch at the start and at the end of the layer, in Hz.
    pub from_hz: f32,
    pub to_hz: f32,
    pub attack: f32,
    pub decay: f32,
    pub gain: f32,
    /// Low-pass cut-off in Hz (0 = off).
    #[serde(default)]
    pub lowpass: f32,
    #[serde(default)]
    pub vibrato_hz: f32,
    /// Vibrato depth in Hz.
    #[serde(default)]
    pub vibrato_depth: f32,
}

impl Default for SynthLayer {
    fn default() -> Self {
        Self { wave: Wave::Sine, from_hz: 440.0, to_hz: 440.0, attack: 0.005, decay: 0.3, gain: 0.8, lowpass: 0.0, vibrato_hz: 0.0, vibrato_depth: 0.0 }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SynthSound {
    pub name: String,
    pub layers: Vec<SynthLayer>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SongNote {
    pub step: u32,
    /// Semitones above the sound's own pitch.
    pub semitone: i32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SongTrack {
    pub sound: String,
    pub volume: f32,
    pub notes: Vec<SongNote>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SongTime {
    #[default]
    Any,
    Day,
    Night,
}

impl SongTime {
    pub const ALL: [Self; 3] = [Self::Any, Self::Day, Self::Night];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Any => "Any time",
            Self::Day => "By day",
            Self::Night => "By night",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Song {
    pub name: String,
    /// Plays by itself during the game.
    pub enabled: bool,
    pub bpm: f32,
    /// Steps of the loop (four steps per beat).
    pub steps: u32,
    /// Empty = every biome.
    #[serde(default)]
    pub biomes: Vec<String>,
    #[serde(default)]
    pub time: SongTime,
    pub tracks: Vec<SongTrack>,
}

impl Default for Song {
    fn default() -> Self {
        Self { name: "Song".to_string(), enabled: false, bpm: 100.0, steps: 32, biomes: Vec::new(), time: SongTime::Any, tracks: Vec::new() }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SoundBank {
    #[serde(default)]
    pub sounds: Vec<SynthSound>,
    #[serde(default)]
    pub songs: Vec<Song>,
}

impl Default for SoundBank {
    fn default() -> Self {
        let note = |step, semitone| SongNote { step, semitone };
        Self {
            sounds: vec![
                SynthSound {
                    name: "bell".to_string(),
                    layers: vec![
                        SynthLayer { from_hz: 660.0, to_hz: 660.0, decay: 0.9, gain: 0.6, ..SynthLayer::default() },
                        SynthLayer { from_hz: 1980.0, to_hz: 1980.0, decay: 0.4, gain: 0.2, ..SynthLayer::default() },
                    ],
                },
                SynthSound {
                    name: "pad".to_string(),
                    layers: vec![SynthLayer { wave: Wave::Triangle, from_hz: 220.0, to_hz: 220.0, attack: 0.15, decay: 1.2, gain: 0.5, lowpass: 900.0, vibrato_hz: 4.0, vibrato_depth: 2.0 }],
                },
                SynthSound {
                    name: "kick".to_string(),
                    layers: vec![SynthLayer { from_hz: 150.0, to_hz: 45.0, attack: 0.001, decay: 0.22, gain: 1.0, ..SynthLayer::default() }],
                },
                SynthSound {
                    name: "hat".to_string(),
                    layers: vec![SynthLayer { wave: Wave::Noise, attack: 0.001, decay: 0.05, gain: 0.3, ..SynthLayer::default() }],
                },
            ],
            songs: vec![Song {
                name: "Calm fields".to_string(),
                tracks: vec![
                    SongTrack { sound: "pad".to_string(), volume: 0.5, notes: vec![note(0, 0), note(8, 3), note(16, 5), note(24, 3)] },
                    SongTrack { sound: "bell".to_string(), volume: 0.35, notes: vec![note(2, 0), note(6, 7), note(12, 3), note(20, 5), note(28, 7)] },
                ],
                ..Song::default()
            }],
        }
    }
}

impl SoundBank {
    pub fn path(project: &ProjectLayout) -> std::path::PathBuf {
        project.root.join(AUDIO_FILE)
    }

    pub fn save(&self, project: &ProjectLayout) -> Result<(), String> {
        save_ron(&Self::path(project), self)
    }
}

static BANK: LazyLock<RwLock<Arc<SoundBank>>> = LazyLock::new(|| RwLock::new(Arc::new(SoundBank::default())));
type Rendered = Arc<Vec<f32>>;
static CACHE: LazyLock<Mutex<HashMap<(String, i32), Rendered>>> = LazyLock::new(|| Mutex::new(HashMap::new()));
static PREVIEW: Mutex<Option<String>> = Mutex::new(None);

pub fn bank() -> Arc<SoundBank> {
    BANK.read().unwrap_or_else(|poisoned| poisoned.into_inner()).clone()
}

pub fn set_bank(bank: SoundBank) {
    *BANK.write().unwrap_or_else(|poisoned| poisoned.into_inner()) = Arc::new(bank);
    CACHE.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clear();
    REVISION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}

static REVISION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// Changes whenever the bank is edited.
pub fn bank_revision() -> u64 {
    REVISION.load(std::sync::atomic::Ordering::Relaxed)
}

/// Start or stop the preview of a song, from the editor.
pub fn set_preview(song: Option<String>) {
    *PREVIEW.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = song;
}

pub fn preview() -> Option<String> {
    PREVIEW.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone()
}

/// Length of a sound, in seconds.
pub fn sound_length(sound: &SynthSound) -> f32 {
    sound.layers.iter().map(|layer| layer.attack + layer.decay).fold(0.0, f32::max).clamp(0.01, 8.0)
}

/// Render a sound, shifted by some semitones.
pub fn render(sound: &SynthSound, semitone: i32) -> Vec<f32> {
    let length = (sound_length(sound) * SAMPLE_RATE) as usize;
    let shift = 2.0_f32.powf(semitone as f32 / 12.0);
    let mut out = vec![0.0_f32; length];
    for layer in &sound.layers {
        let mut phase = 0.0_f32;
        let mut noise = 0x2545_F491_u32;
        let mut filtered = 0.0_f32;
        let alpha = if layer.lowpass > 0.0 {
            1.0 - (-std::f32::consts::TAU * layer.lowpass / SAMPLE_RATE).exp()
        } else {
            1.0
        };
        let span = (layer.attack + layer.decay).max(0.001);
        for (index, slot) in out.iter_mut().enumerate() {
            let t = index as f32 / SAMPLE_RATE;
            if t > span {
                break;
            }
            let progress = t / span;
            let vibrato = (t * layer.vibrato_hz * std::f32::consts::TAU).sin() * layer.vibrato_depth;
            let hz = (layer.from_hz + (layer.to_hz - layer.from_hz) * progress + vibrato) * shift;
            phase = (phase + hz / SAMPLE_RATE).fract();
            let raw = match layer.wave {
                Wave::Sine => (phase * std::f32::consts::TAU).sin(),
                Wave::Square => if phase < 0.5 { 1.0 } else { -1.0 },
                Wave::Saw => phase * 2.0 - 1.0,
                Wave::Triangle => 1.0 - 4.0 * (phase - 0.5).abs(),
                Wave::Noise => {
                    noise ^= noise << 13;
                    noise ^= noise >> 17;
                    noise ^= noise << 5;
                    noise as f32 / u32::MAX as f32 * 2.0 - 1.0
                }
            };
            filtered += (raw - filtered) * alpha;
            let envelope = if t < layer.attack {
                t / layer.attack.max(1e-4)
            } else {
                (1.0 - (t - layer.attack) / layer.decay.max(1e-4)).max(0.0).powi(2)
            };
            *slot += filtered * envelope * layer.gain;
        }
    }
    let peak = out.iter().fold(0.0_f32, |peak, sample| peak.max(sample.abs()));
    if peak > 0.95 {
        let scale = 0.95 / peak;
        out.iter_mut().for_each(|sample| *sample *= scale);
    }
    out
}

/// The samples of a sound of the bank (cached).
pub fn samples(name: &str, semitone: i32) -> Option<Rendered> {
    let key = (name.to_string(), semitone);
    let mut cache = CACHE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(found) = cache.get(&key) {
        return Some(found.clone());
    }
    let bank = bank();
    let sound = bank.sounds.iter().find(|sound| sound.name == name)?;
    let rendered = Arc::new(render(sound, semitone));
    cache.insert(key, rendered.clone());
    Some(rendered)
}

/// The song that fits a place and a time.
pub fn choose_song<'a>(songs: &'a [Song], biome: &str, day: bool) -> Option<&'a Song> {
    songs.iter().find(|song| {
        song.enabled
            && (song.biomes.is_empty() || song.biomes.iter().any(|name| name == "*" || name.eq_ignore_ascii_case(biome)))
            && match song.time {
                SongTime::Any => true,
                SongTime::Day => day,
                SongTime::Night => !day,
            }
    })
}

/// Plays the current song, one step at a time: nothing long is buffered, so
/// it stops at once.
#[derive(Resource, Debug, Default)]
pub struct MusicDirector {
    current: Option<String>,
    step: u32,
    timer: f32,
}

pub struct SynthPlugin;

impl Plugin for SynthPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MusicDirector>()
            .add_systems(Startup, load_bank.after(crate::core::project_io::initialize_project_system))
            .add_systems(Update, run_music);
    }
}

fn load_bank(project: Res<ProjectLayout>, mut ui: ResMut<crate::core::app_state::EditorUiState>) {
    let path = SoundBank::path(&project);
    if !path.is_file() {
        if let Err(error) = SoundBank::default().save(&project) {
            ui.warn(error);
        }
        return;
    }
    match load_ron::<SoundBank>(&path) {
        Ok(loaded) => set_bank(loaded),
        Err(error) => ui.warn(format!("Audio: {error}")),
    }
}

fn run_music(
    time: Res<Time<Real>>,
    pie: Res<PieState>,
    rules: Res<WorldRules>,
    streaming: Res<WorldStreaming>,
    players: Query<&Transform, With<Player>>,
    mut queue: ResMut<SoundQueue>,
    mut director: ResMut<MusicDirector>,
) {
    let bank = bank();
    let wanted = preview().or_else(|| {
        if !pie.is_playing() {
            return None;
        }
        let at = players.single().ok()?.translation;
        let generator = streaming.generator.as_ref()?;
        let biome = generator.biome_name(generator.biome_at(at.x as i32, at.z as i32));
        choose_song(&bank.songs, biome, rules.daylight() > 0.3).map(|song| song.name.clone())
    });
    if director.current != wanted {
        director.current = wanted;
        director.step = 0;
        director.timer = 0.0;
    }
    let Some(song) = director.current.as_ref().and_then(|name| bank.songs.iter().find(|song| song.name == *name)) else {
        return;
    };
    let step_time = 15.0 / song.bpm.max(1.0);
    director.timer -= time.delta_secs();
    if director.timer > 0.0 {
        return;
    }
    director.timer += step_time;
    let step = director.step;
    for track in &song.tracks {
        for note in track.notes.iter().filter(|note| note.step == step) {
            if let Some(samples) = samples(&track.sound, note.semitone) {
                queue.play_samples(samples, track.volume, SoundBus::Music);
            }
        }
    }
    director.step = (step + 1) % song.steps.max(1);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sound_renders_for_its_whole_length_and_stays_in_range() {
        let bank = SoundBank::default();
        for sound in &bank.sounds {
            let samples = render(sound, 0);
            assert_eq!(samples.len(), (sound_length(sound) * SAMPLE_RATE) as usize);
            assert!(samples.iter().all(|sample| sample.abs() <= 1.0));
            assert!(samples.iter().any(|sample| sample.abs() > 0.01), "{} is silent", sound.name);
        }
    }

    #[test]
    fn songs_follow_their_biome_and_time() {
        let song = |name: &str, biomes: &[&str], time| Song { name: name.to_string(), enabled: true, biomes: biomes.iter().map(|b| b.to_string()).collect(), time, ..Song::default() };
        let songs = [song("a", &["Forest"], SongTime::Any), song("b", &[], SongTime::Night)];
        assert_eq!(choose_song(&songs, "Forest", true).unwrap().name, "a");
        assert_eq!(choose_song(&songs, "Desert", false).unwrap().name, "b");
        assert!(choose_song(&songs, "Desert", true).is_none());
    }

    #[test]
    fn the_bank_round_trips() {
        let bank = SoundBank::default();
        let text = ron::ser::to_string_pretty(&bank, ron::ser::PrettyConfig::default()).unwrap();
        assert_eq!(bank, ron::from_str::<SoundBank>(&text).unwrap());
    }
}
