//! Modular DSP synthesizer written in plain Rust.
//!
//! No audio file ships with the engine: every sound is rendered on demand from
//! oscillators, noise, filters and envelopes into a mono `f32` buffer. The
//! renderer is pure (no device access), so it is unit-tested and can run on
//! any thread; [`super::sound_events`] streams the buffers to `rodio`.

use std::f32::consts::TAU;

pub const SAMPLE_RATE: u32 = 44_100;

/// Surface families with distinct footstep / mining colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SurfaceTone {
    Stone,
    Dirt,
    Grass,
    Sand,
    Wood,
    Metal,
    Glass,
    Ice,
}

/// Every procedural sound the engine can render.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SoundKind {
    Footstep(SurfaceTone),
    MineCrack(SurfaceTone),
    BlockBreak(SurfaceTone),
    BlockPlace,
    Laser,
    /// Engine hum at a given RPM.
    Engine { rpm: f32 },
    UiBeep,
    UiClick,
    Impact,
    SwordSwing,
    Parry,
    Hurt,
    Pickup,
    Wind,
    Dash,
    PowerOn,
    PowerOff,
}

/// Small deterministic noise source.
struct Noise(u32);

impl Noise {
    fn next(&mut self) -> f32 {
        // xorshift32
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        (x as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
}

/// One-pole low-pass filter.
struct LowPass {
    state: f32,
    alpha: f32,
}

impl LowPass {
    fn new(cutoff: f32) -> Self {
        let mut filter = Self { state: 0.0, alpha: 0.0 };
        filter.set_cutoff(cutoff);
        filter
    }

    fn set_cutoff(&mut self, cutoff: f32) {
        let dt = 1.0 / SAMPLE_RATE as f32;
        let rc = 1.0 / (TAU * cutoff.max(10.0));
        self.alpha = dt / (rc + dt);
    }

    fn process(&mut self, input: f32) -> f32 {
        self.state += self.alpha * (input - self.state);
        self.state
    }
}

/// Band-pass made of a low-pass minus a lower low-pass.
struct BandPass {
    low: LowPass,
    high: LowPass,
}

impl BandPass {
    fn new(low_cut: f32, high_cut: f32) -> Self {
        Self {
            low: LowPass::new(high_cut),
            high: LowPass::new(low_cut),
        }
    }

    fn set(&mut self, low_cut: f32, high_cut: f32) {
        self.low.set_cutoff(high_cut);
        self.high.set_cutoff(low_cut);
    }

    fn process(&mut self, input: f32) -> f32 {
        let low = self.low.process(input);
        low - self.high.process(low)
    }
}

/// Attack / exponential-decay envelope.
fn envelope(t: f32, attack: f32, decay: f32) -> f32 {
    if t < attack {
        t / attack.max(1e-5)
    } else {
        (-(t - attack) / decay.max(1e-5)).exp()
    }
}

fn samples_for(seconds: f32) -> usize {
    (seconds * SAMPLE_RATE as f32) as usize
}

fn saw(phase: f32) -> f32 {
    2.0 * (phase - phase.floor()) - 1.0
}

fn square(phase: f32) -> f32 {
    if phase.fract() < 0.5 { 1.0 } else { -1.0 }
}

impl SurfaceTone {
    /// (low cut, high cut, decay) of the noise burst.
    fn colour(self) -> (f32, f32, f32) {
        match self {
            Self::Stone => (400.0, 2600.0, 0.045),
            Self::Dirt => (120.0, 900.0, 0.06),
            Self::Grass => (900.0, 5200.0, 0.07),
            Self::Sand => (1500.0, 7000.0, 0.09),
            Self::Wood => (180.0, 1400.0, 0.05),
            Self::Metal => (1200.0, 6500.0, 0.12),
            Self::Glass => (2500.0, 9000.0, 0.08),
            Self::Ice => (1800.0, 8000.0, 0.06),
        }
    }

    /// Resonant body frequency for hard materials.
    fn ring(self) -> Option<f32> {
        match self {
            Self::Metal => Some(1350.0),
            Self::Glass => Some(2900.0),
            Self::Wood => Some(240.0),
            Self::Stone => Some(520.0),
            Self::Ice => Some(2200.0),
            _ => None,
        }
    }
}

/// Render a sound. `pitch` scales every frequency (1.0 = nominal), `seed`
/// varies the noise so repeated footsteps never sound identical.
pub fn render(kind: SoundKind, pitch: f32, seed: u32) -> Vec<f32> {
    let pitch = pitch.clamp(0.25, 4.0);
    let mut noise = Noise(seed | 1);
    let sr = SAMPLE_RATE as f32;

    let mut out = match kind {
        SoundKind::Footstep(tone) => {
            let (low, high, decay) = tone.colour();
            let mut filter = BandPass::new(low * pitch, high * pitch);
            let length = samples_for(decay * 4.0);
            (0..length)
                .map(|i| {
                    let t = i as f32 / sr;
                    filter.process(noise.next()) * envelope(t, 0.003, decay) * 1.6
                })
                .collect()
        }
        SoundKind::MineCrack(tone) => {
            // Three percussive clicks with a short resonant body.
            let (low, high, _) = tone.colour();
            let mut filter = BandPass::new(low, high);
            let ring = tone.ring().unwrap_or(300.0) * pitch;
            let length = samples_for(0.16);
            (0..length)
                .map(|i| {
                    let t = i as f32 / sr;
                    let click = [0.0, 0.035, 0.07]
                        .iter()
                        .map(|start| {
                            let local = t - start;
                            if local >= 0.0 { envelope(local, 0.001, 0.012) } else { 0.0 }
                        })
                        .sum::<f32>();
                    let body = (TAU * ring * t).sin() * envelope(t, 0.001, 0.05) * 0.35;
                    filter.process(noise.next()) * click * 1.4 + body
                })
                .collect()
        }
        SoundKind::BlockBreak(tone) => {
            let (low, high, _) = tone.colour();
            let mut filter = BandPass::new(low * 0.6, high);
            let ring = tone.ring().unwrap_or(180.0) * pitch;
            let length = samples_for(0.35);
            (0..length)
                .map(|i| {
                    let t = i as f32 / sr;
                    let crumble = filter.process(noise.next()) * envelope(t, 0.002, 0.11);
                    let thump = (TAU * 70.0 * pitch * t).sin() * envelope(t, 0.001, 0.06);
                    let body = (TAU * ring * t).sin() * envelope(t, 0.001, 0.09) * 0.3;
                    crumble * 1.2 + thump * 0.6 + body
                })
                .collect()
        }
        SoundKind::BlockPlace => {
            let length = samples_for(0.12);
            let mut filter = LowPass::new(1800.0);
            (0..length)
                .map(|i| {
                    let t = i as f32 / sr;
                    let tone = (TAU * 210.0 * pitch * t).sin() * envelope(t, 0.002, 0.03);
                    tone * 0.7 + filter.process(noise.next()) * envelope(t, 0.001, 0.015) * 0.5
                })
                .collect()
        }
        SoundKind::Laser => {
            // Descending FM chirp.
            let length = samples_for(0.28);
            let mut phase = 0.0f32;
            (0..length)
                .map(|i| {
                    let t = i as f32 / sr;
                    let progress = t / 0.28;
                    let carrier = (1900.0 - 1650.0 * progress) * pitch;
                    let modulator = (TAU * carrier * 0.5 * t).sin() * 3.0;
                    phase += carrier / sr;
                    (TAU * phase + modulator).sin() * envelope(t, 0.002, 0.12) * 0.6
                })
                .collect()
        }
        SoundKind::Engine { rpm } => {
            // Sawtooth slaved to the RPM, with a low-passed combustion rumble.
            let base = (rpm.clamp(300.0, 9000.0) / 60.0) * 2.0 * pitch;
            let length = samples_for(0.18);
            let mut filter = LowPass::new(base * 6.0);
            let mut rumble = LowPass::new(160.0);
            let mut phase = 0.0f32;
            (0..length)
                .map(|i| {
                    let t = i as f32 / sr;
                    phase += base / sr;
                    let fade = (t / 0.02).min(1.0) * ((0.18 - t) / 0.02).clamp(0.0, 1.0);
                    (filter.process(saw(phase)) * 0.5 + rumble.process(noise.next()) * 0.8) * fade
                })
                .collect()
        }
        SoundKind::UiBeep => tone_burst(880.0 * pitch, 0.09, 0.003, 0.05, 0.45),
        SoundKind::UiClick => tone_burst(1500.0 * pitch, 0.03, 0.001, 0.008, 0.4),
        SoundKind::Impact => {
            let length = samples_for(0.3);
            let mut filter = LowPass::new(900.0);
            (0..length)
                .map(|i| {
                    let t = i as f32 / sr;
                    let drop = 110.0 * pitch * (1.0 - t * 1.5).max(0.3);
                    (TAU * drop * t).sin() * envelope(t, 0.001, 0.08) * 0.9
                        + filter.process(noise.next()) * envelope(t, 0.001, 0.03) * 0.8
                })
                .collect()
        }
        SoundKind::SwordSwing => {
            // Band-passed noise sweeping upward: a whoosh.
            let length = samples_for(0.22);
            let mut filter = BandPass::new(400.0, 1200.0);
            (0..length)
                .map(|i| {
                    let t = i as f32 / sr;
                    let progress = t / 0.22;
                    let centre = (500.0 + 2600.0 * progress) * pitch;
                    filter.set(centre * 0.6, centre * 1.4);
                    let shape = (progress * std::f32::consts::PI).sin();
                    filter.process(noise.next()) * shape * 1.8
                })
                .collect()
        }
        SoundKind::Parry => {
            // Inharmonic partials: metallic ring.
            let length = samples_for(0.6);
            let partials = [1.0, 2.76, 5.4, 8.93];
            (0..length)
                .map(|i| {
                    let t = i as f32 / sr;
                    partials
                        .iter()
                        .enumerate()
                        .map(|(index, ratio)| {
                            (TAU * 620.0 * pitch * ratio * t).sin()
                                * envelope(t, 0.001, 0.25 / (index as f32 + 1.0))
                                / (index as f32 + 1.0)
                        })
                        .sum::<f32>()
                        * 0.5
                })
                .collect()
        }
        SoundKind::Hurt => {
            let length = samples_for(0.25);
            let mut filter = LowPass::new(1400.0);
            (0..length)
                .map(|i| {
                    let t = i as f32 / sr;
                    let freq = (420.0 - 600.0 * t) * pitch;
                    filter.process(square(freq * t)) * envelope(t, 0.004, 0.09) * 0.45
                })
                .collect()
        }
        SoundKind::Pickup => {
            // Rising arpeggio: root, third, fifth.
            let notes = [660.0, 830.0, 990.0];
            let note_length = samples_for(0.06);
            let mut buffer = Vec::with_capacity(note_length * notes.len());
            for note in notes {
                buffer.extend(tone_burst(note * pitch, 0.06, 0.002, 0.03, 0.35));
            }
            buffer
        }
        SoundKind::Wind => {
            let length = samples_for(2.0);
            let mut filter = BandPass::new(200.0, 700.0);
            (0..length)
                .map(|i| {
                    let t = i as f32 / sr;
                    let gust = 0.6 + 0.4 * (TAU * 0.35 * t).sin();
                    filter.set(200.0 * gust, 900.0 * gust * pitch);
                    let fade = (t / 0.3).min(1.0) * ((2.0 - t) / 0.4).clamp(0.0, 1.0);
                    filter.process(noise.next()) * gust * fade * 1.6
                })
                .collect()
        }
        SoundKind::Dash => {
            let length = samples_for(0.15);
            let mut filter = BandPass::new(800.0, 4000.0);
            (0..length)
                .map(|i| {
                    let t = i as f32 / sr;
                    filter.process(noise.next()) * envelope(t, 0.005, 0.05) * 1.5
                })
                .collect()
        }
        SoundKind::PowerOn => sweep(220.0 * pitch, 660.0 * pitch, 0.25),
        SoundKind::PowerOff => sweep(660.0 * pitch, 160.0 * pitch, 0.3),
    };

    normalize(&mut out, 0.85);
    out
}

fn tone_burst(freq: f32, seconds: f32, attack: f32, decay: f32, gain: f32) -> Vec<f32> {
    let sr = SAMPLE_RATE as f32;
    (0..samples_for(seconds))
        .map(|i| {
            let t = i as f32 / sr;
            (TAU * freq * t).sin() * envelope(t, attack, decay) * gain
        })
        .collect()
}

fn sweep(from: f32, to: f32, seconds: f32) -> Vec<f32> {
    let sr = SAMPLE_RATE as f32;
    let mut phase = 0.0f32;
    (0..samples_for(seconds))
        .map(|i| {
            let t = i as f32 / sr;
            let progress = t / seconds;
            let freq = from + (to - from) * progress;
            phase += freq / sr;
            let fade = (t / 0.01).min(1.0) * ((seconds - t) / 0.05).clamp(0.0, 1.0);
            ((TAU * phase).sin() * 0.6 + (TAU * phase * 2.0).sin() * 0.2) * fade
        })
        .collect()
}

/// Scale the buffer so its peak equals `peak` (never amplifies silence).
fn normalize(buffer: &mut [f32], peak: f32) {
    let max = buffer.iter().fold(0.0f32, |max, sample| max.max(sample.abs()));
    if max > 1e-4 {
        let gain = peak / max;
        for sample in buffer.iter_mut() {
            *sample *= gain;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_sound_renders_finite_bounded_audio() {
        let kinds = [
            SoundKind::Footstep(SurfaceTone::Grass),
            SoundKind::MineCrack(SurfaceTone::Stone),
            SoundKind::BlockBreak(SurfaceTone::Wood),
            SoundKind::BlockPlace,
            SoundKind::Laser,
            SoundKind::Engine { rpm: 2400.0 },
            SoundKind::UiBeep,
            SoundKind::UiClick,
            SoundKind::Impact,
            SoundKind::SwordSwing,
            SoundKind::Parry,
            SoundKind::Hurt,
            SoundKind::Pickup,
            SoundKind::Wind,
            SoundKind::Dash,
            SoundKind::PowerOn,
            SoundKind::PowerOff,
        ];
        for kind in kinds {
            let buffer = render(kind, 1.0, 7);
            assert!(!buffer.is_empty(), "{kind:?} is empty");
            assert!(buffer.iter().all(|s| s.is_finite() && s.abs() <= 1.0), "{kind:?}");
        }
    }

    #[test]
    fn seeds_vary_noise_based_sounds() {
        let a = render(SoundKind::Footstep(SurfaceTone::Stone), 1.0, 1);
        let b = render(SoundKind::Footstep(SurfaceTone::Stone), 1.0, 2);
        assert_ne!(a, b);
    }
}
