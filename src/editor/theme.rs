//! Studio visual identity.
//!
//! All colours are defined here so panels, the dock chrome and the viewport
//! overlays stay consistent when new tools are added.

use egui::{Color32, CornerRadius, FontId, Margin, Stroke, TextStyle, vec2};

pub const BG_DEEP: Color32 = Color32::from_rgb(8, 12, 17);
pub const BG_PANEL: Color32 = Color32::from_rgb(15, 21, 29);
pub const BG_ELEVATED: Color32 = Color32::from_rgb(22, 31, 42);
pub const BG_HOVER: Color32 = Color32::from_rgb(29, 42, 56);
pub const BORDER: Color32 = Color32::from_rgb(34, 48, 64);
pub const ACCENT: Color32 = Color32::from_rgb(78, 210, 190);
pub const ACCENT_DIM: Color32 = Color32::from_rgb(46, 140, 128);
pub const TEXT: Color32 = Color32::from_rgb(201, 214, 227);
pub const TEXT_DIM: Color32 = Color32::from_rgb(124, 139, 155);
pub const WARN: Color32 = Color32::from_rgb(240, 184, 92);
pub const ERROR: Color32 = Color32::from_rgb(240, 104, 104);

/// Apply the Polyhedron dark theme to every egui style.
pub fn apply_theme(ctx: &egui::Context) {
    install_pixel_font(ctx);
    ctx.set_theme(egui::ThemePreference::Dark);

    ctx.all_styles_mut(|style| {
        style.visuals = egui::Visuals::dark();
        style.visuals.panel_fill = BG_PANEL;
        style.visuals.window_fill = BG_PANEL;
        style.visuals.extreme_bg_color = BG_DEEP;
        style.visuals.text_edit_bg_color = Some(BG_DEEP);
        style.visuals.code_bg_color = BG_ELEVATED;
        style.visuals.faint_bg_color = BG_ELEVATED;
        style.visuals.override_text_color = Some(TEXT);
        style.visuals.window_stroke = Stroke::new(1.0, BORDER);
        style.visuals.window_corner_radius = CornerRadius::same(10);
        style.visuals.menu_corner_radius = CornerRadius::same(8);
        style.visuals.hyperlink_color = ACCENT;
        style.visuals.warn_fg_color = WARN;
        style.visuals.error_fg_color = ERROR;

        style.visuals.selection.bg_fill = ACCENT_DIM;
        style.visuals.selection.stroke = Stroke::new(1.0, ACCENT);

        style.visuals.widgets.noninteractive.bg_fill = BG_PANEL;
        style.visuals.widgets.noninteractive.weak_bg_fill = BG_PANEL;
        style.visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
        style.visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);

        style.visuals.widgets.inactive.bg_fill = BG_ELEVATED;
        style.visuals.widgets.inactive.weak_bg_fill = BG_ELEVATED;
        style.visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, BORDER);
        style.visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT);

        style.visuals.widgets.hovered.bg_fill = BG_HOVER;
        style.visuals.widgets.hovered.weak_bg_fill = BG_HOVER;
        style.visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, ACCENT_DIM);
        style.visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, Color32::WHITE);

        style.visuals.widgets.active.bg_fill = ACCENT_DIM;
        style.visuals.widgets.active.weak_bg_fill = ACCENT_DIM;
        style.visuals.widgets.active.bg_stroke = Stroke::new(1.0, ACCENT);
        style.visuals.widgets.active.fg_stroke = Stroke::new(1.0, Color32::WHITE);

        // Rounded, friendly controls.
        for widgets in [
            &mut style.visuals.widgets.noninteractive,
            &mut style.visuals.widgets.inactive,
            &mut style.visuals.widgets.hovered,
            &mut style.visuals.widgets.active,
            &mut style.visuals.widgets.open,
        ] {
            widgets.corner_radius = CornerRadius::same(7);
        }
        style.visuals.widgets.open.bg_fill = BG_HOVER;
        style.visuals.widgets.open.weak_bg_fill = BG_HOVER;
        style.visuals.slider_trailing_fill = true;
        style.visuals.handle_shape = egui::style::HandleShape::Circle;

        style.spacing.item_spacing = vec2(8.0, 6.0);
        style.spacing.button_padding = vec2(10.0, 5.0);
        style.spacing.interact_size.y = 26.0;
        style.spacing.slider_rail_height = 6.0;
        style.spacing.indent = 14.0;

        style.text_styles.insert(TextStyle::Small, FontId::proportional(10.5));
        style.text_styles.insert(TextStyle::Body, FontId::proportional(12.5));
        style.text_styles.insert(TextStyle::Button, FontId::proportional(12.0));
        style.text_styles.insert(TextStyle::Heading, FontId::proportional(15.0));
        style.text_styles.insert(TextStyle::Monospace, FontId::monospace(11.0));
    });
}

/// Name of the game UI font family ("lcddot_tr").
pub const PIXEL_FAMILY: &str = "pixel";

/// egui only applies `set_fonts` at the start of the *next* pass, so the
/// "pixel" family does not exist during the frame the theme is installed.
/// Until it does, [`pixel`] falls back on the proportional font.
static PIXEL_READY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Called once a pass has begun after [`apply_theme`]: the family is bound.
pub fn mark_pixel_ready() {
    PIXEL_READY.store(true, std::sync::atomic::Ordering::Relaxed);
}

/// Register the pixel font as its own family; the editor keeps its fonts.
fn install_pixel_font(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        PIXEL_FAMILY.to_owned(),
        std::sync::Arc::new(egui::FontData::from_static(include_bytes!("../../assets/fonts/lcddot_tr.ttf"))),
    );
    // Interface text is drawn with Proportional, which has no block symbols:
    // anything only Hack knows would be an empty square without this.
    let hack = fonts.families.get(&egui::FontFamily::Monospace).and_then(|list| list.first().cloned());
    if let (Some(hack), Some(proportional)) = (hack, fonts.families.get_mut(&egui::FontFamily::Proportional)) {
        if !proportional.contains(&hack) {
            proportional.push(hack);
        }
    }
    // Fall back on the default fonts (emoji, symbols) for missing glyphs.
    let mut family = vec![PIXEL_FAMILY.to_owned()];
    if let Some(proportional) = fonts.families.get(&egui::FontFamily::Proportional) {
        family.extend(proportional.iter().cloned());
    }
    fonts
        .families
        .insert(egui::FontFamily::Name(PIXEL_FAMILY.into()), family);
    ctx.set_fonts(fonts);
}

/// The game's LCD dot font (`assets/fonts/lcddot_tr.ttf`) at `size`.
pub fn pixel(size: f32) -> FontId {
    if !PIXEL_READY.load(std::sync::atomic::Ordering::Relaxed) {
        return FontId::proportional(size);
    }
    FontId::new(size, egui::FontFamily::Name(PIXEL_FAMILY.into()))
}

/// Frame used by the menu and status bars.
pub fn bar_frame() -> egui::Frame {
    egui::Frame::NONE
        .fill(BG_DEEP)
        .inner_margin(Margin::symmetric(10, 0))
}

/// Small uppercase section title used inside panels.
pub fn section_label(ui: &mut egui::Ui, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .size(9.5)
            .color(TEXT_DIM)
            .strong(),
    );
}

/// Section header with an emoji icon: `🎨 Couleur`.
pub fn header(ui: &mut egui::Ui, icon: &str, title: &str) {
    ui.add_space(2.0);
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(icon).size(14.0));
        ui.label(egui::RichText::new(title).size(12.5).strong().color(TEXT));
    });
}

/// Rounded card grouping related controls.
pub fn card<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::NONE
        .fill(BG_ELEVATED)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::same(10))
        .outer_margin(Margin::symmetric(0, 3))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

/// Square-ish icon button with a tooltip.
pub fn icon_button(ui: &mut egui::Ui, icon: &str, tooltip: &str) -> egui::Response {
    ui.add(
        egui::Button::new(egui::RichText::new(icon).size(15.0))
            .corner_radius(CornerRadius::same(8))
            .min_size(vec2(30.0, 28.0)),
    )
    .on_hover_text(tooltip)
}

/// Toggle button that stays highlighted while selected.
pub fn toggle_button(ui: &mut egui::Ui, selected: bool, text: &str, tooltip: &str) -> egui::Response {
    let button = egui::Button::new(egui::RichText::new(text).size(12.5).color(if selected {
        Color32::WHITE
    } else {
        TEXT
    }))
    .corner_radius(CornerRadius::same(8))
    .min_size(vec2(30.0, 28.0))
    .fill(if selected { ACCENT_DIM } else { BG_ELEVATED })
    .stroke(Stroke::new(1.0, if selected { ACCENT } else { BORDER }));
    ui.add(button).on_hover_text(tooltip)
}

/// Large call-to-action button (Play, Save...).
pub fn primary_button(ui: &mut egui::Ui, text: &str, color: Color32) -> egui::Response {
    ui.add(
        egui::Button::new(egui::RichText::new(text).size(13.0).strong().color(BG_DEEP))
            .fill(color)
            .corner_radius(CornerRadius::same(14))
            .min_size(vec2(36.0, 28.0)),
    )
}

/// Dimmed helper text.
pub fn hint(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).size(10.0).color(TEXT_DIM));
}

/// Small info icon: the explanation shows when the mouse is over it.
pub fn tip(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new("ℹ").size(12.0).color(ACCENT_DIM)).on_hover_text(text);
}

/// Empty-state message centered in a panel.
pub fn empty_state(ui: &mut egui::Ui, icon: &str, text: &str) {
    ui.vertical_centered(|ui| {
        ui.add_space(18.0);
        ui.label(egui::RichText::new(icon).size(30.0));
        ui.label(egui::RichText::new(text).size(11.5).color(TEXT_DIM));
    });
}

/// Labeled row for property grids: `icon label` on the left.
pub fn prop_label(ui: &mut egui::Ui, icon: &str, label: &str) {
    ui.label(egui::RichText::new(format!("{icon}  {label}")).size(11.5).color(TEXT_DIM));
}

// --- In-game UI (same identity as the studio: dark, modern, rounded) -------

/// Translucent fill of HUD cards.
pub const HUD_FILL: Color32 = Color32::from_rgba_premultiplied(11, 15, 21, 214);
/// Opaque fill of in-game windows (inventory, menus, machine panels).
/// Layer order of the game's windows (inventory, machines, menus...). Below
/// `Foreground`, so the popups they open (combo boxes, menus, color pickers),
/// which egui shows in `Foreground`, always draw on top of them.
pub const WINDOW_ORDER: egui::Order = egui::Order::Middle;
pub const WINDOW_FILL: Color32 = Color32::from_rgba_premultiplied(14, 19, 26, 246);

/// Rounded HUD card painted behind a rect.
pub fn hud_card(painter: &egui::Painter, rect: egui::Rect) {
    painter.rect_filled(rect, CornerRadius::same(12), HUD_FILL);
    painter.rect_stroke(rect, CornerRadius::same(12), Stroke::new(1.0, BORDER), egui::StrokeKind::Inside);
}

/// Small rounded label pill (crosshair hints, item name).
pub fn hud_pill(painter: &egui::Painter, anchor: egui::Pos2, align: egui::Align2, text: &str, size: f32, color: Color32) -> egui::Rect {
    let galley = painter.layout_no_wrap(text.to_string(), FontId::proportional(size), color);
    let rect = align.anchor_size(anchor, galley.size() + vec2(18.0, 8.0));
    painter.rect_filled(rect, CornerRadius::same(255), HUD_FILL);
    painter.rect_stroke(rect, CornerRadius::same(255), Stroke::new(1.0, BORDER), egui::StrokeKind::Inside);
    painter.galley(rect.min + vec2(9.0, 4.0), galley, color);
    rect
}

/// Frame of in-game windows.
pub fn window_frame() -> egui::Frame {
    egui::Frame::NONE
        .fill(WINDOW_FILL)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(CornerRadius::same(16))
        .inner_margin(Margin::same(18))
        .shadow(egui::epaint::Shadow {
            offset: [0, 8],
            blur: 28,
            spread: 0,
            color: Color32::from_black_alpha(120),
        })
}

/// Large rounded menu button. `primary` uses the accent colour.
pub fn game_button(ui: &mut egui::Ui, text: &str, primary: bool, width: f32) -> egui::Response {
    let (fill, color) = if primary { (ACCENT, BG_DEEP) } else { (BG_ELEVATED, TEXT) };
    ui.add(
        egui::Button::new(egui::RichText::new(text).size(16.0).strong().color(color))
            .fill(fill)
            .stroke(Stroke::new(1.0, if primary { ACCENT } else { BORDER }))
            .corner_radius(CornerRadius::same(12))
            .min_size(vec2(width, 44.0)),
    )
}

/// Compact rounded button for windows (same look as `game_button`).
pub fn game_small_button(ui: &mut egui::Ui, text: &str, enabled: bool) -> egui::Response {
    ui.add_enabled(
        enabled,
        egui::Button::new(egui::RichText::new(text).size(13.5).color(if enabled { TEXT } else { TEXT_DIM }))
            .fill(BG_ELEVATED)
            .stroke(Stroke::new(1.0, BORDER))
            .corner_radius(CornerRadius::same(10))
            .min_size(vec2(0.0, 34.0)),
    )
}

/// Tab button used by in-game windows.
pub fn game_tab(ui: &mut egui::Ui, selected: bool, text: &str) -> egui::Response {
    ui.add(
        egui::Button::new(egui::RichText::new(text).size(13.5).color(if selected { Color32::WHITE } else { TEXT_DIM }))
            .fill(if selected { ACCENT_DIM } else { Color32::TRANSPARENT })
            .stroke(Stroke::new(1.0, if selected { ACCENT } else { BORDER }))
            .corner_radius(CornerRadius::same(255))
            .min_size(vec2(0.0, 30.0)),
    )
}

/// Window title row.
pub fn window_title(ui: &mut egui::Ui, title: &str, subtitle: &str) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(title).size(20.0).strong().color(Color32::WHITE));
        if !subtitle.is_empty() {
            ui.label(egui::RichText::new(subtitle).size(12.5).color(ACCENT));
        }
    });
}

/// Vertical scroll area for the game windows that always follows the mouse
/// wheel while the pointer is over it (the windows live on their own layers,
/// where egui's own wheel routing does not reach them).
pub fn wheel_scroll<R>(
    ui: &mut egui::Ui,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    max_height: f32,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    let key = ui.id().with(("wheel_scroll", &id_salt));
    let mut offset: f32 = ui.data(|d| d.get_temp(key)).unwrap_or(0.0);
    let top_left = ui.available_rect_before_wrap().min;
    let area = egui::Rect::from_min_size(top_left, egui::vec2(ui.available_width(), max_height.max(0.0)));
    let hovered = ui.ctx().pointer_hover_pos().is_some_and(|pos| area.contains(pos));
    let mut moved = false;
    if hovered {
        let delta = ui.input(|i| i.smooth_scroll_delta.y);
        if delta.abs() > 0.0 {
            offset = (offset - delta).max(0.0);
            moved = true;
            // Consumed here: the scroll area must not apply it a second time.
            ui.ctx().input_mut(|i| i.smooth_scroll_delta.y = 0.0);
        }
    }
    let mut scroll = egui::ScrollArea::vertical().id_salt(id_salt).max_height(max_height).auto_shrink([false, true]);
    if moved {
        scroll = scroll.vertical_scroll_offset(offset);
    }
    let output = scroll.show(ui, add_contents);
    ui.data_mut(|d| d.insert_temp(key, output.state.offset.y));
    output.inner
}
