//! Maps a patched fixture's manufacturer/model string (straight from the
//! live Eos patch — see `docs/domain/norco-patch-and-groups.md`) to a real
//! shape and real dimensions, instead of the one-size box+cone every
//! fixture used to render as regardless of whether it's a 12cm par or a
//! 1m LED bar.
//!
//! Where a real fixture shape exists, it's one of QLC+'s generic
//! per-category meshes (par/moving-head/hazer — see
//! `assets/qlc-meshes/LICENSE-NOTICE.txt`), scaled to the closest
//! real-world dimension found for the actual fixture (manufacturer
//! listings, Open Fixture Library — see the commit that introduced this
//! file for sources). Not exact to the millimetre, but a real par can no
//! longer looks the same size and shape as a real bar or a real hazer.

use crate::mesh::MeshBuilder;
use crate::obj_mesh::ObjMesh;
use glam::{Quat, Vec3};
use std::sync::OnceLock;

macro_rules! qlc_mesh {
    ($name:ident, $file:literal) => {
        fn $name() -> &'static ObjMesh {
            static CELL: OnceLock<ObjMesh> = OnceLock::new();
            CELL.get_or_init(|| ObjMesh::parse(include_str!(concat!("../assets/qlc-meshes/", $file))))
        }
    };
}

qlc_mesh!(par_mesh, "par.obj");
qlc_mesh!(moving_head_mesh, "moving_head.obj");
qlc_mesh!(hazer_mesh, "hazer.obj");
qlc_mesh!(scanner_mesh, "scanner.obj");
qlc_mesh!(smoke_mesh, "smoke.obj");
qlc_mesh!(strobe_mesh, "strobe.obj");

/// Uniform scale that brings `mesh`'s bounding box's largest dimension to
/// `target_size` metres.
fn scale_to(mesh: &ObjMesh, target_size: f32) -> f32 {
    target_size / (mesh.max_half_extent() * 2.0)
}

/// The converted `moving_head.obj` sits at identity rotation in its own
/// "resting upright" pose (yoke base down, head up) — but every placement
/// quaternion in this project (Augment3d's own convention, per
/// `norco-venue-reference.md`) treats identity as "hung from the truss,"
/// the opposite pose. Confirmed backwards 2026-08-24: with no correction,
/// the truss-hung centre OH movers rendered upright and the floor-standing
/// units rendered hanging — exactly inverted. One 180° pre-rotation on the
/// mesh (not per-fixture data) fixes every moving-head instance at once,
/// since it's a mesh-convention mismatch, not a placement error.
fn moving_head_pre_rotate() -> Quat {
    Quat::from_rotation_x(std::f32::consts::PI)
}

/// How the placement position relates to the mesh's own geometry — the
/// mesh's local origin is usually mid-body, not at any physically
/// meaningful point, so without one of these the model just floats
/// centred on the given position regardless of what that position means
/// physically (a mount pivot, a floor contact point, ...).
#[derive(Clone, Copy)]
pub enum Anchor {
    /// Position is the mesh's own origin — fine only when the origin
    /// already reads as roughly the right point (e.g. a par can's yoke).
    None,
    /// Position is where the model's base touches down — shift up so the
    /// rotated/scaled bottom lands exactly there (floor-standing fixtures:
    /// sinks into the floor otherwise).
    Bottom,
    /// Position is the mount/pivot point things hang *from* — shift down
    /// so the rotated/scaled top lands exactly there (truss/beam-hung
    /// fixtures: the mount point is above the body, not through its
    /// middle — left as `None`, the body can poke up through whatever
    /// it's mounted to).
    Top,
    /// For a shape (like the moving-head mesh) used for *both* hung and
    /// upright-standing instances of the same fixture type: decides
    /// Bottom vs Top per-instance from the fixture's own placement
    /// rotation, not a fixed choice per fixture type. See
    /// `hang_aware_anchor`.
    HangAware,
}

pub enum Shape {
    /// One of QLC+'s generic category meshes, scaled to a real fixture's
    /// approximate overall size. `pre_rotate` is applied before the
    /// fixture's own placement rotation — see `moving_head_pre_rotate`.
    Mesh { mesh: &'static ObjMesh, target_size: f32, pre_rotate: Quat, anchor: Anchor },
    /// An LED bar/batten: one elongated box, no beam (washes along its
    /// length rather than from a point) — no QLC+ mesh fits this shape.
    Bar { length: f32, width: f32, height: f32 },
    /// No dedicated profile — falls back to the generic marker.
    Generic,
}

/// `manufacturer`/`model` come verbatim from the live Eos patch
/// (`fixtures.json`'s `manufacturer`/`model` fields). Matched by substring
/// since Eos model strings vary in exact punctuation between channels of
/// the same physical fixture type (e.g. different DMX-mode suffixes).
pub fn shape_for(manufacturer: &str, model: &str) -> Shape {
    let m = manufacturer.to_ascii_lowercase();
    let mo = model.to_ascii_lowercase();

    if m == "uking" && mo.contains("par") {
        // Open Fixture Library's U`King Par Light B262: 180x180x100mm —
        // largest real dimension ~0.18m. Truss-hung; the yoke clamp (the
        // par can's own mesh origin) already reads as roughly the mount
        // point, so no anchor correction needed.
        return Shape::Mesh {
            mesh: par_mesh(),
            target_size: 0.20,
            pre_rotate: Quat::IDENTITY,
            anchor: Anchor::None,
        };
    }
    if m == "betopper" || ((m == "riukoe" || m == "lixada") && mo.contains("gobo")) {
        // Same physical mesh category (moving head) used for both the
        // truss/beam-hung OH movers (Riukoe centre pair) and the
        // floor/box-standing units (Betopper floor movers, Riukoe outer
        // OH pair sitting on a mount box) — which anchor applies depends
        // on how *this instance* is rotated, not the manufacturer. See
        // `Anchor::HangAware`.
        let target_size = if m == "betopper" { 0.35 } else { 0.235 };
        return Shape::Mesh {
            mesh: moving_head_mesh(),
            target_size,
            pre_rotate: moving_head_pre_rotate(),
            anchor: Anchor::HangAware,
        };
    }
    if m == "rockville" && mo.contains("rockstrip") {
        // Rockville's spec: 40.16 x 2.64 x 2.56 in.
        return Shape::Bar { length: 1.02, width: 0.067, height: 0.065 };
    }
    if m == "chauvet" && mo.contains("slimpar") {
        // Chauvet SlimPAR Tri 7 IRC: ~160x115x115mm — the drum-fill pair
        // on each side (chan 50/51, 52/53), clamped sideways to the
        // pillar rather than hung from a truss. Same par mesh as the
        // Uking pars; the yoke clamp again reads as roughly the mount
        // point, so no anchor correction.
        return Shape::Mesh {
            mesh: par_mesh(),
            target_size: 0.16,
            pre_rotate: Quat::IDENTITY,
            anchor: Anchor::None,
        };
    }
    if m == "chauvet" && mo.contains("hurricane") {
        // Chauvet's spec for the Hurricane Haze 1DX: 11 x 6 x 9 in —
        // largest real dimension ~0.28m. Always floor-standing, no hung
        // installation exists for this fixture, so a fixed Bottom anchor
        // (not HangAware — nothing in the data ever "flips" a hazer).
        return Shape::Mesh {
            mesh: hazer_mesh(),
            target_size: 0.28,
            pre_rotate: Quat::IDENTITY,
            anchor: Anchor::Bottom,
        };
    }
    Shape::Generic
}

/// Not matched by any patched Norco fixture today, but available for the
/// next venue: scanner/smoke/strobe meshes at a generic ~0.3m size.
#[allow(dead_code)]
pub fn generic_shape(kind: &str) -> Shape {
    let mesh = match kind {
        "scanner" => scanner_mesh(),
        "smoke" => smoke_mesh(),
        "strobe" => strobe_mesh(),
        _ => return Shape::Generic,
    };
    Shape::Mesh { mesh, target_size: 0.30, pre_rotate: Quat::IDENTITY, anchor: Anchor::None }
}

/// The lowest/highest Z any vertex of `mesh` reaches once rotated by `rot`
/// and scaled by `scale`, relative to the placement origin — i.e. how far
/// below/above the origin the model's actual base/top sits.
fn rotated_z_extent(mesh: &ObjMesh, rot: Quat, scale: f32) -> (f32, f32) {
    let mut min = f32::INFINITY;
    let mut max = f32::NEG_INFINITY;
    for p in &mesh.positions {
        let z = (rot * (*p * scale)).z;
        min = min.min(z);
        max = max.max(z);
    }
    (min, max)
}

/// For `Anchor::HangAware`: whether *this instance* (given its own
/// placement rotation, before `pre_rotate`) is hanging or standing
/// upright. `moving_head_pre_rotate` makes the mesh's local -Z (Augment3d's
/// "hang direction") point back down at identity placement rotation; a
/// fixture whose placement rotation flips that local -Z to point back *up*
/// in world space has been mounted the opposite way — sitting upright on
/// something, not hanging from it. Works for any 180°-ish flip regardless
/// of which axis it's expressed about (X or Y both invert a Z-aligned
/// vector's Z component the same way), which is what both the Riukoe
/// outer-OH-pair and the Betopper floor movers' live-patch rotations
/// turned out to use.
fn is_mounted_upright(placement_rot: Quat) -> bool {
    (placement_rot * Vec3::NEG_Z).z > 0.0
}

/// A lit fixture's live state, for `live` mode's point-light + beam-cone
/// emission — `None` in headless `shot` mode (no live data to emit) and for
/// any live-mode fixture whose resolved dimmer is at/near zero (blacked
/// out, or just no channel map yet). `color` is already dimmer-scaled (see
/// `scene.rs`) — matches `mesh::PointLight`'s convention.
pub struct LiveEmission {
    pub color: [f32; 3],
    /// The real fixture's beam spread, in degrees, if known
    /// (`FixtureRecord::beam_angle_deg`) — sizes the beam cone's radius.
    /// Falls back to a generic spread for fixtures/venues without it.
    pub beam_angle_deg: Option<f32>,
}

/// `rot` is always the fixed *mount* rotation — it alone decides the
/// Bottom/Top anchor (a mechanical fact of how the fixture is rigged, not
/// something pan/tilt changes) and is what the anchor-point maths below is
/// computed against. `live_rot`, when present (a live pan/tilt reading
/// composed onto `rot` — see `scene.rs`'s live-mode fixture loop), replaces
/// `rot` only for the mesh's actual drawn orientation. Approximation worth
/// noting: a real moving head's yoke stays put while only the head
/// assembly tilts: this single-mesh model rotates the whole fixture body
/// around the anchor point instead, since there's no yoke/head geometry
/// split yet (see the GDTF Geometry-tree note in
/// `docs/research/lighting-console-landscape.md`).
///
/// `emit`, when present, registers a point light and a beam-cone glow at
/// the fixture's resolved anchor point/orientation — computed here rather
/// than by the caller since this function is already the one place that
/// knows a fixture's real lens/anchor position after the Bottom/Top anchor
/// maths below.
pub fn add_typed_fixture(
    mesh: &mut MeshBuilder,
    pos: Vec3,
    rot: Quat,
    live_rot: Option<Quat>,
    manufacturer: &str,
    model: &str,
    color: [f32; 3],
    emit: Option<LiveEmission>,
) {
    match shape_for(manufacturer, model) {
        Shape::Mesh { mesh: asset, target_size, pre_rotate, anchor } => {
            let scale = scale_to(asset, target_size);
            let full_rot = rot * pre_rotate;
            let draw_rot = live_rot.unwrap_or(rot) * pre_rotate;
            let resolved = match anchor {
                Anchor::HangAware => {
                    if is_mounted_upright(rot) { Anchor::Bottom } else { Anchor::Top }
                }
                other => other,
            };
            let anchored_pos = match resolved {
                Anchor::Bottom => {
                    let (min_z, _) = rotated_z_extent(asset, full_rot, scale);
                    pos - Vec3::new(0.0, 0.0, min_z)
                }
                Anchor::Top => {
                    let (_, max_z) = rotated_z_extent(asset, full_rot, scale);
                    pos - Vec3::new(0.0, 0.0, max_z)
                }
                Anchor::None | Anchor::HangAware => pos,
            };
            mesh.add_mesh_asset(anchored_pos, draw_rot, scale, asset, color);
            if let Some(emission) = emit {
                emit_light_and_beam(mesh, anchored_pos, draw_rot, &emission);
            }
        }
        Shape::Bar { length, width, height } => {
            mesh.add_bar(pos, rot, length, width, height, color);
            if let Some(emission) = emit {
                emit_light_and_beam(mesh, pos, rot, &emission);
            }
        }
        Shape::Generic => mesh.add_fixture(pos, rot, color),
    }
}

/// A moving head/par's beam travels along its local -Z (this project's own
/// hang/aim convention — see `venue.rs`). Cone length is a fixed reach into
/// the room rather than derived from anything in the data (there's no
/// "throw distance" concept here yet); radius comes from the real
/// fixture's beam angle when known, so a wide-beam wash reads visibly
/// wider than a tight-beam spot.
fn emit_light_and_beam(mesh: &mut MeshBuilder, pos: Vec3, rot: Quat, emission: &LiveEmission) {
    mesh.add_light(pos, emission.color);
    let length = 2.5f32;
    let angle_deg = emission.beam_angle_deg.unwrap_or(25.0);
    let radius = length * (angle_deg.to_radians() * 0.5).tan();
    mesh.add_glow_cone(pos, rot, Vec3::NEG_Z, length, radius.max(0.05), emission.color, 16);
}
