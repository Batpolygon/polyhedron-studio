//! UMG-Lite renderer, shared by the in-game HUD and the designer preview.

use egui::{Align2, Color32, FontId, Pos2, Rect, Stroke, pos2, vec2};

use std::collections::{BTreeMap, HashMap};

use crate::ui::{Binding, ButtonAction, ListSource, ProgressStyle, UiLayout, Widget, WidgetKind};

/// Selected tabs and scroll offsets, by widget name.
#[derive(Clone, Debug, Default)]
pub struct WidgetState {
    pub tabs: HashMap<String, usize>,
    pub scroll: HashMap<String, f32>,
}

/// The pointer, for what can be clicked.
#[derive(Clone, Copy, Debug, Default)]
pub struct DrawInput {
    pub pointer: Option<Pos2>,
    pub click: bool,
    pub wheel: f32,
}

/// Values the bindings read. Built once per frame by the caller.
#[derive(Clone, Debug, Default)]
pub struct HudData {
    pub health: f32,
    pub stamina: f32,
    pub max_stamina: f32,
    pub mining: f32,
    pub time_of_day: f32,
    pub kills: u32,
    pub deaths: u32,
    pub combo: usize,
    pub powered: usize,
    pub resources: std::collections::BTreeMap<String, u32>,
    /// Hotbar labels (9) and the selected index.
    pub hotbar: Vec<(String, Color32)>,
    pub selected: usize,
    pub max_health: f32,
    /// Weave variables.
    pub variables: BTreeMap<String, f32>,
    pub lists: BTreeMap<String, Vec<f32>>,
    /// Labels of the Etheraltars.
    pub altars: Vec<String>,
    /// Labels and colours of the resources.
    pub resource_looks: BTreeMap<String, (String, Color32)>,
    pub minimap: Option<egui::TextureId>,
}

impl HudData {
    pub fn read(&self, binding: &Binding) -> f32 {
        match binding {
            Binding::None => 0.0,
            Binding::PlayerHealth => self.health,
            Binding::PlayerStamina => self.stamina,
            Binding::MiningProgress => self.mining,
            Binding::TimeOfDay => self.time_of_day,
            Binding::Resource(name) => self.resources.get(name).copied().unwrap_or(0) as f32,
            Binding::Kills => self.kills as f32,
            Binding::Deaths => self.deaths as f32,
            Binding::ComboStage => self.combo as f32,
            Binding::PoweredMachines => self.powered as f32,
            Binding::Fixed(value) => *value,
            Binding::MaxHealth => self.max_health,
            Binding::Variable(name) => self.variables.get(name).copied().unwrap_or(0.0),
        }
    }

    /// `{value}` is the binding, `{name}` a Weave variable.
    pub fn expand(&self, text: &str, value: f32) -> String {
        let number = |value: f32| if value.fract().abs() < 1e-3 { format!("{}", value as i64) } else { format!("{value:.2}") };
        let mut out = String::new();
        let mut rest = text;
        while let Some(open) = rest.find('{') {
            let Some(close) = rest[open..].find('}') else { break };
            out.push_str(&rest[..open]);
            let name = &rest[open + 1..open + close];
            out.push_str(&number(if name == "value" { value } else { self.variables.get(name).copied().unwrap_or(0.0) }));
            rest = &rest[open + close + 1..];
        }
        out.push_str(rest);
        out
    }

    fn rows(&self, source: &ListSource) -> Vec<String> {
        match source {
            ListSource::Inventory => self
                .resources
                .iter()
                .map(|(key, count)| format!("{}  x{count}", self.resource_looks.get(key).map_or(key.as_str(), |look| look.0.as_str())))
                .collect(),
            ListSource::WeaveList(name) => self.lists.get(name).map(|list| list.iter().map(|value| format!("{value}")).collect()).unwrap_or_default(),
            ListSource::Altars => self.altars.clone(),
        }
    }

    /// Sample values for the designer preview.
    pub fn preview() -> Self {
        let mut resources = std::collections::BTreeMap::new();
        resources.insert("crystalline".to_string(), 3);
        resources.insert("voltic".to_string(), 4);
        Self {
            health: 7.5,
            stamina: 6.0,
            max_stamina: 10.0,
            mining: 0.6,
            time_of_day: 0.4,
            kills: 2,
            deaths: 0,
            combo: 2,
            powered: 3,
            resources,
            hotbar: ["Sword", "Remote", "Blaster", "Stone", "Frame", "Motor", "Controller", "Combustor", "-"]
                .iter()
                .map(|label| (label.to_string(), Color32::from_rgb(120, 130, 145)))
                .collect(),
            selected: 0,
            max_health: 10.0,
            variables: BTreeMap::new(),
            lists: BTreeMap::new(),
            altars: vec!["Etheraltar 12 40 -8".to_string()],
            resource_looks: BTreeMap::new(),
            minimap: None,
        }
    }
}

/// Rectangle of a widget inside its parent rectangle.
pub fn widget_rect(widget: &Widget, parent: Rect, scale: f32) -> Rect {
    let factor = widget.anchor.factor();
    let anchor = pos2(
        parent.min.x + parent.width() * factor[0],
        parent.min.y + parent.height() * factor[1],
    );
    let min = anchor + vec2(widget.offset[0], widget.offset[1]) * scale;
    Rect::from_min_size(min, vec2(widget.size[0], widget.size[1]) * scale)
}

fn rgba(color: [u8; 4]) -> Color32 {
    Color32::from_rgba_unmultiplied(color[0], color[1], color[2], color[3])
}

/// Hit-test result for buttons and the designer.
#[derive(Clone, Debug, Default)]
pub struct DrawOutput {
    /// Clicked actions, with the row of a list.
    pub clicked: Vec<(ButtonAction, usize)>,
    pub tooltip: Option<(String, Pos2)>,
    /// (path, rect) of every drawn widget, deepest last.
    pub rects: Vec<(Vec<usize>, Rect)>,
}

/// Draw a layout into `screen`. `pointer` and `click` enable buttons.
pub fn draw_layout(
    painter: &egui::Painter,
    layout: &UiLayout,
    screen: Rect,
    data: &HudData,
    input: DrawInput,
    state: &mut WidgetState,
) -> DrawOutput {
    let scale = (screen.width() / layout.reference[0].max(1.0))
        .min(screen.height() / layout.reference[1].max(1.0))
        .clamp(0.25, 4.0);
    let mut output = DrawOutput::default();
    // The root canvas always fills the screen.
    let mut path = Vec::new();
    draw_widget(painter, &layout.root, screen, scale, data, input, state, &mut path, &mut output, true);
    if let Some((text, position)) = &output.tooltip {
        let galley = painter.layout_no_wrap(text.clone(), FontId::proportional(12.0), Color32::WHITE);
        let at = *position + vec2(14.0, 14.0);
        painter.rect_filled(Rect::from_min_size(at, galley.size() + vec2(12.0, 8.0)), 3.0, Color32::from_black_alpha(220));
        painter.galley(at + vec2(6.0, 4.0), galley, Color32::WHITE);
    }
    output
}

#[allow(clippy::too_many_arguments)]
fn draw_widget(
    painter: &egui::Painter,
    widget: &Widget,
    parent: Rect,
    scale: f32,
    data: &HudData,
    input: DrawInput,
    state: &mut WidgetState,
    path: &mut Vec<usize>,
    output: &mut DrawOutput,
    is_root: bool,
) {
    if !widget.visible {
        return;
    }
    let rect = if is_root { parent } else { widget_rect(widget, parent, scale) };
    output.rects.push((path.clone(), rect));
    let DrawInput { pointer, click, wheel } = input;
    let hovered = pointer.is_some_and(|p| rect.contains(p));
    if hovered && !widget.tooltip.is_empty() {
        output.tooltip = pointer.map(|p| (widget.tooltip.clone(), p));
    }

    match &widget.kind {
        WidgetKind::CanvasPanel { background } => {
            if background[3] > 0 {
                painter.rect_filled(rect, 4.0, rgba(*background));
            }
        }
        WidgetKind::GridSlotContainer {
            columns,
            slots,
            slot_size,
            spacing,
        } => {
            let columns = (*columns).max(1);
            for index in 0..*slots {
                let col = index % columns;
                let row = index / columns;
                let min = rect.min
                    + vec2(col as f32 * (slot_size + spacing), row as f32 * (slot_size + spacing)) * scale;
                let slot = Rect::from_min_size(min, vec2(*slot_size, *slot_size) * scale);
                draw_slot(painter, slot, index as usize, data, scale);
            }
        }
        WidgetKind::ItemSlot { index } => draw_slot(painter, rect, *index, data, scale),
        WidgetKind::ProgressBar {
            binding,
            max,
            style,
            fill,
            background,
        } => {
            let max = if *binding == Binding::PlayerStamina && data.max_stamina > 0.0 {
                data.max_stamina.max(*max)
            } else {
                max.max(1e-4)
            };
            let ratio = (data.read(binding) / max).clamp(0.0, 1.0);
            match style {
                ProgressStyle::Bar => {
                    painter.rect_filled(rect, 2.0, rgba(*background));
                    let mut filled = rect;
                    filled.set_width(rect.width() * ratio);
                    painter.rect_filled(filled, 2.0, rgba(*fill));
                }
                ProgressStyle::Pips(count) => {
                    let count = (*count).clamp(1, 40) as f32;
                    let gap = 3.0 * scale;
                    let pip_w = (rect.width() - gap * (count - 1.0)) / count;
                    let value = ratio * count;
                    for index in 0..count as usize {
                        let min = rect.min + vec2(index as f32 * (pip_w + gap), 0.0);
                        let pip = Rect::from_min_size(min, vec2(pip_w, rect.height()));
                        painter.rect_filled(pip, 2.0, rgba(*background));
                        let amount = (value - index as f32).clamp(0.0, 1.0);
                        if amount > 0.0 {
                            let mut part = pip;
                            part.set_width(pip.width() * amount);
                            painter.rect_filled(part, 2.0, rgba(*fill));
                        }
                    }
                }
            }
        }
        WidgetKind::Label {
            text,
            binding,
            font_size,
            color,
            centered,
        } => {
            let (at, align) = if *centered { (rect.center(), Align2::CENTER_CENTER) } else { (rect.left_center(), Align2::LEFT_CENTER) };
            painter.text(at, align, data.expand(text, data.read(binding)), FontId::proportional(font_size * scale), rgba(*color));
        }
        WidgetKind::Button { text, action } => {
            draw_button(painter, rect, &data.expand(text, 0.0), action, pointer, click, scale, output);
        }
        WidgetKind::ButtonBar { buttons, vertical, spacing } => {
            let count = buttons.len().max(1) as f32;
            let gap = spacing * scale;
            let side = if *vertical { (rect.height() - gap * (count - 1.0)) / count } else { (rect.width() - gap * (count - 1.0)) / count };
            for (index, (text, action)) in buttons.iter().enumerate() {
                let step = (side + gap) * index as f32;
                let button = if *vertical {
                    Rect::from_min_size(rect.min + vec2(0.0, step), vec2(rect.width(), side))
                } else {
                    Rect::from_min_size(rect.min + vec2(step, 0.0), vec2(side, rect.height()))
                };
                draw_button(painter, button, &data.expand(text, 0.0), action, pointer, click, scale, output);
            }
        }
        WidgetKind::Icon { glyph, color } => {
            painter.text(rect.center(), Align2::CENTER_CENTER, glyph, FontId::proportional(rect.height() * 0.8), rgba(*color));
        }
        WidgetKind::ResourceSlot { key } => {
            painter.rect_filled(rect, 4.0, Color32::from_black_alpha(150));
            let (label, color) = data.resource_looks.get(key).cloned().unwrap_or((key.clone(), Color32::GRAY));
            let swatch = Rect::from_center_size(rect.center() - vec2(0.0, 5.0 * scale), vec2(14.0, 14.0) * scale);
            painter.rect_filled(swatch, 2.0, color);
            painter.text(rect.center_bottom() - vec2(0.0, 3.0 * scale), Align2::CENTER_BOTTOM, label.chars().take(9).collect::<String>(), FontId::proportional(9.0 * scale), Color32::WHITE);
            painter.text(rect.right_top() + vec2(-3.0, 2.0) * scale, Align2::RIGHT_TOP, format!("{}", data.resources.get(key).copied().unwrap_or(0)), FontId::monospace(9.0 * scale), Color32::WHITE);
        }
        WidgetKind::ListView { source, row_height, color, on_row } => {
            let rows = data.rows(source);
            let line = row_height * scale;
            let offset = state.scroll.entry(widget.name.clone()).or_insert(0.0);
            if hovered {
                *offset -= wheel;
            }
            *offset = offset.clamp(0.0, (rows.len() as f32 * line - rect.height()).max(0.0));
            let clip = painter.with_clip_rect(rect.intersect(painter.clip_rect()));
            for (index, row) in rows.iter().enumerate() {
                let top = rect.top() + index as f32 * line - *offset;
                let row_rect = Rect::from_min_size(pos2(rect.left(), top), vec2(rect.width(), line));
                let over = hovered && pointer.is_some_and(|p| row_rect.contains(p));
                if over {
                    clip.rect_filled(row_rect, 3.0, Color32::from_white_alpha(24));
                    if click && *on_row != ButtonAction::None {
                        output.clicked.push((on_row.clone(), index));
                    }
                }
                clip.text(row_rect.left_center() + vec2(6.0, 0.0), Align2::LEFT_CENTER, row, FontId::proportional(13.0 * scale), rgba(*color));
            }
        }
        WidgetKind::Tabs { tab_height } => {
            let bar = tab_height * scale;
            let selected = state.tabs.get(&widget.name).copied().unwrap_or(0).min(widget.children.len().saturating_sub(1));
            let width = rect.width() / widget.children.len().max(1) as f32;
            for (index, child) in widget.children.iter().enumerate() {
                let tab = Rect::from_min_size(rect.min + vec2(width * index as f32, 0.0), vec2(width - 2.0, bar));
                let over = pointer.is_some_and(|p| tab.contains(p));
                painter.rect_filled(tab, 3.0, if index == selected { Color32::from_rgb(60, 120, 112) } else { Color32::from_rgb(34, 48, 64) });
                painter.text(tab.center(), Align2::CENTER_CENTER, &child.name, FontId::proportional(12.0 * scale), Color32::WHITE);
                if over && click {
                    state.tabs.insert(widget.name.clone(), index);
                }
            }
            let body = Rect::from_min_max(rect.min + vec2(0.0, bar), rect.max);
            if let Some(child) = widget.children.get(selected) {
                path.push(selected);
                draw_widget(painter, child, body, scale, data, input, state, path, output, false);
                path.pop();
            }
            return;
        }
        WidgetKind::ScrollPanel { content_height } => {
            let offset = state.scroll.entry(widget.name.clone()).or_insert(0.0);
            if hovered {
                *offset -= wheel;
            }
            *offset = offset.clamp(0.0, (content_height * scale - rect.height()).max(0.0));
            let shifted = rect.translate(vec2(0.0, -*offset));
            let clip = painter.with_clip_rect(rect.intersect(painter.clip_rect()));
            let inner = DrawInput { pointer: pointer.filter(|p| rect.contains(*p)), ..input };
            for (index, child) in widget.children.iter().enumerate() {
                path.push(index);
                draw_widget(&clip, child, shifted, scale, data, inner, state, path, output, false);
                path.pop();
            }
            return;
        }
        WidgetKind::Minimap => {
            painter.rect_filled(rect, 3.0, Color32::from_black_alpha(160));
            if let Some(texture) = data.minimap {
                painter.image(texture, rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
            }
        }
    }

    for (index, child) in widget.children.iter().enumerate() {
        path.push(index);
        draw_widget(painter, child, rect, scale, data, input, state, path, output, false);
        path.pop();
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_button(painter: &egui::Painter, rect: Rect, text: &str, action: &ButtonAction, pointer: Option<Pos2>, click: bool, scale: f32, output: &mut DrawOutput) {
    let hovered = pointer.is_some_and(|p| rect.contains(p));
    painter.rect_filled(rect, 4.0, if hovered { Color32::from_rgb(60, 120, 112) } else { Color32::from_rgb(34, 48, 64) });
    painter.rect_stroke(rect, 4.0, Stroke::new(1.0, Color32::from_rgb(78, 210, 190)), egui::StrokeKind::Inside);
    painter.text(rect.center(), Align2::CENTER_CENTER, text, FontId::proportional(13.0 * scale), Color32::WHITE);
    if hovered && click {
        output.clicked.push((action.clone(), 0));
    }
}

fn draw_slot(painter: &egui::Painter, rect: Rect, index: usize, data: &HudData, scale: f32) {
    let selected = index == data.selected;
    painter.rect_filled(rect, 4.0, Color32::from_black_alpha(150));
    painter.rect_stroke(
        rect,
        4.0,
        Stroke::new(
            if selected { 2.0 } else { 1.0 },
            if selected {
                Color32::from_rgb(255, 210, 120)
            } else {
                Color32::from_rgb(70, 86, 104)
            },
        ),
        egui::StrokeKind::Inside,
    );
    if let Some((label, color)) = data.hotbar.get(index) {
        let swatch = Rect::from_center_size(rect.center() - vec2(0.0, 5.0 * scale), vec2(14.0, 14.0) * scale);
        painter.rect_filled(swatch, 2.0, *color);
        let short: String = label.chars().take(7).collect();
        painter.text(
            rect.center_bottom() - vec2(0.0, 4.0 * scale),
            Align2::CENTER_BOTTOM,
            short,
            FontId::proportional(9.0 * scale),
            Color32::from_rgb(220, 226, 232),
        );
    }
    painter.text(
        rect.left_top() + vec2(3.0, 2.0) * scale,
        Align2::LEFT_TOP,
        format!("{}", index + 1),
        FontId::monospace(8.0 * scale),
        Color32::from_white_alpha(140),
    );
}
