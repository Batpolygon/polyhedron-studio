//! Procedural voxel mesher.
//!
//! Full cubes are meshed with a three-axis greedy pass: each visible face is
//! merged into the largest possible same-block rectangle when greedy merging is
//! enabled, otherwise every exposed face stays an independent quad. The other
//! eight shapes (and fused sub-voxel halves) are emitted from their canonical
//! polygons, rotated by the voxel orientation. Any polygon lying on a cell face
//! is culled when the neighbour fully covers that face, so slabs, stairs and
//! ramps hide each other exactly like cubes do.
//!
//! The same buffers feed the Rapier `Collider::trimesh` of the chunk, so 45°
//! ramps and stairs collide continuously.
//!
//! Geometry is grouped by [`BlockId`] so the renderer can use one material and
//! one entity per `(chunk, block)` pair.

use std::collections::BTreeMap;

use bevy::prelude::*;

use crate::voxel::blocks::{BlockDef, BlockId, BlockModel, BlockRegistry};
use crate::voxel::shapes::{
    boundary_face, canonical_faces, covers_face, cube_part_facing, face_part, pair_covers_face,
    Facing, Orientation,
};
use crate::voxel::world::{Voxel, VoxelWorld, CHUNK_SIZE, FLUID_FALLING};

/// CPU-side mesh buffers ready to be uploaded to Bevy.
#[derive(Clone, Debug, Default)]
pub struct MeshData {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
    /// Orientation index (0..24) of the voxel each vertex belongs to. The
    /// shader turns the block's surface pattern with it; 0 (the orientation
    /// of generated terrain) picks a random direction per cell, like the
    /// random texture rotation of Minecraft.
    pub orientations: Vec<f32>,
    /// Orientation applied to the vertices emitted next.
    pub current_orientation: f32,
    /// Linear vertex colours, only for blocks with painted parts (their
    /// material is white, see `block_material`).
    pub colors: Vec<[f32; 4]>,
    /// Colour of the vertices emitted next.
    pub current_color: Option<[f32; 4]>,
}

impl MeshData {
    fn push_vertex(&mut self, position: Vec3, normal: Vec3, uv: Vec2) -> u32 {
        let index = self.positions.len() as u32;
        self.positions.push(position.to_array());
        self.normals.push(normal.to_array());
        self.uvs.push(uv.to_array());
        self.orientations.push(self.current_orientation);
        if let Some(color) = self.current_color {
            self.colors.push(color);
        }
        index
    }

    fn quad(&mut self, corners: [Vec3; 4], normal: Vec3, uvs: [Vec2; 4]) {
        let base = self.positions.len() as u32;
        for (corner, uv) in corners.into_iter().zip(uvs) {
            self.push_vertex(corner, normal, uv);
        }
        self.indices
            .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    /// Emit a quad and let the helper fix the winding if the supplied corners
    /// are ordered clockwise when seen from `outward`.
    fn quad_oriented(&mut self, corners: [Vec3; 4], outward: Vec3, uvs: [Vec2; 4]) {
        let normal = (corners[1] - corners[0]).cross(corners[3] - corners[0]);
        let outward = outward.normalize_or_zero();
        if normal.dot(outward) < 0.0 {
            self.quad(
                [corners[0], corners[3], corners[2], corners[1]],
                outward,
                [uvs[0], uvs[3], uvs[2], uvs[1]],
            );
        } else {
            self.quad(corners, outward, uvs);
        }
    }

    fn triangle(&mut self, corners: [Vec3; 3], normal: Vec3, uvs: [Vec2; 3]) {
        let base = self.positions.len() as u32;
        for (corner, uv) in corners.into_iter().zip(uvs) {
            self.push_vertex(corner, normal, uv);
        }
        self.indices
            .extend_from_slice(&[base, base + 1, base + 2]);
    }

    fn triangle_oriented(&mut self, corners: [Vec3; 3], outward: Vec3, uvs: [Vec2; 3]) {
        let normal = (corners[1] - corners[0]).cross(corners[2] - corners[0]);
        let outward = outward.normalize_or_zero();
        if normal.dot(outward) < 0.0 {
            self.triangle(
                [corners[0], corners[2], corners[1]],
                outward,
                [uvs[0], uvs[2], uvs[1]],
            );
        } else {
            self.triangle(corners, outward, uvs);
        }
    }
}

fn vertex_color([r, g, b]: [u8; 3]) -> [f32; 4] {
    Color::srgb_u8(r, g, b).to_linear().to_f32_array()
}

/// Vertex colour of a part of `def`, when the block has painted parts.
fn part_color(def: &BlockDef, part: usize) -> Option<[f32; 4]> {
    if def.part_colors.is_empty() {
        return None;
    }
    Some(vertex_color(def.part_colors.get(part).unwrap_or(def.color)))
}

/// Build every block surface of a chunk.
pub fn build_chunk_mesh(
    world: &VoxelWorld,
    registry: &BlockRegistry,
    coord: IVec3,
    fusion: bool,
) -> BTreeMap<BlockId, MeshData> {
    let mut output: BTreeMap<BlockId, MeshData> = BTreeMap::new();
    let Some(chunk) = world.chunk(coord) else {
        return output;
    };

    for axis in 0..3 {
        for sign in [-1, 1] {
            greedy_faces(world, registry, chunk, axis, sign, fusion, &mut output);
        }
    }

    let chunk_origin = chunk.origin();
    for y in 0..CHUNK_SIZE {
        for z in 0..CHUNK_SIZE {
            for x in 0..CHUNK_SIZE {
                let local = UVec3::new(x as u32, y as u32, z as u32);
                let Some(voxel) = chunk.get(local) else {
                    continue;
                };
                let def = registry.get(voxel.block);
                if voxel.sub.is_none() && def.is_plain_cube() {
                    continue;
                }

                let origin = Vec3::new(x as f32, y as f32, z as f32);
                let world_cell = chunk_origin + local.as_ivec3();
                if voxel.sub.is_none() && def.is_liquid() {
                    let data = output.entry(voxel.block).or_default();
                    data.current_orientation = 0.0;
                    emit_liquid(data, world, registry, world_cell, voxel.block, origin);
                    continue;
                }
                if let (None, Some(model)) = (voxel.sub, def.custom_model()) {
                    let data = output.entry(voxel.block).or_default();
                    data.current_orientation = voxel.orientation.index() as f32;
                    emit_model(data, world, registry, world_cell, def, model, voxel.orientation, origin);
                    continue;
                }
                for (block, orientation) in voxel.parts() {
                    let data = output.entry(block).or_default();
                    data.current_orientation = orientation.index() as f32;
                    emit_oriented_shape(
                        data,
                        world,
                        registry,
                        world_cell,
                        registry.get(block),
                        orientation,
                        origin,
                    );
                }
            }
        }
    }

    output.retain(|_, data| !data.indices.is_empty());
    output
}

/// Does the voxel fully cover one of its cell faces (all parts combined)?
/// Liquids and custom models never hide their neighbours.
pub fn cell_covers(registry: &BlockRegistry, voxel: &Voxel, face: Facing) -> bool {
    let def = registry.get(voxel.block);
    if voxel.sub.is_none() && (def.is_liquid() || def.custom_model().is_some()) {
        return false;
    }
    let main = (def.shape, voxel.orientation);
    match voxel.sub {
        None => covers_face(main.0, main.1, face),
        Some(sub) => pair_covers_face(main, (registry.get(sub.block).shape, sub.orientation), face),
    }
}

fn facing_for(axis: usize, sign: i32) -> Facing {
    match (axis, sign > 0) {
        (0, true) => Facing::PosX,
        (0, false) => Facing::NegX,
        (1, true) => Facing::PosY,
        (1, false) => Facing::NegY,
        (_, true) => Facing::PosZ,
        (_, false) => Facing::NegZ,
    }
}

fn greedy_faces(
    world: &VoxelWorld,
    registry: &BlockRegistry,
    chunk: &crate::voxel::world::VoxelChunk,
    axis: usize,
    sign: i32,
    fusion: bool,
    output: &mut BTreeMap<BlockId, MeshData>,
) {
    let u_axis = (axis + 1) % 3;
    let v_axis = (axis + 2) % 3;
    let origin = chunk.origin();
    let mut mask = vec![None; CHUNK_SIZE * CHUNK_SIZE];

    for slice in 0..CHUNK_SIZE {
        for v in 0..CHUNK_SIZE {
            for u in 0..CHUNK_SIZE {
                let mut local = [0usize; 3];
                local[axis] = slice;
                local[u_axis] = u;
                local[v_axis] = v;

                let cell = UVec3::new(
                    local[0] as u32,
                    local[1] as u32,
                    local[2] as u32,
                );
                let Some(voxel) = chunk.get(cell) else {
                    mask[u + v * CHUNK_SIZE] = None;
                    continue;
                };
                if voxel.sub.is_some() || !registry.get(voxel.block).is_plain_cube() {
                    mask[u + v * CHUNK_SIZE] = None;
                    continue;
                }

                let mut neighbour = [0i32; 3];
                neighbour[axis] = sign;
                let world_position = origin
                    + IVec3::new(
                        local[0] as i32 + neighbour[0],
                        local[1] as i32 + neighbour[1],
                        local[2] as i32 + neighbour[2],
                    );

                let face = facing_for(axis, sign);
                let hidden = world
                    .get(world_position)
                    .map(|neighbour| cell_covers(registry, &neighbour, face.opposite()))
                    .unwrap_or(false);

                mask[u + v * CHUNK_SIZE] = if hidden { None } else { Some((voxel.block, voxel.orientation.index() as u8)) };
            }
        }

        for v in 0..CHUNK_SIZE {
            let mut u = 0;
            while u < CHUNK_SIZE {
                let index = u + v * CHUNK_SIZE;
                let Some(key) = mask[index] else {
                    u += 1;
                    continue;
                };
                let block = key.0;

                let mut width = 1;
                if fusion {
                    while u + width < CHUNK_SIZE && mask[index + width] == Some(key) {
                        width += 1;
                    }
                }

                let mut height = 1;
                if fusion {
                    'grow: while v + height < CHUNK_SIZE {
                        for offset in 0..width {
                            let probe = (u + offset) + (v + height) * CHUNK_SIZE;
                            if mask[probe] != Some(key) {
                                break 'grow;
                            }
                        }
                        height += 1;
                    }
                }

                for dv in 0..height {
                    for du in 0..width {
                        mask[(u + du) + (v + dv) * CHUNK_SIZE] = None;
                    }
                }

                emit_face(output, registry, block, key.1, axis, sign, slice, u, v, width, height);
                u += width;
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn emit_face(
    output: &mut BTreeMap<BlockId, MeshData>,
    registry: &BlockRegistry,
    block: BlockId,
    orientation: u8,
    axis: usize,
    sign: i32,
    slice: usize,
    u: usize,
    v: usize,
    width: usize,
    height: usize,
) {
    let mut base = [0.0f32; 3];
    base[axis] = slice as f32;
    base[(axis + 1) % 3] = u as f32;
    base[(axis + 2) % 3] = v as f32;

    let axis_vec = axis_unit(axis);
    let u_vec = axis_unit((axis + 1) % 3);
    let v_vec = axis_unit((axis + 2) % 3);
    let mut base = Vec3::from_array(base);
    if sign > 0 {
        base += axis_vec;
    }

    let (t1, size1, t2, size2) = if sign > 0 {
        (u_vec, width as f32, v_vec, height as f32)
    } else {
        (v_vec, height as f32, u_vec, width as f32)
    };

    let corners = [
        base,
        base + t1 * size1,
        base + t1 * size1 + t2 * size2,
        base + t2 * size2,
    ];
    let normal = axis_vec * sign as f32;
    let uvs = [
        Vec2::new(0.0, 0.0),
        Vec2::new(size1, 0.0),
        Vec2::new(size1, size2),
        Vec2::new(0.0, size2),
    ];

    // Greedy quads share block, orientation and facing, hence the same part.
    let local_normal = Orientation(orientation).rotation().inverse() * normal;
    let data = output.entry(block).or_default();
    data.current_orientation = orientation as f32;
    data.current_color = part_color(registry.get(block), cube_part_facing(local_normal));
    data.quad_oriented(corners, normal, uvs);
}

fn axis_unit(axis: usize) -> Vec3 {
    match axis {
        0 => Vec3::X,
        1 => Vec3::Y,
        _ => Vec3::Z,
    }
}

/// Emit one oriented shape, culling polygons hidden by a covering neighbour.
fn emit_oriented_shape(
    data: &mut MeshData,
    world: &VoxelWorld,
    registry: &BlockRegistry,
    world_cell: IVec3,
    def: &BlockDef,
    orientation: Orientation,
    origin: Vec3,
) {
    let shape = def.shape;
    for (index, face) in canonical_faces(shape).iter().enumerate() {
        let vertices: Vec<Vec3> = face
            .vertices
            .iter()
            .map(|vertex| orientation.apply(*vertex))
            .collect();

        if let Some(cell_face) = boundary_face(&vertices) {
            let neighbour = world_cell + cell_face.vector();
            if let Some(other) = world.get(neighbour) {
                if cell_covers(registry, &other, cell_face.opposite()) {
                    continue;
                }
            }
        }

        data.current_color = part_color(def, face_part(shape, index));
        let normal = orientation.apply_direction(face.normal).normalize_or_zero();
        let uvs: Vec<Vec2> = vertices.iter().map(|v| planar_uv(*v, normal)).collect();
        let positions: Vec<Vec3> = vertices.iter().map(|v| *v + origin).collect();

        match positions.len() {
            3 => data.triangle_oriented(
                [positions[0], positions[1], positions[2]],
                normal,
                [uvs[0], uvs[1], uvs[2]],
            ),
            4 => data.quad_oriented(
                [positions[0], positions[1], positions[2], positions[3]],
                normal,
                [uvs[0], uvs[1], uvs[2], uvs[3]],
            ),
            _ => {
                // Fan-triangulate larger convex polygons.
                for index in 1..positions.len() - 1 {
                    data.triangle_oriented(
                        [positions[0], positions[index], positions[index + 1]],
                        normal,
                        [uvs[0], uvs[index], uvs[index + 1]],
                    );
                }
            }
        }
    }
}

/// Surface height of a liquid cell, in `[0, 1]`.
fn liquid_height(world: &VoxelWorld, cell: IVec3, block: BlockId) -> f32 {
    let above = world.get(cell + IVec3::Y);
    if above.map(|voxel| voxel.block == block && voxel.sub.is_none()).unwrap_or(false) {
        return 1.0;
    }
    match world.fluid_level(cell) {
        0 => 0.875,
        FLUID_FALLING => 0.92,
        level => (0.875 * (8 - level.min(7)) as f32 / 8.0).max(0.08),
    }
}

/// Height of a liquid surface corner, averaged over the up to four cells
/// sharing it so neighbouring cells always meet without gaps.
fn liquid_corner(world: &VoxelWorld, cell: IVec3, block: BlockId, cx: i32, cz: i32) -> f32 {
    let mut total = 0.0;
    let mut count = 0.0;
    for dz in (cz - 1)..=cz {
        for dx in (cx - 1)..=cx {
            let probe = cell + IVec3::new(dx, 0, dz);
            let Some(voxel) = world.get(probe) else {
                continue;
            };
            if voxel.block != block || voxel.sub.is_some() {
                continue;
            }
            let height = liquid_height(world, probe, block);
            if height >= 1.0 {
                return 1.0;
            }
            total += height;
            count += 1.0;
        }
    }
    if count > 0.0 { total / count } else { liquid_height(world, cell, block) }
}

/// Liquid cell: a box whose top follows the flow levels around it.
fn emit_liquid(
    data: &mut MeshData,
    world: &VoxelWorld,
    registry: &BlockRegistry,
    cell: IVec3,
    block: BlockId,
    origin: Vec3,
) {
    let same = |offset: IVec3| {
        world
            .get(cell + offset)
            .map(|voxel| voxel.block == block && voxel.sub.is_none())
            .unwrap_or(false)
    };
    let covered = |offset: IVec3, face: Facing| {
        world
            .get(cell + offset)
            .map(|voxel| cell_covers(registry, &voxel, face))
            .unwrap_or(false)
    };

    let h00 = liquid_corner(world, cell, block, 0, 0);
    let h10 = liquid_corner(world, cell, block, 1, 0);
    let h11 = liquid_corner(world, cell, block, 1, 1);
    let h01 = liquid_corner(world, cell, block, 0, 1);
    let corner = |x: f32, h: f32, z: f32| origin + Vec3::new(x, h, z);

    // Top surface.
    let full = h00.min(h10).min(h11).min(h01) >= 0.999;
    if !same(IVec3::Y) && !(full && covered(IVec3::Y, Facing::NegY)) {
        let corners = [corner(0.0, h00, 0.0), corner(1.0, h10, 0.0), corner(1.0, h11, 1.0), corner(0.0, h01, 1.0)];
        let normal = (corners[2] - corners[0]).cross(corners[1] - corners[3]).normalize_or(Vec3::Y);
        let normal = if normal.y < 0.0 { -normal } else { normal };
        let uvs = corners.map(|c| Vec2::new(c.x, c.z));
        data.quad_oriented(corners, normal, uvs);
    }
    // Bottom.
    if !same(IVec3::NEG_Y) && !covered(IVec3::NEG_Y, Facing::PosY) {
        let corners = [corner(0.0, 0.0, 0.0), corner(1.0, 0.0, 0.0), corner(1.0, 0.0, 1.0), corner(0.0, 0.0, 1.0)];
        let uvs = corners.map(|c| Vec2::new(c.x, c.z));
        data.quad_oriented(corners, Vec3::NEG_Y, uvs);
    }
    // Sides: from the floor up to the corner heights.
    let sides = [
        (IVec3::X, Facing::NegX, [(1.0, 0.0, h10), (1.0, 1.0, h11)]),
        (IVec3::NEG_X, Facing::PosX, [(0.0, 1.0, h01), (0.0, 0.0, h00)]),
        (IVec3::Z, Facing::NegZ, [(1.0, 1.0, h11), (0.0, 1.0, h01)]),
        (IVec3::NEG_Z, Facing::PosZ, [(0.0, 0.0, h00), (1.0, 0.0, h10)]),
    ];
    for (offset, opposite, [(ax, az, ah), (bx, bz, bh)]) in sides {
        if same(offset) || covered(offset, opposite) {
            continue;
        }
        let corners = [corner(ax, 0.0, az), corner(bx, 0.0, bz), corner(bx, bh, bz), corner(ax, ah, az)];
        let normal = offset.as_vec3();
        let uvs = corners.map(|c| planar_uv(c - origin, normal));
        data.quad_oriented(corners, normal, uvs);
    }
}

/// Custom multi-box model (block forge), rotated by the voxel orientation.
#[allow(clippy::too_many_arguments)]
fn emit_model(
    data: &mut MeshData,
    world: &VoxelWorld,
    registry: &BlockRegistry,
    cell: IVec3,
    def: &BlockDef,
    model: &BlockModel,
    orientation: crate::voxel::shapes::Orientation,
    origin: Vec3,
) {
    let colored = def.has_vertex_colors();
    for element in &model.elements {
        data.current_color = colored.then(|| vertex_color(element.color.unwrap_or(def.color)));
        let (min, max) = element.bounds();
        if (max - min).min_element() <= 1e-4 {
            continue;
        }
        for face in box_faces(min, max) {
            let vertices: Vec<Vec3> = face.1.iter().map(|vertex| orientation.apply(*vertex)).collect();
            if let Some(cell_face) = boundary_face(&vertices) {
                if let Some(other) = world.get(cell + cell_face.vector()) {
                    if cell_covers(registry, &other, cell_face.opposite()) {
                        continue;
                    }
                }
            }
            let normal = orientation.apply_direction(face.0).normalize_or_zero();
            let uvs = [0, 1, 2, 3].map(|i| planar_uv(vertices[i], normal));
            let corners = [0, 1, 2, 3].map(|i| vertices[i] + origin);
            data.quad_oriented(corners, normal, uvs);
        }
    }
}

/// The six faces of an axis-aligned box: (outward normal, corners).
fn box_faces(min: Vec3, max: Vec3) -> [(Vec3, [Vec3; 4]); 6] {
    let p = |x: f32, y: f32, z: f32| Vec3::new(x, y, z);
    [
        (Vec3::X, [p(max.x, min.y, min.z), p(max.x, max.y, min.z), p(max.x, max.y, max.z), p(max.x, min.y, max.z)]),
        (Vec3::NEG_X, [p(min.x, min.y, min.z), p(min.x, min.y, max.z), p(min.x, max.y, max.z), p(min.x, max.y, min.z)]),
        (Vec3::Y, [p(min.x, max.y, min.z), p(min.x, max.y, max.z), p(max.x, max.y, max.z), p(max.x, max.y, min.z)]),
        (Vec3::NEG_Y, [p(min.x, min.y, min.z), p(max.x, min.y, min.z), p(max.x, min.y, max.z), p(min.x, min.y, max.z)]),
        (Vec3::Z, [p(min.x, min.y, max.z), p(max.x, min.y, max.z), p(max.x, max.y, max.z), p(min.x, max.y, max.z)]),
        (Vec3::NEG_Z, [p(min.x, min.y, min.z), p(min.x, max.y, min.z), p(max.x, max.y, min.z), p(max.x, min.y, min.z)]),
    ]
}

/// Box-projected UV: drop the dominant normal axis.
fn planar_uv(position: Vec3, normal: Vec3) -> Vec2 {
    let n = normal.abs();
    if n.x >= n.y && n.x >= n.z {
        Vec2::new(position.z, position.y)
    } else if n.y >= n.z {
        Vec2::new(position.x, position.z)
    } else {
        Vec2::new(position.x, position.y)
    }
}

/// Concatenate every solid block surface of a chunk into one collision soup
/// (liquids are swum through, not walked on).
/// Expansion directions for the inverted-hull outline shell.
///
/// Pushing every face along its own normal splits the hull apart at edges and
/// corners. Instead, each vertex is pushed along the *sum* of the distinct
/// normals of every face touching that point (T-junctions of greedy quads
/// included), so the shells of neighbouring faces meet exactly: a cube corner
/// moves by `(±1, ±1, ±1)`, an edge by `n1 + n2`, a flat area by `n`.
pub fn outline_directions(data: &MeshData) -> Vec<[f32; 3]> {
    use std::collections::HashMap;
    const Q: f32 = 16.0;
    let key = |p: Vec3| -> [i32; 3] { [(p.x * Q).round() as i32, (p.y * Q).round() as i32, (p.z * Q).round() as i32] };
    let normal_key = |n: Vec3| -> [i8; 3] { [(n.x * 60.0).round() as i8, (n.y * 60.0).round() as i8, (n.z * 60.0).round() as i8] };
    let mut touching: HashMap<[i32; 3], Vec<[i8; 3]>> = HashMap::with_capacity(data.positions.len());
    let mut add = |point: [i32; 3], normal: [i8; 3]| {
        let entry = touching.entry(point).or_default();
        if !entry.contains(&normal) {
            entry.push(normal);
        }
    };
    for triangle in data.indices.chunks_exact(3) {
        let corners = [triangle[0], triangle[1], triangle[2]].map(|i| Vec3::from_array(data.positions[i as usize]));
        let normal = Vec3::from_array(data.normals[triangle[0] as usize]).normalize_or_zero();
        let nk = normal_key(normal);
        for edge in 0..3 {
            let (a, b) = (corners[edge], corners[(edge + 1) % 3]);
            add(key(a), nk);
            // Walk axis-aligned edges on the voxel lattice so vertices of other
            // quads lying on this edge (T-junctions) see this face too.
            let delta = b - a;
            let axes = (delta.x.abs() > 1e-4) as u8 + (delta.y.abs() > 1e-4) as u8 + (delta.z.abs() > 1e-4) as u8;
            if axes != 1 {
                continue;
            }
            let length = delta.length();
            let direction = delta / length;
            let start = (a.dot(direction)).ceil();
            let mut along = start;
            let end = b.dot(direction);
            let base = a - direction * a.dot(direction);
            while along < end - 1e-4 {
                add(key(base + direction * along), nk);
                along += 1.0;
            }
        }
    }
    data.positions
        .iter()
        .zip(&data.normals)
        .map(|(position, own)| {
            let own = Vec3::from_array(*own).normalize_or_zero();
            let Some(normals) = touching.get(&key(Vec3::from_array(*position))) else {
                return own.to_array();
            };
            let mut sum = Vec3::ZERO;
            for n in normals {
                let n = Vec3::new(n[0] as f32, n[1] as f32, n[2] as f32) / 60.0;
                // Faces turned away from this one (thin walls) are ignored.
                if n.dot(own) > -0.5 {
                    sum += n.normalize_or_zero();
                }
            }
            if sum.length_squared() < 1e-6 {
                return own.to_array();
            }
            // Keep the push along the face normal at exactly one unit.
            let along = sum.dot(own);
            if along > 1e-3 {
                sum /= along;
            }
            sum.clamp_length_max(2.0).to_array()
        })
        .collect()
}

pub fn collision_soup(
    surfaces: &BTreeMap<BlockId, MeshData>,
    registry: &BlockRegistry,
) -> (Vec<Vec3>, Vec<[u32; 3]>) {
    let mut vertices = Vec::new();
    let mut triangles = Vec::new();
    for (block, data) in surfaces {
        if registry.get(*block).is_liquid() {
            continue;
        }
        let base = vertices.len() as u32;
        vertices.extend(data.positions.iter().map(|p| Vec3::from_array(*p)));
        for triangle in data.indices.chunks_exact(3) {
            triangles.push([base + triangle[0], base + triangle[1], base + triangle[2]]);
        }
    }
    (vertices, triangles)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::voxel::shapes::VoxelShape;
    use crate::voxel::world::{Voxel, VoxelWorld};

    fn cube_block(fusion: bool) -> MeshData {
        let registry = BlockRegistry::builtin();
        let block = registry.id_or_default("cube_teal");
        let mut world = VoxelWorld::new();

        for x in 0..2 {
            for y in 0..2 {
                for z in 0..2 {
                    world.set(IVec3::new(x, y, z), Some(Voxel::new(block)));
                }
            }
        }

        let meshes = build_chunk_mesh(&world, &registry, IVec3::ZERO, fusion);
        meshes.get(&block).cloned().expect("cube surface")
    }

    #[test]
    fn fused_cube_block_merges_into_six_quads() {
        let mesh = cube_block(true);
        assert_eq!(mesh.positions.len(), 24);
        assert_eq!(mesh.indices.len(), 36);
    }

    #[test]
    fn unfused_cube_block_keeps_every_exposed_face() {
        let mesh = cube_block(false);
        assert_eq!(mesh.positions.len(), 96);
        assert_eq!(mesh.indices.len(), 144);
    }

    #[test]
    fn fused_slabs_hide_like_a_cube() {
        use crate::voxel::shapes::{Facing, Orientation};
        use crate::voxel::world::SubVoxel;
        let registry = BlockRegistry::builtin();
        let slab = registry.id_or_default("slab_stone");
        let cube = registry.id_or_default("cube_teal");
        let mut world = VoxelWorld::new();
        world.set(
            IVec3::new(1, 0, 0),
            Some(Voxel::new(slab).with_sub(SubVoxel {
                block: slab,
                orientation: Orientation::new(Facing::NegY, 0),
            })),
        );
        world.set(IVec3::ZERO, Some(Voxel::new(cube)));
        let meshes = build_chunk_mesh(&world, &registry, IVec3::ZERO, true);
        // The cube's +X face is hidden by the fused cell.
        assert_eq!(meshes.get(&cube).expect("cube").positions.len(), 20);
    }

    #[test]
    fn every_non_cube_shape_emits_geometry() {
        let registry = BlockRegistry::builtin();

        for (block, def) in registry.blocks() {
            if def.shape == VoxelShape::Cube {
                continue;
            }

            let mut world = VoxelWorld::new();
            world.set(IVec3::ZERO, Some(Voxel::new(block)));

            let meshes = build_chunk_mesh(&world, &registry, IVec3::ZERO, true);
            let data = meshes.get(&block).expect("shape surface");
            assert!(
                !data.positions.is_empty(),
                "{} produced no vertices",
                def.label
            );
            assert_eq!(data.indices.len() % 3, 0);
        }
    }

    #[test]
    fn liquids_do_not_hide_solid_neighbours_and_have_no_collision() {
        let registry = BlockRegistry::builtin();
        let water = registry.id_or_default("water");
        let stone = registry.id_or_default("stone");
        let mut world = VoxelWorld::new();
        world.set(IVec3::ZERO, Some(Voxel::new(stone)));
        world.set(IVec3::X, Some(Voxel::new(water)));
        let meshes = build_chunk_mesh(&world, &registry, IVec3::ZERO, true);
        // The stone keeps all six faces.
        assert_eq!(meshes.get(&stone).expect("stone").positions.len(), 24);
        assert!(meshes.contains_key(&water));
        let (_, triangles) = collision_soup(&meshes, &registry);
        assert_eq!(triangles.len(), 12);
    }

    #[test]
    fn custom_models_emit_one_box_per_element() {
        use crate::voxel::blocks::{BlockDef, BlockModel, ModelElement};
        let mut registry = BlockRegistry::builtin();
        let mut def = BlockDef::new("test_table", "Table", crate::voxel::shapes::VoxelShape::Cube, [178, 128, 84]);
        def.model = Some(BlockModel {
            elements: vec![
                ModelElement::new("Top", [0.0, 13.0, 0.0], [16.0, 16.0, 16.0]),
                ModelElement::new("Leg 1", [1.0, 0.0, 1.0], [4.0, 13.0, 4.0]),
                ModelElement::new("Leg 2", [12.0, 0.0, 1.0], [15.0, 13.0, 4.0]),
                ModelElement::new("Leg 3", [1.0, 0.0, 12.0], [4.0, 13.0, 15.0]),
                ModelElement::new("Leg 4", [12.0, 0.0, 12.0], [15.0, 13.0, 15.0]),
            ],
        });
        let table = registry.upsert(def);
        let mut world = VoxelWorld::new();
        world.set(IVec3::ZERO, Some(Voxel::new(table)));
        let meshes = build_chunk_mesh(&world, &registry, IVec3::ZERO, true);
        let data = meshes.get(&table).expect("table");
        assert_eq!(data.positions.len(), 5 * 24);
    }

    /// Top of a variant painted red, the rest in the block colour.
    fn red_top_variant(registry: &mut BlockRegistry, base: &str) -> BlockId {
        use crate::voxel::blocks::SyntheticLook;
        use crate::voxel::shapes::shape_parts;
        let block = registry.id_or_default(base);
        let def = registry.get(block);
        let mut look = SyntheticLook::of(def);
        let top = shape_parts(def.shape).iter().rposition(|part| part.normal.y > 0.99).expect("top");
        look.parts.set(top, [255, 0, 0]);
        registry.ensure_variant(&BlockRegistry::variant_key(base, &look)).expect("variant")
    }

    #[test]
    fn painted_parts_follow_the_orientation() {
        let mut registry = BlockRegistry::builtin();
        let block = red_top_variant(&mut registry, "cube_teal");
        for orientation in [Orientation::IDENTITY, Orientation::new(Facing::PosX, 1)] {
            let mut world = VoxelWorld::new();
            world.set(IVec3::ZERO, Some(Voxel::oriented(block, orientation)));
            let meshes = build_chunk_mesh(&world, &registry, IVec3::ZERO, true);
            let data = meshes.get(&block).expect("cube");
            assert_eq!(data.colors.len(), data.positions.len());
            let up = orientation.apply_direction(Vec3::Y);
            for (normal, color) in data.normals.iter().zip(&data.colors) {
                let red = color[1] < 0.01;
                assert_eq!(red, Vec3::from_array(*normal).dot(up) > 0.9, "{orientation:?}");
            }
        }
    }

    #[test]
    fn painted_shapes_colour_every_vertex() {
        let mut registry = BlockRegistry::builtin();
        let block = red_top_variant(&mut registry, "arch_lime");
        let mut world = VoxelWorld::new();
        world.set(IVec3::ZERO, Some(Voxel::new(block)));
        let meshes = build_chunk_mesh(&world, &registry, IVec3::ZERO, true);
        let data = meshes.get(&block).expect("stairs");
        assert_eq!(data.colors.len(), data.positions.len());
        // Plain blocks carry no colour attribute at all.
        assert!(cube_block(true).colors.is_empty());
    }

    #[test]
    fn model_elements_keep_their_own_colour() {
        use crate::voxel::blocks::{BlockDef, BlockModel, ModelElement};
        let mut registry = BlockRegistry::builtin();
        let mut def = BlockDef::new("test_two_tone", "Two tone", VoxelShape::Cube, [0, 255, 0]);
        let mut top = ModelElement::new("Top", [0.0, 8.0, 0.0], [16.0, 16.0, 16.0]);
        top.color = Some([255, 0, 0]);
        def.model = Some(BlockModel {
            elements: vec![ModelElement::new("Base", [0.0, 0.0, 0.0], [16.0, 8.0, 16.0]), top],
        });
        let block = registry.upsert(def);
        let mut world = VoxelWorld::new();
        world.set(IVec3::ZERO, Some(Voxel::new(block)));
        let meshes = build_chunk_mesh(&world, &registry, IVec3::ZERO, true);
        let data = meshes.get(&block).expect("model");
        assert_eq!(data.colors.len(), data.positions.len());
        for (position, color) in data.positions.iter().zip(&data.colors) {
            // Both boxes meet at y = 0.5; elsewhere the height tells them apart.
            if position[1] != 0.5 {
                assert_eq!(color[0] > 0.5, position[1] > 0.5, "{position:?}");
                assert_eq!(color[1] > 0.5, position[1] < 0.5, "{position:?}");
            }
        }
    }

    #[test]
    fn outline_directions_join_at_cube_corners() {
        let data = cube_block(true);
        let directions = outline_directions(&data);
        for (position, direction) in data.positions.iter().zip(&directions) {
            let p = Vec3::from_array(*position);
            let d = Vec3::from_array(*direction);
            // Every corner of a lone cube moves diagonally away from its centre.
            let expected = (p - Vec3::splat(1.0)).signum();
            assert!((d - expected).length() < 1e-3, "{p:?} -> {d:?}");
        }
    }

    #[test]
    fn hidden_faces_are_culled_across_chunk_boundaries() {
        let registry = BlockRegistry::builtin();
        let block = registry.id_or_default("cube_teal");
        let mut world = VoxelWorld::new();

        world.set(IVec3::new(15, 0, 0), Some(Voxel::new(block)));
        world.set(IVec3::new(16, 0, 0), Some(Voxel::new(block)));

        let meshes = build_chunk_mesh(&world, &registry, IVec3::ZERO, true);
        let data = meshes.get(&block).expect("cube surface");
        assert_eq!(data.positions.len(), 20);
        assert_eq!(data.indices.len(), 30);
    }
}
