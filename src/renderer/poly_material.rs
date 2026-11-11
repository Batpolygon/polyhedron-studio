//! Procedural toon, Voronoi and outline materials.
//!
//! The voxel surfaces are rendered with
//! `ExtendedMaterial<StandardMaterial, PolyMaterial>`. The standard half keeps
//! Bevy's lighting, shadows and tonemapping; the [`PolyMaterial`] extension adds
//! the banded lighting, the 3D Voronoi tint and the rim light through a small
//! uniform bound at slot 100 of the material bind group.
//!
//! Outlines use a second, dedicated [`Material`] on a hidden child shell. Its
//! vertex stage is a custom inverted hull: vertices are pushed along their
//! world normal and the pipeline culls front faces, leaving a depth-tested rim
//! around each voxel surface.
//!
//! Both shader sources live in `project/shaders/` so they can be edited without
//! recompiling the engine. The binary embeds a copy, writes it on first run and
//! can restore it from the console or the toolbox.

use std::fs;
use std::path::Path;
use std::sync::OnceLock;

use bevy::asset::Asset;
use bevy::color::LinearRgba;
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{
    ExtendedMaterial, Material, MaterialExtension, MaterialPipeline, MaterialPipelineKey,
};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, Face, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError,
};
use bevy::shader::{Shader, ShaderRef};

use crate::core::app_state::{EditorUiState, ShadingSettings};
use crate::core::project_io::ProjectLayout;

/// File names used inside `project/shaders/`.
pub const TOON_SHADER_FILE: &str = "poly_toon.wesl";
pub const OUTLINE_SHADER_FILE: &str = "poly_outline.wesl";

/// WESL module paths. They are absolute asset paths, so the shaders can import
/// the `bevy_pbr` package without colliding with their own module path.
const TOON_SHADER_PATH: &str = "polyhedron/render/poly_toon.wesl";
const OUTLINE_SHADER_PATH: &str = "polyhedron/render/poly_outline.wesl";

const TOON_SHADER_SOURCE: &str = include_str!("shaders/poly_toon.wesl");
const OUTLINE_SHADER_SOURCE: &str = include_str!("shaders/poly_outline.wesl");

/// Material extension parameters, mirroring `PolyShading` in `poly_toon.wesl`.
#[derive(Clone, Copy, Debug, ShaderType)]
pub struct PolyShadingUniform {
    pub toon_enabled: u32,
    pub voronoi_enabled: u32,
    pub rim_enabled: u32,
    pub bands: u32,
    pub band_softness: f32,
    pub voronoi_strength: f32,
    pub voronoi_scale: f32,
    pub rim_strength: f32,
    pub rim_color: LinearRgba,
    pub voronoi_color: LinearRgba,
    /// Low-poly Voronoi normal map (0 = flat).
    pub normal_strength: f32,
    /// Facets per block edge.
    pub normal_cells: f32,
    /// Bevel width between facets.
    pub normal_bevel: f32,
    /// Pattern offset.
    pub normal_seed: f32,
    /// Wind sway of the vertices (0 = still).
    pub wind: f32,
    /// Height of the liquid waves (0 = flat).
    pub wave: f32,
    /// Flecks of the block: cells along an edge and emission.
    pub accent_cells: f32,
    pub accent_glow: f32,
    /// Colour of the flecks; `a` is the share of the cells (0 = none).
    pub accent: LinearRgba,
}

impl PolyShadingUniform {
    /// Convert the editor-facing settings into their GPU representation.
    pub fn from_settings(settings: &ShadingSettings) -> Self {
        Self {
            toon_enabled: u32::from(settings.toon),
            voronoi_enabled: u32::from(settings.voronoi),
            rim_enabled: u32::from(settings.rim),
            bands: settings.bands.clamp(2, 12),
            band_softness: settings.band_softness.clamp(0.0, 0.99),
            voronoi_strength: settings.voronoi_strength.clamp(0.0, 1.0),
            voronoi_scale: settings.voronoi_scale.clamp(0.01, 8.0),
            rim_strength: settings.rim_strength.clamp(0.0, 4.0),
            rim_color: srgb(settings.rim_color),
            voronoi_color: srgb(settings.voronoi_color),
            normal_strength: 0.0,
            normal_cells: 3.0,
            normal_bevel: 0.1,
            normal_seed: 0.0,
            wind: 0.0,
            wave: 0.0,
            accent_cells: 1.0,
            accent_glow: 0.0,
            accent: LinearRgba::NONE,
        }
    }
}

impl PolyShadingUniform {
    /// Global settings specialised for one block: Voronoi tint density
    /// (`None`, `3Cells`, `7Cells`, `30Cells`, `200Cells`) and its low-poly
    /// facet normal map.
    pub fn for_look(settings: &ShadingSettings, look: crate::renderer::voxel_render::BlockLook) -> Self {
        let mut uniform = Self::from_settings(settings);
        let density = look.voronoi.density();
        if density <= 0.0 {
            uniform.voronoi_enabled = 0;
        } else {
            // Cells per block edge: the density, scaled by the global slider
            // (0.22 is the reference value).
            uniform.voronoi_scale = (settings.voronoi_scale / 0.22 * density).clamp(0.05, 40.0);
        }
        uniform.wind = settings.effective_wind(look.wind);
        if look.liquid && settings.waves {
            uniform.wave = settings.wave_height.clamp(0.0, 0.5);
        }
        if let Some(accent) = look.accent {
            let [red, green, blue] = accent.color;
            uniform.accent = Color::srgb_u8(red, green, blue).to_linear().with_alpha(accent.amount.clamp(0.0, 1.0));
            uniform.accent_cells = accent.cells.clamp(0.25, 64.0);
            uniform.accent_glow = accent.glow.max(0.0);
        }
        let normal = look.normal;
        if normal.enabled && settings.facets {
            uniform.normal_strength = normal.strength.clamp(0.0, 1.5) * settings.facet_strength;
            uniform.normal_cells = normal.cells.clamp(0.25, 32.0);
            uniform.normal_bevel = normal.bevel.clamp(0.0, 1.0);
            uniform.normal_seed = normal.seed;
        }
        uniform
    }
}

/// Outline shell parameters, mirroring `OutlineSettings` in
/// `poly_outline.wesl`.
#[derive(Clone, Copy, Debug, ShaderType)]
pub struct PolyOutlineUniform {
    pub color: LinearRgba,
    pub width: f32,
    /// Wind sway of the shell (same as its surface).
    pub wind: f32,
    pub _pad1: f32,
    pub _pad2: f32,
}

impl PolyOutlineUniform {
    pub fn from_settings(settings: &ShadingSettings) -> Self {
        Self::for_wind(settings, 0.0)
    }

    /// Outline of blocks whose raw wind value is `wind`.
    pub fn for_wind(settings: &ShadingSettings, wind: f32) -> Self {
        Self {
            color: srgb(settings.outline_color),
            width: settings.outline_width.clamp(0.0, 0.05),
            wind: settings.effective_wind(wind),
            _pad1: 0.0,
            _pad2: 0.0,
        }
    }
}

/// Extension attached to `StandardMaterial` for every voxel surface.
#[derive(Asset, AsBindGroup, TypePath, Debug, Clone)]
pub struct PolyMaterial {
    #[uniform(100)]
    pub settings: PolyShadingUniform,
}

impl Default for PolyMaterial {
    fn default() -> Self {
        Self {
            settings: PolyShadingUniform::from_settings(&ShadingSettings::default()),
        }
    }
}

impl MaterialExtension for PolyMaterial {
    /// The surface vertex stage animates the blocks (wind sway, liquid
    /// waves) with the same displacement as the outline shell, and passes the
    /// block-space position to the fragment stage. Without it the outline
    /// swayed while the block itself stayed still.
    fn vertex_shader() -> ShaderRef {
        ShaderRef::Handle(toon_shader_handle())
    }

    fn fragment_shader() -> ShaderRef {
        ShaderRef::Handle(toon_shader_handle())
    }
}

/// Dedicated material used by the inverted-hull outline shells.
#[derive(Asset, AsBindGroup, TypePath, Debug, Clone)]
pub struct PolyOutlineMaterial {
    #[uniform(0)]
    pub settings: PolyOutlineUniform,
}

impl Default for PolyOutlineMaterial {
    fn default() -> Self {
        Self {
            settings: PolyOutlineUniform::from_settings(&ShadingSettings::default()),
        }
    }
}

impl Material for PolyOutlineMaterial {
    fn vertex_shader() -> ShaderRef {
        ShaderRef::Handle(outline_shader_handle())
    }

    fn fragment_shader() -> ShaderRef {
        ShaderRef::Handle(outline_shader_handle())
    }

    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        // The shell is expanded outward: culling the front faces leaves the
        // inner back faces visible only where they escape the object silhouette.
        descriptor.primitive.cull_mode = Some(Face::Front);
        Ok(())
    }
}

/// Marker for the outline child entities spawned under each voxel surface.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct PolyOutlineShell;

/// Handles owned by the main world so the shaders can be reloaded in place.
#[derive(Resource, Clone, Debug)]
pub struct PolyShaderHandles {
    pub toon: Handle<Shader>,
    pub outline: Handle<Shader>,
}

/// The type used by the voxel renderer for every surface.
pub type PolyVoxelMaterial = ExtendedMaterial<StandardMaterial, PolyMaterial>;

static TOON_SHADER: OnceLock<Handle<Shader>> = OnceLock::new();
static OUTLINE_SHADER: OnceLock<Handle<Shader>> = OnceLock::new();

fn toon_shader_handle() -> Handle<Shader> {
    TOON_SHADER.get().cloned().unwrap_or_default()
}

fn outline_shader_handle() -> Handle<Shader> {
    OUTLINE_SHADER.get().cloned().unwrap_or_default()
}

/// Write the embedded shader sources into `project/shaders/` and register them
/// as assets. This must run before the material plugins are built because
/// [`MaterialExtension::fragment_shader`] is resolved during plugin build.
pub fn install_shaders(app: &mut App) -> PolyShaderHandles {
    // Never call `init_asset` unconditionally here: it replaces the existing
    // `Assets<Shader>` with a fresh allocator while the `AssetServer` still
    // holds in-flight shader loads, which corrupts the asset index space.
    if app.world().get_resource::<Assets<Shader>>().is_none() {
        app.init_asset::<Shader>();
    }

    let project = ProjectLayout::default();
    let directory = project.module_path("shaders");

    let toon_source = install_source(&directory, TOON_SHADER_FILE, TOON_SHADER_SOURCE);
    let outline_source = install_source(&directory, OUTLINE_SHADER_FILE, OUTLINE_SHADER_SOURCE);

    let mut shaders = app.world_mut().resource_mut::<Assets<Shader>>();
    let toon = shaders.add(Shader::from_wesl(toon_source, TOON_SHADER_PATH));
    let outline = shaders.add(Shader::from_wesl(outline_source, OUTLINE_SHADER_PATH));

    let _ = TOON_SHADER.set(toon.clone());
    let _ = OUTLINE_SHADER.set(outline.clone());

    PolyShaderHandles { toon, outline }
}

/// Re-read the shader files after an in-editor edit.
///
/// The flags are published by the console and the toolbox. Reloading replaces
/// the shader asset in place, which requeues every dependent pipeline.
pub fn reload_shader_sources_system(
    mut ui: ResMut<EditorUiState>,
    project: Res<ProjectLayout>,
    handles: Res<PolyShaderHandles>,
    mut shaders: ResMut<Assets<Shader>>,
) {
    if !ui.reload_shader_sources && !ui.reset_shader_sources {
        return;
    }

    let reset = ui.reset_shader_sources;
    ui.reload_shader_sources = false;
    ui.reset_shader_sources = false;

    let directory = project.module_path("shaders");
    let toon_source = read_source(&directory, TOON_SHADER_FILE, TOON_SHADER_SOURCE, reset);
    let outline_source = read_source(&directory, OUTLINE_SHADER_FILE, OUTLINE_SHADER_SOURCE, reset);

    let toon_result = shaders.insert(
        handles.toon.id(),
        Shader::from_wesl(toon_source, TOON_SHADER_PATH),
    );
    let outline_result = shaders.insert(
        handles.outline.id(),
        Shader::from_wesl(outline_source, OUTLINE_SHADER_PATH),
    );

    if toon_result.is_err() || outline_result.is_err() {
        ui.error("Shader reload failed: the shader assets are no longer alive.");
        return;
    }

    if reset {
        ui.action("Shader sources restored from the engine defaults.");
    } else {
        ui.action("Shader sources reloaded from project/shaders.");
    }
}

/// Marker of the shader generation the engine expects (uniform layout).
const SHADER_VERSION_MARKER: &str = "@version 4";

fn install_source(directory: &Path, file: &str, embedded: &str) -> String {
    let path = directory.join(file);
    // An older project copy would not match the uniforms: keep it as a
    // backup and install the current one.
    if path.is_file() {
        let current = fs::read_to_string(&path).unwrap_or_default();
        if !current.contains(SHADER_VERSION_MARKER) {
            let _ = fs::write(path.with_extension("wesl.bak"), current);
            let _ = fs::write(&path, embedded);
            return embedded.to_string();
        }
    }
    if !path.is_file() {
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::write(&path, embedded);
        return embedded.to_string();
    }

    fs::read_to_string(&path).unwrap_or_else(|_| embedded.to_string())
}

fn read_source(directory: &Path, file: &str, embedded: &str, reset: bool) -> String {
    let path = directory.join(file);

    if reset {
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::write(&path, embedded);
        return embedded.to_string();
    }

    match fs::read_to_string(&path) {
        Ok(source) => source,
        Err(_) => {
            let _ = fs::write(&path, embedded);
            embedded.to_string()
        }
    }
}

fn srgb(color: [f32; 3]) -> LinearRgba {
    Color::srgb(
        color[0].clamp(0.0, 1.0),
        color[1].clamp(0.0, 1.0),
        color[2].clamp(0.0, 1.0),
    )
    .to_linear()
}
