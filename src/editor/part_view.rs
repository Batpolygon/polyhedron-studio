//! Paintable view of a synthetic block: the canonical shape drawn with egui,
//! turned by dragging, one part selected by clicking. The real render stays
//! beside it; this view exists so every side of the block can be reached and
//! so the pixel under the mouse maps exactly to a part.

use bevy::math::{Mat3, Vec3};
use egui::{Color32, CornerRadius, Pos2, Sense, Shape, epaint::Mesh, vec2};

use crate::editor::theme;
use crate::voxel::blocks::SyntheticLook;
use crate::voxel::shapes::{canonical_faces, face_part, shape_parts, VoxelShape};

/// Halvings of each triangle: small pieces keep the depth sort exact on the
/// concave shapes (stairs, inner corners).
const SUBDIVISIONS: u32 = 3;

/// Orientation of the view.
#[derive(Clone, Copy, Debug)]
pub struct PartView {
    yaw: f32,
    pitch: f32,
}

impl Default for PartView {
    /// Same three-quarter view as the block icons.
    fn default() -> Self {
        Self { yaw: -std::f32::consts::FRAC_PI_4, pitch: 0.6 }
    }
}

struct Piece {
    points: [Pos2; 3],
    depth: f32,
    part: usize,
    light: f32,
}

fn split(corners: [Vec3; 3], level: u32, out: &mut Vec<[Vec3; 3]>) {
    if level == 0 {
        out.push(corners);
        return;
    }
    let [a, b, c] = corners;
    let (ab, bc, ca) = ((a + b) * 0.5, (b + c) * 0.5, (c + a) * 0.5);
    for piece in [[a, ab, ca], [ab, b, bc], [ca, bc, c], [ab, bc, ca]] {
        split(piece, level - 1, out);
    }
}

/// Visible pieces of the shape, the farthest first.
fn visible_pieces(shape: VoxelShape, view: PartView, center: Pos2, scale: f32) -> Vec<Piece> {
    let rotation = Mat3::from_rotation_x(view.pitch) * Mat3::from_rotation_y(view.yaw);
    let light_direction = Vec3::new(-0.3, 0.6, 0.75).normalize();
    let mut out = Vec::new();
    let mut triangles = Vec::new();
    for (index, face) in canonical_faces(shape).iter().enumerate() {
        let normal = rotation * face.normal;
        if normal.z <= 1e-3 {
            continue;
        }
        let light = 0.55 + 0.45 * normal.dot(light_direction).max(0.0);
        let part = face_part(shape, index);
        triangles.clear();
        for i in 1..face.vertices.len() - 1 {
            split([face.vertices[0], face.vertices[i], face.vertices[i + 1]], SUBDIVISIONS, &mut triangles);
        }
        for corners in &triangles {
            let turned = corners.map(|corner| rotation * (corner - Vec3::splat(0.5)));
            out.push(Piece {
                points: turned.map(|p| center + vec2(p.x, -p.y) * scale),
                depth: (turned[0].z + turned[1].z + turned[2].z) / 3.0,
                part,
                light,
            });
        }
    }
    out.sort_by(|a, b| a.depth.total_cmp(&b.depth));
    out
}

fn contains(points: &[Pos2; 3], at: Pos2) -> bool {
    let side = |a: Pos2, b: Pos2| (b - a).x * (at - a).y - (b - a).y * (at - a).x;
    let (ab, bc, ca) = (side(points[0], points[1]), side(points[1], points[2]), side(points[2], points[0]));
    (ab >= 0.0 && bc >= 0.0 && ca >= 0.0) || (ab <= 0.0 && bc <= 0.0 && ca <= 0.0)
}

/// Part drawn at a screen position.
fn part_at(pieces: &[Piece], at: Pos2) -> Option<usize> {
    pieces.iter().rev().find(|piece| contains(&piece.points, at)).map(|piece| piece.part)
}

fn mix(color: [f32; 3], target: f32, amount: f32) -> [f32; 3] {
    color.map(|channel| channel + (target - channel) * amount)
}

/// Draw the block and handle turning and selecting. `selected` becomes
/// `None` on a click beside the block (the whole block is painted again).
pub fn show(
    ui: &mut egui::Ui,
    side: f32,
    shape: VoxelShape,
    look: &SyntheticLook,
    view: &mut PartView,
    selected: &mut Option<usize>,
) {
    let (rect, response) = ui.allocate_exact_size(vec2(side, side), Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, CornerRadius::same(14), theme::BG_DEEP);
    if response.dragged() {
        let delta = response.drag_delta();
        view.yaw += delta.x * 0.012;
        view.pitch = (view.pitch + delta.y * 0.012).clamp(-1.5, 1.5);
    }

    let pieces = visible_pieces(shape, *view, rect.center(), side * 0.52);
    let hovered = if response.dragged() { None } else { response.hover_pos().and_then(|at| part_at(&pieces, at)) };
    if response.clicked() {
        *selected = response.interact_pointer_pos().and_then(|at| part_at(&pieces, at));
    }

    let pulse = 0.28 + 0.14 * (ui.input(|i| i.time) as f32 * 5.0).sin();
    if selected.is_some() {
        ui.ctx().request_repaint();
    }
    let mut mesh = Mesh::default();
    for piece in &pieces {
        let [r, g, b] = look.parts.get(piece.part).unwrap_or(look.color);
        let mut color = [r as f32, g as f32, b as f32].map(|channel| channel * piece.light);
        if *selected == Some(piece.part) {
            color = mix(color, 255.0, pulse);
        } else if hovered == Some(piece.part) {
            color = mix(color, 255.0, 0.15);
        }
        let color = Color32::from_rgb(color[0] as u8, color[1] as u8, color[2] as u8);
        let first = mesh.vertices.len() as u32;
        for point in piece.points {
            mesh.colored_vertex(point, color);
        }
        mesh.add_triangle(first, first + 1, first + 2);
    }
    painter.add(Shape::mesh(mesh));

    if let Some(part) = hovered.or(*selected) {
        painter.text(
            rect.left_top() + vec2(12.0, 10.0),
            egui::Align2::LEFT_TOP,
            &shape_parts(shape)[part].label,
            egui::FontId::proportional(12.5),
            theme::TEXT_DIM,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::pos2;

    const CENTER: Pos2 = pos2(100.0, 100.0);

    fn label_at(shape: VoxelShape, view: PartView, at: Pos2) -> Option<&'static str> {
        part_at(&visible_pieces(shape, view, CENTER, 100.0), at).map(|part| shape_parts(shape)[part].label.as_str())
    }

    #[test]
    fn clicks_land_on_the_part_facing_the_viewer() {
        let top = PartView { yaw: 0.0, pitch: 1.5 };
        assert_eq!(label_at(VoxelShape::Cube, top, CENTER), Some("Top"));
        let under = PartView { yaw: 0.0, pitch: -1.5 };
        assert_eq!(label_at(VoxelShape::Cube, under, CENTER), Some("Bottom"));
        let back = PartView { yaw: 0.0, pitch: 0.0 };
        assert_eq!(label_at(VoxelShape::Cube, back, CENTER), Some("Back"));
        let front = PartView { yaw: std::f32::consts::PI, pitch: 0.0 };
        assert_eq!(label_at(VoxelShape::Cube, front, CENTER), Some("Front"));
    }

    #[test]
    fn beside_the_block_is_no_part() {
        assert_eq!(label_at(VoxelShape::Cube, PartView::default(), pos2(1.0, 1.0)), None);
    }

    #[test]
    fn the_nearest_face_wins_on_concave_shapes() {
        // Stairs seen from the front: the front wall below, the riser set
        // back behind it above.
        let front = PartView { yaw: std::f32::consts::PI, pitch: 0.0 };
        let shape = VoxelShape::Stairs;
        assert_eq!(label_at(shape, front, pos2(100.0, 125.0)), Some("Front"));
        assert_eq!(label_at(shape, front, pos2(100.0, 75.0)), Some("Front 2"));
    }
}
