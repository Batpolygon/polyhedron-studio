//! Project panel, second part: projects, the content tree (search, rename,
//! safe delete), the global validation and the game export.

use std::path::PathBuf;

use egui::RichText;

use crate::audio::synth::bank;
use crate::core::app_state::EditorUiState;
use crate::core::project_io::ProjectLayout;
use crate::core::projects;
use crate::editor::pickers::catalog;
use crate::editor::theme;
use crate::editor::validation::{validate, Issue};
use crate::gameplay::atmosphere::atmosphere;
use crate::gameplay::crafting::book;
use crate::gameplay::mobs::library::MobLibrary;
use crate::gameplay::quests::story;
use crate::scripting::library::ScriptLibrary;
use crate::voxel::biomes::BiomeLibrary;
use crate::voxel::structures::StructureLibrary;

/// Libraries the validation reads.
pub struct Libraries<'a> {
    pub biomes: &'a BiomeLibrary,
    pub structures: &'a StructureLibrary,
    pub mobs: &'a MobLibrary,
    pub scripts: &'a ScriptLibrary,
}

/// What the panel remembers between frames.
#[derive(Clone, Debug, Default)]
struct PanelState {
    new_name: String,
    example: bool,
    search: String,
    /// File being renamed and its new name.
    renaming: Option<(PathBuf, String)>,
    /// File asked to be deleted, with the files that use it.
    deleting: Option<(PathBuf, Vec<PathBuf>)>,
    issues: Option<Vec<Issue>>,
    /// Files of the project, read again from time to time.
    files: Vec<PathBuf>,
    files_read_at: f64,
}

fn restart(state: &mut EditorUiState, quit: &mut crate::editor::menus::QuitRequest, root: &std::path::Path) {
    match projects::open_project(root) {
        Ok(()) => quit.frames = Some(1),
        Err(error) => state.error(error),
    }
}

fn projects_section(ui: &mut egui::Ui, panel: &mut PanelState, state: &mut EditorUiState, project: &ProjectLayout, quit: &mut crate::editor::menus::QuitRequest) {
    theme::tip(ui, "Opening a project restarts the application. Save your work first.");
    for root in projects::known_projects() {
        ui.horizontal(|ui| {
            let current = root == project.root;
            let name = root.file_name().map(|name| name.to_string_lossy().to_string()).unwrap_or_default();
            ui.label(RichText::new(name).size(10.5).color(if current { theme::ACCENT } else { theme::TEXT })).on_hover_text(root.display().to_string());
            if !current && ui.small_button("Open").clicked() {
                restart(state, quit, &root);
            }
        });
    }
    ui.horizontal(|ui| {
        ui.add(egui::TextEdit::singleline(&mut panel.new_name).hint_text("project name").desired_width(140.0));
        ui.checkbox(&mut panel.example, "Example").on_hover_text("With a quest line, a dialogue, an achievement and a song");
    });
    let base = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    ui.horizontal(|ui| {
        if ui.button("➕ New").clicked() {
            match projects::create_project(&base, &panel.new_name, panel.example) {
                Ok(root) => restart(state, quit, &root),
                Err(error) => state.error(error),
            }
        }
        if ui.button("🗐 Duplicate this one").clicked() {
            match projects::duplicate_project(&base, &project.root, &panel.new_name) {
                Ok(root) => restart(state, quit, &root),
                Err(error) => state.error(error),
            }
        }
    });
}

fn tree_section(ui: &mut egui::Ui, panel: &mut PanelState, state: &mut EditorUiState, project: &ProjectLayout, quit: &mut crate::editor::menus::QuitRequest) {
    ui.add(egui::TextEdit::singleline(&mut panel.search).hint_text("🔍 search the project").desired_width(200.0));
    let needle = panel.search.to_lowercase();
    if let Some((path, name)) = &mut panel.renaming {
        ui.label(format!("Rename {}", path.file_name().unwrap_or_default().to_string_lossy()));
        ui.text_edit_singleline(name);
        let (mut confirm, mut cancel) = (false, false);
        ui.horizontal(|ui| {
            confirm = ui.button("Rename everywhere").clicked();
            cancel = ui.button("Cancel").clicked();
        });
        if confirm {
            match projects::rename_asset(&project.root, path, name) {
                Ok(_) => restart(state, quit, &project.root),
                Err(error) => state.error(error),
            }
        }
        if confirm || cancel {
            panel.renaming = None;
        }
    }
    if let Some((path, users)) = &panel.deleting {
        let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
        if users.is_empty() {
            ui.label(format!("Delete {name}? Nothing uses it."));
        } else {
            ui.label(RichText::new(format!("{name} is used by {} file(s):", users.len())).color(theme::WARN));
            for user in users.iter().take(8) {
                ui.label(RichText::new(user.strip_prefix(&project.root).unwrap_or(user).display().to_string()).size(9.5).color(theme::TEXT_DIM));
            }
        }
        let (mut confirm, mut cancel) = (false, false);
        ui.horizontal(|ui| {
            confirm = ui.button("Move to the trash").on_hover_text("It stays in project/.trash").clicked();
            cancel = ui.button("Cancel").clicked();
        });
        if confirm {
            match projects::trash_asset(&project.root, path) {
                Ok(_) => restart(state, quit, &project.root),
                Err(error) => state.error(error),
            }
        }
        if confirm || cancel {
            panel.deleting = None;
        }
    }
    let now = ui.input(|input| input.time);
    if panel.files.is_empty() || now - panel.files_read_at > 3.0 {
        panel.files = projects::project_files(&project.root);
        panel.files_read_at = now;
    }
    let files = panel.files.clone();
    egui::ScrollArea::vertical().id_salt("project_tree").max_height(260.0).auto_shrink([false, true]).show(ui, |ui| {
        let mut folder = None;
        let mut open = false;
        for path in &files {
            let relative = path.strip_prefix(&project.root).unwrap_or(path);
            let text = relative.display().to_string();
            if !needle.is_empty() && !text.to_lowercase().contains(&needle) {
                continue;
            }
            let parent = relative.parent().map(|parent| parent.display().to_string()).unwrap_or_default();
            if folder.as_ref() != Some(&parent) {
                let title = if parent.is_empty() { "project".to_string() } else { parent.clone() };
                let id = ui.id().with(&parent);
                open = ui.ctx().data_mut(|d| d.get_temp::<bool>(id)).unwrap_or(false) || !needle.is_empty();
                if ui.selectable_label(open, RichText::new(format!("📁 {title}")).size(10.5)).clicked() {
                    open = !open;
                    ui.ctx().data_mut(|d| d.insert_temp(id, open));
                }
                folder = Some(parent);
            }
            if !open {
                continue;
            }
            ui.horizontal(|ui| {
                ui.add_space(12.0);
                ui.label(RichText::new(relative.file_name().unwrap_or_default().to_string_lossy()).size(10.0).color(theme::TEXT_DIM));
                if ui.small_button("✏").on_hover_text("Rename, and every reference to it").clicked() {
                    match projects::asset_id(path) {
                        Some(id) => panel.renaming = Some((path.clone(), id)),
                        None => state.warn("This file has no name to change."),
                    }
                }
                if ui.small_button("🗑").on_hover_text("Delete safely").clicked() {
                    let users = projects::asset_id(path).map(|id| projects::references(&project.root, &id, path)).unwrap_or_default();
                    panel.deleting = Some((path.clone(), users));
                }
            });
        }
    });
}

fn check(libraries: &Libraries) -> Vec<Issue> {
    let mut issues = validate(&catalog(), &book(), libraries.biomes, libraries.structures, &bank(), &atmosphere(), &story());
    issues.extend(libraries.mobs.validate().into_iter().map(|issue| Issue { area: issue.location, message: issue.message }));
    issues.extend(libraries.scripts.validate().into_iter().map(|issue| Issue { area: issue.location, message: issue.message }));
    issues
}

pub fn show(ui: &mut egui::Ui, state: &mut EditorUiState, project: &ProjectLayout, libraries: Libraries, quit: &mut crate::editor::menus::QuitRequest) {
    let id = egui::Id::new("project_tools");
    let mut panel: PanelState = ui.ctx().data_mut(|d| d.get_temp(id)).unwrap_or_default();

    egui::CollapsingHeader::new(RichText::new("PROJECTS").size(9.5).color(theme::ACCENT_DIM)).show(ui, |ui| {
        projects_section(ui, &mut panel, state, project, quit);
    });
    egui::CollapsingHeader::new(RichText::new("CONTENT").size(9.5).color(theme::ACCENT_DIM)).default_open(true).show(ui, |ui| {
        tree_section(ui, &mut panel, state, project, quit);
    });
    egui::CollapsingHeader::new(RichText::new("VALIDATION").size(9.5).color(theme::ACCENT_DIM)).default_open(true).show(ui, |ui| {
        if ui.button("✔ Check the whole project").clicked() {
            panel.issues = Some(check(&libraries));
        }
        if let Some(issues) = &panel.issues {
            if issues.is_empty() {
                ui.label(RichText::new("No problem found.").color(theme::ACCENT));
            }
            for issue in issues.iter().take(60) {
                ui.label(RichText::new(format!("{}: {}", issue.area, issue.message)).size(9.5).color(theme::WARN));
            }
            if issues.len() > 60 {
                ui.label(RichText::new(format!("+{} more", issues.len() - 60)).size(9.5).color(theme::TEXT_DIM));
            }
        }
    });
    egui::CollapsingHeader::new(RichText::new("EXPORT").size(9.5).color(theme::ACCENT_DIM)).show(ui, |ui| {
        theme::tip(ui, "Writes exports/<project>: a game that starts without the studio.");
        if ui.button("📦 Export the game").clicked() {
            match projects::export_current(project) {
                Ok(target) => state.action(format!("Game exported to {}.", target.display())),
                Err(error) => state.error(error),
            }
        }
    });
    ui.ctx().data_mut(|d| d.insert_temp(id, panel));
}
