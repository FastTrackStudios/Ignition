//! The beam's mathematics, on the CPU.
//!
//! `shaders/scatter.wesl` is the copy the GPU runs; this is the same
//! arithmetic in the same order, so it can be tested without a GPU and so
//! the per-beam numbers the shader is handed (the profile's exponent, the
//! peak intensity) come from one place. Change one and change the other.
//!
//! Everything here is honest: values in, values out.

use bevy::math::Vec3;

/// How far past the field angle the beam's light reaches zero, as a ratio
/// of tangents. The cone hull and the profile's support both end here.
pub const EDGE_OVER_FIELD: f32 = 1.15;

/// Stands in for "no end" in a [`Span`]; the shader uses the same number.
pub const FAR: f32 = 1.0e30;

/// The widest half angle the edge of a beam may have, in radians (85°).
const MAX_EDGE_HALF_ANGLE: f32 = 1.483_529_8;

/// The narrowest half angle a beam may have, in radians (0.05°).
const MIN_HALF_ANGLE: f32 = 8.726_646e-4;

/// The steepest and shallowest profile exponents allowed.
const EXPONENT_RANGE: (f32, f32) = (1.0, 24.0);

/// A beam's cone, cut at the lens and at its far end.
///
/// The light is a point source at the *virtual apex*, `near` metres behind
/// the lens along the axis, so the cone is exactly as wide as the lens
/// where it leaves it and the illuminance there is finite (the flux over
/// the lens's area).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cone {
    /// The virtual apex, behind the lens.
    pub apex: Vec3,
    /// Unit vector from the apex through the lens.
    pub axis: Vec3,
    /// Tangent of the half angle at which the light reaches zero.
    pub tan_edge: f32,
    /// Distance from the apex to the lens plane, along the axis.
    pub near: f32,
    /// Distance from the apex to the far cap, along the axis.
    pub far: f32,
}

impl Cone {
    /// The cone of a lens at `lens`, `lens_radius` wide, shining along
    /// `axis` for `range` metres with its light ending at `tan_edge`.
    #[must_use]
    pub fn from_lens(lens: Vec3, axis: Vec3, lens_radius: f32, tan_edge: f32, range: f32) -> Self {
        let near = lens_radius / tan_edge;
        Self {
            apex: lens - axis * near,
            axis,
            tan_edge,
            near,
            far: near + range,
        }
    }
}

/// A view ray: `origin + t * dir`, `dir` a unit vector.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ray {
    pub origin: Vec3,
    pub dir: Vec3,
}

/// The part of a ray inside something: `enter < exit`, both distances
/// along the ray.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Span {
    pub enter: f32,
    pub exit: f32,
}

impl Span {
    /// The span from `enter` to `exit`, or `None` when it is empty.
    #[must_use]
    pub fn new(enter: f32, exit: f32) -> Option<Self> {
        (exit > enter).then_some(Self { enter, exit })
    }

    /// What two spans share.
    #[must_use]
    pub fn intersect(self, other: Self) -> Option<Self> {
        Self::new(self.enter.max(other.enter), self.exit.min(other.exit))
    }

    #[must_use]
    pub fn len(self) -> f32 {
        self.exit - self.enter
    }
}

/// The part of `ray` inside `cone`, no further than `t_max` along it (the
/// first surface the ray meets).
///
/// The cone is a quadric, so the answer is exact: no hull face bounds it.
/// It is built from intervals rather than from classifying crossings, so
/// there is no epsilon to step over a surface with: the ray's part between
/// the lens plane and the far cap, cut by where the quadric is negative.
#[must_use]
pub fn cone_segment(cone: &Cone, ray: &Ray, t_max: f32) -> Option<Span> {
    let from_apex = ray.origin - cone.apex;
    let along = from_apex.dot(cone.axis);
    let rate = ray.dir.dot(cone.axis);
    Span::new(0.0, t_max)?
        .intersect(slab_span(along, rate, cone.near, cone.far)?)?
        .intersect(quadric_span(cone, from_apex, ray.dir)?)
}

/// Where a ray whose height along the axis is `along + t * rate` lies
/// between the heights `near` and `far`.
fn slab_span(along: f32, rate: f32, near: f32, far: f32) -> Option<Span> {
    if rate.abs() < 1.0e-7 {
        return (along > near && along < far).then_some(Span {
            enter: -FAR,
            exit: FAR,
        });
    }
    let (a, b) = ((near - along) / rate, (far - along) / rate);
    Span::new(a.min(b), a.max(b))
}

/// Where the ray is inside the double cone's quadric, on the side the
/// beam shines to.
///
/// `q(t) = |radial(t)|² − tan² · along(t)²` is negative inside. With the
/// radial parts taken explicitly (not as `|p|² − along²`), a narrow beam
/// seen from far away keeps its precision in single floats.
fn quadric_span(cone: &Cone, from_apex: Vec3, dir: Vec3) -> Option<Span> {
    let along = from_apex.dot(cone.axis);
    let rate = dir.dot(cone.axis);
    let radial = from_apex - cone.axis * along;
    let radial_rate = dir - cone.axis * rate;
    let tan2 = cone.tan_edge * cone.tan_edge;

    let a = radial_rate.dot(radial_rate) - tan2 * rate * rate;
    let half_b = radial.dot(radial_rate) - tan2 * along * rate;
    let c = radial.dot(radial) - tan2 * along * along;

    if a.abs() < 1.0e-9 {
        return linear_span(half_b, c);
    }
    let disc = half_b * half_b - a * c;
    if disc < 0.0 {
        // A ray steeper than the cone that misses it; a ray shallower than
        // the cone cannot miss it, so `a < 0` never reaches here.
        return None;
    }
    // The stable form of the roots: no subtraction of nearly equal numbers.
    let q = -(half_b + disc.sqrt().copysign(half_b));
    let (r0, r1) = if q.abs() < 1.0e-30 {
        (0.0, 0.0)
    } else {
        (q / a, c / q)
    };
    let (lo, hi) = (r0.min(r1), r0.max(r1));
    if a > 0.0 {
        return Span::new(lo, hi);
    }
    // Shallower than the cone: inside before the first root and after the
    // second, one in each nappe. The beam's is the one the ray climbs into.
    if rate > 0.0 {
        Some(Span {
            enter: hi,
            exit: FAR,
        })
    } else {
        Some(Span {
            enter: -FAR,
            exit: lo,
        })
    }
}

/// Where `2 · half_b · t + c` is negative: a ray parallel to the cone's
/// surface crosses it once.
fn linear_span(half_b: f32, c: f32) -> Option<Span> {
    if half_b.abs() < 1.0e-12 {
        return (c < 0.0).then_some(Span {
            enter: -FAR,
            exit: FAR,
        });
    }
    let root = -c / (2.0 * half_b);
    if half_b < 0.0 {
        Some(Span {
            enter: root,
            exit: FAR,
        })
    } else {
        Some(Span {
            enter: -FAR,
            exit: root,
        })
    }
}

/// A beam's brightness across its width.
///
/// `x` is the tangent of the angle off the axis over the tangent of the
/// edge, so 0 on the axis and 1 where the light ends. The curve is 1 on
/// the axis, one half at the beam angle, one tenth at the field angle
/// (GDTF's two definitions) and falls smoothly to nothing at the edge:
/// `exp(−ln 2 · (x / x_beam)ⁿ)` with `n` chosen to pass through the field
/// point, times a window from the field to the edge.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Profile {
    /// Where the curve is one half.
    pub x_beam: f32,
    /// Where the curve is one tenth, and the window to zero starts.
    pub x_field: f32,
    /// `n`: about 2 for a wash, large for a spot's flat field.
    pub exponent: f32,
}

impl Profile {
    /// The profile of a beam whose beam and field *half* angles are given
    /// in radians. Returns it with the tangent of its edge.
    #[must_use]
    pub fn from_half_angles(beam: f32, field: f32) -> (Self, f32) {
        let field = field.clamp(MIN_HALF_ANGLE, MAX_EDGE_HALF_ANGLE);
        let beam = beam.clamp(MIN_HALF_ANGLE * 0.5, field);
        let tan_edge = (field.tan() * EDGE_OVER_FIELD).min(MAX_EDGE_HALF_ANGLE.tan());
        let x_field = field.tan() / tan_edge;
        let x_beam = beam.tan() / tan_edge;
        // (x_field / x_beam)ⁿ = ln 10 / ln 2.
        let ratio = (x_field / x_beam).max(1.000_1);
        let exponent = (core::f32::consts::LOG2_10.ln() / ratio.ln())
            .clamp(EXPONENT_RANGE.0, EXPONENT_RANGE.1);
        (
            Self {
                x_beam,
                x_field,
                exponent,
            },
            tan_edge,
        )
    }

    /// The brightness at `x`, 1 on the axis and 0 from the edge outwards.
    #[must_use]
    pub fn at(&self, x: f32) -> f32 {
        let core = (-core::f32::consts::LN_2 * (x / self.x_beam).powf(self.exponent)).exp();
        core * (1.0 - smoothstep(self.x_field, 1.0, x))
    }
}

/// Hermite step from 0 at `lo` to 1 at `hi`, as WGSL's.
#[must_use]
pub fn smoothstep(lo: f32, hi: f32, x: f32) -> f32 {
    let t = ((x - lo) / (hi - lo)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The haze the beams shine through. Uniform.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Medium {
    /// Light lost per metre of path (scattered away or absorbed).
    pub extinction: f32,
    /// Henyey-Greenstein `g`: 0 scatters evenly, towards 1 forwards.
    pub anisotropy: f32,
}

/// The Henyey-Greenstein phase function, per steradian (its integral over
/// the sphere is 1). `cos_theta` is 1 for light scattered straight on.
#[must_use]
pub fn phase(anisotropy: f32, cos_theta: f32) -> f32 {
    let g = anisotropy;
    let g2 = g * g;
    (1.0 - g2) / (4.0 * core::f32::consts::PI * (1.0 + g2 - 2.0 * g * cos_theta).powf(1.5))
}

/// What a beam needs said about it to be integrated along a ray.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shaft {
    pub cone: Cone,
    pub profile: Profile,
    /// The fraction of the range at which the beam starts fading out, so
    /// its far cap is never seen.
    pub fade_start: f32,
}

impl Shaft {
    /// Everything but the inverse square at the point `from_apex`, seen
    /// along `dir`: profile, phase, extinction and end fade. `t` is how far
    /// the point is along the view ray.
    fn weight(&self, medium: &Medium, from_apex: Vec3, dir: Vec3, t: f32) -> f32 {
        let cone = &self.cone;
        let along = from_apex.dot(cone.axis).max(1.0e-4);
        let dist = from_apex.length().max(1.0e-4);
        let radial = (from_apex.dot(from_apex) - along * along).max(0.0).sqrt();
        let x = radial / (along * cone.tan_edge);
        let cos_theta = from_apex.dot(-dir) / dist;
        let path = t.max(0.0) + (dist - cone.near).max(0.0);
        let depth = (along - cone.near) / (cone.far - cone.near);
        self.profile.at(x)
            * phase(medium.anisotropy, cos_theta)
            * (-medium.extinction * path).exp()
            * (1.0 - smoothstep(self.fade_start, 1.0, depth))
    }
}

/// The light a beam of unit intensity (1 cd on its axis) scatters towards
/// the eye along `span` of `ray`, per unit of scattering coefficient: the
/// integral of `profile · phase · extinction · fade / distance²`, in 1/m.
///
/// The inverse square is integrated exactly on each of `pieces` equal
/// parts of the span (an arctangent); the other terms, which are smooth,
/// are taken at each part's middle. Nothing in it is discrete in space, so
/// the result is continuous in the ray, the cone and the span.
#[must_use]
pub fn scatter(shaft: &Shaft, medium: &Medium, ray: &Ray, span: Span, pieces: u8) -> f32 {
    let from_apex = ray.origin - shaft.cone.apex;
    let foot = from_apex.dot(ray.dir);
    let offset = from_apex - ray.dir * foot;
    let h2 = offset.dot(offset).max(1.0e-8);
    let h = h2.sqrt();
    let step = span.len() / f32::from(pieces);

    (0..pieces)
        .map(|piece| {
            let u0 = span.enter + foot + step * f32::from(piece);
            let u1 = u0 + step;
            // atan(u1/h) − atan(u0/h), as one arctangent that stays exact
            // when the ray passes through the apex's line (h → 0).
            let inverse_square = (h * step).atan2(h2 + u0 * u1) / h;
            let t = span.enter + step * (f32::from(piece) + 0.5);
            inverse_square * shaft.weight(medium, from_apex + ray.dir * t, ray.dir, t)
        })
        .sum()
}

/// The solid angle a beam's light is spread over, weighted by its
/// profile: `∫ profile dω` over the cone, in steradians.
#[must_use]
pub fn effective_solid_angle(profile: &Profile, tan_edge: f32) -> f32 {
    const STEPS: u8 = 96;
    let edge = tan_edge.atan();
    let step = edge / f32::from(STEPS);
    let ring = |theta: f32| profile.at(theta.tan() / tan_edge) * theta.sin();
    // Simpson's rule; the integrand is smooth and zero at both ends.
    let sum: f32 = (1..STEPS)
        .map(|i| {
            let weight = if i % 2 == 1 { 4.0 } else { 2.0 };
            weight * ring(step * f32::from(i))
        })
        .sum();
    2.0 * core::f32::consts::PI * (step / 3.0) * (ring(0.0) + sum + ring(edge))
}

/// A fixture's on-axis intensity in candela: its luminous flux in lumens
/// over the solid angle its profile spreads it across. Integrating the
/// profile times this over the cone gives the flux back, whatever the
/// zoom, so a zoomed-in beam is brighter by exactly what it should be.
#[must_use]
pub fn peak_candela(flux_lumens: f32, profile: &Profile, tan_edge: f32) -> f32 {
    flux_lumens / effective_solid_angle(profile, tan_edge).max(1.0e-9)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cone(half_angle_deg: f32) -> Cone {
        Cone::from_lens(
            Vec3::ZERO,
            Vec3::Z,
            0.05,
            half_angle_deg.to_radians().tan(),
            10.0,
        )
    }

    fn ray(origin: Vec3, towards: Vec3) -> Ray {
        Ray {
            origin,
            dir: (towards - origin).normalize(),
        }
    }

    /// A reference value computed in doubles, back in the type under test.
    #[expect(
        clippy::cast_possible_truncation,
        clippy::as_conversions,
        reason = "f64 to f32 has no lossless conversion; the loss is the point"
    )]
    fn narrow(x: f64) -> f32 {
        x as f32
    }

    /// Whether `p` is inside the cut cone, by its definition.
    fn inside(cone: &Cone, p: Vec3) -> bool {
        let rel = p - cone.apex;
        let along = rel.dot(cone.axis);
        let radial = (rel - cone.axis * along).length();
        along > cone.near && along < cone.far && radial < cone.tan_edge * along
    }

    /// The span a fine march finds, to hold the closed form against.
    fn marched(cone: &Cone, ray: &Ray, t_max: f32) -> Option<Span> {
        let steps = 400_000_u32;
        let dt = f64::from(t_max) / f64::from(steps);
        let mut found: Option<Span> = None;
        for i in 0..steps {
            let t = narrow((f64::from(i) + 0.5) * dt);
            if inside(cone, ray.origin + ray.dir * t) {
                found = Some(found.map_or(Span { enter: t, exit: t }, |s| Span {
                    enter: s.enter,
                    exit: t,
                }));
            }
        }
        found
    }

    fn assert_span(cone: &Cone, ray: &Ray, t_max: f32) {
        let exact = cone_segment(cone, ray, t_max);
        let march = marched(cone, ray, t_max);
        match (exact, march) {
            (None, None) => {}
            (Some(e), Some(m)) => {
                assert!(
                    (e.enter - m.enter).abs() < 2.0e-3 && (e.exit - m.exit).abs() < 2.0e-3,
                    "exact {e:?} against marched {m:?}"
                );
            }
            (Some(e), None) => assert!(e.len() < 2.0e-3, "exact {e:?}, march found nothing"),
            (None, Some(m)) => assert!(m.len() < 2.0e-3, "march {m:?}, exact found nothing"),
        }
    }

    #[test]
    fn a_ray_across_the_beam_enters_and_leaves_at_its_edge() {
        let cone = cone(10.0);
        let ray = ray(Vec3::new(-5.0, 0.0, 5.0), Vec3::new(5.0, 0.0, 5.0));
        let span = cone_segment(&cone, &ray, 100.0).unwrap();
        let half_width = cone.tan_edge * (5.0 + cone.near);
        assert!((span.enter - (5.0 - half_width)).abs() < 1.0e-4);
        assert!((span.exit - (5.0 + half_width)).abs() < 1.0e-4);
        assert_span(&cone, &ray, 100.0);
    }

    #[test]
    fn a_ray_that_misses_gets_nothing() {
        let cone = cone(10.0);
        let above = ray(Vec3::new(-5.0, 3.0, 5.0), Vec3::new(5.0, 3.0, 5.0));
        assert_eq!(cone_segment(&cone, &above, 100.0), None);
        let behind = ray(Vec3::new(-5.0, 0.0, -2.0), Vec3::new(5.0, 0.0, -2.0));
        assert_eq!(cone_segment(&cone, &behind, 100.0), None);
        let past = ray(Vec3::new(-5.0, 0.0, 12.0), Vec3::new(5.0, 0.0, 12.0));
        assert_eq!(cone_segment(&cone, &past, 100.0), None);
    }

    #[test]
    fn the_other_nappe_is_not_the_beam() {
        // Behind the apex the quadric is negative too; nothing shines there.
        let cone = cone(20.0);
        let ray = ray(Vec3::new(-5.0, 0.0, -4.0), Vec3::new(5.0, 0.0, -4.0));
        assert_eq!(cone_segment(&cone, &ray, 100.0), None);
    }

    #[test]
    fn a_grazing_ray_gets_a_span_that_shrinks_to_nothing() {
        let cone = cone(10.0);
        let edge = cone.tan_edge * (5.0 + cone.near);
        let mut last = f32::MAX;
        for inset in [0.1, 0.01, 0.001, 0.000_1] {
            let y = edge - inset;
            let ray = ray(Vec3::new(-5.0, y, 5.0), Vec3::new(5.0, y, 5.0));
            let len = cone_segment(&cone, &ray, 100.0).map_or(0.0, Span::len);
            assert!(len < last, "span grows as the ray leaves: {len} >= {last}");
            last = len;
        }
        assert!(last < 0.05);
        let outside = ray(
            Vec3::new(-5.0, edge + 1.0e-3, 5.0),
            Vec3::new(5.0, edge + 1.0e-3, 5.0),
        );
        assert_eq!(cone_segment(&cone, &outside, 100.0), None);
    }

    #[test]
    fn a_camera_inside_the_beam_starts_at_itself() {
        let cone = cone(25.0);
        let eye = Vec3::new(0.2, 0.1, 4.0);
        assert!(inside(&cone, eye));
        for towards in [
            Vec3::new(5.0, 0.0, 4.0),
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 20.0),
            Vec3::new(-3.0, 2.0, 6.0),
        ] {
            let ray = ray(eye, towards);
            let span = cone_segment(&cone, &ray, 100.0).unwrap();
            assert!(span.enter.abs() < 1.0e-6, "starts at {}", span.enter);
            assert_span(&cone, &ray, 30.0);
        }
    }

    #[test]
    fn a_ray_along_the_axis_runs_from_lens_to_far_cap() {
        let cone = cone(2.0);
        let down = ray(Vec3::new(0.0, 0.0, -3.0), Vec3::new(0.0, 0.0, 1.0));
        let span = cone_segment(&cone, &down, 100.0).unwrap();
        assert!((span.enter - 3.0).abs() < 1.0e-4);
        assert!((span.exit - 13.0).abs() < 1.0e-4);
        // Looking back up the beam at the lens.
        let up = ray(Vec3::new(0.01, 0.0, 30.0), Vec3::new(0.01, 0.0, 0.0));
        let span = cone_segment(&cone, &up, 100.0).unwrap();
        assert!((span.enter - 20.0).abs() < 1.0e-3);
        assert!((span.exit - 30.0).abs() < 1.0e-3);
    }

    #[test]
    fn a_ray_parallel_to_the_cones_surface_crosses_it_once() {
        let cone = cone(30.0);
        // Direction along a generator of the cone: the quadratic's leading
        // term vanishes and the crossing is the linear root.
        let dir = Vec3::new(cone.tan_edge, 0.0, 1.0).normalize();
        let ray = Ray {
            origin: Vec3::new(-1.0, 0.0, 1.0),
            dir,
        };
        let span = cone_segment(&cone, &ray, 100.0).unwrap();
        assert!(inside(&cone, ray.origin + ray.dir * (span.enter + 1.0e-3)));
        assert!(!inside(&cone, ray.origin + ray.dir * (span.enter - 1.0e-3)));
        assert_span(&cone, &ray, 30.0);
    }

    #[test]
    fn the_scene_cuts_the_span_short() {
        let cone = cone(10.0);
        let ray = ray(Vec3::new(-5.0, 0.0, 5.0), Vec3::new(5.0, 0.0, 5.0));
        let whole = cone_segment(&cone, &ray, 100.0).unwrap();
        let cut = cone_segment(&cone, &ray, 5.0).unwrap();
        assert!((cut.enter - whole.enter).abs() < 1.0e-6);
        assert!((cut.exit - 5.0).abs() < 1.0e-6);
        assert_eq!(cone_segment(&cone, &ray, whole.enter - 0.1), None);
    }

    #[test]
    fn spans_match_a_march_from_many_directions() {
        let cones = [cone(1.0), cone(12.0), cone(40.0)];
        let eyes = [
            Vec3::new(-6.0, 1.5, 4.0),
            Vec3::new(0.3, -0.2, 14.0),
            Vec3::new(2.0, 5.0, -3.0),
            Vec3::new(0.02, 0.01, 6.0),
        ];
        let targets = [
            Vec3::new(0.0, 0.0, 5.0),
            Vec3::new(0.4, 0.3, 1.0),
            Vec3::new(-1.0, 0.5, 9.5),
            Vec3::new(0.0, 0.0, 0.0),
        ];
        for cone in &cones {
            for eye in eyes {
                for target in targets {
                    assert_span(cone, &ray(eye, target), 25.0);
                }
            }
        }
    }

    fn shaft(half_angle_deg: f32) -> Shaft {
        let (profile, tan_edge) = Profile::from_half_angles(
            (half_angle_deg * 0.6).to_radians(),
            half_angle_deg.to_radians(),
        );
        Shaft {
            cone: Cone::from_lens(Vec3::ZERO, Vec3::Z, 0.06, tan_edge, 30.0),
            profile,
            fade_start: 0.6,
        }
    }

    /// The same integral by brute force, in doubles.
    fn scatter_numeric(shaft: &Shaft, medium: &Medium, ray: &Ray, span: Span) -> f64 {
        let steps = 200_000_u32;
        let dt = f64::from(span.len()) / f64::from(steps);
        (0..steps)
            .map(|i| {
                let t32 = narrow(f64::from(span.enter) + (f64::from(i) + 0.5) * dt);
                let from_apex = ray.origin + ray.dir * t32 - shaft.cone.apex;
                let weight = shaft.weight(medium, from_apex, ray.dir, t32);
                f64::from(weight) / f64::from(from_apex.length_squared()) * dt
            })
            .sum()
    }

    const CLEAR: Medium = Medium {
        extinction: 0.0,
        anisotropy: 0.0,
    };
    const HAZE: Medium = Medium {
        extinction: 0.02,
        anisotropy: 0.3,
    };

    #[test]
    fn the_inverse_square_alone_is_integrated_exactly() {
        // A profile that is 1 everywhere inside, no phase, no extinction and
        // no fade: what is left is ∫ dt/d², which is known in closed form
        // and must not depend on how many pieces it is cut into.
        let flat = Shaft {
            cone: Cone::from_lens(Vec3::ZERO, Vec3::Z, 0.06, 0.3, 30.0),
            profile: Profile {
                x_beam: 1.0e6,
                x_field: 1.0e6,
                exponent: 1.0,
            },
            fade_start: 1.0e6,
        };
        let ray = ray(Vec3::new(-8.0, 0.5, 6.0), Vec3::new(4.0, 0.0, 7.0));
        let span = cone_segment(&flat.cone, &ray, 100.0).unwrap();
        let unit = f64::from(4.0 * core::f32::consts::PI);
        let reference = scatter_numeric(&flat, &CLEAR, &ray, span) * unit;
        for pieces in [1, 4, 16] {
            let closed = f64::from(scatter(&flat, &CLEAR, &ray, span, pieces)) * unit;
            assert!(
                (closed - reference).abs() < 2.0e-5 * reference,
                "{pieces} pieces: {closed} against {reference}"
            );
        }
    }

    #[test]
    fn the_closed_form_matches_a_numeric_integral() {
        let cases = [
            // A narrow beam from the side.
            (
                shaft(1.5),
                ray(Vec3::new(-6.0, 0.0, 8.0), Vec3::new(0.0, 0.02, 8.0)),
            ),
            // A wash from the side, low down.
            (
                shaft(25.0),
                ray(Vec3::new(-9.0, 1.0, 3.0), Vec3::new(0.0, 0.3, 4.0)),
            ),
            // Looking up a spot at its lens, a little off its axis.
            (
                shaft(8.0),
                ray(Vec3::new(0.4, 0.1, 18.0), Vec3::new(0.0, 0.0, 0.0)),
            ),
            // From inside a wash, across it.
            (
                shaft(25.0),
                ray(Vec3::new(0.3, 0.2, 5.0), Vec3::new(6.0, 1.0, 7.0)),
            ),
        ];
        for (shaft, ray) in &cases {
            let span = cone_segment(&shaft.cone, ray, 100.0).unwrap();
            let reference = scatter_numeric(shaft, &HAZE, ray, span);
            let mut last_error = f64::MAX;
            for (pieces, tolerance) in [(4_u8, 0.12), (8, 0.04), (16, 0.012), (64, 0.001)] {
                let closed = f64::from(scatter(shaft, &HAZE, ray, span, pieces));
                let error = ((closed - reference) / reference).abs();
                assert!(
                    error < tolerance,
                    "{pieces} pieces: {closed} against {reference} ({error:.4} off)"
                );
                assert!(error < last_error.max(1.0e-4), "more pieces, worse answer");
                last_error = error;
            }
        }
    }

    #[test]
    fn the_light_is_continuous_as_the_camera_enters_the_cone() {
        // Walk the eye through the cone's edge in small steps: the scattered
        // light may not jump, which is what would show as a pop on screen.
        let shaft = shaft(20.0);
        let target = Vec3::new(3.0, 0.5, 9.0);
        let mut last: Option<f32> = None;
        for i in 0..400_u16 {
            let x = -3.0 + f32::from(i) * 0.01;
            let ray = ray(Vec3::new(x, 0.0, 6.0), target);
            let light = cone_segment(&shaft.cone, &ray, 100.0)
                .map_or(0.0, |span| scatter(&shaft, &HAZE, &ray, span, 6));
            if let Some(last) = last {
                assert!(
                    (light - last).abs() < 0.004,
                    "a jump of {} at x = {x}",
                    light - last
                );
            }
            last = Some(light);
        }
    }

    #[test]
    fn the_profile_passes_through_gdtfs_two_angles() {
        for (beam, field) in [(1.0_f32, 1.6_f32), (7.0, 10.0), (12.0, 25.0), (30.0, 32.0)] {
            let (profile, tan_edge) =
                Profile::from_half_angles(beam.to_radians(), field.to_radians());
            let at = |deg: f32| profile.at(deg.to_radians().tan() / tan_edge);
            assert!((at(0.0) - 1.0).abs() < 1.0e-6);
            assert!((at(beam) - 0.5).abs() < 1.0e-4, "beam: {}", at(beam));
            if profile.exponent < EXPONENT_RANGE.1 && profile.exponent > EXPONENT_RANGE.0 {
                assert!((at(field) - 0.1).abs() < 1.0e-3, "field: {}", at(field));
            }
            assert!(profile.at(1.0).abs() < 1.0e-6);
            let mut last = 1.0_f32;
            for i in 1..=200_u8 {
                let now = profile.at(f32::from(i) / 200.0);
                assert!(now <= last + 1.0e-6, "not monotone at {i}");
                last = now;
            }
        }
    }

    #[test]
    fn the_phase_function_integrates_to_one() {
        for g in [0.0_f32, 0.3, 0.7] {
            let steps = 2000_u16;
            let d = core::f32::consts::PI / f32::from(steps);
            let total: f32 = (0..steps)
                .map(|i| {
                    let theta = (f32::from(i) + 0.5) * d;
                    phase(g, theta.cos()) * 2.0 * core::f32::consts::PI * theta.sin() * d
                })
                .sum();
            assert!((total - 1.0).abs() < 2.0e-3, "g = {g}: {total}");
        }
    }

    /// The flux a beam of the given peak intensity carries: its intensity
    /// over the whole cone, by a finer rule than the one under test.
    fn flux_of(peak: f32, profile: &Profile, tan_edge: f32) -> f64 {
        let steps = 20_000_u16;
        let edge = f64::from(tan_edge.atan());
        let d = edge / f64::from(steps);
        (0..steps)
            .map(|i| {
                let theta = (f64::from(i) + 0.5) * d;
                let x = narrow(theta.tan() / f64::from(tan_edge));
                f64::from(peak) * f64::from(profile.at(x)) * core::f64::consts::TAU * theta.sin() * d
            })
            .sum()
    }

    #[test]
    fn intensity_times_solid_angle_gives_the_flux_back() {
        for (beam, field) in [(1.0_f32, 1.5_f32), (6.0, 9.0), (15.0, 25.0), (30.0, 45.0)] {
            let (profile, tan_edge) =
                Profile::from_half_angles(beam.to_radians(), field.to_radians());
            let peak = peak_candela(20_000.0, &profile, tan_edge);
            let flux = flux_of(peak, &profile, tan_edge);
            assert!(
                (flux - 20_000.0).abs() < 20.0,
                "{beam}°/{field}°: {flux} lm from a 20 000 lm fixture"
            );
        }
    }

    #[test]
    fn zooming_in_brightens_by_the_square_of_the_narrowing() {
        // Small angles, same shape: half the angle is a quarter of the solid
        // angle, so four times the candela.
        let (wide, wide_edge) =
            Profile::from_half_angles(4.0_f32.to_radians(), 6.0_f32.to_radians());
        let (tight, tight_edge) =
            Profile::from_half_angles(2.0_f32.to_radians(), 3.0_f32.to_radians());
        let ratio =
            peak_candela(1.0, &tight, tight_edge) / peak_candela(1.0, &wide, wide_edge);
        assert!((ratio - 4.0).abs() < 0.03, "ratio {ratio}");
    }

    #[test]
    fn a_flat_field_has_the_intensity_of_a_uniform_cone() {
        // A spot whose beam and field angles nearly coincide is a top hat:
        // its candela is its flux over the cone's solid angle, to within the
        // soft rim.
        let half = 10.0_f32.to_radians();
        let (profile, tan_edge) = Profile::from_half_angles(half * 0.97, half);
        let peak = peak_candela(10_000.0, &profile, tan_edge);
        let uniform = 10_000.0 / (2.0 * core::f32::consts::PI * (1.0 - half.cos()));
        assert!(
            (peak / uniform - 1.0).abs() < 0.12,
            "{peak} cd against a uniform cone's {uniform}"
        );
    }
}
