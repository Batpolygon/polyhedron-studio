//! Sound events raised by the game or by Blockly, streamed to `rodio`.
//!
//! Gameplay systems push [`SoundRequest`]s into the [`SoundQueue`] resource.
//! A Bevy system forwards them over a channel to a dedicated audio thread that
//! owns the output device, renders each request with the DSP engine and adds
//! the buffer to the device mixer. If no audio device exists, requests are
//! dropped and the engine keeps running silently.

use std::thread;

use bevy::prelude::*;
use crossbeam_channel::{Receiver, Sender};

use crate::audio::dsp_engine::{self, SoundKind, SurfaceTone};
use crate::audio::AudioSettings;
use crate::voxel::blocks::SurfaceMaterial;

/// Names accepted by the Blockly `PlaySound` block.
pub const SOUND_NAMES: &[&str] = &[
    "ui_beep",
    "ui_click",
    "impact",
    "laser",
    "engine",
    "sword",
    "parry",
    "hurt",
    "pickup",
    "wind",
    "dash",
    "power_on",
    "power_off",
    "place",
    "break",
    "mine",
    "step",
];

/// Map a script sound name to a DSP recipe.
pub fn sound_from_name(name: &str) -> Option<SoundKind> {
    Some(match name {
        "ui_beep" => SoundKind::UiBeep,
        "ui_click" => SoundKind::UiClick,
        "impact" => SoundKind::Impact,
        "laser" => SoundKind::Laser,
        "engine" => SoundKind::Engine { rpm: 2200.0 },
        "sword" => SoundKind::SwordSwing,
        "parry" => SoundKind::Parry,
        "hurt" => SoundKind::Hurt,
        "pickup" => SoundKind::Pickup,
        "wind" => SoundKind::Wind,
        "dash" => SoundKind::Dash,
        "power_on" => SoundKind::PowerOn,
        "power_off" => SoundKind::PowerOff,
        "place" => SoundKind::BlockPlace,
        "break" => SoundKind::BlockBreak(SurfaceTone::Stone),
        "mine" => SoundKind::MineCrack(SurfaceTone::Stone),
        "step" => SoundKind::Footstep(SurfaceTone::Stone),
        _ => return None,
    })
}

/// DSP colour of a block surface.
pub fn surface_tone(material: SurfaceMaterial) -> SurfaceTone {
    match material {
        SurfaceMaterial::Stone => SurfaceTone::Stone,
        SurfaceMaterial::Dirt => SurfaceTone::Dirt,
        SurfaceMaterial::Grass => SurfaceTone::Grass,
        SurfaceMaterial::Sand => SurfaceTone::Sand,
        SurfaceMaterial::Wood => SurfaceTone::Wood,
        SurfaceMaterial::Metal => SurfaceTone::Metal,
        SurfaceMaterial::Glass => SurfaceTone::Glass,
        SurfaceMaterial::Ice => SurfaceTone::Ice,
    }
}

/// Category used for the mixer volumes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SoundBus {
    Sfx,
    Ui,
    Music,
}

static NAMED: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// Ask for a sound by name from anywhere (built-in or made in the editor).
pub fn queue_named_sound(name: &str) {
    let mut pending = NAMED.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if pending.len() < 32 {
        pending.push(name.to_string());
    }
}

/// Every sound name: built-in, then made in the editor.
pub fn sound_names() -> Vec<String> {
    let mut names: Vec<String> = SOUND_NAMES.iter().map(|name| name.to_string()).collect();
    names.extend(crate::audio::synth::bank().sounds.iter().map(|sound| sound.name.clone()));
    names
}

#[derive(Clone, Copy, Debug)]
pub struct SoundRequest {
    pub kind: SoundKind,
    pub volume: f32,
    pub pitch: f32,
    pub bus: SoundBus,
}

impl SoundRequest {
    pub fn sfx(kind: SoundKind) -> Self {
        Self {
            kind,
            volume: 1.0,
            pitch: 1.0,
            bus: SoundBus::Sfx,
        }
    }

    pub fn ui(kind: SoundKind) -> Self {
        Self {
            bus: SoundBus::Ui,
            ..Self::sfx(kind)
        }
    }

    pub fn with_volume(mut self, volume: f32) -> Self {
        self.volume = volume;
        self
    }

    pub fn with_pitch(mut self, pitch: f32) -> Self {
        self.pitch = pitch;
        self
    }
}

/// Requests raised this frame.
#[derive(Resource, Debug, Default)]
pub struct SoundQueue {
    pending: Vec<SoundRequest>,
    samples: Vec<(std::sync::Arc<Vec<f32>>, f32, SoundBus)>,
    pub played: u64,
}

impl SoundQueue {
    pub fn play(&mut self, request: SoundRequest) {
        // A burst of identical events in one frame would only clip.
        if self.pending.len() < 32 {
            self.pending.push(request);
        }
    }

    pub fn play_kind(&mut self, kind: SoundKind) {
        self.play(SoundRequest::sfx(kind));
    }

    /// Play rendered samples.
    pub fn play_samples(&mut self, samples: std::sync::Arc<Vec<f32>>, volume: f32, bus: SoundBus) {
        if self.samples.len() < 32 {
            self.samples.push((samples, volume, bus));
        }
    }

    /// Play a sound by name; false when no sound has this name.
    pub fn play_named(&mut self, name: &str) -> bool {
        if let Some(samples) = crate::audio::synth::samples(name, 0) {
            self.play_samples(samples, 1.0, SoundBus::Sfx);
            return true;
        }
        match sound_from_name(name) {
            Some(kind) => {
                self.play_kind(kind);
                true
            }
            None => false,
        }
    }
}

struct RenderJob {
    kind: SoundKind,
    gain: f32,
    pitch: f32,
    seed: u32,
    /// Samples made in the editor, instead of `kind`.
    samples: Option<std::sync::Arc<Vec<f32>>>,
}

/// Channel to the audio thread.
#[derive(Resource)]
pub struct AudioBridge {
    sender: Option<Sender<RenderJob>>,
    seed: u32,
}

impl AudioBridge {
    pub fn start() -> Self {
        let (sender, receiver) = crossbeam_channel::bounded::<RenderJob>(128);
        let spawned = thread::Builder::new()
            .name("polyhedron-audio".to_string())
            .spawn(move || audio_thread(receiver));
        match spawned {
            Ok(_) => Self {
                sender: Some(sender),
                seed: 0x1234_5678,
            },
            Err(error) => {
                warn!("audio thread could not start: {error}");
                Self {
                    sender: None,
                    seed: 1,
                }
            }
        }
    }

}

fn audio_thread(receiver: Receiver<RenderJob>) {
    let handle = match rodio::DeviceSinkBuilder::open_default_sink() {
        Ok(handle) => handle,
        Err(error) => {
            warn!("no audio output device ({error}); procedural audio is muted");
            // Drain so senders never block.
            while receiver.recv().is_ok() {}
            return;
        }
    };

    while let Ok(job) = receiver.recv() {
        let mut samples = match &job.samples {
            Some(samples) => samples.as_ref().clone(),
            None => dsp_engine::render(job.kind, job.pitch, job.seed),
        };
        for sample in &mut samples {
            *sample *= job.gain;
        }
        let buffer = rodio::buffer::SamplesBuffer::new(
            rodio::nz!(1),
            rodio::nz!(44_100),
            samples,
        );
        handle.mixer().add(buffer);
    }
}

/// Forward this frame's requests to the audio thread.
pub fn flush_sound_queue(
    mut queue: ResMut<SoundQueue>,
    mut bridge: ResMut<AudioBridge>,
    settings: Res<AudioSettings>,
) {
    for name in std::mem::take(&mut *NAMED.lock().unwrap_or_else(|poisoned| poisoned.into_inner())) {
        queue.play_named(&name);
    }
    if queue.pending.is_empty() && queue.samples.is_empty() {
        return;
    }
    let requests: Vec<SoundRequest> = queue.pending.drain(..).collect();
    let rendered: Vec<_> = queue.samples.drain(..).collect();
    let Some(sender) = bridge.sender.clone() else {
        return;
    };

    for request in requests {
        let bus = match request.bus {
            SoundBus::Sfx => settings.sfx_volume,
            SoundBus::Ui => settings.ui_volume,
            SoundBus::Music => settings.music_volume,
        };
        let gain = (settings.master_volume * bus * request.volume).clamp(0.0, 1.5);
        if gain <= 0.001 {
            continue;
        }
        bridge.seed = bridge.seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let job = RenderJob {
            kind: request.kind,
            gain,
            pitch: request.pitch,
            seed: bridge.seed,
            samples: None,
        };
        // A full channel means the device is far behind: drop the sound.
        if sender.try_send(job).is_ok() {
            queue.played += 1;
        }
    }
    for (samples, volume, bus) in rendered {
        let bus = match bus {
            SoundBus::Sfx => settings.sfx_volume,
            SoundBus::Ui => settings.ui_volume,
            SoundBus::Music => settings.music_volume,
        };
        let gain = (settings.master_volume * bus * volume).clamp(0.0, 1.5);
        let job = RenderJob { kind: SoundKind::UiClick, gain, pitch: 1.0, seed: 0, samples: Some(samples) };
        if gain > 0.001 && sender.try_send(job).is_ok() {
            queue.played += 1;
        }
    }
}
