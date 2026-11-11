//! Sky: a gradient dome, a square sun with a soft square halo, a square-pixel
//! crescent moon and a rotating star field.
//!
//! Everything follows the camera (so it never gets closer) and is tagged
//! [`WorldVisual`], which hides it in the studio workspaces. During play the
//! sky follows [`WorldRules::time_of_day`]; in the editor it shows the time set
//! in the World tab.

use std::f32::consts::{PI, TAU};

use bevy::asset::RenderAssetUsages;
use bevy::light::NotShadowCaster;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

use crate::core::editor_camera::EditorCamera;
use crate::editor::studio_common::WorldVisual;
use crate::gameplay::world_rules::WorldRules;

const DOME_RADIUS: f32 = 760.0;
const BODY_DISTANCE: f32 = 640.0;
const STAR_DISTANCE: f32 = 700.0;
const STAR_COUNT: usize = 900;
const DOME_SEGMENTS: usize = 32;
const DOME_RINGS: usize = 16;

#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum SkyPart {
    Dome,
    Sun,
    SunHalo,
    Moon,
    Stars,
}

#[derive(Resource, Default)]
struct SkyState {
    spawned: bool,
    dome_mesh: Option<Handle<Mesh>>,
    star_material: Option<Handle<StandardMaterial>>,
    halo_material: Option<Handle<StandardMaterial>>,
    moon_material: Option<Handle<StandardMaterial>>,
    sun_material: Option<Handle<StandardMaterial>>,
    /// Signature of the last dome colouring (time, biome sky, weather,
    /// edits) to skip rebuilds.
    last: Option<(i32, [i32; 3], i32, u64)>,
}

pub struct SkyPlugin;

impl Plugin for SkyPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SkyState>().add_systems(
            PostUpdate,
            update_sky
                .after(crate::physics::player_controller::follow_player_camera)
                .before(bevy::transform::TransformSystems::Propagate),
        );
    }
}

/// Direction of the sun for a time of day (same orbit as the sun light).
pub fn sun_direction(time_of_day: f32) -> Vec3 {
    let angle = time_of_day * TAU - PI * 0.5;
    Vec3::new(0.35, angle.sin(), angle.cos()).normalize()
}

fn unlit(materials: &mut Assets<StandardMaterial>, color: Color, blend: bool) -> Handle<StandardMaterial> {
    materials.add(StandardMaterial {
        base_color: color,
        unlit: true,
        fog_enabled: false,
        cull_mode: None,
        alpha_mode: if blend { AlphaMode::Blend } else { AlphaMode::Opaque },
        ..default()
    })
}

/// Flat square in the XY plane facing +Z, centred on the origin.
fn square(size: f32) -> Mesh {
    let h = size * 0.5;
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, vec![[-h, -h, 0.0], [h, -h, 0.0], [h, h, 0.0], [-h, h, 0.0]]);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 0.0, 1.0]; 4]);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]]);
    mesh.insert_indices(Indices::U32(vec![0, 1, 2, 0, 2, 3]));
    mesh
}

/// Pixel-art crescent: a disc minus an offset disc, one square per pixel.
fn crescent(size: f32) -> Mesh {
    const GRID: i32 = 12;
    let pixel = size / GRID as f32;
    let mut positions = Vec::new();
    let mut indices = Vec::new();
    let r = GRID as f32 * 0.5;
    for y in 0..GRID {
        for x in 0..GRID {
            let p = Vec2::new(x as f32 + 0.5 - r, y as f32 + 0.5 - r);
            let inside = p.length() <= r - 0.2;
            let bite = (p - Vec2::new(r * 0.55, r * 0.25)).length() <= r * 0.86;
            if !inside || bite {
                continue;
            }
            let x0 = (x as f32 - r) * pixel;
            let y0 = (y as f32 - r) * pixel;
            let base = positions.len() as u32;
            positions.extend([[x0, y0, 0.0], [x0 + pixel, y0, 0.0], [x0 + pixel, y0 + pixel, 0.0], [x0, y0 + pixel, 0.0]]);
            indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
        }
    }
    let count = positions.len();
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 0.0, 1.0]; count]);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0, 0.0]; count]);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

fn hash(index: u32, salt: u32) -> f32 {
    let mut h = index.wrapping_mul(0x9E37_79B9) ^ salt.wrapping_mul(0x85EB_CA6B);
    h ^= h >> 16;
    h = h.wrapping_mul(0x7FEB_352D);
    h ^= h >> 15;
    h = h.wrapping_mul(0x846C_A68B);
    h ^= h >> 16;
    (h & 0xFF_FFFF) as f32 / 16_777_215.0
}

/// Star field: small squares scattered on a sphere, each facing the centre.
fn star_field() -> Mesh {
    let mut positions = Vec::with_capacity(STAR_COUNT * 4);
    let mut colors = Vec::with_capacity(STAR_COUNT * 4);
    let mut indices = Vec::with_capacity(STAR_COUNT * 6);
    for i in 0..STAR_COUNT as u32 {
        let z = hash(i, 1) * 2.0 - 1.0;
        let a = hash(i, 2) * TAU;
        let s = (1.0 - z * z).sqrt();
        let dir = Vec3::new(s * a.cos(), z, s * a.sin());
        let size = 1.4 + hash(i, 3).powi(4) * 3.6;
        let bright = 0.55 + hash(i, 4) * 0.45;
        let tint = hash(i, 5);
        let color = [bright * (0.85 + 0.15 * tint), bright * 0.92, bright * (1.0 - 0.12 * tint), 1.0];
        let tangent = dir.any_orthonormal_vector();
        let bitangent = dir.cross(tangent);
        let center = dir * STAR_DISTANCE;
        let base = positions.len() as u32;
        for (u, v) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
            positions.push((center + (tangent * u + bitangent * v) * size * 0.5).to_array());
            colors.push(color);
        }
        indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    let count = positions.len();
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0]; count]);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0, 0.0]; count]);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

fn dome_directions() -> Vec<Vec3> {
    let mut directions = Vec::new();
    for ring in 0..=DOME_RINGS {
        let polar = ring as f32 / DOME_RINGS as f32 * PI;
        for segment in 0..=DOME_SEGMENTS {
            let azimuth = segment as f32 / DOME_SEGMENTS as f32 * TAU;
            directions.push(Vec3::new(polar.sin() * azimuth.cos(), polar.cos(), polar.sin() * azimuth.sin()));
        }
    }
    directions
}

fn dome_mesh() -> Mesh {
    let directions = dome_directions();
    let positions: Vec<[f32; 3]> = directions.iter().map(|d| (*d * DOME_RADIUS).to_array()).collect();
    let normals: Vec<[f32; 3]> = directions.iter().map(|d| (-*d).to_array()).collect();
    let mut indices = Vec::new();
    let row = DOME_SEGMENTS as u32 + 1;
    for ring in 0..DOME_RINGS as u32 {
        for segment in 0..DOME_SEGMENTS as u32 {
            let a = ring * row + segment;
            let b = a + row;
            indices.extend([a, a + 1, b, a + 1, b + 1, b]);
        }
    }
    let count = positions.len();
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0, 0.0]; count]);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![[1.0, 1.0, 1.0, 1.0]; count]);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

/// Sky colour seen in direction `dir` (linear-ish sRGB triplet).
pub fn sky_color(dir: Vec3, sun: Vec3, daylight: f32, biome_sky: Vec3) -> Vec3 {
    let zenith_day = biome_sky * Vec3::new(0.62, 0.78, 0.95);
    let horizon_day = biome_sky.lerp(Vec3::new(0.86, 0.93, 1.0), 0.6);
    let zenith_night = Vec3::new(0.008, 0.012, 0.035);
    let horizon_night = Vec3::new(0.03, 0.045, 0.1);
    let zenith = zenith_night.lerp(zenith_day, daylight);
    let horizon = horizon_night.lerp(horizon_day, daylight);
    blend_sky(dir, sun, daylight, zenith, horizon)
}

/// The gradient, with the sun's glow.
fn blend_sky(dir: Vec3, sun: Vec3, daylight: f32, zenith: Vec3, horizon: Vec3) -> Vec3 {
    let height = dir.y.max(0.0);
    let mut color = horizon.lerp(zenith, height.powf(0.55));

    // Sunrise / sunset glow around the sun when it is near the horizon.
    let twilight = (1.0 - (sun.y.abs() / 0.35)).clamp(0.0, 1.0);
    let toward_sun = dir.dot(sun).max(0.0);
    let glow = twilight * toward_sun.powf(3.0) * (1.0 - height).powf(2.0);
    color = color.lerp(Vec3::new(1.0, 0.52, 0.24), glow * 0.85);
    // A faint warm halo around the sun during the day.
    color += Vec3::new(0.25, 0.22, 0.14) * toward_sun.powf(24.0) * daylight;
    // Below the horizon: fade to a darker band so the void never shows.
    if dir.y < 0.0 {
        color *= 1.0 - (-dir.y).min(1.0) * 0.45;
    }
    color
}

#[allow(clippy::too_many_arguments)]
fn update_sky(
    mut commands: Commands,
    mut state: ResMut<SkyState>,
    rules: Res<WorldRules>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    cameras: Query<&Transform, (With<EditorCamera>, Without<SkyPart>)>,
    mut parts: Query<(&SkyPart, &mut Transform)>,
) {
    if !state.spawned {
        state.spawned = true;
        let dome = meshes.add(dome_mesh());
        state.dome_mesh = Some(dome.clone());
        let dome_material = materials.add(StandardMaterial {
            base_color: Color::WHITE,
            unlit: true,
            fog_enabled: false,
            cull_mode: None,
            ..default()
        });
        let sun_material = unlit(&mut materials, Color::srgb(1.0, 0.97, 0.82), false);
        state.sun_material = Some(sun_material.clone());
        let halo = unlit(&mut materials, Color::srgba(1.0, 0.9, 0.6, 0.22), true);
        let moon = unlit(&mut materials, Color::srgba(0.9, 0.93, 1.0, 1.0), true);
        let stars = unlit(&mut materials, Color::srgba(1.0, 1.0, 1.0, 0.0), true);
        state.halo_material = Some(halo.clone());
        state.moon_material = Some(moon.clone());
        state.star_material = Some(stars.clone());
        let spawn = |commands: &mut Commands, part: SkyPart, mesh: Handle<Mesh>, material: Handle<StandardMaterial>| {
            commands.spawn((
                Name::new("Sky"),
                part,
                WorldVisual,
                NotShadowCaster,
                bevy::light::NotShadowReceiver,
                Mesh3d(mesh),
                MeshMaterial3d(material),
                Transform::default(),
            ));
        };
        spawn(&mut commands, SkyPart::Dome, dome, dome_material);
        spawn(&mut commands, SkyPart::Sun, meshes.add(square(46.0)), sun_material);
        spawn(&mut commands, SkyPart::SunHalo, meshes.add(square(92.0)), halo);
        spawn(&mut commands, SkyPart::Moon, meshes.add(crescent(40.0)), moon);
        spawn(&mut commands, SkyPart::Stars, meshes.add(star_field()), stars);
        return;
    }
    let Ok(camera) = cameras.single() else {
        return;
    };
    let eye = camera.translation;
    let time = rules.time_of_day;
    let sun = sun_direction(time);
    let daylight = rules.daylight();
    let night = 1.0 - daylight;
    let data = crate::gameplay::atmosphere::atmosphere();
    let config = &data.sky;
    let keyed = config.sample(time);

    // Recolour the dome only when the sky visibly changed.
    let signature = (
        (time * 2000.0) as i32,
        [(rules.biome_sky.x * 100.0) as i32, (rules.biome_sky.y * 100.0) as i32, (rules.biome_sky.z * 100.0) as i32],
        (rules.weather_tint * 100.0) as i32,
        crate::gameplay::atmosphere::atmosphere_revision(),
    );
    if state.last != Some(signature) {
        state.last = Some(signature);
        if let Some(mut mesh) = state.dome_mesh.as_ref().and_then(|handle| meshes.get_mut(handle)) {
            let colors: Vec<[f32; 4]> = dome_directions()
                .iter()
                .map(|dir| {
                    let c = match keyed {
                        Some((zenith, horizon)) => blend_sky(*dir, sun, daylight, zenith, horizon),
                        None => sky_color(*dir, sun, daylight, rules.biome_sky),
                    };
                    let c = c.lerp(rules.weather_color * (0.25 + 0.75 * daylight), rules.weather_tint);
                    let linear = Color::srgb(c.x.clamp(0.0, 1.0), c.y.clamp(0.0, 1.0), c.z.clamp(0.0, 1.0)).to_linear();
                    [linear.red, linear.green, linear.blue, 1.0]
                })
                .collect();
            mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
        }
    }
    if let Some(mut material) = state.star_material.as_ref().and_then(|handle| materials.get_mut(handle)) {
        material.base_color = Color::srgba(1.0, 1.0, 1.0, ((night * 1.3 - 0.3) * config.stars * (1.0 - rules.weather_tint)).clamp(0.0, 1.0));
    }
    if let Some(mut material) = state.halo_material.as_ref().and_then(|handle| materials.get_mut(handle)) {
        let twilight = (1.0 - (sun.y.abs() / 0.35)).clamp(0.0, 1.0);
        material.base_color = Color::srgba(1.0, 0.86 - 0.3 * twilight, 0.55 - 0.25 * twilight, 0.18 + 0.12 * twilight);
    }
    if let Some(mut material) = state.moon_material.as_ref().and_then(|handle| materials.get_mut(handle)) {
        let [r, g, b] = config.moon_color;
        material.base_color = Color::srgba(r, g, b, (0.35 + 0.65 * night) * (1.0 - rules.weather_tint));
    }
    if let Some(mut material) = state.sun_material.as_ref().and_then(|handle| materials.get_mut(handle)) {
        let [r, g, b] = config.sun_color;
        material.base_color = Color::srgb(r, g, b);
    }

    // The orbit axis of the sky, so stars turn with the sun.
    let axis = sun_direction(0.0).cross(sun_direction(0.25)).normalize_or(Vec3::X);
    let spin = Quat::from_axis_angle(axis, time * TAU);
    for (part, mut transform) in &mut parts {
        match part {
            SkyPart::Dome => *transform = Transform::from_translation(eye),
            SkyPart::Stars => *transform = Transform::from_translation(eye).with_rotation(spin),
            SkyPart::Sun | SkyPart::SunHalo => {
                let offset = if *part == SkyPart::SunHalo { 1.0 } else { 0.0 };
                let position = eye + sun * (BODY_DISTANCE + offset);
                *transform = Transform::from_translation(position).looking_at(eye, axis).with_scale(Vec3::splat(config.sun_size));
            }
            SkyPart::Moon => {
                let moon = -sun;
                *transform = Transform::from_translation(eye + moon * BODY_DISTANCE).looking_at(eye, axis).with_scale(Vec3::splat(config.moon_size));
            }
        }
    }
}
