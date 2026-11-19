//! Every fixture's light in the air, from the analytic beam pass.
//!
//! `ignition-beams` draws a beam as one cone hull whose pixels integrate
//! the beam's scattering in closed form: no march, no grid, no history, so
//! nothing to bead or block (`features/lighting/beams/README.md`). This
//! module is the visualizer's side of it. Each emitter gets an [`AirBeam`]
//! child (three where the fixture has a prism), and each frame that child's
//! `Beam` is written from what the visualizer already resolved for the
//! fixture: its cone as the spill light has it (the profile's beam and
//! field angles, zoomed by the wire), its flux, its colour with the dimmer
//! and the strobe's shutter in it, its gobo and how far that has turned,
//! its prism. Pan and tilt reach it through the transform tree like
//! everything else under the emitter.
//!
//! Bevy's volumetric fog is not on the cameras then, and nothing is
//! marched. `IGNITION_BEAMS=0` brings the fog back in place of this, for
//! comparing the two; it is not a path to ship on.
// r[impl viz.haze-is-volumetric] - light in the air is the beam pass's, off the same emitters

use std::collections::HashMap;

use bevy::prelude::*;
use ignition_beams::{
    Beam, BeamCamera, BeamGobos, BeamTier, BeamsPlugin, Gobo, GoboLayer, Haze, Quality, gobo_image,
};

use crate::fixture_profile::is_hazer;
use crate::gobo::{GoboLibrary, GoboProjector, PRISM_FACETS};
use crate::haze::HazeLevel;
use crate::spawn::{
    BeamEmitter, EmitterPhotometry, EmitterState, FOG_SCATTERING_ASYMMETRY, FixtureSpill,
    VizSettings,
};

/// What draws the light in the air.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AirPath {
    /// The analytic beam pass. The default.
    Beams,
    /// Bevy's volumetric fog, marched. Only for comparison.
    Fog,
}

/// The path asked for: the beams, unless `IGNITION_BEAMS=0`.
#[must_use]
pub fn air_path() -> AirPath {
    air_path_for(std::env::var("IGNITION_BEAMS").ok().as_deref())
}

/// The path a value of `IGNITION_BEAMS` asks for.
#[must_use]
pub fn air_path_for(value: Option<&str>) -> AirPath {
    match value.map(str::trim) {
        Some("0" | "off" | "fog") => AirPath::Fog,
        _ => AirPath::Beams,
    }
}

/// The beam tier a quality preset's air dial stands for.
///
/// The presets say how fine the air is as a number of fog steps (12 at
/// potato to 64 at ultra, 192 for a still); the beam pass has four tiers.
#[must_use]
pub const fn tier_for(fog_steps: u32) -> BeamTier {
    match fog_steps {
        0..=20 => BeamTier::Low,
        21..=32 => BeamTier::Medium,
        33..=64 => BeamTier::High,
        _ => BeamTier::Ultra,
    }
}

/// What a camera carries to see the beams.
#[must_use]
pub fn beam_camera(fog_steps: u32) -> BeamCamera {
    BeamCamera {
        quality: Quality {
            tier: tier_for(fog_steps),
            ..Quality::default()
        },
        ..BeamCamera::default()
    }
}

/// Light the haze scatters per metre, in a normally hazed room at the
/// operator's dial of 1.
///
/// The beams are in real candela and the camera in real exposure, so this
/// is the one number that says how bright the air is. It is set so the
/// recital's chorus reads as it did under the fog, whose own gain
/// (`spawn::FOG_LIGHT_GAIN`) was tuned by eye and not in these units.
pub const AIR_SCATTERING_PER_METRE: f32 = 0.25;

/// [`AIR_SCATTERING_PER_METRE`], or `IGNITION_AIR_SCATTERING` for trying
/// another.
#[must_use]
pub fn air_scattering() -> f32 {
    std::env::var("IGNITION_AIR_SCATTERING")
        .ok()
        .and_then(|v| v.trim().parse::<f32>().ok())
        .unwrap_or(AIR_SCATTERING_PER_METRE)
}

/// The haze a camera's beams shine through, for the room's hazer level and
/// the operator's dial.
#[must_use]
pub fn haze_for(dial: f32, hazer_level: f32) -> Haze {
    Haze {
        density: dial * hazer_level,
        scattering_at_full: air_scattering(),
        extinction_at_full: crate::spawn::haze_extinction_per_metre(1.0, 1.0),
        anisotropy: FOG_SCATTERING_ASYMMETRY,
    }
}

/// One beam in the air under an emitter: facet 0 always, facets 1 and 2
/// where the fixture has a prism to split it.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct AirBeam {
    pub facet: usize,
}

/// Which layer of the gobo texture each wheel's slot is.
#[derive(Resource, Default)]
pub struct GoboLayers {
    by_slot: HashMap<(Option<String>, usize), u16>,
}

impl GoboLayers {
    /// The layer for a wheel (by its key in the gobo library, `None` for
    /// the built-in one) and a slot on it.
    #[must_use]
    pub fn layer(&self, wheel: Option<&str>, slot: usize) -> Option<u16> {
        self.by_slot.get(&(wheel.map(str::to_owned), slot)).copied()
    }
}

/// The side of every layer of the gobo texture.
const GOBO_LAYER_SIZE: u16 = 256;

pub struct AirBeamsPlugin;

impl Plugin for AirBeamsPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(BeamsPlugin)
            .init_resource::<GoboLayers>()
            .add_systems(
                Update,
                (
                    attach_air_beams,
                    pack_gobos,
                    turn_prism_facets.after(crate::gobo::update_gobo_projectors),
                    drive_beam_haze,
                ),
            )
            .add_systems(
                PostUpdate,
                write_air_beams.after(crate::spawn::update_beams),
            );
    }
}

/// Gives every new emitter its beams in the air.
#[expect(
    clippy::needless_pass_by_value,
    reason = "Bevy system params are taken by value by the system signature itself; see docs/ops/clippy.md"
)]
fn attach_air_beams(
    mut commands: Commands,
    venue: Res<crate::spawn::VenueRes>,
    emitters: Query<(Entity, &BeamEmitter), Added<BeamEmitter>>,
) {
    for (entity, emitter) in &emitters {
        let Some(record) = venue.0.fixtures.get(emitter.fixture) else {
            continue;
        };
        // A hazer has no light of its own, as it has no spill.
        if is_hazer(record.model.as_deref().unwrap_or("")) {
            continue;
        }
        for facet in 0..PRISM_FACETS {
            commands.spawn((
                AirBeam { facet },
                // Dark until `write_air_beams` says otherwise.
                Beam {
                    dimmer: 0.0,
                    ..Beam::default()
                },
                Transform::default(),
                Name::new(format!("{} beam {facet}", record.name)),
                ChildOf(entity),
            ));
        }
    }
}

/// How open a gobo mask is at a point of its unit disc, 0 to 1. The mask's
/// first row is the top of the picture, which is up in the beam.
fn mask_openness(mask: &crate::gobo::GoboMask, x: f32, y: f32) -> f32 {
    let size = crate::num::f32_of_u32(mask.size);
    let texel = |along: f32| {
        let scaled = (along.mul_add(0.5, 0.5) * size).clamp(0.0, (size - 1.0).max(0.0));
        (0..mask.size)
            .rev()
            .find(|&i| crate::num::f32_of_u32(i) <= scaled)
            .unwrap_or(0)
    };
    let index = texel(-y)
        .saturating_mul(mask.size)
        .saturating_add(texel(x))
        .saturating_mul(4)
        .saturating_add(3);
    let blocked = mask
        .rgba
        .get(crate::num::usize_of_u32(index))
        .copied()
        .unwrap_or(u8::MAX);
    1.0 - f32::from(blocked) / 255.0
}

/// Packs every wheel's masks into the one texture the beams sample, once
/// the gobo library is loaded.
#[expect(
    clippy::needless_pass_by_value,
    reason = "Bevy system params are taken by value by the system signature itself; see docs/ops/clippy.md"
)]
fn pack_gobos(
    library: Res<GoboLibrary>,
    mut layers: ResMut<GoboLayers>,
    mut gobos: ResMut<BeamGobos>,
    mut images: ResMut<Assets<Image>>,
    mut packed: Local<bool>,
) {
    if *packed || library.wheel(None).is_none() {
        return;
    }
    *packed = true;
    let mut pictures = Vec::new();
    for (wheel, spec) in library.wheels() {
        for (slot, mask) in spec.slots.iter().enumerate() {
            let Some(mask) = mask else { continue };
            let Ok(layer) = u16::try_from(pictures.len()) else {
                break;
            };
            pictures.push(GoboLayer::sample(GOBO_LAYER_SIZE, |x, y| {
                mask_openness(mask, x, y)
            }));
            layers
                .by_slot
                .insert((wheel.map(str::to_owned), slot), layer);
        }
    }
    if let Some(image) = gobo_image(&pictures) {
        gobos.0 = images.add(image);
    }
}

/// The rotation of a beam's facet under its emitter: turned with the gobo,
/// and thrown off the axis when the prism is in. The same rotation the
/// facet's decal has, so the pattern in the air lands on the pattern on
/// the wall.
fn facet_rotation(facet: usize, projector: &GoboProjector) -> Quat {
    crate::gobo::facet_transform(
        facet,
        1.0,
        1.0,
        projector.spin,
        projector.prism_in,
        projector.prism,
    )
    .rotation
}

/// Turns each fixture's facets with its gobo and its prism.
fn turn_prism_facets(
    projectors: Query<(&GoboProjector, &Children)>,
    mut beams: Query<(&AirBeam, &mut Transform)>,
) {
    for (projector, children) in &projectors {
        for child in children.iter() {
            let Ok((beam, mut transform)) = beams.get_mut(child) else {
                continue;
            };
            let rotation = facet_rotation(beam.facet, projector);
            if transform.rotation != rotation {
                transform.rotation = rotation;
            }
        }
    }
}

/// What one emitter's light is this frame, for its beams in the air.
struct Lit {
    colour: LinearRgba,
    beam_angle: f32,
    field_angle: f32,
    range: f32,
    flux: f32,
    lens_radius: f32,
}

/// The beam of facet `facet` of an emitter's light: the whole light without
/// a prism, a share of it on each facet with one, dark otherwise.
fn facet_beam(lit: Option<&Lit>, facet: usize, prism_in: bool, gobo: Option<Gobo>) -> Beam {
    let dark = Beam {
        dimmer: 0.0,
        ..Beam::default()
    };
    let Some(lit) = lit else { return dark };
    if facet > 0 && !prism_in {
        return dark;
    }
    let facets = if prism_in {
        crate::num::f32_of_usize(PRISM_FACETS)
    } else {
        1.0
    };
    Beam {
        beam_angle: lit.beam_angle,
        field_angle: lit.field_angle,
        lens_radius: lit.lens_radius,
        range: lit.range,
        flux: lit.flux / facets,
        colour: lit.colour,
        dimmer: 1.0,
        gobo,
        ..Beam::default()
    }
}

/// Writes every beam in the air from its emitter's state and its spill
/// light's cone, after the spill has been aimed and sized.
#[expect(
    clippy::needless_pass_by_value,
    reason = "Bevy system params are taken by value by the system signature itself; see docs/ops/clippy.md"
)]
fn write_air_beams(
    layers: Res<GoboLayers>,
    emitters: Query<(
        &EmitterState,
        &Children,
        Option<&EmitterPhotometry>,
        Option<&GoboProjector>,
    )>,
    spills: Query<&SpotLight, With<FixtureSpill>>,
    mut beams: Query<(&AirBeam, &mut Beam)>,
) {
    for (state, children, photometry, projector) in &emitters {
        let spill = children.iter().find_map(|child| spills.get(child).ok());
        let lit = state.color.zip(spill).map(|(colour, spill)| Lit {
            // As the spill takes it: the colour with the dimmer in it.
            colour: Color::srgb(colour[0], colour[1], colour[2]).to_linear(),
            beam_angle: spill.inner_angle * 2.0,
            field_angle: spill.outer_angle * 2.0,
            range: spill.range,
            flux: state.lumens,
            lens_radius: photometry.map_or(0.05, |p| p.lens_radius),
        });
        let gobo = projector.and_then(|projector| {
            let layer = layers.layer(projector.fixture_type.as_deref(), projector.slot?)?;
            Some(Gobo {
                slot: layer,
                // The facet's own transform carries the wheel's turn.
                rotation: 0.0,
                blur: 0.0,
            })
        });
        let prism_in = projector.is_some_and(|projector| projector.prism_in);
        for child in children.iter() {
            let Ok((air, mut beam)) = beams.get_mut(child) else {
                continue;
            };
            let next = facet_beam(lit.as_ref(), air.facet, prism_in, gobo);
            if *beam != next {
                *beam = next;
            }
        }
    }
}

/// The room's haze on every camera that sees beams: the hazers' settled
/// level times the operator's dial.
#[expect(
    clippy::needless_pass_by_value,
    reason = "Bevy system params are taken by value by the system signature itself; see docs/ops/clippy.md"
)]
fn drive_beam_haze(
    level: Res<HazeLevel>,
    settings: Res<VizSettings>,
    mut cameras: Query<&mut BeamCamera>,
) {
    let haze = haze_for(settings.haze, level.level);
    for mut camera in &mut cameras {
        if camera.haze != haze {
            camera.haze = haze;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // r[verify viz.haze-is-volumetric] - the beams draw the air unless the fog is asked for
    #[test]
    fn the_beams_are_the_default_and_the_fog_is_asked_for() {
        assert_eq!(air_path_for(None), AirPath::Beams);
        assert_eq!(air_path_for(Some("1")), AirPath::Beams);
        assert_eq!(air_path_for(Some("0")), AirPath::Fog);
        assert_eq!(air_path_for(Some(" fog ")), AirPath::Fog);
    }

    #[test]
    fn the_presets_air_dial_picks_a_tier() {
        assert_eq!(tier_for(12), BeamTier::Low);
        assert_eq!(tier_for(32), BeamTier::Medium);
        assert_eq!(tier_for(64), BeamTier::High);
        assert_eq!(tier_for(192), BeamTier::Ultra);
    }

    #[test]
    fn no_haze_is_no_light_in_the_air() {
        assert!(haze_for(1.6, 0.0).scattering().abs() < f32::EPSILON);
        assert!(haze_for(0.0, 1.0).scattering().abs() < f32::EPSILON);
        let half = haze_for(1.0, 0.5).scattering();
        let full = haze_for(1.0, 1.0).scattering();
        assert!((full / half - 2.0).abs() < 1.0e-5);
    }

    fn lit() -> Lit {
        Lit {
            colour: LinearRgba::WHITE,
            beam_angle: 0.2,
            field_angle: 0.3,
            range: 30.0,
            flux: 9_000.0,
            lens_radius: 0.05,
        }
    }

    #[test]
    fn a_dark_fixture_has_no_beam_in_the_air() {
        for facet in 0..PRISM_FACETS {
            assert!(facet_beam(None, facet, true, None).dimmer.abs() < f32::EPSILON);
        }
    }

    #[test]
    fn without_a_prism_only_the_first_facet_is_lit_and_carries_everything() {
        let lit = lit();
        let first = facet_beam(Some(&lit), 0, false, None);
        assert!((first.flux - 9_000.0).abs() < 1.0e-3);
        assert!((first.dimmer - 1.0).abs() < f32::EPSILON);
        assert!(facet_beam(Some(&lit), 1, false, None).dimmer.abs() < f32::EPSILON);
    }

    // r[verify viz.gobo-raster] - a prism triples the beam and shares its light
    #[test]
    fn a_prism_shares_the_light_between_its_facets() {
        let lit = lit();
        let total: f32 = (0..PRISM_FACETS)
            .map(|facet| facet_beam(Some(&lit), facet, true, None).flux)
            .sum();
        assert!((total - 9_000.0).abs() < 1.0e-2, "{total}");
    }

    #[test]
    fn a_mask_is_open_where_its_alpha_is_clear_and_its_top_row_is_up() {
        // Two by two: the top row open, the bottom row blocked.
        let mask = crate::gobo::GoboMask {
            size: 2,
            rgba: vec![0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 255, 0, 0, 0, 255],
        };
        assert!((mask_openness(&mask, -0.5, 0.5) - 1.0).abs() < 1.0e-6);
        assert!((mask_openness(&mask, 0.5, 0.5) - 1.0).abs() < 1.0e-6);
        assert!(mask_openness(&mask, 0.5, -0.5).abs() < 1.0e-6);
    }
}
