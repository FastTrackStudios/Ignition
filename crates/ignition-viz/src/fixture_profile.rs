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
    ///
    /// `split_z`, when `Some`, is a yoke/head split point in the mesh's own
    /// raw local Z (pre-scale, pre-`pre_rotate`) — a live tilt reading
    /// rotates only the geometry above this point (the head) around it,
    /// instead of the whole mesh rotating around the fixture's mount
    /// anchor. `None` for fixtures that don't tilt (pars, hazers), where
    /// there's nothing to split. See `add_typed_fixture` and
    /// `mesh::add_mesh_asset_split`.
    Mesh { mesh: &'static ObjMesh, target_size: f32, pre_rotate: Quat, anchor: Anchor, split_z: Option<f32> },
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
        // point, so no anchor correction needed. No tilt attribute exists
        // for a par, so no yoke/head split either.
        return Shape::Mesh {
            mesh: par_mesh(),
            target_size: 0.20,
            pre_rotate: Quat::IDENTITY,
            anchor: Anchor::None,
            split_z: None,
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
            split_z: Some(MOVING_HEAD_SPLIT_Z),
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
        // point, so no anchor correction. No tilt channel, no split.
        return Shape::Mesh {
            mesh: par_mesh(),
            target_size: 0.16,
            pre_rotate: Quat::IDENTITY,
            anchor: Anchor::None,
            split_z: None,
        };
    }
    if m == "chauvet" && mo.contains("hurricane") {
        // Chauvet's spec for the Hurricane Haze 1DX: 11 x 6 x 9 in —
        // largest real dimension ~0.28m. Always floor-standing, no hung
        // installation exists for this fixture, so a fixed Bottom anchor
        // (not HangAware — nothing in the data ever "flips" a hazer). No
        // pan/tilt channel, no split.
        return Shape::Mesh {
            mesh: hazer_mesh(),
            target_size: 0.28,
            pre_rotate: Quat::IDENTITY,
            anchor: Anchor::Bottom,
            split_z: None,
        };
    }
    Shape::Generic
}

/// The same mesh-frame correction `add_typed_fixture` folds into a
/// fixture's *actual drawn beam direction* (`head_full_rot = rot * pan *
/// tilt * pre_rotate`, used by `emit_light_and_beam`) — exposed
/// separately so anything computing an aim direction *without* going
/// through `add_typed_fixture` (currently: `ignition_core::focus`'s
/// Focus Point solver, reached via `venue.rs::FixtureRecord::placement`)
/// can target the same real convention instead of a naive `mount_rot *
/// NEG_Z` that ignores it.
///
/// This isn't optional for movers: Norco's real Eos-exported `quat` for
/// every Riukoe/Betopper unit checked is a 180°-class rotation (e.g.
/// `{w:0, x:1, y:0, z:0}`) — the *fixture's own mount data* encodes it
/// mounted "flipped" relative to this project's naive baseline, the same
/// physical fact `moving_head_pre_rotate`'s own doc comment already
/// describes for the mesh ("confirmed backwards... the truss-hung centre
/// OH movers rendered upright... exactly inverted"). A Focus Point
/// solved against `mount_rot` alone is off by that same flip — reported
/// directly as a beam landing "behind the fixture" instead of at its
/// intended target.
pub fn beam_pre_rotate(manufacturer: &str, model: &str) -> Quat {
    match shape_for(manufacturer, model) {
        Shape::Mesh { pre_rotate, .. } => pre_rotate,
        Shape::Bar { .. } | Shape::Generic => Quat::IDENTITY,
    }
}

/// Where the moving-head mesh's yoke/base ends and its head begins, in the
/// mesh's own raw local Z (`assets/qlc-meshes/moving_head.obj`, unscaled,
/// pre-`pre_rotate`) — derived from the mesh's own vertex distribution: a
/// visible "waist" (far fewer vertices) around z≈0 separates a wider
/// cluster from z=-0.49 to -0.1 (yoke arms + base) from a wider cluster
/// from z=0.1 to 0.33 (head/lens housing). Used to decide, per-triangle,
/// whether a live tilt reading should move that geometry — see
/// `Shape::Mesh::split_z`.
const MOVING_HEAD_SPLIT_Z: f32 = -0.02;

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
    Shape::Mesh { mesh, target_size: 0.30, pre_rotate: Quat::IDENTITY, anchor: Anchor::None, split_z: None }
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
/// composed onto `rot` — see `scene.rs`'s live-mode fixture loop) is split
/// into pan and tilt separately: pan rotates the whole fixture (base
/// included — a real yoke rotates on its mount when panning), tilt rotates
/// only the head, pivoting at the shape's own `split_z` (see
/// `mesh::add_mesh_asset_split`) — a real moving head's base stays put
/// while only the head assembly tilts. Shapes with `split_z: None` (pars,
/// hazers — nothing tilts) just get pan folded into the whole mesh's
/// rotation, same as before this split existed.
///
/// `emit`, when present, registers a point light and a beam-cone glow at
/// the fixture's resolved anchor point, aimed along the *head's* full
/// pan+tilt orientation (computed here rather than by the caller since
/// this function is already the one place that knows a fixture's real
/// lens/anchor position after the Bottom/Top anchor maths below). `throw`
/// is the real-world reach that beam-cone should have — see `BeamThrow`.
#[allow(clippy::too_many_arguments)]
pub fn add_typed_fixture(
    mesh: &mut MeshBuilder,
    pos: Vec3,
    rot: Quat,
    live_pan_tilt: Option<(Quat, Quat)>,
    manufacturer: &str,
    model: &str,
    color: [f32; 3],
    emit: Option<LiveEmission>,
    throw: &BeamThrow,
) {
    match shape_for(manufacturer, model) {
        Shape::Mesh { mesh: asset, target_size, pre_rotate, anchor, split_z } => {
            let scale = scale_to(asset, target_size);
            let full_rot = rot * pre_rotate;
            let (pan, tilt) = live_pan_tilt.unwrap_or((Quat::IDENTITY, Quat::IDENTITY));
            // The head's full orientation (pan+tilt) — used for the beam
            // and, when there's no split_z to divide the mesh, for the
            // whole body's drawn rotation too.
            let head_full_rot = rot * pan * tilt * pre_rotate;
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
            match (split_z, live_pan_tilt) {
                // Only pay for the vertex-duplicating split draw when
                // there's an actual live tilt reading to apply — with no
                // live data (`shot`'s headless path, or a live fixture
                // that's simply idle at zero) this must produce the exact
                // same vertex/index buffer as the plain `add_mesh_asset`
                // path below, or `shot`'s regression screenshots stop
                // being byte-identical for no visual reason.
                (Some(split), Some((pan, tilt))) => {
                    let outer_rot = rot * pan;
                    mesh.add_mesh_asset_split(anchored_pos, outer_rot, tilt, pre_rotate, scale, asset, split, color);
                }
                _ => {
                    let draw_rot = rot * pan * pre_rotate;
                    mesh.add_mesh_asset(anchored_pos, draw_rot, scale, asset, color);
                }
            }
            if let Some(emission) = emit {
                emit_light_and_beam(mesh, anchored_pos, head_full_rot, &emission, throw);
            }
        }
        Shape::Bar { length, width, height } => {
            mesh.add_bar(pos, rot, length, width, height, color);
            if let Some(emission) = emit {
                emit_light_and_beam(mesh, pos, rot, &emission, throw);
            }
        }
        Shape::Generic => mesh.add_fixture(pos, rot, color),
    }
}

/// How far a live beam-cone's glow actually reaches — replaces what used
/// to be a flat `2.5` metres regardless of the room's real size
/// ("there's no throw distance concept here yet"). Reported directly:
/// with Norco's real ~3.4m truss height, a fixed 2.5m beam visibly
/// stopped mid-air well short of the floor — "cones that just stop
/// within a few feet." Computed once per `build_scene` call from the
/// venue's own real bounds (`BeamThrow::for_venue`), not per-fixture —
/// the room's geometry doesn't change fixture to fixture. Not a real
/// raycast against the room's actual mesh (walls, risers, screens) —
/// just the flat floor plane, which is the case that mattered here
/// (every downward-aimed beam in this venue) — a real occlusion raycast
/// is a further improvement, not attempted this pass.
///
/// Since then this stops at **any** of the room's six bounding planes,
/// not only the floor. Only intersecting the floor meant every
/// upward-aimed fixture — Norco's uplights, and any mover tilted above
/// horizontal — got a flat 10m beam that punched straight through the
/// ceiling and ended in mid-air, which is exactly what a beam is never
/// allowed to do. It is still an axis-aligned box, not the room's real
/// mesh: a beam still passes through a riser or a hung screen. That
/// remains the further improvement it always was.
#[derive(Clone, Copy)]
pub struct BeamThrow {
    /// The room's bounding box — a beam stops at whichever face it
    /// reaches first, the same way a real beam stops at the surface it
    /// hits instead of passing through it.
    min: Vec3,
    max: Vec3,
    /// The longest throw any beam gets, whatever the geometry says.
    max_reach: f32,
}

impl BeamThrow {
    pub fn for_venue(venue: &crate::venue::Venue) -> Self {
        let (min, max) = venue.bounds();
        let room_diag = ((max.x - min.x).powi(2) + (max.y - min.y).powi(2)).sqrt();
        // The room's own diagonal, not the 10m this used to clamp to.
        // That clamp dates from flat-brightness beam cones, where a long
        // beam was as bright at its far end as at its near end and a
        // roomful of them blew the render out to solid white; with the
        // real falloff curve a beam is down to a few percent long before
        // it crosses the room, so the clamp bought nothing and cost a
        // visible hard edge — every beam that reached neither a surface
        // nor the clamp ended in a rounded cap hanging in mid-air.
        //
        // `bounds()` is a bound on object *centres* (it exists to frame
        // the default cameras), so it does not describe the room's real
        // extent and cannot be relied on to stop every beam — Norco's
        // truss fixtures sit above every room object's centre, so an
        // uplight finds no face at all above it. What actually stops
        // those is the glow pass's depth test against the room geometry:
        // a beam is occluded by the ceiling it runs into whether or not
        // its geometry was clipped there first. The box test below is
        // just a cheap way to not build cone geometry nobody can see.
        Self { min, max, max_reach: room_diag.max(5.0) }
    }

    /// Distance from `origin` along `direction` to the first bounding-box
    /// face — the standard slab test, one axis at a time, keeping the
    /// nearest positive hit. An axis the beam runs parallel to simply
    /// contributes nothing.
    fn reach(&self, origin: Vec3, direction: Vec3) -> f32 {
        let mut nearest = self.max_reach;
        for axis in 0..3 {
            let d = direction[axis];
            if d.abs() < 1e-3 {
                continue;
            }
            let plane = if d < 0.0 { self.min[axis] } else { self.max[axis] };
            let t = (plane - origin[axis]) / d;
            if t > 0.0 && t < nearest {
                nearest = t;
            }
        }
        // A fixture sitting exactly on a bounding face (a floor uplight,
        // a fixture flush to the back wall) would otherwise get a
        // zero-length beam.
        nearest.max(0.3)
    }
}

#[cfg(test)]
mod beam_throw_tests {
    use super::*;

    fn throw() -> BeamThrow {
        BeamThrow { min: Vec3::new(-5.0, -10.0, 0.0), max: Vec3::new(5.0, 10.0, 6.0), max_reach: 10.0 }
    }

    #[test]
    fn a_downward_beam_stops_at_the_floor() {
        let r = throw().reach(Vec3::new(0.0, 0.0, 4.0), Vec3::NEG_Z);
        assert!((r - 4.0).abs() < 1e-4, "{r}");
    }

    #[test]
    fn an_upward_beam_stops_at_the_ceiling_instead_of_in_mid_air() {
        // The regression this test exists for: uplights used to get a
        // flat 10m and end above the roof.
        let r = throw().reach(Vec3::new(0.0, 0.0, 1.0), Vec3::Z);
        assert!((r - 5.0).abs() < 1e-4, "{r}");
    }

    #[test]
    fn a_level_beam_stops_at_the_wall_it_is_aimed_at() {
        let r = throw().reach(Vec3::new(0.0, -8.0, 3.0), Vec3::Y);
        assert!((r - 10.0).abs() < 1e-4, "{r}");
    }

    #[test]
    fn a_long_diagonal_is_still_capped() {
        let r = throw().reach(Vec3::new(-4.0, -9.0, 5.5), Vec3::new(1.0, 2.0, -0.2).normalize());
        assert!(r <= 10.0, "{r}");
    }
}

/// A moving head/par's beam travels along its local -Z (this project's own
/// hang/aim convention — see `venue.rs`). Cone length is `throw`'s real
/// reach for this beam's actual aim direction (see `BeamThrow`); radius
/// comes from the real fixture's beam angle when known, so a wide-beam
/// wash reads visibly wider than a tight-beam spot, scaled by the now
/// much more realistic length.
fn emit_light_and_beam(mesh: &mut MeshBuilder, pos: Vec3, rot: Quat, emission: &LiveEmission, throw: &BeamThrow) {
    let direction = rot * Vec3::NEG_Z;
    let length = throw.reach(pos, direction);
    let half_angle_deg = beam_half_angle_deg(emission.beam_angle_deg);
    let radius = length * half_angle_deg.to_radians().tan();
    mesh.add_light(pos, emission.color, direction, half_angle_deg);
    mesh.add_glow_cone(pos, rot, Vec3::NEG_Z, length, radius.max(0.05), emission.color, half_angle_deg, GLOW_CONE_SEGMENTS);
}

/// The beam **half** angle in degrees, which is what everything
/// downstream wants: the cone's own geometry, the spill light's cone
/// test, and `fs_glow`'s falloff curve. `FixtureRecord::beam_angle_deg`
/// is the full angle, the way a manufacturer quotes it.
///
/// Half, not full, is also what ASLS's shader means by its `angle`
/// attribute — `MovingHead.set angle()` stores `angle / 2` before it
/// reaches the buffer, so their `radians(vAngle) * distance * distance`
/// term is a half-angle. Feeding it the full angle (as the first pass at
/// this port did) doubles the quadratic term and makes every beam die
/// out at roughly half the distance it should.
///
/// Zero and negative are treated as "not known" rather than "a beam with
/// no spread": Norco's real patch has fifteen fixtures carrying 0.0 or a
/// near-zero angle, which produced 5cm-wide pencil beams with no
/// distance falloff at all (`radians(0) * d * d` is zero, so nothing in
/// the curve ever dimmed them) — the hard white streaks visible through
/// the middle of the rig.
pub(crate) fn beam_half_angle_deg(full_angle_deg: Option<f32>) -> f32 {
    /// ASLS's own `BEAM_MAX_ANGLE`, in the same half-angle terms.
    const MAX_HALF_ANGLE_DEG: f32 = 45.0;
    /// ASLS's fallback is a 10 degree half angle (`get angle()`); this
    /// crate's has always been 25 degrees full, which is the same order.
    const DEFAULT_FULL_ANGLE_DEG: f32 = 25.0;

    let full = match full_angle_deg {
        Some(a) if a > 0.1 => a,
        _ => DEFAULT_FULL_ANGLE_DEG,
    };
    (full * 0.5).min(MAX_HALF_ANGLE_DEG)
}

#[cfg(test)]
mod beam_angle_tests {
    use super::beam_half_angle_deg;

    #[test]
    fn a_manufacturer_full_angle_becomes_a_half_angle() {
        assert_eq!(beam_half_angle_deg(Some(15.0)), 7.5);
    }

    #[test]
    fn a_missing_or_zero_angle_falls_back_rather_than_making_a_pencil_beam() {
        // Norco's patch carries both of these for real.
        assert_eq!(beam_half_angle_deg(None), 12.5);
        assert_eq!(beam_half_angle_deg(Some(0.0)), 12.5);
    }

    #[test]
    fn an_absurd_angle_is_clamped_the_way_asls_clamps_its_own() {
        assert_eq!(beam_half_angle_deg(Some(360.0)), 45.0);
    }
}

/// Radial segments per beam cone. Sixteen left a visibly faceted
/// silhouette once beams got large enough to fill much of the frame;
/// beams are now two rings deep instead of seven (see
/// `mesh::GLOW_CONE_RING_STEPS`), so this is cheaper in total vertices
/// than the faceted version was. ASLS uses 100 on a single instanced
/// cylinder, which is free for them and would not be here.
pub(crate) const GLOW_CONE_SEGMENTS: u32 = 48;
