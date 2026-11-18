//! The rig and what it does: three trusses of moving heads over a dark
//! stage, and the scenes that are known to break a marched fog.
//!
//! All of it is plain values: where a fixture hangs, and for a scene and a
//! moment, where it points, in what colour, how bright. `main.rs` applies
//! those to entities.

#![expect(
    clippy::arithmetic_side_effects,
    clippy::suboptimal_flops,
    reason = "arithmetic on floats and glam vectors: the first lint cannot tell them from integers, and a fused multiply-add would only obscure a look's geometry"
)]

use bevy::{color::LinearRgba, math::Vec3};
use ignition_beams::Gobo;

/// The stage: a floor at y = 0 and a back wall at z = `WALL_Z`.
pub const WALL_Z: f32 = -9.0;
pub const TRIM: f32 = 7.0;
const TRUSS_Z: [f32; 3] = [4.0, -0.5, -5.0];
const TRUSS_HALF_WIDTH: f32 = 9.5;

/// The three kinds of mover on the rig.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A beam fixture: a couple of degrees, a big lens, a long bright shaft.
    Beam,
    /// A profile spot with gobos.
    Spot,
    /// A wash.
    Wash,
}

/// What a kind of fixture is, optically: the numbers a GDTF file carries.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Optics {
    /// Beam angle (to half intensity), full, in degrees.
    pub beam_deg: f32,
    /// Field angle (to a tenth), full, in degrees.
    pub field_deg: f32,
    pub lens_radius: f32,
    /// Luminous flux at full, in lumens.
    pub flux: f32,
}

impl Kind {
    #[must_use]
    pub const fn optics(self) -> Optics {
        match self {
            Self::Beam => Optics {
                beam_deg: 1.8,
                field_deg: 3.0,
                lens_radius: 0.075,
                flux: 9_000.0,
            },
            Self::Spot => Optics {
                beam_deg: 13.0,
                field_deg: 19.0,
                lens_radius: 0.065,
                flux: 22_000.0,
            },
            Self::Wash => Optics {
                beam_deg: 26.0,
                field_deg: 46.0,
                lens_radius: 0.11,
                flux: 16_000.0,
            },
        }
    }
}

/// One mover on the rig.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fixture {
    pub index: u16,
    pub kind: Kind,
    /// Where its lens hangs.
    pub position: Vec3,
    /// Which truss it is on, 0 at the front.
    pub truss: u8,
    /// Its place along its truss, −1 at stage right to 1 at stage left.
    pub along: f32,
}

/// `count` movers spread over the three trusses, kinds alternating beam,
/// spot, wash along each.
#[must_use]
pub fn rig(count: u16) -> Vec<Fixture> {
    let per_truss = count.div_ceil(3).max(1);
    (0..count)
        .map(|index| {
            let truss = index % 3;
            let slot = index / 3;
            let along = if per_truss > 1 {
                f32::from(slot) / f32::from(per_truss.saturating_sub(1)) * 2.0 - 1.0
            } else {
                0.0
            };
            let kind = match (slot.wrapping_add(truss)) % 3 {
                0 => Kind::Beam,
                1 => Kind::Spot,
                _ => Kind::Wash,
            };
            let truss = u8::try_from(truss).unwrap_or(0);
            let z = TRUSS_Z.get(usize::from(truss)).copied().unwrap_or(0.0);
            Fixture {
                index,
                kind,
                position: Vec3::new(along * TRUSS_HALF_WIDTH, TRIM, z),
                truss,
                along,
            }
        })
        .collect()
}

/// The truss lines, for drawing them: each end to end.
#[must_use]
pub fn trusses() -> [(Vec3, Vec3); 3] {
    TRUSS_Z.map(|z| {
        (
            Vec3::new(-TRUSS_HALF_WIDTH - 0.5, TRIM + 0.35, z),
            Vec3::new(TRUSS_HALF_WIDTH + 0.5, TRIM + 0.35, z),
        )
    })
}

/// What one fixture is doing at one moment.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Look {
    /// The point it is aimed at.
    pub aim: Vec3,
    pub colour: LinearRgba,
    /// 0 to 1, the strobe's shutter folded in.
    pub dimmer: f32,
    pub gobo: Option<Gobo>,
    /// Zoom: its angles as a multiple of the kind's.
    pub zoom: f32,
}

impl Look {
    const fn dark() -> Self {
        Self {
            aim: Vec3::ZERO,
            colour: LinearRgba::WHITE,
            dimmer: 0.0,
            gobo: None,
            zoom: 1.0,
        }
    }
}

/// The scenes: one that looks like a show, and the ones that break a
/// marched fog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scene {
    /// Sweeps, colours, a strobe, rotating gobos: everything at once.
    Show,
    /// Only the beam fixtures, white, panning fast, seen from the side.
    SidePan,
    /// The beam fixtures pointed at the camera, from dead on to a miss.
    AtCamera,
    /// The beam fixtures strobing at 25 Hz.
    Strobe,
    /// Every fixture aimed at one point centre stage.
    Overlap,
    /// Half the rig ending on the floor, half on the back wall.
    FloorWall,
    /// The camera inside a wash, looking across the stage.
    InsideWash,
    /// `data/songs/beam-alias.json`: every mover full white, the ranks
    /// crossed steeply in the air.
    Alias,
    /// The spots, each with a rotating gobo.
    Gobos,
}

impl Scene {
    pub const ALL: [Self; 9] = [
        Self::Show,
        Self::SidePan,
        Self::AtCamera,
        Self::Strobe,
        Self::Overlap,
        Self::FloorWall,
        Self::InsideWash,
        Self::Alias,
        Self::Gobos,
    ];

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Show => "show",
            Self::SidePan => "side-pan",
            Self::AtCamera => "at-camera",
            Self::Strobe => "strobe",
            Self::Overlap => "overlap",
            Self::FloorWall => "floor-wall",
            Self::InsideWash => "inside-wash",
            Self::Alias => "alias",
            Self::Gobos => "gobos",
        }
    }

    #[must_use]
    pub fn named(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|scene| scene.name() == name)
    }

    /// Where the camera stands and what it looks at.
    #[must_use]
    pub const fn camera(self) -> (Vec3, Vec3) {
        match self {
            Self::Show | Self::Alias | Self::Strobe | Self::Gobos => {
                (Vec3::new(0.0, 2.2, 15.0), Vec3::new(0.0, 3.6, 0.0))
            }
            Self::SidePan => (Vec3::new(-17.0, 3.0, 2.0), Vec3::new(0.0, 3.4, -0.5)),
            Self::AtCamera => (AT_CAMERA_EYE, Vec3::new(0.0, 4.6, 0.0)),
            Self::Overlap => (Vec3::new(6.0, 1.7, 13.0), Vec3::new(0.0, 3.0, -1.0)),
            Self::FloorWall => (Vec3::new(-7.0, 1.4, 9.0), Vec3::new(1.0, 2.2, -6.0)),
            Self::InsideWash => (INSIDE_WASH_EYE, Vec3::new(3.0, 2.5, -4.0)),
        }
    }
}

const AT_CAMERA_EYE: Vec3 = Vec3::new(0.0, 1.8, 13.0);
/// A metre and a half under a wash on the front truss, inside its cone.
const INSIDE_WASH_EYE: Vec3 = Vec3::new(-0.4, 5.3, 3.7);

const WHITE: LinearRgba = LinearRgba::WHITE;
const fn rgb(red: f32, green: f32, blue: f32) -> LinearRgba {
    LinearRgba::new(red, green, blue, 1.0)
}
const PALETTE: [LinearRgba; 6] = [
    rgb(1.0, 0.05, 0.03),
    rgb(0.02, 0.2, 1.0),
    rgb(1.0, 0.45, 0.04),
    rgb(0.03, 0.9, 0.5),
    rgb(0.7, 0.05, 1.0),
    rgb(0.0, 0.75, 1.0),
];

fn palette(index: u16) -> LinearRgba {
    PALETTE
        .get(usize::from(index) % PALETTE.len())
        .copied()
        .unwrap_or(WHITE)
}

/// A shutter open for the first half of each cycle at `hz`.
fn strobe(t: f32, hz: f32) -> f32 {
    if (t * hz).fract() < 0.5 { 1.0 } else { 0.0 }
}

/// What `fixture` does in `scene` at `t` seconds.
#[must_use]
pub fn look(scene: Scene, fixture: &Fixture, t: f32) -> Look {
    match scene {
        Scene::Show => show(fixture, t),
        Scene::SidePan => side_pan(fixture, t),
        Scene::AtCamera => at_camera(fixture, t),
        Scene::Strobe => strobing(fixture, t),
        Scene::Overlap => Look {
            aim: Vec3::new(0.0, 1.2, -1.0),
            colour: palette(u16::from(fixture.truss)),
            dimmer: 1.0,
            ..Look::dark()
        },
        Scene::FloorWall => floor_wall(fixture),
        Scene::InsideWash => inside_wash(fixture, t),
        Scene::Alias => alias(fixture),
        Scene::Gobos => gobos(fixture, t),
    }
}

/// The floor under a fixture, pushed about by a slow figure.
fn sweep(fixture: &Fixture, t: f32, reach: f32, rate: f32) -> Vec3 {
    let phase = t * rate + f32::from(fixture.index) * 0.37;
    Vec3::new(
        fixture.position.x * 0.6 + phase.sin() * reach,
        0.0,
        fixture.position.z * 0.5 + (phase * 0.7).cos() * reach * 0.6 + 1.5,
    )
}

fn show(fixture: &Fixture, t: f32) -> Look {
    match fixture.kind {
        Kind::Beam => Look {
            aim: sweep(fixture, t, 7.0, 1.3) + Vec3::new(0.0, 0.0, 6.0),
            colour: WHITE,
            dimmer: if fixture.truss == 2 {
                strobe(t, 12.5)
            } else {
                1.0
            },
            ..Look::dark()
        },
        Kind::Spot => Look {
            aim: sweep(fixture, t, 4.0, 0.6),
            colour: palette(u16::from(fixture.truss).wrapping_add(fixture.index / 9)),
            dimmer: 1.0,
            gobo: Some(Gobo {
                slot: fixture.index % 3 + 1,
                rotation: t * 1.5,
                blur: 0.0,
            }),
            ..Look::dark()
        },
        Kind::Wash => Look {
            aim: Vec3::new(
                fixture.position.x * 0.7,
                0.0,
                fixture.position.z * 0.4 - 1.0,
            ),
            colour: palette(u16::from(fixture.truss).wrapping_add(1)),
            dimmer: 0.8,
            ..Look::dark()
        },
    }
}

/// A fast pan: the aim point crosses the stage several times a second.
fn side_pan(fixture: &Fixture, t: f32) -> Look {
    if fixture.kind != Kind::Beam {
        return Look::dark();
    }
    let phase = t * 6.0 + fixture.along * 2.0;
    Look {
        aim: Vec3::new(
            fixture.position.x + phase.sin() * 6.0,
            0.0,
            fixture.position.z + phase.cos() * 5.0,
        ),
        colour: WHITE,
        dimmer: 1.0,
        ..Look::dark()
    }
}

fn at_camera(fixture: &Fixture, t: f32) -> Look {
    if fixture.kind != Kind::Beam {
        return Look::dark();
    }
    // The middle of each truss dead on, the rest missing by more and more:
    // some beams are looked straight into, some graze the lens, some pass.
    let phase = t * 0.8 + f32::from(fixture.index) * 0.9;
    let miss = 0.1 + 2.6 * fixture.along.abs();
    Look {
        aim: AT_CAMERA_EYE + Vec3::new(phase.cos(), phase.sin(), 0.0) * miss,
        colour: if fixture.truss == 1 {
            palette(fixture.index / 3)
        } else {
            WHITE
        },
        dimmer: 1.0,
        ..Look::dark()
    }
}

fn strobing(fixture: &Fixture, t: f32) -> Look {
    match fixture.kind {
        Kind::Beam => Look {
            aim: Vec3::new(fixture.position.x * 1.3, 0.0, fixture.position.z + 7.0),
            colour: WHITE,
            dimmer: strobe(t + f32::from(fixture.truss) * 0.013, 25.0),
            ..Look::dark()
        },
        Kind::Spot => Look {
            aim: Vec3::new(fixture.position.x * 0.5, 0.0, 2.0),
            colour: rgb(0.02, 0.1, 1.0),
            dimmer: 0.6,
            ..Look::dark()
        },
        Kind::Wash => Look::dark(),
    }
}

fn floor_wall(fixture: &Fixture) -> Look {
    let to_wall = fixture.index.is_multiple_of(2);
    let aim = if to_wall {
        Vec3::new(
            fixture.position.x * 0.9,
            1.0 + f32::from(fixture.truss) * 1.6,
            WALL_Z,
        )
    } else {
        Vec3::new(
            fixture.position.x * 0.7 - 2.0,
            0.0,
            fixture.position.z + 5.0,
        )
    };
    Look {
        aim,
        colour: if to_wall {
            palette(fixture.index / 2)
        } else {
            WHITE
        },
        dimmer: 1.0,
        ..Look::dark()
    }
}

fn inside_wash(fixture: &Fixture, t: f32) -> Look {
    match fixture.kind {
        // The washes all pour down around the camera.
        Kind::Wash => Look {
            aim: Vec3::new(fixture.position.x * 0.5, 0.0, fixture.position.z * 0.6),
            colour: palette(fixture.index / 3),
            dimmer: 1.0,
            zoom: 1.2,
            ..Look::dark()
        },
        Kind::Beam => Look {
            aim: sweep(fixture, t, 5.0, 1.0),
            colour: WHITE,
            dimmer: 1.0,
            ..Look::dark()
        },
        Kind::Spot => Look::dark(),
    }
}

/// The cue in `data/songs/beam-alias.json`: nothing lit but the movers,
/// full white, the ranks aimed across each other so the beams cross in the
/// air at a steep angle, off the vertical.
fn alias(fixture: &Fixture) -> Look {
    if fixture.kind == Kind::Wash {
        return Look::dark();
    }
    let side = if fixture.kind == Kind::Beam {
        1.0
    } else {
        -1.0
    };
    Look {
        aim: Vec3::new(
            -4.2 * side + fixture.along * 1.5,
            0.4,
            9.0 - f32::from(fixture.truss) * 3.0,
        ),
        colour: WHITE,
        dimmer: 1.0,
        zoom: if fixture.kind == Kind::Spot { 0.5 } else { 1.0 },
        ..Look::dark()
    }
}

fn gobos(fixture: &Fixture, t: f32) -> Look {
    if fixture.kind != Kind::Spot {
        return Look::dark();
    }
    Look {
        aim: Vec3::new(
            fixture.position.x * 0.8,
            0.0,
            fixture.position.z * 0.3 + 4.0,
        ),
        colour: if fixture.truss == 1 {
            WHITE
        } else {
            palette(fixture.index / 3)
        },
        dimmer: 1.0,
        gobo: Some(Gobo {
            slot: fixture.index % 3 + 1,
            rotation: t
                * 2.0
                * (if fixture.index.is_multiple_of(2) {
                    1.0
                } else {
                    -1.0
                }),
            blur: 0.0,
        }),
        zoom: 1.3,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rig_has_every_kind_on_every_truss() {
        let fixtures = rig(72);
        assert_eq!(fixtures.len(), 72);
        for truss in 0..3_u8 {
            for kind in [Kind::Beam, Kind::Spot, Kind::Wash] {
                assert!(
                    fixtures.iter().any(|f| f.truss == truss && f.kind == kind),
                    "truss {truss} has no {kind:?}"
                );
            }
        }
        assert!(fixtures.iter().all(|f| f.along.abs() <= 1.0));
    }

    #[test]
    fn scenes_are_found_by_name() {
        for scene in Scene::ALL {
            assert_eq!(Scene::named(scene.name()), Some(scene));
        }
    }

    #[test]
    fn a_strobe_is_dark_half_the_time() {
        let open = (0..1000_u16)
            .filter(|&i| strobe(f32::from(i) / 1000.0, 25.0) > 0.5)
            .count();
        assert!((480..=520).contains(&open), "{open} of 1000");
    }
}
