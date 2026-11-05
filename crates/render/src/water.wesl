// Water shader: the vertex colour (depth tint and edge fade) with ripples
// from the current frame of the water's animation, tiled in world space, then
// lit like a StandardMaterial.

import bevy_pbr::render::{
    mesh_view_bindings::{globals, view_transmission_texture, view_transmission_sampler},
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
    // The frames are dark, about a quarter grey; in the game's screenshots
    // water is about as bright as its depth colour, so they only add ripples
    // around it. Scaled in gamma terms, as the game's colours are.
    let t = textureSample(frames, frames_sampler, uv, frame);
    let ripples = pow(saturate(pow(t.rgb, vec3(1.0 / 2.2)) * 3.0), vec3(2.2));
    pbr_input.material.base_color *= vec4(ripples, t.a);

    // The game blends gamma-encoded colours, which no alpha in linear light
    // matches for both dark water over pale ground and a pale film over a
    // dark bridge deck. So blend here, over the scene behind the water.
    let a = pbr_input.material.base_color.a;
    pbr_input.material.base_color.a = 1.0;
    // Set only to read the scene behind; it shouldn't dim or refract.
    pbr_input.material.specular_transmission = 0.0;

    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);

    let size = vec2<f32>(textureDimensions(view_transmission_texture));
    let behind = textureSampleLevel(view_transmission_texture, view_transmission_sampler, in.position.xy / size, 0.0);
    let gamma = mix(pow(behind.rgb, vec3(1.0 / 2.2)), pow(max(out.color.rgb, vec3(0.0)), vec3(1.0 / 2.2)), a);
    out.color = vec4(pow(gamma, vec3(2.2)), 1.0);
    return out;
}
