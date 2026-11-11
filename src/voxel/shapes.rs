//! The nine canonical voxel shapes, their 24 orientations and their geometry.
//!
//! Every shape is authored once in a canonical frame: the unit cell
//! `[0, 1]^3`, "up" along local `+Y` and the tall / back side along local `+Z`.
//! A voxel stores an [`Orientation`] (6 facings x 4 rolls, packed in 5 bits)
//! that rotates the canonical shape about the cell centre.
//!
//! The same canonical description drives three consumers:
//! - the mesher ([`ShapeFace`] polygons, culled against neighbour faces);
//! - the sub-voxel fusion rules ([`shape_contains`], overlap tests);
//! - the face-coverage table used for hidden-face removal ([`covers_face`]).

use std::sync::OnceLock;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// The nine primitive shapes of the engine.
///
/// The legacy names used by the first studio prototype are accepted as serde
/// aliases so old `project/blocks/*.ron` files keep loading.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum VoxelShape {
    Cube,
    /// Horizontal half cube (bottom slab in canonical orientation).
    #[serde(alias = "HalfCylinder")]
    HHalfCube,
    /// Vertical half cube (back half in canonical orientation).
    #[serde(alias = "Pillar")]
    VHalfCube,
    #[serde(alias = "Arch")]
    Stairs,
    #[serde(alias = "Cylinder")]
    StairsInCorner,
    #[serde(alias = "Sphere")]
    StairsOutCorner,
    #[serde(alias = "Wedge")]
    Ramp,
    #[serde(alias = "InnerCorner")]
    RampInCorner,
    #[serde(alias = "OuterCorner")]
    RampOutCorner,
}

impl VoxelShape {
    pub const ALL: [Self; 9] = [
        Self::Cube,
        Self::HHalfCube,
        Self::VHalfCube,
        Self::Stairs,
        Self::StairsInCorner,
        Self::StairsOutCorner,
        Self::Ramp,
        Self::RampInCorner,
        Self::RampOutCorner,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Cube => "Cube",
            Self::HHalfCube => "H Half Cube",
            Self::VHalfCube => "V Half Cube",
            Self::Stairs => "Stairs",
            Self::StairsInCorner => "Stairs In Corner",
            Self::StairsOutCorner => "Stairs Out Corner",
            Self::Ramp => "Ramp",
            Self::RampInCorner => "Ramp In Corner",
            Self::RampOutCorner => "Ramp Out Corner",
        }
    }

    pub const fn short_label(self) -> &'static str {
        match self {
            Self::Cube => "CUBE",
            Self::HHalfCube => "H-HALF",
            Self::VHalfCube => "V-HALF",
            Self::Stairs => "STAIRS",
            Self::StairsInCorner => "ST-IN",
            Self::StairsOutCorner => "ST-OUT",
            Self::Ramp => "RAMP",
            Self::RampInCorner => "RP-IN",
            Self::RampOutCorner => "RP-OUT",
        }
    }

    /// Half shapes can share a cell with a second, complementary half.
    pub const fn is_half(self) -> bool {
        matches!(self, Self::HHalfCube | Self::VHalfCube)
    }

    pub const fn index(self) -> usize {
        match self {
            Self::Cube => 0,
            Self::HHalfCube => 1,
            Self::VHalfCube => 2,
            Self::Stairs => 3,
            Self::StairsInCorner => 4,
            Self::StairsOutCorner => 5,
            Self::Ramp => 6,
            Self::RampInCorner => 7,
            Self::RampOutCorner => 8,
        }
    }
}

/// One of the six axis directions, used for facings and cell faces.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Facing {
    PosY,
    NegY,
    PosX,
    NegX,
    PosZ,
    NegZ,
}

impl Facing {
    pub const ALL: [Self; 6] = [
        Self::PosY,
        Self::NegY,
        Self::PosX,
        Self::NegX,
        Self::PosZ,
        Self::NegZ,
    ];

    pub const fn index(self) -> usize {
        match self {
            Self::PosY => 0,
            Self::NegY => 1,
            Self::PosX => 2,
            Self::NegX => 3,
            Self::PosZ => 4,
            Self::NegZ => 5,
        }
    }

    pub const fn from_index(index: usize) -> Self {
        Self::ALL[index % 6]
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::PosY => "+Y",
            Self::NegY => "-Y",
            Self::PosX => "+X",
            Self::NegX => "-X",
            Self::PosZ => "+Z",
            Self::NegZ => "-Z",
        }
    }

    pub fn vector(self) -> IVec3 {
        match self {
            Self::PosY => IVec3::Y,
            Self::NegY => IVec3::NEG_Y,
            Self::PosX => IVec3::X,
            Self::NegX => IVec3::NEG_X,
            Self::PosZ => IVec3::Z,
            Self::NegZ => IVec3::NEG_Z,
        }
    }

    pub const fn opposite(self) -> Self {
        match self {
            Self::PosY => Self::NegY,
            Self::NegY => Self::PosY,
            Self::PosX => Self::NegX,
            Self::NegX => Self::PosX,
            Self::PosZ => Self::NegZ,
            Self::NegZ => Self::PosZ,
        }
    }

    /// Closest axis direction to an arbitrary vector.
    pub fn from_vector(vector: Vec3) -> Self {
        let mut best = Self::PosY;
        let mut best_dot = f32::MIN;
        for facing in Self::ALL {
            let dot = facing.vector().as_vec3().dot(vector);
            if dot > best_dot {
                best_dot = dot;
                best = facing;
            }
        }
        best
    }

    /// Rotation taking local `+Y` onto this facing.
    fn base_rotation(self) -> Quat {
        use std::f32::consts::{FRAC_PI_2, PI};
        match self {
            Self::PosY => Quat::IDENTITY,
            Self::NegY => Quat::from_rotation_x(PI),
            Self::PosX => Quat::from_rotation_z(-FRAC_PI_2),
            Self::NegX => Quat::from_rotation_z(FRAC_PI_2),
            Self::PosZ => Quat::from_rotation_x(FRAC_PI_2),
            Self::NegZ => Quat::from_rotation_x(-FRAC_PI_2),
        }
    }
}

/// Packed facing (where local `+Y` points) and roll (quarter turns about it).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Orientation(pub u8);

impl Orientation {
    pub const IDENTITY: Self = Self(0);
    pub const COUNT: usize = 24;

    pub const fn new(facing: Facing, roll: u8) -> Self {
        Self((facing.index() as u8) * 4 + (roll % 4))
    }

    pub const fn facing(self) -> Facing {
        Facing::from_index((self.0 / 4) as usize)
    }

    pub const fn roll(self) -> u8 {
        self.0 % 4
    }

    pub const fn index(self) -> usize {
        (self.0 as usize) % Self::COUNT
    }

    pub fn next_roll(self) -> Self {
        Self::new(self.facing(), (self.roll() + 1) % 4)
    }

    pub fn next_facing(self) -> Self {
        Self::new(Facing::from_index(self.facing().index() + 1), self.roll())
    }

    pub fn rotation(self) -> Quat {
        let roll = Quat::from_rotation_y(self.roll() as f32 * std::f32::consts::FRAC_PI_2);
        self.facing().base_rotation() * roll
    }

    /// Transform a canonical cell point (in `[0, 1]^3`) into the oriented cell.
    pub fn apply(self, local: Vec3) -> Vec3 {
        if self.0 == 0 {
            return local;
        }
        let rotated = self.rotation() * (local - Vec3::splat(0.5)) + Vec3::splat(0.5);
        snap(rotated)
    }

    /// Inverse of [`Self::apply`].
    pub fn unapply(self, oriented: Vec3) -> Vec3 {
        if self.0 == 0 {
            return oriented;
        }
        self.rotation().inverse() * (oriented - Vec3::splat(0.5)) + Vec3::splat(0.5)
    }

    pub fn apply_direction(self, direction: Vec3) -> Vec3 {
        self.rotation() * direction
    }

    /// Orientation whose up axis matches `up` and whose back side (`+Z`)
    /// points as close as possible to `back`.
    pub fn facing_toward(up: Facing, back: Vec3) -> Self {
        let mut best = Self::new(up, 0);
        let mut best_dot = f32::MIN;
        for roll in 0..4 {
            let candidate = Self::new(up, roll);
            let dot = candidate.apply_direction(Vec3::Z).dot(back);
            if dot > best_dot + 1e-4 {
                best_dot = dot;
                best = candidate;
            }
        }
        best
    }

    pub fn label(self) -> String {
        format!("{} r{}", self.facing().label(), self.roll() * 90)
    }
}

/// Round to the 1/1024 grid so rotated corners land exactly on cell planes.
fn snap(value: Vec3) -> Vec3 {
    (value * 1024.0).round() / 1024.0
}

/// A convex polygon of a canonical shape, wound counter-clockwise when seen
/// from outside.
#[derive(Clone, Debug)]
pub struct ShapeFace {
    pub vertices: Vec<Vec3>,
    pub normal: Vec3,
}

/// Canonical point-in-shape test, used for coverage and fusion.
pub fn shape_contains(shape: VoxelShape, p: Vec3) -> bool {
    let inside = |v: f32| (0.0..=1.0).contains(&v);
    if !(inside(p.x) && inside(p.y) && inside(p.z)) {
        return false;
    }

    match shape {
        VoxelShape::Cube => true,
        VoxelShape::HHalfCube => p.y <= 0.5,
        VoxelShape::VHalfCube => p.z >= 0.5,
        VoxelShape::Stairs => p.y <= 0.5 || p.z >= 0.5,
        VoxelShape::StairsInCorner => p.y <= 0.5 || p.z >= 0.5 || p.x >= 0.5,
        VoxelShape::StairsOutCorner => p.y <= 0.5 || (p.z >= 0.5 && p.x >= 0.5),
        VoxelShape::Ramp => p.y <= p.z,
        VoxelShape::RampInCorner => p.y <= p.z.max(p.x),
        VoxelShape::RampOutCorner => p.y <= p.z.min(p.x),
    }
}

/// Oriented point-in-shape test.
pub fn oriented_contains(shape: VoxelShape, orientation: Orientation, p: Vec3) -> bool {
    shape_contains(shape, orientation.unapply(p))
}

/// Sample points used for coverage and overlap tests.
const SAMPLES: [f32; 3] = [1.0 / 6.0, 0.5, 5.0 / 6.0];
const FACE_DEPTH: f32 = 0.02;

/// Points just inside the given cell face.
fn face_samples(face: Facing) -> impl Iterator<Item = Vec3> {
    let mut points = Vec::with_capacity(9);
    for a in SAMPLES {
        for b in SAMPLES {
            let point = match face {
                Facing::PosX => Vec3::new(1.0 - FACE_DEPTH, a, b),
                Facing::NegX => Vec3::new(FACE_DEPTH, a, b),
                Facing::PosY => Vec3::new(a, 1.0 - FACE_DEPTH, b),
                Facing::NegY => Vec3::new(a, FACE_DEPTH, b),
                Facing::PosZ => Vec3::new(a, b, 1.0 - FACE_DEPTH),
                Facing::NegZ => Vec3::new(a, b, FACE_DEPTH),
            };
            points.push(point);
        }
    }
    points.into_iter()
}

type CoverageTable = Vec<[bool; 6]>;

fn coverage_table() -> &'static CoverageTable {
    static TABLE: OnceLock<CoverageTable> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut table = Vec::with_capacity(VoxelShape::ALL.len() * Orientation::COUNT);
        for shape in VoxelShape::ALL {
            for orientation in 0..Orientation::COUNT {
                let orientation = Orientation(orientation as u8);
                let mut faces = [false; 6];
                for face in Facing::ALL {
                    faces[face.index()] = face_samples(face)
                        .all(|point| oriented_contains(shape, orientation, point));
                }
                table.push(faces);
            }
        }
        table
    })
}

/// Does the oriented shape fully cover the given cell face?
pub fn covers_face(shape: VoxelShape, orientation: Orientation, face: Facing) -> bool {
    coverage_table()[shape.index() * Orientation::COUNT + orientation.index()][face.index()]
}

/// Union coverage of two shapes sharing a cell (sub-voxel fusion).
pub fn pair_covers_face(
    a: (VoxelShape, Orientation),
    b: (VoxelShape, Orientation),
    face: Facing,
) -> bool {
    if covers_face(a.0, a.1, face) || covers_face(b.0, b.1, face) {
        return true;
    }
    face_samples(face).all(|point| {
        oriented_contains(a.0, a.1, point) || oriented_contains(b.0, b.1, point)
    })
}

/// Interior sample grid for overlap tests (never on the half planes).
const OVERLAP_SAMPLES: [f32; 4] = [0.125, 0.375, 0.625, 0.875];

/// True when two oriented shapes share volume (they cannot be fused).
pub fn shapes_overlap(a: (VoxelShape, Orientation), b: (VoxelShape, Orientation)) -> bool {
    for x in OVERLAP_SAMPLES {
        for y in OVERLAP_SAMPLES {
            for z in OVERLAP_SAMPLES {
                let point = Vec3::new(x, y, z);
                if oriented_contains(a.0, a.1, point) && oriented_contains(b.0, b.1, point) {
                    return true;
                }
            }
        }
    }
    false
}

fn quad(a: Vec3, b: Vec3, c: Vec3, d: Vec3) -> ShapeFace {
    let normal = (b - a).cross(d - a).normalize_or_zero();
    ShapeFace {
        vertices: vec![a, b, c, d],
        normal,
    }
}

fn tri(a: Vec3, b: Vec3, c: Vec3) -> ShapeFace {
    let normal = (b - a).cross(c - a).normalize_or_zero();
    ShapeFace {
        vertices: vec![a, b, c],
        normal,
    }
}

/// Axis-aligned box faces, optionally skipping some (`skip` indexed by
/// [`Facing::index`]).
fn box_faces(min: Vec3, max: Vec3, skip: [bool; 6], out: &mut Vec<ShapeFace>) {
    let (x0, y0, z0) = (min.x, min.y, min.z);
    let (x1, y1, z1) = (max.x, max.y, max.z);
    let v = Vec3::new;

    if !skip[Facing::PosY.index()] {
        out.push(quad(v(x0, y1, z0), v(x0, y1, z1), v(x1, y1, z1), v(x1, y1, z0)));
    }
    if !skip[Facing::NegY.index()] {
        out.push(quad(v(x0, y0, z0), v(x1, y0, z0), v(x1, y0, z1), v(x0, y0, z1)));
    }
    if !skip[Facing::PosX.index()] {
        out.push(quad(v(x1, y0, z0), v(x1, y1, z0), v(x1, y1, z1), v(x1, y0, z1)));
    }
    if !skip[Facing::NegX.index()] {
        out.push(quad(v(x0, y0, z0), v(x0, y0, z1), v(x0, y1, z1), v(x0, y1, z0)));
    }
    if !skip[Facing::PosZ.index()] {
        out.push(quad(v(x0, y0, z1), v(x1, y0, z1), v(x1, y1, z1), v(x0, y1, z1)));
    }
    if !skip[Facing::NegZ.index()] {
        out.push(quad(v(x0, y0, z0), v(x0, y1, z0), v(x1, y1, z0), v(x1, y0, z0)));
    }
}

const NO_SKIP: [bool; 6] = [false; 6];

fn skip(faces: &[Facing]) -> [bool; 6] {
    let mut out = [false; 6];
    for face in faces {
        out[face.index()] = true;
    }
    out
}

fn build_faces(shape: VoxelShape) -> Vec<ShapeFace> {
    let v = Vec3::new;
    let mut out = Vec::new();

    match shape {
        VoxelShape::Cube => box_faces(Vec3::ZERO, Vec3::ONE, NO_SKIP, &mut out),
        VoxelShape::HHalfCube => box_faces(Vec3::ZERO, v(1.0, 0.5, 1.0), NO_SKIP, &mut out),
        VoxelShape::VHalfCube => box_faces(v(0.0, 0.0, 0.5), Vec3::ONE, NO_SKIP, &mut out),
        VoxelShape::Stairs => {
            // Lower slab without its top, then the exposed front tread.
            box_faces(Vec3::ZERO, v(1.0, 0.5, 1.0), skip(&[Facing::PosY]), &mut out);
            out.push(quad(
                v(0.0, 0.5, 0.0),
                v(0.0, 0.5, 0.5),
                v(1.0, 0.5, 0.5),
                v(1.0, 0.5, 0.0),
            ));
            box_faces(v(0.0, 0.5, 0.5), Vec3::ONE, skip(&[Facing::NegY]), &mut out);
        }
        VoxelShape::StairsInCorner => {
            box_faces(Vec3::ZERO, v(1.0, 0.5, 1.0), skip(&[Facing::PosY]), &mut out);
            // The only exposed tread is the front-left quarter.
            out.push(quad(
                v(0.0, 0.5, 0.0),
                v(0.0, 0.5, 0.5),
                v(0.5, 0.5, 0.5),
                v(0.5, 0.5, 0.0),
            ));
            // Upper back half (z >= 0.5); its front face is only exposed on x < 0.5.
            box_faces(
                v(0.0, 0.5, 0.5),
                Vec3::ONE,
                skip(&[Facing::NegY, Facing::NegZ]),
                &mut out,
            );
            out.push(quad(
                v(0.0, 0.5, 0.5),
                v(0.0, 1.0, 0.5),
                v(0.5, 1.0, 0.5),
                v(0.5, 0.5, 0.5),
            ));
            // Upper front-right quarter (x >= 0.5, z < 0.5).
            box_faces(
                v(0.5, 0.5, 0.0),
                v(1.0, 1.0, 0.5),
                skip(&[Facing::NegY, Facing::PosZ]),
                &mut out,
            );
        }
        VoxelShape::StairsOutCorner => {
            box_faces(Vec3::ZERO, v(1.0, 0.5, 1.0), skip(&[Facing::PosY]), &mut out);
            // L-shaped tread around the raised back-right quarter.
            out.push(quad(
                v(0.0, 0.5, 0.0),
                v(0.0, 0.5, 1.0),
                v(0.5, 0.5, 1.0),
                v(0.5, 0.5, 0.0),
            ));
            out.push(quad(
                v(0.5, 0.5, 0.0),
                v(0.5, 0.5, 0.5),
                v(1.0, 0.5, 0.5),
                v(1.0, 0.5, 0.0),
            ));
            box_faces(v(0.5, 0.5, 0.5), Vec3::ONE, skip(&[Facing::NegY]), &mut out);
        }
        VoxelShape::Ramp => {
            // Bottom, back wall, slope and two side triangles.
            out.push(quad(v(0.0, 0.0, 0.0), v(1.0, 0.0, 0.0), v(1.0, 0.0, 1.0), v(0.0, 0.0, 1.0)));
            out.push(quad(v(0.0, 0.0, 1.0), v(1.0, 0.0, 1.0), v(1.0, 1.0, 1.0), v(0.0, 1.0, 1.0)));
            out.push(quad(v(0.0, 0.0, 0.0), v(0.0, 1.0, 1.0), v(1.0, 1.0, 1.0), v(1.0, 0.0, 0.0)));
            out.push(tri(v(0.0, 0.0, 0.0), v(0.0, 0.0, 1.0), v(0.0, 1.0, 1.0)));
            out.push(tri(v(1.0, 0.0, 0.0), v(1.0, 1.0, 1.0), v(1.0, 0.0, 1.0)));
        }
        VoxelShape::RampInCorner => {
            // Solid with apex edges along the back (+Z) and right (+X) walls.
            out.push(quad(v(0.0, 0.0, 0.0), v(1.0, 0.0, 0.0), v(1.0, 0.0, 1.0), v(0.0, 0.0, 1.0)));
            out.push(quad(v(0.0, 0.0, 1.0), v(1.0, 0.0, 1.0), v(1.0, 1.0, 1.0), v(0.0, 1.0, 1.0)));
            out.push(quad(v(1.0, 0.0, 0.0), v(1.0, 1.0, 0.0), v(1.0, 1.0, 1.0), v(1.0, 0.0, 1.0)));
            // Two slopes meeting on the diagonal from (0,0,0) to (1,1,1).
            out.push(tri(v(0.0, 0.0, 0.0), v(0.0, 1.0, 1.0), v(1.0, 1.0, 1.0)));
            out.push(tri(v(0.0, 0.0, 0.0), v(1.0, 1.0, 1.0), v(1.0, 1.0, 0.0)));
            // Side triangles on -X and -Z.
            out.push(tri(v(0.0, 0.0, 0.0), v(0.0, 0.0, 1.0), v(0.0, 1.0, 1.0)));
            out.push(tri(v(0.0, 0.0, 0.0), v(1.0, 1.0, 0.0), v(1.0, 0.0, 0.0)));
        }
        VoxelShape::RampOutCorner => {
            // Pyramid-like corner: apex at (1,1,1).
            out.push(quad(v(0.0, 0.0, 0.0), v(1.0, 0.0, 0.0), v(1.0, 0.0, 1.0), v(0.0, 0.0, 1.0)));
            out.push(tri(v(0.0, 0.0, 1.0), v(1.0, 0.0, 1.0), v(1.0, 1.0, 1.0)));
            out.push(tri(v(1.0, 0.0, 0.0), v(1.0, 1.0, 1.0), v(1.0, 0.0, 1.0)));
            out.push(tri(v(0.0, 0.0, 0.0), v(0.0, 0.0, 1.0), v(1.0, 1.0, 1.0)));
            out.push(tri(v(0.0, 0.0, 0.0), v(1.0, 1.0, 1.0), v(1.0, 0.0, 0.0)));
        }
    }

    // Normalise winding: every face must point away from the shape interior.
    for face in &mut out {
        let centroid = face.vertices.iter().copied().sum::<Vec3>() / face.vertices.len() as f32;
        let probe = centroid + face.normal * 0.01;
        if shape_contains(shape, probe) && !on_cell_boundary(probe) {
            face.vertices.reverse();
            face.normal = -face.normal;
        }
    }

    out
}

fn on_cell_boundary(p: Vec3) -> bool {
    p.x <= 0.0 || p.x >= 1.0 || p.y <= 0.0 || p.y >= 1.0 || p.z <= 0.0 || p.z >= 1.0
}

/// Canonical faces of a shape (cached).
pub fn canonical_faces(shape: VoxelShape) -> &'static [ShapeFace] {
    static FACES: OnceLock<Vec<Vec<ShapeFace>>> = OnceLock::new();
    &FACES.get_or_init(|| VoxelShape::ALL.iter().map(|shape| build_faces(*shape)).collect())
        [shape.index()]
}

/// Most colourable parts a shape can have (see [`shape_parts`]).
pub const MAX_SHAPE_PARTS: usize = 16;

/// A colourable part of a shape: every polygon lying on the same plane.
#[derive(Clone, Debug)]
pub struct ShapePart {
    pub label: String,
    pub normal: Vec3,
    /// Plane offset along the normal.
    plane: f32,
    /// Indices into [`canonical_faces`].
    pub faces: Vec<usize>,
}

fn direction_label(normal: Vec3) -> &'static str {
    const AXES: [(Vec3, &str); 6] = [
        (Vec3::Y, "Top"),
        (Vec3::NEG_Y, "Bottom"),
        (Vec3::X, "Right"),
        (Vec3::NEG_X, "Left"),
        (Vec3::NEG_Z, "Front"),
        (Vec3::Z, "Back"),
    ];
    AXES.iter().find(|(axis, _)| axis.dot(normal) > 0.999).map_or("Slope", |(_, label)| label)
}

fn build_parts(shape: VoxelShape) -> Vec<ShapePart> {
    let mut parts: Vec<ShapePart> = Vec::new();
    for (index, face) in canonical_faces(shape).iter().enumerate() {
        let plane = face.normal.dot(face.vertices[0]);
        let same_plane = |part: &&mut ShapePart| part.normal.dot(face.normal) > 0.999 && (part.plane - plane).abs() < 1e-3;
        if let Some(part) = parts.iter_mut().find(same_plane) {
            part.faces.push(index);
            continue;
        }
        let name = direction_label(face.normal);
        let rank = 1 + parts.iter().filter(|part| direction_label(part.normal) == name).count();
        let label = if rank == 1 { name.to_string() } else { format!("{name} {rank}") };
        parts.push(ShapePart { label, normal: face.normal, plane, faces: vec![index] });
    }
    parts
}

/// Colourable parts of a shape (cached).
pub fn shape_parts(shape: VoxelShape) -> &'static [ShapePart] {
    static PARTS: OnceLock<Vec<Vec<ShapePart>>> = OnceLock::new();
    &PARTS.get_or_init(|| VoxelShape::ALL.iter().map(|shape| build_parts(*shape)).collect())
        [shape.index()]
}

/// Part holding a canonical face.
pub fn face_part(shape: VoxelShape, face: usize) -> usize {
    shape_parts(shape).iter().position(|part| part.faces.contains(&face)).unwrap_or(0)
}

/// Part of a cube turned towards a canonical direction.
pub fn cube_part_facing(normal: Vec3) -> usize {
    shape_parts(VoxelShape::Cube)
        .iter()
        .position(|part| part.normal.dot(normal) > 0.9)
        .unwrap_or(0)
}

/// Which cell face (if any) a transformed polygon lies on.
pub fn boundary_face(vertices: &[Vec3]) -> Option<Facing> {
    const EPS: f32 = 1e-3;
    let all = |predicate: &dyn Fn(Vec3) -> bool| vertices.iter().all(|v| predicate(*v));

    if all(&|v| v.x >= 1.0 - EPS) {
        Some(Facing::PosX)
    } else if all(&|v| v.x <= EPS) {
        Some(Facing::NegX)
    } else if all(&|v| v.y >= 1.0 - EPS) {
        Some(Facing::PosY)
    } else if all(&|v| v.y <= EPS) {
        Some(Facing::NegY)
    } else if all(&|v| v.z >= 1.0 - EPS) {
        Some(Facing::PosZ)
    } else if all(&|v| v.z <= EPS) {
        Some(Facing::NegZ)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orientation_packs_facing_and_roll() {
        for facing in Facing::ALL {
            for roll in 0..4 {
                let orientation = Orientation::new(facing, roll);
                assert_eq!(orientation.facing(), facing);
                assert_eq!(orientation.roll(), roll);
            }
        }
    }

    #[test]
    fn facings_rotate_up_onto_their_axis() {
        for facing in Facing::ALL {
            let up = Orientation::new(facing, 0).apply_direction(Vec3::Y);
            assert!((up - facing.vector().as_vec3()).length() < 1e-4, "{facing:?}");
        }
    }

    #[test]
    fn cube_covers_every_face_in_every_orientation() {
        for orientation in 0..Orientation::COUNT {
            for face in Facing::ALL {
                assert!(covers_face(VoxelShape::Cube, Orientation(orientation as u8), face));
            }
        }
    }

    #[test]
    fn slab_covers_only_its_bottom() {
        let o = Orientation::IDENTITY;
        assert!(covers_face(VoxelShape::HHalfCube, o, Facing::NegY));
        assert!(!covers_face(VoxelShape::HHalfCube, o, Facing::PosY));
        assert!(!covers_face(VoxelShape::HHalfCube, o, Facing::PosX));
    }

    #[test]
    fn ramp_covers_bottom_and_back() {
        let o = Orientation::IDENTITY;
        assert!(covers_face(VoxelShape::Ramp, o, Facing::NegY));
        assert!(covers_face(VoxelShape::Ramp, o, Facing::PosZ));
        assert!(!covers_face(VoxelShape::Ramp, o, Facing::NegZ));
    }

    #[test]
    fn complementary_slabs_fuse_into_a_full_cell() {
        let bottom = (VoxelShape::HHalfCube, Orientation::IDENTITY);
        let top = (VoxelShape::HHalfCube, Orientation::new(Facing::NegY, 0));
        assert!(!shapes_overlap(bottom, top));
        for face in Facing::ALL {
            assert!(pair_covers_face(bottom, top, face));
        }
        assert!(shapes_overlap(bottom, bottom));
    }

    #[test]
    fn coplanar_polygons_form_one_part() {
        assert_eq!(shape_parts(VoxelShape::Cube).len(), 6);
        for shape in VoxelShape::ALL {
            let parts = shape_parts(shape);
            assert!(parts.len() <= MAX_SHAPE_PARTS, "{shape:?} has {} parts", parts.len());
            let faces: usize = parts.iter().map(|part| part.faces.len()).sum();
            assert_eq!(faces, canonical_faces(shape).len());
        }
        // The stairs side is one L: its two polygons share a part.
        let left = shape_parts(VoxelShape::Stairs).iter().find(|part| part.label == "Left").expect("left");
        assert_eq!(left.faces.len(), 2);
    }

    #[test]
    fn every_shape_has_outward_faces() {
        for shape in VoxelShape::ALL {
            let faces = canonical_faces(shape);
            assert!(!faces.is_empty());
            for face in faces {
                let centroid =
                    face.vertices.iter().copied().sum::<Vec3>() / face.vertices.len() as f32;
                let probe = centroid + face.normal * 0.01;
                assert!(
                    !shape_contains(shape, probe) || on_cell_boundary(probe),
                    "{shape:?} has an inward face"
                );
            }
        }
    }
}
