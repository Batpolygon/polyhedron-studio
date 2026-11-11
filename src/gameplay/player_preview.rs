//! 3D replica of the player shown in the top-right HUD panel, and the 3D
//! character of the main menu.
//!
//! The avatar module builds a second third-person body (same limbs, same
//! animation, same held item) far below the world on its own render layer.
//! A dedicated camera renders only that layer into an image; the UI paints the
//! image like any other texture, so windows and popups draw over it normally
//! (it no longer vanishes when a menu opens).

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::RenderLayers;
use bevy::camera::{ClearColorConfig, RenderTarget};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureFormat};
use bevy_egui::{EguiPostUpdateSet, EguiTextureHandle, EguiUserTextures};

use crate::core::pie_manager::PieState;

/// Render layer of the replica (the world uses layer 0).
pub const PREVIEW_LAYER: usize = 3;
/// Where the replica stands (far away from any terrain).
pub const PREVIEW_ORIGIN: Vec3 = Vec3::new(0.0, -4000.0, 0.0);
/// Background of the preview card (matches the HUD panels).
pub const PREVIEW_BACKGROUND: Color = Color::srgb(0.075, 0.1, 0.135);

/// Where the HUD wants the replica drawn this frame (logical points), and the
/// texture the replica camera renders into.
#[derive(Resource, Debug, Default)]
pub struct PlayerPreview {
    pub rect: Option<egui::Rect>,
    pub pixels_per_point: f32,
    pub texture: Option<egui::TextureId>,
    image: Option<Handle<Image>>,
}

/// Marker of the preview camera.
#[derive(Component)]
pub struct PreviewCamera;

/// Where the main menu wants its 3D character drawn this frame.
#[derive(Resource, Debug, Default)]
pub struct MenuPreview {
    pub rect: Option<egui::Rect>,
    pub pixels_per_point: f32,
    pub texture: Option<egui::TextureId>,
    image: Option<Handle<Image>>,
}

/// Marker of the main menu character camera.
#[derive(Component)]
pub struct MenuPreviewCamera;

/// Paint a preview texture into `rect` (nothing before its first frame).
pub fn paint_preview(painter: &egui::Painter, rect: egui::Rect, texture: Option<egui::TextureId>) {
    if let Some(texture) = texture {
        painter.image(
            texture,
            rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
    }
}

pub struct PlayerPreviewPlugin;

impl Plugin for PlayerPreviewPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PlayerPreview>()
            .init_resource::<MenuPreview>()
            .add_systems(Startup, (spawn_preview_camera, spawn_menu_camera))
            .add_systems(
                PostUpdate,
                (sync_preview_camera, sync_menu_camera).after(EguiPostUpdateSet::ProcessOutput),
            );
    }
}

/// A render-target image registered with egui.
fn target_image(images: &mut Assets<Image>, textures: &mut EguiUserTextures) -> (Handle<Image>, egui::TextureId) {
    let mut image = Image::new_target_texture(256, 256, TextureFormat::Bgra8UnormSrgb, None);
    image.asset_usage = RenderAssetUsages::default();
    let handle = images.add(image);
    let texture = textures.add_image(EguiTextureHandle::Strong(handle.clone()));
    (handle, texture)
}

fn spawn_preview_camera(
    mut commands: Commands,
    mut preview: ResMut<PlayerPreview>,
    mut images: ResMut<Assets<Image>>,
    mut textures: ResMut<EguiUserTextures>,
) {
    let (image, texture) = target_image(&mut images, &mut textures);
    preview.image = Some(image.clone());
    preview.texture = Some(texture);
    let target = PREVIEW_ORIGIN + Vec3::new(0.0, 0.05, 0.0);
    let eye = PREVIEW_ORIGIN + Vec3::new(0.95, 0.45, -2.7);
    commands
        .spawn((
            Name::new("Player preview camera"),
            PreviewCamera,
            Camera3d::default(),
            Camera {
                order: -20,
                is_active: false,
                clear_color: ClearColorConfig::Custom(PREVIEW_BACKGROUND),
                ..default()
            },
            RenderTarget::from(image),
            Projection::Perspective(PerspectiveProjection {
                fov: 0.62,
                ..default()
            }),
            Transform::from_translation(eye).looking_at(target, Vec3::Y),
            RenderLayers::layer(PREVIEW_LAYER),
        ))
        .with_children(|parent| {
            parent.spawn((
                DirectionalLight {
                    illuminance: 9_000.0,
                    shadow_maps_enabled: false,
                    ..default()
                },
                Transform::from_xyz(-1.0, 2.0, 1.0).looking_at(Vec3::ZERO, Vec3::Y),
                RenderLayers::layer(PREVIEW_LAYER),
            ));
        });
}

fn spawn_menu_camera(
    mut commands: Commands,
    mut preview: ResMut<MenuPreview>,
    mut images: ResMut<Assets<Image>>,
    mut textures: ResMut<EguiUserTextures>,
) {
    use crate::gameplay::avatar::{MENU_LAYER, MENU_ORIGIN};
    let (image, texture) = target_image(&mut images, &mut textures);
    preview.image = Some(image.clone());
    preview.texture = Some(texture);
    let target = MENU_ORIGIN + Vec3::new(0.0, 0.02, 0.0);
    let eye = MENU_ORIGIN + Vec3::new(0.0, 0.25, 3.7);
    commands
        .spawn((
            Name::new("Menu character camera"),
            MenuPreviewCamera,
            Camera3d::default(),
            Camera {
                order: -21,
                is_active: false,
                clear_color: ClearColorConfig::Custom(PREVIEW_BACKGROUND),
                ..default()
            },
            RenderTarget::from(image),
            Projection::Perspective(PerspectiveProjection { fov: 0.55, ..default() }),
            Transform::from_translation(eye).looking_at(target, Vec3::Y),
            RenderLayers::layer(MENU_LAYER),
        ))
        .with_children(|parent| {
            parent.spawn((
                DirectionalLight { illuminance: 9_500.0, shadow_maps_enabled: false, ..default() },
                Transform::from_xyz(-1.0, 2.0, 1.5).looking_at(Vec3::ZERO, Vec3::Y),
                RenderLayers::layer(MENU_LAYER),
            ));
        });
}

/// Physical pixel size of `rect` (logical points), or `None` when too small.
fn pixel_size(rect: Option<egui::Rect>, pixels_per_point: f32) -> Option<UVec2> {
    let rect = rect?;
    let scale = pixels_per_point.max(0.1);
    let width = (rect.width() * scale).round().clamp(0.0, 2048.0) as u32;
    let height = (rect.height() * scale).round().clamp(0.0, 2048.0) as u32;
    (width > 4 && height > 4).then_some(UVec2::new(width, height))
}

/// Turn the camera on while the UI shows its image, and keep the image at the
/// on-screen pixel size (the camera follows the image's size).
fn drive_camera(camera: &mut Camera, image: Option<&Handle<Image>>, size: Option<UVec2>, images: &mut Assets<Image>) {
    let active = size.is_some();
    if camera.is_active != active {
        camera.is_active = active;
    }
    let (Some(size), Some(handle)) = (size, image) else {
        return;
    };
    let current = images.get(handle).map(|image| image.size());
    if current != Some(size) {
        if let Some(mut image) = images.get_mut(handle) {
            image.resize(Extent3d { width: size.x, height: size.y, depth_or_array_layers: 1 });
        }
    }
}

fn sync_menu_camera(
    preview: Res<MenuPreview>,
    mut images: ResMut<Assets<Image>>,
    mut cameras: Query<&mut Camera, With<MenuPreviewCamera>>,
) {
    let Ok(mut camera) = cameras.single_mut() else {
        return;
    };
    let size = pixel_size(preview.rect, preview.pixels_per_point);
    drive_camera(&mut camera, preview.image.as_ref(), size, &mut images);
}

fn sync_preview_camera(
    pie: Res<PieState>,
    preview: Res<PlayerPreview>,
    mut images: ResMut<Assets<Image>>,
    mut cameras: Query<&mut Camera, With<PreviewCamera>>,
) {
    let Ok(mut camera) = cameras.single_mut() else {
        return;
    };
    let size = if pie.is_playing() { pixel_size(preview.rect, preview.pixels_per_point) } else { None };
    drive_camera(&mut camera, preview.image.as_ref(), size, &mut images);
}
