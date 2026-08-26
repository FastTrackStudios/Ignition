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

use crate::obj_mesh::ObjMesh;
use bevy::math::{Quat, Vec3};
use std::sync::OnceLock;

macro_rules! qlc_mesh {
    ($name:ident, $file:literal) => {
        fn $name() -> &'static ObjMesh {
            static CELL: OnceLock<ObjMesh> = OnceLock::new();
            CELL.get_or_init(|| {
                ObjMesh::parse(include_str!(concat!("../assets/qlc-meshes/", $file)))
            })
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
    Mesh {
        mesh: &'static ObjMesh,
        target_size: f32,
        pre_rotate: Quat,
        anchor: Anchor,
        split_z: Option<f32>,
    },
    /// An LED bar/batten: one elongated box, no beam (washes along its
    /// length rather than from a point) — no QLC+ mesh fits this shape.
    Bar {
        length: f32,
        width: f32,
        height: f32,
    },
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
        return Shape::Bar {
            length: 1.02,
            width: 0.067,
            height: 0.065,
        };
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
    Shape::Mesh {
        mesh,
        target_size: 0.30,
        pre_rotate: Quat::IDENTITY,
        anchor: Anchor::None,
        split_z: None,
    }
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
/// What one fixture actually looks like once its profile, its mount pose
/// and any live pan/tilt have all been resolved — a plain description,
/// with no renderer types in it.
///
/// This used to be a function that pushed triangles into a mesh builder.
/// Separating the decisions from the drawing is what lets `spawn.rs` turn
/// a fixture into an entity hierarchy (body, and a head child that tilts
/// under it) instead of into a flat vertex buffer, and it makes every
/// rule below — the anchor maths especially — testable without a GPU.
pub struct FixtureVisual {
    pub body: BodyVisual,
    /// Where the body is drawn, after the Bottom/Top anchor correction.
    pub position: Vec3,
    /// The body's own rotation: mount pose plus live pan. A real yoke
    /// rotates on its mount when panning, so pan belongs here.
    pub body_rot: Quat,
    /// The head's rotation *relative to the body* — live tilt, and
    /// nothing else. `None` for a fixture with nothing that tilts (a par,
    /// a hazer), where the whole body is the head.
    pub head_tilt: Option<Quat>,
    /// Where the head pivots, in the body's own local space — a real
    /// moving head's base stays put while only the head assembly tilts.
    pub head_pivot: Option<Vec3>,
    pub beam: Option<BeamVisual>,
}

pub enum BodyVisual {
    /// A QLC+ category mesh and the uniform scale that brings it to the
    /// real fixture's size. `pre_rotate` is baked into the body/head
    /// rotations already; `split_z` is the head/yoke divide in the mesh's
    /// own local Z, `None` when the shape has no moving head.
    Mesh {
        asset: &'static ObjMesh,
        scale: f32,
        pre_rotate: Quat,
        split_z: Option<f32>,
    },
    /// An LED bar/batten: one elongated box, no beam (it washes along its
    /// length rather than from a point).
    Bar {
        length: f32,
        width: f32,
        height: f32,
    },
    /// No dedicated profile — the generic marker.
    Generic,
}

/// A beam ready to draw: where it starts, where it goes, how far it gets
/// and how wide it is by the time it arrives.
#[derive(Clone, Copy, Debug)]
pub struct BeamVisual {
    /// World position of the lens.
    pub origin: Vec3,
    /// Normalized world aim direction.
    pub direction: Vec3,
    /// Real throw distance for this aim (see `BeamThrow`).
    pub length: f32,
    /// Cone radius at that distance.
    pub far_radius: f32,
    /// Beam **half** angle in degrees — see `beam_half_angle_deg`.
    pub half_angle_deg: f32,
    /// Already dimmer-scaled.
    pub color: [f32; 3],
}

/// `rot` is always the fixed *mount* rotation — it alone decides the
/// Bottom/Top anchor (a mechanical fact of how the fixture is rigged, not
/// something pan/tilt changes). `live_pan_tilt`, when present, is a live
/// reading split into pan and tilt separately, because they act on
/// different parts of the fixture (see `FixtureVisual`).
#[allow(clippy::too_many_arguments)]
pub fn resolve_fixture(
    pos: Vec3,
    rot: Quat,
    live_pan_tilt: Option<(Quat, Quat)>,
    manufacturer: &str,
    model: &str,
    emit: Option<LiveEmission>,
    throw: &BeamThrow,
) -> FixtureVisual {
    let (pan, tilt) = live_pan_tilt.unwrap_or((Quat::IDENTITY, Quat::IDENTITY));

    match shape_for(manufacturer, model) {
        Shape::Mesh {
            mesh: asset,
            target_size,
            pre_rotate,
            anchor,
            split_z,
        } => {
            let scale = scale_to(asset, target_size);
            let full_rot = rot * pre_rotate;
            // The head's full orientation (pan + tilt) — what the beam is
            // aimed along.
            let head_full_rot = rot * pan * tilt * pre_rotate;
            let resolved = match anchor {
                Anchor::HangAware => {
                    if is_mounted_upright(rot) {
                        Anchor::Bottom
                    } else {
                        Anchor::Top
                    }
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
            FixtureVisual {
                body: BodyVisual::Mesh {
                    asset,
                    scale,
                    pre_rotate,
                    split_z,
                },
                position: anchored_pos,
                body_rot: rot * pan,
                head_tilt: split_z.map(|_| tilt),
                head_pivot: split_z.map(|z| Vec3::new(0.0, 0.0, z * scale)),
                beam: emit.map(|e| beam_visual(anchored_pos, head_full_rot, &e, throw)),
            }
        }
        Shape::Bar {
            length,
            width,
            height,
        } => FixtureVisual {
            body: BodyVisual::Bar {
                length,
                width,
                height,
            },
            position: pos,
            body_rot: rot,
            head_tilt: None,
            head_pivot: None,
            beam: emit.map(|e| beam_visual(pos, rot, &e, throw)),
        },
        Shape::Generic => FixtureVisual {
            body: BodyVisual::Generic,
            position: pos,
            body_rot: rot,
            head_tilt: None,
            head_pivot: None,
            beam: None,
        },
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
    /// Sized by the room's surfaces (`Venue::room_extent`), padded so a
    /// fixture flush to a wall still has somewhere to throw, and capped
    /// at the room's full diagonal — so no beam can end before the
    /// surface it is aimed at. `bounds()` used to size this, and it is a
    /// bound on object *centres*: a beam aimed at the far wall stopped
    /// at the centre of the audience floor, with its cap hanging in the
    /// air ("lights just stop in mid air or get cut off").
    // r[impl viz.beam-reach] - a beam reaches the room's surface, never short of it
    pub fn for_venue(venue: &crate::venue::Venue) -> Self {
        let (min, max) = venue.room_extent();
        let pad = Vec3::splat(0.5);
        let (min, max) = (min - pad, max + pad);
        let room_diag = (max - min).length();
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
        Self {
            min,
            max,
            max_reach: room_diag.max(5.0),
        }
    }

    /// How far a fixture's spill light is allowed to reach: comfortably
    /// past every surface in the room. Bevy's range is a hard cutoff on
    /// both the lit surfaces and the volumetric shaft — `length * 1.2`
    /// ended shafts short of the floor when the throw itself was short,
    /// and 40 was fine for one room and wrong for the next.
    // r[impl viz.beam-reach] - spill is cut by the walls, not by its range
    pub fn spill_range(&self) -> f32 {
        self.max_reach * 1.5
    }

    /// Distance from `origin` along `direction` to the first bounding-box
    /// face — the standard slab test, one axis at a time, keeping the
    /// nearest positive hit. An axis the beam runs parallel to simply
    /// contributes nothing.
    pub fn reach(&self, origin: Vec3, direction: Vec3) -> f32 {
        let mut nearest = self.max_reach;
        for axis in 0..3 {
            let d = direction[axis];
            if d.abs() < 1e-3 {
                continue;
            }
            let plane = if d < 0.0 {
                self.min[axis]
            } else {
                self.max[axis]
            };
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
        BeamThrow {
            min: Vec3::new(-5.0, -10.0, 0.0),
            max: Vec3::new(5.0, 10.0, 6.0),
            max_reach: 10.0,
        }
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

    /// r[verify viz.beam-reach] - the spill's range is past the longest throw
    #[test]
    fn the_spill_range_is_comfortably_past_the_longest_throw() {
        let t = throw();
        assert!(t.spill_range() > t.max_reach);
        let (a, b) = (Vec3::new(-4.0, -9.0, 5.5), Vec3::new(1.0, 2.0, -0.2).normalize());
        assert!(t.spill_range() > t.reach(a, b));
    }

    #[test]
    fn a_long_diagonal_is_still_capped() {
        let r = throw().reach(
            Vec3::new(-4.0, -9.0, 5.5),
            Vec3::new(1.0, 2.0, -0.2).normalize(),
        );
        assert!(r <= 10.0, "{r}");
    }
}

/// A moving head/par's beam travels along its local -Z (this project's own
/// hang/aim convention — see `venue.rs`). Cone length is `throw`'s real
/// reach for this beam's actual aim direction (see `BeamThrow`); radius
/// comes from the real fixture's beam angle when known, so a wide-beam
/// wash reads visibly wider than a tight-beam spot, scaled by the now
/// much more realistic length.
fn beam_visual(pos: Vec3, rot: Quat, emission: &LiveEmission, throw: &BeamThrow) -> BeamVisual {
    let direction = (rot * Vec3::NEG_Z).normalize_or_zero();
    let length = throw.reach(pos, direction);
    let half_angle_deg = beam_half_angle_deg(emission.beam_angle_deg);
    BeamVisual {
        origin: pos,
        direction,
        length,
        far_radius: (length * half_angle_deg.to_radians().tan()).max(0.05),
        half_angle_deg,
        color: emission.color,
    }
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

/// Assumed luminous efficacy for these fixtures' LEDs, in lumens per
/// watt. An assumption, stated as one: manufacturers of gear at this
/// price point publish wattage and almost never publish lumens, and the
/// ones that do are not measuring the same way. Mid-range for RGB LED
/// stage fixtures.
///
/// Only the *ratio* between fixtures really matters here — the absolute
/// level is set by the exposure dial — and wattage captures that ratio
/// far better than the single global number this replaced.
pub const LUMENS_PER_WATT: f32 = 38.0;

/// The fixture's real electrical power draw in watts, from the
/// manufacturer's own spec. Same sourcing rule as `shape_for`'s
/// dimensions: a real published figure or nothing.
///
/// This exists because luminous output is what decides whether a fixture
/// cuts a visible shaft through haze. Bevy takes a spot light's output
/// in lumens and divides by the cone's solid angle to get its actual
/// radiance, so a wide wash and a narrow beam with the *same* lumens
/// differ enormously in how bright the air along the beam gets. Giving
/// every fixture the same output — which this renderer did — makes a par
/// carve the same beam as a 1.72-degree beam fixture, which is not what
/// either of them does.
pub fn power_watts(manufacturer: &str, model: &str) -> f32 {
    let m = manufacturer.to_ascii_lowercase();
    let mo = model.to_ascii_lowercase();

    // Open Fixture Library's U`King Par Light B262: 36W LED.
    if m == "uking" && mo.contains("par") {
        return 36.0;
    }
    // Betopper LB150: 150W, and genuinely a 1.72-degree beam — the
    // patch's suspiciously tight angle turned out to be the real spec.
    if m == "betopper" {
        return 150.0;
    }
    // Riukoe/Lixada mini gobo moving head — sold as 30W.
    if m == "riukoe" || m == "lixada" {
        return 30.0;
    }
    // Chauvet SlimPAR Tri 7 IRC: 7 x 3W tri-colour LEDs.
    if m == "chauvet" && mo.contains("slimpar") {
        return 21.0;
    }
    // Rockville Rockstrip 252: "Max power: 50VA" per Rockville's own
    // listing — not the 126W a 252 x 0.5W LED count would suggest.
    //
    // They also publish 1,452 lux at 1m, which is a directly measured
    // 1,452 cd. That is two orders of magnitude below
    // `SHAFT_CANDELA_THRESHOLD`, so a Rockstrip is unambiguously a wash:
    // it lights what it is aimed at and puts nothing visible in the air.
    // Worth stating because the patch carries 0.0 for its beam angle, so
    // it falls back to a generic 25 degrees, and at the wattage this
    // used to claim that fallback made every strip cut a huge cone
    // across the stage.
    if m == "rockville" && mo.contains("rockstrip") {
        return 50.0;
    }
    // A hazer is not a light source. It emits haze, which is a property
    // of the room the fog volume already models — giving it an output
    // made it shine a white beam it does not have.
    if mo.contains("haze") || mo.contains("hazer") || mo.contains("fog") {
        return 0.0;
    }
    // Unknown fixture: a mid-sized LED par's worth, so it reads as
    // present rather than either invisible or dominant.
    40.0
}

/// Widest a field half-angle is allowed to be: a Bevy spot light's
/// outer cone stops just short of a hemisphere.
pub const MAX_FIELD_HALF_ANGLE_DEG: f32 = 80.0;

/// A fixture's field half-angle when nothing states one: twice the beam,
/// where an LED par's 10% edge typically sits relative to its 50% edge.
// r[impl viz.profile-optics] - field is 2x beam when nothing states one
pub fn assumed_field_half_angle_deg(beam_half_deg: f32) -> f32 {
    (beam_half_deg * 2.0).min(MAX_FIELD_HALF_ANGLE_DEG)
}

/// Peak luminous intensity in candela — lumens spread over the cone's
/// solid angle. This, not raw output, is what decides whether a fixture
/// cuts a visible shaft through haze: it is brightness *per direction*,
/// so a narrow beam and a wide wash of the same wattage differ by orders
/// of magnitude.
pub fn peak_candela(lumens: f32, half_angle_deg: f32) -> f32 {
    let solid_angle = core::f32::consts::TAU * (1.0 - half_angle_deg.to_radians().cos());
    lumens / solid_angle.max(1e-6)
}

/// Above this many candela a fixture is bright enough per-direction to
/// light the haze it passes through into a visible shaft; below it, it
/// simply lights whatever it is pointed at.
///
/// Calibrated against the operator's own observation of this rig: "the
/// pars with a single one on I never see a cone of light, just an area
/// gets lit up." Their 36W 30-degree pars land near 6,400 cd and the
/// Chauvet SlimPARs near 8,400, while the mini gobo movers are around
/// 39,000 and the 1.72-degree Betopper beams are past 8,000,000.
///
/// It sat at 20,000 — above every par — on that observation. Lowered to
/// 5,000 on the next: the house view showed the pars' pools on the
/// stage and nothing in the air, and no amount of haze could change
/// that, because below the threshold a par gets no volumetric light at
/// all. Now a 36W par cuts a faint cone that the haze dial scales, and
/// a hazer-less room can still be had with `--haze 0`.
pub const SHAFT_CANDELA_THRESHOLD: f32 = 5_000.0;

/// Radial segments per beam cone — Bevy's `ConicalFrustum` mesher calls
/// them "resolution". Sixteen left a visibly faceted silhouette once
/// beams got large enough to fill much of the frame. ASLS uses 100 on
/// their one instanced cylinder; here every distinct (radius, length)
/// pair is its own mesh asset, so it is not quite free.
pub const BEAM_CONE_SEGMENTS: u32 = 48;

// --- Emitters -------------------------------------------------------------

/// The colour emitters a fixture type mixes with, in the order its
/// `ChannelMap` lists them — what `ignition_core::color::solve` needs to
/// turn a colour intent into this fixture's levels
/// (`show.rs::apply_cue_output`).
///
/// A real GDTF profile carries measured chromaticities
/// (`gdtf_import::import_emitters`); every hand-authored map in
/// `channel_map.rs` gets `typical_emitter`'s class-of-LED values instead,
/// which are close enough that a pale colour on an RGBW par lands on its
/// white and a saturated one on its primaries.
#[derive(Debug, Clone, PartialEq)]
// r[impl color.emitter-solve] - the fixture type's emitter data
pub struct FixtureEmitters {
    pub channels: Vec<(ignition_proto::ColorChannel, ignition_core::color::Emitter)>,
}

/// A typical LED of this colour: chromaticity from the common
/// stage-LED bins (red 625 nm, green 525 nm, blue 465 nm, amber 595 nm,
/// 400 nm UV, a lime phosphor, a ~5600 K white), output relative to the
/// white.
pub fn typical_emitter(channel: ignition_proto::ColorChannel) -> ignition_core::color::Emitter {
    use ignition_core::color::emitter;
    use ignition_proto::ColorChannel::*;
    match channel {
        Red => emitter("Red", 0.690, 0.310, 0.25),
        Green => emitter("Green", 0.200, 0.700, 0.60),
        Blue => emitter("Blue", 0.140, 0.060, 0.10),
        White => emitter("White", 0.330, 0.340, 1.00),
        Amber => emitter("Amber", 0.580, 0.415, 0.40),
        Uv => emitter("UV", 0.170, 0.010, 0.02),
        Lime => emitter("Lime", 0.380, 0.560, 0.80),
    }
}

impl FixtureEmitters {
    /// Every `ColorAdd` channel in `map`, with a typical emitter for
    /// each — `None` for a fixture with no additive colour at all (a
    /// colour-wheel mover, a hazer).
    pub fn from_channel_map(map: &ignition_proto::ChannelMap) -> Option<FixtureEmitters> {
        let channels: Vec<_> = map
            .channels
            .iter()
            .filter_map(|(_, attr)| match attr {
                ignition_proto::Attribute::ColorAdd { channel } => {
                    Some((*channel, typical_emitter(*channel)))
                }
                _ => None,
            })
            .collect();
        (!channels.is_empty()).then_some(FixtureEmitters { channels })
    }

    /// Whether solving is worth it: any emitter that is not one of the
    /// three the RGB triple already addresses directly.
    pub fn beyond_rgb(&self) -> bool {
        use ignition_proto::ColorChannel::*;
        self.channels
            .iter()
            .any(|(c, _)| !matches!(c, Red | Green | Blue))
    }

    pub fn emitters(&self) -> Vec<ignition_core::color::Emitter> {
        self.channels.iter().map(|(_, e)| e.clone()).collect()
    }

    /// Levels per channel for `intent`, in `channels` order.
    // r[impl color.emitter-solve]
    pub fn solve(
        &self,
        intent: &ignition_core::color::Intent,
        quality: f32,
    ) -> Vec<(ignition_proto::ColorChannel, f32)> {
        let levels = ignition_core::color::solve(intent, &self.emitters(), quality);
        self.channels
            .iter()
            .zip(levels)
            .map(|((c, _), v)| (*c, v))
            .collect()
    }
}

#[cfg(test)]
mod emitter_tests {
    use super::*;
    use ignition_core::color::{Intent, Rgb};
    use ignition_proto::ColorChannel;

    #[test]
    /// r[verify color.emitter-solve] - an RGBW map yields four emitters, an RGB map three
    fn emitters_follow_the_channel_map() {
        let rgbw = crate::channel_map::channel_map_for("x", "RGBW Spot Light 6ch").unwrap();
        let e = FixtureEmitters::from_channel_map(&rgbw).unwrap();
        assert_eq!(e.channels.len(), 4);
        assert!(e.beyond_rgb());
        let par = crate::channel_map::channel_map_for("Uking", "Par").unwrap();
        let e = FixtureEmitters::from_channel_map(&par).unwrap();
        assert_eq!(e.channels.len(), 3);
        assert!(!e.beyond_rgb());
        let mover = crate::channel_map::channel_map_for("Betopper", "150W Beam").unwrap();
        assert!(FixtureEmitters::from_channel_map(&mover).is_none());
    }

    #[test]
    /// r[verify color.cct] - white on an RGBW fixture goes to the white emitter
    fn white_on_rgbw_uses_the_white() {
        let rgbw = crate::channel_map::channel_map_for("x", "RGBW Spot Light 6ch").unwrap();
        let e = FixtureEmitters::from_channel_map(&rgbw).unwrap();
        let levels = e.solve(&Intent::Rgb(Rgb::WHITE), 0.5);
        let w = levels
            .iter()
            .find(|(c, _)| *c == ColorChannel::White)
            .map(|(_, v)| *v)
            .unwrap();
        assert!(w > 0.5, "{levels:?}");
    }
}

// --- Fixture profile: everything output needs to know about a type ------

/// One slot on a colour wheel: the byte that selects it and the colour it
/// makes. The colour is an `Intent` so the nearest-slot search
/// (`ignition_core::color::nearest_wheel_slot`) can compare it with the
/// preset's intent in xy — a cheap mover's gel-like wheel is a list of
/// approximate chromaticities, a GDTF wheel a list of measured ones.
// r[impl color.mix-or-wheel] - the per-model slot table
#[derive(Debug, Clone, PartialEq)]
pub struct ColorWheelSlot {
    pub name: String,
    pub byte: u8,
    pub color: ignition_core::color::Intent,
}

impl ColorWheelSlot {
    pub fn xy(name: &str, byte: u8, x: f32, y: f32) -> Self {
        Self {
            name: name.to_string(),
            byte,
            color: ignition_core::color::Intent::Xy {
                x,
                y,
                luminance: 1.0,
            },
        }
    }
}

/// The colour space a fixture type's RGB-shaped channels are declared
/// in: one of the spaces the core names, or GDTF's own primaries (a
/// `Custom` space, or the ProPhoto / ANSI E1.54 presets), which the core
/// does not carry by name and so are converted here from their xy.
// r[impl color.spaces] - the fixture type's GDTF colour space
#[derive(Debug, Clone, PartialEq)]
pub enum DeclaredColorSpace {
    Known(ignition_core::color::ColorSpace),
    Primaries {
        red: (f32, f32),
        green: (f32, f32),
        blue: (f32, f32),
        white: (f32, f32),
    },
}

impl Default for DeclaredColorSpace {
    fn default() -> Self {
        DeclaredColorSpace::Known(ignition_core::color::ColorSpace::Srgb)
    }
}

impl DeclaredColorSpace {
    /// `intent`, with an RGB triple re-read in this space. Anything but
    /// an `Rgb` is already space-independent and passes through; an sRGB
    /// space passes an `Rgb` through unchanged too.
    // r[impl color.spaces] - an RGB intent is interpreted in the fixture's space
    pub fn interpret(&self, intent: &ignition_core::color::Intent) -> ignition_core::color::Intent {
        use ignition_core::color::{ColorSpace, Intent, Xyz};
        let Intent::Rgb(rgb) = intent else {
            return intent.clone();
        };
        let triple = [rgb.red, rgb.green, rgb.blue];
        match self {
            DeclaredColorSpace::Known(ColorSpace::Srgb) => intent.clone(),
            DeclaredColorSpace::Known(space) => Intent::from_space(*space, triple),
            DeclaredColorSpace::Primaries {
                red,
                green,
                blue,
                white,
            } => {
                // Standard RGB->XYZ from primaries: columns are the
                // primaries' XYZ (at Y-free scale) scaled so the white
                // point comes out at Y = 1.
                let col = |(x, y): (f32, f32)| [x / y, 1.0, (1.0 - x - y) / y];
                let [r, g, b] = [col(*red), col(*green), col(*blue)];
                let w = col(*white);
                // Solve [r g b] * s = w for the per-primary scales.
                let m = [[r[0], g[0], b[0]], [r[1], g[1], b[1]], [r[2], g[2], b[2]]];
                let s = solve3(m, w).unwrap_or([1.0, 1.0, 1.0]);
                let [rr, gg, bb] = triple;
                let xyz = Xyz {
                    x: r[0] * s[0] * rr + g[0] * s[1] * gg + b[0] * s[2] * bb,
                    y: r[1] * s[0] * rr + g[1] * s[1] * gg + b[1] * s[2] * bb,
                    z: r[2] * s[0] * rr + g[2] * s[1] * gg + b[2] * s[2] * bb,
                };
                let xyy = xyz.to_xyy();
                Intent::Xy {
                    x: xyy.x,
                    y: xyy.y,
                    luminance: xyy.luminance,
                }
            }
        }
    }
}

/// Cramer's rule for a 3x3 system; `None` when singular.
fn solve3(m: [[f32; 3]; 3], v: [f32; 3]) -> Option<[f32; 3]> {
    let det = |m: [[f32; 3]; 3]| {
        m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
            - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
    };
    let d = det(m);
    if d.abs() < 1e-9 {
        return None;
    }
    let mut out = [0.0; 3];
    for (i, slot) in out.iter_mut().enumerate() {
        let mut mi = m;
        for row in 0..3 {
            mi[row][i] = v[row];
        }
        *slot = det(mi) / d;
    }
    Some(out)
}

/// A fixture type as the output stage sees it: its channel layout, its
/// colour system (emitters, wheel, which of the two a preset lands on,
/// the space its RGB channels are declared in) and the value every
/// attribute rests at.
// r[impl color.mix-or-wheel] - preference per fixture type
// r[impl color.spaces] - the fixture type's declared colour space
// r[impl playback.defaults] - every attribute has a default
#[derive(Debug, Clone, PartialEq)]
pub struct FixtureProfile {
    pub map: ignition_proto::ChannelMap,
    pub emitters: Option<FixtureEmitters>,
    pub wheel: Vec<ColorWheelSlot>,
    pub color_preference: ignition_core::color::ColorPreference,
    pub color_space: DeclaredColorSpace,
    pub defaults: std::collections::HashMap<ignition_proto::Attribute, f32>,
}

impl FixtureProfile {
    /// A profile from a channel map alone: emitters from the map's colour
    /// channels (`typical_emitter`), no wheel table, mixing preferred where
    /// mixing exists and the wheel otherwise, sRGB, and `rest_defaults`.
    pub fn from_channel_map(map: ignition_proto::ChannelMap) -> Self {
        let emitters = FixtureEmitters::from_channel_map(&map);
        let defaults = rest_defaults(&map);
        Self {
            color_preference: preference_for(emitters.is_some(), false),
            emitters,
            wheel: Vec::new(),
            color_space: DeclaredColorSpace::default(),
            defaults,
            map,
        }
    }

    /// The same profile with a wheel table, re-deciding the preference
    /// now that there is a wheel to prefer.
    pub fn with_wheel(mut self, wheel: Vec<ColorWheelSlot>) -> Self {
        self.color_preference = preference_for(self.emitters.is_some(), !wheel.is_empty());
        self.wheel = wheel;
        self
    }

    /// The wheel as `nearest_wheel_slot` wants it.
    pub fn wheel_slots(&self) -> Vec<(u8, ignition_core::color::Intent)> {
        self.wheel
            .iter()
            .map(|s| (s.byte, s.color.clone()))
            .collect()
    }

    /// Whether this type can take a colour at all, one way or the other.
    pub fn has_color(&self) -> bool {
        self.emitters.is_some() || !self.wheel.is_empty()
    }
}

/// Mixing where mixing exists, the wheel otherwise; a type with neither
/// keeps the default (`Mix`) and never gets asked.
// r[impl color.mix-or-wheel] - the default preference
pub fn preference_for(mixes: bool, has_wheel: bool) -> ignition_core::color::ColorPreference {
    use ignition_core::color::ColorPreference;
    if !mixes && has_wheel {
        ColorPreference::Wheel
    } else {
        ColorPreference::Mix
    }
}

/// The rest value of every attribute a channel map carries, in the cue
/// engine's own units: dimmer, strobe and colour off, pan and tilt at 0°
/// (centre), zoom and focus mid-travel, iris open, wheels at their first
/// slot. Fine channels are never programmed and get no default.
// r[impl playback.defaults] - a sensible rest where the fixture type says nothing
pub fn rest_defaults(
    map: &ignition_proto::ChannelMap,
) -> std::collections::HashMap<ignition_proto::Attribute, f32> {
    use ignition_proto::Attribute::*;
    map.channels
        .iter()
        .filter_map(|(_, attr)| {
            let value = match attr {
                Dimmer | Strobe | ColorAdd { .. } | ColorWheel { .. } | GoboWheel { .. } => 0.0,
                Pan | Tilt => 0.0,
                Zoom | Focus => 0.5,
                Iris => 1.0,
                Custom(_) => 0.0,
                PanFine | TiltFine => return None,
            };
            Some((attr.clone(), value))
        })
        .collect()
}

#[cfg(test)]
mod profile_tests {
    use super::*;
    use ignition_core::color::ColorPreference;
    use ignition_proto::Attribute;

    /// r[verify playback.defaults] - every attribute a map carries rests somewhere sensible
    #[test]
    fn rest_defaults_cover_every_programmed_attribute() {
        let map = ignition_proto::ChannelMap::new(
            8,
            vec![
                (0, Attribute::Pan),
                (1, Attribute::PanFine),
                (2, Attribute::Tilt),
                (3, Attribute::Dimmer),
                (4, Attribute::Zoom),
                (5, Attribute::Iris),
                (6, Attribute::Strobe),
                (7, Attribute::ColorWheel { slot: 0 }),
            ],
        );
        let d = rest_defaults(&map);
        assert_eq!(d[&Attribute::Pan], 0.0);
        assert_eq!(d[&Attribute::Zoom], 0.5);
        assert_eq!(d[&Attribute::Iris], 1.0);
        assert_eq!(d[&Attribute::Strobe], 0.0);
        assert_eq!(d[&Attribute::Dimmer], 0.0);
        assert!(
            !d.contains_key(&Attribute::PanFine),
            "fine channels are derived"
        );
        assert_eq!(d.len(), 7);
    }

    /// r[verify color.mix-or-wheel] - the default preference follows what the type has
    #[test]
    fn preference_defaults_to_mix_where_mixing_exists_and_wheel_otherwise() {
        assert_eq!(preference_for(true, true), ColorPreference::Mix);
        assert_eq!(preference_for(true, false), ColorPreference::Mix);
        assert_eq!(preference_for(false, true), ColorPreference::Wheel);
        let rgbw = crate::channel_map::channel_map_for("x", "RGBW Spot Light 6ch").unwrap();
        let p = FixtureProfile::from_channel_map(rgbw);
        assert_eq!(p.color_preference, ColorPreference::Mix);
        let mover = crate::channel_map::channel_map_for("Betopper", "150W Beam").unwrap();
        let p = FixtureProfile::from_channel_map(mover)
            .with_wheel(vec![ColorWheelSlot::xy("Red", 20, 0.68, 0.31)]);
        assert_eq!(p.color_preference, ColorPreference::Wheel);
        assert_eq!(p.wheel_slots()[0].0, 20);
    }

    /// r[verify color.spaces] - the same triple means a different colour in a wider space
    #[test]
    fn an_rgb_intent_is_read_in_the_declared_space() {
        use ignition_core::color::{ColorSpace, Intent, Rgb};
        let red = Intent::Rgb(Rgb::new(1.0, 0.0, 0.0));
        assert_eq!(DeclaredColorSpace::default().interpret(&red), red);
        let wide = DeclaredColorSpace::Known(ColorSpace::Rec2020).interpret(&red);
        let Intent::Xy { x, .. } = wide else {
            panic!("{wide:?}")
        };
        assert!(x > 0.7, "Rec.2020 red is further out than sRGB's 0.64: {x}");
        // sRGB spelled out as primaries lands where the core's sRGB does.
        let srgb_primaries = DeclaredColorSpace::Primaries {
            red: (0.64, 0.33),
            green: (0.30, 0.60),
            blue: (0.15, 0.06),
            white: (0.3127, 0.3290),
        };
        let teal = Intent::Rgb(Rgb::new(0.2, 0.7, 0.6));
        let a = srgb_primaries.interpret(&teal).xyy().unwrap();
        let b = teal.xyy().unwrap();
        assert!(
            (a.x - b.x).abs() < 2e-3 && (a.y - b.y).abs() < 2e-3,
            "{a:?} vs {b:?}"
        );
        assert!((a.luminance - b.luminance).abs() < 2e-2, "{a:?} vs {b:?}");
        // A CCT is not a triple and is never reinterpreted.
        let cct = Intent::Cct {
            kelvin: 3200.0,
            tint: 0.0,
        };
        assert_eq!(srgb_primaries.interpret(&cct), cct);
    }
}
