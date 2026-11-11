//! Polyhedron toon shading extension.
//!
//! This fragment shader is used as the `MaterialExtension` of the voxel
//! `ExtendedMaterial<StandardMaterial, PolyMaterial>`. It keeps every Bevy PBR
//! light and shadow, then layers the studio look on top:
//!
//! - a banded luminance quantisation (cel shading),
//! - a 3D Voronoi (Worley) cell tint,
//! - a low-poly Voronoi normal map: every cell becomes a tilted facet,
//! - a Fresnel rim light.
//!
//! The Voronoi patterns are computed in **block space** (the position inside
//! the voxel, from the mesh-local position passed by the vertex stage), so
//! they stay attached to a block when it moves (movables, rotors). The
//! pattern turns with the voxel orientation (UV_1.x); generated terrain
//! (orientation 0) gets a random direction per cell.
//!
//! The vertex stage also animates the vertices: leaves and plants sway in the
//! wind (`wind`), liquid surfaces wave (`wave`).
//!
//! The parameters live in a single uniform at binding 100 of the material bind
//! group, which leaves bindings 0-99 to `StandardMaterial`.
//!
//! @version 4

import bevy_pbr::render::{
    forward_io::{FragmentOutput, Vertex, VertexOutput, decompress_vertex},
    mesh_functions,
    mesh_view_bindings::globals,
    view_transformations::position_world_to_clip,
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{
        alpha_discard, apply_pbr_lighting, main_pass_post_lighting_processing,
    },
};

struct PolyShading {
    toon_enabled: u32,
    voronoi_enabled: u32,
    rim_enabled: u32,
    bands: u32,
    band_softness: f32,
    voronoi_strength: f32,
    voronoi_scale: f32,
    rim_strength: f32,
    rim_color: vec4<f32>,
    voronoi_color: vec4<f32>,
    normal_strength: f32,
    normal_cells: f32,
    normal_bevel: f32,
    normal_seed: f32,
    /// Wind sway of the vertices (0 = still).
    wind: f32,
    /// Height of the liquid waves (0 = flat).
    wave: f32,
    /// Flecks of the block: cells along an edge and emission.
    accent_cells: f32,
    accent_glow: f32,
    /// Colour of the flecks; `a` is the share of the cells (0 = none).
    accent: vec4<f32>,
};

@group(constants::MATERIAL_BIND_GROUP) @binding(100)
var<uniform> shading: PolyShading;

struct VoronoiSample {
    first: f32,
    second: f32,
    cell: f32,
    feature: vec3<f32>,
};

/// Hash a lattice cell into a 3D jitter vector in the unit cube.
fn hash3(position: vec3<f32>) -> vec3<f32> {
    let waves = vec3<f32>(
        dot(position, vec3<f32>(127.1, 311.7, 74.7)),
        dot(position, vec3<f32>(269.5, 183.3, 246.1)),
        dot(position, vec3<f32>(113.5, 271.9, 124.6)),
    );
    return fract(sin(waves) * 43758.5453);
}

/// Hash a lattice cell into a single stable scalar.
fn hash1(position: vec3<f32>) -> f32 {
    return fract(sin(dot(position, vec3<f32>(127.1, 311.7, 74.7))) * 43758.5453);
}

/// Sample the two closest feature points of a 3x3x3 Voronoi neighbourhood.
fn voronoi_sample(position: vec3<f32>) -> VoronoiSample {
    let base = floor(position);
    let local = fract(position);
    var first = 8.0;
    var second = 8.0;
    var cell = 0.0;
    var nearest = base;

    for (var z: i32 = -1; z <= 1; z = z + 1) {
        for (var y: i32 = -1; y <= 1; y = y + 1) {
            for (var x: i32 = -1; x <= 1; x = x + 1) {
                let offset = vec3<f32>(f32(x), f32(y), f32(z));
                let feature = base + offset;
                let jitter = hash3(feature);
                let delta = offset + jitter - local;
                let distance = length(delta);

                if distance < first {
                    second = first;
                    first = distance;
                    cell = hash1(feature);
                    nearest = feature;
                } else if distance < second {
                    second = distance;
                }
            }
        }
    }

    return VoronoiSample(first, second, cell, nearest);
}

/// Displacement of a vertex by the wind (shared with the outline shader).
fn wind_offset(p: vec3<f32>, strength: f32, time: f32) -> vec3<f32> {
    let sway = sin(time * 1.7 + p.x * 0.45 + p.z * 0.31) + 0.5 * sin(time * 2.9 + p.x * 1.3 - p.y * 0.7 + p.z * 0.9);
    let flutter = sin(time * 6.3 + dot(p, vec3<f32>(3.1, 2.3, 4.7))) * 0.25;
    return vec3<f32>(
        (sway + flutter) * 0.045,
        flutter * 0.015,
        (cos(time * 1.3 + p.z * 0.4 + p.x * 0.2) + flutter) * 0.035,
    ) * strength;
}

/// Vertical displacement of a liquid vertex (always below the rest height).
fn wave_offset(p: vec3<f32>, height: f32, time: f32) -> f32 {
    let w = sin(time * 1.6 + p.x * 0.8 + p.z * 0.6) * 0.6 + sin(time * 2.3 - p.x * 0.5 + p.z * 1.1) * 0.4;
    return (w - 1.0) * 0.5 * height;
}

/// Index (0..6) of the dominant axis of a normal: x+, x-, y+, y-, z+, z-.
fn face_index(n: vec3<f32>) -> f32 {
    let a = abs(n);
    if a.x >= a.y && a.x >= a.z {
        return select(1.0, 0.0, n.x >= 0.0);
    }
    if a.y >= a.z {
        return select(3.0, 2.0, n.y >= 0.0);
    }
    return select(5.0, 4.0, n.z >= 0.0);
}

@vertex
fn vertex(vertex_in: Vertex) -> VertexOutput {
    let vertex = decompress_vertex(vertex_in, vertex_in.instance_index);
    let world_from_local = mesh_functions::get_world_from_local(vertex_in.instance_index);
    var world_position = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(vertex.position, 1.0));
    let time = globals.time;
    if shading.wind > 0.0001 {
        world_position = vec4<f32>(world_position.xyz + wind_offset(world_position.xyz, shading.wind, time), world_position.w);
    }
    if shading.wave > 0.0001 {
        world_position.y = world_position.y + wave_offset(world_position.xyz, shading.wave, time);
    }

    var out: VertexOutput;
    out.world_position = world_position;
    out.position = position_world_to_clip(world_position.xyz);
    out.world_normal = mesh_functions::mesh_normal_local_to_world(vertex.normal, vertex_in.instance_index);
    // Mesh-local position (block space) and the face of the vertex.
@if(VERTEX_UVS_A)
    out.uv = vertex.position.xy;
@if(VERTEX_UVS_B)
    out.uv_b = vec2<f32>(vertex.position.z, face_index(vertex.normal) + 8.0 * round(vertex.uv_b.x));
@if(VERTEX_TANGENTS)
    out.world_tangent = mesh_functions::mesh_tangent_local_to_world(world_from_local, vertex.tangent, vertex_in.instance_index);
@if(VERTEX_COLORS)
    out.color = vertex.color;
@if(VERTEX_OUTPUT_INSTANCE_INDEX)
    out.instance_index = vertex_in.instance_index;
@if(VISIBILITY_RANGE_DITHER)
    out.visibility_range_dither = mesh_functions::get_visibility_range_dither_level(vertex_in.instance_index, world_from_local[3]);
    return out;
}

/// Quarter turns of `p` about +Y (as `Quat::from_rotation_y`).
fn turn_y(p: vec3<f32>, quarters: i32) -> vec3<f32> {
    var q = p;
    for (var i = 0; i < quarters; i = i + 1) {
        q = vec3<f32>(q.z, q.y, -q.x);
    }
    return q;
}

/// Inverse of the voxel orientation `index` (facing * 4 + roll, see
/// `Orientation::rotation` in shapes.rs) applied to `p`: from the cell to the
/// block's own frame.
fn unorient(p: vec3<f32>, index: i32) -> vec3<f32> {
    let facing = index / 4;
    let roll = index % 4;
    var q = p;
    if facing == 1 {
        q = vec3<f32>(q.x, -q.y, -q.z);
    } else if facing == 2 {
        q = vec3<f32>(-q.y, q.x, q.z);
    } else if facing == 3 {
        q = vec3<f32>(q.y, -q.x, q.z);
    } else if facing == 4 {
        q = vec3<f32>(q.x, q.z, -q.y);
    } else if facing == 5 {
        q = vec3<f32>(q.x, -q.z, q.y);
    }
    return turn_y(q, (4 - roll) % 4);
}

/// Position inside the voxel (0..1 on each axis), attached to the block and
/// turned with its orientation.
fn block_space(in: VertexOutput) -> vec3<f32> {
    var local = in.world_position.xyz;
    var face = 2.0;
    var orientation = 0.0;
@if(VERTEX_UVS_A && VERTEX_UVS_B) {
    local = vec3<f32>(in.uv.x, in.uv.y, in.uv_b.x);
    orientation = floor((in.uv_b.y + 0.5) / 8.0);
    face = in.uv_b.y - orientation * 8.0;
}
    let index = i32(round(face));
    var normal = vec3<f32>(0.0);
    let sign = select(1.0, -1.0, (index % 2) == 1);
    if index < 2 {
        normal.x = sign;
    } else if index < 4 {
        normal.y = sign;
    } else {
        normal.z = sign;
    }
    // Nudge into the block the face belongs to.
    let cell = floor(local - normal * 0.01);
    var rotation = i32(round(orientation));
    if rotation == 0 {
        // Generated terrain: a random direction per cell.
        rotation = i32(floor(hash1(cell + vec3<f32>(0.37, 11.9, 5.3)) * 23.999));
    }
    return unorient(local - cell - vec3<f32>(0.5), rotation) + vec3<f32>(0.5);
}

/// Low-poly facet normal: the Voronoi cell under the fragment picks a random
/// tilt, projected on the surface plane, so flat faces read as chiselled
/// facets. Cell borders blend back to the geometric normal over `bevel`.
fn facet_normal(normal: vec3<f32>, block: vec3<f32>) -> vec3<f32> {
    let seed = vec3<f32>(17.31, 5.13, 11.71) * shading.normal_seed;
    let sample = voronoi_sample(block * shading.normal_cells + seed);
    let random = hash3(sample.feature + vec3<f32>(41.7, 13.3, 97.1)) * 2.0 - vec3<f32>(1.0);
    let tangent = random - normal * dot(random, normal);
    let tilted = normalize(normal + tangent * shading.normal_strength);
    let edge = smoothstep(0.0, max(shading.normal_bevel, 0.0001) * 0.5, sample.second - sample.first);
    let blend = select(edge, 1.0, shading.normal_bevel <= 0.0);
    return normalize(mix(normal, tilted, blend));
}

/// Quantise the luminance of a lit colour into `bands` soft steps while
/// preserving its hue.
fn toon_band(color: vec3<f32>, bands: f32, softness: f32) -> vec3<f32> {
    let luminance = dot(color, vec3<f32>(0.2126, 0.7152, 0.0722));
    if luminance <= 0.0001 {
        return color;
    }

    let scaled = luminance * bands;
    let lower = floor(scaled) / bands;
    let upper = lower + 1.0 / bands;
    let fraction = fract(scaled);
    let edge = max(clamp(softness, 0.0, 0.999) * 0.5, 0.001);
    let blend = smoothstep(0.5 - edge, 0.5 + edge, fraction);
    let quantised = mix(lower, upper, blend);

    return color * (quantised / luminance);
}

@fragment
fn fragment(
    in: VertexOutput,
    @builtin(front_facing) is_front: bool,
) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    let block = block_space(in);

    if shading.normal_strength > 0.0001 {
        pbr_input.N = facet_normal(normalize(pbr_input.N), block);
    }

    if shading.voronoi_enabled != 0u {
        let sample = voronoi_sample(block * shading.voronoi_scale + vec3<f32>(3.7, 1.3, 5.9));
        // Cell borders are where the two closest feature points are equidistant.
        let border = 1.0 - smoothstep(0.0, 0.09, sample.second - sample.first);
        let tint = mix(
            vec3<f32>(1.0),
            shading.voronoi_color.rgb,
            clamp(shading.voronoi_strength, 0.0, 1.0) * border,
        );
        pbr_input.material.base_color = vec4<f32>(
            pbr_input.material.base_color.rgb * tint,
            pbr_input.material.base_color.a,
        );
    }

    if shading.accent.a > 0.0001 {
        let fleck = voronoi_sample(block * shading.accent_cells + vec3<f32>(9.1, 4.3, 2.7));
        if fleck.cell < shading.accent.a {
            // The cell borders keep the block colour: flecks stay separate.
            let inside = smoothstep(0.0, 0.14, fleck.second - fleck.first);
            pbr_input.material.base_color = vec4<f32>(
                mix(pbr_input.material.base_color.rgb, shading.accent.rgb, inside),
                pbr_input.material.base_color.a,
            );
            pbr_input.material.emissive = vec4<f32>(
                pbr_input.material.emissive.rgb + shading.accent.rgb * shading.accent_glow * inside,
                pbr_input.material.emissive.a,
            );
        }
    }

    pbr_input.material.base_color = alpha_discard(
        pbr_input.material.flags,
        pbr_input.material.alpha_cutoff,
        pbr_input.material.base_color,
    );

    var output_color = apply_pbr_lighting(pbr_input);

    if shading.rim_enabled != 0u {
        let view_direction = normalize(pbr_input.V);
        let facing = clamp(dot(pbr_input.N, view_direction), 0.0, 1.0);
        let rim = pow(1.0 - facing, 3.0);
        output_color = vec4<f32>(
            output_color.rgb + shading.rim_color.rgb * rim * shading.rim_strength,
            output_color.a,
        );
    }

    if shading.toon_enabled != 0u {
        let bands = f32(max(shading.bands, 2u));
        output_color = vec4<f32>(
            toon_band(output_color.rgb, bands, shading.band_softness),
            output_color.a,
        );
    }

    var out: FragmentOutput;
    out.color = main_pass_post_lighting_processing(pbr_input, output_color);
    return out;
}
