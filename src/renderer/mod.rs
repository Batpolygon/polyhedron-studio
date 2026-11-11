//! Rendering boundary.
//!
//! Stage 4 keeps the procedural studio lighting and the editor gizmos, and
//! replaces the voxel `StandardMaterial` with the WGSL toon / Voronoi / outline
//! pipeline without changing the ECS layout. The shader sources are installed
//! before the material plugins are built because Bevy resolves
//! [`bevy::pbr::MaterialExtension::fragment_shader`] during plugin construction.

pub mod clouds;
pub mod demo_scene;
pub mod icons;
pub mod particles;
pub mod poly_material;
pub mod sky;
pub mod voxel_render;

use bevy::pbr::MaterialPlugin;
use bevy::prelude::*;

use crate::voxel::VoxelSet;

pub struct RendererPlugin;

impl Plugin for RendererPlugin {
    fn build(&self, app: &mut App) {
        // `install_shaders` must run first: `MaterialPlugin::build` immediately
        // initializes the material shaders from the static `MaterialExtension`
        // callbacks, which read the handles stored by this call.
        let shaders = poly_material::install_shaders(app);

        app.insert_resource(shaders)
            .add_plugins((
                MaterialPlugin::<poly_material::PolyVoxelMaterial>::default(),
                MaterialPlugin::<poly_material::PolyOutlineMaterial>::default(),
            ))
            .add_plugins((particles::ParticlePlugin, clouds::CloudPlugin, sky::SkyPlugin, icons::IconPlugin))
            .init_resource::<voxel_render::VoxelRenderState>()
            .add_systems(Startup, demo_scene::setup_demo_scene)
            .add_systems(
                Update,
                (
                    poly_material::reload_shader_sources_system,
                    demo_scene::draw_editor_gizmos,
                    voxel_render::sync_voxel_meshes.in_set(VoxelSet::Sync),
                    voxel_render::draw_voxel_cursor.after(VoxelSet::Input),
                ),
            );
    }
}
