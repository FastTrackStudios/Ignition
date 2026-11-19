//! Adds the air target to the picture.
//!
//! The beams were summed on their own so the sum can be shaped before it
//! meets the room: where it is brightest it pales to white instead of
//! clipping in the beam's colour, as on camera. Then a dither below one
//! output step, so a beam's faint end fades out instead of banding.
//!
//! Conditions: `HALF_RES` (the air target is half the view's size and is
//! brought up with a depth-aware filter; the governor's last resort),
//! `COMPRESS` (also compress the brightest channel to below 1, for a view
//! with no tonemapper after it).
//!
//! Ported in part from ASLS Studio (GPL-3.0): the roll-off from
//! `beam_shading.js` (`ROLLOFF_GLSL`) and the depth-weighted upsample from
//! `air_beams.js` (`COMPOSITE_FRAGMENT`). See the crate's README.

import package::shaders::scatter::screen_noise;

/// The same uniform the beams read (`beams.wesl`, `BeamView`).
struct BeamView {
    clip_from_world: mat4x4<f32>,
    world_from_clip: mat4x4<f32>,
    size: vec4<f32>,
    medium: vec4<f32>,
    look: vec4<f32>,
    haze_from_world: mat4x4<f32>,
    haze_drift: vec4<f32>,
}

@group(0) @binding(0) var<uniform> view: BeamView;
@group(0) @binding(1) var air_texture: texture_2d<f32>;
@group(0) @binding(2) var depth_texture: texture_depth_2d;

struct Fullscreen {
    @builtin(position) position: vec4<f32>,
}

/// One triangle that covers the screen.
@vertex
fn vertex(@builtin(vertex_index) index: u32) -> Fullscreen {
    let corner = vec2(f32(index >> 1u), f32(index & 1u)) * 2.0;
    return Fullscreen(vec4(corner * vec2(2.0, -2.0) + vec2(-1.0, 1.0), 0.0, 1.0));
}

@if(!HALF_RES)
fn air_at(pixel: vec2<f32>) -> vec3<f32> {
    return textureLoad(air_texture, vec2<i32>(pixel), 0).rgb;
}

/// The four nearest air texels, each weighted by how close its depth is to
/// this pixel's, so a beam does not bleed over an edge in front of it.
@if(HALF_RES)
fn air_at(pixel: vec2<f32>) -> vec3<f32> {
    let here = textureLoad(depth_texture, vec2<i32>(pixel), 0);
    let limit = vec2<i32>(view.size.xy) - 1;
    let base = pixel * (view.size.xy / view.size.zw) - 0.5;
    let corner = floor(base);
    let f = base - corner;
    var sum = vec3(0.0);
    var total = 0.0;
    for (var j = 0; j < 2; j++) {
        for (var i = 0; i < 2; i++) {
            let tap = clamp(vec2<i32>(corner) + vec2(i, j), vec2(0), limit);
            // The depth the air pass read for that texel (`beams.wesl`).
            let centre = (vec2<f32>(tap) + 0.5) * (view.size.zw / view.size.xy);
            let there = textureLoad(depth_texture, vec2<i32>(centre), 0);
            let bilinear = select(1.0 - f.x, f.x, i == 1) * select(1.0 - f.y, f.y, j == 1);
            let weight = bilinear / (0.02 + abs(here - there) / max(max(here, there), 1.0e-9));
            sum += textureLoad(air_texture, tap, 0).rgb * weight;
            total += weight;
        }
    }
    return sum / max(total, 1.0e-6);
}

/// A saturating beam pales where it is brightest. The hue is kept and the
/// brightest channel is unchanged; what is past `view.look.z` bleeds
/// towards white at the rate `view.look.y`.
fn roll_off(colour: vec3<f32>) -> vec3<f32> {
    let peak = max(max(colour.r, colour.g), colour.b);
    if peak <= 0.0 {
        return vec3(0.0);
    }
    let bleed = 1.0 - exp(-max(peak - view.look.z, 0.0) * view.look.y);
    @if(COMPRESS)
    let mapped = 1.0 - exp(-peak);
    @if(!COMPRESS)
    let mapped = peak;
    return mix(colour * (mapped / peak), vec3(mapped), bleed);
}

@fragment
fn fragment(in: Fullscreen) -> @location(0) vec4<f32> {
    let colour = roll_off(air_at(in.position.xy));
    // Dithered where a display steps evenly (about a square root), by less
    // than one of its steps.
    let noise = (screen_noise(in.position.xy) - 0.5) * view.look.w;
    let stepped = max(sqrt(colour) + noise, vec3(0.0));
    return vec4(stepped * stepped, 0.0);
}
