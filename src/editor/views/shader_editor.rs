//! Shader editor: live variables of the toon / Voronoi / outline pipeline
//! (with wind and water waves), presets of those variables saved in
//! `project/shaders/presets/*.ron`, and the WESL source of
//! `project/shaders/*.wesl`, saved and hot-reloaded without restarting the
//! engine.

use std::fs;

use egui::RichText;

use crate::core::app_state::EditorUiState;
use crate::core::project_io::ProjectLayout;
use crate::editor::theme;
use crate::renderer::poly_material::{OUTLINE_SHADER_FILE, TOON_SHADER_FILE};

#[derive(Clone, Debug, Default)]
struct SourceState {
    file: usize,
    text: String,
    loaded_file: Option<usize>,
    dirty: bool,
}

const FILES: [&str; 2] = [TOON_SHADER_FILE, OUTLINE_SHADER_FILE];

/// Preset list and the name being typed.
#[derive(Clone, Debug, Default)]
struct PresetState {
    names: Vec<String>,
    selected: Option<String>,
    name: String,
    loaded: bool,
}

fn presets_dir(project: &ProjectLayout) -> std::path::PathBuf {
    project.module_path("shaders").join("presets")
}

fn list_presets(project: &ProjectLayout) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(presets_dir(project))
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|entry| {
                    let path = entry.path();
                    if path.extension()?.to_str()? != "ron" {
                        return None;
                    }
                    Some(path.file_stem()?.to_string_lossy().to_string())
                })
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

/// File remembering the last preset applied or saved.
fn last_file(project: &ProjectLayout) -> std::path::PathBuf {
    presets_dir(project).join("last.txt")
}

fn remember(project: &ProjectLayout, name: &str) {
    let _ = fs::create_dir_all(presets_dir(project));
    let _ = fs::write(last_file(project), name);
}

/// The preset to apply at startup: the last one used, else the first one
/// created; a `Default` preset (engine defaults) is created when there is none.
pub fn startup_preset(project: &ProjectLayout) -> Option<(String, crate::core::app_state::ShadingSettings)> {
    let dir = presets_dir(project);
    if list_presets(project).is_empty() {
        let _ = fs::create_dir_all(&dir);
        let _ = crate::core::project_io::save_ron(&dir.join("Default.ron"), &crate::core::app_state::ShadingSettings::default());
    }
    let names = list_presets(project);
    let last = fs::read_to_string(last_file(project)).ok().map(|text| text.trim().to_string()).filter(|name| names.contains(name));
    let first = || {
        names
            .iter()
            .min_by_key(|name| {
                fs::metadata(dir.join(format!("{name}.ron")))
                    .and_then(|meta| meta.created().or_else(|_| meta.modified()))
                    .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
            })
            .cloned()
    };
    let name = last.or_else(first)?;
    let settings = crate::core::project_io::load_ron::<crate::core::app_state::ShadingSettings>(&dir.join(format!("{name}.ron"))).ok()?;
    Some((name, settings))
}

/// Startup: apply the last used (or first) shader preset.
pub fn apply_startup_preset(project: bevy::prelude::Res<ProjectLayout>, mut state: bevy::prelude::ResMut<EditorUiState>) {
    if let Some((name, mut settings)) = startup_preset(&project) {
        settings.revision = state.shading.revision;
        state.shading = settings;
        state.shading.touch();
        state.info(format!("Shader preset '{name}' loaded."));
    }
}

/// Save all: the live variables go to the preset in use, an edited source to
/// its file.
pub fn save_open_work(ctx: &egui::Context, project: &ProjectLayout, state: &mut EditorUiState) -> Result<(), String> {
    let name = fs::read_to_string(last_file(project)).map(|text| text.trim().to_string()).unwrap_or_default();
    let name = if name.is_empty() { "Default".to_string() } else { name };
    let _ = fs::create_dir_all(presets_dir(project));
    crate::core::project_io::save_ron(&presets_dir(project).join(format!("{name}.ron")), &state.shading)?;
    remember(project, &name);

    let id = egui::Id::new("polyhedron_shader_source");
    let mut source: SourceState = ctx.data_mut(|data| data.get_temp(id)).unwrap_or_default();
    if source.dirty {
        let path = project.module_path("shaders").join(FILES[source.file]);
        fs::write(&path, &source.text).map_err(|error| format!("cannot write {}: {error}", path.display()))?;
        source.dirty = false;
        state.reload_shader_sources = true;
        ctx.data_mut(|data| data.insert_temp(id, source));
    }
    Ok(())
}

fn sanitize(name: &str) -> String {
    name.trim()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else if c == ' ' { '_' } else { '-' })
        .collect()
}

/// Save / load / delete presets of the live variables.
fn presets_section(ui: &mut egui::Ui, state: &mut EditorUiState, project: &ProjectLayout) {
    theme::section_label(ui, "PRESETS");
    let id = egui::Id::new("polyhedron_shader_presets");
    let mut presets: PresetState = ui.ctx().data_mut(|data| data.get_temp(id)).unwrap_or_default();
    if !presets.loaded {
        presets.names = list_presets(project);
        presets.loaded = true;
        presets.selected = fs::read_to_string(last_file(project))
            .ok()
            .map(|text| text.trim().to_string())
            .filter(|name| presets.names.contains(name));
        if let Some(name) = &presets.selected {
            presets.name = name.clone();
        }
    }
    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt("shader_preset_combo")
            .width(160.0)
            .selected_text(presets.selected.clone().unwrap_or_else(|| "Choose a preset".to_string()))
            .show_ui(ui, |ui| {
                for name in presets.names.clone() {
                    if ui.selectable_label(presets.selected.as_deref() == Some(name.as_str()), &name).clicked() {
                        presets.selected = Some(name.clone());
                        presets.name = name;
                    }
                }
            });
        let can_load = presets.selected.is_some();
        if ui.add_enabled(can_load, egui::Button::new("Load")).on_hover_text("Apply the preset").clicked() {
            if let Some(name) = &presets.selected {
                let path = presets_dir(project).join(format!("{name}.ron"));
                match crate::core::project_io::load_ron::<crate::core::app_state::ShadingSettings>(&path) {
                    Ok(mut loaded) => {
                        loaded.revision = state.shading.revision;
                        state.shading = loaded;
                        state.shading.touch();
                        state.action(format!("Shader preset '{name}' applied."));
                        remember(project, name);
                    }
                    Err(error) => state.error(error),
                }
            }
        }
        if ui.add_enabled(can_load, egui::Button::new("Delete")).clicked() {
            if let Some(name) = presets.selected.take() {
                let _ = fs::remove_file(presets_dir(project).join(format!("{name}.ron")));
                presets.names = list_presets(project);
                state.info(format!("Shader preset '{name}' deleted."));
            }
        }
    });
    ui.horizontal(|ui| {
        ui.add(egui::TextEdit::singleline(&mut presets.name).hint_text("Preset name").desired_width(160.0));
        let stem = sanitize(&presets.name);
        if ui.add_enabled(!stem.is_empty(), egui::Button::new("Save current")).clicked() {
            let path = presets_dir(project).join(format!("{stem}.ron"));
            match crate::core::project_io::save_ron(&path, &state.shading) {
                Ok(()) => {
                    presets.names = list_presets(project);
                    presets.selected = Some(stem.clone());
                    remember(project, &stem);
                    state.action(format!("Shader preset '{stem}' saved."));
                }
                Err(error) => state.error(error),
            }
        }
        if ui.button("Defaults").on_hover_text("Engine default look").clicked() {
            let revision = state.shading.revision;
            state.shading = crate::core::app_state::ShadingSettings { revision, ..Default::default() };
            state.shading.touch();
        }
    });
    ui.ctx().data_mut(|data| data.insert_temp(id, presets));
}

pub fn show(ui: &mut egui::Ui, state: &mut EditorUiState, project: &ProjectLayout) {
    presets_section(ui, state, project);
    ui.separator();
    theme::section_label(ui, "LIVE VARIABLES");
    let shading = &mut state.shading;
    let mut changed = false;
    egui::Grid::new("shader_vars").num_columns(2).spacing([8.0, 3.0]).show(ui, |ui| {
        ui.label("Toon bands");
        changed |= ui.add(egui::Slider::new(&mut shading.bands, 2..=12)).changed();
        ui.end_row();
        ui.label("Band softness");
        changed |= ui.add(egui::Slider::new(&mut shading.band_softness, 0.0..=0.99)).changed();
        ui.end_row();
        ui.label("Voronoi strength");
        changed |= ui.add(egui::Slider::new(&mut shading.voronoi_strength, 0.0..=1.0)).changed();
        ui.end_row();
        ui.label("Voronoi scale");
        changed |= ui.add(egui::Slider::new(&mut shading.voronoi_scale, 0.02..=2.0)).changed();
        ui.end_row();
        ui.label("Rim strength");
        changed |= ui.add(egui::Slider::new(&mut shading.rim_strength, 0.0..=4.0)).changed();
        ui.end_row();
        ui.label("Outline width");
        changed |= ui.add(egui::Slider::new(&mut shading.outline_width, 0.0..=0.05)).changed();
        ui.end_row();
        ui.label("Outline colour");
        changed |= ui.color_edit_button_rgb(&mut shading.outline_color).changed();
        ui.end_row();
        ui.label("Rim colour");
        changed |= ui.color_edit_button_rgb(&mut shading.rim_color).changed();
        ui.end_row();
        ui.label("Voronoi colour");
        changed |= ui.color_edit_button_rgb(&mut shading.voronoi_color).changed();
        ui.end_row();
    });
    ui.horizontal(|ui| {
        changed |= ui.checkbox(&mut shading.toon, "Toon").changed();
        changed |= ui.checkbox(&mut shading.voronoi, "Voronoi").changed();
        changed |= ui.checkbox(&mut shading.rim, "Rim").changed();
        changed |= ui.checkbox(&mut shading.outline, "Outline").changed();
        changed |= ui.checkbox(&mut shading.facets, "Facets").changed();
    });
    ui.horizontal(|ui| {
        ui.label("Facet relief");
        changed |= ui
            .add_enabled(shading.facets, egui::Slider::new(&mut shading.facet_strength, 0.0..=2.0))
            .on_hover_text("Multiplies the relief of every block's low-poly Voronoi normal map")
            .changed();
    });
    ui.horizontal(|ui| {
        changed |= ui.checkbox(&mut shading.wind, "Wind").on_hover_text("Blocks with a Wind value (leaves) sway").changed();
        changed |= ui
            .add_enabled(shading.wind, egui::Slider::new(&mut shading.wind_strength, 0.0..=3.0).text("strength"))
            .changed();
    });
    ui.horizontal(|ui| {
        changed |= ui.checkbox(&mut shading.waves, "Water waves").on_hover_text("Liquid surfaces wave").changed();
        changed |= ui
            .add_enabled(shading.waves, egui::Slider::new(&mut shading.wave_height, 0.0..=0.4).text("height"))
            .changed();
    });
    if changed {
        shading.touch();
    }
    theme::tip(ui, "Each block's Voronoi density and low-poly facets are set in the Forge (Blocks workspace); sky and fog come from the biome under the player.");

    ui.separator();
    theme::section_label(ui, "WESL SOURCE");
    let id = egui::Id::new("polyhedron_shader_source");
    let mut source: SourceState = ui.ctx().data_mut(|data| data.get_temp(id)).unwrap_or_default();
    let directory = project.module_path("shaders");

    ui.horizontal(|ui| {
        for (index, file) in FILES.iter().enumerate() {
            if ui.selectable_label(source.file == index, *file).clicked() {
                source.file = index;
            }
        }
    });
    if source.loaded_file != Some(source.file) {
        let path = directory.join(FILES[source.file]);
        source.text = fs::read_to_string(&path).unwrap_or_else(|error| format!("// cannot read {}: {error}", path.display()));
        source.loaded_file = Some(source.file);
        source.dirty = false;
    }

    ui.horizontal(|ui| {
        if ui
            .add_enabled(source.dirty, egui::Button::new("Save + hot reload"))
            .clicked()
        {
            let path = directory.join(FILES[source.file]);
            match fs::write(&path, &source.text) {
                Ok(()) => {
                    source.dirty = false;
                    state.reload_shader_sources = true;
                    state.action(format!("Saved {}; reloading pipelines.", path.display()));
                }
                Err(error) => state.error(format!("cannot write {}: {error}", path.display())),
            }
        }
        if ui.button("Reload from disk").clicked() {
            source.loaded_file = None;
            state.reload_shader_sources = true;
        }
        if ui.button("Restore defaults").clicked() {
            state.reset_shader_sources = true;
            source.loaded_file = None;
        }
        if source.dirty {
            ui.label(RichText::new("modified").size(9.0).color(theme::WARN));
        }
    });

    egui::ScrollArea::vertical()
        .id_salt("shader_source_scroll")
        .max_height(ui.available_height().max(120.0))
        .show(ui, |ui| {
            let response = ui.add(
                egui::TextEdit::multiline(&mut source.text)
                    .code_editor()
                    .desired_width(f32::INFINITY)
                    .desired_rows(24),
            );
            if response.changed() {
                source.dirty = true;
            }
        });

    ui.ctx().data_mut(|data| data.insert_temp(id, source));
}
