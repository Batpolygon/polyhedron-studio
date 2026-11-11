//! Ambience workspace: sounds, music, particles, sky and weather, all made
//! here. Nothing is imported.

use egui::{pos2, vec2, Color32, Rect, Sense, Stroke};

use crate::audio::sound_events::{queue_named_sound, sound_names};
use crate::audio::synth::{self, bank, set_bank, Song, SongNote, SongTime, SongTrack, SoundBank, SynthLayer, SynthSound, Wave};
use crate::core::app_state::EditorUiState;
use crate::core::project_io::ProjectLayout;
use crate::editor::pickers::{self, Kind};
use crate::editor::theme;
use crate::gameplay::atmosphere::{atmosphere, set_atmosphere, Atmosphere, ParticleDef, SkyKey, WeatherDef};

fn slider(ui: &mut egui::Ui, label: &str, value: &mut f32, range: std::ops::RangeInclusive<f32>) -> bool {
    ui.add(egui::Slider::new(value, range).text(label)).changed()
}

fn selected(ui: &egui::Ui, key: &str, length: usize) -> usize {
    let index: usize = ui.ctx().data_mut(|d| d.get_temp(egui::Id::new(key))).unwrap_or(0);
    index.min(length.saturating_sub(1))
}

fn select(ui: &egui::Ui, key: &str, index: usize) {
    ui.ctx().data_mut(|d| d.insert_temp(egui::Id::new(key), index));
}

/// Names of a list as selectable rows, with Add and Delete.
fn name_list<T>(ui: &mut egui::Ui, key: &str, items: &mut Vec<T>, name: impl Fn(&T) -> &str, fresh: impl Fn() -> T) -> bool {
    let mut changed = false;
    let current = selected(ui, key, items.len());
    ui.horizontal_wrapped(|ui| {
        for (index, item) in items.iter().enumerate() {
            if ui.selectable_label(index == current, name(item)).clicked() {
                select(ui, key, index);
            }
        }
        if ui.small_button("+").on_hover_text("New").clicked() {
            items.push(fresh());
            select(ui, key, items.len() - 1);
            changed = true;
        }
        if !items.is_empty() && ui.small_button("🗙").on_hover_text("Delete").clicked() {
            items.remove(current);
            changed = true;
        }
    });
    changed
}

fn waveform(ui: &mut egui::Ui, sound: &SynthSound) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width().min(420.0), 70.0), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 3.0, theme::BG_ELEVATED);
    let samples = synth::render(sound, 0);
    let columns = rect.width() as usize;
    if samples.is_empty() || columns == 0 {
        return;
    }
    for column in 0..columns {
        let start = column * samples.len() / columns;
        let end = ((column + 1) * samples.len() / columns).max(start + 1).min(samples.len());
        let peak = samples[start..end].iter().fold(0.0_f32, |peak, sample| peak.max(sample.abs()));
        let x = rect.left() + column as f32;
        let half = peak * rect.height() * 0.48;
        painter.line_segment([pos2(x, rect.center().y - half), pos2(x, rect.center().y + half)], Stroke::new(1.0, theme::ACCENT));
    }
}

fn layer_editor(ui: &mut egui::Ui, layer: &mut SynthLayer) -> bool {
    let mut changed = false;
    egui::ComboBox::from_id_salt("wave").selected_text(layer.wave.label()).show_ui(ui, |ui| {
        for wave in Wave::ALL {
            changed |= ui.selectable_value(&mut layer.wave, wave, wave.label()).changed();
        }
    });
    changed |= slider(ui, "Start pitch (Hz)", &mut layer.from_hz, 20.0..=8000.0);
    changed |= slider(ui, "End pitch (Hz)", &mut layer.to_hz, 20.0..=8000.0);
    changed |= slider(ui, "Attack (s)", &mut layer.attack, 0.0..=2.0);
    changed |= slider(ui, "Decay (s)", &mut layer.decay, 0.01..=6.0);
    changed |= slider(ui, "Volume", &mut layer.gain, 0.0..=1.0);
    changed |= slider(ui, "Low-pass (Hz, 0 = off)", &mut layer.lowpass, 0.0..=12000.0);
    changed |= slider(ui, "Vibrato speed", &mut layer.vibrato_hz, 0.0..=30.0);
    changed |= slider(ui, "Vibrato depth", &mut layer.vibrato_depth, 0.0..=200.0);
    changed
}

fn sound_editor(ui: &mut egui::Ui, sound: &mut SynthSound) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label("Name");
        changed |= ui.text_edit_singleline(&mut sound.name).changed();
        if ui.button("▶ Play").clicked() {
            queue_named_sound(&sound.name);
        }
    });
    waveform(ui, sound);
    let mut remove = None;
    for (index, layer) in sound.layers.iter_mut().enumerate() {
        ui.push_id(index, |ui| {
            egui::CollapsingHeader::new(format!("Layer {}", index + 1)).default_open(index == 0).show(ui, |ui| {
                changed |= layer_editor(ui, layer);
                if ui.small_button("Delete the layer").clicked() {
                    remove = Some(index);
                }
            });
        });
    }
    if let Some(index) = remove {
        sound.layers.remove(index);
        changed = true;
    }
    if ui.small_button("+ layer").clicked() {
        sound.layers.push(SynthLayer::default());
        changed = true;
    }
    changed
}

/// The notes of a track as a grid: steps across, semitones up.
fn note_grid(ui: &mut egui::Ui, track: &mut SongTrack, steps: u32) -> bool {
    const ROWS: i32 = 13;
    let cell = vec2(12.0, 9.0);
    let size = vec2(cell.x * steps as f32, cell.y * ROWS as f32);
    let mut changed = false;
    egui::ScrollArea::horizontal().id_salt("notes").show(ui, |ui| {
        let (rect, response) = ui.allocate_exact_size(size, Sense::click());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 2.0, theme::BG_ELEVATED);
        for step in 0..=steps {
            let x = rect.left() + step as f32 * cell.x;
            let color = if step % 4 == 0 { theme::BORDER } else { Color32::from_white_alpha(10) };
            painter.line_segment([pos2(x, rect.top()), pos2(x, rect.bottom())], Stroke::new(1.0, color));
        }
        for note in &track.notes {
            let row = (ROWS - 1 - note.semitone).clamp(0, ROWS - 1) as f32;
            let min = rect.min + vec2(note.step as f32 * cell.x, row * cell.y);
            painter.rect_filled(Rect::from_min_size(min, cell - vec2(1.0, 1.0)), 1.0, theme::ACCENT);
        }
        if response.clicked() {
            if let Some(pointer) = response.interact_pointer_pos() {
                let offset = pointer - rect.min;
                let step = (offset.x / cell.x) as u32;
                let semitone = ROWS - 1 - (offset.y / cell.y) as i32;
                let note = SongNote { step, semitone };
                match track.notes.iter().position(|existing| *existing == note) {
                    Some(index) => {
                        track.notes.remove(index);
                    }
                    None => track.notes.push(note),
                }
                changed = true;
            }
        }
    });
    changed
}

fn song_editor(ui: &mut egui::Ui, song: &mut Song, sounds: &[SynthSound]) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label("Name");
        changed |= ui.text_edit_singleline(&mut song.name).changed();
        changed |= ui.checkbox(&mut song.enabled, "Plays in game").changed();
        let playing = synth::preview().as_deref() == Some(song.name.as_str());
        if ui.button(if playing { "⏹ Stop" } else { "▶ Play" }).clicked() {
            synth::set_preview(if playing { None } else { Some(song.name.clone()) });
        }
    });
    changed |= slider(ui, "Tempo (BPM)", &mut song.bpm, 30.0..=240.0);
    changed |= ui.add(egui::Slider::new(&mut song.steps, 4..=128).text("Steps")).changed();
    ui.horizontal(|ui| {
        ui.label("When");
        egui::ComboBox::from_id_salt("song_time").selected_text(song.time.label()).show_ui(ui, |ui| {
            for time in SongTime::ALL {
                changed |= ui.selectable_value(&mut song.time, time, time.label()).changed();
            }
        });
    });
    ui.horizontal(|ui| {
        ui.label("Biomes");
        changed |= pickers::pick_list(ui, "song_biomes", &mut song.biomes, &[Kind::Biome]);
    });
    let steps = song.steps;
    let mut remove = None;
    for (index, track) in song.tracks.iter_mut().enumerate() {
        ui.push_id(index, |ui| {
            ui.horizontal(|ui| {
                egui::ComboBox::from_id_salt("track_sound").selected_text(track.sound.as_str()).show_ui(ui, |ui| {
                    for sound in sounds {
                        if ui.selectable_label(track.sound == sound.name, sound.name.as_str()).clicked() {
                            track.sound.clone_from(&sound.name);
                            changed = true;
                        }
                    }
                });
                changed |= ui.add(egui::Slider::new(&mut track.volume, 0.0..=1.0).text("Volume")).changed();
                if ui.small_button("🗙").clicked() {
                    remove = Some(index);
                }
            });
            changed |= note_grid(ui, track, steps);
        });
    }
    if let Some(index) = remove {
        song.tracks.remove(index);
        changed = true;
    }
    if ui.small_button("+ track").clicked() {
        let sound = sounds.first().map(|sound| sound.name.clone()).unwrap_or_default();
        song.tracks.push(SongTrack { sound, volume: 0.6, notes: Vec::new() });
        changed = true;
    }
    changed
}

/// Tab: the sounds and the songs of the project.
pub fn show_audio(ui: &mut egui::Ui, state: &mut EditorUiState, project: &ProjectLayout) {
    let mut data: SoundBank = (*bank()).clone();
    let mut changed = false;
    ui.horizontal(|ui| {
        if ui.button("💾 Save").on_hover_text("Write project/audio.ron").clicked() {
            match data.save(project) {
                Ok(()) => state.action("Sounds saved to project/audio.ron."),
                Err(error) => state.error(error),
            }
        }
        theme::tip(ui, "Sounds are built from oscillators: no file is imported.");
    });
    egui::ScrollArea::vertical().id_salt("audio_scroll").show(ui, |ui| {
        theme::section_label(ui, "SOUNDS");
        changed |= name_list(ui, "audio_sound", &mut data.sounds, |sound| &sound.name, || SynthSound {
            name: "sound".to_string(),
            layers: vec![SynthLayer::default()],
        });
        let current = selected(ui, "audio_sound", data.sounds.len());
        if let Some(sound) = data.sounds.get_mut(current) {
            changed |= sound_editor(ui, sound);
        }
        ui.separator();
        theme::section_label(ui, "MUSIC");
        changed |= name_list(ui, "audio_song", &mut data.songs, |song| &song.name, Song::default);
        let current = selected(ui, "audio_song", data.songs.len());
        let sounds = data.sounds.clone();
        if let Some(song) = data.songs.get_mut(current) {
            changed |= song_editor(ui, song, &sounds);
        }
    });
    if changed {
        set_bank(data);
    }
}

/// Moving dots that follow the same laws as the game's particles.
fn particle_preview(ui: &mut egui::Ui, def: &ParticleDef) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width().min(320.0), 180.0), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 3.0, Color32::from_rgb(16, 20, 28));
    let scale = 24.0;
    let origin = rect.center() + vec2(0.0, 20.0);
    let period = def.life.max(0.05);
    let time = ui.input(|input| input.time) as f32 % (period * 1.5);
    let color = Color32::from_rgb(def.color[0], def.color[1], def.color[2]);
    for index in 0..def.count.min(64) {
        let hash = |salt: u32| ((index.wrapping_mul(2_654_435_761) ^ salt.wrapping_mul(40_503)).wrapping_mul(2_166_136_261) >> 16 & 0xFFFF) as f32 / 65_535.0 - 0.5;
        let direction = vec2(hash(1), hash(2)).normalized() * def.speed * (0.5 + hash(3).abs());
        if time > period {
            continue;
        }
        let x = direction.x * time;
        let y = (direction.y + def.lift) * time - 0.5 * def.gravity * time * time;
        let at = origin + vec2(x, -y) * scale;
        let fade = 1.0 - time / period;
        painter.rect_filled(Rect::from_center_size(at, vec2(1.0, 1.0) * (def.size * scale * 2.0).max(1.5) * (0.4 + 0.6 * fade)), 0.0, color);
    }
    ui.ctx().request_repaint();
}

fn color_u8(ui: &mut egui::Ui, label: &str, color: &mut [u8; 3]) -> bool {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.color_edit_button_srgb(color).changed()
    })
    .inner
}

fn color_f32(ui: &mut egui::Ui, label: &str, color: &mut [f32; 3]) -> bool {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.color_edit_button_rgb(color).changed()
    })
    .inner
}

/// Tab: the particle effects.
pub fn show_particles(ui: &mut egui::Ui, state: &mut EditorUiState, project: &ProjectLayout) {
    let mut data: Atmosphere = (*atmosphere()).clone();
    let mut changed = false;
    if ui.button("💾 Save").on_hover_text("Write project/atmosphere.ron").clicked() {
        match data.save(project) {
            Ok(()) => state.action("Effects saved to project/atmosphere.ron."),
            Err(error) => state.error(error),
        }
    }
    changed |= name_list(ui, "particle", &mut data.particles, |def| &def.name, ParticleDef::default);
    let current = selected(ui, "particle", data.particles.len());
    if let Some(def) = data.particles.get_mut(current) {
        particle_preview(ui, def);
        ui.horizontal(|ui| {
            ui.label("Name");
            changed |= ui.text_edit_singleline(&mut def.name).changed();
        });
        changed |= color_u8(ui, "Colour", &mut def.color);
        changed |= ui.checkbox(&mut def.glow, "Glows").changed();
        changed |= ui.add(egui::Slider::new(&mut def.count, 1..=64).text("Count")).changed();
        changed |= slider(ui, "Speed", &mut def.speed, 0.0..=30.0);
        changed |= slider(ui, "Lift", &mut def.lift, -20.0..=20.0);
        changed |= slider(ui, "Gravity", &mut def.gravity, -20.0..=40.0);
        changed |= slider(ui, "Life (s)", &mut def.life, 0.05..=6.0);
        changed |= slider(ui, "Size", &mut def.size, 0.01..=0.6);
        theme::tip(ui, "Use it with the Weave block Spawn particle, by name.");
    }
    if changed {
        set_atmosphere(data);
    }
}

fn weather_editor(ui: &mut egui::Ui, weather: &mut WeatherDef) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label("Name");
        changed |= ui.text_edit_singleline(&mut weather.name).changed();
    });
    ui.horizontal(|ui| {
        ui.label("Biomes");
        changed |= pickers::pick_list(ui, "weather_biomes", &mut weather.biomes, &[Kind::Biome]);
    });
    changed |= slider(ui, "Chance per check", &mut weather.chance, 0.0..=1.0);
    changed |= slider(ui, "Duration (s)", &mut weather.duration, 5.0..=900.0);
    changed |= color_u8(ui, "Drops", &mut weather.color);
    changed |= ui.checkbox(&mut weather.glow, "Glows").changed();
    changed |= slider(ui, "Drops per second", &mut weather.rate, 0.0..=400.0);
    changed |= slider(ui, "Fall speed", &mut weather.fall_speed, 0.5..=40.0);
    changed |= slider(ui, "Drop size", &mut weather.size, 0.01..=0.3);
    changed |= color_f32(ui, "Sky tint", &mut weather.sky_tint);
    changed |= slider(ui, "Tint strength", &mut weather.tint, 0.0..=1.0);
    changed |= slider(ui, "Sunlight kept", &mut weather.light, 0.0..=1.0);
    changed
}

/// Tab: sky, clouds and weather.
pub fn show_sky(ui: &mut egui::Ui, state: &mut EditorUiState, project: &ProjectLayout) {
    let mut data: Atmosphere = (*atmosphere()).clone();
    let mut changed = false;
    if ui.button("💾 Save").on_hover_text("Write project/atmosphere.ron").clicked() {
        match data.save(project) {
            Ok(()) => state.action("Sky saved to project/atmosphere.ron."),
            Err(error) => state.error(error),
        }
    }
    egui::ScrollArea::vertical().id_salt("sky_scroll").show(ui, |ui| {
        theme::section_label(ui, "SKY GRADIENT");
        theme::tip(ui, "No key: the sky follows the biome. 0 = midnight, 0.5 = noon.");
        let mut remove = None;
        for (index, key) in data.sky.keys.iter_mut().enumerate() {
            ui.push_id(index, |ui| {
                ui.horizontal(|ui| {
                    changed |= ui.add(egui::DragValue::new(&mut key.time).range(0.0..=1.0).speed(0.005)).changed();
                    changed |= ui.color_edit_button_rgb(&mut key.zenith).on_hover_text("Top of the sky").changed();
                    changed |= ui.color_edit_button_rgb(&mut key.horizon).on_hover_text("Horizon").changed();
                    if ui.small_button("🗙").clicked() {
                        remove = Some(index);
                    }
                });
            });
        }
        if let Some(index) = remove {
            data.sky.keys.remove(index);
            changed = true;
        }
        if ui.small_button("+ key").clicked() {
            data.sky.keys.push(SkyKey { time: 0.5, zenith: [0.28, 0.5, 0.82], horizon: [0.75, 0.86, 0.97] });
            changed = true;
        }
        ui.separator();
        theme::section_label(ui, "SUN, MOON AND STARS");
        changed |= color_f32(ui, "Sun", &mut data.sky.sun_color);
        changed |= slider(ui, "Sun size", &mut data.sky.sun_size, 0.1..=4.0);
        changed |= color_f32(ui, "Moon", &mut data.sky.moon_color);
        changed |= slider(ui, "Moon size", &mut data.sky.moon_size, 0.1..=4.0);
        changed |= slider(ui, "Stars", &mut data.sky.stars, 0.0..=1.0);
        ui.separator();
        theme::section_label(ui, "CLOUDS");
        changed |= ui.checkbox(&mut data.sky.clouds, "Clouds").changed();
        changed |= slider(ui, "Height", &mut data.sky.cloud_height, 30.0..=200.0);
        changed |= slider(ui, "Wind", &mut data.sky.cloud_speed, 0.0..=12.0);
        ui.separator();
        theme::section_label(ui, "WEATHER");
        changed |= name_list(ui, "weather", &mut data.weather, |weather| &weather.name, WeatherDef::default);
        let current = selected(ui, "weather", data.weather.len());
        if let Some(weather) = data.weather.get_mut(current) {
            changed |= weather_editor(ui, weather);
        }
    });
    if changed {
        set_atmosphere(data);
    }
}

/// Sounds usable by a block or a mob.
pub fn sound_field(ui: &mut egui::Ui, salt: &str, sound: &mut Option<String>) -> bool {
    let mut changed = false;
    egui::ComboBox::from_id_salt(salt).selected_text(sound.clone().unwrap_or_else(|| "default".to_string())).show_ui(ui, |ui| {
        if ui.selectable_label(sound.is_none(), "default").clicked() {
            *sound = None;
            changed = true;
        }
        for name in sound_names() {
            if ui.selectable_label(sound.as_deref() == Some(name.as_str()), name.as_str()).clicked() {
                *sound = Some(name);
                changed = true;
            }
        }
    });
    changed
}
