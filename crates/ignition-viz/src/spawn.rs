//! Turns a `Venue` into entities, once, at startup — and then keeps the
//! live-driven parts of them up to date.
//!
//! The pre-Bevy renderer rebuilt the entire scene's vertex buffer every
//! frame, because a flat vertex buffer was the only thing it had. Here
//! the room is spawned once and never touched again, and a lit fixture's
//! per-frame work is writing a handful of numbers into its own material
//! and light: no geometry is rebuilt to move a mover.

use crate::dmx::DmxUniverses;
use crate::fixture_profile::{
    BeamThrow, BodyVisual, LUMENS_PER_WATT, MAX_FIELD_HALF_ANGLE_DEG, SHAFT_CANDELA_THRESHOLD,
    assumed_field_half_angle_deg, beam_half_angle_deg, is_hazer, peak_candela, power_watts,
    resolve_fixture,
};
use crate::gdtf_geometry::{self, GdtfFixture, GdtfLibrary, PanJoint, TiltJoint};
use crate::venue::Venue;
use crate::video::{ContentKind, VideoSource};
use bevy::asset::RenderAssetUsages;
use bevy::image::Image;
use bevy::light::{FogVolume, VolumetricLight};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use std::sync::Mutex;

/// The loaded `.gdtf` profiles, if `--gdtf-dir` was given. Absent means
/// every fixture falls back to its QLC+ category shape, which is what
/// happened before GDTF geometry existed.
#[derive(Resource, Default)]
pub struct GdtfLibraryRes(pub Option<GdtfLibrary>);

/// Below this, a fixture reads as blacked out — no spill light, no beam.
/// Keeps a dimmer sitting at DMX 1-2 (rounding noise, not an actual cue)
/// from drawing a visible beam.
const MIN_VISIBLE_DIMMER: f32 = 0.02;

/// How far a fixture can be from a piece of structure and still count as
/// mounted to it. Generous, because "on the truss" and "at ceiling
/// height" are the same thing to an operator and venue data rounds
/// differently — but finite, so a fixture out in open air stays anchored
/// to the world rather than snapping to something across the room.
const MAX_RIG_DISTANCE: f32 = 0.6;

/// The room's real world-space box, corners rotated.
///
/// `Venue::room_extent` takes each surface's `position ± size/2` in the
/// surface's *own* axes and ignores its orientation, which is right for
/// a room of unrotated boxes and wrong for this one: Norco's walls are
/// specified in a frame where the long axis is not the room's, so the
/// extent comes out 25 m tall and 6 m deep — a volume standing on its
/// end, mostly below the floor.
///
/// Nothing noticed while the fog covered the whole frustum regardless.
/// The froxel path clips to the volume, as the march's hull does, and
/// a beam ten metres into the room fell outside it — which is how a
/// room of beams rendered with none.
///
/// This rotates each box's eight corners before taking the union, and
/// applies the same base-pivot correction the spawn loop does for walls
/// and faces. `room_extent` is left alone: `BeamThrow` is calibrated
/// against it.
///
/// It also unions in every fixture's own position. The room records are
/// the *surfaces* — floor, walls, ceiling — and the truss hangs outside
/// them, so a volume built from surfaces alone stops below the lamps
/// and every beam starts in clean air. The comment at the call site has
/// always said the fog "has to reach past every wall, the fixtures and
/// the floor"; this is that sentence, made true.
// r[impl viz.beam-reach] - the haze volume is the room's rotated extent
fn haze_bounds(venue: &crate::venue::Venue) -> (Vec3, Vec3) {
    let (mut min, mut max) = (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY));
    for g in &venue.room {
        let rot = g.orientation();
        let size = g.size.to_vec3().max(Vec3::splat(0.02));
        let pos = g.position.to_vec3();
        let centre = if g.name.starts_with("Wall") || g.name.starts_with("Face") {
            pos + rot * Vec3::Z * (size.z * 0.5)
        } else {
            pos
        };
        let half = size * 0.5;
        for corner in 0..8 {
            let signs = Vec3::new(
                if corner & 1 == 0 { -1.0 } else { 1.0 },
                if corner & 2 == 0 { -1.0 } else { 1.0 },
                if corner & 4 == 0 { -1.0 } else { 1.0 },
            );
            let world = centre + rot * (half * signs);
            min = min.min(world);
            max = max.max(world);
        }
    }
    for f in &venue.fixtures {
        let at = f.position.to_vec3();
        min = min.min(at);
        max = max.max(at);
    }

    if !min.is_finite() || !max.is_finite() {
        return venue.room_extent();
    }
    (min, max)
}

/// Maps the operator's haze dial onto Bevy's `FogVolume::density_factor`,
/// whose own default is 0.1. Above roughly 0.3 the room stops being a
/// room: the fog absorbs its own light shafts and everything goes flat.
///
/// 0.11 while every spill light was cut off a few metres past its
/// fixture, so only the air right under the rig was ever lit. Now the
/// spill reaches the whole room the way light does, and forty-eight
/// pars aimed across the house light every metre of air between the
/// back wall and the stage; at 0.11 the shipped 1.6 put the house view
/// behind a brown veil. Halved, so 1.6 lands near Bevy's own default.
// r[impl viz.exposure] - haze is a density, calibrated against a lit room
const VOLUMETRIC_HAZE_SCALE: f32 = 0.05;

/// `FogVolume::density_factor` for the operator's dial and the room's
/// hazer level (`haze::HazeLevel`): the dial is the multiplier, the
/// hazers are what put the haze there.
// r[impl viz.haze-is-volumetric] - density is dial x hazer output
pub fn haze_density(haze: f32, hazer_level: f32) -> f32 {
    haze * VOLUMETRIC_HAZE_SCALE * hazer_level
}

/// `FogVolume::light_intensity`: a gain on what the fog scatters toward
/// the camera, without touching how much it dims the room behind it.
/// Bevy's fog at a density that leaves a twenty-metre house readable
/// scatters too little of a 1,600 cd par to show its cone; this is the
/// camera's aperture for the air, the way `exposure` is for surfaces.
pub const FOG_LIGHT_GAIN: f32 = 2.5;

/// `FogVolume::scattering_asymmetry`: how much the haze throws light
/// forward rather than sideways. Bevy's default of 0.8 is a clear-sky
/// aerosol — a shaft seen head-on is twenty times the same shaft seen
/// square across, so a mover swung toward the house whited the room
/// out while forty-eight pars seen from the side barely showed. Glycol
/// haze is nearer isotropic; at 0.3 a par's cone reads from the seats
/// and a beam into the camera is bright, not a veil.
pub const FOG_SCATTERING_ASYMMETRY: f32 = 0.3;

/// How strongly the haze scatters the light that crosses it: Bevy's own
/// default. It sat at 0.6 while the spill lights were range-clipped;
/// with the spill reaching the whole room, the density above is the
/// dial that matters and this one is left at the engine's default.
const HAZE_SCATTERING: f32 = 0.3;

/// How much of the light crossing a metre of haze is lost to it —
/// Bevy's fog volume computes `density * (absorption + scattering)` per
/// metre, and the haze camera's occluders write the same term so the
/// composite dims the room exactly as the in-camera fog would have.
// r[impl viz.performance-budget] - one extinction, two renderers
pub fn haze_extinction_per_metre(haze: f32, hazer_level: f32) -> f32 {
    haze_density(haze, hazer_level) * (HAZE_ABSORPTION + HAZE_SCATTERING)
}

/// `FogVolume::absorption`, as spawned below.
const HAZE_ABSORPTION: f32 = 0.05;

/// How brightly a lit fixture's housing glows in the colour it is
/// emitting, at full dimmer.
///
/// Deliberately dull — just enough to pick every fixture out and read
/// what colour it is in, and nowhere near enough to compete with what it
/// is actually throwing. It was 14, which haloed every lens hard enough
/// that the rig read as a wall of glowing dots rather than as light in a
/// room. The light coming out should speak for itself.
///
/// A *dark* fixture emits nothing. It briefly had a faint always-on glow
/// so the rig could be found in a blacked-out room, which is no longer
/// needed now the fixtures actually light it — and a housing that glows
/// when its lamp is off is not something any fixture does.
const LIT_BODY_GLOW: f32 = 0.22;

/// How hot a bar's cell faces run relative to the colour they emit: hot
/// enough for bloom to halo the strip the way a lit LED bar does.
const BAR_FACE_GLOW: f32 = 4.0;

/// How much of a bar's spill cone stays at full before the edge falls
/// off — see `update_beams`.
const BAR_INNER_FRACTION: f32 = 0.9;
/// The narrowest a strip's wash spreads on its short axis, as a half
/// angle. A lensless LED bar floods 100-120 degrees across its width —
/// that is why an uplight bar set against a wall washes it from the
/// skirting up. The borrowed Ultra Bar profile says 40 degrees, and at
/// 40 a bar 13 cm out from the wall did not touch it until 0.4 m up
/// ("their light doesn't appear until like halfway up the wall").
// r[impl viz.bar-emitters] - a bar floods across its width
const BAR_MIN_HALF_ANGLE_DEG: f32 = 60.0;

/// What a fixture's housing emits as spawned: nothing. A body is matte
/// black until `update_fixture_bodies` decides otherwise, and with the
/// glow off (the default) it never does.
// r[impl viz.body-glow] - spawned matte
const FIXTURE_GLOW: f32 = 0.0;

/// The housing's base colour with the glow off: near-black, so a par
/// reads as the black can it is and not as a coloured block.
const BODY_GREY: f32 = 0.03;

/// How brightly a screen's own content emits. A TV in a dark room is
/// genuinely one of the brighter things in it, but this is well under
/// what a lit fixture puts out so the rig still reads as the light
/// source.
const SCREEN_GLOW: f32 = 2.2;

/// How far below the surface a rigged fixture hangs — clearance for the
/// clamp and yoke, and what keeps a fixture from being coplanar with the
/// ceiling it hangs from.
const RIG_DROP: f32 = 0.08;

// Room palette, carried over from the pre-Bevy renderer — every one of
// these was an operator call at some point, so they are kept verbatim
// rather than re-picked. What changed is that they are now base colours
// on a PBR material instead of raw vertex colours: the procedural
// surface texturing the old shader did (plank grain, ashlar blocks,
// ceiling grid) has no equivalent yet and is tracked separately.
const ROOM_COLOR: Color = Color::srgb(0.42, 0.44, 0.48);
const CEILING_COLOR: Color = Color::srgb(0.16, 0.16, 0.17);
const COLUMN_COLOR: Color = Color::srgb(0.24, 0.235, 0.20);
const COLUMN_CAP_COLOR: Color = Color::srgb(0.05, 0.05, 0.05);
const PILLAR_COLOR: Color = Color::srgb(0.09, 0.07, 0.06);
const MOUNT_BOX_COLOR: Color = Color::srgb(0.30, 0.31, 0.34);
const FLOOR_COLOR: Color = Color::srgb(0.26, 0.17, 0.11);
const STAGE_FLOOR_COLOR: Color = Color::srgb(0.055, 0.05, 0.045);
const SCREEN_COLOR: Color = Color::srgb(0.02, 0.02, 0.03);
const PROP_COLOR: Color = Color::srgb(0.48, 0.30, 0.62);

/// The loaded venue, kept around so the live-update system can re-read a
/// fixture's mount pose and personality each frame.
#[derive(Resource)]
pub struct VenueRes(pub Venue);

/// Live DMX state, shared with the sACN/Art-Net listener threads.
#[derive(Resource)]
pub struct DmxRes(pub DmxUniverses);

/// Global visualizer settings the operator can dial — the equivalent of
/// QLC+ / grandMA3's 3D-view ambient and haze sliders.
#[derive(Resource)]
pub struct VizSettings {
    /// How much particulate is in the air to scatter a beam into
    /// something visible — a hazer's fluid-output dial, roughly 0..2,
    /// with 1.0 a normally hazed room. At 0 a beam is inert, the way it
    /// would genuinely look in clean air.
    ///
    /// A multiplier on the room's hazer level (`haze::HazeLevel`), not
    /// a density of its own: the hazers put the haze there, this dial
    /// says how much of it to show. `haze_density` maps the product onto
    /// Bevy's `density_factor`.
    pub haze: f32,
    /// Non-fixture room lighting. 0 by default: a real dark venue has no
    /// ambient fill, and everything you see is a fixture's beam or its
    /// spill. Only a dial back toward a lit room for readability.
    pub ambient: f32,
    /// Whether to draw the props layer — see `VizConfig::show_props`.
    pub show_props: bool,
    /// Whether to draw the operator overlay — see `VizConfig::overlay`.
    pub overlay: bool,
    /// Whether to draw the frame-rate readout — see `VizConfig::fps`.
    pub fps: bool,
    /// Whether a lit fixture's housing glows in the colour it is putting
    /// out. Off by default: the real fixtures are black boxes, and a
    /// par that lights up orange when it is sending orange reads as a
    /// lamp, not a luminaire. On, it is the "which of them is on"
    /// affordance `FixtureBody` describes.
    // r[impl viz.body-glow] - the switch, off unless asked
    pub body_glow: bool,
    /// Whether the deck takes the deferred path, for the camera's
    /// screen-space reflections — `RenderQuality::ssr`. A deferred
    /// material on a camera with no deferred prepass is a panic in
    /// Bevy's prepass queue, so the two are one switch.
    pub reflective_deck: bool,
    /// Asset path of what every screen displays, or `None` for screens
    /// that are off. One image for all of them is the placeholder; real
    /// projection mapping needs per-surface content and its own
    /// addressing, which this is the first piece of.
    pub screen_content: Option<String>,
    /// Per-canvas sources, by canvas name. A canvas with no entry here
    /// falls back to `screen_content`, which is what makes the flag
    /// optional rather than a new requirement.
    ///
    /// A source is a still or a clip depending on its extension alone —
    /// `.png` and `.webp` go through the asset server as they always
    /// have, `.mov` and `.mp4` through `crate::video`. Deliberately no
    /// new syntax: a show file that had to declare which one it meant
    /// would be a show file that breaks when the content changes.
    pub canvas_content: std::collections::HashMap<String, String>,
    /// Where each canvas's cover-crop is centred in its source, 0..=1
    /// (top/left to bottom/right); absent means the middle. How a wide
    /// canvas is told to show the band with the face in it.
    pub canvas_focus: std::collections::HashMap<String, f32>,
    /// Where the asset server reads from — see `VizConfig::assets_dir`.
    ///
    /// Needed here because a clip is *not* loaded through the asset
    /// server: it has no Bevy loader, and it needs a decoder thread of
    /// its own rather than a one-shot load. Canvas paths stay relative
    /// to the same root either way, so this is what resolves one.
    pub assets_dir: String,
    /// Room/screen/prop objects whose name contains any of these are not
    /// drawn — `--exclude Ceiling` for a plan view that would otherwise
    /// just render the roof, and the escape hatch for a venue whose
    /// ceiling sits below its own truss (Norco's does; see the README's
    /// venue notes).
    pub exclude: Vec<String>,
}

impl VizSettings {
    fn skip(&self, name: &str) -> bool {
        self.exclude.iter().any(|e| name.contains(e.as_str()))
    }
}

/// Marks a fixture's root entity and remembers which venue record it came
/// from, so the live system can resolve it without a name lookup.
#[derive(Component)]
pub struct Fixture {
    pub index: usize,
    /// The fixture's mount rotation *in its parent's frame*. Live pan is
    /// composed onto this each frame. Stored rather than re-read from the
    /// venue record because a rigged fixture's parent is the surface it
    /// hangs from, not the world.
    pub base_rot: Quat,
}

/// The part of a moving head that tilts — a child of the fixture root, so
/// tilting it is one `Transform` write and the base stays put.
#[derive(Component)]
pub struct FixtureHead;

/// An entity whose pose *is* the lens: its `GlobalTransform` translation
/// is where the beam starts and its local -Z is the aim.
///
/// This is the seam that lets both fixture paths share one beam
/// implementation. For a QLC+ profile it sits on the head (or the body,
/// for something that does not tilt); for a real GDTF profile it is the
/// file's own `<Beam>` node, several joints deep. Either way the beam and
/// spill hang off it as children and the transform hierarchy has already
/// worked out where they are by the time they are read — no code
/// recomposes a world matrix by hand.
#[derive(Component)]
pub struct BeamEmitter {
    pub fixture: usize,
}

/// What the live resolve decided this emitter should be doing, written by
/// `update_live_fixtures` and read after transforms propagate. Separated
/// because the beam's world pose is not known until propagation has run,
/// but its colour is known before.
#[derive(Component, Default)]
pub struct EmitterState {
    /// `None` when the fixture is dark.
    pub color: Option<[f32; 3]>,
    /// The beam half-angle: the 50% edge, the bright shaft.
    pub half_angle_deg: f32,
    /// The field half-angle: the 10% edge, where the spill's outer cone
    /// sits. A par's field is well outside its beam; a beam fixture's
    /// barely is — that difference is what makes one a soft wash and the
    /// other a hard shaft.
    pub field_half_angle_deg: f32,
    /// The fixture's own luminous output at full, before exposure.
    pub lumens: f32,
}

/// A surface that displays content — a TV, and in time any mapped
/// projection surface. Its own emissive texture is what it shows, so it
/// reads as a lit display in a dark room rather than as a panel that
/// happens to be lit by the rig.
#[derive(Component)]
pub struct ScreenSurface;

/// A fixture's housing material, so its body can show what it is
/// currently doing.
///
/// A lit fixture glows in the colour it is actually putting out; a dark
/// one emits nothing. Without this the rig looks identical whether it is
/// on or off, which was reported as "I don't really see all the ceiling
/// pars on" — the light was there, but nothing about the fixtures said
/// which of them were producing it.
///
/// A fixture drawn from primitives or a 3DS/OBJ mesh has one material,
/// the venue's body material, in `material`. A fixture drawn from its
/// profile's GLB arrives with the *file's* materials on its meshes —
/// one set of handles shared by every fixture of that type, which is
/// why the hover tint never reached a par (nothing pointed the fixture
/// at those handles) and why tinting them naively would have lit all
/// forty-eight pars at once. `gdtf_assets::adopt_scene_materials`
/// clones each such material once per fixture and lists the clone in
/// `parts`, so this component names every material a fixture owns.
#[derive(Component, Default)]
pub struct FixtureBody {
    pub material: Handle<StandardMaterial>,
    /// Per-fixture clones of the GLB's materials, one per distinct
    /// source material — a body in three meshes sharing one material
    /// gets one clone.
    pub parts: Vec<PartMaterial>,
}

/// One material a fixture's GLB parts came with, cloned for this fixture.
#[derive(Clone, Debug)]
pub struct PartMaterial {
    /// The file's material, as the loader handed it out.
    pub source: AssetId<StandardMaterial>,
    /// This fixture's own copy — the one its meshes now draw with.
    pub material: Handle<StandardMaterial>,
    /// What the file's material emitted before we touched it. The glow
    /// and the tint are added on top of this, never over it.
    pub base_emissive: LinearRgba,
}

impl PartMaterial {
    /// A part the file already makes emissive is a lens, and stays the
    /// file's colour on hover: the tint marks the housing, not the glass.
    pub fn is_lens(&self) -> bool {
        self.base_emissive != LinearRgba::BLACK
    }
}

impl FixtureBody {
    pub fn new(material: Handle<StandardMaterial>) -> Self {
        Self {
            material,
            parts: Vec::new(),
        }
    }

    /// Every material a hover or selection tint applies to: the body
    /// material and each non-lens GLB part.
    pub fn tintable(&self) -> impl Iterator<Item = &Handle<StandardMaterial>> {
        std::iter::once(&self.material).chain(
            self.parts
                .iter()
                .filter(|p| !p.is_lens())
                .map(|p| &p.material),
        )
    }

    /// Whether `handle` is one of this fixture's own — the body material
    /// or a clone — rather than a file material still to be adopted.
    pub fn owns(&self, handle: &Handle<StandardMaterial>) -> bool {
        self.material.id() == handle.id()
            || self.parts.iter().any(|p| p.material.id() == handle.id())
    }
}

/// The profile's own optics, on every emitter of a fixture whose profile
/// resolved: the `<Beam>` node's BeamAngle and FieldAngle as half-angles.
/// The profile is the single source of truth for how wide a light is —
/// the patch's `beam_angle_deg` is consulted only for a fixture with no
/// profile at all (see `fixture_optics`).
// r[impl viz.profile-optics] - the profile's angles ride on the emitter
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct ProfileOptics {
    pub beam_half_deg: f32,
    pub field_half_deg: f32,
}

/// The one rule for how wide a fixture's light is. A resolved profile's
/// `<Beam>` angles win outright; a fixture with no profile falls back to
/// the patch's single angle as its beam, with the field assumed at
/// twice that. The patch used to win whenever it carried a number — so
/// a par patched at 30 degrees and profiled at 30/60 was drawn as a
/// 30-degree hard cone, which is the "par cones too narrow" report.
// r[impl viz.profile-optics] - profile first, patch only as the fallback
pub fn fixture_optics(
    f: &crate::venue::FixtureRecord,
    profile: Option<&GdtfFixture>,
) -> ProfileOptics {
    match profile.and_then(GdtfFixture::optics) {
        Some((beam, field)) => {
            let beam_half = (beam * 0.5).clamp(0.05, MAX_FIELD_HALF_ANGLE_DEG);
            ProfileOptics {
                beam_half_deg: beam_half,
                field_half_deg: (field * 0.5).clamp(beam_half, MAX_FIELD_HALF_ANGLE_DEG),
            }
        }
        None => {
            let beam_half = beam_half_angle_deg(f.beam_angle_deg);
            ProfileOptics {
                beam_half_deg: beam_half,
                field_half_deg: assumed_field_half_angle_deg(beam_half),
            }
        }
    }
}

/// The lens a beam leaves through, as a radius in metres.
///
/// A fixture is not a point and it matters most exactly where a point
/// model is worst: at the apex, where the inverse square runs away and
/// the cone is narrower than a pixel of the haze buffer. Give the light
/// its real lens and the near field is bounded and a couple of hundred
/// millimetres wide, which is both what the fixture looks like and what
/// the raymarch can resolve.
///
/// Zero by default, which is upstream's behaviour, because a lens does
/// **not** fix the blocky cone apex — that was the theory and it was
/// measured wrong. Setting it changes the near field markedly (79% of
/// pixels move between 75 mm and 2 m), so it is a look decision, not a
/// bug fix, and it stays off until someone decides they prefer it.
///
/// The clamp it feeds is still worth having and still correct: without
/// it `1/d²` runs to infinity at the light's position. Nothing is ever
/// *at* a light on a surface, so upstream never has to care; in fog it
/// is a singularity in the middle of the picture.
///
/// GDTF carries `BeamRadius` per beam geometry, so the honest version of
/// this reads it per fixture rather than taking one number for the rig.
/// `IGNITION_LENS_RADIUS` sets it, for comparing.
// r[impl viz.haze-is-volumetric] - a beam may start at the lens, not at a point
pub const DEFAULT_LENS_RADIUS_M: f32 = 0.0;

pub fn lens_radius() -> f32 {
    std::env::var("IGNITION_LENS_RADIUS")
        .ok()
        .and_then(|v| v.trim().parse::<f32>().ok())
        .unwrap_or(DEFAULT_LENS_RADIUS_M)
}

/// A fixture's spill: the light it actually throws onto the room, as
/// and, through the haze, the shaft it shows in the air. Also a
/// child of the emitter, with an identity local transform — a Bevy spot
/// light shines along its entity's -Z, which is already this project's
/// beam-axis convention, so it needs no aiming code at all.
#[derive(Component)]
pub struct FixtureSpill;

/// A linear source: an LED bar's whole front, as one emitter.
///
/// A bar's GDTF profile carries one `<Beam>` per cell — twelve on the
/// Ultra Bar 12 — in a line across the housing. Drawn as twelve point
/// emitters that is twelve pencil cones and twelve dots, which is not
/// what a batten does: it puts up a wide flat wash from the whole strip.
/// So the cells are folded into one emitter at the strip's centre, whose
/// local X runs along the strip and whose -Z is the aim, with the
/// strip's extent kept here to size the emissive face and the wedge.
// r[impl viz.bar-emitters] - the strip is one emitter, not a row of points
#[derive(Component)]
pub struct BarEmitter {
    /// Half the strip's length along the emitter's local X: from the
    /// outermost `<Beam>` nodes, plus half a cell pitch beyond each so
    /// the end cells are whole.
    pub half_length: f32,
    /// Half the strip's width along local Y — the cell pitch, capped by
    /// the housing.
    pub half_width: f32,
    /// The emissive face of each cell, in bar order, so a cell can show
    /// its own colour.
    pub cells: Vec<Handle<StandardMaterial>>,
}

/// Cells this close to one line, and this parallel, are one strip. A
/// bar's cells sit on a line to within the file author's rounding;
/// a real multi-lens fixture that is not a bar (a derby, a spider with
/// two heads) fails one of the two.
const BAR_LINE_TOLERANCE: f32 = 0.01;

/// A multi-`<Beam>` profile is a bar when four or more beams fire the
/// same way from points on one line square to that aim. Returns the
/// strip: its centre pose (local X along the strip, -Z the aim) and the
/// half-length and cell pitch.
// r[impl viz.bar-emitters] - four or more beams on a line are a bar
pub fn bar_strip(beams: &[(Vec3, Quat)]) -> Option<(Vec3, Quat, f32, f32)> {
    if beams.len() < 4 {
        return None;
    }
    let aim = (beams[0].1 * Vec3::NEG_Z).normalize_or_zero();
    if beams
        .iter()
        .any(|(_, r)| (r * Vec3::NEG_Z).dot(aim) < 0.999)
    {
        return None;
    }
    // The strip runs from the first cell to the one farthest from it.
    let first = beams[0].0;
    let last = beams
        .iter()
        .map(|(p, _)| *p)
        .max_by(|a, b| a.distance(first).total_cmp(&b.distance(first)))
        .unwrap_or(first);
    let span = last - first;
    let length = span.length();
    if length < BAR_LINE_TOLERANCE {
        return None;
    }
    let along = span / length;
    // Square to the aim: a strip fires out of its face, not along it.
    if along.dot(aim).abs() > 0.05 {
        return None;
    }
    // Every cell on that line.
    if beams.iter().any(|(p, _)| {
        let d = *p - first;
        (d - along * d.dot(along)).length() > BAR_LINE_TOLERANCE
    }) {
        return None;
    }
    let pitch = length / (beams.len() - 1) as f32;
    let centre = (first + last) * 0.5;
    // A frame whose X is the strip and whose -Z is the aim.
    let x = along;
    let z = -aim;
    let y = z.cross(x).normalize_or_zero();
    let rot = Quat::from_mat3(&Mat3::from_cols(x, y, z));
    Some((centre, rot, length * 0.5 + pitch * 0.5, pitch))
}

/// A cache key for one way of splitting a QLC+ mesh: the split height's
/// bits and which half, or zero for the whole.
fn split_key(split: Option<(f32, bool)>) -> u64 {
    match split {
        None => 0,
        Some((z, upper)) => (u64::from(z.to_bits()) << 1) | u64::from(upper) | (1 << 63),
    }
}

/// Spawns a bar's one emitter: the strip's centre pose under the node
/// the cells hang from (so the file's own tree carries it — see
/// `GdtfNode::bar_cells`), a thin emissive face per cell tiling the
/// strip, and one wide spill light. The spill is a single volumetric spot
/// with no shadow map — the haze shows its wash as one wide fan, and
/// twelve shadowed spots was what drew the dots.
// r[impl viz.bar-emitters] - the emissive face and one spill per bar
// r[impl viz.haze-is-volumetric] - a bar's light in the air is its spill in the fog
#[allow(clippy::too_many_arguments)]
fn spawn_bar_emitter(
    commands: &mut Commands,
    parent: Entity,
    fixture: usize,
    name: &str,
    centre: Vec3,
    rot: Quat,
    half_length: f32,
    pitch: f32,
    cells: &[(Vec3, Quat)],
    meshes: &mut Assets<Mesh>,
    standard: &mut Assets<StandardMaterial>,
    optics: ProfileOptics,
    spill_range: f32,
) {
    // A cell face is a square of the pitch, capped so a sparse bar does
    // not read as a slab; proud of the housing by the face's own depth.
    let half_width = (pitch * 0.5).min(0.03);
    const FACE_DEPTH: f32 = 0.004;
    let emitter = commands
        .spawn((
            BeamEmitter { fixture },
            EmitterState::default(),
            Transform {
                translation: centre,
                rotation: rot,
                scale: Vec3::ONE,
            },
            Visibility::default(),
            Name::new(format!("{name} strip")),
            ChildOf(parent),
        ))
        .id();

    let along = rot * Vec3::X;
    let face = meshes.add(Cuboid::new(pitch * 0.9, half_width * 2.0, FACE_DEPTH));
    let mut cell_materials = Vec::with_capacity(cells.len());
    for (pos, _) in cells {
        let x = (*pos - centre).dot(along);
        let material = standard.add(StandardMaterial {
            base_color: Color::srgb(0.06, 0.06, 0.06),
            emissive: LinearRgba::BLACK,
            perceptual_roughness: 0.5,
            ..default()
        });
        commands.spawn((
            Mesh3d(face.clone()),
            MeshMaterial3d(material.clone()),
            Transform::from_translation(Vec3::new(x, 0.0, -FACE_DEPTH * 0.5)),
            ChildOf(emitter),
        ));
        cell_materials.push(material);
    }
    commands.entity(emitter).insert((
        BarEmitter {
            half_length,
            half_width,
            cells: cell_materials,
        },
        optics,
    ));

    commands.spawn((
        FixtureSpill,
        SpotLight {
            intensity: 0.0,
            range: spill_range,
            shadow_maps_enabled: false,
            // The lens, not a point — see `lens_radius`.
            radius: lens_radius(),
            ..default()
        },
        VolumetricLight,
        bevy::camera::visibility::RenderLayers::from_layers(&[0, crate::haze::HAZE_LAYER]),
        Transform::default(),
        Visibility::Hidden,
        Name::new(format!("{name} spill")),
        ChildOf(emitter),
    ));
}

/// A piece of room structure a fixture can be rigged to: the ceiling, a
/// wall, the deck, the truss beam over the drums.
#[derive(Clone, Copy)]
pub struct RigSurface {
    pub entity: Entity,
    pub center: Vec3,
    pub rot: Quat,
    pub half_extents: Vec3,
    /// Whether this is the ceiling, which needs the clamp below.
    pub is_ceiling: bool,
}

impl RigSurface {
    /// Distance from a point to this surface's box, zero inside it.
    pub fn distance_to(&self, point: Vec3) -> f32 {
        let local = (self.rot.inverse() * (point - self.center)).abs() - self.half_extents;
        local.max(Vec3::ZERO).length() + local.max_element().min(0.0)
    }
}

/// The structure a fixture is mounted to: whichever candidate surface it
/// is physically closest to, or `None` if it is not near anything.
///
/// Nearest-surface rather than a rule per fixture type, because that is
/// what the operator described and it is what the geometry already says:
/// the truss pars are under the ceiling, the back-wall pars are against
/// the back wall, the overhead movers hang off the beam over the drums,
/// and the strips and floor movers sit on the deck. Rigging each to what
/// it is actually attached to means moving that structure moves them.
pub fn nearest_rig_surface(fixture_pos: Vec3, surfaces: &[RigSurface]) -> Option<RigSurface> {
    surfaces
        .iter()
        .map(|s| (s, s.distance_to(fixture_pos)))
        .filter(|(_, d)| *d <= MAX_RIG_DISTANCE)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(s, _)| *s)
}

/// A fixture's pose expressed relative to the structure it hangs from, so
/// moving that structure moves the fixture with it.
///
/// The ceiling gets one correction the others do not: venue data can put
/// a truss *above* the room's own ceiling plane — Norco's does, by half a
/// metre — and a fixture left there is invisible from inside the room and
/// shines its beam from outside the roof. Every other surface keeps the
/// fixture's real measured offset, because there is no equivalent
/// contradiction to resolve and inventing one would move fixtures that
/// are already right.
pub fn rig_to_surface(fixture_pos: Vec3, fixture_rot: Quat, surface: &RigSurface) -> (Vec3, Quat) {
    let inv = surface.rot.inverse();
    let mut local = inv * (fixture_pos - surface.center);
    if surface.is_ceiling {
        local.z = local.z.min(-RIG_DROP);
    }
    (local, inv * fixture_rot)
}

/// A plain matte surface. `emissive` scales the base colour into an
/// emissive term, so a fixture's own housing can glow with what it is
/// putting out while room geometry stays lit only by the rig.
fn solid(
    materials: &mut Assets<StandardMaterial>,
    color: Color,
    emissive: f32,
) -> Handle<StandardMaterial> {
    let base = LinearRgba::from(color);
    materials.add(StandardMaterial {
        base_color: color,
        emissive: LinearRgba::rgb(
            base.red * emissive,
            base.green * emissive,
            base.blue * emissive,
        ),
        perceptual_roughness: 0.9,
        ..default()
    })
}

/// The stage deck: black plywood with a coat of paint on it, so the rig
/// and the band show in it a little. Roughness low enough for the
/// screen-space reflections to pick it up (see `app::spawn_camera`),
/// and the one material that takes the deferred path, which is what
/// those reflections march against. On a camera without a deferred
/// prepass it is drawn forward like everything else.
// r[impl viz.post-processing] - the deck is the one reflective surface
fn deck(
    materials: &mut Assets<StandardMaterial>,
    color: Color,
    deferred: bool,
) -> Handle<StandardMaterial> {
    use bevy::material::OpaqueRendererMethod;
    materials.add(StandardMaterial {
        base_color: color,
        perceptual_roughness: DECK_ROUGHNESS,
        metallic: 0.0,
        reflectance: 0.5,
        opaque_render_method: if deferred {
            OpaqueRendererMethod::Deferred
        } else {
            OpaqueRendererMethod::Auto
        },
        ..default()
    })
}

/// A wet-look deck: reflective enough to mirror a beam, rough enough
/// that the reflection is a smear and not a second rig.
pub const DECK_ROUGHNESS: f32 = 0.25;

/// A screen showing content: emitting its own image rather than merely
/// reflecting the room's light.
/// A display material lit by `content` — what a screen panel and a
/// camera quad both wear.
pub fn display_material(
    materials: &mut Assets<StandardMaterial>,
    content: Handle<Image>,
) -> Handle<StandardMaterial> {
    display(materials, content)
}

fn display(
    materials: &mut Assets<StandardMaterial>,
    content: Handle<Image>,
) -> Handle<StandardMaterial> {
    materials.add(StandardMaterial {
        base_color: Color::BLACK,
        base_color_texture: Some(content.clone()),
        emissive: LinearRgba::rgb(SCREEN_GLOW, SCREEN_GLOW, SCREEN_GLOW),
        emissive_texture: Some(content),
        ..default()
    })
}

/// The time the video canvases are presented at.
///
/// A canvas is not a media player with a play button — it is a picture
/// of the song at a point in time, the same way a cue is. Set `seconds`
/// from the transport every frame and a scrub drags the graphics with
/// the music, backwards as readily as forwards. That is the whole reason
/// `VideoSource::frame_at` takes a time instead of keeping one:
///
/// ```no_run
/// # use ignition_viz::spawn::CanvasClock;
/// # fn wire(world: &mut bevy::prelude::World, position: f64) {
/// world.insert_resource(CanvasClock::at(position));
/// # }
/// ```
///
/// `free_running` is for the standalone `viz` binary, which has no
/// transport to follow and would otherwise show one frozen frame.
#[derive(Resource, Debug, Clone, Copy)]
pub struct CanvasClock {
    /// Where in the song we are, in seconds.
    pub seconds: f64,
    /// Advance `seconds` with the app's own elapsed time, because
    /// nothing outside is setting it.
    pub free_running: bool,
}

impl Default for CanvasClock {
    fn default() -> Self {
        Self {
            seconds: 0.0,
            free_running: true,
        }
    }
}

impl CanvasClock {
    /// A clock pinned to a supplied position — what a host driving the
    /// visualizer from a transport inserts each frame.
    pub fn at(seconds: f64) -> Self {
        Self {
            seconds,
            free_running: false,
        }
    }
}

/// What a moving canvas is showing on the texture path: a decoded clip.
/// A generated picture presents against the same clock, but as a
/// material rather than a texture — see `canvas_material.rs`.
// r[impl canvas.clip-is-a-source]
enum CanvasSource {
    Clip(VideoSource),
}

impl CanvasSource {
    fn size(&self) -> (u32, u32) {
        match self {
            CanvasSource::Clip(v) => v.size(),
        }
    }

    fn frame_at(&mut self, secs: f64) -> Option<&[u8]> {
        match self {
            CanvasSource::Clip(v) => v.frame_at(secs),
        }
    }
}

/// A canvas whose source is a clip rather than a still.
struct CanvasVideo {
    canvas: String,
    source: CanvasSource,
    image: Handle<Image>,
}

/// Every playing canvas, and the texture each one writes into.
///
/// The `Mutex` is not protecting anything: a `VideoSource` owns the
/// receiving end of a channel, which is `Send` but not `Sync`, and a
/// Bevy `Resource` has to be both. Every access below is through
/// `get_mut`, which does not lock.
#[derive(Resource)]
pub struct CanvasVideos(Mutex<Vec<CanvasVideo>>);

/// The texture a clip's frames land in, sized to the clip.
///
/// `RenderAssetUsages::all()` rather than the render world alone: the
/// CPU-side copy is the thing being written every frame, and dropping it
/// would leave nothing to write into after the first upload.
fn blank_frame((width, height): (u32, u32)) -> Image {
    Image::new(
        Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        vec![0; (width as usize) * (height as usize) * 4],
        // Srgb, matching what the still path gets from a PNG. The
        // sampled value feeds an emissive channel, and a linear texture
        // there reads as a screen with its gamma wound up.
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::all(),
    )
}

/// Puts the frame that belongs at the current clock into each playing
/// canvas's texture.
///
/// Nothing here decodes: a clip's `frame_at` only ever hands over what a
/// worker thread has already produced, and answers `None` when
/// what is on screen is already right. `None` is the *usual* answer — the visualizer runs
/// at 120 fps and a clip at 30 — and it is what keeps this from
/// re-uploading eight megabytes four times per clip frame.
pub fn update_canvas_videos(
    time: Res<Time>,
    mut clock: ResMut<CanvasClock>,
    mut videos: ResMut<CanvasVideos>,
    mut images: ResMut<Assets<Image>>,
) {
    if clock.free_running {
        clock.seconds += f64::from(time.delta_secs());
    }
    let seconds = clock.seconds;

    for entry in videos.0.get_mut().expect("canvas video lock").iter_mut() {
        let Some(frame) = entry.source.frame_at(seconds) else {
            continue;
        };
        let Some(mut image) = images.get_mut(&entry.image) else {
            continue;
        };
        let Some(data) = image.data.as_mut() else {
            continue;
        };
        if data.len() == frame.len() {
            data.copy_from_slice(frame);
        } else {
            // A decoder that changed frame size mid-clip. Nothing here
            // handles that, and silently writing a short row would be a
            // sheared picture rather than an obvious fault.
            tracing::warn!(
                canvas = entry.canvas,
                expected = data.len(),
                got = frame.len(),
                "viz: canvas frame is the wrong size; skipping it"
            );
        }
    }
}

/// Spawns the room, its screens and props, and one entity per patched
/// fixture (plus that fixture's beam and spill, initially hidden).
// A Bevy system's "arguments" are its data dependencies, which the
// scheduler reads to decide what can run in parallel — splitting this one
// up to satisfy an argument count would hide that, not simplify it.
#[allow(clippy::too_many_arguments)]
pub fn spawn_venue(
    mut commands: Commands,
    venue: Res<VenueRes>,
    settings: Res<VizSettings>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut standard: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut canvas_materials: ResMut<Assets<crate::canvas_material::CanvasMaterial>>,
    gdtf_library: Res<GdtfLibraryRes>,
    asset_server: Res<AssetServer>,
    mut programme_view: ResMut<crate::camera::ProgrammeView>,
    mut camera_sources: ResMut<crate::camera::CameraSources>,
) {
    let venue = &venue.0;
    let unit_cube = meshes.add(Cuboid::from_length(1.0));
    // Every piece of room structure a fixture might be mounted to.
    let mut rig_surfaces: Vec<RigSurface> = Vec::new();

    // Room surfaces are lit only by the rig, which is the point — but a
    // fixture that is switched off would then be invisible in a dark
    // room, and an operator needs to see where the rig physically *is*
    // whether or not it is currently doing anything. Fixture bodies get a
    // small emissive term so they read as their own objects: enough for
    // bloom to give them a soft presence, far below anything a lit beam
    // puts out.

    /// Room geometry emits nothing; only the rig lights it.
    const UNLIT: f32 = 0.0;

    for g in &venue.room {
        if settings.skip(&g.name) {
            continue;
        }
        let color = if g.name == "Ceiling" {
            CEILING_COLOR
        } else if g.name.starts_with("Stage") {
            STAGE_FLOOR_COLOR
        } else if g.name.starts_with("Floor") {
            FLOOR_COLOR
        } else if g.name.starts_with("Column") {
            COLUMN_COLOR
        } else if g.name.starts_with("Beam") {
            PILLAR_COLOR
        } else if g.name.starts_with("Mount Box") {
            MOUNT_BOX_COLOR
        } else {
            ROOM_COLOR
        };
        let pos = g.position.to_vec3();
        let rot = g.orientation();
        let size = g.size.to_vec3().max(Vec3::splat(0.02));
        // Walls and risers ("Face - ...") are pivoted at their BASE in the
        // source data, not their centre — confirmed by cross-checking wall
        // heights against the real ceiling height: e.g. Wall - Upstage's
        // base z (0.1524) plus its height (3.302) lands on 3.4544, the
        // ceiling, to four decimals, for every wall checked. Left
        // uncorrected every wall floats half its height too low, leaving a
        // dark band under the ceiling.
        let center = if g.name.starts_with("Wall") || g.name.starts_with("Face") {
            pos + rot * Vec3::Z * (size.z * 0.5)
        } else {
            pos
        };
        // Two entities per room object: an unscaled anchor at its pose,
        // and the box mesh scaled to its size underneath. The split is
        // what lets anything be *rigged to* a room surface — a child of
        // the scaled box would inherit the box's own stretch, so a par
        // hung from a 12x20x0.02m ceiling would come out 12m wide and
        // paper-thin. See `RoomAnchors`.
        let anchor = commands
            .spawn((
                Transform {
                    translation: center,
                    rotation: rot,
                    scale: Vec3::ONE,
                },
                Visibility::default(),
                Name::new(g.name.clone()),
            ))
            .id();
        let material = if g.name.starts_with("Stage") {
            deck(&mut standard, color, settings.reflective_deck)
        } else {
            solid(&mut standard, color, UNLIT)
        };
        commands.spawn((
            Mesh3d(unit_cube.clone()),
            MeshMaterial3d(material),
            Transform::from_scale(size),
            ChildOf(anchor),
        ));
        rig_surfaces.push(RigSurface {
            entity: anchor,
            center,
            rot,
            half_extents: size * 0.5,
            is_ceiling: g.name == "Ceiling",
        });
        if g.name.starts_with("Column") {
            // A black capstone on top of the column — its own entity
            // rather than trying to two-tone one box.
            let cap_height = 0.06;
            commands.spawn((
                Mesh3d(unit_cube.clone()),
                MeshMaterial3d(solid(&mut standard, COLUMN_CAP_COLOR, UNLIT)),
                Transform {
                    translation: Vec3::Z * (size.z * 0.5 + cap_height * 0.5),
                    scale: Vec3::new(size.x * 1.02, size.y * 1.02, cap_height),
                    ..default()
                },
                ChildOf(anchor),
            ));
        }
    }

    // Pillars are an architectural detail like the columns, not
    // set-dressing — always drawn, unlike the rest of props.json.
    for g in venue
        .props
        .iter()
        .filter(|g| g.name.starts_with("Pillar") && !settings.skip(&g.name))
    {
        commands.spawn((
            Mesh3d(unit_cube.clone()),
            MeshMaterial3d(solid(&mut standard, PILLAR_COLOR, UNLIT)),
            Transform {
                translation: g.position.to_vec3(),
                rotation: g.orientation(),
                scale: g.size.to_vec3().max(Vec3::splat(0.02)),
            },
            Name::new(g.name.clone()),
        ));
    }

    if settings.show_props {
        let mut person_index = 0usize;
        for g in &venue.props {
            // Pillars are drawn unconditionally above — architecture, not
            // dressing.
            if g.name.starts_with("Pillar") || settings.skip(&g.name) {
                continue;
            }
            // People and mic stands have real shapes now (see `props.rs`);
            // everything else is still a labelled box, which is honest
            // about how much is known about it.
            if g.name.starts_with("Person") {
                // The drummer is the one person whose pose is dictated
                // by the furniture; everyone else just stands.
                let pose = if g.name.contains("Drummer") {
                    crate::props::Pose::SittingAtDrums
                } else {
                    crate::props::Pose::Standing
                };
                crate::props::spawn_person(&mut commands, &asset_server, g, person_index, pose);
                person_index += 1;
            } else if g.name.starts_with("Drum Kit") {
                crate::props::spawn_drum_kit(&mut commands, &asset_server, g);
            } else if g.name.starts_with("Mic") {
                crate::props::spawn_mic_stand(&mut commands, &mut meshes, &mut standard, g);
            } else if let Some(prop) = crate::props::glb_prop_for(&g.name) {
                crate::props::spawn_glb_prop(
                    &mut commands,
                    &asset_server,
                    &mut meshes,
                    &mut standard,
                    g,
                    prop,
                );
            } else {
                commands.spawn((
                    Mesh3d(unit_cube.clone()),
                    MeshMaterial3d(solid(&mut standard, PROP_COLOR, UNLIT)),
                    Transform {
                        translation: g.position.to_vec3(),
                        rotation: g.orientation(),
                        scale: g.size.to_vec3().max(Vec3::splat(0.02)),
                    },
                    Name::new(g.name.clone()),
                ));
            }
        }
    }

    // One texture per canvas, and one slice per panel. Panels sharing a
    // canvas therefore share a texture and show different parts of it —
    // the difference between three screens playing one video and three
    // screens each playing their own copy.
    let slices = crate::canvas::slices(&venue.screens);
    let aspects = crate::canvas::canvas_aspects(&venue.screens);
    let mut canvas_content: std::collections::HashMap<String, Handle<Image>> =
        std::collections::HashMap::new();
    // Each canvas's source aspect, so a slice can be cover-fitted rather
    // than stretched. A still whose size cannot be read is taken as
    // 16:9, which every clip in the folder is.
    let mut source_aspect: std::collections::HashMap<String, f32> =
        std::collections::HashMap::new();
    let mut playing: Vec<CanvasVideo> = Vec::new();
    // `proc:` canvases: no texture at all — each panel gets a material
    // that paints the recipe itself (`canvas_material.rs`).
    let mut procedural: std::collections::HashMap<String, ignition_core::canvas::CanvasRecipe> =
        std::collections::HashMap::new();
    // Two canvases showing the same clip share one decoder and one
    // texture. The side TVs both play the same loop; decoding it twice
    // was two threads and two uploads for one picture.
    let mut by_path: std::collections::HashMap<String, Handle<Image>> =
        std::collections::HashMap::new();
    for screen in &venue.screens {
        let canvas = screen.canvas_name().to_string();
        if canvas_content.contains_key(&canvas) {
            continue;
        }
        let path = settings
            .canvas_content
            .get(&canvas)
            .or(settings.screen_content.as_ref());
        let Some(path) = path else { continue };
        // A camera as the source: the programme's target, or a preset
        // camera's, allocated here and drawn into once the camera is up.
        // r[impl canvas.camera-source] - `camera:programme` / `camera:<preset>` at spawn
        if let Some(source) = crate::camera::CameraSource::parse(path) {
            let target = camera_sources.target_for(&source, &mut programme_view, &mut images);
            let (w, h) = crate::camera::SOURCE_SIZE;
            source_aspect.insert(canvas.clone(), w as f32 / h as f32);
            canvas_content.insert(canvas, target);
            continue;
        }
        if let Some(handle) = by_path.get(path) {
            canvas_content.insert(canvas.clone(), handle.clone());
            if let Some(aspect) = playing.iter().find(|v| v.image == *handle).map(|v| {
                let (w, h) = v.source.size();
                w as f32 / h.max(1) as f32
            }) {
                source_aspect.insert(canvas, aspect);
            }
            continue;
        }
        match crate::video::content_kind(path) {
            ContentKind::Still => {
                let file = std::path::Path::new(&settings.assets_dir).join(path);
                if let Ok((w, h)) = image::image_dimensions(&file)
                    && h > 0
                {
                    source_aspect.insert(canvas.clone(), w as f32 / h as f32);
                }
                canvas_content.insert(canvas, asset_server.load(path.clone()));
            }
            ContentKind::Video => {
                let file = std::path::Path::new(&settings.assets_dir).join(path);
                match VideoSource::open(&file) {
                    Ok(source) => {
                        let (w, h) = source.size();
                        if h > 0 {
                            source_aspect.insert(canvas.clone(), w as f32 / h as f32);
                        }
                        let handle = images.add(blank_frame(source.size()));
                        canvas_content.insert(canvas.clone(), handle.clone());
                        by_path.insert(path.clone(), handle.clone());
                        playing.push(CanvasVideo {
                            canvas,
                            source: CanvasSource::Clip(source),
                            image: handle,
                        });
                    }
                    // One canvas going dark is not a reason to take the
                    // rig down — the lights are the show, and a screen
                    // with no content already renders as an unlit panel.
                    Err(error) => tracing::warn!(
                        canvas,
                        file = %file.display(),
                        %error,
                        "viz: canvas clip did not open; that screen stays dark"
                    ),
                }
            }
            // r[impl canvas.procedural] - a sweep on the back wall with no clip
            ContentKind::Procedural => match crate::canvas::parse_procedural(path) {
                Ok(recipe) => {
                    // A recipe is drawn at the canvas's own aspect, so
                    // the cover-fit below is the identity and the
                    // picture spans the panels edge to edge.
                    let aspect = aspects.get(&canvas).copied().unwrap_or(16.0 / 9.0);
                    source_aspect.insert(canvas.clone(), aspect);
                    procedural.insert(canvas, recipe);
                }
                Err(error) => tracing::warn!(
                    canvas,
                    content = path,
                    error,
                    "viz: canvas recipe did not parse; that screen stays dark"
                ),
            },
        }
    }
    commands.insert_resource(CanvasVideos(Mutex::new(playing)));

    for g in &venue.screens {
        if settings.skip(&g.name) {
            continue;
        }
        let rot = g.orientation();
        let size = g.size.to_vec3();
        // Same base-pivot convention as walls: a TV's position is its
        // bottom edge. The panel's local Y is its height axis, and its
        // local +Z is the face — which the venue's own euler angles
        // already point at the audience.
        let center = g.position.to_vec3() + rot * Vec3::Y * (size.y * 0.5);
        let depth = 0.05_f32.max(size.z);
        let body = commands
            .spawn((
                Transform {
                    translation: center,
                    rotation: rot,
                    scale: Vec3::ONE,
                },
                Visibility::default(),
                Name::new(g.name.clone()),
            ))
            .id();
        commands.spawn((
            Mesh3d(unit_cube.clone()),
            MeshMaterial3d(solid(&mut standard, SCREEN_COLOR, UNLIT)),
            Transform::from_scale(Vec3::new(size.x, size.y, depth)),
            ChildOf(body),
        ));
        // This panel's piece of its canvas, cover-fitted and focused.
        let slice = slices
            .get(&g.name)
            .copied()
            .unwrap_or(crate::canvas::Slice::FULL)
            .cover_at(
                aspects.get(g.canvas_name()).copied().unwrap_or(16.0 / 9.0),
                source_aspect
                    .get(g.canvas_name())
                    .copied()
                    .unwrap_or(16.0 / 9.0),
                settings
                    .canvas_focus
                    .get(g.canvas_name())
                    .copied()
                    .unwrap_or(0.5),
            );
        if let Some(recipe) = procedural.get(g.canvas_name()) {
            // A generated picture: the material paints it at the
            // screen's own resolution, no texture in between.
            let panel = crate::canvas_material::spawn_panel(
                &mut commands,
                body,
                recipe,
                slice,
                size,
                depth,
                SCREEN_GLOW,
                &mut canvas_materials,
                &mut meshes,
            );
            commands.entity(panel).insert(crate::camera::CanvasPanel {
                canvas: g.canvas_name().to_string(),
                slice: slices
                    .get(&g.name)
                    .copied()
                    .unwrap_or(crate::canvas::Slice::FULL),
                size,
                depth,
                body,
            });
        } else if let Some(content) = canvas_content.get(g.canvas_name()) {
            // The display itself: a quad just proud of the bezel, lit by
            // its own content rather than by the room.
            commands.spawn((
                ScreenSurface,
                crate::camera::CanvasPanel {
                    canvas: g.canvas_name().to_string(),
                    slice: slices
                        .get(&g.name)
                        .copied()
                        .unwrap_or(crate::canvas::Slice::FULL),
                    size,
                    depth,
                    body,
                },
                Mesh3d(meshes.add(crate::canvas::sliced_quad(slice))),
                MeshMaterial3d(display(&mut standard, content.clone())),
                Transform {
                    translation: Vec3::Z * (depth * 0.5 + 0.005),
                    scale: Vec3::new(size.x * 0.94, size.y * 0.94, 1.0),
                    ..default()
                },
                ChildOf(body),
            ));
        }
    }

    {
        // One volume covering the room. Haze is a property of the air in
        // here, not of any one fixture, which is the whole reason this
        // reads better than a cone per beam: two beams crossing actually
        // brighten where they overlap.
        // The room's real extent, not the centre bounds — the fog has
        // to reach past every wall, the fixtures and the floor, not
        // stop at them.
        // r[impl viz.beam-reach] - haze fills the whole room
        let (min, max) = haze_bounds(venue);
        let center = (min + max) * 0.5;
        // Twice the room, so a beam that escapes through a gap in the
        // venue's walls dies of distance long before it meets the
        // volume's hard edge.
        let size = (max - min).max(Vec3::splat(4.0)) * 1.3;
        commands.spawn((
            FogVolume {
                density_factor: haze_density(
                    settings.haze,
                    crate::haze::HazeLevel::default().level,
                ),
                scattering: HAZE_SCATTERING,
                absorption: HAZE_ABSORPTION,
                light_intensity: FOG_LIGHT_GAIN,
                scattering_asymmetry: FOG_SCATTERING_ASYMMETRY,
                ..default()
            },
            Transform {
                translation: center,
                scale: size,
                ..default()
            },
            Name::new("Haze"),
        ));
    }

    let throw = BeamThrow::for_venue(venue);
    // Which fixtures get a shadow map: the brightest per-direction, up
    // to the budget. Decided over the whole rig before anything is
    // spawned, because it is a ranking, not a per-fixture property.
    // r[impl viz.performance-budget] - shadow maps are budgeted across the rig
    let shadowed = {
        let candidates: Vec<crate::budget::ShadowCandidate> = venue
            .fixtures
            .iter()
            .map(|f| {
                let manufacturer = f.manufacturer.as_deref().unwrap_or("");
                let model = f.model.as_deref().unwrap_or("");
                let profile = gdtf_library
                    .0
                    .as_ref()
                    .and_then(|lib| lib.find(manufacturer, model));
                let optics = fixture_optics(f, profile);
                let candela = peak_candela(
                    power_watts(manufacturer, model) * LUMENS_PER_WATT,
                    optics.beam_half_deg,
                );
                crate::budget::ShadowCandidate {
                    candela,
                    cuts_a_shaft: f.patched && candela >= SHAFT_CANDELA_THRESHOLD,
                    moves: fixture_moves(manufacturer, model, profile),
                }
            })
            .collect();
        crate::budget::shadow_budget(&candidates, crate::budget::shadow_budget_setting())
    };
    // One mesh asset per model, shared by every fixture of the type.
    let mut shared_meshes = gdtf_geometry::SharedMeshes::new(&mut meshes);
    for (index, f) in venue.fixtures.iter().enumerate() {
        // Unpatched channels (Norco's phantom 19/98) have no real
        // position — the patch reports (0,0,0), which would render as a
        // stray fixture at the room's origin.
        if !f.patched {
            continue;
        }
        let manufacturer = f.manufacturer.as_deref().unwrap_or("");
        let model = f.model.as_deref().unwrap_or("");
        // A real fixture is a black box. The category tint (wash blue,
        // mover orange) is a reading aid, and it only makes sense next
        // to the glow it belongs with; with the glow off the housing is
        // near-black matte, lit only by the rig around it.
        // r[impl viz.body-glow] - no tint without the glow
        let body_color: Color = if settings.body_glow {
            let c = f.kind().color();
            Color::srgb(c[0], c[1], c[2])
        } else {
            Color::srgb(BODY_GREY, BODY_GREY, BODY_GREY)
        };
        let body_material = solid(&mut standard, body_color, FIXTURE_GLOW);

        // A real GDTF profile, when the library has one for this
        // manufacturer/model, replaces the QLC+ category mesh entirely:
        // real nested geometry with the manufacturer's own dimensions,
        // and joints the file itself identifies rather than a Z-split
        // guessed from a placeholder mesh's vertex histogram.
        let gdtf = gdtf_library
            .0
            .as_ref()
            .and_then(|lib| lib.find(manufacturer, model));

        // Rig a fixture to the surface it hangs from, rather than
        // leaving it floating at an absolute height: move the ceiling and
        // the whole overhead rig follows it.
        //
        // Norco's data needs this to be more than bookkeeping. Its 47
        // truss pars are patched at z = 3.25m and its ceiling plane sits
        // at 2.743m (exactly 9 feet), so the rig is half a metre *above*
        // the room's own roof and is hidden behind it from every view
        // inside the room. Rather than silently rewriting either number,
        // an overhead fixture is hung from the ceiling: it keeps its real
        // x/y, and its height becomes an offset from the ceiling instead
        // of an absolute — clamped so it hangs below the surface rather
        // than through it.
        let (parent, local_pos, local_rot) =
            match nearest_rig_surface(f.position.to_vec3(), &rig_surfaces) {
                Some(surface) => {
                    let (local_pos, local_rot) =
                        rig_to_surface(f.position.to_vec3(), f.orientation(), &surface);
                    (Some(surface.entity), local_pos, local_rot)
                }
                None => (None, f.position.to_vec3(), f.orientation()),
            };

        let mut root_cmd = commands.spawn((
            Fixture {
                index,
                base_rot: local_rot,
            },
            FixtureBody::new(body_material.clone()),
            Transform {
                // The QLC+ anchor correction is a body offset applied to
                // the mesh child below — putting it here as well is what
                // sank the floor movers halfway through the floor, by
                // applying it twice.
                translation: local_pos,
                rotation: local_rot,
                scale: Vec3::ONE,
            },
            Visibility::default(),
            Name::new(f.name.clone()),
        ));
        if let Some(anchor) = parent {
            root_cmd.insert(ChildOf(anchor));
        }
        let root = root_cmd.id();

        // Where the beam comes from — one system: an emitter is a node
        // of the fixture's transform tree and nothing else positions it.
        // With a GDTF profile the emitter *is* the file's `<Beam>` node
        // entity (a bar's strip hangs under the node its cells hang
        // under), so the mount, every joint the file declares and any
        // display scale carry it with no placement arithmetic here.
        // Only a fixture with no profile at all takes the QLC+
        // placeholder path below, whose emitter is the guessed head.
        // r[impl viz.one-emitter-tree] - the emitter is a node of the tree
        let mut emitters: Vec<Entity> = Vec::new();
        let optics = fixture_optics(f, gdtf);

        if let Some(gdtf) = gdtf {
            let mut nodes: Vec<Entity> = Vec::new();
            gdtf_geometry::spawn_gdtf_tree(
                &mut commands,
                root,
                &gdtf.root,
                &mut shared_meshes,
                &body_material,
                Some(&asset_server),
                &mut emitters,
                &mut nodes,
            );
            // A row of cells is a strip, not a row of spots: the cell
            // nodes stay in the tree (they carry the file's placement)
            // but stop being emitters, and one linear emitter spanning
            // them takes over — under the cells' own parent, placed
            // from the cells' local poses.
            // r[impl viz.bar-emitters] - a bar's cells fold into one strip
            if let Some((parent_index, cells)) = gdtf.root.bar_cells()
                && let Some((centre, rot, half_length, pitch)) = bar_strip(&cells)
            {
                emitters.clear();
                spawn_bar_emitter(
                    &mut commands,
                    nodes[parent_index],
                    index,
                    &f.name,
                    centre,
                    rot,
                    half_length,
                    pitch,
                    &cells,
                    shared_meshes.assets,
                    &mut standard,
                    optics,
                    throw.spill_range(),
                );
            }
        } else {
            // No profile: the QLC+ category placeholder, with its
            // guessed yoke split and mesh correction. Kept only for
            // fixtures the library cannot resolve; a shipped venue
            // resolves every fixture (see `viz.gdtf-aliases`).
            let visual = resolve_fixture(
                f.position.to_vec3(),
                f.orientation(),
                None,
                manufacturer,
                model,
                None,
                &throw,
            );
            match &visual.body {
                BodyVisual::Mesh {
                    asset,
                    scale,
                    pre_rotate,
                    split_z,
                } => {
                    // With a split, the yoke is drawn on the root and the
                    // head becomes a child that tilts about the split
                    // point. With none, the whole mesh is the body.
                    let (yoke_split, head_split) = match split_z {
                        Some(z) => (Some((*z, false)), Some((*z, true))),
                        None => (None, None),
                    };
                    // The QLC+ path's anchor correction is a body offset,
                    // not a mount move, so it goes on the drawn mesh
                    // rather than the fixture root — the root stays at
                    // the real patched position for both paths.
                    //
                    // It is computed in *world* space ("shift the body up
                    // until its base lands on the mount point") but is
                    // being applied as a child's local translation, so it
                    // has to be brought into the root's frame first.
                    // Without this it is rotated by the fixture's own
                    // mount: the floor movers are patched with a 180
                    // degree flip, which turned "up" into "down" and sank
                    // them through the stage by twice the correction.
                    let anchor =
                        f.orientation().inverse() * (visual.position - f.position.to_vec3());
                    commands.spawn((
                        Mesh3d(shared_meshes.get_or_add(
                            (std::ptr::from_ref(asset) as usize, split_key(yoke_split)),
                            || asset.to_bevy_mesh(yoke_split),
                        )),
                        MeshMaterial3d(body_material.clone()),
                        Transform {
                            translation: anchor,
                            rotation: *pre_rotate,
                            scale: Vec3::splat(*scale),
                        },
                        ChildOf(root),
                    ));
                    match head_split {
                        Some(_) => {
                            // The joint carries position and rotation and
                            // nothing else. The mesh's scale goes on a
                            // child, because the beam hangs off the joint
                            // too and would otherwise inherit the shrink
                            // that fits a 1-unit mesh into a 24cm fixture
                            // — a metres-long beam rendered a few
                            // centimetres long.
                            // The joint carries the *real* pan/tilt
                            // convention and nothing else, because the
                            // beam hangs off it: `mount * pan * tilt *
                            // -Z`, which is what `focus.rs` solves for
                            // and what a DMX reading means.
                            //
                            // `pre_rotate` is a correction for how the
                            // QLC+ placeholder mesh was authored, not a
                            // statement about where the fixture points,
                            // so it belongs on the mesh alone. It was on
                            // the joint, which put a 180-degree flip
                            // between the aim that was asked for and the
                            // beam that came out — every focus solve
                            // rendered mirrored.
                            let pivot = anchor + visual.head_pivot.unwrap_or(Vec3::ZERO);
                            let head = commands
                                .spawn((
                                    FixtureHead,
                                    Transform::from_translation(pivot),
                                    Visibility::default(),
                                    ChildOf(root),
                                ))
                                .id();
                            commands.spawn((
                                Mesh3d(shared_meshes.get_or_add(
                                    (std::ptr::from_ref(asset) as usize, split_key(head_split)),
                                    || asset.to_bevy_mesh(head_split),
                                )),
                                MeshMaterial3d(body_material.clone()),
                                Transform {
                                    rotation: *pre_rotate,
                                    scale: Vec3::splat(*scale),
                                    ..default()
                                },
                                ChildOf(head),
                            ));
                            emitters.push(head);
                        }
                        None => emitters.push(root),
                    }
                }
                BodyVisual::Bar {
                    length,
                    width,
                    height,
                } => {
                    commands.spawn((
                        Mesh3d(unit_cube.clone()),
                        MeshMaterial3d(body_material.clone()),
                        Transform::from_scale(Vec3::new(*length, *width, *height)),
                        ChildOf(root),
                    ));
                    emitters.push(root);
                }
                BodyVisual::Generic => {
                    commands.spawn((
                        Mesh3d(unit_cube.clone()),
                        MeshMaterial3d(body_material.clone()),
                        Transform::from_scale(Vec3::splat(0.15)),
                        ChildOf(root),
                    ));
                }
            }
        }

        // Beam and spill exist from the start and are simply hidden while
        // the fixture is dark — cheaper and steadier than spawning and
        // despawning entities as cues fade in and out. Both are children
        // of the emitter with an identity local transform: their world
        // pose is the emitter's, by construction.
        for emitter in emitters {
            commands.entity(emitter).insert((
                BeamEmitter { fixture: index },
                EmitterState::default(),
                optics,
            ));
            // A hazer is a fixture with no light of its own: it drives
            // the haze the others show in (see `haze::HazeLevel`), and
            // gets no spill and no place in the fog.
            if is_hazer(model) {
                continue;
            }
            let mut spill = commands.spawn((
                FixtureSpill,
                SpotLight {
                    intensity: 0.0,
                    range: throw.spill_range(),
                    // The lens, not a point — see `lens_radius`.
                    radius: lens_radius(),
                    // A shadow map is a render pass per light per
                    // frame and a texture the fog samples at every
                    // step — the budgeted part of the frame. The
                    // fog pass itself skips the shadow fetch for a
                    // light without one, so a par still cuts its cone
                    // in the haze; it just cuts nobody's silhouette
                    // out of it. See `budget.rs`.
                    // `IGNITION_SHADOWS=0` takes every one away, which
                    // is how you find out whether what you are looking
                    // at in a beam is the shadow map rather than the
                    // march.
                    shadow_maps_enabled: shadowed[index]
                        && !std::env::var("IGNITION_SHADOWS").is_ok_and(|v| v.trim() == "0"),
                    ..default()
                },
                Transform::default(),
                Visibility::Hidden,
                Name::new(format!("{} spill", f.name)),
                ChildOf(emitter),
            ));
            // Every spill is in the fog. What it shows there is decided
            // by the haze in the room, not by a per-fixture flag: with
            // the hazers off nothing shows in the air at all, and as the
            // haze builds every fixture's light appears in it, pars and
            // movers alike. On the haze camera's layer too, so the fog
            // it marches has this light in its clusters.
            // r[impl viz.haze-is-volumetric] - every emitter lights the fog, nothing else lights the air
            spill.insert((
                VolumetricLight,
                bevy::camera::visibility::RenderLayers::from_layers(&[0, crate::haze::HAZE_LAYER]),
            ));
        }
    }
}

/// Whether a fixture pans or tilts: its GDTF names a pan or tilt
/// geometry, or its channel map carries the attribute.
pub fn fixture_moves(manufacturer: &str, model: &str, profile: Option<&GdtfFixture>) -> bool {
    fn any(node: &crate::gdtf_geometry::GdtfNode) -> bool {
        node.is_pan || node.is_tilt || node.children.iter().any(any)
    }
    profile.is_some_and(|p| any(&p.root))
        || crate::channel_map::channel_map_for(manufacturer, model).is_some_and(|map| {
            map.offset_of(&ignition_proto::Attribute::Pan).is_some()
                || map.offset_of(&ignition_proto::Attribute::Tilt).is_some()
        })
}

/// Resolves every patched fixture against the current DMX state and
/// writes what changed onto the entities: the body's pan, the head's or
/// the GDTF joint's tilt, and each emitter's colour.
///
/// Deliberately does *not* touch the beam or the spill. Where a beam
/// starts and which way it points is a consequence of the joints this
/// system just moved, and that is not known until Bevy has propagated
/// transforms — `update_beams` runs after that and reads the answer
/// rather than recomputing it.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub fn update_live_fixtures(
    venue: Res<VenueRes>,
    dmx: Option<Res<DmxRes>>,
    mut fixtures: Query<(Entity, &Fixture, &mut Transform), Without<FixtureHead>>,
    mut heads: Query<(&ChildOf, &mut Transform), (With<FixtureHead>, Without<Fixture>)>,
    mut pan_joints: Query<
        (&ChildOf, &mut Transform),
        (
            With<PanJoint>,
            Without<Fixture>,
            Without<FixtureHead>,
            Without<TiltJoint>,
        ),
    >,
    mut tilt_joints: Query<
        (&ChildOf, &mut Transform),
        (
            With<TiltJoint>,
            Without<Fixture>,
            Without<FixtureHead>,
            Without<PanJoint>,
        ),
    >,
    mut emitters: Query<(&BeamEmitter, &mut EmitterState, &ProfileOptics)>,
    child_of: Query<&ChildOf>,
    live_dmx: Res<LiveDmx>,
    time: Res<Time>,
    mut haze_level: ResMut<crate::haze::HazeLevel>,
) {
    let Some(_dmx) = dmx else { return };
    let venue = &venue.0;
    let elapsed = time.elapsed_secs();

    // The hazers' output this frame, for the fog to settle toward.
    // r[impl viz.haze-is-volumetric] - the hazers drive the haze
    let hazers: Vec<f32> = venue
        .fixtures
        .iter()
        .enumerate()
        .filter(|(_, f)| f.patched && is_hazer(f.model.as_deref().unwrap_or("")))
        .map(|(i, _)| match live_dmx.0.get(i) {
            Some(Some(live)) => live.dimmer,
            _ => 0.0,
        })
        .collect();
    haze_level.settle(crate::haze::hazer_target(&hazers), time.delta_secs());

    // Resolve once, so every entity family below agrees on the same
    // frame's DMX rather than each re-reading it.
    let mut resolved: Vec<Option<Live>> = (0..venue.fixtures.len()).map(|_| None).collect();
    for (index, f) in venue.fixtures.iter().enumerate() {
        let Some(Some(live)) = live_dmx.0.get(index) else {
            continue;
        };
        let manufacturer = f.manufacturer.as_deref().unwrap_or("");
        let model = f.model.as_deref().unwrap_or("");

        // A fixture with no colour channel at all (a hazer's plain Dimmer
        // map) still emits — as white, since a hazer genuinely does haze
        // up whatever light is already in the air, and no beam at all
        // would read as broken.
        // The strobe byte gates the frame like a shutter would: a rate
        // from the wire, a square wave on the clock. Decoded, not read
        // from the engine, so a strobe a cue asks for and a strobe the
        // patch actually carries are the same thing on screen.
        // r[impl viz.driven-by-dmx] - strobe and zoom come off the bytes too
        let shutter_open = strobe_open(live.strobe, elapsed);
        let color = (live.dimmer > MIN_VISIBLE_DIMMER && shutter_open).then(|| {
            if live.has_color {
                [
                    live.color[0] * live.dimmer,
                    live.color[1] * live.dimmer,
                    live.color[2] * live.dimmer,
                ]
            } else {
                [live.dimmer; 3]
            }
        });

        resolved[index] = Some(Live {
            pan: Quat::from_axis_angle(Vec3::Z, live.pan_deg.to_radians()),
            tilt: Quat::from_axis_angle(Vec3::X, live.tilt_deg.to_radians()),
            color,
            zoom: live.zoom,
            lumens: power_watts(manufacturer, model) * LUMENS_PER_WATT,
        });
    }

    // Which venue record each fixture root came from, so the joint passes
    // can resolve their own fixture by walking up to it.
    let mut index_of_root: std::collections::HashMap<Entity, usize> =
        std::collections::HashMap::new();
    for (entity, fixture, mut transform) in &mut fixtures {
        index_of_root.insert(entity, fixture.index);
        let Some(Some(live)) = resolved.get(fixture.index) else {
            continue;
        };
        // Pan turns the whole fixture on its mount, which is what a real
        // yoke does. A GDTF file that declares its own pan joint gets
        // that applied there instead, below. `base_rot`, not the venue
        // record's own orientation — a rigged fixture's rotation is
        // relative to the surface it hangs from.
        transform.rotation = fixture.base_rot * live.pan;
    }

    // Root of the fixture this entity belongs to, however deep it sits —
    // a GDTF joint can be several nodes down.
    let fixture_index_of = |entity: Entity| -> Option<usize> {
        let mut current = entity;
        for _ in 0..16 {
            if let Some(index) = index_of_root.get(&current) {
                return Some(*index);
            }
            current = child_of.get(current).ok()?.parent();
        }
        None
    };

    for (parent, mut transform) in &mut heads {
        let Some(live) = fixture_index_of(parent.parent())
            .and_then(|i| resolved.get(i))
            .and_then(|o| o.as_ref())
        else {
            continue;
        };
        // Tilt alone. See the joint's own comment in `spawn_venue`.
        transform.rotation = live.tilt;
    }

    // GDTF joints: the file itself said which node each attribute drives,
    // so a live reading rotates the manufacturer's own axis. Everything
    // below the joint follows because Bevy propagates transforms.
    for (parent, mut transform) in &mut pan_joints {
        let Some(live) = fixture_index_of(parent.parent())
            .and_then(|i| resolved.get(i))
            .and_then(|o| o.as_ref())
        else {
            continue;
        };
        transform.rotation = live.pan;
    }
    for (parent, mut transform) in &mut tilt_joints {
        let Some(live) = fixture_index_of(parent.parent())
            .and_then(|i| resolved.get(i))
            .and_then(|o| o.as_ref())
        else {
            continue;
        };
        transform.rotation = live.tilt;
    }

    for (emitter, mut state, optics) in &mut emitters {
        match resolved.get(emitter.fixture).and_then(|o| o.as_ref()) {
            Some(live) => {
                state.color = live.color;
                // The emitter's own optics, zoomed by the wire.
                state.half_angle_deg = zoomed_half_angle_deg(optics.beam_half_deg, live.zoom);
                state.field_half_angle_deg =
                    zoomed_half_angle_deg(optics.field_half_deg, live.zoom)
                        .clamp(state.half_angle_deg, MAX_FIELD_HALF_ANGLE_DEG);
                state.lumens = live.lumens;
            }
            None => state.color = None,
        }
    }
}

struct Live {
    pan: Quat,
    tilt: Quat,
    color: Option<[f32; 3]>,
    zoom: Option<f32>,
    lumens: f32,
}

/// Whether a strobing fixture is lit this frame. `None` (no strobe
/// channel) and zero (shutter open) are always lit; above that the
/// byte sets a rate from 1 to 25 Hz, and the shutter is open for half
/// of each period.
pub fn strobe_open(strobe: Option<f32>, elapsed_secs: f32) -> bool {
    match strobe {
        Some(rate) if rate > 0.001 => {
            let hz = 1.0 + rate * 24.0;
            (elapsed_secs * hz).fract() < 0.5
        }
        _ => true,
    }
}

/// The cone a zoom byte draws: the fixture's nominal angle when the
/// personality has no zoom, otherwise half of it at byte 0 through
/// one and a half times it at byte 255. A stand-in for the optic's
/// real range until profiles carry one, but it moves with the wire.
pub fn zoomed_half_angle_deg(nominal_half_deg: f32, zoom: Option<f32>) -> f32 {
    match zoom {
        Some(z) => nominal_half_deg * (0.5 + z.clamp(0.0, 1.0)),
        None => nominal_half_deg,
    }
}

/// What to hand a Bevy `SpotLight` so it throws the fixture's real
/// peak intensity down its axis.
///
/// Bevy divides a spot light's `intensity` by 4π steradians whatever
/// its cone — it does *not* concentrate the lumens into the cone. So
/// fed raw lumens a 1.7-degree beam fixture and a 60-degree par of the
/// same wattage were equally bright per direction, and the exposure
/// had been wound up to 3000 to get the beams to read, which blew every
/// par out to white. Instead the fixture's lumens are spread over its
/// own field cone to get its candela, and the light is given the lumens
/// an omnidirectional source of that candela would have.
///
/// That is the whole of it: real photometry in, nothing scaled, nothing
/// capped. How bright it all comes out is the camera's business —
/// `app::stage_exposure` sets the stage EV100, `--exposure` is stops on
/// it, and the auto exposure rides the frame from there — the same way
/// a 150 W beam and a 36 W par share a venue and a phone camera copes.
// r[impl viz.exposure] - real candela in, the camera decides the rest
pub fn spot_lumens(lumens: f32, field_half_angle_rad: f32) -> f32 {
    peak_candela(lumens, field_half_angle_rad.to_degrees()) * 4.0 * core::f32::consts::PI
}

/// Aims and colours every spill from where its emitter actually ended
/// up. The spill is the fixture's whole light: what lands on the room,
/// and — through the fog volume — what shows in the air.
///
/// Runs in `PostUpdate` after transform propagation, which is the whole
/// point: the emitter's `GlobalTransform` already accounts for the mount
/// pose, the pan, the tilt and — for a GDTF fixture — every joint and
/// offset in the manufacturer's own geometry tree. Nothing here knows or
/// cares which of those applied.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub fn update_beams(
    time: Res<Time>,
    venue: Res<VenueRes>,
    mut standard: ResMut<Assets<StandardMaterial>>,
    emitters: Query<(
        &EmitterState,
        &GlobalTransform,
        Option<&Children>,
        Option<&BarEmitter>,
    )>,
    mut spill_q: Query<(&mut Visibility, &mut SpotLight), With<FixtureSpill>>,
) {
    let throw = BeamThrow::for_venue(&venue.0);
    let _ = time.elapsed_secs();

    for (state, global, children, bar) in &emitters {
        let Some(children) = children else { continue };
        let origin = global.translation();
        // This project's aim convention: a fixture emits along its own
        // local -Z, which is also the axis a Bevy spot light shines down.
        let direction = (global.rotation() * Vec3::NEG_Z).normalize_or_zero();
        let length = throw.reach(origin, direction);

        // A strip's cells show its colour on the housing, lit or dark.
        // r[impl viz.bar-emitters] - every cell face carries the colour
        if let Some(bar) = bar {
            let emissive = match state.color {
                Some(c) => LinearRgba::rgb(
                    c[0] * BAR_FACE_GLOW,
                    c[1] * BAR_FACE_GLOW,
                    c[2] * BAR_FACE_GLOW,
                ),
                None => LinearRgba::BLACK,
            };
            for cell in &bar.cells {
                let unchanged = standard.get(cell).is_some_and(|m| m.emissive == emissive);
                if unchanged {
                    continue;
                }
                if let Some(mut m) = standard.get_mut(cell) {
                    m.emissive = emissive;
                }
            }
        }

        for child in children.iter() {
            if let Ok((mut visibility, mut light)) = spill_q.get_mut(child) {
                match state.color {
                    Some(color) => {
                        *visibility = Visibility::Visible;
                        // A strip's one spill has to cover the strip's
                        // own length as well as its spread — a cone is
                        // the only shape a spot light has, so it opens to
                        // where the wedge's long side reaches.
                        // The spill's cone is the datasheet's: full to
                        // the beam angle (the 50% edge), falling off to
                        // nothing at the field angle (the 10% edge). A
                        // par's field is twice its beam, so it is a
                        // soft pool; a beam fixture's field is barely
                        // wider, so it is a hard shaft. That difference
                        // — not a per-category penumbra guess — is what
                        // separates the two.
                        // r[impl viz.profile-optics] - beam is the inner cone, field the outer
                        let (inner, outer) = match bar {
                            // A strip's one spill has to cover the
                            // strip's own length as well as its spread
                            // — a cone is the only shape a spot light
                            // has, so it opens to where the wedge's
                            // long side reaches. It keeps most of its
                            // cone at full: its wash has to start at
                            // the housing and run up the wall it stands
                            // against, and a fully soft edge left the
                            // wall dark until halfway up.
                            // r[impl viz.bar-emitters] - a bar's wash begins at the bar
                            Some(bar) => {
                                let outer = (state
                                    .field_half_angle_deg
                                    .max(BAR_MIN_HALF_ANGLE_DEG)
                                    .to_radians()
                                    + (bar.half_length / length.max(0.1)).atan())
                                .min(core::f32::consts::FRAC_PI_2 - 0.01);
                                (outer * BAR_INNER_FRACTION, outer)
                            }
                            None => {
                                let outer = state
                                    .field_half_angle_deg
                                    .to_radians()
                                    .min(core::f32::consts::FRAC_PI_2 - 0.01);
                                (state.half_angle_deg.to_radians().min(outer), outer)
                            }
                        };
                        light.outer_angle = outer;
                        light.inner_angle = inner;
                        light.range = throw.spill_range();
                        light.color = Color::srgb(color[0], color[1], color[2]);
                        light.intensity = spot_lumens(state.lumens, outer);
                    }
                    None => {
                        *visibility = Visibility::Hidden;
                        light.intensity = 0.0;
                    }
                }
            }
        }
    }
}

/// Lights each fixture's own housing with the colour it is emitting, so
/// the rig reads as a rig at a glance — which fixtures are up, in what
/// colour — rather than as a static model.
pub fn update_fixture_bodies(
    dmx: Option<Res<DmxRes>>,
    settings: Res<VizSettings>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    bodies: Query<(&Fixture, &FixtureBody)>,
    live_dmx: Res<LiveDmx>,
) {
    let Some(_dmx) = dmx else { return };

    for (fixture, body) in &bodies {
        let live = live_dmx.0.get(fixture.index).copied().flatten();
        let emissive = body_emissive(live, settings.body_glow);

        // `get_mut` marks the material modified whether or not anything
        // changed, and a modified material is re-uploaded — 69 bodies
        // re-uploaded every frame for a rig sitting still. Look first.
        let unchanged = materials
            .get(&body.material)
            .is_some_and(|m| m.emissive == emissive);
        if !unchanged && let Some(mut material) = materials.get_mut(&body.material) {
            material.emissive = emissive;
        }
        // A GLB part glows on top of what its file gave it.
        for part in &body.parts {
            let target = part.base_emissive + emissive;
            let unchanged = materials
                .get(&part.material)
                .is_some_and(|m| m.emissive == target);
            if unchanged {
                continue;
            }
            if let Some(mut material) = materials.get_mut(&part.material) {
                material.emissive = target;
            }
        }
    }
}

/// What a fixture's housing emits this frame. With the glow off — the
/// default — nothing, whatever the fixture is doing: the housing is a
/// black box lit only by the rig around it. With it on, a lit fixture
/// glows its own colour, scaled by the dimmer so a fixture at 20% reads
/// as at 20%, and hot enough at full for bloom to halo it.
// r[impl viz.body-glow] - a body emits nothing unless the glow is on
pub fn body_emissive(live: Option<crate::dmx::ResolvedAttributes>, body_glow: bool) -> LinearRgba {
    if !body_glow {
        return LinearRgba::BLACK;
    }
    match live {
        Some(live) if live.dimmer > MIN_VISIBLE_DIMMER => {
            let c = if live.has_color { live.color } else { [1.0; 3] };
            let gain = live.dimmer * LIT_BODY_GLOW;
            LinearRgba::rgb(c[0] * gain, c[1] * gain, c[2] * gain)
        }
        // Dark: emits nothing, and is lit only by whatever else in the
        // rig happens to fall on it.
        _ => LinearRgba::BLACK,
    }
}

/// This frame's DMX, decoded once for every patched fixture.
///
/// `update_live_fixtures` and `update_fixture_bodies` both need it, and
/// each used to resolve the whole rig for itself — twice the byte
/// reads, twice the lock traffic, for the same answer.
#[derive(Resource, Default)]
pub struct LiveDmx(pub Vec<Option<crate::dmx::ResolvedAttributes>>);

/// Decodes the rig's bytes for the frame. Runs ahead of everything that
/// reads `LiveDmx`.
pub fn resolve_live_dmx(venue: Res<VenueRes>, dmx: Option<Res<DmxRes>>, mut out: ResMut<LiveDmx>) {
    let Some(dmx) = dmx else { return };
    let venue = &venue.0;
    let patch = venue.patch();
    out.0.clear();
    out.0
        .extend(venue.fixtures.iter().enumerate().map(|(index, f)| {
            if !f.patched {
                return None;
            }
            let entry = patch.get(index)?;
            Some(dmx.0.resolve(&entry.address, &entry.map))
        }));
}

/// Applies the ambient dial to Bevy's global ambient light.
// r[impl viz.exposure] - the fill is photometric too
pub fn apply_ambient(settings: Res<VizSettings>, mut ambient: ResMut<GlobalAmbientLight>) {
    if settings.is_changed() {
        ambient.brightness = settings.ambient * AMBIENT_LUX_PER_UNIT;
    }
}

/// What one unit of the ambient dial is worth, in lux on every surface.
/// A dial of one is a dim room's worklight; the shipped 0.15 is a
/// couple of lux, just enough to find the truss by. Calibrated against
/// `app::STAGE_EV100` the same as the fixtures are: this was 200 under
/// Bevy's Blender exposure, and the stage camera is 3.6 stops more
/// open than that.
pub const AMBIENT_LUX_PER_UNIT: f32 = 200.0 / 12.0;

#[cfg(test)]
mod rigging_tests {
    use super::*;

    fn surface(name_is_ceiling: bool, center: Vec3, half: Vec3) -> RigSurface {
        RigSurface {
            entity: Entity::from_raw_u32(1).unwrap(),
            center,
            rot: Quat::IDENTITY,
            half_extents: half,
            is_ceiling: name_is_ceiling,
        }
    }

    /// Norco's real structure, roughly: a ceiling plane at 9 ft, the deck
    /// at zero, the back wall upstage, and the short truss beam the
    /// overhead movers hang from.
    fn venue() -> Vec<RigSurface> {
        vec![
            surface(
                true,
                Vec3::new(0.0, -6.5, 2.743),
                Vec3::new(5.95, 9.45, 0.01),
            ),
            surface(
                false,
                Vec3::new(0.0, -1.9, 0.0),
                Vec3::new(5.95, 1.52, 0.01),
            ),
            surface(
                false,
                Vec3::new(0.0, 2.95, 1.45),
                Vec3::new(4.57, 0.01, 1.30),
            ),
            surface(
                false,
                Vec3::new(0.0, 2.34, 2.36),
                Vec3::new(1.63, 0.08, 0.08),
            ),
        ]
    }

    fn which(pos: Vec3) -> usize {
        let all = venue();
        let picked = nearest_rig_surface(pos, &all).expect("something should be in range");
        all.iter().position(|s| s.center == picked.center).unwrap()
    }

    #[test]
    fn each_fixture_rigs_to_what_it_is_actually_mounted_to() {
        // A truss par under the ceiling.
        assert_eq!(which(Vec3::new(2.0, -2.0, 2.70)), 0, "truss par -> ceiling");
        // A strip lying on the deck.
        assert_eq!(which(Vec3::new(2.0, -1.9, 0.05)), 1, "strip -> floor");
        // A par mounted on the back wall.
        assert_eq!(
            which(Vec3::new(3.0, 2.90, 2.30)),
            2,
            "back wall par -> wall"
        );
        // An overhead mover on the beam over the drums.
        assert_eq!(which(Vec3::new(1.0, 2.32, 2.30)), 3, "OH mover -> beam");
    }

    #[test]
    fn a_fixture_out_in_open_air_rigs_to_nothing() {
        assert!(nearest_rig_surface(Vec3::new(0.0, -8.0, 1.5), &venue()).is_none());
    }

    #[test]
    fn a_fixture_above_the_ceiling_is_pulled_down_to_hang_from_it() {
        // Norco patches its truss pars half a metre above its own ceiling.
        let ceiling = venue()[0];
        let (local, _) = rig_to_surface(Vec3::new(1.0, -2.0, 3.25), Quat::IDENTITY, &ceiling);
        assert_eq!((local.x, local.y), (1.0, 4.5));
        assert!((local.z + RIG_DROP).abs() < 1e-6, "{}", local.z);
    }

    #[test]
    fn every_other_surface_keeps_the_fixtures_real_offset() {
        // The clamp is a fix for one contradiction in the ceiling data,
        // not a general rule — a wall-mounted par must stay where it was
        // measured.
        let wall = venue()[2];
        let (local, _) = rig_to_surface(Vec3::new(3.0, 2.90, 2.30), Quat::IDENTITY, &wall);
        assert!((local.z - (2.30 - 1.45)).abs() < 1e-6, "{}", local.z);
    }

    #[test]
    fn the_local_pose_reproduces_the_world_pose_when_the_surface_is_rotated() {
        let s = RigSurface {
            entity: Entity::from_raw_u32(1).unwrap(),
            center: Vec3::new(0.0, 2.95, 1.45),
            rot: Quat::from_rotation_z(0.7),
            half_extents: Vec3::new(4.0, 0.05, 1.3),
            is_ceiling: false,
        };
        let pos = Vec3::new(1.0, 2.0, 2.0);
        let rot = Quat::from_rotation_x(0.3);
        let (local_pos, local_rot) = rig_to_surface(pos, rot, &s);
        let world = s.center + s.rot * local_pos;
        assert!(world.distance(pos) < 1e-5, "{world:?}");
        assert!((s.rot * local_rot).angle_between(rot) < 1e-5);
    }
}

#[cfg(test)]
mod body_and_bar_tests {
    use super::*;
    use crate::dmx::ResolvedAttributes;

    /// r[verify viz.body-glow] - the default material emits nothing, lit or not
    #[test]
    fn a_fixture_body_emits_nothing_unless_the_glow_is_on() {
        let mut materials: Assets<StandardMaterial> = Assets::default();
        let handle = solid(&mut materials, Color::srgb(0.25, 0.75, 0.95), FIXTURE_GLOW);
        let material = materials.get(&handle).unwrap();
        assert_eq!(
            material.emissive,
            LinearRgba::BLACK,
            "the spawned body is matte"
        );

        let lit = ResolvedAttributes {
            dimmer: 1.0,
            color: [1.0, 0.5, 0.1],
            has_color: true,
            ..Default::default()
        };
        assert_eq!(
            body_emissive(Some(lit), false),
            LinearRgba::BLACK,
            "off: black at full"
        );
        assert_eq!(body_emissive(None, false), LinearRgba::BLACK);
        let on = body_emissive(Some(lit), true);
        assert!(
            on.red > 0.0 && on.red > on.blue,
            "on: the fixture's own colour"
        );
        assert_eq!(
            body_emissive(None, true),
            LinearRgba::BLACK,
            "on but dark: nothing"
        );
    }

    fn record(beam_angle_deg: Option<f32>) -> crate::venue::FixtureRecord {
        serde_json::from_value(serde_json::json!({
            "chan": 1, "name": "p", "tags": [], "patched": true,
            "manufacturer": "uking", "model": "par",
            "position": {"x": 0.0, "y": 0.0, "z": 0.0},
            "eulers": {"x": 0.0, "y": 0.0, "z": 0.0},
            "quat": {"w": 1.0, "x": 0.0, "y": 0.0, "z": 0.0},
            "size": {"x": 0.2, "y": 0.2, "z": 0.2},
            "beam_angle_deg": beam_angle_deg,
        }))
        .expect("record")
    }

    fn profile(beam: Option<f32>, field: Option<f32>) -> GdtfFixture {
        GdtfFixture {
            fixture_type_name: "x".into(),
            dmx_mode_name: "x".into(),
            root: gdtf_geometry::GdtfNode {
                name: "Beam".into(),
                shape: gdtf_geometry::GdtfShape::None,
                local_pos: Vec3::ZERO,
                local_rot: Quat::IDENTITY,
                is_pan: false,
                is_tilt: false,
                is_beam: true,
                children: vec![],
            },
            beam_angle_deg: beam,
            field_angle_deg: field,
        }
    }

    /// The profile wins whenever one resolves; the patch's angle is only
    /// the fallback for a fixture with no profile.
    // r[verify viz.profile-optics] - profile first, patch only as the fallback
    #[test]
    fn the_profiles_angles_win_over_the_patch() {
        let patch_30 = record(Some(30.0));
        assert_eq!(
            fixture_optics(&patch_30, Some(&profile(Some(20.0), Some(34.0)))),
            ProfileOptics {
                beam_half_deg: 10.0,
                field_half_deg: 17.0
            },
            "the datasheet's 20/34, not the patch's 30"
        );
        assert_eq!(
            fixture_optics(&patch_30, Some(&profile(Some(20.0), None))),
            ProfileOptics {
                beam_half_deg: 10.0,
                field_half_deg: 20.0
            },
            "no field in the file: twice the beam"
        );
        assert_eq!(
            fixture_optics(&patch_30, Some(&profile(None, None))),
            ProfileOptics {
                beam_half_deg: 15.0,
                field_half_deg: 30.0
            },
            "a profile with no angle at all defers to the patch"
        );
        assert_eq!(
            fixture_optics(&patch_30, None),
            ProfileOptics {
                beam_half_deg: 15.0,
                field_half_deg: 30.0
            },
            "no profile: the patch's 30 as the beam, field assumed"
        );
        let unknown = fixture_optics(&record(Some(0.0)), None);
        assert_eq!(
            unknown.beam_half_deg, 12.5,
            "nothing anywhere: the old default"
        );
    }

    /// Per-direction brightness, not raw lumens, is what the spot light
    /// is fed — so a beam fixture out-shines a par by its optics, and
    /// nothing scales or caps it: the numbers are the datasheet's.
    // r[verify viz.exposure] - real candela in, uncapped
    #[test]
    fn a_beam_fixture_is_brighter_per_direction_than_a_par_of_more_watts() {
        let par = spot_lumens(36.0 * LUMENS_PER_WATT, 30f32.to_radians());
        let beam = spot_lumens(150.0 * LUMENS_PER_WATT, 1.29f32.to_radians());
        assert!(beam > par * 20.0, "beam {beam} vs par {par}");
        // No cap: doubling the beam's wattage doubles it.
        let beam2 = spot_lumens(300.0 * LUMENS_PER_WATT, 1.29f32.to_radians());
        assert!((beam2 / beam - 2.0).abs() < 1e-3);
        // Real photometry: the par is ~1,600 cd.
        let par_cd = par / (4.0 * core::f32::consts::PI);
        assert!((1500.0..1800.0).contains(&par_cd), "{par_cd}");
    }

    /// The strip is spawned under the node its cells hang from, and its
    /// world pose is that node's pose times the strip's centre — the
    /// hierarchy places it, the same as a point emitter.
    // r[verify viz.one-emitter-tree] - a bar's strip hangs where its cells hang
    #[test]
    fn a_bars_strip_hangs_under_the_cells_parent_and_lands_on_the_cells() {
        use bevy::MinimalPlugins;
        use bevy::app::App;
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(
            "../../data/gdtf/American_DJ@Ultra_Bar_12@Close_-_Needs_Work_on_Strobes_and_Programs.gdtf",
        );
        if !path.exists() {
            return;
        }
        let fixture = gdtf_geometry::import_geometry(&path, None).unwrap();
        let (parent_index, cells) = fixture.root.bar_cells().expect("a bar");
        let (centre, rot, half_length, pitch) = bar_strip(&cells).expect("a strip");

        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(bevy::transform::TransformPlugin);
        let mount = Transform {
            translation: Vec3::new(2.0, -1.0, 0.0),
            rotation: Quat::from_rotation_z(0.6) * Quat::from_rotation_x(-0.4),
            scale: Vec3::ONE,
        };
        let material = Handle::<StandardMaterial>::default();
        let world = app.world_mut();
        let root = world.spawn((mount, Visibility::default())).id();
        let mut emitters = Vec::new();
        let mut nodes = Vec::new();
        let mut queue = bevy::ecs::world::CommandQueue::default();
        {
            let mut commands = Commands::new(&mut queue, world);
            let mut meshes = Assets::<Mesh>::default();
            let mut standard = Assets::<StandardMaterial>::default();
            gdtf_geometry::spawn_gdtf_tree(
                &mut commands,
                root,
                &fixture.root,
                &mut gdtf_geometry::SharedMeshes::new(&mut meshes),
                &material,
                None,
                &mut emitters,
                &mut nodes,
            );
            spawn_bar_emitter(
                &mut commands,
                nodes[parent_index],
                0,
                "bar",
                centre,
                rot,
                half_length,
                pitch,
                &cells,
                &mut meshes,
                &mut standard,
                ProfileOptics {
                    beam_half_deg: 20.0,
                    field_half_deg: 22.5,
                },
                30.0,
            );
        }
        queue.apply(world);
        app.update();
        let world = app.world_mut();
        let strip = world
            .query_filtered::<Entity, With<BarEmitter>>()
            .iter(world)
            .next()
            .expect("one strip");
        assert_eq!(
            world.entity(strip).get::<ChildOf>().unwrap().parent(),
            nodes[parent_index],
            "under the cells' parent, not the fixture root"
        );
        let parent_world = *world
            .entity(nodes[parent_index])
            .get::<GlobalTransform>()
            .unwrap();
        let expected = parent_world
            * Transform {
                translation: centre,
                rotation: rot,
                scale: Vec3::ONE,
            };
        let actual = world.entity(strip).get::<GlobalTransform>().unwrap();
        assert!(actual.translation().distance(expected.translation()) < 1e-5);
        // The strip's centre is the centre of the cells, in the world.
        let mut cell_centre = Vec3::ZERO;
        for e in &emitters {
            cell_centre += world
                .entity(*e)
                .get::<GlobalTransform>()
                .unwrap()
                .translation();
        }
        cell_centre /= emitters.len() as f32;
        // The strip is centred between the end cells; the file's cells
        // are a fraction of a millimetre off an even pitch.
        assert!(
            actual.translation().distance(cell_centre) < 2e-3,
            "strip at {:?}, cells centred at {cell_centre:?}",
            actual.translation()
        );
        // Every child of the strip (faces, wedge, spill) sits at the
        // strip's own pose: only the faces are offset, along the strip.
        let along = actual.rotation() * Vec3::X;
        let children: Vec<Entity> = world
            .entity(strip)
            .get::<Children>()
            .unwrap()
            .iter()
            .collect();
        for c in children {
            let d = world
                .entity(c)
                .get::<GlobalTransform>()
                .unwrap()
                .translation()
                - actual.translation();
            let off_axis = d - along * d.dot(along);
            assert!(off_axis.length() < 0.01, "{off_axis:?}");
        }
    }

    fn ultra_bar_beams() -> Option<Vec<(Vec3, Quat)>> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(
            "../../data/gdtf/American_DJ@Ultra_Bar_12@Close_-_Needs_Work_on_Strobes_and_Programs.gdtf",
        );
        if !path.exists() {
            return None;
        }
        Some(
            gdtf_geometry::import_geometry(&path, None)
                .unwrap()
                .root
                .beam_poses(),
        )
    }

    /// r[verify viz.bar-emitters] - the Ultra Bar's twelve cells are one strip
    #[test]
    fn twelve_cells_on_a_line_are_one_strip_the_width_of_the_bar() {
        let Some(beams) = ultra_bar_beams() else {
            return;
        };
        assert_eq!(beams.len(), 12);
        let (centre, rot, half_length, pitch) = bar_strip(&beams).expect("a bar");
        // Cells run from x = -0.486 to +0.486; a pitch either side makes
        // the strip the housing's 1.06 m.
        assert!((pitch - 0.0884).abs() < 0.002, "pitch {pitch}");
        assert!(
            (half_length - 0.530).abs() < 0.005,
            "half length {half_length}"
        );
        assert!(
            centre.abs().max_element() < 0.002,
            "centred on the bar: {centre:?}"
        );
        let along = rot * Vec3::X;
        let aim = rot * Vec3::NEG_Z;
        assert!(along.abs_diff_eq(Vec3::X, 1e-4) || along.abs_diff_eq(Vec3::NEG_X, 1e-4));
        assert!(
            aim.abs_diff_eq(Vec3::NEG_Z, 1e-4),
            "fires out of the face: {aim:?}"
        );
    }

    /// r[verify viz.bar-emitters] - a single lens, or lenses off a line, are not a bar
    #[test]
    fn a_par_or_a_scattered_multi_lens_fixture_is_not_a_bar() {
        let one = vec![(Vec3::new(0.0, 0.0, -0.277), Quat::IDENTITY)];
        assert!(bar_strip(&one).is_none());
        // Four lenses at the corners of a square: not on one line.
        let square: Vec<_> = [(-0.1, -0.1), (0.1, -0.1), (0.1, 0.1), (-0.1, 0.1)]
            .into_iter()
            .map(|(x, y)| (Vec3::new(x, y, 0.0), Quat::IDENTITY))
            .collect();
        assert!(bar_strip(&square).is_none());
        // Four on a line but fanned out: not one wash.
        let fanned: Vec<_> = (0..4)
            .map(|i| {
                (
                    Vec3::new(i as f32 * 0.1, 0.0, 0.0),
                    Quat::from_rotation_y((i as f32) * 0.3),
                )
            })
            .collect();
        assert!(bar_strip(&fanned).is_none());
    }
}
