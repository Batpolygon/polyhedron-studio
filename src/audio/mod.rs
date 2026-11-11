//! Procedural audio.
//!
//! No audio file ships with the engine:
//! - [`dsp_engine`] renders footsteps, mining cracks, lasers, engines, UI bips
//!   and wind from oscillators, noise, filters and envelopes;
//! - [`sound_events`] queues requests from gameplay and Blockly and streams
//!   the rendered buffers to the output device on a dedicated thread.

pub mod dsp_engine;
pub mod sound_events;
pub mod synth;

use bevy::prelude::*;

use crate::audio::sound_events::{AudioBridge, SoundQueue};

/// Mixer volumes, edited from the console / inspector.
#[derive(Resource, Debug, Clone)]
pub struct AudioSettings {
    pub master_volume: f32,
    pub music_volume: f32,
    pub sfx_volume: f32,
    pub ui_volume: f32,
    pub footsteps: bool,
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self {
            master_volume: 0.8,
            music_volume: 0.6,
            sfx_volume: 0.9,
            ui_volume: 0.7,
            footsteps: true,
        }
    }
}

pub struct AudioPlugin;

impl Plugin for AudioPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AudioSettings>()
            .init_resource::<SoundQueue>()
            .insert_resource(AudioBridge::start())
            .add_plugins(synth::SynthPlugin)
            .add_systems(PostUpdate, sound_events::flush_sound_queue);
    }
}
