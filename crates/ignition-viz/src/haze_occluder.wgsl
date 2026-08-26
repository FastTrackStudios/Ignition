// What the haze camera sees of the room: nothing, and how far away it
// is. Every occluder's twin draws black — the haze camera's picture is
// meant to be the light *in the air* and no light on any surface — and
// writes the haze's transmittance along the ray to that surface into
// alpha, which Bevy's fog pass leaves alone (its alpha blend is
// `Zero, One`). The composite reads it back to dim what the haze sits in
// front of. See `haze.rs`.

#import bevy_pbr::forward_io::VertexOutput
#import bevy_pbr::mesh_view_bindings::view

// x = extinction per metre (density * (absorption + scattering)), yzw spare.
@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> occluder: vec4<f32>;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let distance = length(in.world_position.xyz - view.world_position);
    return vec4<f32>(0.0, 0.0, 0.0, exp(-occluder.x * distance));
}
