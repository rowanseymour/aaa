// Turns the picture, drawn in stored colour values, into linear light for
// the screen, each channel stopping at 1 as the game's screen does.

import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput;

@group(0) @binding(0) var screen_texture: texture_2d<f32>;
@group(0) @binding(1) var screen_sampler: sampler;

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let c = textureLoad(screen_texture, vec2<i32>(in.position.xy), 0);
    let s = saturate(c.rgb);
    let linear = select(pow((s + 0.055) / 1.055, vec3(2.4)), s / 12.92, s <= vec3(0.04045));
    return vec4(linear, c.a);
}
