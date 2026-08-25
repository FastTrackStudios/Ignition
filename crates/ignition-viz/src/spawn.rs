//! Turns a `Venue` into entities, once, at startup — and then keeps the
//! live-driven parts of them up to date.
//!
//! The pre-Bevy renderer rebuilt the entire scene's vertex buffer every
//! frame, because a flat vertex buffer was the only thing it had. Here
//! the room is spawned once and never touched again, and a lit fixture's
//! per-frame work is writing a handful of numbers into its own material
//! and light: no geometry is rebuilt to move a mover.

use crate::beam::{BeamMaterial, beam_mesh, beam_transform};
use crate::channel_map::channel_map_for;
use crate::dmx::DmxUniverses;
use crate::fixture_profile::{
    BEAM_CONE_SEGMENTS, BeamThrow, BodyVisual, LUMENS_PER_WATT, SHAFT_CANDELA_THRESHOLD,
    beam_half_angle_deg, peak_candela, power_watts, resolve_fixture,
};
use crate::gdtf_geometry::{self, GdtfLibrary, PanJoint, TiltJoint};
use crate::venue::Venue;
use bevy::light::{FogVolume, VolumetricLight};
use bevy::prelude::*;

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

/// Maps the operator's haze dial onto Bevy's `FogVolume::density_factor`,
/// whose own default is 0.1. Above roughly 0.3 the room stops being a
/// room: the fog absorbs its own light shafts and everything goes flat.
const VOLUMETRIC_HAZE_SCALE: f32 = 0.11;

/// The same dial for the hand-drawn cone, whose brightness is a plain
/// multiplier on an additive material rather than a density.
const SHADER_HAZE_SCALE: f32 = 10.0;

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
const CEILING_COLOR: Color = Color::srgb(0.045, 0.045, 0.05);
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
    /// Normalized deliberately: the two beam styles want wildly
    /// different raw numbers (Bevy's `density_factor` defaults to 0.1;
    /// the hand-drawn cone's gain wanted ~10), and an operator dial that
    /// changes meaning when you switch renderer is not a dial. Each
    /// style scales it on the way in.
    pub haze: f32,
    /// Non-fixture room lighting. 0 by default: a real dark venue has no
    /// ambient fill, and everything you see is a fixture's beam or its
    /// spill. Only a dial back toward a lit room for readability.
    pub ambient: f32,
    /// Whether to draw the props layer — see `VizConfig::show_props`.
    pub show_props: bool,
    /// Global exposure: a multiplier on every fixture's *real* luminous
    /// output (`fixture_profile::power_watts` x `LUMENS_PER_WATT`).
    ///
    /// This replaced a single absolute lumen figure applied to every
    /// fixture alike, which meant a 36W par and a 150W beam fixture were
    /// equally bright and therefore cut equally visible shafts. Relative
    /// output is now the fixture's own; this only sets the overall level.
    pub exposure: f32,
    /// How beams are drawn — see `BeamStyle`.
    pub beam_style: BeamStyle,
    /// Asset path of what every screen displays, or `None` for screens
    /// that are off. One image for all of them is the placeholder; real
    /// projection mapping needs per-surface content and its own
    /// addressing, which this is the first piece of.
    pub screen_content: Option<String>,
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

/// How a beam in the air is produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BeamStyle {
    /// Bevy's own volumetric fog: a `FogVolume` filling the room, and
    /// every fixture's spot light marked `VolumetricLight`, so a shaft
    /// is what the renderer computes from the light actually passing
    /// through haze — occluded by geometry, shaped by the light's real
    /// cone, and with no beam mesh anywhere.
    ///
    /// This is the physical answer and the one that scales: haze is a
    /// property of the room rather than something re-drawn per fixture.
    /// It needs shadow maps on every contributing light, which is the
    /// cost to watch on a rig this size.
    Volumetric,
    /// The hand-drawn additive cone (`beam.wgsl`, a port of ASLS's own
    /// beam shader). Cheap and independent of shadow maps, but the haze
    /// is drawn *per beam* rather than being in the room, so beams do
    /// not interact with each other or with anything they pass behind.
    Shader,
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
    pub half_angle_deg: f32,
    /// Inner cone angle as a fraction of the outer one. 0 is fully soft
    /// (ASLS's unfocused default); nearer 1 is a hard-edged beam.
    pub penumbra_inner: f32,
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
#[derive(Component)]
pub struct FixtureBody {
    pub material: Handle<StandardMaterial>,
}

/// A fixture's beam cone — a child of its `BeamEmitter`.
#[derive(Component)]
pub struct FixtureBeam;

/// A fixture's spill: the light it actually throws onto the room, as
/// distinct from the visible shaft of haze the beam cone draws. Also a
/// child of the emitter, with an identity local transform — a Bevy spot
/// light shines along its entity's -Z, which is already this project's
/// beam-axis convention, so it needs no aiming code at all.
#[derive(Component)]
pub struct FixtureSpill;

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

/// A screen showing content: emitting its own image rather than merely
/// reflecting the room's light.
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
    mut beams: ResMut<Assets<BeamMaterial>>,
    gdtf_library: Res<GdtfLibraryRes>,
    asset_server: Res<AssetServer>,
) {
    let venue = &venue.0;
    let unit_cube = meshes.add(Cuboid::from_length(1.0));
    // Every piece of room structure a fixture might be mounted to.
    let mut rig_surfaces: Vec<RigSurface> = Vec::new();
    let beam_cone = meshes.add(Mesh::from(
        beam_mesh().mesh().resolution(BEAM_CONE_SEGMENTS),
    ));

    // Room surfaces are lit only by the rig, which is the point — but a
    // fixture that is switched off would then be invisible in a dark
    // room, and an operator needs to see where the rig physically *is*
    // whether or not it is currently doing anything. Fixture bodies get a
    // small emissive term so they read as their own objects: enough for
    // bloom to give them a soft presence, far below anything a lit beam
    // puts out.

    /// Room geometry emits nothing; only the rig lights it.
    const UNLIT: f32 = 0.0;
    /// Fixtures start dark; `update_fixture_bodies` lights a housing
    /// once its own lamp is up.
    const FIXTURE_GLOW: f32 = 0.0;

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
        commands.spawn((
            Mesh3d(unit_cube.clone()),
            MeshMaterial3d(solid(&mut standard, color, UNLIT)),
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

    let screen_content: Option<Handle<Image>> = settings
        .screen_content
        .as_ref()
        .map(|path| asset_server.load(path.clone()));
    let screen_quad = meshes.add(Rectangle::new(1.0, 1.0));

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
        if let Some(content) = &screen_content {
            // The display itself: a quad just proud of the bezel, lit by
            // its own content rather than by the room.
            commands.spawn((
                ScreenSurface,
                Mesh3d(screen_quad.clone()),
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

    if settings.beam_style == BeamStyle::Volumetric {
        // One volume covering the room. Haze is a property of the air in
        // here, not of any one fixture, which is the whole reason this
        // reads better than a cone per beam: two beams crossing actually
        // brighten where they overlap.
        let (min, max) = venue.bounds();
        let center = (min + max) * 0.5;
        // Generous over the venue's own bounds, which are a bound on
        // object *centres* — the fog has to reach past the fixtures and
        // the floor, not stop at them.
        let size = (max - min).max(Vec3::splat(4.0)) * 1.6;
        commands.spawn((
            FogVolume {
                density_factor: settings.haze * VOLUMETRIC_HAZE_SCALE,
                scattering: 0.6,
                absorption: 0.05,
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
    for (index, f) in venue.fixtures.iter().enumerate() {
        // Unpatched channels (Norco's phantom 19/98) have no real
        // position — the patch reports (0,0,0), which would render as a
        // stray fixture at the room's origin.
        if !f.patched {
            continue;
        }
        let manufacturer = f.manufacturer.as_deref().unwrap_or("");
        let model = f.model.as_deref().unwrap_or("");
        let visual = resolve_fixture(
            f.position.to_vec3(),
            f.orientation(),
            None,
            manufacturer,
            model,
            None,
            &throw,
        );
        let body_color: Color = {
            let c = f.kind().color();
            Color::srgb(c[0], c[1], c[2])
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
            FixtureBody {
                material: body_material.clone(),
            },
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

        // Where the beam comes from. The GDTF path gets it from the
        // file's own `<Beam>` node; otherwise it is the head if the
        // fixture has one, and the body if it does not.
        let mut emitters: Vec<Entity> = Vec::new();

        if let Some(gdtf) = gdtf {
            gdtf_geometry::spawn_gdtf_tree(
                &mut commands,
                root,
                &gdtf.root,
                &mut meshes,
                &body_material,
                &mut emitters,
            );
        } else {
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
                        Mesh3d(meshes.add(asset.to_bevy_mesh(yoke_split))),
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
                                Mesh3d(meshes.add(asset.to_bevy_mesh(head_split))),
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
        // despawning entities as cues fade in and out.
        for emitter in emitters {
            commands
                .entity(emitter)
                .insert((BeamEmitter { fixture: index }, EmitterState::default()));
            if settings.beam_style == BeamStyle::Shader {
                commands.spawn((
                    FixtureBeam,
                    Mesh3d(beam_cone.clone()),
                    MeshMaterial3d(beams.add(BeamMaterial::new(
                        LinearRgba::BLACK,
                        Vec3::ZERO,
                        Vec3::NEG_Z,
                        12.5,
                        1.0,
                        settings.haze,
                    ))),
                    Transform::default(),
                    Visibility::Hidden,
                    Name::new(format!("{} beam", f.name)),
                    ChildOf(emitter),
                ));
            }
            // Whether this fixture is bright enough per-direction to
            // light the air into a visible shaft, or only bright enough
            // to light what it points at. See `SHAFT_CANDELA_THRESHOLD`.
            let candela = peak_candela(
                power_watts(manufacturer, model) * LUMENS_PER_WATT,
                beam_half_angle_deg(f.beam_angle_deg),
            );
            let cuts_a_shaft =
                settings.beam_style == BeamStyle::Volumetric && candela >= SHAFT_CANDELA_THRESHOLD;

            let mut spill = commands.spawn((
                FixtureSpill,
                SpotLight {
                    intensity: 0.0,
                    range: 40.0,
                    // Shadow maps are what volumetric shafts are
                    // raymarched against, and they are the expensive
                    // part — so only the fixtures that actually cast a
                    // shaft pay for them.
                    shadow_maps_enabled: cuts_a_shaft,
                    ..default()
                },
                Transform::default(),
                Visibility::Hidden,
                Name::new(format!("{} spill", f.name)),
                ChildOf(emitter),
            ));
            if cuts_a_shaft {
                spill.insert(VolumetricLight);
            }
        }
    }
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
    mut emitters: Query<(&BeamEmitter, &mut EmitterState)>,
    child_of: Query<&ChildOf>,
) {
    let Some(dmx) = dmx else { return };
    let venue = &venue.0;

    // Resolve once, so every entity family below agrees on the same
    // frame's DMX rather than each re-reading it.
    let mut resolved: Vec<Option<Live>> = (0..venue.fixtures.len()).map(|_| None).collect();
    for (index, f) in venue.fixtures.iter().enumerate() {
        if !f.patched {
            continue;
        }
        let manufacturer = f.manufacturer.as_deref().unwrap_or("");
        let model = f.model.as_deref().unwrap_or("");
        let (Some(addr), Some(map)) = (f.dmx_address(), channel_map_for(manufacturer, model))
        else {
            continue;
        };
        let live = dmx.0.resolve(&addr, &map);

        // A fixture with no colour channel at all (a hazer's plain Dimmer
        // map) still emits — as white, since a hazer genuinely does haze
        // up whatever light is already in the air, and no beam at all
        // would read as broken.
        let color = (live.dimmer > MIN_VISIBLE_DIMMER).then(|| {
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
            half_angle_deg: beam_half_angle_deg(f.beam_angle_deg),
            penumbra_inner: penumbra_inner_for(f.kind()),
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

    for (emitter, mut state) in &mut emitters {
        match resolved.get(emitter.fixture).and_then(|o| o.as_ref()) {
            Some(live) => {
                state.color = live.color;
                state.half_angle_deg = live.half_angle_deg;
                state.penumbra_inner = live.penumbra_inner;
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
    half_angle_deg: f32,
    penumbra_inner: f32,
    lumens: f32,
}

/// How hard a fixture's cone edge is, as a fraction of its outer angle.
///
/// A par or wash throws a soft-edged pool with no defined edge at all; a
/// beam-type moving head throws a shaft you can see the sides of. Both
/// are the same `SpotLight` with a different penumbra, which is how ASLS
/// models it too — see `update_beams`.
fn penumbra_inner_for(kind: crate::venue::FixtureKind) -> f32 {
    match kind {
        // Fully soft, ASLS's unfocused default.
        crate::venue::FixtureKind::Wash | crate::venue::FixtureKind::Other => 0.0,
        // Movers here are beam/gobo fixtures with a real optic; keep
        // enough of an inner cone that the shaft has visible sides.
        crate::venue::FixtureKind::Mover => 0.55,
    }
}

/// Sizes, aims and colours every beam and spill from where its emitter
/// actually ended up.
///
/// Runs in `PostUpdate` after transform propagation, which is the whole
/// point: the emitter's `GlobalTransform` already accounts for the mount
/// pose, the pan, the tilt and — for a GDTF fixture — every joint and
/// offset in the manufacturer's own geometry tree. Nothing here knows or
/// cares which of those applied.
#[allow(clippy::type_complexity)]
pub fn update_beams(
    time: Res<Time>,
    venue: Res<VenueRes>,
    settings: Res<VizSettings>,
    mut beam_materials: ResMut<Assets<BeamMaterial>>,
    emitters: Query<(&EmitterState, &GlobalTransform, Option<&Children>)>,
    mut beam_q: Query<
        (
            &mut Transform,
            &mut Visibility,
            &MeshMaterial3d<BeamMaterial>,
        ),
        (With<FixtureBeam>, Without<FixtureSpill>),
    >,
    mut spill_q: Query<
        (&mut Visibility, &mut SpotLight),
        (With<FixtureSpill>, Without<FixtureBeam>),
    >,
) {
    let throw = BeamThrow::for_venue(&venue.0);
    let seconds = time.elapsed_secs();

    for (state, global, children) in &emitters {
        let Some(children) = children else { continue };
        let origin = global.translation();
        // This project's aim convention: a fixture emits along its own
        // local -Z, which is also the axis a Bevy spot light shines down.
        let direction = (global.rotation() * Vec3::NEG_Z).normalize_or_zero();
        let length = throw.reach(origin, direction);
        let far_radius = (length * state.half_angle_deg.to_radians().tan()).max(0.05);

        for child in children.iter() {
            if let Ok((mut transform, mut visibility, material)) = beam_q.get_mut(child) {
                match state.color {
                    Some(color) => {
                        *visibility = Visibility::Visible;
                        // Local to the emitter, so it inherits the aim.
                        *transform = beam_transform(Vec3::ZERO, Vec3::NEG_Z, length, far_radius);
                        if let Some(mut m) = beam_materials.get_mut(&material.0) {
                            m.color = LinearRgba::rgb(color[0], color[1], color[2]);
                            // The shader works in world space, so these
                            // stay world even though the mesh is local.
                            m.direction_angle = Vec4::new(
                                direction.x,
                                direction.y,
                                direction.z,
                                state.half_angle_deg,
                            );
                            m.origin_length = Vec4::new(origin.x, origin.y, origin.z, length);
                            m.params =
                                Vec4::new(settings.haze * SHADER_HAZE_SCALE, seconds, 0.0, 0.0);
                        }
                    }
                    None => *visibility = Visibility::Hidden,
                }
            }
            if let Ok((mut visibility, mut light)) = spill_q.get_mut(child) {
                match state.color {
                    Some(color) => {
                        *visibility = Visibility::Visible;
                        let outer = state.half_angle_deg.to_radians();
                        light.outer_angle = outer;
                        // A fully soft cone, not a hard-edged one. This
                        // is ASLS's default: their focus channel drives
                        // `SpotLight.penumbra` as `1.2 - 1.2*(focus/100)`
                        // clamped to at least 0.3, so an unfocused
                        // fixture is entirely penumbra. Three.js's
                        // penumbra is the fraction of the cone that
                        // falls off, and Bevy spells the same thing as
                        // the gap between inner and outer angle — so
                        // penumbra 1.0 is `inner_angle = 0`.
                        //
                        // This is most of what separates a par from a
                        // beam fixture. Reported as "they don't have the
                        // kind of direct beam lights that they are
                        // showing right now": a hard inner cone made
                        // every par read as a tight defined shaft
                        // instead of a wash.
                        light.inner_angle = outer * state.penumbra_inner;
                        light.range = length * 1.2;
                        light.color = Color::srgb(color[0], color[1], color[2]);
                        // The fixture's own output, scaled only by the
                        // global exposure. Bevy divides lumens by the
                        // cone's solid angle to get radiance, so this is
                        // what makes a narrow beam fixture cut a shaft
                        // through the haze while a wide par of similar
                        // wattage just lights what it points at — which
                        // is what they actually do.
                        light.intensity = state.lumens * settings.exposure;
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
    venue: Res<VenueRes>,
    dmx: Option<Res<DmxRes>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    bodies: Query<(&Fixture, &FixtureBody)>,
) {
    let Some(dmx) = dmx else { return };
    let venue = &venue.0;

    for (fixture, body) in &bodies {
        let Some(record) = venue.fixtures.get(fixture.index) else {
            continue;
        };
        let manufacturer = record.manufacturer.as_deref().unwrap_or("");
        let model = record.model.as_deref().unwrap_or("");

        let live = record
            .dmx_address()
            .zip(channel_map_for(manufacturer, model))
            .map(|(addr, map)| dmx.0.resolve(&addr, &map));

        let emissive = match live {
            Some(live) if live.dimmer > MIN_VISIBLE_DIMMER => {
                let c = if live.has_color { live.color } else { [1.0; 3] };
                // Scaled by the dimmer so a fixture at 20% reads as at
                // 20%, and hot enough at full for bloom to halo it the
                // way a real lit lens does.
                let gain = live.dimmer * LIT_BODY_GLOW;
                LinearRgba::rgb(c[0] * gain, c[1] * gain, c[2] * gain)
            }
            // Dark: emits nothing, and is lit only by whatever else in
            // the rig happens to fall on it.
            _ => LinearRgba::BLACK,
        };

        if let Some(mut material) = materials.get_mut(&body.material) {
            material.emissive = emissive;
        }
    }
}

/// Applies the ambient dial to Bevy's global ambient light.
pub fn apply_ambient(settings: Res<VizSettings>, mut ambient: ResMut<GlobalAmbientLight>) {
    if settings.is_changed() {
        ambient.brightness = settings.ambient * 200.0;
    }
}

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
