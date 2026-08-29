//! The froxel uniform is declared twice — once in Rust for `encase` to
//! lay out, once in WGSL for the shader to read — and nothing checks
//! that the two agree.
//!
//! They have disagreed twice. The first time `dilution` and
//! `has_density_texture` were swapped, so the shader read the flag's
//! bit pattern as a float (a denormal, near zero) and the flag read a
//! float's bits (never zero): the fog's falloff was silently disabled
//! and the density texture silently always on. Nothing failed, nothing
//! logged, and the picture was merely wrong — which is the worst shape
//! a bug can have.
//!
//! Names in order is not enough, as the second occurrence proved: the
//! two agreed on every name and still disagreed on every *offset*,
//! because WGSL's uniform layout rounds a `vec3` up to sixteen bytes
//! while `encase` packs `UVec3` into twelve. The shader then read
//! `near` as `far` and `far` as `scattering`, put every froxel at the
//! wrong depth, and rendered a room with no beams in it. Nothing
//! failed and nothing logged.
//!
//! So the rule this enforces is structural: **every member of the
//! uniform is sixteen bytes wide and sixteen-byte aligned** — a `vec4`
//! or a `mat4x4`, never a bare scalar and never a `vec3`. Scalars live
//! in the lanes of a vector. Two declarations built that way cannot
//! disagree about offsets, whatever either compiler believes about
//! packing.
// r[verify viz.haze-is-volumetric] - the two declarations of the grid's uniform agree

use std::path::Path;

/// Field names, in declaration order, from a Rust struct.
fn rust_fields(source: &str, name: &str) -> Vec<String> {
    let start = source
        .find(&format!("pub struct {name} {{"))
        .unwrap_or_else(|| panic!("no `{name}` in the Rust source"));
    let body = &source[start..];
    let end = body.find("\n}").expect("unterminated struct");
    body[..end].lines().skip(1).filter_map(field_name).collect()
}

/// Field names, in declaration order, from a WGSL struct.
fn wgsl_fields(source: &str, name: &str) -> Vec<String> {
    let start = source
        .find(&format!("struct {name} {{"))
        .unwrap_or_else(|| panic!("no `{name}` in the WGSL source"));
    let body = &source[start..];
    let end = body.find("\n}").expect("unterminated struct");
    body[..end].lines().skip(1).filter_map(field_name).collect()
}

/// `    name: type,` — and nothing else. Comments, attributes and blank
/// lines are skipped, which is what makes the two lists comparable.
fn field_name(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with("//") || trimmed.starts_with('#') {
        return None;
    }
    let (name, _) = trimmed.split_once(':')?;
    let name = name.trim();
    name.chars()
        .all(|c| c.is_ascii_lowercase() || c == '_' || c.is_ascii_digit())
        .then(|| name.to_owned())
}

/// Whether a declared type is sixteen bytes wide and aligned to
/// sixteen, in either language.
fn is_a_full_lane(ty: &str) -> bool {
    let ty = ty.trim().trim_end_matches(',').trim();
    matches!(ty, "Vec4" | "UVec4" | "IVec4" | "Mat4")
        || ty.starts_with("vec4<")
        || ty.starts_with("mat4x4<")
}

/// `    name: type,` split into both halves.
fn field_pair(line: &str) -> Option<(String, String)> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with("//") || trimmed.starts_with('#') {
        return None;
    }
    let (name, ty) = trimmed.split_once(':')?;
    let name = name.trim();
    name.chars()
        .all(|c| c.is_ascii_lowercase() || c == '_' || c.is_ascii_digit())
        .then(|| (name.to_owned(), ty.trim().trim_end_matches(',').to_owned()))
}

#[test]
fn every_field_of_the_froxel_uniform_fills_a_whole_lane() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .join("bevy-pbr-vendored/src/volumetric_fog");

    for (file, name) in [
        ("froxel.rs", "FroxelUniform"),
        ("froxel.wgsl", "FroxelGrid"),
    ] {
        let source = std::fs::read_to_string(root.join(file)).expect(file);
        let marker = if file.ends_with(".rs") {
            format!("pub struct {name} {{")
        } else {
            format!("struct {name} {{")
        };
        let start = source
            .find(&marker)
            .unwrap_or_else(|| panic!("no {name} in {file}"));
        let body = &source[start..];
        let end = body.find("\n}").expect("unterminated struct");

        let fields: Vec<_> = body[..end].lines().skip(1).filter_map(field_pair).collect();
        assert!(!fields.is_empty(), "parsed no fields from {file}");
        for (field, ty) in fields {
            assert!(
                is_a_full_lane(&ty),
                "{file}: `{field}: {ty}` is not a whole sixteen-byte lane. A scalar or a \
                 vec3 here lets the Rust and WGSL layouts disagree about every offset below \
                 it; put it in a lane of one of the vectors instead."
            );
        }
    }
}

#[test]
fn the_froxel_uniform_is_declared_the_same_way_twice() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .join("bevy-pbr-vendored/src/volumetric_fog");

    let rust = std::fs::read_to_string(root.join("froxel.rs")).expect("froxel.rs");
    let wgsl = std::fs::read_to_string(root.join("froxel.wgsl")).expect("froxel.wgsl");

    let declared = rust_fields(&rust, "FroxelUniform");
    let read = wgsl_fields(&wgsl, "FroxelGrid");

    assert!(
        !declared.is_empty() && !read.is_empty(),
        "parsed nothing: {declared:?} / {read:?}"
    );
    assert_eq!(
        declared, read,
        "the froxel uniform's fields are in different orders in Rust and WGSL, so each \
         field reads the bits of another. Rust: {declared:?}, WGSL: {read:?}"
    );
}
