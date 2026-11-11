//! Cubic crumble particles.
//!
//! Any system can push a [`ParticleBurst`] into [`ParticleQueue`]; the spawner
//! turns it into small unlit cubes with velocity, gravity, spin and a lifetime.
//! Mining cracks, block breaks, hits, sparks and machine smoke all use it.

use std::collections::HashMap;

use bevy::prelude::*;

use crate::core::pie_manager::PieOnly;

/// Hard cap on live particles.
const MAX_PARTICLES: usize = 1_500;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParticleStyle {
    /// Heavy chunks that fall and bounce (mining, breaking).
    Crumble,
    /// Fast bright sparks (hits, energy, cables).
    Spark,
    /// Slow rising puffs (combustor, dash).
    Smoke,
}

impl ParticleStyle {
    /// Script names: `crumble`, `spark`, `hit`, `smoke`, `dust`.
    pub fn from_name(name: &str) -> Self {
        match name {
            "spark" | "hit" | "energy" => Self::Spark,
            "smoke" | "dust" | "puff" => Self::Smoke,
            _ => Self::Crumble,
        }
    }
}

/// The laws of an effect made in the editor (instead of a style's).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ParticleShape {
    pub speed: f32,
    pub lift: f32,
    pub gravity: f32,
    pub life: f32,
    pub size: f32,
    pub glow: bool,
    /// Vanishes on the first solid block (rain).
    pub fragile: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct ParticleBurst {
    pub position: Vec3,
    pub color: Vec3,
    pub count: u32,
    pub style: ParticleStyle,
    /// Initial velocity bias (e.g. hit direction).
    pub bias: Vec3,
    pub shape: Option<ParticleShape>,
    /// Horizontal size of the area the particles appear in.
    pub scatter: f32,
}

impl ParticleBurst {
    pub fn new(position: Vec3, color: Vec3, count: u32, style: ParticleStyle) -> Self {
        Self {
            position,
            color,
            count,
            style,
            bias: Vec3::ZERO,
            shape: None,
            scatter: 0.0,
        }
    }
}

#[derive(Resource, Debug, Default)]
pub struct ParticleQueue {
    pending: Vec<ParticleBurst>,
    seed: u32,
}

impl ParticleQueue {
    pub fn burst(&mut self, burst: ParticleBurst) {
        if self.pending.len() < 64 {
            self.pending.push(burst);
        }
    }

    fn random(&mut self) -> f32 {
        self.seed = self.seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        ((self.seed >> 8) & 0xFFFF) as f32 / 65_535.0
    }
}

#[derive(Component, Debug)]
pub struct Particle {
    pub velocity: Vec3,
    pub spin: Vec3,
    pub life: f32,
    pub max_life: f32,
    pub gravity: f32,
    pub size: f32,
    pub fragile: bool,
}

#[derive(Resource, Default)]
struct ParticleAssets {
    mesh: Option<Handle<Mesh>>,
    materials: HashMap<[u8; 4], Handle<StandardMaterial>>,
}

pub struct ParticlePlugin;

impl Plugin for ParticlePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ParticleQueue>()
            .init_resource::<ParticleAssets>()
            .add_systems(Update, (spawn_particles, simulate_particles).chain());
    }
}

fn spawn_particles(
    mut commands: Commands,
    mut queue: ResMut<ParticleQueue>,
    mut assets: ResMut<ParticleAssets>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    live: Query<(), With<Particle>>,
) {
    if queue.pending.is_empty() {
        return;
    }
    let mesh = assets
        .mesh
        .get_or_insert_with(|| meshes.add(Cuboid::new(1.0, 1.0, 1.0)))
        .clone();
    let mut budget = MAX_PARTICLES.saturating_sub(live.iter().count());
    let bursts: Vec<ParticleBurst> = queue.pending.drain(..).collect();

    for burst in bursts {
        let glow = burst.shape.map_or(burst.style == ParticleStyle::Spark, |shape| shape.glow);
        let unlit = burst.shape.is_some() || burst.style != ParticleStyle::Crumble;
        let key = [
            (burst.color.x.clamp(0.0, 1.0) * 255.0) as u8 & 0xF8,
            (burst.color.y.clamp(0.0, 1.0) * 255.0) as u8 & 0xF8,
            (burst.color.z.clamp(0.0, 1.0) * 255.0) as u8 & 0xF8,
            u8::from(glow) | u8::from(unlit) << 1,
        ];
        let material = assets
            .materials
            .entry(key)
            .or_insert_with(|| {
                let color = Color::srgb_u8(key[0], key[1], key[2]);
                materials.add(StandardMaterial {
                    base_color: color,
                    emissive: if glow {
                        LinearRgba::from(color) * 3.0
                    } else {
                        LinearRgba::BLACK
                    },
                    unlit,
                    ..default()
                })
            })
            .clone();

        for _ in 0..burst.count.min(64) {
            if budget == 0 {
                return;
            }
            budget -= 1;
            let jitter = Vec3::new(
                queue.random() - 0.5,
                queue.random() - 0.5,
                queue.random() - 0.5,
            );
            let spot = burst.position + Vec3::new(queue.random() - 0.5, 0.0, queue.random() - 0.5) * burst.scatter;
            let (velocity, gravity, life, size) = if let Some(shape) = burst.shape {
                (jitter.normalize_or_zero() * shape.speed * (0.5 + queue.random()) + Vec3::Y * shape.lift, shape.gravity, shape.life * (0.7 + queue.random() * 0.6), shape.size)
            } else {
                match burst.style {
                ParticleStyle::Crumble => (
                    Vec3::new(jitter.x * 4.0, 2.0 + queue.random() * 3.0, jitter.z * 4.0),
                    18.0,
                    0.6 + queue.random() * 0.5,
                    0.08 + queue.random() * 0.08,
                ),
                ParticleStyle::Spark => (
                    jitter.normalize_or_zero() * (5.0 + queue.random() * 6.0),
                    6.0,
                    0.25 + queue.random() * 0.25,
                    0.04 + queue.random() * 0.03,
                ),
                ParticleStyle::Smoke => (
                    Vec3::new(jitter.x * 0.8, 1.0 + queue.random(), jitter.z * 0.8),
                    -0.6,
                    1.0 + queue.random() * 0.8,
                    0.12 + queue.random() * 0.1,
                ),
                }
            };
            commands.spawn((
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material.clone()),
                Transform::from_translation(spot + jitter * 0.6)
                    .with_scale(Vec3::splat(size)),
                Particle {
                    velocity: velocity + burst.bias,
                    spin: jitter * 12.0,
                    life,
                    max_life: life,
                    gravity,
                    size,
                    fragile: burst.shape.is_some_and(|shape| shape.fragile),
                },
                PieOnly,
            ));
        }
    }
}

fn simulate_particles(
    mut commands: Commands,
    time: Res<Time>,
    world: Res<crate::voxel::world::VoxelWorld>,
    mut particles: Query<(Entity, &mut Particle, &mut Transform)>,
) {
    let delta = time.delta_secs().min(0.05);
    for (entity, mut particle, mut transform) in &mut particles {
        particle.life -= delta;
        if particle.life <= 0.0 {
            commands.entity(entity).despawn();
            continue;
        }
        particle.velocity.y -= particle.gravity * delta;
        let next = transform.translation + particle.velocity * delta;
        // Bounce on solid voxels.
        let cell = next.floor().as_ivec3();
        if particle.fragile && world.get(cell).is_some() {
            commands.entity(entity).despawn();
            continue;
        }
        if world.get(cell).is_some() && particle.velocity.y < 0.0 {
            particle.velocity.y *= -0.35;
            particle.velocity.x *= 0.6;
            particle.velocity.z *= 0.6;
        } else {
            transform.translation = next;
        }
        let spin = particle.spin * delta;
        transform.rotate(Quat::from_euler(EulerRot::XYZ, spin.x, spin.y, spin.z));
        let fade = (particle.life / particle.max_life).clamp(0.0, 1.0);
        transform.scale = Vec3::splat(particle.size * (0.4 + 0.6 * fade));
    }
}
