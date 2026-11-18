//! Every beam in the air: one hull per beam into the air target, added
//! together.
//!
//! The hull is a 24-sided truncated cone a little larger than the beam and
//! only decides which pixels run. Its back faces are drawn, so a pixel
//! runs once per beam whether the camera is outside the beam or in it.
//! Each pixel then finds the exact part of its view ray inside the cone,
//! cuts it at the first surface in the depth buffer, and integrates the
//! beam's light along it in closed form (`scatter.wesl`).
//!
//! Conditions, set per quality tier (`quality.rs`):
//! `constants::PIECES` (parts the span is cut into), `GOBO_SOFT` (one gobo
//! sample where the ray passes closest to the axis), `GOBO_STEPS` with
//! `constants::GOBO_SAMPLES` (the gobo averaged along the span).
//!
//! Ported in part from ASLS Studio (GPL-3.0),
//! `src/plugins/visualizer/air_beams.js` and `beam_shading.js`: the hull
//! pushed out to contain the circle, the two gobo techniques and the soft
//! knee. See the crate's README.

import package::shaders::scatter::{
    Beam, Cone, FAR, cone_of, cone_span, scatter, profile, closest_to_axis, gobo_uv, screen_noise,
};

/// What every beam shares for one view (`BeamViewUniform` in `render.rs`).
struct BeamView {
    clip_from_world: mat4x4<f32>,
    world_from_clip: mat4x4<f32>,
    /// The air target's size (xy) and the view's (zw), in pixels.
    size: vec4<f32>,
    /// Scattering per metre (x), extinction per metre (y), the phase's
    /// anisotropy (z), the camera's exposure (w).
    medium: vec4<f32>,
    /// How far gobo samples are jittered, 0 to 1 (x); how fast a highlight
    /// pales to white (y); where it starts to (z); dither amplitude (w).
    look: vec4<f32>,
}

@group(0) @binding(0) var<uniform> view: BeamView;
@group(0) @binding(1) var<storage, read> beams: array<Beam>;
@group(0) @binding(2) var depth_texture: texture_depth_2d;
@group(0) @binding(3) var gobo_texture: texture_2d_array<f32>;
@group(0) @binding(4) var gobo_sampler: sampler;

const SEGMENTS: u32 = 24u;
const TAU: f32 = 6.28318530717959;
/// 1 / cos(π / 24): the ring pushed out so the polygon contains the circle.
const RING_SCALE: f32 = 1.00862896;

struct HullVertex {
    @builtin(position) position: vec4<f32>,
    @location(0) @interpolate(flat) instance: u32,
}

/// A point on the hull's ring `segment` (and the next for `segment + 1`),
/// at the lens (`far == false`) or at the far cap.
fn ring_point(beam: Beam, segment: u32, far: bool) -> vec3<f32> {
    let cone = cone_of(beam);
    let angle = TAU * f32(segment % SEGMENTS) / f32(SEGMENTS);
    let radial = beam.right_x_beam.xyz * cos(angle) + beam.up_lens_radius.xyz * sin(angle);
    let along = select(cone.near, cone.far, far);
    return cone.apex + cone.axis * along + radial * (along * cone.tan_edge * RING_SCALE);
}

/// The hull, from nothing but the vertex index: twelve vertices a segment
/// (the side's two triangles, a lens-cap triangle and a far-cap triangle),
/// wound counter-clockwise seen from outside.
@vertex
fn vertex(@builtin(vertex_index) index: u32, @builtin(instance_index) instance: u32) -> HullVertex {
    let beam = beams[instance];
    let cone = cone_of(beam);
    let segment = index / 12u;
    let corner = index % 12u;

    let near_0 = ring_point(beam, segment, false);
    let near_1 = ring_point(beam, segment + 1u, false);
    let far_0 = ring_point(beam, segment, true);
    let far_1 = ring_point(beam, segment + 1u, true);
    let near_centre = cone.apex + cone.axis * cone.near;
    let far_centre = cone.apex + cone.axis * cone.far;

    var world: vec3<f32>;
    switch corner {
        case 0u: { world = near_0; }
        case 1u: { world = near_1; }
        case 2u: { world = far_0; }
        case 3u: { world = near_1; }
        case 4u: { world = far_1; }
        case 5u: { world = far_0; }
        case 6u: { world = far_centre; }
        case 7u: { world = far_0; }
        case 8u: { world = far_1; }
        case 9u: { world = near_centre; }
        case 10u: { world = near_1; }
        default: { world = near_0; }
    }
    return HullVertex(view.clip_from_world * vec4(world, 1.0), instance);
}

/// The world point at a place on screen and a depth-buffer value.
fn unproject(ndc: vec2<f32>, depth: f32) -> vec3<f32> {
    let world = view.world_from_clip * vec4(ndc, depth, 1.0);
    return world.xyz / world.w;
}

/// One gobo texel, `blur` mip levels down, at a point in the beam.
fn gobo_at(beam: Beam, cone: Cone, from_apex: vec3<f32>, blur: f32) -> f32 {
    let uv = gobo_uv(beam, cone, from_apex);
    return textureSampleLevel(gobo_texture, gobo_sampler, uv, i32(beam.gobo_knee.x), blur).r;
}

/// The gobo where the ray passes closest to the beam's axis, softened:
/// the shaft carries the gobo's shape without ever being cut to black.
@if(GOBO_SOFT)
fn gobo_modulation(beam: Beam, cone: Cone, origin: vec3<f32>, dir: vec3<f32>, span: vec2<f32>, pixel: vec2<f32>) -> f32 {
    let t = closest_to_axis(cone, origin, dir, span);
    return 0.4 + 0.6 * gobo_at(beam, cone, origin + dir * t - cone.apex, beam.gobo_knee.z + 2.5);
}

/// The gobo averaged along the span, weighted as the light itself is
/// (profile over distance squared).
///
/// Each sample stands for its own stretch of the ray: its mip level is
/// chosen from how far the gobo moves across that stretch, so it is an
/// average of the stretch and not a point in it. The jitter only moves the
/// sample inside its stretch. The result is a ratio of two sums with the
/// same weights, so it can only scale the closed-form light between zero
/// and one; with no gobo it is exactly one, and the beam cannot be noisy.
@if(GOBO_STEPS)
fn gobo_modulation(beam: Beam, cone: Cone, origin: vec3<f32>, dir: vec3<f32>, span: vec2<f32>, pixel: vec2<f32>) -> f32 {
    let from_apex = origin - cone.apex;
    let step = (span.y - span.x) / f32(constants::GOBO_SAMPLES);
    let texels = 0.5 * f32(textureDimensions(gobo_texture).x);
    let offset = mix(0.5, screen_noise(pixel), view.look.x);

    var sum = 0.0;
    var total = 0.0;
    var uv_from = gobo_uv(beam, cone, from_apex + dir * span.x);
    for (var i = 0u; i < constants::GOBO_SAMPLES; i++) {
        let uv_to = gobo_uv(beam, cone, from_apex + dir * (span.x + step * f32(i + 1u)));
        let at = from_apex + dir * (span.x + step * (f32(i) + offset));
        let along = max(dot(at, cone.axis), 1.0e-4);
        let radial = sqrt(max(dot(at, at) - along * along, 0.0));
        let weight = profile(beam, radial / (along * cone.tan_edge)) / max(dot(at, at), 1.0e-6);
        let footprint = log2(max(length(uv_to - uv_from) * texels, 1.0));
        sum += weight * gobo_at(beam, cone, at, beam.gobo_knee.z + footprint);
        total += weight;
        uv_from = uv_to;
    }
    if total <= 0.0 {
        return 1.0;
    }
    return sum / total;
}

@fragment
fn fragment(in: HullVertex) -> @location(0) vec4<f32> {
    let beam = beams[in.instance];
    let cone = cone_of(beam);

    // The view ray through this pixel, from the near plane. Taken from the
    // pixel and not from the hull, and from two depths so an orthographic
    // camera works the same.
    let uv = in.position.xy / view.size.xy;
    let ndc = vec2(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0);
    let origin = unproject(ndc, 1.0);
    let dir = normalize(unproject(ndc, 0.5) - origin);

    // The first surface: reverse depth, 0 where there is none.
    let depth = textureLoad(depth_texture, vec2<i32>(uv * view.size.zw), 0);
    var t_max = FAR;
    if depth > 0.0 {
        t_max = dot(unproject(ndc, depth) - origin, dir);
    }

    let span = cone_span(cone, origin, dir, t_max);
    if span.y <= span.x {
        discard;
    }

    var light = scatter(beam, cone, view.medium.yz, origin, dir, span);
    @if(GOBO_SOFT || GOBO_STEPS)
    if beam.gobo_knee.x >= 0.0 {
        light *= gobo_modulation(beam, cone, origin, dir, span, in.position.xy + f32(in.instance) * 7.31);
    }

    // Candela × scattering per metre × the integral (1/m) is nits; the
    // camera's exposure makes it what the frame holds. The soft knee is
    // per beam, so a view along one beam's axis does not blow out against
    // the beams beside it.
    let air = beam.colour_candela.w * view.medium.x * beam.profile_haze_fade.z * light * view.medium.w;
    return vec4(beam.colour_candela.rgb * (air / (1.0 + air / beam.gobo_knee.w)), 1.0);
}
