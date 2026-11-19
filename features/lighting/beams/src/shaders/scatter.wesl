//! The beam's mathematics: where a view ray is inside a beam's cone, and
//! how much of the beam's light the haze scatters along that part.
//!
//! No bindings and no entry points, so it can be imported anywhere and
//! tested on its own. `src/math.rs` is the same arithmetic on the CPU, in
//! the same order, with tests; change one and change the other.
//!
//! Ported in part from ASLS Studio (GPL-3.0),
//! `src/plugins/visualizer/air_beams.js`: the cone cut at the lens with
//! its apex behind it, the arctangent integral of the inverse square over
//! equal pieces of the segment, the Henyey-Greenstein phase, the end fade
//! and the closest-approach point. The crate's README lists what was
//! ported and what was changed.

/// Stands in for "no end"; `math.rs` has the same number.
const FAR: f32 = 1.0e30;
const PI: f32 = 3.14159265358979;
const LN_2: f32 = 0.69314718;

/// One beam, as the storage buffer holds it (`GpuBeam` in `beam.rs`).
/// Every member is a `vec4`, so there is no padding to get wrong.
struct Beam {
    /// The lens's centre (xyz); how far the beam reaches from it (w).
    lens_range: vec4<f32>,
    /// Unit vector the beam shines along (xyz); tangent of the half angle
    /// at which its light reaches zero (w).
    axis_tan_edge: vec4<f32>,
    /// The gobo's horizontal (xyz); where the profile is one half (w).
    right_x_beam: vec4<f32>,
    /// The gobo's vertical (xyz); the lens's radius (w).
    up_lens_radius: vec4<f32>,
    /// Linear colour (rgb); on-axis candela, dimmer and strobe folded in (w).
    colour_candela: vec4<f32>,
    /// The profile's exponent (x), where its window to zero starts (y),
    /// how strongly this beam shows in the haze (z), the fraction of its
    /// range at which it starts fading out (w).
    profile_haze_fade: vec4<f32>,
    /// Gobo layer, negative for none (x); its rotation in radians (y); its
    /// blur as a mip level, from focus and frost (z); the soft knee (w).
    gobo_knee: vec4<f32>,
    /// Where this beam's occluders start in their buffer (x) and how many
    /// it has (y).
    occluders: vec4<f32>,
}

/// The most occluders one beam is cut by; `math.rs` has the same number.
const MAX_OCCLUDERS: u32 = 6u;
/// The most pieces a span is cut into: the size of a density array.
const MAX_PIECES: u32 = 16u;

/// A beam's cone, cut at the lens and at its far end.
struct Cone {
    /// The virtual apex, behind the lens.
    apex: vec3<f32>,
    axis: vec3<f32>,
    tan_edge: f32,
    /// Distance from the apex to the lens plane, along the axis.
    near: f32,
    /// Distance from the apex to the far cap, along the axis.
    far: f32,
}

fn cone_of(beam: Beam) -> Cone {
    let axis = beam.axis_tan_edge.xyz;
    let tan_edge = beam.axis_tan_edge.w;
    let near = beam.up_lens_radius.w / tan_edge;
    return Cone(beam.lens_range.xyz - axis * near, axis, tan_edge, near, near + beam.lens_range.w);
}

/// What two spans (x: enter, y: exit) share; empty when `y <= x`.
fn span_intersect(a: vec2<f32>, b: vec2<f32>) -> vec2<f32> {
    return vec2(max(a.x, b.x), min(a.y, b.y));
}

/// Where a ray whose height along the axis is `along + t * rate` lies
/// between the lens plane and the far cap.
fn slab_span(along: f32, rate: f32, near: f32, far: f32) -> vec2<f32> {
    if abs(rate) < 1.0e-7 {
        if along > near && along < far {
            return vec2(-FAR, FAR);
        }
        return vec2(0.0, -1.0);
    }
    let a = (near - along) / rate;
    let b = (far - along) / rate;
    return vec2(min(a, b), max(a, b));
}

/// Where the ray is inside the cone's quadric, on the side the beam shines
/// to. `q(t) = |radial(t)|² − tan² · along(t)²` is negative inside.
fn quadric_span(cone: Cone, from_apex: vec3<f32>, dir: vec3<f32>) -> vec2<f32> {
    let along = dot(from_apex, cone.axis);
    let rate = dot(dir, cone.axis);
    let radial = from_apex - cone.axis * along;
    let radial_rate = dir - cone.axis * rate;
    let tan2 = cone.tan_edge * cone.tan_edge;

    let a = dot(radial_rate, radial_rate) - tan2 * rate * rate;
    let half_b = dot(radial, radial_rate) - tan2 * along * rate;
    let c = dot(radial, radial) - tan2 * along * along;

    if abs(a) < 1.0e-9 {
        // Parallel to the cone's surface: one crossing.
        if abs(half_b) < 1.0e-12 {
            if c < 0.0 {
                return vec2(-FAR, FAR);
            }
            return vec2(0.0, -1.0);
        }
        let root = -c / (2.0 * half_b);
        if half_b < 0.0 {
            return vec2(root, FAR);
        }
        return vec2(-FAR, root);
    }
    let disc = half_b * half_b - a * c;
    if disc < 0.0 && a > 0.0 {
        // A ray steeper than the cone, missing it.
        return vec2(0.0, -1.0);
    }
    // A ray shallower than the cone cannot miss it: there a negative
    // discriminant is rounding (a ray along the axis has exactly zero).
    // The stable form of the roots: no subtraction of nearly equal numbers.
    var signed_root = sqrt(max(disc, 0.0));
    if half_b < 0.0 {
        signed_root = -signed_root;
    }
    let q = -(half_b + signed_root);
    var r0 = 0.0;
    var r1 = 0.0;
    if abs(q) >= 1.0e-30 {
        r0 = q / a;
        r1 = c / q;
    }
    let lo = min(r0, r1);
    let hi = max(r0, r1);
    if a > 0.0 {
        return vec2(lo, hi);
    }
    // Shallower than the cone: inside before the first root and after the
    // second, one in each nappe. The beam's is the one the ray climbs into.
    if rate > 0.0 {
        return vec2(hi, FAR);
    }
    return vec2(-FAR, lo);
}

/// The part of the ray `origin + t * dir` inside `cone`, no further than
/// `t_max` (the first surface). Empty when `y <= x`.
///
/// Exact: the hull that was rasterised to get here never bounds it, or
/// brightness would step along every facet.
fn cone_span(cone: Cone, origin: vec3<f32>, dir: vec3<f32>, t_max: f32) -> vec2<f32> {
    let from_apex = origin - cone.apex;
    let slab = slab_span(dot(from_apex, cone.axis), dot(dir, cone.axis), cone.near, cone.far);
    let inside = span_intersect(slab, quadric_span(cone, from_apex, dir));
    return span_intersect(vec2(0.0, t_max), inside);
}

/// The shadow a sphere (centre xyz, radius w) casts in light from `apex`:
/// itself a cone, from the sphere's centre on. Empty when the apex is in
/// the sphere.
fn shadow_cone(apex: vec3<f32>, sphere: vec4<f32>) -> Cone {
    let to_centre = sphere.xyz - apex;
    let distance = length(to_centre);
    if distance <= sphere.w * 1.001 {
        return Cone(apex, vec3(0.0, 0.0, 1.0), 0.0, FAR, FAR);
    }
    let reach = sqrt(distance * distance - sphere.w * sphere.w);
    return Cone(apex, to_centre / distance, sphere.w / reach, distance, FAR);
}

/// What is left of a span outside its occluders' shadows.
struct LitSpans {
    spans: array<vec2<f32>, 7>,
    count: u32,
}

/// The parts of `span` of the ray that no sphere shadows from `apex`, in
/// order along the ray. The shadows are sorted by where they start and
/// their union is walked once.
fn lit_spans(apex: vec3<f32>, origin: vec3<f32>, dir: vec3<f32>, span: vec2<f32>, spheres: array<vec4<f32>, 6>, count: u32) -> LitSpans {
    var balls = spheres;
    var shadows: array<vec2<f32>, 6>;
    var shadowed = 0u;
    for (var i = 0u; i < min(count, MAX_OCCLUDERS); i++) {
        let shadow = span_intersect(span, cone_span(shadow_cone(apex, balls[i]), origin, dir, FAR));
        if shadow.y <= shadow.x {
            continue;
        }
        // Insertion, by where the shadow starts.
        var at = shadowed;
        while at > 0u && shadows[at - 1u].x > shadow.x {
            shadows[at] = shadows[at - 1u];
            at -= 1u;
        }
        shadows[at] = shadow;
        shadowed += 1u;
    }
    var lit: LitSpans;
    lit.count = 0u;
    var cursor = span.x;
    for (var i = 0u; i < shadowed; i++) {
        if shadows[i].x > cursor {
            lit.spans[lit.count] = vec2(cursor, shadows[i].x);
            lit.count += 1u;
        }
        cursor = max(cursor, shadows[i].y);
    }
    if span.y > cursor {
        lit.spans[lit.count] = vec2(cursor, span.y);
        lit.count += 1u;
    }
    return lit;
}

/// Hermite step from 0 at `lo` to 1 at `hi`: WGSL's `smoothstep`, written
/// out so the CPU evaluator the tests run this file with has it too.
fn smooth_step(lo: f32, hi: f32, x: f32) -> f32 {
    let t = clamp((x - lo) / (hi - lo), 0.0, 1.0);
    return t * t * (3.0 - 2.0 * t);
}

/// The beam's brightness across its width: 1 on the axis, one half at the
/// beam angle, one tenth at the field angle, nothing from the edge out.
/// `x` is the tangent of the angle off the axis over the edge's.
fn profile(beam: Beam, x: f32) -> f32 {
    let core = exp(-LN_2 * pow(x / beam.right_x_beam.w, beam.profile_haze_fade.x));
    return core * (1.0 - smooth_step(beam.profile_haze_fade.y, 1.0, x));
}

/// Henyey-Greenstein, per steradian. `cos_theta` is 1 for light scattered
/// straight on (looking into the beam).
fn phase(g: f32, cos_theta: f32) -> f32 {
    let g2 = g * g;
    return (1.0 - g2) / (4.0 * PI * pow(1.0 + g2 - 2.0 * g * cos_theta, 1.5));
}

/// Everything but the inverse square at a point `from_apex`, seen along
/// `dir` from `t` metres away: profile, phase, extinction and end fade.
/// `medium` is extinction per metre (x) and the phase's anisotropy (y).
fn weight(beam: Beam, cone: Cone, medium: vec2<f32>, from_apex: vec3<f32>, dir: vec3<f32>, t: f32) -> f32 {
    let along = max(dot(from_apex, cone.axis), 1.0e-4);
    let dist = max(length(from_apex), 1.0e-4);
    let radial = sqrt(max(dot(from_apex, from_apex) - along * along, 0.0));
    let x = radial / (along * cone.tan_edge);
    let cos_theta = dot(from_apex, -dir) / dist;
    let path = max(t, 0.0) + max(dist - cone.near, 0.0);
    let depth = (along - cone.near) / (cone.far - cone.near);
    return profile(beam, x)
        * phase(medium.y, cos_theta)
        * exp(-medium.x * path)
        * (1.0 - smooth_step(beam.profile_haze_fade.w, 1.0, depth));
}

/// The light a beam of unit intensity scatters towards the eye along
/// `span` of the ray, per unit of scattering coefficient, in 1/m.
///
/// The inverse square from the apex is integrated exactly on each of
/// `PIECES` equal parts of the span; the other terms, which are smooth,
/// are taken at each part's middle. There is no step in space, no noise
/// and no history: the result is continuous in everything it is given.
///
/// `density` is the haze at each part's middle as a multiple of the room's
/// (all ones for even haze): the caller samples it, so this stays free of
/// bindings.
fn scatter(beam: Beam, cone: Cone, medium: vec2<f32>, origin: vec3<f32>, dir: vec3<f32>, span: vec2<f32>, density: array<f32, 16>) -> f32 {
    var haze = density;
    let from_apex = origin - cone.apex;
    let foot = dot(from_apex, dir);
    let offset = from_apex - dir * foot;
    let h2 = max(dot(offset, offset), 1.0e-8);
    let h = sqrt(h2);
    let step = (span.y - span.x) / f32(constants::PIECES);

    var sum = 0.0;
    for (var piece = 0u; piece < constants::PIECES; piece++) {
        let u0 = span.x + foot + step * f32(piece);
        let u1 = u0 + step;
        // atan(u1/h) − atan(u0/h) as one arctangent, exact as h → 0.
        let inverse_square = atan2(h * step, h2 + u0 * u1) / h;
        let t = span.x + step * (f32(piece) + 0.5);
        sum += inverse_square * haze[piece] * weight(beam, cone, medium, from_apex + dir * t, dir, t);
    }
    return sum;
}

/// Where the ray passes closest to the beam's axis, kept inside `span`.
fn closest_to_axis(cone: Cone, origin: vec3<f32>, dir: vec3<f32>, span: vec2<f32>) -> f32 {
    let from_apex = origin - cone.apex;
    let along_rate = dot(dir, cone.axis);
    let denom = 1.0 - along_rate * along_rate;
    var t = 0.5 * (span.x + span.y);
    if denom > 1.0e-4 {
        t = (along_rate * dot(cone.axis, from_apex) - dot(from_apex, dir)) / denom;
    }
    return clamp(t, span.x, span.y);
}

/// A point's place in the beam's gobo: the unit disc is the field angle.
/// `rotation` turns the gobo about the axis.
fn gobo_uv(beam: Beam, cone: Cone, from_apex: vec3<f32>) -> vec2<f32> {
    let along = max(dot(from_apex, cone.axis), 1.0e-4);
    let scale = 1.0 / (along * cone.tan_edge * beam.profile_haze_fade.y);
    let disc = vec2(dot(from_apex, beam.right_x_beam.xyz), dot(from_apex, beam.up_lens_radius.xyz)) * scale;
    let c = cos(beam.gobo_knee.y);
    let s = sin(beam.gobo_knee.y);
    return vec2(c * disc.x - s * disc.y, s * disc.x + c * disc.y) * 0.5 + 0.5;
}

/// Interleaved gradient noise in [0, 1): a fixed screen pattern, the same
/// every frame. Only ever used below one output step, or to place gobo
/// samples; never to place a sample of the beam itself.
fn screen_noise(pixel: vec2<f32>) -> f32 {
    return fract(52.9829189 * fract(dot(pixel, vec2(0.06711056, 0.00583715))));
}
