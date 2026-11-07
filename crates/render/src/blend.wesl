// Ground shader: blends up to four terrain layers per tile through
// transition masks, then lights the result like a StandardMaterial.
//
// Per vertex (the same on all four corners of a tile):
//   color  terrain type of each layer, lowest first
//   uv_b   mask of layers 1 and 2 (x = m1 + 512 * m2) and of layer 3 (y)
//   uv     position within the tile, v = 0 on the north edge

import bevy_pbr::render::{
    pbr_fragment::pbr_input_from_standard_material,
    forward_io::{VertexOutput, FragmentOutput},
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
};

struct Layers {
    repeats: array<vec4<f32>, 256>,
    edge: vec4<f32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> layers: Layers;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var textures: texture_2d_array<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var textures_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(103) var masks: texture_2d_array<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(104) var masks_sampler: sampler;

fn terrain(kind: u32, ground: vec2<f32>) -> vec3<f32> {
    let uv = vec2(ground.x, -ground.y) / layers.repeats[kind].xy;
    return textureSample(textures, textures_sampler, uv, kind).rgb;
}

fn mask(index: u32, uv: vec2<f32>) -> f32 {
    return textureSample(masks, masks_sampler, uv, index).r;
}

// 1 inside the map, falling to 0 at its edge; matches terrain.rs.
fn edge_light(world: vec2<f32>) -> f32 {
    let size = layers.edge.x;
    let fade = layers.edge.y;
    if fade <= 0.0 {
        return 1.0;
    }
    let d = min(min(world.x, world.y), min(size - world.x, size - world.y));
    return smoothstep(0.0, fade, d);
}

@fragment
fn fragment(
    in: VertexOutput,
    @builtin(front_facing) is_front: bool,
) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);

    // Game space: x east, y north.
    let ground = vec2(in.world_position.x, -in.world_position.z);
    let kinds = vec4<u32>(round(in.color));
    let packed = vec2<u32>(round(in.uv_b));
    let m = vec3<u32>(packed.x % 512u, packed.x / 512u, packed.y);

    var c = terrain(kinds.x, ground);
    c = mix(c, terrain(kinds.y, ground), mask(m.x, in.uv));
    c = mix(c, terrain(kinds.z, ground), mask(m.y, in.uv));
    c = mix(c, terrain(kinds.w, ground), mask(m.z, in.uv));
    pbr_input.material.base_color = vec4(c, 1.0);

    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = vec4(out.color.rgb * edge_light(ground), out.color.a);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
