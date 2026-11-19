//! The shader's mathematics and the Rust copy of it are the same.
//!
//! `shaders/scatter.wesl` is what the GPU runs; `src/math.rs` is what the
//! tests in this crate prove things about. This runs the WESL itself, on
//! the CPU with `wesl`'s evaluator, and holds its answers against the
//! Rust for the same inputs: where a ray is inside a cone, the light
//! scattered along it, the profile, the phase function, and what is left
//! of a span behind occluders. No GPU.

#![expect(
    clippy::arithmetic_side_effects,
    reason = "arithmetic on floats and glam vectors, which the lint cannot tell from integers"
)]
#![expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test: a shader that does not compile or evaluate is the failure being reported"
)]

use bevy::math::Vec3;
use ignition_beams::math::{
    Cone, Medium, Profile, Ray, Shaft, Span, Sphere, cone_segment, lit_spans, phase, scatter,
    scatter_past, shadow_cone,
};
use wesl::{CompileResult, Compiler, resolver::VirtualResolver};

/// The pieces the shader is compiled with here, as a tier would set them.
const PIECES: u8 = 6;

/// The shader's module, compiled for evaluation. Two things are done to
/// the text and nothing else: `constants::PIECES`, which Bevy supplies per
/// pipeline, is written in, and every function is marked `@const`, which
/// is what lets the evaluator call it.
fn shader() -> CompileResult {
    let source = include_str!("../src/shaders/scatter.wesl")
        .replace("constants::PIECES", &format!("{PIECES}u"))
        .replace("\nfn ", "\n@const fn ");
    let mut resolver = VirtualResolver::new();
    resolver.add_module("package::main".parse().unwrap(), source.into());
    let mut compiler = Compiler::default().with_resolver(resolver);
    compiler.options.keep_main = true;
    compiler
        .compile("main.wesl")
        .unwrap_or_else(|error| panic!("scatter.wesl does not compile: {error}"))
}

/// Every number in an evaluated value, in order: `vec2f(1.5f, 2f)` is
/// `[1.5, 2.0]`.
fn numbers(value: &str) -> Vec<f32> {
    value
        .split(|c: char| !(c.is_ascii_digit() || matches!(c, '.' | '-' | '+' | 'e' | 'E')))
        .filter(|token| token.chars().any(|c| c.is_ascii_digit()))
        .filter_map(|token| token.parse().ok())
        .collect()
}

fn eval(shader: &CompileResult, expression: &str) -> Vec<f32> {
    let value = shader
        .eval(expression)
        .unwrap_or_else(|error| panic!("{expression}\n{error}"))
        .to_string();
    // Type names carry digits of their own (`vec2f`, `array<vec2<f32>, 7>`).
    let bare = ["vec2", "vec3", "vec4", "f32", "u32", ", 7>"]
        .iter()
        .fold(value, |text, name| text.replace(name, " "));
    numbers(&bare)
}

fn v3(v: Vec3) -> String {
    format!("vec3<f32>({:?}, {:?}, {:?})", v.x, v.y, v.z)
}

fn wesl_cone(cone: &Cone) -> String {
    format!(
        "Cone({}, {}, {:?}, {:?}, {:?})",
        v3(cone.apex),
        v3(cone.axis),
        cone.tan_edge,
        cone.near,
        cone.far
    )
}

/// The beam whose cone and profile a [`Shaft`] is, as the shader's struct.
fn wesl_beam(shaft: &Shaft) -> String {
    let cone = &shaft.cone;
    let lens = cone.apex + cone.axis * cone.near;
    let lens_radius = cone.near * cone.tan_edge;
    let zero = "vec4<f32>(0.0, 0.0, 0.0, 0.0)";
    format!(
        "Beam(vec4<f32>({}, {:?}), vec4<f32>({}, {:?}), vec4<f32>(1.0, 0.0, 0.0, {:?}), \
         vec4<f32>(0.0, 1.0, 0.0, {:?}), {zero}, vec4<f32>({:?}, {:?}, 1.0, {:?}), \
         vec4<f32>(-1.0, 0.0, 0.0, 3.0), {zero})",
        v3(lens),
        cone.far - cone.near,
        v3(cone.axis),
        cone.tan_edge,
        shaft.profile.x_beam,
        lens_radius,
        shaft.profile.exponent,
        shaft.profile.x_field,
        shaft.fade_start,
    )
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

fn ray(origin: Vec3, towards: Vec3) -> Ray {
    Ray {
        origin,
        dir: (towards - origin).normalize(),
    }
}

/// Rays that cross, graze, miss, start inside and run along a beam.
fn rays() -> Vec<Ray> {
    vec![
        ray(Vec3::new(-6.0, 0.0, 8.0), Vec3::new(0.0, 0.02, 8.0)),
        ray(Vec3::new(-9.0, 1.0, 3.0), Vec3::new(0.0, 0.3, 4.0)),
        ray(Vec3::new(0.4, 0.1, 18.0), Vec3::new(0.0, 0.0, 0.0)),
        ray(Vec3::new(0.3, 0.2, 5.0), Vec3::new(6.0, 1.0, 7.0)),
        ray(Vec3::new(0.0, 0.0, 25.0), Vec3::new(0.0, 0.0, 0.0)),
        ray(Vec3::new(-5.0, 9.0, 5.0), Vec3::new(5.0, 9.0, 5.0)),
        ray(Vec3::new(2.0, 5.0, -3.0), Vec3::new(-1.0, 0.5, 9.5)),
    ]
}

const HAZE: Medium = Medium {
    extinction: 0.02,
    anisotropy: 0.3,
};

fn close(a: f32, b: f32, tolerance: f32) -> bool {
    (a - b).abs() <= tolerance * a.abs().max(b.abs()).max(1.0)
}

#[test]
fn the_shader_finds_the_same_segment_of_every_ray() {
    let shader = shader();
    for half_angle in [1.5, 12.0, 40.0] {
        let shaft = shaft(half_angle);
        for ray in rays() {
            let rust = cone_segment(&shaft.cone, &ray, 25.0);
            let wesl = eval(
                &shader,
                &format!(
                    "cone_span({}, {}, {}, 25.0)",
                    wesl_cone(&shaft.cone),
                    v3(ray.origin),
                    v3(ray.dir)
                ),
            );
            let (enter, exit) = (wesl[0], wesl[1]);
            match rust {
                Some(span) => assert!(
                    close(span.enter, enter, 1.0e-4) && close(span.exit, exit, 1.0e-4),
                    "{half_angle}°, {ray:?}: Rust {span:?}, WESL {enter}..{exit}"
                ),
                None => assert!(
                    exit <= enter,
                    "{half_angle}°, {ray:?}: WESL {enter}..{exit}"
                ),
            }
        }
    }
}

#[test]
fn the_shader_scatters_the_same_light_along_every_ray() {
    let shader = shader();
    let ones = "array<f32, 16>(1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0)";
    let mut compared = 0_u8;
    for half_angle in [1.5, 12.0, 40.0] {
        let shaft = shaft(half_angle);
        for ray in rays() {
            let Some(span) = cone_segment(&shaft.cone, &ray, 25.0) else {
                continue;
            };
            let rust = scatter(&shaft, HAZE, &ray, span, PIECES);
            let wesl = eval(
                &shader,
                &format!(
                    "scatter({}, {}, vec2<f32>({:?}, {:?}), {}, {}, vec2<f32>({:?}, {:?}), {ones})",
                    wesl_beam(&shaft),
                    wesl_cone(&shaft.cone),
                    HAZE.extinction,
                    HAZE.anisotropy,
                    v3(ray.origin),
                    v3(ray.dir),
                    span.enter,
                    span.exit
                ),
            )[0];
            assert!(
                (rust - wesl).abs() <= 2.0e-4 * rust.abs().max(1.0e-6),
                "{half_angle}°, {ray:?}: Rust {rust}, WESL {wesl}"
            );
            compared += 1;
        }
    }
    assert!(compared >= 9, "only {compared} rays met a beam");
}

#[test]
fn the_shader_has_the_same_profile_and_phase() {
    let shader = shader();
    let shaft = shaft(12.0);
    for i in 0..=20_u8 {
        let x = f32::from(i) / 20.0 * 1.1;
        let wesl = eval(&shader, &format!("profile({}, {x:?})", wesl_beam(&shaft)))[0];
        assert!(close(shaft.profile.at(x), wesl, 1.0e-5), "profile at {x}");
        let cos = f32::from(i) / 10.0 - 1.0;
        let wesl = eval(&shader, &format!("phase(0.3, {cos:?})"))[0];
        assert!(close(phase(0.3, cos), wesl, 1.0e-5), "phase at {cos}");
    }
}

#[test]
fn the_shader_leaves_the_same_light_behind_occluders() {
    let shader = shader();
    let shaft = shaft(8.0);
    let spheres = [
        Sphere {
            centre: Vec3::new(0.0, 0.0, 6.0),
            radius: 0.3,
        },
        Sphere {
            centre: Vec3::new(0.4, 0.1, 9.0),
            radius: 0.25,
        },
    ];
    let wesl_spheres = format!(
        "array<vec4<f32>, 6>(vec4<f32>({}, 0.3), vec4<f32>({}, 0.25), vec4<f32>(), vec4<f32>(), vec4<f32>(), vec4<f32>())",
        v3(spheres[0].centre)
            .trim_start_matches("vec3<f32>(")
            .trim_end_matches(')'),
        v3(spheres[1].centre)
            .trim_start_matches("vec3<f32>(")
            .trim_end_matches(')'),
    );
    for ray in [
        ray(Vec3::new(-6.0, 0.0, 9.0), Vec3::new(0.0, 0.0, 9.0)),
        ray(Vec3::new(-6.0, 0.2, 12.0), Vec3::new(3.0, 0.0, 11.0)),
        ray(Vec3::new(0.0, 0.0, 20.0), Vec3::new(0.0, 0.0, 8.0)),
        ray(Vec3::new(-6.0, 0.0, 3.0), Vec3::new(0.0, 0.0, 3.0)),
    ] {
        let span = cone_segment(&shaft.cone, &ray, 25.0).unwrap();
        let shadows: Vec<Span> = spheres
            .iter()
            .filter_map(|s| shadow_cone(shaft.cone.apex, *s))
            .filter_map(|shadow| cone_segment(&shadow, &ray, ignition_beams::math::FAR))
            .collect();
        let rust = lit_spans(span, &shadows);
        let wesl = eval(
            &shader,
            &format!(
                "lit_spans({}, {}, {}, vec2<f32>({:?}, {:?}), {wesl_spheres}, 2u)",
                v3(shaft.cone.apex),
                v3(ray.origin),
                v3(ray.dir),
                span.enter,
                span.exit
            ),
        );
        // Seven spans of two numbers, then the count.
        let count = wesl.last().copied().unwrap();
        assert!(
            close(f32::from(u8::try_from(rust.len()).unwrap()), count, 1.0e-6),
            "{ray:?}: {rust:?} against {wesl:?}"
        );
        for (i, lit) in rust.iter().enumerate() {
            assert!(
                close(lit.enter, wesl[i * 2], 1.0e-4) && close(lit.exit, wesl[i * 2 + 1], 1.0e-4),
                "{ray:?}: {rust:?} against {wesl:?}"
            );
        }
        // And the Rust the tests prove things about agrees with itself.
        let whole = scatter(&shaft, HAZE, &ray, span, PIECES);
        assert!(scatter_past(&shaft, HAZE, &ray, span, &spheres, PIECES) <= whole * 1.000_1);
    }
}
