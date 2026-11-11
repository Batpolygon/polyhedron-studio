//! In-game UI drawn over the play view, in the studio style (dark, modern,
//! rounded):
//!
//! - the hotbar (rounded slots, accent ring on the selected one), hearts and
//!   stamina above it in survival, the held item name;
//! - the crosshair with the combo (only when > 0) and the aimed block name
//!   above it, and what can be done with it below (break, place, interact);
//! - the top-right card: a live 3D replica of the player and the minimap with
//!   the coordinates (both can be turned off in the settings);
//! - the chat feed and input (`T`, `/`);
//! - the drag-and-drop inventory (`Tab` / `I`) with tabs (Blocks, Items,
//!   Inventory) and a detail card for the clicked entry.

use crate::core::game_rules::game_rules;
use bevy::prelude::*;
use egui::{Align2, Color32, CornerRadius, FontId, Rect, Shape, Stroke, pos2, vec2};

use crate::core::pie_manager::PieState;
use crate::editor::theme;
use crate::gameplay::chat::{Chat, MESSAGE_LIFETIME};
use crate::gameplay::inventory::{HotbarItem, Inventory};
use crate::gameplay::items::{ItemKind, ItemLibrary};
use crate::gameplay::player_preview::PlayerPreview;
use crate::physics::player_controller::Player;
use crate::voxel::blocks::{BlockFunction, BlockRegistry};
use crate::voxel::world::VoxelWorld;

const MAP_RADIUS: i32 = 28;
const MAP_SIDE: usize = (MAP_RADIUS * 2 + 1) as usize;
/// Columns scanned per frame (keeps the minimap from causing hitches).
const MAP_BUDGET: usize = 260;
const SLOT: f32 = 54.0;
const SLOT_GAP: f32 = 6.0;

/// Top-down colours around the player, scanned incrementally.
#[derive(Resource, Default)]
pub struct Minimap {
    pub position: Vec3,
    pub yaw: f32,
    /// Column colour cache, stamped with the world revision it was read at.
    columns: std::collections::HashMap<IVec2, (Color32, u64)>,
    /// Centre column of the current image.
    center: IVec2,
    texture: Option<egui::TextureHandle>,
    dirty: bool,
    refresh_cursor: usize,
}

pub struct GameUiPlugin;

impl Plugin for GameUiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Minimap>().add_systems(Update, update_minimap);
    }
}

fn column_color(world: &VoxelWorld, registry: &BlockRegistry, column: IVec2, top: i32) -> Color32 {
    for y in (top - 64..=top).rev() {
        let Some(voxel) = world.get(IVec3::new(column.x, y, column.y)) else {
            continue;
        };
        let def = registry.get(voxel.block);
        let c = def.color;
        let shade = (1.0 + (y - top + 20) as f32 * 0.018).clamp(0.55, 1.25);
        let channel = |v: u8| ((v as f32 * shade).min(255.0)) as u8;
        return Color32::from_rgb(channel(c[0]), channel(c[1]), channel(c[2]));
    }
    Color32::from_rgb(10, 13, 18)
}

fn update_minimap(
    pie: Res<PieState>,
    settings: Res<crate::core::settings::GameSettings>,
    world: Res<VoxelWorld>,
    registry: Res<BlockRegistry>,
    players: Query<(&Player, &Transform)>,
    mut map: ResMut<Minimap>,
) {
    if !pie.is_playing() {
        if pie.stopped_this_frame {
            map.columns.clear();
        }
        return;
    }
    let Ok((player, transform)) = players.single() else {
        return;
    };
    map.position = transform.translation;
    map.yaw = player.yaw;
    if !settings.show_minimap {
        return;
    }
    let center = IVec2::new(transform.translation.x.floor() as i32, transform.translation.z.floor() as i32);
    if center != map.center {
        map.center = center;
        map.dirty = true;
    }
    let top = transform.translation.y.floor() as i32 + 24;
    let revision = world.revision;

    // 1. Missing columns, nearest rings first. 2. Stale columns, round-robin.
    let mut budget = MAP_BUDGET;
    'rings: for ring in 0..=MAP_RADIUS {
        for dz in -ring..=ring {
            for dx in -ring..=ring {
                if dx.abs() != ring && dz.abs() != ring {
                    continue;
                }
                let column = center + IVec2::new(dx, dz);
                if map.columns.contains_key(&column) {
                    continue;
                }
                let color = column_color(&world, &registry, column, top);
                map.columns.insert(column, (color, revision));
                map.dirty = true;
                budget -= 1;
                if budget == 0 {
                    break 'rings;
                }
            }
        }
    }
    if budget > 0 {
        let total = MAP_SIDE * MAP_SIDE;
        for _ in 0..budget.min(total) {
            let index = map.refresh_cursor % total;
            map.refresh_cursor = map.refresh_cursor.wrapping_add(1);
            let column = center + IVec2::new((index % MAP_SIDE) as i32 - MAP_RADIUS, (index / MAP_SIDE) as i32 - MAP_RADIUS);
            if map.columns.get(&column).is_some_and(|(_, stamp)| *stamp != revision) {
                let color = column_color(&world, &registry, column, top);
                if map.columns.get(&column).map(|(c, _)| *c) != Some(color) {
                    map.dirty = true;
                }
                map.columns.insert(column, (color, revision));
            }
        }
    }
    // Forget far columns now and then.
    if map.columns.len() > MAP_SIDE * MAP_SIDE * 4 {
        let keep = MAP_RADIUS * 2;
        map.columns.retain(|column, _| (*column - center).abs().max_element() <= keep);
    }
}

/// Upload the minimap image when it changed; returns the texture to draw.
pub fn minimap_texture(ctx: &egui::Context, map: &mut Minimap) -> Option<egui::TextureId> {
    if map.dirty || map.texture.is_none() {
        map.dirty = false;
        let mut pixels = Vec::with_capacity(MAP_SIDE * MAP_SIDE);
        for dz in -MAP_RADIUS..=MAP_RADIUS {
            for dx in -MAP_RADIUS..=MAP_RADIUS {
                let column = map.center + IVec2::new(dx, dz);
                pixels.push(map.columns.get(&column).map(|(c, _)| *c).unwrap_or(Color32::from_rgb(10, 13, 18)));
            }
        }
        let bytes: Vec<u8> = pixels.iter().flat_map(|c| [c.r(), c.g(), c.b(), 255]).collect();
        let image = egui::ColorImage::from_rgba_unmultiplied([MAP_SIDE, MAP_SIDE], &bytes);
        match &mut map.texture {
            Some(texture) => texture.set(image, egui::TextureOptions::NEAREST),
            None => map.texture = Some(ctx.load_texture("polyhedron_minimap", image, egui::TextureOptions::NEAREST)),
        }
    }
    map.texture.as_ref().map(|texture| texture.id())
}

fn rgb(color: [u8; 3], factor: f32) -> Color32 {
    let channel = |v: u8| ((v as f32 * factor).min(255.0)) as u8;
    Color32::from_rgb(channel(color[0]), channel(color[1]), channel(color[2]))
}

/// Stable colour of a raw material name (same idea as the dropped cubes).
fn material_color(name: &str) -> Color32 {
    let hash = name.bytes().fold(2166136261u32, |h, b| (h ^ b as u32).wrapping_mul(16777619));
    egui::epaint::Hsva::new((hash % 360) as f32 / 360.0, 0.6, 0.85, 1.0).into()
}

fn iso_cube(painter: &egui::Painter, rect: Rect, color: [u8; 3]) {
    let s = rect.width().min(rect.height()) * 0.78;
    let c = rect.center() + vec2(0.0, s * 0.05);
    let top = [
        pos2(c.x, c.y - s * 0.5),
        pos2(c.x + s * 0.45, c.y - s * 0.25),
        pos2(c.x, c.y),
        pos2(c.x - s * 0.45, c.y - s * 0.25),
    ];
    let left = [pos2(c.x - s * 0.45, c.y - s * 0.25), c, pos2(c.x, c.y + s * 0.5), pos2(c.x - s * 0.45, c.y + s * 0.25)];
    let right = [c, pos2(c.x + s * 0.45, c.y - s * 0.25), pos2(c.x + s * 0.45, c.y + s * 0.25), pos2(c.x, c.y + s * 0.5)];
    let outline = Stroke::new(1.0, Color32::from_black_alpha(140));
    painter.add(Shape::convex_polygon(top.to_vec(), rgb(color, 1.15), outline));
    painter.add(Shape::convex_polygon(left.to_vec(), rgb(color, 0.85), outline));
    painter.add(Shape::convex_polygon(right.to_vec(), rgb(color, 0.62), outline));
}

/// Procedural icon of a hotbar / inventory entry.
pub fn draw_item_icon(painter: &egui::Painter, rect: Rect, item: &HotbarItem, registry: &BlockRegistry, items: &ItemLibrary) {
    // A real 3D render of the entry once it is baked.
    if let Some(key) = crate::renderer::icons::key_for(item, registry) {
        if crate::renderer::icons::paint(painter, rect.expand(rect.width() * 0.14), &key) {
            return;
        }
    }
    let c = rect.center();
    let s = rect.width().min(rect.height());
    match item {
        HotbarItem::Empty => {}
        HotbarItem::SpawnEgg(name) => {
            // Cubic egg (the baked icon replaces it once ready).
            let color = material_color(name);
            iso_cube(painter, Rect::from_center_size(c, vec2(s * 0.8, s * 0.9)), [color.r(), color.g(), color.b()]);
        }
        HotbarItem::Block(id) => iso_cube(painter, rect, registry.get(*id).color),
        HotbarItem::Resource(id) => draw_material_icon(painter, rect, id),
        HotbarItem::Item(id) => {
            let def = items.find_or_placeholder(id);
            {
                // Side view (X right, Y up) fitted into the slot.
                let (min, max) = def.bounds();
                let extent = (max.x - min.x).max(max.y - min.y).max(0.05);
                let scale = s * 0.78 / extent;
                let origin = c - vec2((min.x + max.x) * 0.5 * scale, -(min.y + max.y) * 0.5 * scale);
                let model = def.model();
                let mut parts: Vec<_> = model.iter().collect();
                parts.sort_by(|a, b| a.bounds().1.z.total_cmp(&b.bounds().1.z));
                for part in parts {
                    let (a, b) = part.bounds();
                    let r = Rect::from_min_max(origin + vec2(a.x * scale, -b.y * scale), origin + vec2(b.x * scale, -a.y * scale));
                    painter.rect_filled(r, 0.0, rgb(part.color, 1.0));
                    painter.rect_stroke(r, 0.0, Stroke::new(1.0, Color32::from_black_alpha(120)), egui::StrokeKind::Inside);
                }
            }
        }
    }
}

/// Icon of a raw resource that is neither a block nor an item.
fn draw_material_icon(painter: &egui::Painter, rect: Rect, name: &str) {
    // A group (`@floral`) shows the resource it is named after.
    let resource = name.trim_start_matches('@');
    if crate::gameplay::crafting::is_resource(resource) && crate::renderer::icons::paint(painter, rect.shrink(4.0), &format!("res:{resource}")) {
        return;
    }
    let c = rect.center();
    let s = rect.width() * 0.3;
    let points = vec![pos2(c.x, c.y - s), pos2(c.x + s, c.y), pos2(c.x, c.y + s), pos2(c.x - s, c.y)];
    painter.add(Shape::convex_polygon(points, material_color(name), Stroke::new(1.0, Color32::from_black_alpha(160))));
}

fn slot_box(painter: &egui::Painter, rect: Rect, selected: bool, hovered: bool) {
    let fill = if selected {
        Color32::from_rgba_premultiplied(24, 46, 48, 230)
    } else if hovered {
        Color32::from_rgba_premultiplied(30, 40, 52, 230)
    } else {
        theme::HUD_FILL
    };
    painter.rect_filled(rect, CornerRadius::same(10), fill);
    let stroke = if selected { Stroke::new(2.0, theme::ACCENT) } else { Stroke::new(1.0, theme::BORDER) };
    painter.rect_stroke(rect, CornerRadius::same(10), stroke, egui::StrokeKind::Inside);
}

/// Hearts or stamina as a row of rounded pips.
fn pip_row(painter: &egui::Painter, rect: Rect, value: f32, max: f32, fill: Color32, empty: Color32) {
    let count = max.round().max(1.0) as usize;
    let gap = 3.0;
    let width = (rect.width() - gap * (count as f32 - 1.0)) / count as f32;
    for index in 0..count {
        let pip = Rect::from_min_size(pos2(rect.left() + index as f32 * (width + gap), rect.top()), vec2(width, rect.height()));
        painter.rect_filled(pip, CornerRadius::same(4), empty);
        let amount = (value - index as f32).clamp(0.0, 1.0);
        if amount > 0.0 {
            let part = Rect::from_min_size(pip.min, vec2(pip.width() * amount, pip.height()));
            painter.rect_filled(part, CornerRadius::same(4), fill);
        }
    }
}

/// Bottom hotbar, vitals (survival) and held item name.
pub fn draw_hotbar(
    painter: &egui::Painter,
    screen: Rect,
    inventory: &Inventory,
    registry: &BlockRegistry,
    items: &ItemLibrary,
    stats: &crate::gameplay::player_stats::PlayerStats,
) {
    let count = 9.0;
    let width = SLOT * count + SLOT_GAP * (count - 1.0);
    let left = screen.center().x - width * 0.5;
    let top = screen.bottom() - SLOT - 18.0;
    let bar = Rect::from_min_size(pos2(left - 8.0, top - 8.0), vec2(width + 16.0, SLOT + 16.0));
    theme::hud_card(painter, bar);

    for index in 0..9 {
        let x = left + index as f32 * (SLOT + SLOT_GAP);
        let rect = Rect::from_min_size(pos2(x, top), vec2(SLOT, SLOT));
        let selected = index == inventory.selected;
        slot_box(painter, rect, selected, false);
        painter.text(rect.left_top() + vec2(6.0, 4.0), Align2::LEFT_TOP, format!("{}", index + 1), theme::pixel(12.0), theme::TEXT_DIM);
        let item = &inventory.hotbar[index];
        draw_item_icon(painter, rect.shrink(9.0), item, registry, items);
        if inventory.survival {
            if let Some(key) = item.resource_key(registry) {
                let count = inventory.count(&key);
                painter.text(
                    rect.right_bottom() + vec2(-6.0, -4.0),
                    Align2::RIGHT_BOTTOM,
                    format!("{count}"),
                    FontId::proportional(13.0),
                    if count == 0 { theme::ERROR } else { Color32::WHITE },
                );
            }
        }
    }

    // Vitals (survival only: creative has no health and no stamina).
    let mut label_y = bar.top() - 10.0;
    if inventory.survival {
        let row = bar.top() - 22.0;
        let half = (bar.width() - 24.0) * 0.5;
        let hearts = Rect::from_min_size(pos2(bar.left() + 4.0, row), vec2(half, 12.0));
        let stamina = Rect::from_min_size(pos2(bar.right() - 4.0 - half, row), vec2(half, 12.0));
        pip_row(painter, hearts, stats.health, stats.max_health, Color32::from_rgb(236, 84, 96), Color32::from_rgba_premultiplied(40, 14, 18, 200));
        pip_row(painter, stamina, stats.stamina, stats.max_stamina, Color32::from_rgb(92, 206, 140), Color32::from_rgba_premultiplied(14, 34, 24, 200));
        label_y = row - 10.0;
    }
    let name = inventory.current().label(registry, items);
    if !name.is_empty() {
        theme::hud_pill(painter, pos2(screen.center().x, label_y), Align2::CENTER_BOTTOM, &name, 14.0, Color32::WHITE);
    }
}

/// What the HUD shows of the game state for the top-right card.
pub struct StatusInfo<'a> {
    pub show_preview: bool,
    pub show_minimap: bool,
    pub biome: Option<&'a str>,
    pub time_of_day: f32,
}

/// Top-right card: 3D player replica and minimap. Returns the card bottom.
pub fn draw_status_panel(
    ctx: &egui::Context,
    painter: &egui::Painter,
    screen: Rect,
    map: &mut Minimap,
    preview: &mut PlayerPreview,
    info: &StatusInfo,
) -> f32 {
    preview.rect = None;
    preview.pixels_per_point = ctx.pixels_per_point();
    if !info.show_preview && !info.show_minimap {
        return screen.top() + 8.0;
    }
    let section = 150.0;
    let sections = info.show_preview as u8 as f32 + info.show_minimap as u8 as f32;
    let width = 14.0 + sections * (section + 14.0);
    let height = 14.0 + 20.0 + section + 30.0;
    let card = Rect::from_min_size(pos2(screen.right() - width - 12.0, screen.top() + 12.0), vec2(width, height));
    theme::hud_card(painter, card);

    let mut x = card.left() + 14.0;
    let title_y = card.top() + 10.0;
    if info.show_preview {
        painter.text(pos2(x, title_y), Align2::LEFT_TOP, "PLAYER", FontId::proportional(11.0), theme::TEXT_DIM);
        let area = Rect::from_min_size(pos2(x, title_y + 20.0), vec2(section, section));
        painter.rect_filled(area, CornerRadius::same(10), Color32::from_rgb(19, 26, 34));
        // The preview camera renders the 3D replica into a texture shown here.
        let inner = area.shrink(3.0);
        preview.rect = Some(inner);
        crate::gameplay::player_preview::paint_preview(painter, inner, preview.texture);
        x += section + 14.0;
    }
    if info.show_minimap {
        painter.text(pos2(x, title_y), Align2::LEFT_TOP, "MAP", FontId::proportional(11.0), theme::TEXT_DIM);
        if let Some(biome) = info.biome {
            painter.text(pos2(x + section, title_y), Align2::RIGHT_TOP, biome, FontId::proportional(11.0), theme::ACCENT);
        }
        let area = Rect::from_min_size(pos2(x, title_y + 20.0), vec2(section, section));
        painter.rect_filled(area, CornerRadius::same(10), Color32::from_rgb(10, 13, 18));
        if let Some(texture) = minimap_texture(ctx, map) {
            // Smooth scrolling: shift the UV by the fraction of the block.
            let frac = vec2(map.position.x - map.position.x.floor(), map.position.z - map.position.z.floor());
            let texel = 1.0 / MAP_SIDE as f32;
            let inner = area.shrink(4.0);
            let shrink = 2.0 * texel;
            let uv = Rect::from_min_max(
                pos2(shrink + (frac.x - 0.5) * texel, shrink + (frac.y - 0.5) * texel),
                pos2(1.0 - shrink + (frac.x - 0.5) * texel, 1.0 - shrink + (frac.y - 0.5) * texel),
            );
            painter.image(texture, inner, uv, Color32::WHITE);
            painter.rect_stroke(area, CornerRadius::same(10), Stroke::new(4.0, Color32::from_rgb(11, 15, 21)), egui::StrokeKind::Inside);
            let center = inner.center();
            let forward = vec2(-map.yaw.sin(), -map.yaw.cos());
            let side = vec2(-forward.y, forward.x);
            let points = vec![center + forward * 8.0, center - forward * 5.0 + side * 5.5, center - forward * 2.0, center - forward * 5.0 - side * 5.5];
            painter.add(Shape::convex_polygon(vec![points[0], points[1], points[2]], Color32::WHITE, Stroke::NONE));
            painter.add(Shape::convex_polygon(vec![points[0], points[2], points[3]], Color32::WHITE, Stroke::new(1.0, Color32::BLACK)));
            painter.text(area.center_top() + vec2(0.0, 6.0), Align2::CENTER_TOP, "N", FontId::proportional(10.0), theme::TEXT_DIM);
        }
    }
    let p = map.position;
    let hours = ((info.time_of_day * 24.0) + 24.0) % 24.0;
    painter.text(
        pos2(card.left() + 14.0, card.bottom() - 10.0),
        Align2::LEFT_BOTTOM,
        format!("X {:.0}   Y {:.0}   Z {:.0}", p.x, p.y, p.z),
        theme::pixel(13.0),
        theme::TEXT,
    );
    painter.text(
        pos2(card.right() - 14.0, card.bottom() - 10.0),
        Align2::RIGHT_BOTTOM,
        format!("{:02}:{:02}", hours as u32, ((hours.fract()) * 60.0) as u32),
        theme::pixel(13.0),
        theme::TEXT_DIM,
    );
    card.bottom()
}

/// What the player aims at, for the crosshair hints.
#[derive(Clone, Debug, Default)]
pub struct CrosshairInfo {
    pub target: Option<String>,
    pub breakable: bool,
    pub unbreakable: bool,
    pub interact: Option<String>,
    pub can_place: bool,
    pub combo: u32,
    pub mining: f32,
    pub attack_key: String,
    pub use_key: String,
    pub interact_key: String,
    /// Hitmarker: remaining fraction (1 = just hit, 0 = gone).
    pub hitmarker: f32,
    /// The hit was a heavy combo finisher.
    pub heavy_hit: bool,
    /// Extra hint shown under the crosshair (e.g. customise key).
    pub extra_hint: Option<(String, String)>,
}

/// Verb shown for `E` on a block function.
pub fn interact_verb(function: BlockFunction, scripted: bool) -> Option<&'static str> {
    match function {
        BlockFunction::None => scripted.then_some("Use"),
        BlockFunction::Movable => Some("Launch"),
        BlockFunction::Motor => Some("Configure"),
        BlockFunction::Wagon => Some("Sit"),
        _ => Some("Open"),
    }
}

/// Crosshair, aimed block and action hints.
pub fn draw_crosshair(painter: &egui::Painter, screen: Rect, info: &CrosshairInfo) {
    let center = screen.center();
    let stroke = Stroke::new(2.0, Color32::from_white_alpha(230));
    let shadow = Stroke::new(4.0, Color32::from_black_alpha(90));
    for s in [shadow, stroke] {
        painter.line_segment([center - vec2(8.0, 0.0), center + vec2(8.0, 0.0)], s);
        painter.line_segment([center - vec2(0.0, 8.0), center + vec2(0.0, 8.0)], s);
    }

    // Hitmarker: four diagonal ticks that pop out and fade.
    if info.hitmarker > 0.0 {
        let fade = info.hitmarker.clamp(0.0, 1.0);
        let grow = 1.0 - fade;
        let (inner, outer, color) = if info.heavy_hit {
            (7.0 + grow * 5.0, 17.0 + grow * 6.0, Color32::from_rgb(255, 196, 80))
        } else {
            (6.0 + grow * 4.0, 13.0 + grow * 4.0, Color32::WHITE)
        };
        let alpha = (fade * 255.0) as u8;
        let tick = Stroke::new(2.5, Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), alpha));
        let back = Stroke::new(4.5, Color32::from_black_alpha((fade * 110.0) as u8));
        for s in [back, tick] {
            for (dx, dy) in [(1.0, 1.0), (-1.0, 1.0), (1.0, -1.0), (-1.0, -1.0)] {
                let direction = vec2(dx, dy).normalized();
                painter.line_segment([center + direction * inner, center + direction * outer], s);
            }
        }
    }

    // Above: target name, combo on top of it.
    let mut y = center.y - 22.0;
    if let Some(name) = &info.target {
        let rect = theme::hud_pill(painter, pos2(center.x, y), Align2::CENTER_BOTTOM, name, 13.0, Color32::WHITE);
        y = rect.top() - 6.0;
    }
    if info.combo > 0 {
        theme::hud_pill(painter, pos2(center.x, y), Align2::CENTER_BOTTOM, &format!("COMBO x{}", info.combo), 14.0, theme::WARN);
    }

    // Below: mining progress, then what can be done.
    let mut y = center.y + 20.0;
    if info.mining > 0.0 {
        let bar = Rect::from_center_size(pos2(center.x, y + 3.0), vec2(70.0, 6.0));
        painter.rect_filled(bar, CornerRadius::same(3), Color32::from_black_alpha(150));
        painter.rect_filled(
            Rect::from_min_size(bar.min, vec2(bar.width() * info.mining.clamp(0.0, 1.0), bar.height())),
            CornerRadius::same(3),
            theme::WARN,
        );
        y += 14.0;
    }
    let mut hints: Vec<(String, String)> = Vec::new();
    if info.target.is_some() {
        if info.breakable {
            hints.push((info.attack_key.clone(), "Break".to_string()));
        }
        if let Some(verb) = &info.interact {
            hints.push((info.interact_key.clone(), verb.clone()));
        }
        if info.can_place {
            hints.push((info.use_key.clone(), "Place".to_string()));
        }
    }
    if info.target.is_some() && info.unbreakable {
        hints.push((String::new(), "Unbreakable".to_string()));
    }
    if let Some(extra) = &info.extra_hint {
        hints.push(extra.clone());
    }
    if hints.is_empty() {
        return;
    }
    // Lay the hints out on one centred row.
    let font = FontId::proportional(12.0);
    let galleys: Vec<_> = hints
        .iter()
        .map(|(key, verb)| {
            let key = (!key.is_empty()).then(|| painter.layout_no_wrap(key.clone(), FontId::proportional(11.0), theme::BG_DEEP));
            let verb = painter.layout_no_wrap(verb.clone(), font.clone(), Color32::WHITE);
            (key, verb)
        })
        .collect();
    let widths: Vec<f32> = galleys
        .iter()
        .map(|(key, verb)| key.as_ref().map(|k| k.size().x + 12.0 + 6.0).unwrap_or(0.0) + verb.size().x + 18.0)
        .collect();
    let total = widths.iter().sum::<f32>() + 6.0 * (widths.len() as f32 - 1.0);
    let mut x = center.x - total * 0.5;
    for ((key, verb), width) in galleys.into_iter().zip(widths) {
        let pill = Rect::from_min_size(pos2(x, y), vec2(width, 24.0));
        painter.rect_filled(pill, CornerRadius::same(255), theme::HUD_FILL);
        painter.rect_stroke(pill, CornerRadius::same(255), Stroke::new(1.0, theme::BORDER), egui::StrokeKind::Inside);
        let mut cursor = pill.left() + 6.0;
        if let Some(key) = key {
            let badge = Rect::from_min_size(pos2(cursor, pill.top() + 4.0), vec2(key.size().x + 12.0, 16.0));
            painter.rect_filled(badge, CornerRadius::same(5), theme::ACCENT);
            painter.galley(pos2(badge.left() + 6.0, badge.center().y - key.size().y * 0.5), key, theme::BG_DEEP);
            cursor = badge.right() + 6.0;
        } else {
            cursor += 3.0;
        }
        painter.galley(pos2(cursor, pill.center().y - verb.size().y * 0.5), verb, Color32::WHITE);
        x += width + 6.0;
    }
}

/// Chat feed (fading when closed) and input line.
pub fn draw_chat(ctx: &egui::Context, chat: &mut Chat, screen: Rect) {
    let width = (screen.width() * 0.42).clamp(320.0, 620.0);
    let bottom = screen.bottom() - 110.0;
    let now = chat.now;
    let visible: Vec<_> = chat
        .lines
        .iter()
        .rev()
        .filter(|line| chat.open || now - line.time < MESSAGE_LIFETIME as f64)
        .take(if chat.open { 14 } else { 8 })
        .cloned()
        .collect();

    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("chat_feed")));
    let mut y = bottom - if chat.open { 44.0 } else { 0.0 };
    for line in &visible {
        let age = (now - line.time) as f32;
        let alpha = if chat.open { 1.0 } else { (1.0 - (age - (MESSAGE_LIFETIME - 1.5)) / 1.5).clamp(0.0, 1.0) };
        let galley = painter.layout(line.text.clone(), FontId::proportional(13.0), line.kind.color().gamma_multiply(alpha), width - 20.0);
        let rect = Rect::from_min_size(pos2(screen.left() + 14.0, y - galley.size().y - 8.0), vec2(width, galley.size().y + 8.0));
        painter.rect_filled(rect, CornerRadius::same(8), Color32::from_black_alpha((130.0 * alpha) as u8));
        painter.galley(rect.min + vec2(10.0, 4.0), galley, Color32::WHITE);
        y = rect.top() - 3.0;
    }

    if !chat.open {
        return;
    }
    let rect = Rect::from_min_size(pos2(screen.left() + 14.0, bottom - 38.0), vec2(width, 36.0));
    let id = egui::Id::new("chat_input_area");
    let mut ui = egui::Ui::new(
        ctx.clone(),
        id,
        egui::UiBuilder::new().layer_id(egui::LayerId::new(egui::Order::Foreground, id)).max_rect(rect),
    );
    let mut submit = false;
    let mut close = false;
    let mut move_cursor_to_end = false;
    egui::Frame::NONE
        .fill(theme::WINDOW_FILL)
        .stroke(Stroke::new(1.0, theme::ACCENT_DIM))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(egui::Margin::symmetric(10, 6))
        .show(&mut ui, |ui| {
            ui.set_width(rect.width() - 22.0);
            // Tab completes before the text field sees the key.
            let tab = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Tab));
            if tab && !chat.just_opened {
                chat.complete();
                move_cursor_to_end = true;
            }
            let edit = egui::TextEdit::singleline(&mut chat.input)
                .id(egui::Id::new("chat_input"))
                .hint_text("Type a message or /help")
                .font(FontId::proportional(14.0))
                .frame(egui::Frame::NONE)
                .desired_width(f32::INFINITY);
            let response = ui.add(edit);
            if chat.just_opened {
                response.request_focus();
            } else if !response.has_focus() && !response.lost_focus() {
                response.request_focus();
            }
            let (enter, escape, up, down) = ui.input(|i| {
                (
                    i.key_pressed(egui::Key::Enter),
                    i.key_pressed(egui::Key::Escape),
                    i.key_pressed(egui::Key::ArrowUp),
                    i.key_pressed(egui::Key::ArrowDown),
                )
            });
            if enter && !chat.just_opened {
                submit = true;
            }
            if escape {
                close = true;
            }
            if up {
                chat.recall(-1);
                move_cursor_to_end = true;
            }
            if down {
                chat.recall(1);
                move_cursor_to_end = true;
            }
        });
    if move_cursor_to_end {
        let input_id = egui::Id::new("chat_input");
        if let Some(mut state) = egui::text_edit::TextEditState::load(ctx, input_id) {
            let end = chat.input.chars().count();
            state.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(end))));
            state.store(ctx, input_id);
        }
    }

    // Completion candidates above the input (Tab cycles them).
    let candidates = crate::gameplay::chat::completions(&chat.input, &chat.sources);
    if !candidates.is_empty() && chat.input.starts_with('/') {
        let current = chat.cycle.as_ref().filter(|cycle| cycle.applied == chat.input).map(|cycle| cycle.candidates[cycle.index].clone());
        let shown: Vec<&String> = candidates.iter().take(8).collect();
        let line = 20.0;
        let height = shown.len() as f32 * line + 10.0;
        let list = Rect::from_min_size(pos2(rect.left(), rect.top() - height - 4.0), vec2(260.0, height));
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("chat_completions")));
        painter.rect_filled(list, CornerRadius::same(8), theme::WINDOW_FILL);
        painter.rect_stroke(list, CornerRadius::same(8), Stroke::new(1.0, theme::BORDER), egui::StrokeKind::Inside);
        for (index, candidate) in shown.iter().enumerate() {
            let row = Rect::from_min_size(pos2(list.left() + 4.0, list.top() + 5.0 + index as f32 * line), vec2(list.width() - 8.0, line));
            let active = current.as_deref() == Some(candidate.as_str());
            if active {
                painter.rect_filled(row, CornerRadius::same(5), theme::ACCENT_DIM);
            }
            painter.text(
                row.left_center() + vec2(8.0, 0.0),
                Align2::LEFT_CENTER,
                candidate.as_str(),
                FontId::proportional(13.0),
                if active { Color32::WHITE } else { theme::TEXT },
            );
        }
        if candidates.len() > shown.len() {
            painter.text(
                list.right_bottom() + vec2(-8.0, -4.0),
                Align2::RIGHT_BOTTOM,
                format!("+{}", candidates.len() - shown.len()),
                FontId::proportional(11.0),
                theme::TEXT_DIM,
            );
        }
        painter.text(list.right_top() + vec2(-8.0, -12.0), Align2::RIGHT_CENTER, "Tab", FontId::proportional(11.0), theme::TEXT_DIM);
    }
    if chat.just_opened {
        // The key that opened the chat must not leak into the field.
        chat.input = chat.prefill.clone();
        chat.just_opened = false;
    }
    if submit {
        let entry = std::mem::take(&mut chat.input);
        chat.submitted.push(entry);
        chat.close();
    } else if close {
        chat.close();
    }
}

/// Drag payload of the inventory screen and the machine panels.
#[derive(Clone, Debug)]
pub enum Drag {
    /// An inventory entry (block, item, resource).
    Entry(HotbarItem),
    /// A hotbar slot.
    Slot(usize),
    /// A stack stored in a machine (container).
    Stored(String),
    /// A storage slot of the survival inventory.
    Grid(usize),
}

/// One draggable resource tile (machine panels): `key` with `count`.
pub fn resource_tile(
    ui: &mut egui::Ui,
    registry: &BlockRegistry,
    items: &ItemLibrary,
    key: &str,
    count: Option<u32>,
    payload: Option<Drag>,
) -> egui::Response {
    let entry = match HotbarItem::from_resource(key, registry, items) {
        Some(item) => Entry::Item(item),
        None => Entry::Resource(key.to_string()),
    };
    let label = crate::gameplay::crafting::display_name(key, registry, items);
    let response = drag_tile(ui, registry, items, &entry, count, false, payload);
    if !response.dragged() {
        response.clone().on_hover_ui(|ui| entry_tooltip(ui, &entry, &label, registry, items));
    }
    response
}

/// The tile following the cursor while a resource is dragged (machine panels).
pub fn paint_drag_ghost(ctx: &egui::Context, pointer: egui::Pos2, key: &str, registry: &BlockRegistry, items: &ItemLibrary) {
    let entry = match HotbarItem::from_resource(key, registry, items) {
        Some(item) => Entry::Item(item),
        None => Entry::Resource(key.to_string()),
    };
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("machine_drag")));
    let rect = Rect::from_center_size(pointer, vec2(SLOT, SLOT));
    paint_tile(&painter, rect, registry, items, &entry, None, true, false);
    ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
}

/// An empty slot drawn at the cursor (machine panels).
pub fn empty_tile(ui: &mut egui::Ui, highlighted: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(SLOT, SLOT), egui::Sense::hover());
    slot_box(ui.painter(), rect, highlighted, response.hovered());
    response
}

/// The 30 inventory slots under a machine panel: tiles drag into the
/// machine; returns what was dropped on the strip (from the machine).
pub fn inventory_strip(ui: &mut egui::Ui, inventory: &Inventory, registry: &BlockRegistry, items: &ItemLibrary, max_height: f32) -> Option<Drag> {
    ui.label(egui::RichText::new("INVENTORY").size(11.0).color(theme::TEXT_DIM));
    // Survival: the real storage slots (same order as the inventory window).
    let tiles: Vec<Option<(String, u32, Drag)>> = if inventory.survival {
        inventory.grid.iter().enumerate().map(|(cell, key)| key.as_ref().map(|key| (key.clone(), inventory.count(key), Drag::Grid(cell)))).collect()
    } else {
        let stacks = inventory.stacks();
        let empty = game_rules().inventory.slots.saturating_sub(stacks.len());
        stacks
            .into_iter()
            .map(|(key, count)| {
                let held = HotbarItem::from_resource(&key, registry, items).unwrap_or(HotbarItem::Resource(key.clone()));
                Some((key, count, Drag::Entry(held)))
            })
            .chain(std::iter::repeat_n(None, empty))
            .collect()
    };
    let (_, dropped) = ui.dnd_drop_zone::<Drag, ()>(egui::Frame::NONE, |ui| {
        theme::wheel_scroll(ui, "machine_inventory_strip", max_height, |ui| {
            ui.spacing_mut().item_spacing = vec2(5.0, 5.0);
            for row in tiles.chunks(10) {
                ui.horizontal(|ui| {
                    for tile in row {
                        match tile {
                            Some((key, count, payload)) => {
                                resource_tile(ui, registry, items, key, Some(*count), Some(payload.clone()));
                            }
                            None => {
                                empty_tile(ui, false);
                            }
                        }
                    }
                });
            }
        });
    });
    dropped.map(|payload| (*payload).clone())
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum InventoryTab {
    #[default]
    Blocks,
    Machines,
    Items,
    Resources,
    Mobs,
    Inventory,
    Craft,
}

impl InventoryTab {
    /// Creative: the whole library, resources included.
    const CREATIVE: [(Self, &'static str); 6] = [
        (Self::Blocks, "Blocks"),
        (Self::Machines, "Machines"),
        (Self::Items, "Items & tools"),
        (Self::Resources, "Resources"),
        (Self::Mobs, "Mobs"),
        (Self::Craft, "Craft"),
    ];
    /// Survival: what the player owns, in slots.
    const SURVIVAL: [(Self, &'static str); 2] = [(Self::Inventory, "Inventory"), (Self::Craft, "Craft")];
}

/// What an inventory tile shows.
#[derive(Clone, Debug, PartialEq)]
enum Entry {
    Item(HotbarItem),
    /// Raw resource (no block, no item).
    Resource(String),
}

/// Per-session inventory screen state (kept in egui memory).
#[derive(Clone, Debug, Default)]
struct InventoryUi {
    tab: InventoryTab,
    search: String,
    inspected: Option<(Entry, String)>,
}

/// Paint one tile (slot box, icon, count) into `rect`.
#[allow(clippy::too_many_arguments)]
fn paint_tile(
    painter: &egui::Painter,
    rect: Rect,
    registry: &BlockRegistry,
    items: &ItemLibrary,
    entry: &Entry,
    count: Option<u32>,
    selected: bool,
    hovered: bool,
) {
    slot_box(painter, rect, selected, hovered);
    match entry {
        Entry::Item(item) => draw_item_icon(painter, rect.shrink(9.0), item, registry, items),
        Entry::Resource(name) => draw_material_icon(painter, rect, name),
    }
    if let Some(count) = count {
        painter.text(
            rect.right_bottom() + vec2(-6.0, -4.0),
            Align2::RIGHT_BOTTOM,
            format!("{count}"),
            FontId::proportional(13.0),
            if count == 0 { theme::ERROR } else { Color32::WHITE },
        );
    }
}

/// A tile that can be hovered (tooltip), clicked and dragged. The drag only
/// starts once the pointer moved, so clicks and hover keep working.
#[allow(clippy::too_many_arguments)]
fn drag_tile(
    ui: &mut egui::Ui,
    registry: &BlockRegistry,
    items: &ItemLibrary,
    entry: &Entry,
    count: Option<u32>,
    selected: bool,
    payload: Option<Drag>,
) -> egui::Response {
    let sense = if payload.is_some() { egui::Sense::click_and_drag() } else { egui::Sense::click() };
    let (rect, response) = ui.allocate_exact_size(vec2(SLOT, SLOT), sense);
    let painter = ui.painter_at(rect.expand(4.0));
    paint_tile(&painter, rect, registry, items, entry, count, selected, response.hovered());
    if let Some(payload) = payload {
        if response.drag_started() {
            egui::DragAndDrop::set_payload(ui.ctx(), payload);
        }
        if response.hovered() && !response.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        }
    }
    response
}

/// Short tooltip of an entry (name, kind, one key fact).
fn entry_tooltip(ui: &mut egui::Ui, entry: &Entry, label: &str, registry: &BlockRegistry, items: &ItemLibrary) {
    ui.label(egui::RichText::new(label).size(14.0).strong().color(Color32::WHITE));
    match entry {
        Entry::Item(HotbarItem::Block(id)) => {
            let def = registry.get(*id);
            let kind = if def.synthetic {
                "Synthetic block (customisable)".to_string()
            } else if def.function != BlockFunction::None {
                format!("Machine - {}", def.function.label())
            } else {
                "Block".to_string()
            };
            ui.label(egui::RichText::new(kind).size(11.5).color(theme::ACCENT));
            if def.function != BlockFunction::None {
                ui.label(egui::RichText::new(function_help(def.function)).size(11.5).color(theme::TEXT_DIM));
            } else if def.hardness < 1000.0 {
                ui.label(egui::RichText::new(format!("{} - {:.1} s by hand", def.material.label(), def.hardness)).size(11.5).color(theme::TEXT_DIM));
            }
        }
        Entry::Item(HotbarItem::Item(id)) => {
            if let Some(def) = items.find(id) {
                ui.label(egui::RichText::new(def.kind.label()).size(11.5).color(theme::ACCENT));
            }
        }
        Entry::Item(HotbarItem::SpawnEgg(name)) => {
            ui.label(egui::RichText::new("Spawn egg").size(11.5).color(theme::ACCENT));
            ui.label(egui::RichText::new(format!("Right-click a block to summon a {name}.")).size(11.5).color(theme::TEXT_DIM));
        }
        Entry::Item(HotbarItem::Resource(id)) => {
            if let Some(info) = crate::gameplay::crafting::resource_info(id) {
                ui.label(egui::RichText::new(info.tier.label()).size(11.5).color(theme::ACCENT));
                ui.label(egui::RichText::new(info.description.as_str()).size(11.5).color(theme::TEXT_DIM));
            }
        }
        Entry::Item(HotbarItem::Empty) => {}
        Entry::Resource(name) => {
            if let Some(info) = crate::gameplay::crafting::resource_info(name) {
                ui.label(egui::RichText::new(info.tier.label()).size(11.5).color(theme::ACCENT));
                ui.label(egui::RichText::new(info.description.as_str()).size(11.5).color(theme::TEXT_DIM));
            }
        }
    }
    ui.label(egui::RichText::new("Click for details, drag to the hotbar").size(10.5).color(theme::TEXT_DIM));
}

fn detail_row(ui: &mut egui::Ui, label: &str, value: String) {
    ui.horizontal(|ui| {
        ui.add_sized(vec2(92.0, 18.0), egui::Label::new(egui::RichText::new(label).size(12.0).color(theme::TEXT_DIM)));
        ui.label(egui::RichText::new(value).size(12.5).color(theme::TEXT));
    });
}

/// Detail card of the clicked entry.
fn detail_card(ui: &mut egui::Ui, entry: &(Entry, String), registry: &BlockRegistry, items: &ItemLibrary, count: Option<u32>) {
    let (entry, name) = entry;
    // Big 3D icon on top.
    let (icon_rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 96.0), egui::Sense::hover());
    let icon_rect = Rect::from_center_size(icon_rect.center(), vec2(96.0, 96.0));
    match entry {
        Entry::Item(item) => draw_item_icon(ui.painter(), icon_rect.shrink(10.0), item, registry, items),
        Entry::Resource(resource) => draw_material_icon(ui.painter(), icon_rect, resource),
    }
    ui.label(egui::RichText::new(name).size(16.0).strong().color(Color32::WHITE));
    ui.add_space(4.0);
    match entry {
        Entry::Item(HotbarItem::Block(id)) => {
            let def = registry.get(*id);
            ui.label(
                egui::RichText::new(if def.synthetic { "Synthetic block" } else if def.function != BlockFunction::None { "Machine" } else { "Block" })
                    .size(12.0)
                    .color(theme::ACCENT),
            );
            detail_row(ui, "Id", def.id.clone());
            detail_row(ui, "Shape", if def.model.is_some() { "Custom model".to_string() } else { def.shape.label().to_string() });
            detail_row(ui, "Material", def.material.label().to_string());
            if def.is_liquid() {
                detail_row(ui, "Liquid", format!("flows {} blocks", def.flow_range));
            } else if def.hardness >= 1000.0 {
                detail_row(ui, "Hardness", "unbreakable".to_string());
            } else {
                detail_row(ui, "Hardness", format!("{:.1} s by hand", def.hardness));
            }
            if def.function != BlockFunction::None {
                detail_row(ui, "Function", def.function.label().to_string());
                ui.label(egui::RichText::new(function_help(def.function)).size(11.5).color(theme::TEXT_DIM));
            }
            if def.synthetic || def.variant_of.is_some() {
                ui.label(egui::RichText::new("Hold it and press the Customize key to choose its colour and Voronoi relief before placing it.").size(11.5).color(theme::TEXT_DIM));
            }
            if def.emissive > 0.0 {
                detail_row(ui, "Light", format!("glows ({:.1})", def.emissive));
            }
            let drop = crate::gameplay::inventory::drop_for(registry, *id)
                .map(|drop| crate::gameplay::crafting::display_name(&drop, registry, items))
                .unwrap_or_else(|| "nothing".to_string());
            detail_row(ui, "Drops", drop);
        }
        Entry::Item(HotbarItem::Item(id)) => {
            if let Some(def) = items.find(id) {
                ui.label(egui::RichText::new(def.kind.label()).size(12.0).color(theme::ACCENT));
                detail_row(ui, "Id", def.id.clone());
                match def.kind {
                    ItemKind::Tool => {
                        detail_row(ui, "Mining", format!("x{:.1} speed", def.mining_speed));
                        detail_row(ui, "Damage", format!("{:.1}", def.damage));
                    }
                    ItemKind::Weapon => {
                        detail_row(ui, "Damage", format!("{:.1}", def.damage));
                        detail_row(ui, "Knockback", format!("{:.1}", def.knockback));
                    }
                    ItemKind::Food => detail_row(ui, "Heals", format!("{:.1} hearts", def.heal)),
                    ItemKind::Cable => detail_row(ui, "Use", "RMB on a port, then on another port".to_string()),
                    ItemKind::Blaster => detail_row(ui, "Bolt damage", format!("{:.1}", def.damage)),
                    ItemKind::Remote => detail_row(ui, "Use", "E on a Controller connects / disconnects it; hold Sprint to drive the connected movable (else the nearest one)".to_string()),
                    ItemKind::Brush => detail_row(ui, "Use", "RMB paints a synthetic block; Customize picks the colour".to_string()),
                    ItemKind::Wrench => detail_row(ui, "Use", "RMB rotates the aimed block".to_string()),
                    ItemKind::Binder => detail_row(ui, "Use", format!("RMB binds an entity for {} Faunal", game_rules().tools.bind_cost)),
                    ItemKind::Atomixer => detail_row(ui, "Mining", format!("x{:.1} on everything, sometimes Atomic", def.mining_speed)),
                    ItemKind::EtheraGun => detail_row(ui, "Use", "Hold RMB to carry an entity (1 Ethereal)".to_string()),
                    ItemKind::BlastyGun => detail_row(ui, "Explosion", format!("{:.0} damage, 1 Conflux", def.damage)),
                    ItemKind::Armor => detail_row(ui, "Armour", format!("-{:.0} damage taken (RMB to wear)", def.armor)),
                    ItemKind::Etherealink => detail_row(ui, "Use", format!("RMB teleports to an Etheraltar ({} Ethereal)", game_rules().tools.link_cost)),
                    ItemKind::Material => {}
                }
                if let Some(ammo) = &def.ammo {
                    detail_row(ui, "Uses", crate::gameplay::crafting::resource_label(ammo));
                }
                detail_row(ui, "Stack", format!("{}", def.stack));
            }
        }
        Entry::Item(HotbarItem::SpawnEgg(mob)) => {
            ui.label(egui::RichText::new("Spawn egg").size(12.0).color(theme::ACCENT));
            ui.label(egui::RichText::new(format!("Right-click a block to summon a {mob} on top of it.")).size(12.0).color(theme::TEXT_DIM));
        }
        Entry::Item(HotbarItem::Resource(id)) => {
            if let Some(info) = crate::gameplay::crafting::resource_info(id) {
                ui.label(egui::RichText::new(info.tier.label()).size(11.5).color(theme::ACCENT));
                ui.label(egui::RichText::new(info.description.as_str()).size(11.5).color(theme::TEXT_DIM));
            }
        }
        Entry::Item(HotbarItem::Empty) => {}
        Entry::Resource(resource) => match crate::gameplay::crafting::resource_info(resource) {
            Some(info) => {
                ui.label(egui::RichText::new(info.tier.label()).size(12.0).color(theme::ACCENT));
                ui.label(egui::RichText::new(info.description.as_str()).size(12.0).color(theme::TEXT_DIM));
            }
            None => {
                ui.label(egui::RichText::new("Resource").size(12.0).color(theme::ACCENT));
                ui.label(egui::RichText::new("Raw material used by machines and recipes.").size(12.0).color(theme::TEXT_DIM));
            }
        },
    }
    // Where it is crafted.
    let key = match entry {
        Entry::Item(item) => item.resource_key(registry),
        Entry::Resource(resource) => Some(resource.clone()),
    };
    if let Some(key) = key {
        let mut stations: Vec<&str> = crate::gameplay::crafting::Station::ALL
            .into_iter()
            .filter(|station| crate::gameplay::crafting::catalog(*station, registry, items).iter().any(|recipe| recipe.output == key))
            .map(|station| station.label())
            .collect();
        stations.dedup();
        if !stations.is_empty() {
            detail_row(ui, "Crafted at", stations.join(", "));
        }
    }
    if let Some(count) = count {
        detail_row(ui, "Owned", format!("{count}"));
    }
}

/// Recipe list of a station. Returns `(recipe, times)` when a craft button
/// was clicked.
pub fn recipe_list(
    ui: &mut egui::Ui,
    station: crate::gameplay::crafting::Station,
    inventory: &Inventory,
    registry: &BlockRegistry,
    items: &ItemLibrary,
    enabled: bool,
    max_height: f32,
) -> Option<(crate::gameplay::crafting::Recipe, u32)> {
    use crate::gameplay::crafting::{available, can_afford, catalog, display_name};
    let mut clicked = None;
    theme::wheel_scroll(ui, ("recipes", station.label()), max_height, |ui| {
        for recipe in catalog(station, registry, items) {
            let affordable = !inventory.survival || can_afford(inventory, &recipe);
            egui::Frame::NONE
                .fill(theme::BG_ELEVATED)
                .stroke(Stroke::new(1.0, if affordable { theme::BORDER } else { Color32::from_rgb(60, 36, 40) }))
                .corner_radius(CornerRadius::same(10))
                .inner_margin(egui::Margin::symmetric(8, 6))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        let (rect, _) = ui.allocate_exact_size(vec2(40.0, 40.0), egui::Sense::hover());
                        match HotbarItem::from_resource(&recipe.output, registry, items) {
                            Some(item) => draw_item_icon(ui.painter(), rect.shrink(3.0), &item, registry, items),
                            None => draw_material_icon(ui.painter(), rect, &recipe.output),
                        }
                        ui.vertical(|ui| {
                            ui.label(
                                egui::RichText::new(format!("{}  x{}", display_name(&recipe.output, registry, items), recipe.count))
                                    .size(13.5)
                                    .color(Color32::WHITE),
                            );
                            let mut job = egui::text::LayoutJob::default();
                            for (position, (key, amount)) in recipe.inputs.iter().enumerate() {
                                if position > 0 {
                                    job.append(" + ", 0.0, egui::TextFormat::simple(FontId::proportional(11.5), theme::TEXT_DIM));
                                }
                                let have = available(inventory, key);
                                let color = if !inventory.survival || have >= *amount { theme::TEXT_DIM } else { theme::ERROR };
                                let text = if inventory.survival {
                                    format!("{amount} {} ({have})", display_name(key, registry, items))
                                } else {
                                    format!("{amount} {}", display_name(key, registry, items))
                                };
                                job.append(&text, 0.0, egui::TextFormat::simple(FontId::proportional(11.5), color));
                            }
                            ui.label(job);
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if theme::game_small_button(ui, "x5", enabled && affordable).clicked() {
                                clicked = Some((recipe.clone(), 5));
                            }
                            if theme::game_small_button(ui, "Craft", enabled && affordable).clicked() {
                                clicked = Some((recipe.clone(), 1));
                            }
                        });
                    });
                });
            ui.add_space(4.0);
        }
    });
    clicked
}

/// Columns of the creative inventory side panel.
const SIDE_COLUMNS: usize = 5;

/// Moves asked by the storage slots this frame.
#[derive(Default)]
struct SlotMoves {
    /// Swap two storage slots.
    grid_swap: Option<(usize, usize)>,
    /// Exchange a storage slot with a hotbar slot (cell, slot).
    grid_hotbar: Option<(usize, usize)>,
    /// Creative: put a library entry in a storage slot.
    grid_set: Option<(usize, Option<String>)>,
}

/// The storage slots of the inventory, each one a drop target: drag a slot
/// onto another to swap them, to the hotbar to equip it; creative also takes
/// library entries.
#[allow(clippy::too_many_arguments)]
fn slot_cells(
    ui: &mut egui::Ui,
    inventory: &Inventory,
    registry: &BlockRegistry,
    items: &ItemLibrary,
    state: &mut InventoryUi,
    columns: usize,
    moves: &mut SlotMoves,
) {
    for row in (0..inventory.grid.len()).collect::<Vec<_>>().chunks(columns) {
        ui.horizontal(|ui| {
            for &cell in row {
                let key = inventory.grid[cell].clone();
                let (_, dropped) = ui.dnd_drop_zone::<Drag, ()>(egui::Frame::NONE, |ui| match &key {
                    Some(key) => {
                        let entry = match HotbarItem::from_resource(key, registry, items) {
                            Some(item) => Entry::Item(item),
                            None => Entry::Resource(key.clone()),
                        };
                        let label = crate::gameplay::crafting::display_name(key, registry, items);
                        let inspected = state.inspected.as_ref().is_some_and(|(e, _)| *e == entry);
                        let count = inventory.survival.then(|| inventory.count(key));
                        let response = drag_tile(ui, registry, items, &entry, count, inspected, Some(Drag::Grid(cell)));
                        if response.double_clicked() {
                            moves.grid_hotbar = Some((cell, inventory.selected));
                        } else if response.clicked() {
                            state.inspected = Some((entry.clone(), label.clone()));
                        }
                        if !response.dragged() {
                            response.on_hover_ui(|ui| entry_tooltip(ui, &entry, &label, registry, items));
                        }
                    }
                    None => {
                        let (rect, _) = ui.allocate_exact_size(vec2(SLOT, SLOT), egui::Sense::hover());
                        slot_box(ui.painter(), rect, false, false);
                    }
                });
                if let Some(payload) = dropped {
                    match payload.as_ref() {
                        Drag::Grid(from) => moves.grid_swap = Some((*from, cell)),
                        Drag::Slot(slot) => moves.grid_hotbar = Some((cell, *slot)),
                        Drag::Entry(item) if !inventory.survival => moves.grid_set = Some((cell, item.resource_key(registry))),
                        _ => {}
                    }
                }
            }
        });
    }
}

/// Full inventory with tabs, tooltips, crafting and drag and drop.
/// Returns `false` when closed.
pub fn draw_inventory(
    ctx: &egui::Context,
    inventory: &mut Inventory,
    registry: &BlockRegistry,
    items: &ItemLibrary,
    mobs: &[String],
    log: &mut crate::core::app_state::EditorUiState,
) -> bool {
    let screen = ctx.viewport_rect();
    let dim = ctx.layer_painter(egui::LayerId::new(egui::Order::Background, egui::Id::new("inventory_dim")));
    dim.rect_filled(screen, 0.0, Color32::from_black_alpha(110));
    if inventory.survival {
        inventory.tidy(registry);
    }

    let state_id = egui::Id::new("inventory_ui_state");
    let mut state: InventoryUi = ctx.data_mut(|d| d.get_temp::<InventoryUi>(state_id)).unwrap_or_default();
    let tabs: &[(InventoryTab, &str)] = if inventory.survival { &InventoryTab::SURVIVAL } else { &InventoryTab::CREATIVE };
    if !tabs.iter().any(|(tab, _)| *tab == state.tab) {
        state.tab = tabs[0].0;
    }

    // Entries of the grid: (entry, count, label).
    let mut entries: Vec<(Entry, Option<u32>, String)> = Vec::new();
    let needle = state.search.to_ascii_lowercase();
    let matches = |label: &str| needle.is_empty() || label.to_ascii_lowercase().contains(&needle);
    match state.tab {
        InventoryTab::Blocks | InventoryTab::Machines => {
            let machines = state.tab == InventoryTab::Machines;
            for (id, def) in registry.blocks() {
                // Variants live in the hotbar only.
                if def.variant_of.is_some() || def.hidden || (def.function != BlockFunction::None) != machines {
                    continue;
                }
                if matches(&def.label) {
                    entries.push((Entry::Item(HotbarItem::Block(id)), None, def.label.clone()));
                }
            }
        }
        InventoryTab::Items => {
            for item in &items.items {
                if matches(&item.label) {
                    entries.push((Entry::Item(HotbarItem::Item(item.id.clone())), None, item.label.clone()));
                }
            }
        }
        InventoryTab::Resources => {
            // Pure resources: every one in creative, the ones owned in survival.
            for info in crate::gameplay::crafting::book().resources.iter() {
                let owned = inventory.count(&info.id);
                if inventory.survival && owned == 0 {
                    continue;
                }
                if matches(&info.label) {
                    let count = inventory.survival.then_some(owned);
                    entries.push((Entry::Resource(info.id.clone()), count, info.label.clone()));
                }
            }
        }
        InventoryTab::Mobs => {
            for mob in mobs {
                let egg = HotbarItem::SpawnEgg(mob.clone());
                let label = egg.label(registry, items);
                if matches(&label) {
                    entries.push((Entry::Item(egg), None, label));
                }
            }
        }
        InventoryTab::Inventory => {
            // The 30 storage slots, in their own order (see `Inventory::grid`).
            for key in inventory.grid.clone().into_iter().flatten() {
                let count = inventory.count(&key);
                match HotbarItem::from_resource(&key, registry, items) {
                    Some(item) => {
                        let label = item.label(registry, items);
                        entries.push((Entry::Item(item), Some(count), label));
                    }
                    None => entries.push((Entry::Resource(key.clone()), Some(count), crate::gameplay::crafting::resource_label(&key))),
                }
            }
        }
        InventoryTab::Craft => {}
    }

    let columns = 9usize;
    let grid_width = columns as f32 * (SLOT + 6.0);
    let detail_width = 240.0;
    // Creative: a side panel with the player's inventory.
    let side_width = if inventory.survival { 0.0 } else { SIDE_COLUMNS as f32 * (SLOT + 6.0) + 12.0 };
    let width = grid_width + detail_width + side_width + 70.0;
    let rect = Rect::from_center_size(screen.center(), vec2(width.min(screen.width() - 20.0), (screen.height() - 60.0).min(660.0)));
    let id = egui::Id::new("inventory_window");
    let mut ui = egui::Ui::new(
        ctx.clone(),
        id,
        egui::UiBuilder::new().layer_id(egui::LayerId::new(theme::WINDOW_ORDER, id)).max_rect(rect),
    );
    let mut open = true;
    let mut assign: Option<(usize, Option<HotbarItem>)> = None;
    let mut swap: Option<(usize, usize)> = None;
    // Storage slot moves (survival slots, creative side panel).
    let mut moves = SlotMoves::default();
    let mut craft: Option<(crate::gameplay::crafting::Recipe, u32)> = None;
    theme::window_frame().show(&mut ui, |ui| {
        ui.set_width(rect.width() - 38.0);
        ui.set_height(rect.height() - 38.0);
        ui.horizontal(|ui| {
            theme::window_title(ui, "Inventory", inventory.mode().label());
            ui.add_space(12.0);
            for (tab, label) in tabs {
                if theme::game_tab(ui, state.tab == *tab, label).clicked() {
                    state.tab = *tab;
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if theme::icon_button(ui, "X", "Close").clicked() {
                    open = false;
                }
                if !matches!(state.tab, InventoryTab::Inventory | InventoryTab::Craft) {
                    ui.add(egui::TextEdit::singleline(&mut state.search).hint_text("Search").desired_width(150.0));
                }
            });
        });
        theme::hint(
            ui,
            if state.tab == InventoryTab::Craft {
                "Craft any block or item from your resources. The Synthesizer turns vegetal + mineral matter into Synthetic; the Assembler and the Infuser make the other resources."
            } else {
                "Hover: info  -  click: details  -  double-click: put in the selected slot  -  drag onto the hotbar  -  drag out of the window to drop it on the ground"
            },
        );
        ui.add_space(6.0);

        let grid_height = ui.available_height() - SLOT - 36.0;
        ui.horizontal_top(|ui| {
            if state.tab == InventoryTab::Craft {
                ui.vertical(|ui| {
                    ui.set_width(grid_width + detail_width + 14.0);
                    craft = recipe_list(ui, crate::gameplay::crafting::Station::Hand, inventory, registry, items, true, grid_height);
                });
                return;
            }
            // Grid (also a drop zone that clears dragged hotbar slots).
            ui.vertical(|ui| {
                ui.set_width(grid_width + 14.0);
                let (_, dropped) = ui.dnd_drop_zone::<Drag, ()>(egui::Frame::NONE, |ui| {
                    theme::wheel_scroll(ui, ("inventory_grid", state.tab as u8), grid_height, |ui| {
                        ui.set_min_width(grid_width);
                        ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                        let slot_view = state.tab == InventoryTab::Inventory;
                        if entries.is_empty() && !slot_view {
                            theme::empty_state(ui, "-", "Nothing here.");
                        }
                        let filled = entries.len();
                        let empty_slots = if slot_view { game_rules().inventory.slots.saturating_sub(filled) } else { 0 };
                        let tiles: Vec<Option<&(Entry, Option<u32>, String)>> =
                            entries.iter().map(Some).chain(std::iter::repeat_n(None, empty_slots)).collect();
                        if slot_view {
                            ui.label(
                                egui::RichText::new(format!(
                                    "{} / {} slots  -  stacks of {}",
                                    filled.min(game_rules().inventory.slots),
                                    game_rules().inventory.slots,
                                    game_rules().inventory.stack_size
                                ))
                                .size(11.0)
                                .color(theme::TEXT_DIM),
                            );
                        }
                        if slot_view {
                            // Survival slots: each one is a drop target, drag to reorganise.
                            slot_cells(ui, inventory, registry, items, &mut state, columns, &mut moves);
                        }
                        for row in tiles.chunks(columns).filter(|_| !slot_view) {
                            ui.horizontal(|ui| {
                                for tile in row {
                                    let Some((entry, count, label)) = tile else {
                                        let (rect, _) = ui.allocate_exact_size(vec2(SLOT, SLOT), egui::Sense::hover());
                                        slot_box(ui.painter(), rect, false, false);
                                        continue;
                                    };
                                    let inspected = state.inspected.as_ref().is_some_and(|(e, l)| e == entry && l == label);
                                    let held = match entry {
                                        Entry::Item(item) => item.clone(),
                                        Entry::Resource(id) => HotbarItem::Resource(id.clone()),
                                    };
                                    let payload = Some(Drag::Entry(held.clone()));
                                    let response = drag_tile(ui, registry, items, entry, *count, inspected, payload);
                                    if response.double_clicked() {
                                        assign = Some((inventory.selected, Some(held)));
                                    } else if response.clicked() {
                                        state.inspected = Some((entry.clone(), label.clone()));
                                    }
                                    if !response.dragged() {
                                        response.on_hover_ui(|ui| entry_tooltip(ui, entry, label, registry, items));
                                    }
                                }
                            });
                        }
                    });
                });
                // A hotbar slot dropped on the grid: survival moves it into a
                // free storage slot; creative keeps it (nothing disappears).
                if let Some(payload) = dropped {
                    if let Drag::Slot(slot) = payload.as_ref() {
                        if inventory.survival && moves.grid_hotbar.is_none() {
                            if let Some(free) = inventory.grid.iter().position(Option::is_none) {
                                moves.grid_hotbar = Some((free, *slot));
                            }
                        }
                    }
                }
            });

            // Detail card of the clicked entry.
            ui.vertical(|ui| {
                ui.set_width(detail_width);
                theme::card(ui, |ui| {
                    ui.set_min_height(grid_height - 20.0);
                    match &state.inspected {
                        Some(entry) => {
                            let key = match &entry.0 {
                                Entry::Item(item) => item.resource_key(registry),
                                Entry::Resource(resource) => Some(resource.clone()),
                            };
                            let count = inventory.survival.then(|| key.map(|key| inventory.count(&key))).flatten();
                            detail_card(ui, entry, registry, items, count);
                            let held = match &entry.0 {
                                Entry::Item(item) => item.clone(),
                                Entry::Resource(id) => HotbarItem::Resource(id.clone()),
                            };
                            ui.add_space(8.0);
                            ui.horizontal(|ui| {
                                if theme::game_small_button(ui, &format!("Put in slot {}", inventory.selected + 1), true).clicked() {
                                    assign = Some((inventory.selected, Some(held.clone())));
                                }
                                if let Some(key) = held.resource_key(registry) {
                                    let owned = inventory.count(&key);
                                    if theme::game_small_button(ui, "Drop 1", !inventory.survival || owned > 0).clicked() {
                                        inventory.throw_requests.push((key.clone(), 1));
                                    }
                                    if inventory.survival && theme::game_small_button(ui, "Drop all", owned > 0).clicked() {
                                        inventory.throw_requests.push((key.clone(), owned));
                                    }
                                }
                            });
                            // Collected blocks become pure resources (grass -> vegetal matter...).
                            if let Some(key) = held.resource_key(registry) {
                                if let Some((target, amount)) = crate::gameplay::crafting::conversion_for(&key) {
                                    let owned = inventory.count(&key);
                                    ui.add_space(4.0);
                                    ui.label(
                                        egui::RichText::new(format!(
                                            "Converts to {} x{amount}",
                                            crate::gameplay::crafting::resource_label(&target)
                                        ))
                                        .size(11.5)
                                        .color(theme::TEXT_DIM),
                                    );
                                    ui.horizontal(|ui| {
                                        for (label, units) in [("Convert 1", 1), ("Convert all", owned)] {
                                            if theme::game_small_button(ui, label, owned > 0).clicked() {
                                                match crate::gameplay::crafting::convert(inventory, &key, units) {
                                                    Ok((target, produced)) => log.action(format!(
                                                        "Converted into {produced} {}.",
                                                        crate::gameplay::crafting::resource_label(&target)
                                                    )),
                                                    Err(error) => log.warn(error),
                                                }
                                            }
                                        }
                                    });
                                }
                            }
                        }
                        None => theme::empty_state(ui, "i", "Click an entry to see its details."),
                    }
                });
            });

            // Creative: the player's own inventory beside the library (drag
            // entries into it, reorganise it, drag it to the hotbar).
            if !inventory.survival {
                ui.vertical(|ui| {
                    ui.set_width(side_width);
                    ui.label(egui::RichText::new("INVENTORY").size(11.0).color(theme::TEXT_DIM));
                    theme::wheel_scroll(ui, "creative_inventory_panel", grid_height - 16.0, |ui| {
                        ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                        slot_cells(ui, inventory, registry, items, &mut state, SIDE_COLUMNS, &mut moves);
                    });
                });
            }
        });

        ui.separator();
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            ui.label(egui::RichText::new("HOTBAR").size(11.0).color(theme::TEXT_DIM));
            for slot in 0..9 {
                let item = inventory.hotbar[slot].clone();
                let count = if inventory.survival { item.resource_key(registry).map(|key| inventory.count(&key)) } else { None };
                let selected = slot == inventory.selected;
                let label = item.label(registry, items);
                let (_, dropped) = ui.dnd_drop_zone::<Drag, ()>(egui::Frame::NONE, |ui| {
                    let entry = Entry::Item(item.clone());
                    let payload = (item != HotbarItem::Empty).then_some(Drag::Slot(slot));
                    let response = drag_tile(ui, registry, items, &entry, count, selected, payload);
                    if response.clicked() {
                        swap = Some((slot, slot));
                    }
                    if item != HotbarItem::Empty && !response.dragged() {
                        response.on_hover_ui(|ui| entry_tooltip(ui, &entry, &label, registry, items));
                    }
                });
                if let Some(payload) = dropped {
                    match payload.as_ref() {
                        Drag::Entry(entry) => assign = Some((slot, Some(entry.clone()))),
                        Drag::Slot(from) => swap = Some((*from, slot)),
                        Drag::Grid(cell) => moves.grid_hotbar = Some((*cell, slot)),
                        Drag::Stored(_) => {}
                    }
                }
            }
        });
    });

    // The dragged entry follows the pointer; released outside the window it
    // is thrown on the ground (a hotbar slot also empties).
    if let (Some(payload), Some(pointer)) = (egui::DragAndDrop::payload::<Drag>(ctx), ctx.pointer_interact_pos()) {
        let item = match payload.as_ref() {
            Drag::Entry(item) => item.clone(),
            Drag::Slot(slot) => inventory.hotbar[*slot].clone(),
            Drag::Stored(key) => HotbarItem::from_resource(key, registry, items).unwrap_or(HotbarItem::Resource(key.clone())),
            Drag::Grid(cell) => match inventory.grid.get(*cell).cloned().flatten() {
                Some(key) => HotbarItem::from_resource(&key, registry, items).unwrap_or(HotbarItem::Resource(key)),
                None => HotbarItem::Empty,
            },
        };
        if ctx.input(|i| i.pointer.any_released()) && !rect.contains(pointer) {
            if let Some(key) = item.resource_key(registry) {
                let owned = inventory.count(&key);
                let count = if inventory.survival { owned } else { 1 };
                if count > 0 {
                    inventory.throw_requests.push((key, count));
                }
            }
            if let Drag::Slot(slot) = payload.as_ref() {
                assign = Some((*slot, None));
            }
            // Creative: a storage slot thrown out of the window empties.
            if let (Drag::Grid(cell), false) = (payload.as_ref(), inventory.survival) {
                moves.grid_set = Some((*cell, None));
            }
        }
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("inventory_drag")));
        let rect = Rect::from_center_size(pointer, vec2(SLOT, SLOT));
        paint_tile(&painter, rect, registry, items, &Entry::Item(item), None, true, false);
        ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
    }

    if let Some((slot, item)) = assign {
        inventory.hotbar[slot] = item.unwrap_or(HotbarItem::Empty);
        inventory.revision = inventory.revision.wrapping_add(1);
    }
    if let Some((a, b)) = moves.grid_swap {
        inventory.swap_grid(a, b);
    }
    if let Some((cell, key)) = moves.grid_set {
        inventory.set_grid(cell, key);
    }
    if let Some((cell, slot)) = moves.grid_hotbar {
        inventory.swap_grid_hotbar(cell, slot, registry, items);
        if inventory.survival {
            inventory.tidy(registry);
        }
    }
    if let Some((from, to)) = swap {
        if from == to {
            inventory.selected = to;
        } else {
            inventory.hotbar.swap(from, to);
        }
        inventory.revision = inventory.revision.wrapping_add(1);
    }
    if let Some((recipe, times)) = craft {
        {
            let mut made = 0;
            let mut name = String::new();
            for _ in 0..times {
                match crate::gameplay::crafting::craft(inventory, &recipe, registry, items) {
                    Ok((label, count)) => {
                        made += count;
                        name = label;
                    }
                    Err(error) => {
                        if made == 0 {
                            log.warn(error);
                        }
                        break;
                    }
                }
            }
            if made > 0 {
                log.action(format!("Crafted {made} {name}."));
            }
        }
    }
    ctx.data_mut(|d| d.insert_temp(state_id, state));
    open
}

/// One-line help of a machine function.
pub fn function_help(function: BlockFunction) -> &'static str {
    match function {
        BlockFunction::None => "",
        BlockFunction::Movable => "Subject to gravity with a fixed rotation. Press E to launch the structure; E on it again settles it back into blocks.",
        BlockFunction::Motor => "Press E to configure. Turns the blocks attached to its hub. On a movable: spins the blocks on its hub (wheels, propellers); with nothing on the hub it pushes the movable.",
        BlockFunction::Controller => "Connect the Remote Control to it (E) to drive its movable from afar.",
        BlockFunction::Wagon => "Seat subject to gravity, faces where it goes. Press E to sit.",
        BlockFunction::Combustor => "Burns 10 Floral or 10 Faunal into 1 Voltic of power for a small field.",
        BlockFunction::Solar => "Solaronel: turns daylight into Voltic (1 every 9 s).",
        BlockFunction::Synthesizer => "Press E: Floral + Mineral or Floral + Crystalline = Synthetic.",
        BlockFunction::Infuser => "Press E to infuse two resources (Celliom, Aetech, Voltiom...). Needs power.",
        BlockFunction::Vacuum => "Astral-Vacum: pulls the chosen resources out of the containers in its radius.",
        BlockFunction::Harvester => "Mnemo-Harvestor: bio-machine gathering raw resource blocks into a container.",
        BlockFunction::Energizer => "Energiser: spends Voltic to power every machine in its field.",
        BlockFunction::Container => "Stores resources. Press E to open it.",
        BlockFunction::Lamp => "Light: emits light when powered.",
        BlockFunction::Assembler => "Press E: Synthetic, Technic, Conflux and Omnicore, automatically.",
        BlockFunction::Omnichorus => "Press E to send your Omnicore into the network.",
        BlockFunction::Repulsor => "Pushes the entities and movables around it. Press E to configure.",
        BlockFunction::Terminal => "Write commands; while powered it runs them one by one in a loop.",
        BlockFunction::Comandor => "Simplified command interface: pick a command and its arguments.",
        BlockFunction::Transmitter => "Logic gate on its Signal inputs, or Decoder of resources into values.",
        BlockFunction::Visualiser => "Shows the output value of the machine plugged into it.",
        BlockFunction::Emitter => "Emits a set value (Boolean, Float, String).",
        BlockFunction::Receiver => "Stores the value of a connected output into a variable.",
        BlockFunction::Screen => "Shows a colour from a signal (Boolean / Float / String).",
        BlockFunction::Atomiser => "Conflux (or Garbage) + 100 Voltic = 1 Atomic.",
        BlockFunction::Ressourcer => "Collects / distributes the resources of the entities walking on it.",
        BlockFunction::Ringer => "Attracts the targeted entities in a radius.",
        BlockFunction::Laser => "Line trace: hit (Boolean), length (Float) or name (String).",
        BlockFunction::Etheraltar => "Teleport and respawn point, paid in Ethereal.",
        BlockFunction::Kineticster => "Turns pressure into Voltic (1 every 12 s).",
        BlockFunction::Reconstructor => "Duplicates a resource fed 10 times, with Atomic and 1000 Voltic per copy.",
        BlockFunction::Excavator => "Generates the resources of its biome (10 Voltic per batch of 4 to 8, every 4 s).",
        BlockFunction::Ethereaper => "Extracts the Ethereal of the entities on it until they die.",
        BlockFunction::Omnireactor => "Produces power from the Omnicore network.",
        BlockFunction::Invocateral => "Summons the entities of a discovered biome for Ethereal.",
        BlockFunction::Televerseur => "Saves constructions / machines to a file and loads them back.",
        BlockFunction::MnemoDigestor => "Bio-machine attacking the entities walking on it.",
        BlockFunction::MnemoCloner => "Bio-machine making a Mnemo double of a killed entity.",
        BlockFunction::AstralModifier => "Modifies the player stats for Astral.",
        BlockFunction::AstralContainer => "Container with room for 1 000 000 resources.",
        BlockFunction::Custom => "Machine assembled from components in the Forge. Press E for its storage.",
    }
}

/// Death screen: the three respawn choices.
pub fn draw_death_screen(ctx: &egui::Context, death: &mut crate::gameplay::death::DeathState, ethereal: u32, survival: bool) {
    use crate::gameplay::death::RespawnChoice;
    let screen = ctx.viewport_rect();
    let id = egui::Id::new("death_screen");
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Middle, egui::Id::new("death_veil")));
    painter.rect_filled(screen, CornerRadius::ZERO, Color32::from_rgba_premultiplied(70, 8, 12, 150));
    let rect = Rect::from_center_size(screen.center(), vec2(440.0_f32.min(screen.width() - 20.0), 420.0_f32.min(screen.height() - 20.0)));
    let mut ui = egui::Ui::new(ctx.clone(), id, egui::UiBuilder::new().layer_id(egui::LayerId::new(theme::WINDOW_ORDER, id)).max_rect(rect));
    theme::window_frame().show(&mut ui, |ui| {
        ui.set_width(rect.width() - 38.0);
        ui.vertical_centered(|ui| {
            ui.label(egui::RichText::new("YOU DIED").size(34.0).strong().color(theme::ERROR));
            ui.add_space(4.0);
            if survival {
                ui.label(egui::RichText::new(format!("Ethereal: {ethereal}")).size(13.0).color(theme::TEXT_DIM));
            }
        });
        ui.add_space(12.0);
        if theme::game_button(ui, "Respawn at the world spawn", true, ui.available_width()).clicked() {
            death.choice = Some(RespawnChoice::WorldSpawn);
        }
        ui.add_space(6.0);
        let label = format!("Respawn where I died ({} Ethereal)", game_rules().vitals.respawn_here_cost);
        if theme::game_button(ui, &label, false, ui.available_width()).clicked() {
            death.choice = Some(RespawnChoice::WhereDied);
        }
        ui.add_space(6.0);
        if death.altars.is_empty() {
            theme::hint(ui, "No Etheraltar placed: place one to respawn on it.");
        }
        for altar in death.altars.clone() {
            let label = format!("Respawn at the Etheraltar {} {} {} ({} Ethereal)", altar.x, altar.y, altar.z, game_rules().vitals.respawn_altar_cost);
            if theme::game_button(ui, &label, false, ui.available_width()).clicked() {
                death.choice = Some(RespawnChoice::Etheraltar(altar));
            }
            ui.add_space(4.0);
        }
        if let Some(error) = &death.error {
            ui.label(egui::RichText::new(error).size(13.0).color(theme::ERROR));
        }
    });
}

/// Brush colour, chosen before painting (Customize key with the Brush).
fn draw_brush_designer(ctx: &egui::Context) -> bool {
    let mut look = crate::gameplay::tools::brush_look();
    let screen = ctx.viewport_rect();
    let rect = Rect::from_center_size(screen.center(), vec2(380.0_f32.min(screen.width() - 20.0), 300.0_f32.min(screen.height() - 20.0)));
    let id = egui::Id::new("brush_designer_window");
    let mut ui = egui::Ui::new(ctx.clone(), id, egui::UiBuilder::new().layer_id(egui::LayerId::new(theme::WINDOW_ORDER, id)).max_rect(rect));
    let mut open = true;
    theme::window_frame().show(&mut ui, |ui| {
        ui.set_width(rect.width() - 38.0);
        ui.horizontal(|ui| {
            theme::window_title(ui, "Brush", "Colour to paint");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if theme::icon_button(ui, "X", "Close (Customize / Esc)").clicked() {
                    open = false;
                }
            });
        });
        theme::hint(ui, "Use (right click) on a synthetic block paints it with this colour.");
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.color_edit_button_srgb(&mut look.color);
            ui.label(egui::RichText::new(format!("#{:02X}{:02X}{:02X}", look.color[0], look.color[1], look.color[2])).size(12.5).color(theme::TEXT_DIM));
        });
        ui.horizontal_wrapped(|ui| {
            for (_, rgb) in crate::core::settings::PART_PALETTE {
                let (swatch, response) = ui.allocate_exact_size(vec2(22.0, 22.0), egui::Sense::click());
                ui.painter().rect_filled(swatch, CornerRadius::same(6), Color32::from_rgb(rgb[0], rgb[1], rgb[2]));
                if look.color == rgb {
                    ui.painter().rect_stroke(swatch, CornerRadius::same(6), Stroke::new(2.0, theme::ACCENT), egui::StrokeKind::Outside);
                }
                if response.clicked() {
                    look.color = rgb;
                }
            }
        });
        ui.checkbox(&mut look.normal.enabled, "Low-poly facets");
    });
    crate::gameplay::tools::set_brush_look(look);
    open
}

/// Look being edited in the synthetic block designer.
#[derive(Clone, Debug)]
struct DesignerState {
    /// Base synthetic block id the designer was opened on.
    base: String,
    look: crate::voxel::blocks::SyntheticLook,
    /// Look shown by the 3D preview (updated when the mouse is released).
    shown: crate::voxel::blocks::SyntheticLook,
    /// Part being painted (`None`: the whole block).
    selected: Option<usize>,
    view: crate::editor::part_view::PartView,
}

/// Synthetic block designer (Customize key): colour and Voronoi relief of
/// the held synthetic block, applied to the selected hotbar slot. Returns
/// `false` when closed.
pub fn draw_synthetic_designer(
    ctx: &egui::Context,
    inventory: &mut Inventory,
    registry: &mut BlockRegistry,
    log: &mut crate::core::app_state::EditorUiState,
) -> bool {
    use crate::voxel::blocks::{SyntheticLook, VoronoiCells};
    let HotbarItem::Block(held) = inventory.current().clone() else {
        // The Brush picks its colour here too.
        return matches!(inventory.current(), HotbarItem::Item(id) if id == "brush") && draw_brush_designer(ctx);
    };
    let held_def = registry.get(held).clone();
    if !held_def.synthetic {
        return false;
    }
    let base_id = held_def.variant_of.clone().unwrap_or_else(|| held_def.id.clone());
    let state_id = egui::Id::new("synthetic_designer");
    let mut state: DesignerState = ctx
        .data_mut(|d| d.get_temp::<DesignerState>(state_id))
        .filter(|state| state.base == base_id)
        .unwrap_or_else(|| {
            let look = SyntheticLook::of(&held_def);
            DesignerState { base: base_id.clone(), look, shown: look, selected: None, view: Default::default() }
        });

    let screen = ctx.viewport_rect();
    let rect = Rect::from_center_size(screen.center(), vec2(620.0_f32.min(screen.width() - 20.0), 520.0_f32.min(screen.height() - 20.0)));
    let id = egui::Id::new("synthetic_designer_window");
    let mut ui = egui::Ui::new(
        ctx.clone(),
        id,
        egui::UiBuilder::new().layer_id(egui::LayerId::new(theme::WINDOW_ORDER, id)).max_rect(rect),
    );
    let mut open = true;
    let mut apply = false;
    let mut reset = false;
    let base = registry.find(&base_id).unwrap_or(held);
    // Custom models have no shape parts to paint.
    let shape = registry.get(base).shape;
    let parts: &[crate::voxel::shapes::ShapePart] =
        if registry.get(base).custom_model().is_some() { &[] } else { crate::voxel::shapes::shape_parts(shape) };
    theme::window_frame().show(&mut ui, |ui| {
        ui.set_width(rect.width() - 38.0);
        ui.horizontal(|ui| {
            theme::window_title(ui, "Synthetic block", &registry.get(base).label);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if theme::icon_button(ui, "X", "Close (Customize / Esc)").clicked() {
                    open = false;
                }
            });
        });
        theme::hint(
            ui,
            if parts.is_empty() {
                "Choose the colour and the low-poly Voronoi relief, then apply it to the selected hotbar slot."
            } else {
                "Click a part of the block to paint it alone, drag to turn the block, then apply it to the selected hotbar slot."
            },
        );
        ui.add_space(8.0);
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(220.0);
                if !parts.is_empty() {
                    crate::editor::part_view::show(ui, 220.0, shape, &state.look, &mut state.view, &mut state.selected);
                    ui.add_space(8.0);
                }
                ui.horizontal(|ui| {
                    // 3D preview of the look.
                    let side = if parts.is_empty() { 220.0 } else { 92.0 };
                    let (preview, _) = ui.allocate_exact_size(vec2(side, side), egui::Sense::hover());
                    ui.painter().rect_filled(preview, CornerRadius::same(14), theme::BG_DEEP);
                    let def = registry.variant_def(base, &state.shown);
                    let key = format!("preview:{}", def.id);
                    match crate::renderer::icons::request_preview(&key, &def) {
                        Some(texture) => {
                            ui.painter().image(texture, preview.shrink(side * 0.04), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                        }
                        None => {
                            iso_cube(ui.painter(), preview.shrink(side * 0.18), state.shown.color);
                            ctx.request_repaint();
                        }
                    }
                    if !parts.is_empty() {
                        ui.vertical(|ui| {
                            ui.label(egui::RichText::new("Final render").size(11.5).color(theme::TEXT_DIM));
                            if theme::game_small_button(ui, "Whole block", state.selected.is_some()).clicked() {
                                state.selected = None;
                            }
                            let painted = state.selected.is_some_and(|part| state.look.parts.get(part).is_some());
                            if theme::game_small_button(ui, "Clear part", painted).clicked() {
                                if let Some(part) = state.selected {
                                    state.look.parts.clear(part);
                                }
                            }
                        });
                    }
                });
            });
            ui.add_space(14.0);
            ui.vertical(|ui| {
                let selected = state.selected.filter(|part| *part < parts.len());
                let look = &mut state.look;
                let heading = match selected {
                    Some(part) => format!("COLOUR - {}", parts[part].label.to_uppercase()),
                    None if parts.is_empty() => "COLOUR".to_string(),
                    None => "COLOUR - WHOLE BLOCK".to_string(),
                };
                theme::section_label(ui, &heading);
                let before = selected.map_or(look.color, |part| look.parts.get(part).unwrap_or(look.color));
                let mut color = before;
                ui.horizontal(|ui| {
                    ui.color_edit_button_srgb(&mut color);
                    ui.label(egui::RichText::new(format!("#{:02X}{:02X}{:02X}", color[0], color[1], color[2])).size(12.5).color(theme::TEXT_DIM));
                });
                ui.horizontal_wrapped(|ui| {
                    for (_, rgb) in crate::core::settings::PART_PALETTE {
                        let (swatch, response) = ui.allocate_exact_size(vec2(20.0, 20.0), egui::Sense::click());
                        ui.painter().rect_filled(swatch, CornerRadius::same(6), Color32::from_rgb(rgb[0], rgb[1], rgb[2]));
                        if color == rgb {
                            ui.painter().rect_stroke(swatch, CornerRadius::same(6), Stroke::new(2.0, theme::ACCENT), egui::StrokeKind::Outside);
                        }
                        if response.clicked() {
                            color = rgb;
                        }
                    }
                });
                if color != before {
                    match selected {
                        Some(part) => look.parts.set(part, color),
                        None => look.color = color,
                    }
                }
                ui.add_space(6.0);
                theme::section_label(ui, "VORONOI");
                egui::Grid::new("designer_grid").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                    ui.label("Cells tint");
                    egui::ComboBox::from_id_salt("designer_cells").selected_text(look.voronoi.label()).show_ui(ui, |ui| {
                        for cells in VoronoiCells::ALL {
                            ui.selectable_value(&mut look.voronoi, cells, cells.label());
                        }
                    });
                    ui.end_row();
                    ui.label("Relief");
                    ui.checkbox(&mut look.normal.enabled, "Low-poly facets");
                    ui.end_row();
                    ui.label("Strength");
                    ui.add_enabled(look.normal.enabled, egui::Slider::new(&mut look.normal.strength, 0.0..=1.5));
                    ui.end_row();
                    ui.label("Facets per block");
                    ui.add_enabled(look.normal.enabled, egui::Slider::new(&mut look.normal.cells, 0.5..=12.0));
                    ui.end_row();
                    ui.label("Bevel");
                    ui.add_enabled(look.normal.enabled, egui::Slider::new(&mut look.normal.bevel, 0.0..=1.0));
                    ui.end_row();
                    ui.label("Pattern");
                    ui.horizontal(|ui| {
                        ui.add_enabled(look.normal.enabled, egui::DragValue::new(&mut look.normal.seed).range(0.0..=999.0).speed(1.0));
                        if theme::game_small_button(ui, "Random", look.normal.enabled).clicked() {
                            let time = ctx.input(|i| i.time);
                            look.normal.seed = ((time * 7919.0) as u64 % 1000) as f32;
                        }
                    });
                    ui.end_row();
                });
                look.normal.seed = look.normal.seed.round();
            });
        });
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if theme::game_button(ui, &format!("Apply to slot {}", inventory.selected + 1), true, 200.0).clicked() {
                apply = true;
            }
            if theme::game_small_button(ui, "Back to default", true).clicked() {
                reset = true;
            }
        });
    });
    // The preview follows the edits once the mouse is released.
    if !ctx.input(|i| i.pointer.any_down()) {
        state.shown = state.look;
    }
    if apply {
        let key = BlockRegistry::variant_key(&base_id, &state.look);
        match registry.ensure_variant(&key) {
            Some(id) => {
                let slot = inventory.selected;
                inventory.hotbar[slot] = HotbarItem::Block(id);
                inventory.revision = inventory.revision.wrapping_add(1);
                log.action(format!("Synthetic block customised (#{:02X}{:02X}{:02X}).", state.look.color[0], state.look.color[1], state.look.color[2]));
                open = false;
            }
            None => log.warn("This block cannot be customised."),
        }
    }
    if reset {
        let slot = inventory.selected;
        inventory.hotbar[slot] = HotbarItem::Block(base);
        inventory.revision = inventory.revision.wrapping_add(1);
        state.look = SyntheticLook::of(registry.get(base));
        state.shown = state.look;
        state.selected = None;
    }
    ctx.data_mut(|d| d.insert_temp(state_id, state));
    open
}
