//! 🔬 Détails: transform, size, pivot and colour of the selected cube, the
//! keyframe under the playhead in Animation mode, and mob-wide settings.

use egui::{Color32, RichText};

use crate::core::app_state::EditorUiState;
use crate::editor::mob_studio::{MobEditMode, MobStudio};
use crate::editor::theme;
use crate::gameplay::mobs::library::MobLibrary;
use crate::gameplay::mobs::model::Interpolation;

/// Friendly palette for one-click colours.
const SWATCHES: [[f32; 3]; 12] = [
    [0.93, 0.93, 0.93],
    [0.35, 0.35, 0.38],
    [0.86, 0.30, 0.30],
    [0.95, 0.60, 0.25],
    [0.97, 0.82, 0.35],
    [0.45, 0.80, 0.40],
    [0.25, 0.60, 0.35],
    [0.31, 0.72, 0.78],
    [0.30, 0.50, 0.90],
    [0.60, 0.42, 0.88],
    [0.92, 0.50, 0.75],
    [0.55, 0.38, 0.25],
];

/// `X Y Z` drag fields coloured like the gizmo axes.
fn vec3_fields(ui: &mut egui::Ui, value: &mut [f32; 3], speed: f64, suffix: &str) -> bool {
    let colors = [
        Color32::from_rgb(240, 90, 95),
        Color32::from_rgb(120, 220, 110),
        Color32::from_rgb(95, 145, 250),
    ];
    let mut changed = false;
    ui.horizontal(|ui| {
        for (index, component) in value.iter_mut().enumerate() {
            ui.label(RichText::new(["X", "Y", "Z"][index]).color(colors[index]).strong());
            changed |= ui
                .add(
                    egui::DragValue::new(component)
                        .speed(speed)
                        .fixed_decimals(3)
                        .suffix(suffix),
                )
                .changed();
        }
    });
    changed
}

pub fn show(
    ui: &mut egui::Ui,
    library: &mut MobLibrary,
    studio: &mut MobStudio,
    state: &mut EditorUiState,
    programs: &[String],
) {
    let Some(rig) = library.current() else {
        theme::empty_state(ui, "🐾", "No mob selected");
        return;
    };
    let rig_name = rig.name.clone();
    let bones: Vec<String> = rig.boxes.iter().map(|b| b.name.clone()).collect();

    egui::ScrollArea::vertical().id_salt("mob_details_scroll").show(ui, |ui| {
        // --- Selected socket ----------------------------------------------------
        if let Some(socket_name) = library.socket_selection.clone() {
            if let Some(socket) = library
                .current_mut()
                .and_then(|r| r.sockets.iter_mut().find(|s| s.name == socket_name))
            {
                let mut changed = false;
                let mut renamed = None;
                theme::card(ui, |ui| {
                    theme::header(ui, "📌", "Socket");
                    if ui.text_edit_singleline(&mut socket.name).changed() {
                        renamed = Some(socket.name.clone());
                        changed = true;
                    }
                    ui.horizontal(|ui| {
                        theme::prop_label(ui, "📦", "Cube");
                        egui::ComboBox::from_id_salt("socket_bone")
                            .selected_text(socket.bone.as_str())
                            .show_ui(ui, |ui| {
                                for bone in &bones {
                                    changed |= ui.selectable_value(&mut socket.bone, bone.clone(), bone).changed();
                                }
                            });
                    });
                    theme::prop_label(ui, "📐", "Position");
                    changed |= vec3_fields(ui, &mut socket.offset, 0.01, "");
                    theme::prop_label(ui, "⟳", "Rotation");
                    changed |= vec3_fields(ui, &mut socket.rotation, 1.0, " deg");
                });
                if let Some(name) = renamed {
                    library.socket_selection = Some(name);
                }
                if changed {
                    library.touch();
                }
                return;
            }
        }

        // --- Selected cube ----------------------------------------------------------
        let Some(bone_name) = library.bone_selection.clone() else {
            theme::empty_state(ui, "👆", "Click a cube in the 3D view or in the skeleton");
            mob_settings(ui, library, programs, &rig_name);
            return;
        };
        let parent_options: Vec<String> = library
            .current()
            .map(|rig| {
                rig.boxes
                    .iter()
                    .filter(|b| b.name != bone_name && !rig.is_ancestor(&bone_name, &b.name))
                    .map(|b| b.name.clone())
                    .collect()
            })
            .unwrap_or_default();

        // Name + parent.
        let mut new_name = bone_name.clone();
        let mut new_parent: Option<Option<String>> = None;
        theme::card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("📦").size(16.0));
                ui.add(egui::TextEdit::singleline(&mut new_name).desired_width(ui.available_width() - 4.0));
            });
            let current_parent = library
                .current()
                .and_then(|r| r.bone(&bone_name))
                .and_then(|b| b.parent.clone());
            ui.horizontal(|ui| {
                theme::prop_label(ui, "🔗", "Parent");
                egui::ComboBox::from_id_salt("bone_parent")
                    .selected_text(current_parent.clone().unwrap_or_else(|| "- none -".into()))
                    .show_ui(ui, |ui| {
                        if ui.selectable_label(current_parent.is_none(), "- none -").clicked() {
                            new_parent = Some(None);
                        }
                        for option in &parent_options {
                            if ui.selectable_label(current_parent.as_deref() == Some(option), option).clicked() {
                                new_parent = Some(Some(option.clone()));
                            }
                        }
                    });
            });
        });
        if new_name != bone_name && !new_name.trim().is_empty() {
            if let Some(rig) = library.current_mut() {
                match rig.rename_box(&bone_name, new_name.trim()) {
                    Ok(()) => {
                        library.bone_selection = Some(new_name.trim().to_string());
                        library.touch();
                    }
                    Err(error) => state.warn(error),
                }
            }
            return;
        }
        if let Some(parent) = new_parent {
            if let Some(rig) = library.current_mut() {
                match rig.reparent(&bone_name, parent) {
                    Ok(()) => library.touch(),
                    Err(error) => state.warn(error),
                }
            }
        }

        // Animation mode: the keyframe under the playhead.
        if studio.edit == MobEditMode::Animate {
            keyframe_card(ui, library, &bone_name);
        }

        // Transform.
        let Some(bone) = library.current_mut().and_then(|r| r.bone_mut(&bone_name)) else {
            return;
        };
        let mut changed = false;
        theme::card(ui, |ui| {
            theme::header(ui, "📐", "Transform (model)");
            theme::prop_label(ui, "✛", "Position");
            changed |= vec3_fields(ui, &mut bone.offset, 0.01, "");
            theme::prop_label(ui, "↗", "Size");
            changed |= vec3_fields(ui, &mut bone.size, 0.01, "");
            theme::prop_label(ui, "⟳", "Rotation");
            changed |= vec3_fields(ui, &mut bone.rotation, 1.0, " deg");
            theme::prop_label(ui, "📍", "Pivot point");
            changed |= vec3_fields(ui, &mut bone.pivot, 0.01, "");
            ui.horizontal(|ui| {
                if ui.small_button("⟲ Rotation 0").clicked() {
                    bone.rotation = [0.0; 3];
                    changed = true;
                }
                if ui.small_button("📍 Pivot at the centre").clicked() {
                    bone.pivot = [0.0; 3];
                    changed = true;
                }
                if ui
                    .small_button("📍 Pivot at the bottom")
                    .on_hover_text("Handy for legs and arms")
                    .clicked()
                {
                    bone.pivot = [0.0, -bone.size[1] * 0.5, 0.0];
                    changed = true;
                }
            });
        });

        theme::card(ui, |ui| {
            theme::header(ui, "🎨", "Colour");
            ui.horizontal(|ui| {
                changed |= ui.color_edit_button_rgb(&mut bone.color).changed();
                changed |= ui.checkbox(&mut bone.visible, "👁 Visible").changed();
            });
            changed |= ui.add(egui::Slider::new(&mut bone.emissive, 0.0..=6.0).text("Glow")).changed();
            ui.horizontal_wrapped(|ui| {
                for swatch in SWATCHES {
                    let color = Color32::from_rgb((swatch[0] * 255.0) as u8, (swatch[1] * 255.0) as u8, (swatch[2] * 255.0) as u8);
                    let button = egui::Button::new("")
                        .fill(color)
                        .corner_radius(6)
                        .min_size(egui::vec2(20.0, 20.0));
                    if ui.add(button).clicked() {
                        bone.color = swatch;
                        changed = true;
                    }
                }
            });
        });
        if changed {
            library.touch();
        }

        mob_settings(ui, library, programs, &rig_name);
    });
}

fn keyframe_card(ui: &mut egui::Ui, library: &mut MobLibrary, bone: &str) {
    let time = library.preview.time;
    theme::card(ui, |ui| {
        theme::header(ui, "🔑", &format!("Key at {time:.2} s"));
        let exists = library
            .current_clip()
            .and_then(|clip| clip.track(bone))
            .and_then(|track| track.key_at(time))
            .is_some();
        if !exists {
            theme::hint(ui, "No key here. Move the cube with the gizmo or:");
            if ui.button("🔑 Create a key").clicked() {
                let pose = library
                    .current()
                    .zip(library.current_clip())
                    .map(|(rig, clip)| crate::gameplay::mobs::pose::sample_clip(rig, clip, time).get(bone))
                    .unwrap_or_default();
                if let Some(clip) = library.current_clip_mut() {
                    clip.ensure_track(bone).upsert(crate::gameplay::mobs::model::Keyframe {
                        time,
                        translation: pose.translation.to_array(),
                        rotation: pose.rotation.to_array(),
                        scale: pose.scale.to_array(),
                        interpolation: Interpolation::Auto,
                    });
                }
                library.key_bone = Some(bone.to_string());
                library.key_time = Some(time);
                library.touch();
            }
            return;
        }
        let mut changed = false;
        let mut delete = false;
        if let Some(key) = library
            .current_clip_mut()
            .and_then(|clip| clip.track_mut(bone))
            .and_then(|track| track.key_at_mut(time))
        {
            theme::prop_label(ui, "✛", "Offset");
            changed |= vec3_fields(ui, &mut key.translation, 0.01, "");
            theme::prop_label(ui, "⟳", "Rotation");
            changed |= vec3_fields(ui, &mut key.rotation, 1.0, " deg");
            theme::prop_label(ui, "↗", "Scale");
            changed |= vec3_fields(ui, &mut key.scale, 0.01, "");
            ui.horizontal(|ui| {
                theme::prop_label(ui, "〰", "Curve");
                for interpolation in Interpolation::ALL {
                    if theme::toggle_button(ui, key.interpolation == interpolation, interpolation.label(), "Interpolation to the next key").clicked() {
                        key.interpolation = interpolation;
                        changed = true;
                    }
                }
            });
            if ui.button("🗑 Delete this key").clicked() {
                delete = true;
            }
        }
        if delete {
            if let Some(track) = library.current_clip_mut().and_then(|clip| clip.track_mut(bone)) {
                track.remove_at(time);
            }
            changed = true;
        }
        if changed {
            library.touch();
        }
    });
}

fn mob_settings(ui: &mut egui::Ui, library: &mut MobLibrary, programs: &[String], rig_name: &str) {
    let Some(rig) = library.current_mut() else {
        return;
    };
    let mut changed = false;
    egui::CollapsingHeader::new(RichText::new(format!("⚙ Settings of '{rig_name}'")).strong())
        .default_open(false)
        .show(ui, |ui| {
            theme::card(ui, |ui| {
                ui.horizontal(|ui| {
                    theme::prop_label(ui, "🏷", "Name");
                    changed |= ui.text_edit_singleline(&mut rig.name).changed();
                });
                theme::prop_label(ui, "📝", "Description");
                changed |= ui.text_edit_multiline(&mut rig.description).changed();
                theme::prop_label(ui, "↗", "Global scale");
                changed |= vec3_fields(ui, &mut rig.scale, 0.01, "");
            });
            theme::card(ui, |ui| {
                theme::header(ui, "🎮", "In game");
                ui.horizontal(|ui| {
                    theme::prop_label(ui, "❤", "Health");
                    changed |= ui.add(egui::DragValue::new(&mut rig.health).range(1.0..=1000.0)).changed();
                    theme::prop_label(ui, "🔢", "Count");
                    changed |= ui
                        .add(egui::DragValue::new(&mut rig.spawn_count).range(0..=16))
                        .on_hover_text("Copies spawned around the player on launch (F7)")
                        .changed();
                });
                changed |= ui.checkbox(&mut rig.hostile, "⚔ Hostile (attacks the player)").changed();
                theme::prop_label(ui, "🕸", "Scripts (Weave)");
                let mut scripts = rig.all_scripts();
                let mut scripts_changed = false;
                let mut remove = None;
                for (index, name) in scripts.iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(format!("🕸 {name}")).color(theme::TEXT));
                        if ui.small_button("🗑").on_hover_text("Remove this script").clicked() {
                            remove = Some(index);
                        }
                    });
                }
                if let Some(index) = remove {
                    scripts.remove(index);
                    scripts_changed = true;
                }
                egui::ComboBox::from_id_salt("mob_script_add")
                    .selected_text("➕ Add a script")
                    .show_ui(ui, |ui| {
                        for program in programs {
                            if !scripts.contains(program) && ui.selectable_label(false, program).clicked() {
                                scripts.push(program.clone());
                                scripts_changed = true;
                            }
                        }
                    });
                if scripts_changed {
                    rig.set_scripts(scripts);
                    changed = true;
                }
                egui::Grid::new("mob_stats").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
                    ui.label("🏃 Speed x");
                    changed |= ui.add(egui::Slider::new(&mut rig.speed, 0.0..=5.0)).changed();
                    ui.end_row();
                    ui.label("⚔ Damage x");
                    changed |= ui.add(egui::Slider::new(&mut rig.damage, 0.0..=10.0)).changed();
                    ui.end_row();
                    ui.label("⏱ Attack cooldown (s)");
                    changed |= ui.add(egui::Slider::new(&mut rig.attack_cooldown, 0.05..=5.0)).changed();
                    ui.end_row();
                    ui.label("📏 Reach x");
                    changed |= ui.add(egui::Slider::new(&mut rig.reach, 0.2..=4.0)).changed();
                    ui.end_row();
                    ui.label("🛡 Knockback resistance");
                    changed |= ui.add(egui::Slider::new(&mut rig.knockback_resistance, 0.0..=1.0)).changed();
                    ui.end_row();
                });
                ui.horizontal(|ui| {
                    theme::prop_label(ui, "💎", "Loot");
                    changed |= crate::editor::pickers::pick_option(ui, "mob_loot", &mut rig.loot, &[crate::editor::pickers::Kind::Resource], "none");
                });
                ui.horizontal(|ui| {
                    ui.label("Hurt sound");
                    changed |= crate::editor::views::ambience::sound_field(ui, "mob_hurt_sound", &mut rig.hurt_sound);
                    ui.label("Death sound");
                    changed |= crate::editor::views::ambience::sound_field(ui, "mob_death_sound", &mut rig.death_sound);
                });
                if rig.behavior == crate::gameplay::mobs::model::MobBehavior::Brain {
                    egui::CollapsingHeader::new("⚙ Brain").default_open(true).show(ui, |ui| {
                        changed |= crate::editor::views::assembly::mob_brain(ui, &mut rig.brain);
                    });
                }
                // Native behaviour, natural spawning and drops.
                egui::Grid::new("mob_behaviour").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
                    use crate::gameplay::mobs::model::{MobBehavior, SpawnTime};
                    ui.label("🚦 Behaviour");
                    egui::ComboBox::from_id_salt("mob_behavior").selected_text(rig.behavior.label()).show_ui(ui, |ui| {
                        for behavior in MobBehavior::ALL {
                            changed |= ui.selectable_value(&mut rig.behavior, behavior, behavior.label()).changed();
                        }
                    });
                    ui.end_row();
                    ui.label("🌍 Biomes");
                    changed |= crate::editor::pickers::pick_list(ui, "mob_biomes", &mut rig.biomes, &[crate::editor::pickers::Kind::Biome]);
                    ui.end_row();
                    ui.label("🌙 Spawns");
                    egui::ComboBox::from_id_salt("mob_spawn_time").selected_text(rig.spawn_time.label()).show_ui(ui, |ui| {
                        for time in SpawnTime::ALL {
                            changed |= ui.selectable_value(&mut rig.spawn_time, time, time.label()).changed();
                        }
                    });
                    ui.end_row();
                    ui.label("✨ Ethereal");
                    changed |= ui.add(egui::Slider::new(&mut rig.ethereal, 0.0..=50.0)).changed();
                    ui.end_row();
                    ui.label("🎁 Drops");
                    changed |= crate::editor::pickers::pick_amounts(ui, "mob_drops", &mut rig.drops, &[crate::editor::pickers::Kind::Resource, crate::editor::pickers::Kind::Item]);
                    ui.end_row();
                    ui.label("😨 Fears");
                    changed |= crate::editor::pickers::pick_list(ui, "mob_fears", &mut rig.fears, &[crate::editor::pickers::Kind::Mob]);
                    ui.end_row();
                    ui.label("🐎 Rideable");
                    changed |= ui.checkbox(&mut rig.rideable, "").changed();
                    ui.end_row();
                    ui.label("💥 Explodes on death");
                    changed |= ui.checkbox(&mut rig.explodes, "").changed();
                    ui.end_row();
                });
            });
        });
    if changed {
        library.touch();
    }
}
