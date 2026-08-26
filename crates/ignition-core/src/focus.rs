//! Pure pan/tilt-to-aim-at-a-point math — what a grandMA3-style "Focus
//! Point" preset actually computes: given a fixture's real hung position
//! and mount orientation (`ignition_proto::Placement` — already exists,
//! extracted from the live rig), solve for the Pan/Tilt values that aim its
//! beam at an arbitrary XYZ location in the room. This is the one preset
//! type this project can do *better* than a flat DMX-only cue engine,
//! since real 3D venue geometry already exists here (`docs/domain/norco-
//! venue-reference.md`) — most budget consoles don't have it at all.
//!
//! No fixture/DMX/rendering knowledge — plain vector/quaternion math over
//! `ignition_proto`'s own f64 `Vec3`/`Quat` (not `glam`, so `ignition-core`
//! stays free of a rendering-crate dependency; see `lib.rs`'s `no_std`
//! aspiration). Targets the *canonical* pan/tilt convention
//! `mount_rot * RotZ(pan) * RotX(tilt) * NEG_Z` — the same one
//! `dmx.rs::resolve()` decodes DMX bytes into and `gdtf_geometry.rs`'s
//! kinematic-chain drawing uses (its own doc: "GDTF's own coordinate
//! convention turned out to already agree" with local -Z as the beam
//! direction). Deliberately does *not* include `fixture_profile.rs`'s
//! `moving_head_pre_rotate` — that is a QLC+-placeholder-mesh-authoring
//! correction specific to one rendering asset, not a statement about the
//! real physical/DMX convention a focus-point calculation should target.

use ignition_proto::{Quat, Vec3};

fn v_sub(a: Vec3, b: Vec3) -> Vec3 {
    Vec3 {
        x: a.x - b.x,
        y: a.y - b.y,
        z: a.z - b.z,
    }
}

fn v_len(a: Vec3) -> f64 {
    (a.x * a.x + a.y * a.y + a.z * a.z).sqrt()
}

fn v_normalize(a: Vec3) -> Vec3 {
    let len = v_len(a);
    if len > 1e-9 {
        Vec3 {
            x: a.x / len,
            y: a.y / len,
            z: a.z / len,
        }
    } else {
        a
    }
}

fn q_conjugate(q: Quat) -> Quat {
    Quat {
        w: q.w,
        x: -q.x,
        y: -q.y,
        z: -q.z,
    }
}

/// Rotates `v` by unit quaternion `q` — the standard `2*dot(u,v)*u +
/// (s^2 - dot(u,u))*v + 2*s*cross(u,v)` expansion of `q * v * q^-1`.
fn q_rotate(q: Quat, v: Vec3) -> Vec3 {
    let u = Vec3 {
        x: q.x,
        y: q.y,
        z: q.z,
    };
    let s = q.w;
    let dot_uv = u.x * v.x + u.y * v.y + u.z * v.z;
    let dot_uu = u.x * u.x + u.y * u.y + u.z * u.z;
    let cross_uv = Vec3 {
        x: u.y * v.z - u.z * v.y,
        y: u.z * v.x - u.x * v.z,
        z: u.x * v.y - u.y * v.x,
    };
    Vec3 {
        x: 2.0 * dot_uv * u.x + (s * s - dot_uu) * v.x + 2.0 * s * cross_uv.x,
        y: 2.0 * dot_uv * u.y + (s * s - dot_uu) * v.y + 2.0 * s * cross_uv.y,
        z: 2.0 * dot_uv * u.z + (s * s - dot_uu) * v.z + 2.0 * s * cross_uv.z,
    }
}

/// Solves for `(pan_deg, tilt_deg)` such that
/// `mount_rot * RotZ(pan) * RotX(tilt) * NEG_Z` points from `fixture_pos`
/// toward `target` — the inverse of the composition every live pan/tilt
/// reading in this project already goes through (`scene.rs`'s
/// `live_pan_tilt` construction). `tilt_deg` comes out in `[0, 180]`
/// (unsigned — "how far off straight-down", matching most real fixtures'
/// own tilt-range definition); `pan_deg` in `(-180, 180]`. A fixture whose
/// pan/tilt channel range doesn't cover the solved angles simply clips at
/// the byte-encoding step (`ignition_viz::show`), the same as a live
/// operator asking for an out-of-range focus.
// r[impl focus.point]
// r[impl focus.resolve-at-output] - a pure function of the fixture's current position, nothing baked
// r[impl focus.units] - metres in, degrees out
pub fn pan_tilt_deg_to_point(fixture_pos: Vec3, mount_rot: Quat, target: Vec3) -> (f32, f32) {
    pan_tilt_deg_along(mount_rot, v_sub(target, fixture_pos))
}

/// Solves for `(pan_deg, tilt_deg)` such that the fixture's beam points
/// along `world_dir`, regardless of where the fixture is hung.
///
/// The difference from `pan_tilt_deg_to_point` is the difference between
/// a group of fixtures converging on one spot and a group of fixtures
/// pointing the *same way*. Aiming a row of movers at a shared point
/// fans them; aiming them along a shared direction makes their beams
/// parallel, which is a distinct and very common look and cannot be
/// expressed as a focus point at any finite distance.
///
/// `world_dir` need not be normalized. A zero vector leaves the fixture
/// at its mount pose rather than producing garbage angles.
// r[impl focus.orientation]
// r[impl focus.two-kinds] - the orientation half; `pan_tilt_deg_to_point` is the point half
// r[impl focus.units] - degrees
pub fn pan_tilt_deg_along(mount_rot: Quat, world_dir: Vec3) -> (f32, f32) {
    if v_len(world_dir) < 1e-9 {
        return (0.0, 0.0);
    }
    let local = q_rotate(q_conjugate(mount_rot), v_normalize(world_dir));
    let tilt = local.x.hypot(local.y).atan2(-local.z);
    let pan = (-local.x).atan2(local.y);
    (pan.to_degrees() as f32, tilt.to_degrees() as f32)
}

/// A fixture's pan and tilt travel, in degrees — the whole range, so a
/// 540° pan is ±270 about centre and a 270° tilt is 135 either side of
/// straight down. What `pan_tilt_deg_to_point_within` clamps against.
/// Degrees, always: there is no unit field to disagree with the venue.
// r[impl focus.units] - degrees, no per-fixture unit flag
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PanTiltRange {
    pub pan_deg: f32,
    pub tilt_deg: f32,
}

impl Default for PanTiltRange {
    /// The ranges most movers in the rig quote: 540° pan, 270° tilt.
    fn default() -> Self {
        Self {
            pan_deg: 540.0,
            tilt_deg: 270.0,
        }
    }
}

/// Whether a solved aim was inside the fixture's travel.
///
/// `Clamped` carries the angles the fixture *can* reach — its nearest
/// edge — rather than wrapping round the far side of the yoke, which
/// would put the beam somewhere nobody asked for.
// r[impl focus.unreachable]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reach {
    Ok,
    /// The requested aim was outside the range; the returned angles sit
    /// on the nearest limit.
    Clamped,
}

/// Clamps `(pan, tilt)` into `range`, saying whether it had to.
///
/// Pan is centred on zero and tilt on straight down (`[0, tilt/2]`),
/// matching the convention `pan_tilt_deg_to_point` solves in.
// r[impl focus.unreachable] - clamp to the nearest reachable orientation, never wrap
pub fn reachable(pan: f32, tilt: f32, range: PanTiltRange) -> ((f32, f32), Reach) {
    let half_pan = range.pan_deg.abs() / 2.0;
    let half_tilt = range.tilt_deg.abs() / 2.0;
    let p = pan.clamp(-half_pan, half_pan);
    let t = tilt.clamp(0.0, half_tilt);
    let reach = if (p - pan).abs() < 1e-6 && (t - tilt).abs() < 1e-6 {
        Reach::Ok
    } else {
        Reach::Clamped
    };
    ((p, t), reach)
}

/// `pan_tilt_deg_to_point`, then clamped into `range` — the form that
/// can *report* an aim the fixture cannot reach instead of leaving the
/// clip to the DMX encoder.
// r[impl focus.unreachable]
// r[impl focus.resolve-at-output]
pub fn pan_tilt_deg_to_point_within(
    fixture_pos: Vec3,
    mount_rot: Quat,
    target: Vec3,
    range: PanTiltRange,
) -> ((f32, f32), Reach) {
    let (pan, tilt) = pan_tilt_deg_to_point(fixture_pos, mount_rot, target);
    reachable(pan, tilt, range)
}

/// The room a show's coordinates are read in: an origin and an extent
/// in metres, from the venue. Nothing in the solve consults it — a point
/// outside is still solved and still aimed at — it exists so a focus
/// that has wandered off the stage can be *reported*.
///
/// `None` on a `Show` means unbounded, which is what a venue that has
/// not measured its room gets.
// r[impl focus.stage-space] - declared, from the venue, metres
// r[impl focus.units] - metres, no unit field
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StageSpace {
    /// The corner of the box with the smallest coordinates.
    pub origin: Vec3,
    /// The box's size along each axis, metres.
    pub extent: Vec3,
}

impl StageSpace {
    /// Whether `p` sits inside the box (inclusive).
    // r[impl focus.stage-space] - the one check the reporter makes
    pub fn contains(&self, p: Vec3) -> bool {
        let inside = |lo: f64, len: f64, v: f64| v >= lo.min(lo + len) && v <= lo.max(lo + len);
        inside(self.origin.x, self.extent.x, p.x)
            && inside(self.origin.y, self.extent.y, p.y)
            && inside(self.origin.z, self.extent.z, p.z)
    }
}

/// The direction a fixture at `offset` metres from a splay's origin
/// points: straight down, leaned outward along `axis` by
/// `degrees_per_metre` for every metre of offset along that axis. A
/// fixture on the axis points down; one two metres left leans left.
///
/// Derived from the fixture's real position, so a re-hung head takes
/// the angle its new place implies rather than the one authored for
/// its old one.
// r[impl focus.pattern.parallel-out]
// r[impl focus.units] - metres in, degrees per metre
pub fn splay_direction(axis: crate::selection::Axis, offset: Vec3, degrees_per_metre: f32) -> Vec3 {
    use crate::selection::Axis;
    let along = axis.of(offset);
    let angle = (along * degrees_per_metre as f64).to_radians();
    let (s, c) = (angle.sin(), angle.cos());
    let (ax, ay, az) = match axis {
        Axis::X => (1.0, 0.0, 0.0),
        Axis::Y => (0.0, 1.0, 0.0),
        Axis::Z => (0.0, 0.0, 1.0),
    };
    // Down is -Z; lean toward +axis for a positive offset, -axis for a
    // negative one (the sign rides in `angle`). A splay along Z has no
    // sideways component to lean into, so it stays down.
    if matches!(axis, Axis::Z) {
        return Vec3 {
            x: 0.0,
            y: 0.0,
            z: -1.0,
        };
    }
    Vec3 {
        x: ax * s,
        y: ay * s,
        z: az * s - c,
    }
}

/// `a + b`.
pub fn v_add(a: Vec3, b: Vec3) -> Vec3 {
    Vec3 {
        x: a.x + b.x,
        y: a.y + b.y,
        z: a.z + b.z,
    }
}

/// `a * k`.
pub fn v_scale(a: Vec3, k: f64) -> Vec3 {
    Vec3 {
        x: a.x * k,
        y: a.y * k,
        z: a.z * k,
    }
}

/// Aims a fixture at `base_point + delta`: the point the cascade already
/// resolved for this channel, offset by a `RecipeApply::FocusDelta` in
/// metres, then solved to `(pan_deg, tilt_deg)` through the fixture's
/// placement.
///
/// This is what the cue player calls when an emit carries a focus delta
/// (`recipe::FocusDeltaEmit`): the delta is a *room* offset, so it is
/// added to the point before the angles are solved, and every fixture
/// gets its own angles for the same metre. Returns `None` when the rig
/// has no placement for `chan`, in which case the delta cannot mean
/// anything and the caller leaves pan/tilt alone — a fixture with no
/// point aim ignores the delta rather than treating it as a point.
// r[impl focus.delta]
// r[impl focus.orbit-in-metres] - the per-frame solve every fixture does for a path in metres
// r[impl focus.resolve-at-output]
pub fn resolve_focus_delta(
    chan: crate::ChanId,
    base_point: Vec3,
    delta: Vec3,
    rig: &crate::selection::Rig,
) -> Option<(f32, f32)> {
    let p = rig.placement(chan)?;
    Some(pan_tilt_deg_to_point(
        p.position,
        p.orientation,
        v_add(base_point, delta),
    ))
}

#[cfg(test)]
mod delta_tests {
    use super::*;
    use crate::selection::{FixtureInfo, Rig};
    use ignition_proto::Placement;

    fn rig() -> Rig {
        Rig::new(vec![FixtureInfo {
            chan: 3,
            placement: Some(Placement {
                position: Vec3 {
                    x: 0.0,
                    y: 0.0,
                    z: 5.0,
                },
                orientation: Quat {
                    w: 1.0,
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
            }),
            manufacturer: String::new(),
            model: String::new(),
            tags: Vec::new(),
        }])
    }

    /// The delta is metres in the room: a point straight below the head
    /// offset two metres sideways solves to the same angles as aiming at
    /// that offset point directly.
    /// r[verify focus.delta]
    #[test]
    fn a_delta_offsets_the_point_before_solving() {
        let below = Vec3 {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        let two_over = Vec3 {
            x: 2.0,
            y: 0.0,
            z: 0.0,
        };
        let rig = rig();
        let got = resolve_focus_delta(3, below, two_over, &rig).unwrap();
        let want = pan_tilt_deg_to_point(
            Vec3 {
                x: 0.0,
                y: 0.0,
                z: 5.0,
            },
            Quat {
                w: 1.0,
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            v_add(below, two_over),
        );
        assert_eq!(got, want);
        assert!(
            (got.1 - 21.8).abs() < 0.5,
            "atan(2/5) of tilt, got {}",
            got.1
        );
        // No placement: nothing to solve, the caller leaves pan/tilt be.
        assert!(resolve_focus_delta(99, below, two_over, &rig).is_none());
    }
}

#[cfg(test)]
mod direction_tests {
    use super::*;

    fn rot_z(deg: f64) -> Quat {
        let h = deg.to_radians() / 2.0;
        Quat {
            w: h.cos(),
            x: 0.0,
            y: 0.0,
            z: h.sin(),
        }
    }

    /// The point of the direction form: fixtures hung in different places
    /// and at different mount angles all end up pointing the same way,
    /// which aiming them at any shared point cannot achieve.
    /// r[verify focus.orientation]
    #[test]
    fn fixtures_with_different_mounts_end_up_beam_parallel() {
        let want = Vec3 {
            x: 0.0,
            y: -1.0,
            z: -0.3,
        };
        let mut aimed = Vec::new();
        for yaw in [0.0, 35.0, -70.0, 180.0] {
            let mount = rot_z(yaw);
            let (pan, tilt) = pan_tilt_deg_along(mount, want);
            // Recompose what the fixture would actually point along.
            let dir = q_rotate(
                mount,
                q_rotate(
                    rot_z(pan as f64),
                    q_rotate(
                        {
                            let h = (tilt as f64).to_radians() / 2.0;
                            Quat {
                                w: h.cos(),
                                x: h.sin(),
                                y: 0.0,
                                z: 0.0,
                            }
                        },
                        Vec3 {
                            x: 0.0,
                            y: 0.0,
                            z: -1.0,
                        },
                    ),
                ),
            );
            aimed.push(v_normalize(dir));
        }
        let want = v_normalize(want);
        for a in &aimed {
            let dot = a.x * want.x + a.y * want.y + a.z * want.z;
            assert!(dot > 0.9999, "aimed {a:?}, wanted {want:?} (dot {dot})");
        }
    }

    /// A far-away focus point approximates parallel; the direction form
    /// is exact. Worth pinning so nobody replaces one with the other.
    /// r[verify focus.two-kinds]
    /// r[verify focus.orientation]
    #[test]
    fn a_focus_point_fans_where_a_direction_does_not() {
        let mount = Quat {
            w: 1.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        let target = Vec3 {
            x: 0.0,
            y: -10.0,
            z: 0.0,
        };
        let left = Vec3 {
            x: -3.0,
            y: 0.0,
            z: 3.0,
        };
        let right = Vec3 {
            x: 3.0,
            y: 0.0,
            z: 3.0,
        };
        let (pan_l, _) = pan_tilt_deg_to_point(left, mount, target);
        let (pan_r, _) = pan_tilt_deg_to_point(right, mount, target);
        assert!(
            (pan_l - pan_r).abs() > 20.0,
            "a shared point should fan them"
        );

        let dir = Vec3 {
            x: 0.0,
            y: -1.0,
            z: -0.3,
        };
        let (dpan_l, dtilt_l) = pan_tilt_deg_along(mount, dir);
        let (dpan_r, dtilt_r) = pan_tilt_deg_along(mount, dir);
        assert_eq!((dpan_l, dtilt_l), (dpan_r, dtilt_r));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const IDENTITY: Quat = Quat {
        w: 1.0,
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };

    fn forward_direction(mount_rot: Quat, pan_deg: f64, tilt_deg: f64) -> Vec3 {
        // Mirrors scene.rs/fixture_profile.rs's real composition order:
        // mount_rot * RotZ(pan) * RotX(tilt) * NEG_Z.
        let pan_q = Quat {
            w: (pan_deg.to_radians() * 0.5).cos(),
            x: 0.0,
            y: 0.0,
            z: (pan_deg.to_radians() * 0.5).sin(),
        };
        let tilt_q = Quat {
            w: (tilt_deg.to_radians() * 0.5).cos(),
            x: (tilt_deg.to_radians() * 0.5).sin(),
            y: 0.0,
            z: 0.0,
        };
        let combined = q_mul(q_mul(mount_rot, pan_q), tilt_q);
        q_rotate(
            combined,
            Vec3 {
                x: 0.0,
                y: 0.0,
                z: -1.0,
            },
        )
    }

    fn q_mul(a: Quat, b: Quat) -> Quat {
        Quat {
            w: a.w * b.w - a.x * b.x - a.y * b.y - a.z * b.z,
            x: a.w * b.x + a.x * b.w + a.y * b.z - a.z * b.y,
            y: a.w * b.y - a.x * b.z + a.y * b.w + a.z * b.x,
            z: a.w * b.z + a.x * b.y - a.y * b.x + a.z * b.w,
        }
    }

    /// r[verify focus.point]
    #[test]
    fn a_target_straight_below_a_hung_fixture_needs_no_pan_or_tilt() {
        let (pan, tilt) = pan_tilt_deg_to_point(
            Vec3 {
                x: 0.0,
                y: 0.0,
                z: 5.0,
            },
            IDENTITY,
            Vec3 {
                x: 0.0,
                y: 0.0,
                z: -5.0,
            },
        );
        assert!(pan.abs() < 0.5, "expected ~0 pan, got {pan}");
        assert!(tilt.abs() < 0.5, "expected ~0 tilt, got {tilt}");
    }

    /// r[verify focus.point]
    /// r[verify focus.units]
    #[test]
    fn a_target_level_and_in_front_needs_a_90_degree_tilt() {
        let (_, tilt) = pan_tilt_deg_to_point(
            Vec3 {
                x: 0.0,
                y: 0.0,
                z: 5.0,
            },
            IDENTITY,
            Vec3 {
                x: 0.0,
                y: 10.0,
                z: 5.0,
            },
        );
        assert!(
            (tilt - 90.0).abs() < 0.5,
            "expected ~90deg tilt, got {tilt}"
        );
    }

    /// The real proof: pick arbitrary pan/tilt, compute where that beam
    /// actually points (`forward_direction`, mirroring the project's real
    /// composition), place a target far out along that exact direction,
    /// then solve for pan/tilt back from the target — must recover the
    /// original angles. Run under a non-identity mount rotation too, so
    /// the `q_conjugate` inverse-transform step is exercised, not just the
    /// identity case.
    /// r[verify focus.point]
    /// r[verify focus.resolve-at-output]
    #[test]
    fn round_trips_through_a_rotated_mount() {
        let mount_rot = Quat {
            w: (25f64.to_radians() * 0.5).cos(),
            x: 0.0,
            y: (25f64.to_radians() * 0.5).sin(),
            z: 0.0,
        };
        let fixture_pos = Vec3 {
            x: 1.0,
            y: 2.0,
            z: 3.0,
        };
        let (pan0, tilt0) = (37.0f64, 52.0f64);
        let dir = forward_direction(mount_rot, pan0, tilt0);
        let target = Vec3 {
            x: fixture_pos.x + dir.x * 10.0,
            y: fixture_pos.y + dir.y * 10.0,
            z: fixture_pos.z + dir.z * 10.0,
        };
        let (pan, tilt) = pan_tilt_deg_to_point(fixture_pos, mount_rot, target);
        assert!(
            (pan as f64 - pan0).abs() < 0.5,
            "expected pan ~{pan0}, got {pan}"
        );
        assert!(
            (tilt as f64 - tilt0).abs() < 0.5,
            "expected tilt ~{tilt0}, got {tilt}"
        );
    }
}

#[cfg(test)]
mod reach_and_room_tests {
    use super::*;
    use crate::selection::Axis;

    const IDENTITY: Quat = Quat {
        w: 1.0,
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };

    fn v(x: f64, y: f64, z: f64) -> Vec3 {
        Vec3 { x, y, z }
    }

    /// An aim past the yoke's travel clamps to the edge and says so; one
    /// inside is untouched. Clamping never flips to the far side.
    /// r[verify focus.unreachable]
    #[test]
    fn an_unreachable_aim_clamps_to_the_nearest_edge_and_reports() {
        let range = PanTiltRange::default();
        assert_eq!(reachable(10.0, 30.0, range), ((10.0, 30.0), Reach::Ok));
        // Straight up and slightly behind: tilt would be ~174°.
        let ((pan, tilt), reach) =
            pan_tilt_deg_to_point_within(v(0.0, 0.0, 5.0), IDENTITY, v(0.0, -0.5, 10.0), range);
        assert_eq!(reach, Reach::Clamped);
        assert!(
            (tilt - 135.0).abs() < 1e-3,
            "tilt clamps to half of 270, got {tilt}"
        );
        assert!(pan.abs() <= 270.0);
        let narrow = PanTiltRange {
            pan_deg: 90.0,
            tilt_deg: 90.0,
        };
        assert_eq!(
            reachable(80.0, 60.0, narrow),
            ((45.0, 45.0), Reach::Clamped)
        );
        assert_eq!(
            reachable(-80.0, 10.0, narrow),
            ((-45.0, 10.0), Reach::Clamped)
        );
    }

    /// The stage space is a box in metres; a point on its face is inside.
    /// r[verify focus.stage-space]
    /// r[verify focus.units]
    #[test]
    fn the_stage_space_is_a_box_in_metres() {
        let stage = StageSpace {
            origin: v(-15.0, -15.0, 0.0),
            extent: v(30.0, 30.0, 15.0),
        };
        assert!(stage.contains(v(0.0, 0.0, 0.0)));
        assert!(stage.contains(v(15.0, -15.0, 15.0)));
        assert!(!stage.contains(v(0.0, 0.0, -0.1)));
        assert!(!stage.contains(v(16.0, 0.0, 1.0)));
    }

    /// A splay direction leans by degrees per metre of offset along the
    /// axis, sign and all, and by nothing across it.
    /// r[verify focus.pattern.parallel-out]
    #[test]
    fn a_splay_leans_by_offset_along_its_axis() {
        let down = splay_direction(Axis::X, v(0.0, 4.0, 0.0), 10.0);
        assert!((down.z + 1.0).abs() < 1e-9 && down.x.abs() < 1e-9);
        let right = splay_direction(Axis::X, v(3.0, 0.0, 0.0), 10.0);
        assert!((right.x - 30f64.to_radians().sin()).abs() < 1e-9);
        let left = splay_direction(Axis::X, v(-3.0, 0.0, 0.0), 10.0);
        assert!((left.x + right.x).abs() < 1e-9, "mirror image");
        assert!((left.z - right.z).abs() < 1e-9);
    }
}
