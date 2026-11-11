//! 3D icons of blocks, items, tools, spawn eggs and resources.
//!
//! The UI asks for an icon with [`icon`] (any thread, any frame). Unknown
//! keys are queued; [`bake_icons`] renders each one once: the model is
//! spawned on a private render layer with its own light and a camera that
//! renders into a small transparent texture. Two frames later the camera
//! and the model are removed and the texture is registered with egui. The
//! cache is cleared when blocks, items or the shading change.
//!
//! Keys: `block:<id>`, `item:<id>`,
//! `egg:<mob>`, `res:<resource>` and `preview:<n>` (synthetic block
//! designer, see [`request_preview`]).

use std::collections::{HashMap, HashSet};
use std::sync::{LazyLock, Mutex};

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::RenderLayers;
use bevy::camera::{ClearColorConfig, RenderTarget};
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use bevy_egui::{EguiTextureHandle, EguiUserTextures};

use crate::core::app_state::EditorUiState;
use crate::gameplay::inventory::HotbarItem;
use crate::gameplay::items::{item_mesh, ItemLibrary};
use crate::gameplay::mobs::library::MobLibrary;
use crate::renderer::poly_material::PolyVoxelMaterial;
use crate::renderer::voxel_render::{block_material, material_handle, mesh_from_data, VoxelRenderState};
use crate::voxel::blocks::{BlockDef, BlockRegistry};
use crate::voxel::mesher;
use crate::voxel::world::{Voxel, VoxelWorld};

/// Pixel size of an icon texture.
const ICON_SIZE: u32 = 128;
/// Icons baked at the same time (one render layer each).
const SLOTS: usize = 4;
/// First render layer used by the baker (the world uses 0, the HUD replica 3).
const FIRST_LAYER: usize = 10;
/// Where the models are built (far from the world; layers keep them apart).
const ORIGIN: Vec3 = Vec3::new(0.0, -6000.0, 0.0);
/// Frames a camera renders before its texture is used.
const FRAMES: u8 = 3;

#[derive(Default)]
struct IconCache {
    ready: HashMap<String, egui::TextureId>,
    queued: HashSet<String>,
    wanted: Vec<String>,
    previews: HashMap<String, BlockDef>,
}

static CACHE: LazyLock<Mutex<IconCache>> = LazyLock::new(|| Mutex::new(IconCache::default()));

/// Texture of an icon, or `None` (queued) when it is not baked yet.
pub fn icon(key: &str) -> Option<egui::TextureId> {
    let mut cache = CACHE.lock().ok()?;
    if let Some(id) = cache.ready.get(key) {
        return Some(*id);
    }
    if cache.queued.insert(key.to_string()) {
        cache.wanted.push(key.to_string());
    }
    None
}

/// Icon of a block definition that is not in the registry (designer
/// preview). Returns the texture once baked.
pub fn request_preview(key: &str, def: &BlockDef) -> Option<egui::TextureId> {
    if let Ok(mut cache) = CACHE.lock() {
        if !cache.ready.contains_key(key) {
            cache.previews.entry(key.to_string()).or_insert_with(|| def.clone());
        }
    }
    icon(key)
}

/// Icon key of a hotbar entry.
pub fn key_for(item: &HotbarItem, registry: &BlockRegistry) -> Option<String> {
    match item {
        HotbarItem::Empty => None,
        HotbarItem::Block(id) => Some(format!("block:{}", registry.get(*id).id)),
        HotbarItem::Item(id) => Some(format!("item:{id}")),
        HotbarItem::SpawnEgg(name) => Some(format!("egg:{name}")),
        HotbarItem::Resource(id) => Some(format!("res:{id}")),
    }
}

/// Paint an icon into `rect`; `false` when it is not baked yet.
pub fn paint(painter: &egui::Painter, rect: egui::Rect, key: &str) -> bool {
    match icon(key) {
        Some(texture) => {
            painter.image(
                texture,
                rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
            true
        }
        None => false,
    }
}

struct Job {
    key: String,
    slot: usize,
    camera: Entity,
    root: Entity,
    image: Handle<Image>,
    frames: u8,
}

#[derive(Resource, Default)]
pub struct IconBaker {
    jobs: Vec<Job>,
    images: HashMap<String, Handle<Image>>,
    cube: Option<Handle<Mesh>>,
    signature: Option<(u64, u64, u64, u64)>,
    /// Second bake of each icon (key, time due): the first render of a new
    /// kind of mesh can come out empty while its shader is still compiling.
    refresh: Vec<(String, f64)>,
    refreshed: HashSet<String>,
}

/// Delay before the second bake of an icon (seconds).
const REFRESH_DELAY: f64 = 1.5;

pub struct IconPlugin;

impl Plugin for IconPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<IconBaker>().add_systems(Update, bake_icons);
    }
}

fn layer(slot: usize) -> RenderLayers {
    RenderLayers::layer(FIRST_LAYER + slot)
}

fn slot_origin(slot: usize) -> Vec3 {
    ORIGIN + Vec3::X * (slot as f32 * 40.0)
}

#[allow(clippy::too_many_arguments)]
fn bake_icons(
    time: Res<Time>,
    mut commands: Commands,
    mut baker: ResMut<IconBaker>,
    registry: Res<BlockRegistry>,
    items: Res<ItemLibrary>,
    mobs: Res<MobLibrary>,
    ui: Res<EditorUiState>,
    mut render: ResMut<VoxelRenderState>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut voxel_materials: ResMut<Assets<PolyVoxelMaterial>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut textures: ResMut<EguiUserTextures>,
) {
    // Anything that changes the look invalidates every icon.
    let signature = (registry.revision, items.revision, ui.shading.revision, mobs.revision);
    if baker.signature != Some(signature) {
        baker.signature = Some(signature);
        for job in baker.jobs.drain(..) {
            commands.entity(job.camera).despawn();
            commands.entity(job.root).despawn();
        }
        for (_, handle) in baker.images.drain() {
            textures.remove_image(&handle);
            images.remove(handle.id());
        }
        baker.refresh.clear();
        baker.refreshed.clear();
        if let Ok(mut cache) = CACHE.lock() {
            cache.ready.clear();
            cache.queued.clear();
            cache.wanted.clear();
        }
    }

    // Finished jobs: keep the texture, drop the scene.
    let mut finished = Vec::new();
    for job in baker.jobs.iter_mut() {
        job.frames += 1;
        if job.frames > FRAMES {
            finished.push(job.key.clone());
        }
    }
    for key in finished {
        let Some(index) = baker.jobs.iter().position(|job| job.key == key) else {
            continue;
        };
        let job = baker.jobs.swap_remove(index);
        commands.entity(job.camera).despawn();
        commands.entity(job.root).despawn();
        let texture = textures.add_image(EguiTextureHandle::Strong(job.image.clone()));
        // A second bake replaces the first texture.
        if let Some(old) = baker.images.insert(job.key.clone(), job.image) {
            textures.remove_image(&old);
            images.remove(old.id());
        }
        if !job.key.starts_with("preview:") && baker.refreshed.insert(job.key.clone()) {
            let due = time.elapsed_secs_f64() + REFRESH_DELAY;
            baker.refresh.push((job.key.clone(), due));
        }
        if let Ok(mut cache) = CACHE.lock() {
            cache.ready.insert(job.key, texture);
        }
    }
    // Icons due for their second bake go back in the queue.
    let now = time.elapsed_secs_f64();
    let due: Vec<String> = baker.refresh.iter().filter(|(_, at)| *at <= now).map(|(key, _)| key.clone()).collect();
    if !due.is_empty() {
        baker.refresh.retain(|(_, at)| *at > now);
        if let Ok(mut cache) = CACHE.lock() {
            cache.wanted.extend(due);
        }
    }

    // New jobs, one per free slot.
    let (mut wanted, previews) = match CACHE.lock() {
        Ok(mut cache) => {
            let free = SLOTS.saturating_sub(baker.jobs.len());
            let take = free.min(cache.wanted.len());
            let wanted: Vec<String> = cache.wanted.drain(..take).collect();
            let previews: HashMap<String, BlockDef> =
                wanted.iter().filter_map(|key| cache.previews.remove(key).map(|def| (key.clone(), def))).collect();
            (wanted, previews)
        }
        Err(_) => return,
    };
    let cube = baker.cube.get_or_insert_with(|| meshes.add(Cuboid::new(1.0, 1.0, 1.0))).clone();
    for key in wanted.drain(..) {
        let used: Vec<usize> = baker.jobs.iter().map(|job| job.slot).collect();
        let Some(slot) = (0..SLOTS).find(|slot| !used.contains(slot)) else {
            // No slot: requeue for the next frame.
            if let Ok(mut cache) = CACHE.lock() {
                cache.wanted.push(key);
            }
            continue;
        };
        let layer = layer(slot);
        let origin = slot_origin(slot);
        let root = commands
            .spawn((Name::new(format!("Icon {key}")), Transform::from_translation(origin), Visibility::Visible, layer.clone()))
            .id();
        let built = build_model(
            &mut commands,
            root,
            &key,
            previews.get(&key),
            &layer,
            &registry,
            &items,
            &mobs,
            &ui,
            &cube,
            &mut render,
            &mut meshes,
            &mut voxel_materials,
            &mut materials,
        );
        if !built {
            // Unknown or empty model: a plain gem tinted from the key, so
            // every entry still gets an icon (and is never stuck in the queue).
            let hash = key.bytes().fold(2166136261u32, |h, b| (h ^ b as u32).wrapping_mul(16777619));
            let rgb = [90 + (hash & 0x7f) as u8, 90 + ((hash >> 8) & 0x7f) as u8, 90 + ((hash >> 16) & 0x7f) as u8];
            let color = Color::srgb_u8(rgb[0], rgb[1], rgb[2]);
            let material = materials.add(StandardMaterial { base_color: color, perceptual_roughness: 0.55, metallic: 0.3, ..default() });
            commands.spawn((
                Mesh3d(cube.clone()),
                MeshMaterial3d(material),
                Transform::from_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_4) * Quat::from_rotation_x(0.6155)).with_scale(Vec3::splat(0.8)),
                NotShadowCaster,
                layer.clone(),
                ChildOf(root),
            ));
        }

        let mut image = Image::new_target_texture(ICON_SIZE, ICON_SIZE, TextureFormat::Bgra8UnormSrgb, None);
        image.asset_usage = RenderAssetUsages::default();
        let image = images.add(image);
        let eye = origin + Vec3::new(1.55, 1.25, 1.55) * 1.35;
        let camera = commands
            .spawn((
                Name::new(format!("Icon camera {key}")),
                Camera3d::default(),
                Camera {
                    order: -60 - slot as isize,
                    clear_color: ClearColorConfig::Custom(Color::NONE),
                    ..default()
                },
                RenderTarget::from(image.clone()),
                Projection::Perspective(PerspectiveProjection { fov: 0.62, ..default() }),
                Transform::from_translation(eye).looking_at(origin, Vec3::Y),
                layer.clone(),
            ))
            .with_children(|parent| {
                parent.spawn((
                    DirectionalLight { illuminance: 9_500.0, shadow_maps_enabled: false, ..default() },
                    Transform::from_xyz(-0.6, 1.4, 0.9).looking_at(Vec3::ZERO, Vec3::Y),
                    layer.clone(),
                ));
            })
            .id();
        baker.jobs.push(Job { key, slot, camera, root, image, frames: 0 });
    }
}

/// Spawn the model of `key` under `root`; `false` when the key is unknown.
#[allow(clippy::too_many_arguments)]
fn build_model(
    commands: &mut Commands,
    root: Entity,
    key: &str,
    preview: Option<&BlockDef>,
    layer: &RenderLayers,
    registry: &BlockRegistry,
    items: &ItemLibrary,
    mobs: &MobLibrary,
    ui: &EditorUiState,
    cube: &Handle<Mesh>,
    render: &mut VoxelRenderState,
    meshes: &mut Assets<Mesh>,
    voxel_materials: &mut Assets<PolyVoxelMaterial>,
    materials: &mut Assets<StandardMaterial>,
) -> bool {
    let (kind, id) = key.split_once(':').unwrap_or((key, ""));
    let piece = |commands: &mut Commands, mesh: Handle<Mesh>, material: MeshMaterial3d<StandardMaterial>, transform: Transform| {
        commands.spawn((Mesh3d(mesh), material, transform, NotShadowCaster, layer.clone(), ChildOf(root)));
    };
    let solid = |materials: &mut Assets<StandardMaterial>, rgb: [u8; 3], emissive: f32, metal: f32| {
        let color = Color::srgb_u8(rgb[0], rgb[1], rgb[2]);
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: color,
            emissive: color.to_linear() * emissive,
            perceptual_roughness: 0.55,
            metallic: metal,
            ..default()
        }))
    };
    match kind {
        "block" | "preview" => {
            // The real voxel mesh and material (shape, model, colour, relief).
            // A preview is not registered: mesh it from a private copy, the
            // mesh carries its painted parts.
            let scratch = preview.map(|def| {
                let mut copy = registry.clone();
                let block = copy.upsert(def.clone());
                (copy, block)
            });
            let (meshing, block) = match &scratch {
                Some((copy, block)) => (copy, *block),
                None => match registry.find(id) {
                    Some(block) => (registry, block),
                    None => return false,
                },
            };
            let def = meshing.get(block).clone();
            let mut world = VoxelWorld::new();
            world.set(IVec3::ZERO, Some(Voxel::new(block)));
            let Some(data) = mesher::build_chunk_mesh(&world, meshing, IVec3::ZERO, true).remove(&block) else {
                return false;
            };
            let material = if preview.is_some() {
                voxel_materials.add(block_material(&def, &ui.shading))
            } else {
                material_handle(render, voxel_materials, registry, block, &ui.shading)
            };
            commands.spawn((
                Mesh3d(meshes.add(mesh_from_data(data))),
                MeshMaterial3d(material),
                Transform::from_translation(Vec3::splat(-0.5)),
                NotShadowCaster,
                layer.clone(),
                ChildOf(root),
            ));
            // The parts animated in game, at rest.
            for part in def.parts.iter().flatten() {
                let (center, size) = part.center_size();
                let material = solid(materials, part.color, part.emissive, part.metallic);
                piece(commands, cube.clone(), material, Transform::from_translation(center).with_rotation(part.rotation_quat()).with_scale(size));
            }
            true
        }
        "item" => {
            let def = items.find_or_placeholder(id);
            if def.model().is_empty() {
                return false;
            }
            let (min, max) = def.bounds();
            let size = (max - min).max_element().max(0.05);
            let center = (min + max) * 0.5;
            let scale = 1.25 / size;
            let material = MeshMaterial3d(materials.add(StandardMaterial { base_color: Color::WHITE, perceptual_roughness: 0.6, ..default() }));
            piece(
                commands,
                meshes.add(item_mesh(&def)),
                material,
                Transform::from_rotation(Quat::from_rotation_y(-0.6) * Quat::from_rotation_z(0.5))
                    .with_scale(Vec3::splat(scale))
                    * Transform::from_translation(-center),
            );
            true
        }
        "egg" => {
            // Same cubic egg as the one held in hand (`items::egg_boxes`).
            let rig = mobs.rigs.iter().find(|rig| rig.name == id);
            for (center, size, color) in crate::gameplay::items::egg_boxes(rig) {
                let material = solid(materials, color, 0.03, 0.0);
                piece(commands, cube.clone(), material, Transform::from_translation(center * 1.15).with_scale(size * 1.15));
            }
            true
        }
        "res" => {
            // Groups (`@floral`) show the resource they are named after.
            let Some(info) = crate::gameplay::crafting::resource_info(id.trim_start_matches('@')) else {
                return false;
            };
            let glow = match info.tier {
                crate::gameplay::crafting::Tier::Raw => 0.05,
                crate::gameplay::crafting::Tier::Basic => 0.2,
                crate::gameplay::crafting::Tier::Overcharged => 0.6,
                crate::gameplay::crafting::Tier::EndGame => 1.2,
            };
            let material = solid(materials, info.color, glow, 0.3);
            // A cut gem: a cube balanced on its corner.
            piece(
                commands,
                cube.clone(),
                material,
                Transform::from_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_4) * Quat::from_rotation_x(0.6155))
                    .with_scale(Vec3::splat(0.95)),
            );
            true
        }
        _ => false,
    }
}
