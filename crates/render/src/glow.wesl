// Additive models: lit like a StandardMaterial plus the extra ambient light
// their material reflects, then scaled by their source factor. The game adds
// gamma-encoded colours, so the scaling is done on those.

import bevy_pbr::render::{
    mesh_view_bindings::{lights, view},
    pbr_fragment::pbr_input_from_standard_material,
    forward_io::{VertexOutput, FragmentOutput},
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
};

struct Glow {
    // Extra ambient gain (rgb) and source factor (w): 0 one, 1 the colour,
    // 2 alpha.
    params: vec4<f32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> glow: Glow;

@fragment
fn fragment(
    in: VertexOutput,
    @builtin(front_facing) is_front: bool,
) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    let base = pbr_input.material.base_color;

    let lit = apply_pbr_lighting(pbr_input).rgb
        + base.rgb * glow.params.rgb * lights.ambient_color.rgb * view.exposure;
    let c = pow(saturate(lit), vec3(1.0 / 2.2));
    var added = c;
    if glow.params.w == 1.0 {
        added = c * c;
    } else if glow.params.w == 2.0 {
        added = c * base.a;
    }

    var out: FragmentOutput;
    // Alpha 1 so the additive blend's premultiply leaves the colour be.
    out.color = main_pass_post_lighting_processing(pbr_input, vec4(pow(added, vec3(2.2)), 1.0));
    return out;
}
