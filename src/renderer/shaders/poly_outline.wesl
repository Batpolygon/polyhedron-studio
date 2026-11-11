//! Polyhedron inverted-hull outline shader.
//!
//! The outline pass uses a copy of the voxel surface mesh on a child entity. The
//! vertex stage pushes every vertex along its joined expansion direction, and the pipeline
//! culls front faces so only the expanded back shell survives. The shell is
//! depth tested against the scene, which keeps the outline hidden behind solid
//! geometry.
//!
//! The world-space offset is scaled by the distance to the camera so the
//! outline keeps a constant screen thickness. Shells of windy blocks
//! (leaves) sway exactly like their surface (`wind_offset` is shared with
//! `poly_toon.wesl`).
//!
//! @version 4

import bevy_pbr::render::{
    forward_io::{decompress_vertex, Vertex},
    mesh_functions,
    mesh_view_bindings::{globals, view},
    view_transformations::position_world_to_clip,
};

struct OutlineSettings {
    color: vec4<f32>,
    width: f32,
    wind: f32,
    _pad1: f32,
    _pad2: f32,
};

@group(constants::MATERIAL_BIND_GROUP) @binding(0)
var<uniform> outline: OutlineSettings;

struct OutlineOutput {
    @builtin(position) clip_position: vec4<f32>,
};

/// Same displacement as the surface (see `poly_toon.wesl`).
fn wind_offset(p: vec3<f32>, strength: f32, time: f32) -> vec3<f32> {
    let sway = sin(time * 1.7 + p.x * 0.45 + p.z * 0.31) + 0.5 * sin(time * 2.9 + p.x * 1.3 - p.y * 0.7 + p.z * 0.9);
    let flutter = sin(time * 6.3 + dot(p, vec3<f32>(3.1, 2.3, 4.7))) * 0.25;
    return vec3<f32>(
        (sway + flutter) * 0.045,
        flutter * 0.015,
        (cos(time * 1.3 + p.z * 0.4 + p.x * 0.2) + flutter) * 0.035,
    ) * strength;
}

@vertex
fn vertex(vertex_in: Vertex) -> OutlineOutput {
    let vertex = decompress_vertex(vertex_in, vertex_in.instance_index);
    let world_from_local = mesh_functions::get_world_from_local(vertex_in.instance_index);

    let local_position = vec4<f32>(vertex.position, 1.0);
    var world_position = mesh_functions::mesh_position_local_to_world(
        world_from_local,
        local_position,
    );
    if outline.wind > 0.0001 {
        world_position = vec4<f32>(world_position.xyz + wind_offset(world_position.xyz, outline.wind, globals.time), world_position.w);
    }
    // The shell mesh stores *expansion directions* in its normal attribute:
    // the sum of the normals of every face meeting at that vertex. They are
    // deliberately not normalised, so the shells of adjacent faces meet at
    // edges and corners instead of splitting apart.
    let expand = (world_from_local * vec4<f32>(vertex.normal, 0.0)).xyz;

    let distance = max(length(view.world_position - world_position.xyz), 0.001);
    let offset = expand * outline.width * distance;
    world_position = vec4<f32>(world_position.xyz + offset, world_position.w);

    var out: OutlineOutput;
    out.clip_position = position_world_to_clip(world_position.xyz);
    return out;
}

@fragment
fn fragment() -> @location(0) vec4<f32> {
    return outline.color;
}
