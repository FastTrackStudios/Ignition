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
pub fn pan_tilt_deg_along(mount_rot: Quat, world_dir: Vec3) -> (f32, f32) {
    if v_len(world_dir) < 1e-9 {
        return (0.0, 0.0);
    }
    let local = q_rotate(q_conjugate(mount_rot), v_normalize(world_dir));
    let tilt = local.x.hypot(local.y).atan2(-local.z);
    let pan = (-local.x).atan2(local.y);
    (pan.to_degrees() as f32, tilt.to_degrees() as f32)
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
