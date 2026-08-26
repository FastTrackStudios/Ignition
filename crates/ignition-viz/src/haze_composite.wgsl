// Lays the haze camera's picture over the main camera's: the light in
// the air, added; the room behind it, dimmed by the transmittance the
// occluders wrote into alpha. Drawn as the nearest thing in the
// transparent pass, so it lands after every beam and before bloom —
// which is what lets a shaft bloom the way it did when the fog was
// marched at full size. The blend that does the work is set in
// `HazeCompositeMaterial::specialize`: colour = fog + scene * alpha.

#import bevy_pbr::forward_io::VertexOutput
#import bevy_pbr::mesh_view_bindings::view

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var haze_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var haze_sampler: sampler;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let uv = (in.position.xy - view.viewport.xy) / view.viewport.zw;
    return textureSample(haze_texture, haze_sampler, uv);
}
