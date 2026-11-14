// Every particle system's squares, in one mesh. Each square brings its
// system's look: its layer of the texture array and how it blends and is
// lit (uv_b), and its emissive colour and alpha cutoff (the tangent). The
// vertex stage passes them through as they are.

import bevy_pbr::render::{
    mesh_functions,
    forward_io::{Vertex, VertexOutput, FragmentOutput, decompress_vertex},
    view_transformations::position_world_to_clip,
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
};

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var textures: texture_2d_array<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var textures_sampler: sampler;

// How a square blends, in the low bits of its flags, and whether it's lit.
const OVER: u32 = 0u;
const ADD: u32 = 1u;
const OPAQUE: u32 = 2u;
const LIT: u32 = 4u;

@vertex
fn vertex(vertex_in: Vertex) -> VertexOutput {
    let vertex = decompress_vertex(vertex_in, vertex_in.instance_index);
    let world_from_local = mesh_functions::get_world_from_local(vertex_in.instance_index);
    var out: VertexOutput;
    out.world_position = mesh_functions::mesh_position_local_to_world(
        world_from_local,
        vec4<f32>(vertex.position, 1.0),
    );
    out.position = position_world_to_clip(out.world_position.xyz);
    out.world_normal = vertex.normal;
    out.uv = vertex.uv;
    out.uv_b = vertex.uv_b;
    out.world_tangent = vertex.tangent;
    out.color = vertex.color;
    @if(VERTEX_OUTPUT_INSTANCE_INDEX)
    out.instance_index = vertex_in.instance_index;
    return out;
}

@fragment
fn fragment(
    in: VertexOutput,
    @builtin(front_facing) is_front: bool,
) -> FragmentOutput {
    let layer = u32(round(in.uv_b.x));
    let flags = u32(round(in.uv_b.y));
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    pbr_input.material.base_color *= textureSample(textures, textures_sampler, in.uv, layer);
    if pbr_input.material.base_color.a < in.world_tangent.w {
        discard;
    }
    var colour = pbr_input.material.base_color;
    if (flags & LIT) != 0u {
        // Alpha 0: the model's own colour, not scaled by the exposure.
        pbr_input.material.emissive = vec4(in.world_tangent.rgb, 0.0);
        colour = apply_pbr_lighting(pbr_input);
    }
    colour = main_pass_post_lighting_processing(pbr_input, colour);

    // The material blends premultiplied colours, which covers all three.
    var out: FragmentOutput;
    switch flags & 3u {
        case ADD: {
            out.color = vec4(colour.rgb * colour.a, 0.0);
        }
        case OPAQUE: {
            out.color = vec4(colour.rgb, 1.0);
        }
        default: {
            out.color = vec4(colour.rgb * colour.a, colour.a);
        }
    }
    return out;
}
