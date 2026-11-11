//! Procedural studio scene.
//!
//! The voxel world now owns every authored object. This module only provides the
//! lighting, the dark backdrop plane, the editor grid and the selection gizmos.

use std::f32::consts::PI;

use bevy::prelude::*;

use crate::core::app_state::{EditorState, EditorUiState};
use crate::voxel::world::CHUNK_SIZE;

/// Build the stage-3 studio environment.
pub fn setup_demo_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.insert_resource(GlobalAmbientLight {
        color: Color::srgb(0.42, 0.52, 0.66),
        brightness: 260.0,
        ..default()
    });

    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(120.0, 120.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.026, 0.036, 0.050),
            perceptual_roughness: 0.98,
            metallic: 0.0,
            ..default()
        })),
        Transform::from_xyz(0.0, -0.6, 0.0),
        crate::editor::studio_common::WorldVisual,
    ));

    commands.spawn((
        DirectionalLight {
            color: Color::srgb(0.92, 0.96, 1.0),
            illuminance: 12_000.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(14.0, 22.0, 12.0)
            .looking_at(Vec3::new(0.0, 1.5, 0.0), Vec3::Y),
    ));

    commands.spawn((
        PointLight {
            color: Color::srgb(0.30, 0.82, 0.76),
            intensity: 160_000.0,
            range: 32.0,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_xyz(-8.0, 8.0, -6.0),
    ));

    commands.spawn((
        PointLight {
            color: Color::srgb(0.96, 0.62, 0.34),
            intensity: 80_000.0,
            range: 26.0,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_xyz(12.0, 6.0, 8.0),
    ));
}

/// Draw the editor grid, the world axis triad and the selected chunk bounds.
pub fn draw_editor_gizmos(
    mut gizmos: Gizmos,
    ui: Res<EditorUiState>,
    editor: Res<EditorState>,
) {
    if !editor.workspace.shows_world() {
        return;
    }
    if ui.show_grid {
        gizmos
            .grid(
                Isometry3d::new(
                    Vec3::new(0.0, 0.003, 0.0),
                    Quat::from_rotation_x(PI * 0.5),
                ),
                UVec2::splat(48),
                Vec2::splat(1.0),
                Color::srgba(0.18, 0.24, 0.30, 0.50),
            )
            .outer_edges();
    }

    if !ui.show_gizmos {
        return;
    }

    gizmos.line(Vec3::ZERO, Vec3::X * 2.2, Color::srgb(0.86, 0.27, 0.32));
    gizmos.line(Vec3::ZERO, Vec3::Y * 2.2, Color::srgb(0.38, 0.82, 0.42));
    gizmos.line(Vec3::ZERO, Vec3::Z * 2.2, Color::srgb(0.30, 0.52, 0.94));

    if let Some(coord) = editor.selected_entry().and_then(|entry| entry.coord) {
        let min = Vec3::new(
            (coord.x * CHUNK_SIZE as i32) as f32,
            (coord.y * CHUNK_SIZE as i32) as f32,
            (coord.z * CHUNK_SIZE as i32) as f32,
        );
        let max = min + Vec3::splat(CHUNK_SIZE as f32);
        draw_box(
            &mut gizmos,
            min,
            max,
            Color::srgba(0.30, 0.82, 0.74, 0.85),
        );
    }
}

fn draw_box(gizmos: &mut Gizmos<'_, '_>, min: Vec3, max: Vec3, color: Color) {
    let corners = [
        Vec3::new(min.x, min.y, min.z),
        Vec3::new(max.x, min.y, min.z),
        Vec3::new(max.x, min.y, max.z),
        Vec3::new(min.x, min.y, max.z),
        Vec3::new(min.x, max.y, min.z),
        Vec3::new(max.x, max.y, min.z),
        Vec3::new(max.x, max.y, max.z),
        Vec3::new(min.x, max.y, max.z),
    ];

    const EDGES: [(usize, usize); 12] = [
        (0, 1),
        (1, 2),
        (2, 3),
        (3, 0),
        (4, 5),
        (5, 6),
        (6, 7),
        (7, 4),
        (0, 4),
        (1, 5),
        (2, 6),
        (3, 7),
    ];

    for (start, end) in EDGES {
        gizmos.line(corners[start], corners[end], color);
    }
}
