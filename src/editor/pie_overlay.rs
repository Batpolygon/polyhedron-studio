//! Full-window overlay drawn while Play-In-Editor runs.
//!
//! The dock is folded away during PIE; this overlay draws the underwater
//! tint, the custom HUD authored in the UMG-Lite designer (layout `HUD`,
//! rendered by [`crate::ui::render::draw_layout`]), the PIE banner and the
//! FPS counter. The built-in HUD (hotbar, vitals, crosshair, status card,
//! chat) is drawn by [`crate::editor::game_ui`].

use egui::{Align2, Color32, CornerRadius, FontId, pos2, vec2};

use crate::core::app_state::EditorUiState;
use crate::core::pie_manager::PieState;
use crate::editor::theme;
use crate::gameplay::player_stats::PlayerStats;
use crate::ui::render::{draw_layout, DrawInput, HudData, WidgetState};
use crate::ui::{Binding, ButtonAction, UiLayout, WidgetKind};

/// Player-dependent bits of the overlay.
#[derive(Clone, Debug, Default)]
pub struct PlayerView {
    /// Colour of the liquid the eyes are in.
    pub underwater: Option<[u8; 3]>,
    pub show_fps: bool,
    pub paused: bool,
    pub pause_key: String,
    pub creative: bool,
    /// Where the FPS counter goes (under the status card).
    pub fps_y: f32,
}

/// Hide the layout widgets the built-in HUD already shows.
fn filter_layout(layout: &mut UiLayout, view: &PlayerView, data: &HudData) {
    for child in &mut layout.root.children {
        if child.name == "Hotbar" {
            child.visible = false;
        }
        match &child.kind {
            // No health or stamina in creative.
            WidgetKind::ProgressBar { binding: Binding::PlayerHealth | Binding::PlayerStamina, .. } if view.creative => {
                child.visible = false;
            }
            // Combo labels only while a combo is running.
            WidgetKind::Label { binding: Binding::ComboStage, .. } if data.combo == 0 => child.visible = false,
            _ => {}
        }
    }
}

/// Buttons only react while the mouse is free.
fn pointer_input(ctx: &egui::Context, pie: &PieState) -> DrawInput {
    if pie.cursor_captured {
        return DrawInput::default();
    }
    ctx.input(|input| DrawInput {
        pointer: input.pointer.hover_pos(),
        click: input.pointer.primary_clicked(),
        wheel: input.smooth_scroll_delta.y,
    })
}

/// Draw a screen above the game; returns the actions clicked.
pub fn draw_screen(ctx: &egui::Context, pie: &PieState, layout: &UiLayout, data: &HudData, widgets: &mut WidgetState) -> Vec<(ButtonAction, usize)> {
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new(("ui_screen", layout.name.as_str()))));
    draw_layout(&painter, layout, ctx.viewport_rect(), data, pointer_input(ctx, pie), widgets).clicked
}

/// Draw the overlay and the HUD; returns the HUD buttons clicked this frame.
pub fn show(
    ctx: &egui::Context,
    pie: &PieState,
    stats: &PlayerStats,
    ui: &EditorUiState,
    hud: Option<&UiLayout>,
    data: &HudData,
    view: &PlayerView,
    widgets: &mut WidgetState,
) -> Vec<(ButtonAction, usize)> {
    let screen = ctx.viewport_rect();
    let center = screen.center();

    // Underwater tint, under the HUD.
    if let Some(color) = view.underwater {
        let tint = ctx.layer_painter(egui::LayerId::new(egui::Order::Background, egui::Id::new("pie_underwater")));
        tint.rect_filled(screen, 0.0, Color32::from_rgba_unmultiplied(color[0] / 3, color[1] / 2, color[2], 110));
    }

    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("pie_overlay")));

    // UMG-Lite HUD. Buttons only react while the mouse is free.
    let mut clicked = Vec::new();
    if let Some(layout) = hud {
        let mut layout = layout.clone();
        filter_layout(&mut layout, view, data);
        clicked = draw_layout(&painter, &layout, screen, data, pointer_input(ctx, pie), widgets).clicked;
    }

    // Banner.
    if !crate::core::projects::game_only() {
    let text = format!(
        "PLAYING  {:05.1}s   F7 studio  -  {}",
        pie.elapsed,
        if view.paused {
            "paused".to_string()
        } else if pie.cursor_captured {
            format!("{} pause", view.pause_key)
        } else {
            "click to capture the mouse".to_string()
        }
    );
    let galley = painter.layout_no_wrap(text, FontId::proportional(11.0), theme::TEXT_DIM);
    let banner = egui::Rect::from_center_size(pos2(center.x, screen.top() + 18.0), galley.size() + vec2(24.0, 10.0));
    painter.rect_filled(banner, CornerRadius::same(255), theme::HUD_FILL);
    painter.galley(banner.min + vec2(12.0, 5.0), galley, theme::TEXT_DIM);
    }

    if view.show_fps {
        painter.text(
            pos2(screen.right() - 16.0, view.fps_y + 8.0),
            Align2::RIGHT_TOP,
            format!("{:.0} fps", ui.fps),
            theme::pixel(12.0),
            Color32::from_white_alpha(170),
        );
    }

    if stats.is_dead() {
        theme::hud_pill(&painter, center + vec2(0.0, -60.0), Align2::CENTER_CENTER, "YOU DIED", 24.0, theme::ERROR);
    }
    clicked
}
