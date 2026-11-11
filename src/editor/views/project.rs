//! Project browser panel.
//!
//! The panel reads the open `.ron` project directly from disk and exposes the
//! world save/load controls. The content tree is in `project_tools`.

use egui::RichText;

use crate::core::app_state::EditorUiState;
use crate::core::project_io::ProjectLayout;
use crate::editor::theme;
use crate::editor::world_actions;
use crate::voxel::blocks::BlockRegistry;
use crate::voxel::generator;
use crate::voxel::world::VoxelWorld;

pub fn show(
    ui: &mut egui::Ui,
    project: &ProjectLayout,
    state: &mut EditorUiState,
    world: &mut VoxelWorld,
    registry: &BlockRegistry,
) {
    theme::section_label(ui, "PROJECT");
    ui.label(
        RichText::new(&project.manifest.name)
            .size(13.0)
            .strong()
            .color(theme::TEXT),
    );
    ui.label(
        RichText::new(project.root.display().to_string())
            .size(9.5)
            .color(theme::TEXT_DIM),
    );

    ui.horizontal(|ui| {
        ui.label(
            RichText::new(format!("format v{}", project.manifest.format_version))
                .size(9.5)
                .color(theme::TEXT_DIM),
        );
        ui.label(
            RichText::new(format!("engine {}", project.manifest.engine_version))
                .size(9.5)
                .color(theme::TEXT_DIM),
        );
    });

    ui.separator();
    theme::section_label(ui, "WORLD");

    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(&mut state.world_name)
                .hint_text("world name")
                .desired_width(150.0),
        );

        if ui.button("Save").clicked() {
            world_actions::save_current(world, registry, project, state);
        }

        if ui.button("Load").clicked() {
            let name = state.world_name.clone();
            world_actions::load_named(world, registry, project, state, &name);
        }
    });

    let stats = world.stats();
    let status = if world.revision == state.last_saved_revision {
        "saved"
    } else {
        "modified"
    };
    ui.label(
        RichText::new(format!(
            "{status}  •  {} chunks  •  {} voxels  •  {} blocks",
            stats.chunks,
            stats.voxels,
            registry.len()
        ))
        .size(9.5)
        .color(if status == "saved" {
            theme::ACCENT_DIM
        } else {
            theme::WARN
        }),
    );

    let worlds = world_actions::list_worlds(project);
    if worlds.is_empty() {
        ui.label(
            RichText::new("No saved worlds. Ctrl+S writes project/worlds/.")
                .size(9.5)
                .color(theme::TEXT_DIM)
                .italics(),
        );
    } else {
        egui::ScrollArea::vertical()
            .id_salt("polyhedron_world_list")
            .max_height(96.0)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for name in &worlds {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(name)
                                .size(10.0)
                                .color(theme::TEXT_DIM),
                        );
                        if ui.small_button("Load").clicked() {
                            world_actions::load_named(
                                world, registry, project, state, name,
                            );
                        }
                    });
                }
            });
    }

    if ui
        .button("Regenerate demo world")
        .on_hover_text("Discard the current world and rebuild the procedural sample.")
        .clicked()
    {
        generator::generate_demo_world(world, registry);
        state.action("Demo world regenerated.");
    }

    ui.separator();
    ui.label(
        RichText::new(format!("entry: {}", project.manifest.entry_scene))
            .size(9.5)
            .color(theme::TEXT_DIM),
    );
    if ui
        .button(RichText::new("Log project summary").size(10.5))
        .clicked()
    {
        state.info(format!(
            "{} - {} modules, entry scene '{}'",
            project.manifest.name,
            project.modules.len(),
            project.manifest.entry_scene,
        ));
    }
}
