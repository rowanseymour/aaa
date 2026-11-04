// Water shader: the vertex colour (depth tint and edge fade) times the
// current frame of the water's animation, tiled in world space, then lit like
// a StandardMaterial.

import bevy_pbr::render::{
    mesh_view_bindings::globals,
    pbr_fragment::pbr_input_from_standard_material,
    forward_io::{VertexOutput, FragmentOutput},
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
};

struct Animation {
    // Texture repeat in world units (xy), frames per second (z) and the
    // number of frames (w).
    params: vec4<f32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> animation: Animation;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var frames: texture_2d_array<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var frames_sampler: sampler;

@fragment
fn fragment(
    in: VertexOutput,
    @builtin(front_facing) is_front: bool,
) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);

    // Game space x east, y north; unlike the ground's, v runs north.
    let uv = vec2(in.world_position.x, -in.world_position.z) / animation.params.xy;
    let count = u32(animation.params.w);
    let frame = u32(globals.time * animation.params.z) % count;
    pbr_input.material.base_color *= textureSample(frames, frames_sampler, uv, frame);

    // The game blends gamma-encoded colours, which covers what's below far
    // more than the same alpha does in linear light. Over a background, a
    // dark surface of alpha a leaves (1 - a) of it in gamma terms, so
    // (1 - a)^2.2 of it in linear ones.
    let a = pbr_input.material.base_color.a;
    pbr_input.material.base_color.a = 1.0 - pow(1.0 - a, 2.2);

    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
