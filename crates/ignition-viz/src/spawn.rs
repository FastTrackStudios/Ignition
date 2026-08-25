//! Turns a `Venue` into entities, once, at startup — and then keeps the
//! live-driven parts of them up to date.
//!
//! The pre-Bevy renderer rebuilt the entire scene's vertex buffer every
//! frame, because a flat vertex buffer was the only thing it had. Here
//! the room is spawned once and never touched again, and a lit fixture's
//! per-frame work is writing a handful of numbers into its own material
//! and light: no geometry is rebuilt to move a mover.

use crate::beam::{beam_mesh, beam_transform, BeamMaterial};
use crate::channel_map::channel_map_for;
use crate::dmx::DmxUniverses;
use crate::fixture_profile::{
    beam_half_angle_deg, resolve_fixture, BeamThrow, BodyVisual, BEAM_CONE_SEGMENTS,
};
use crate::gdtf_geometry::{self, GdtfLibrary, PanJoint, TiltJoint};
use crate::venue::Venue;
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

/// A fixture patched at or above the ceiling (less this slack) counts as
/// rigged to it. Generous, because "on the truss" and "at ceiling height"
/// are the same thing to an operator and venue data rounds differently.
const CEILING_RIG_TOLERANCE: f32 = 0.35;

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
    /// something visible. At 0 a beam is inert, the way it would
    /// genuinely look in clean air.
    pub haze: f32,
    /// Non-fixture room lighting. 0 by default: a real dark venue has no
    /// ambient fill, and everything you see is a fixture's beam or its
    /// spill. Only a dial back toward a lit room for readability.
    pub ambient: f32,
    /// Whether to draw the props layer — see `VizConfig::show_props`.
    pub show_props: bool,
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
pub struct FixtureHead {
    /// The QLC+ mesh's own convention correction (see
    /// `fixture_profile`'s `moving_head_pre_rotate`). The live update
    /// rewrites this entity's rotation every frame, so it has to compose
    /// the tilt *onto* this rather than replacing it — otherwise the head
    /// and its beam end up aimed along the mesh's unrotated axis, which
    /// is 180 degrees out.
    pub pre_rotate: Quat,
}

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

/// Whether a fixture counts as rigged to the overhead surface, rather
/// than standing on the floor or clamped to a wall.
fn is_rigged_overhead(fixture_pos: Vec3, ceiling_pos: Vec3) -> bool {
    fixture_pos.z >= ceiling_pos.z - CEILING_RIG_TOLERANCE
}

/// A fixture's pose expressed relative to the surface it hangs from, so
/// moving that surface moves the fixture with it.
///
/// The height is clamped to hang *below* the surface. Venue data can put
/// a truss above the room's own ceiling plane — Norco's does, by half a
/// metre — and a fixture left there is invisible from inside the room and
/// shines its beam from outside the roof.
fn rig_to_surface(
    fixture_pos: Vec3,
    fixture_rot: Quat,
    surface_pos: Vec3,
    surface_rot: Quat,
) -> (Vec3, Quat) {
    let inv = surface_rot.inverse();
    let mut local = inv * (fixture_pos - surface_pos);
    local.z = local.z.min(-RIG_DROP);
    (local, inv * fixture_rot)
}

/// Spawns the room, its screens and props, and one entity per patched
/// fixture (plus that fixture's beam and spill, initially hidden).
pub fn spawn_venue(
    mut commands: Commands,
    venue: Res<VenueRes>,
    settings: Res<VizSettings>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut standard: ResMut<Assets<StandardMaterial>>,
    mut beams: ResMut<Assets<BeamMaterial>>,
    gdtf_library: Res<GdtfLibraryRes>,
) {
    let venue = &venue.0;
    let unit_cube = meshes.add(Cuboid::from_length(1.0));
    // Name -> (anchor entity, world position, world rotation), so a
    // fixture rigged to a room surface can be parented to it.
    let mut room_anchors: std::collections::HashMap<String, (Entity, Vec3, Quat)> =
        std::collections::HashMap::new();
    let beam_cone = meshes.add(Mesh::from(beam_mesh().mesh().resolution(BEAM_CONE_SEGMENTS)));

    // Room surfaces are lit only by the rig, which is the point — but a
    // fixture that is switched off would then be invisible in a dark
    // room, and an operator needs to see where the rig physically *is*
    // whether or not it is currently doing anything. Fixture bodies get a
    // small emissive term so they read as their own objects: enough for
    // bloom to give them a soft presence, far below anything a lit beam
    // puts out.
    let mut solid = |color: Color, emissive: f32| {
        let base = LinearRgba::from(color);
        standard.add(StandardMaterial {
            base_color: color,
            emissive: LinearRgba::rgb(base.red * emissive, base.green * emissive, base.blue * emissive),
            perceptual_roughness: 0.9,
            ..default()
        })
    };
    /// Room geometry emits nothing; only the rig lights it.
    const UNLIT: f32 = 0.0;
    /// A fixture's own housing, so the rig is visible in the dark.
    const FIXTURE_GLOW: f32 = 1.6;

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
                Transform { translation: center, rotation: rot, scale: Vec3::ONE },
                Visibility::default(),
                Name::new(g.name.clone()),
            ))
            .id();
        commands.spawn((
            Mesh3d(unit_cube.clone()),
            MeshMaterial3d(solid(color, UNLIT)),
            Transform::from_scale(size),
            ChildOf(anchor),
        ));
        room_anchors.insert(g.name.clone(), (anchor, center, rot));
        if g.name.starts_with("Column") {
            // A black capstone on top of the column — its own entity
            // rather than trying to two-tone one box.
            let cap_height = 0.06;
            commands.spawn((
                Mesh3d(unit_cube.clone()),
                MeshMaterial3d(solid(COLUMN_CAP_COLOR, UNLIT)),
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
    for g in venue.props.iter().filter(|g| g.name.starts_with("Pillar") && !settings.skip(&g.name)) {
        commands.spawn((
            Mesh3d(unit_cube.clone()),
            MeshMaterial3d(solid(PILLAR_COLOR, UNLIT)),
            Transform {
                translation: g.position.to_vec3(),
                rotation: g.orientation(),
                scale: g.size.to_vec3().max(Vec3::splat(0.02)),
            },
            Name::new(g.name.clone()),
        ));
    }

    if settings.show_props {
        for g in &venue.props {
            // People have no dedicated shape yet: as a plain box a
            // standing person is just a tall box with no readable
            // silhouette, easy to mistake for a fixture. Pillars are
            // drawn unconditionally above — architecture, not dressing.
            if g.name.starts_with("Person") || g.name.starts_with("Pillar") || settings.skip(&g.name) {
                continue;
            }
            commands.spawn((
                Mesh3d(unit_cube.clone()),
                MeshMaterial3d(solid(PROP_COLOR, UNLIT)),
                Transform {
                    translation: g.position.to_vec3(),
                    rotation: g.orientation(),
                    scale: g.size.to_vec3().max(Vec3::splat(0.02)),
                },
                Name::new(g.name.clone()),
            ));
        }
    }

    for g in &venue.screens {
        if settings.skip(&g.name) {
            continue;
        }
        let rot = g.orientation();
        let size = g.size.to_vec3();
        // Same base-pivot convention as walls: a TV's position is its
        // bottom edge. The panel's local Y is its height axis.
        let center = g.position.to_vec3() + rot * Vec3::Y * (size.y * 0.5);
        commands.spawn((
            Mesh3d(unit_cube.clone()),
            MeshMaterial3d(solid(SCREEN_COLOR, UNLIT)),
            Transform {
                translation: center,
                rotation: rot,
                scale: Vec3::new(size.x, size.y, 0.05_f32.max(size.z)),
            },
            Name::new(g.name.clone()),
        ));
    }

    // The surface an overhead fixture is rigged to, if the venue has one.
    let ceiling = room_anchors.get("Ceiling").copied();

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
        let body_material = solid(body_color, FIXTURE_GLOW);

        // A real GDTF profile, when the library has one for this
        // manufacturer/model, replaces the QLC+ category mesh entirely:
        // real nested geometry with the manufacturer's own dimensions,
        // and joints the file itself identifies rather than a Z-split
        // guessed from a placeholder mesh's vertex histogram.
        let gdtf = gdtf_library.0.as_ref().and_then(|lib| lib.find(manufacturer, model));

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
        let rigged_to = ceiling
            .filter(|(_, ceiling_pos, _)| is_rigged_overhead(f.position.to_vec3(), *ceiling_pos));

        let (parent, local_pos, local_rot) = match rigged_to {
            Some((anchor, ceiling_pos, ceiling_rot)) => {
                let (local_pos, local_rot) =
                    rig_to_surface(f.position.to_vec3(), f.orientation(), ceiling_pos, ceiling_rot);
                (Some(anchor), local_pos, local_rot)
            }
            None => (None, f.position.to_vec3(), f.orientation()),
        };

        let mut root_cmd = commands.spawn((
            Fixture { index, base_rot: local_rot },
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
                BodyVisual::Mesh { asset, scale, pre_rotate, split_z } => {
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
                    let anchor = visual.position - f.position.to_vec3();
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
                            let pivot = anchor + visual.head_pivot.unwrap_or(Vec3::ZERO);
                            let head = commands
                                .spawn((
                                    FixtureHead { pre_rotate: *pre_rotate },
                                    Transform {
                                        translation: pivot,
                                        rotation: *pre_rotate,
                                        scale: Vec3::ONE,
                                    },
                                    Visibility::default(),
                                    ChildOf(root),
                                ))
                                .id();
                            commands.spawn((
                                Mesh3d(meshes.add(asset.to_bevy_mesh(head_split))),
                                MeshMaterial3d(body_material.clone()),
                                Transform::from_scale(Vec3::splat(*scale)),
                                ChildOf(head),
                            ));
                            emitters.push(head);
                        }
                        None => emitters.push(root),
                    }
                }
                BodyVisual::Bar { length, width, height } => {
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
            commands.entity(emitter).insert((BeamEmitter { fixture: index }, EmitterState::default()));
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
            commands.spawn((
                FixtureSpill,
                SpotLight {
                    intensity: 0.0,
                    range: 40.0,
                    shadow_maps_enabled: false,
                    ..default()
                },
                Transform::default(),
                Visibility::Hidden,
                Name::new(format!("{} spill", f.name)),
                ChildOf(emitter),
            ));
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
    mut heads: Query<(&FixtureHead, &ChildOf, &mut Transform), Without<Fixture>>,
    mut pan_joints: Query<
        (&ChildOf, &mut Transform),
        (With<PanJoint>, Without<Fixture>, Without<FixtureHead>, Without<TiltJoint>),
    >,
    mut tilt_joints: Query<
        (&ChildOf, &mut Transform),
        (With<TiltJoint>, Without<Fixture>, Without<FixtureHead>, Without<PanJoint>),
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
        let (Some(addr), Some(map)) = (f.dmx_address(), channel_map_for(manufacturer, model)) else {
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
        });
    }

    // Which venue record each fixture root came from, so the joint passes
    // can resolve their own fixture by walking up to it.
    let mut index_of_root: std::collections::HashMap<Entity, usize> = std::collections::HashMap::new();
    for (entity, fixture, mut transform) in &mut fixtures {
        index_of_root.insert(entity, fixture.index);
        let Some(Some(live)) = resolved.get(fixture.index) else { continue };
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

    for (head, parent, mut transform) in &mut heads {
        let Some(live) = fixture_index_of(parent.parent())
            .and_then(|i| resolved.get(i))
            .and_then(|o| o.as_ref())
        else {
            continue;
        };
        transform.rotation = live.tilt * head.pre_rotate;
    }

    // GDTF joints: the file itself said which node each attribute drives,
    // so a live reading rotates the manufacturer's own axis. Everything
    // below the joint follows because Bevy propagates transforms.
    for (parent, mut transform) in &mut pan_joints {
        let Some(live) = fixture_index_of(parent.parent()).and_then(|i| resolved.get(i)).and_then(|o| o.as_ref())
        else {
            continue;
        };
        transform.rotation = live.pan;
    }
    for (parent, mut transform) in &mut tilt_joints {
        let Some(live) = fixture_index_of(parent.parent()).and_then(|i| resolved.get(i)).and_then(|o| o.as_ref())
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
        (&mut Transform, &mut Visibility, &MeshMaterial3d<BeamMaterial>),
        (With<FixtureBeam>, Without<FixtureSpill>),
    >,
    mut spill_q: Query<(&mut Visibility, &mut SpotLight), (With<FixtureSpill>, Without<FixtureBeam>)>,
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
                            m.direction_angle =
                                Vec4::new(direction.x, direction.y, direction.z, state.half_angle_deg);
                            m.origin_length = Vec4::new(origin.x, origin.y, origin.z, length);
                            m.params = Vec4::new(settings.haze, seconds, 0.0, 0.0);
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
                        light.inner_angle = outer * 0.8;
                        light.range = length * 1.2;
                        light.color = Color::srgb(color[0], color[1], color[2]);
                        // Lumens. The fixture's own brightness is already
                        // in `color`, so this is only the headroom that
                        // makes a lit surface read against a dark room.
                        light.intensity = 60_000.0;
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

/// Applies the ambient dial to Bevy's global ambient light.
pub fn apply_ambient(settings: Res<VizSettings>, mut ambient: ResMut<GlobalAmbientLight>) {
    if settings.is_changed() {
        ambient.brightness = settings.ambient * 200.0;
    }
}

#[cfg(test)]
mod rigging_tests {
    use super::*;

    const CEILING: Vec3 = Vec3::new(0.0, 0.0, 2.743);

    #[test]
    fn a_truss_fixture_counts_as_overhead_and_a_floor_one_does_not() {
        // Norco's real numbers: 47 pars at 3.25, floor movers near zero.
        assert!(is_rigged_overhead(Vec3::new(1.0, 2.0, 3.25), CEILING));
        assert!(!is_rigged_overhead(Vec3::new(1.0, 2.0, 0.02), CEILING));
        // Just under the ceiling still counts — "on the truss" and "at
        // ceiling height" are the same thing to an operator.
        assert!(is_rigged_overhead(Vec3::new(0.0, 0.0, 2.5), CEILING));
    }

    #[test]
    fn a_fixture_above_the_ceiling_is_pulled_down_to_hang_from_it() {
        let (local, _) = rig_to_surface(Vec3::new(1.0, 2.0, 3.25), Quat::IDENTITY, CEILING, Quat::IDENTITY);
        // Real x/y kept, height clamped to just below the surface.
        assert_eq!((local.x, local.y), (1.0, 2.0));
        assert!(local.z < 0.0, "must hang below the ceiling, got {}", local.z);
        assert!((local.z + RIG_DROP).abs() < 1e-6, "{}", local.z);
    }

    #[test]
    fn a_fixture_already_below_the_ceiling_keeps_its_real_drop() {
        let (local, _) = rig_to_surface(Vec3::new(0.0, 0.0, 2.0), Quat::IDENTITY, CEILING, Quat::IDENTITY);
        assert!((local.z - (2.0 - CEILING.z)).abs() < 1e-6, "{}", local.z);
    }

    #[test]
    fn the_local_pose_reproduces_the_world_pose_when_the_surface_is_rotated() {
        let surface_rot = Quat::from_rotation_z(0.7);
        let fixture_pos = Vec3::new(1.0, 2.0, 2.0);
        let fixture_rot = Quat::from_rotation_x(0.3);
        let (local_pos, local_rot) = rig_to_surface(fixture_pos, fixture_rot, CEILING, surface_rot);
        // Composing the parent back on must return where it started.
        let world_pos = CEILING + surface_rot * local_pos;
        assert!(world_pos.distance(fixture_pos) < 1e-5, "{world_pos:?}");
        assert!((surface_rot * local_rot).angle_between(fixture_rot) < 1e-5);
    }
}
