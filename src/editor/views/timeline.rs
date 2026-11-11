//! 🎬 Timeline: animation clips, playback and keyframes of the selected mob.
//!
//! Top row: clip picker and clip tools, then the transport (⏮ ▶ ⏹), then key
//! tools for the selected cube. Below: a ruler you can click / drag to scrub
//! and one lane per cube with its keyframes as diamonds (drag to retime,
//! click to select, double-click a lane to add a key).

use egui::{Align2, Color32, FontId, Rect, RichText, Sense, Stroke, pos2, vec2};

use crate::core::app_state::EditorUiState;
use crate::editor::mob_studio::{MobEditMode, MobStudio};
use crate::editor::theme;
use crate::gameplay::mobs::library::MobLibrary;
use crate::gameplay::mobs::model::{Interpolation, Keyframe};

const LABEL_WIDTH: f32 = 130.0;
const ROW_HEIGHT: f32 = 22.0;

pub fn show(ui: &mut egui::Ui, library: &mut MobLibrary, studio: &mut MobStudio, state: &mut EditorUiState) {
    if library.current().is_none() {
        theme::empty_state(ui, "🎬", "Create a mob to animate it");
        return;
    }

    clip_bar(ui, library, studio, state);
    ui.add_space(2.0);

    let Some(clip) = library.current_clip() else {
        theme::empty_state(ui, "🎞", "No animation: click ➕ to create one (Idle, Walk, Attack...)");
        return;
    };
    let duration = clip.duration_or(1.0);

    egui::Frame::NONE
        .fill(theme::BG_DEEP)
        .corner_radius(10)
        .inner_margin(egui::Margin::same(6))
        .show(ui, |ui| {
            ruler(ui, library, duration);
            let bones: Vec<(String, [f32; 3])> = library
                .current()
                .map(|rig| {
                    rig.hierarchy_order()
                        .into_iter()
                        .map(|index| (rig.boxes[index].name.clone(), rig.boxes[index].color))
                        .collect()
                })
                .unwrap_or_default();
            egui::ScrollArea::vertical()
                .id_salt("timeline_tracks")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for (bone, color) in &bones {
                        track(ui, library, studio, bone, *color, duration);
                    }
                });
        });
}

fn clip_bar(ui: &mut egui::Ui, library: &mut MobLibrary, studio: &mut MobStudio, state: &mut EditorUiState) {
    ui.horizontal_wrapped(|ui| {
        // Clip picker.
        let clips: Vec<String> = library
            .current()
            .map(|rig| rig.clips.iter().map(|c| c.name.clone()).collect())
            .unwrap_or_default();
        egui::ComboBox::from_id_salt("timeline_clip")
            .selected_text(RichText::new(format!("🎞 {}", library.clip_name())).strong())
            .width(150.0)
            .show_ui(ui, |ui| {
                for name in &clips {
                    if ui.selectable_label(library.clip_selection == *name, name).clicked() {
                        library.clip_selection = name.clone();
                        library.preview.time = 0.0;
                    }
                }
            });
        if theme::icon_button(ui, "➕", "New animation").clicked() {
            if let Some(name) = library.add_clip() {
                state.action(format!("Animation '{name}' created."));
            }
            studio.edit = MobEditMode::Animate;
        }
        if theme::icon_button(ui, "🗐", "Duplicate the animation").clicked() {
            let current = library.clip_selection.clone();
            if let Some(name) = library.current_mut().and_then(|rig| rig.duplicate_clip(&current)) {
                library.clip_selection = name;
                library.touch();
            }
        }
        if theme::icon_button(ui, "🗑", "Delete the animation").clicked() {
            let current = library.clip_selection.clone();
            if let Some(rig) = library.current_mut() {
                rig.remove_clip(&current);
            }
            library.clamp_selection();
            library.touch();
        }

        // Clip settings.
        let selected = library.clip_selection.clone();
        let mut renamed: Option<String> = None;
        if let Some(rig) = library.current_mut() {
            let is_default = rig.default_clip == selected;
            let mut set_default = false;
            if let Some(clip) = rig.clip_mut(&selected) {
                let mut name = clip.name.clone();
                if ui.add(egui::TextEdit::singleline(&mut name).desired_width(90.0)).changed() && !name.trim().is_empty() {
                    clip.name = name.trim().to_string();
                    renamed = Some(clip.name.clone());
                }
                ui.add(
                    egui::DragValue::new(&mut clip.duration)
                        .range(0.05..=60.0)
                        .speed(0.02)
                        .prefix("⏱ ")
                        .suffix(" s"),
                )
                .on_hover_text("Duration");
                if theme::toggle_button(ui, clip.looping, "🔁", "Loop the animation").clicked() {
                    clip.looping = !clip.looping;
                }
                if theme::toggle_button(ui, is_default, "⭐", "Default animation (Idle)").clicked() {
                    set_default = true;
                }
            }
            if set_default {
                rig.default_clip = selected.clone();
            }
            if let Some(name) = &renamed {
                if is_default {
                    rig.default_clip = name.clone();
                }
            }
        }
        // Keep the selection in sync with a rename.
        if let Some(name) = renamed {
            library.clip_selection = name;
            library.touch();
        }

        let keys = library.current_clip().map(|clip| clip.key_count()).unwrap_or(0);
        ui.label(RichText::new(format!("🔑 {keys}")).color(theme::TEXT_DIM))
            .on_hover_text("Number of keys in this animation");

        ui.separator();

        // Transport.
        if theme::icon_button(ui, "⏮", "Back to the start").clicked() {
            library.preview.time = 0.0;
        }
        let playing = library.preview.playing;
        if theme::primary_button(ui, if playing { "⏸" } else { "▶" }, theme::ACCENT)
            .on_hover_text("Play / pause (Space)")
            .clicked()
        {
            library.preview.playing = !playing;
            studio.edit = MobEditMode::Animate;
        }
        if theme::icon_button(ui, "⏹", "Stop").clicked() {
            crate::gameplay::mobs::rewind_preview(&mut library.preview);
        }
        ui.label(
            RichText::new(format!("{:.2} / {:.2} s", library.preview.time, library.clip_duration()))
                .monospace()
                .color(theme::ACCENT),
        );
        ui.add(
            egui::DragValue::new(&mut library.preview.speed)
                .range(0.1..=4.0)
                .speed(0.02)
                .prefix("x "),
        )
        .on_hover_text("Playback speed");

        ui.separator();

        // Keys for the selected cube.
        let bone = library.bone_selection.clone();
        let enabled = bone.is_some();
        if ui
            .add_enabled(enabled, egui::Button::new("🔑 Key").corner_radius(8))
            .on_hover_text("Record the pose of the selected cube at the playhead")
            .clicked()
        {
            if let Some(bone) = &bone {
                add_key(library, bone, library.preview.time);
                studio.edit = MobEditMode::Animate;
            }
        }
        let selected_key = library.key_bone.clone().zip(library.key_time);
        if ui
            .add_enabled(selected_key.is_some(), egui::Button::new("🗑 Key").corner_radius(8))
            .on_hover_text("Delete the selected key")
            .clicked()
        {
            if let Some((bone, time)) = selected_key.clone() {
                if let Some(track) = library.current_clip_mut().and_then(|c| c.track_mut(&bone)) {
                    track.remove_at(time);
                }
                library.key_bone = None;
                library.key_time = None;
                library.touch();
            }
        }
        if let Some((bone, time)) = selected_key {
            let current = library
                .current_clip()
                .and_then(|c| c.track(&bone))
                .and_then(|t| t.key_at(time))
                .map(|k| k.interpolation);
            if let Some(current) = current {
                egui::ComboBox::from_id_salt("key_interp")
                    .selected_text(format!("〰 {}", current.label()))
                    .show_ui(ui, |ui| {
                        for interpolation in Interpolation::ALL {
                            if ui.selectable_label(current == interpolation, interpolation.label()).clicked() {
                                if let Some(key) = library
                                    .current_clip_mut()
                                    .and_then(|c| c.track_mut(&bone))
                                    .and_then(|t| t.key_at_mut(time))
                                {
                                    key.interpolation = interpolation;
                                }
                                library.touch();
                            }
                        }
                    });
            }
        }
    });
}

fn ruler(ui: &mut egui::Ui, library: &mut MobLibrary, duration: f32) {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    let lane = lane_rect(rect);
    painter.rect_filled(lane, 6.0, theme::BG_ELEVATED);

    let step = nice_step(duration, lane.width());
    let mut t = 0.0;
    while t <= duration + 1e-4 {
        let x = lane.left() + t / duration * lane.width();
        painter.line_segment([pos2(x, lane.bottom() - 6.0), pos2(x, lane.bottom())], Stroke::new(1.0, theme::TEXT_DIM));
        painter.text(pos2(x + 3.0, lane.top() + 2.0), Align2::LEFT_TOP, format!("{t:.2}s"), FontId::monospace(9.0), theme::TEXT_DIM);
        t += step;
    }
    painter.text(
        pos2(rect.left() + 6.0, rect.center().y),
        Align2::LEFT_CENTER,
        "⏱ time",
        FontId::proportional(11.0),
        theme::TEXT_DIM,
    );

    if let Some(pointer) = response.interact_pointer_pos() {
        if (response.clicked() || response.dragged()) && lane.contains(pos2(pointer.x.clamp(lane.left(), lane.right()), lane.center().y)) {
            let time = ((pointer.x - lane.left()) / lane.width()).clamp(0.0, 1.0) * duration;
            library.preview.scrub(time, duration);
            library.preview.playing = false;
        }
    }
    let x = lane.left() + (library.preview.time / duration).clamp(0.0, 1.0) * lane.width();
    painter.add(egui::Shape::convex_polygon(
        vec![pos2(x - 6.0, lane.top()), pos2(x + 6.0, lane.top()), pos2(x, lane.top() + 8.0)],
        theme::ACCENT,
        Stroke::NONE,
    ));
}

fn track(
    ui: &mut egui::Ui,
    library: &mut MobLibrary,
    studio: &mut MobStudio,
    bone: &str,
    color: [f32; 3],
    duration: f32,
) {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_HEIGHT), Sense::click());
    let painter = ui.painter_at(rect);
    let selected = library.bone_selection.as_deref() == Some(bone);
    let lane = lane_rect(rect).shrink2(vec2(0.0, 2.0));

    painter.rect_filled(rect.shrink(1.0), 6.0, if selected { theme::BG_HOVER } else { Color32::TRANSPARENT });
    painter.rect_filled(lane, 5.0, theme::BG_PANEL);
    let swatch = Rect::from_center_size(pos2(rect.left() + 12.0, rect.center().y), vec2(10.0, 10.0));
    painter.rect_filled(
        swatch,
        3.0,
        Color32::from_rgb((color[0] * 255.0) as u8, (color[1] * 255.0) as u8, (color[2] * 255.0) as u8),
    );
    painter.text(
        pos2(rect.left() + 24.0, rect.center().y),
        Align2::LEFT_CENTER,
        bone,
        FontId::proportional(11.5),
        if selected { Color32::WHITE } else { theme::TEXT },
    );
    // Playhead line.
    let x = lane.left() + (library.preview.time / duration).clamp(0.0, 1.0) * lane.width();
    painter.line_segment([pos2(x, rect.top()), pos2(x, rect.bottom())], Stroke::new(1.0, theme::ACCENT_DIM));

    if response.clicked() {
        library.bone_selection = Some(bone.to_string());
        library.socket_selection = None;
    }
    if response.double_clicked() {
        if let Some(pointer) = response.interact_pointer_pos() {
            if lane.contains(pointer) {
                let time = ((pointer.x - lane.left()) / lane.width()).clamp(0.0, 1.0) * duration;
                library.preview.scrub(time, duration);
                add_key(library, bone, time);
                studio.edit = MobEditMode::Animate;
            }
        }
    }

    let keys: Vec<(usize, f32, Interpolation)> = library
        .current_clip()
        .and_then(|clip| clip.track(bone))
        .map(|track| track.keys.iter().enumerate().map(|(i, k)| (i, k.time, k.interpolation)).collect())
        .unwrap_or_default();

    let mut retime = None;
    let mut select = None;
    for (index, time, interpolation) in keys {
        let center = pos2(lane.left() + time / duration * lane.width(), lane.center().y);
        let is_selected = library.key_bone.as_deref() == Some(bone)
            && library.key_time.map(|t| (t - time).abs() < 1e-3).unwrap_or(false);
        let hit = ui.interact(
            Rect::from_center_size(center, vec2(14.0, 14.0)),
            ui.id().with(("key", bone, index)),
            Sense::click_and_drag(),
        );
        let fill = match interpolation {
            Interpolation::Step => theme::WARN,
            Interpolation::Linear => theme::ACCENT_DIM,
            Interpolation::Bezier => theme::ACCENT,
            Interpolation::Auto => Color32::from_rgb(150, 170, 240),
        };
        let r = if is_selected || hit.hovered() { 7.0 } else { 5.5 };
        painter.add(egui::Shape::convex_polygon(
            vec![
                pos2(center.x, center.y - r),
                pos2(center.x + r, center.y),
                pos2(center.x, center.y + r),
                pos2(center.x - r, center.y),
            ],
            if is_selected { Color32::WHITE } else { fill },
            Stroke::new(1.0, theme::BG_DEEP),
        ));
        if hit.clicked() {
            select = Some(time);
        }
        if hit.dragged() {
            let delta = hit.drag_delta().x / lane.width().max(1.0) * duration;
            retime = Some((index, (time + delta).clamp(0.0, duration)));
        }
        hit.on_hover_text(format!("🔑 {bone} at {time:.2} s - {}", interpolation.label()));
    }

    if let Some(time) = select {
        library.key_bone = Some(bone.to_string());
        library.key_time = Some(time);
        library.bone_selection = Some(bone.to_string());
        library.preview.scrub(time, duration);
        library.preview.playing = false;
        studio.edit = MobEditMode::Animate;
    }
    if let Some((index, time)) = retime {
        if let Some(track) = library.current_clip_mut().and_then(|c| c.track_mut(bone)) {
            if let Some(key) = track.keys.get_mut(index) {
                key.time = time;
            }
            track.keys.sort_by(|a, b| a.time.total_cmp(&b.time));
        }
        library.key_bone = Some(bone.to_string());
        library.key_time = Some(time);
        library.touch();
    }
}

fn add_key(library: &mut MobLibrary, bone: &str, time: f32) {
    let duration = library.clip_duration();
    let time = time.clamp(0.0, duration);
    let pose = library
        .current()
        .zip(library.current_clip())
        .map(|(rig, clip)| crate::gameplay::mobs::pose::sample_clip(rig, clip, time).get(bone))
        .unwrap_or_default();
    if let Some(clip) = library.current_clip_mut() {
        clip.ensure_track(bone).upsert(Keyframe {
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

fn lane_rect(rect: Rect) -> Rect {
    Rect::from_min_max(pos2((rect.left() + LABEL_WIDTH).min(rect.right() - 8.0), rect.top()), rect.max)
}

fn nice_step(duration: f32, width: f32) -> f32 {
    let raw = duration / (width / 70.0).max(2.0);
    [0.05, 0.1, 0.25, 0.5, 1.0, 2.0, 5.0, 10.0, 30.0, 60.0]
        .into_iter()
        .find(|c| *c >= raw)
        .unwrap_or(120.0)
}
