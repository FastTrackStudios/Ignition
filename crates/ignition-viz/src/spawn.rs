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
use crate::fixture_profile::{resolve_fixture, BeamThrow, BeamVisual, BodyVisual, LiveEmission, BEAM_CONE_SEGMENTS};
use crate::venue::Venue;
use bevy::prelude::*;

/// Below this, a fixture reads as blacked out — no spill light, no beam.
/// Keeps a dimmer sitting at DMX 1-2 (rounding noise, not an actual cue)
/// from drawing a visible beam.
const MIN_VISIBLE_DIMMER: f32 = 0.02;

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
}

/// Marks a fixture's root entity and remembers which venue record it came
/// from, so the live system can resolve it without a name lookup.
#[derive(Component)]
pub struct Fixture {
    pub index: usize,
}

/// The part of a moving head that tilts — a child of the fixture root, so
/// tilting it is one `Transform` write and the base stays put.
#[derive(Component)]
pub struct FixtureHead;

/// A fixture's beam cone.
#[derive(Component)]
pub struct FixtureBeam {
    pub fixture: usize,
}

/// A fixture's spill: the light it actually throws onto the room, as
/// distinct from the visible shaft of haze the beam cone draws.
#[derive(Component)]
pub struct FixtureSpill {
    pub fixture: usize,
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
) {
    let venue = &venue.0;
    let unit_cube = meshes.add(Cuboid::from_length(1.0));
    let beam_cone = meshes.add(Mesh::from(beam_mesh().mesh().resolution(BEAM_CONE_SEGMENTS)));

    let mut solid = |color: Color| {
        standard.add(StandardMaterial {
            base_color: color,
            perceptual_roughness: 0.9,
            ..default()
        })
    };

    for g in &venue.room {
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
        commands.spawn((
            Mesh3d(unit_cube.clone()),
            MeshMaterial3d(solid(color)),
            Transform { translation: center, rotation: rot, scale: size },
            Name::new(g.name.clone()),
        ));
        if g.name.starts_with("Column") {
            // A black capstone on top of the column — its own entity
            // rather than trying to two-tone one box.
            let cap_height = 0.06;
            commands.spawn((
                Mesh3d(unit_cube.clone()),
                MeshMaterial3d(solid(COLUMN_CAP_COLOR)),
                Transform {
                    translation: center + rot * Vec3::Z * (size.z * 0.5 + cap_height * 0.5),
                    rotation: rot,
                    scale: Vec3::new(size.x * 1.02, size.y * 1.02, cap_height),
                },
                Name::new(format!("{} cap", g.name)),
            ));
        }
    }

    // Pillars are an architectural detail like the columns, not
    // set-dressing — always drawn, unlike the rest of props.json.
    for g in venue.props.iter().filter(|g| g.name.starts_with("Pillar")) {
        commands.spawn((
            Mesh3d(unit_cube.clone()),
            MeshMaterial3d(solid(PILLAR_COLOR)),
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
            if g.name.starts_with("Person") || g.name.starts_with("Pillar") {
                continue;
            }
            commands.spawn((
                Mesh3d(unit_cube.clone()),
                MeshMaterial3d(solid(PROP_COLOR)),
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
        let rot = g.orientation();
        let size = g.size.to_vec3();
        // Same base-pivot convention as walls: a TV's position is its
        // bottom edge. The panel's local Y is its height axis.
        let center = g.position.to_vec3() + rot * Vec3::Y * (size.y * 0.5);
        commands.spawn((
            Mesh3d(unit_cube.clone()),
            MeshMaterial3d(solid(SCREEN_COLOR)),
            Transform {
                translation: center,
                rotation: rot,
                scale: Vec3::new(size.x, size.y, 0.05_f32.max(size.z)),
            },
            Name::new(g.name.clone()),
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
        let body_material = solid(body_color);

        let root = commands
            .spawn((
                Fixture { index },
                Transform {
                    translation: visual.position,
                    rotation: visual.body_rot,
                    scale: Vec3::ONE,
                },
                Visibility::default(),
                Name::new(f.name.clone()),
            ))
            .id();

        match &visual.body {
            BodyVisual::Mesh { asset, scale, pre_rotate, split_z } => {
                // With a split, the yoke is drawn on the root and the head
                // becomes a child that tilts about the split point. With
                // none, the whole mesh is the body.
                let (yoke_split, head_split) = match split_z {
                    Some(z) => (Some((*z, false)), Some((*z, true))),
                    None => (None, None),
                };
                commands.spawn((
                    Mesh3d(meshes.add(asset.to_bevy_mesh(yoke_split))),
                    MeshMaterial3d(body_material.clone()),
                    Transform {
                        rotation: *pre_rotate,
                        scale: Vec3::splat(*scale),
                        ..default()
                    },
                    ChildOf(root),
                ));
                if head_split.is_some() {
                    let pivot = visual.head_pivot.unwrap_or(Vec3::ZERO);
                    commands.spawn((
                        FixtureHead,
                        Mesh3d(meshes.add(asset.to_bevy_mesh(head_split))),
                        MeshMaterial3d(body_material.clone()),
                        Transform {
                            translation: pivot,
                            rotation: *pre_rotate,
                            scale: Vec3::splat(*scale),
                        },
                        ChildOf(root),
                    ));
                }
            }
            BodyVisual::Bar { length, width, height } => {
                commands.spawn((
                    Mesh3d(unit_cube.clone()),
                    MeshMaterial3d(body_material.clone()),
                    Transform::from_scale(Vec3::new(*length, *width, *height)),
                    ChildOf(root),
                ));
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

        // Beam and spill exist from the start and are simply hidden while
        // the fixture is dark — cheaper and steadier than spawning and
        // despawning entities as cues fade in and out.
        commands.spawn((
            FixtureBeam { fixture: index },
            Mesh3d(beam_cone.clone()),
            MeshMaterial3d(beams.add(BeamMaterial::new(
                LinearRgba::BLACK,
                visual.position,
                Vec3::NEG_Z,
                12.5,
                1.0,
                settings.haze,
            ))),
            Transform::default(),
            Visibility::Hidden,
            Name::new(format!("{} beam", f.name)),
        ));
        commands.spawn((
            FixtureSpill { fixture: index },
            SpotLight {
                intensity: 0.0,
                range: 40.0,
                shadow_maps_enabled: false,
                ..default()
            },
            Transform::default(),
            Visibility::Hidden,
            Name::new(format!("{} spill", f.name)),
        ));
    }
}

/// Resolves every patched fixture against the current DMX state and
/// writes the result into the entities `spawn_venue` already made: body
/// pan, head tilt, beam colour/aim/visibility, spill colour/intensity.
// A Bevy system's "arguments" are its data dependencies, which the
// scheduler reads to decide what can run in parallel — splitting this one
// up to satisfy an argument count would hide that, not simplify it.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub fn update_live_fixtures(
    time: Res<Time>,
    venue: Res<VenueRes>,
    dmx: Option<Res<DmxRes>>,
    settings: Res<VizSettings>,
    mut beam_materials: ResMut<Assets<BeamMaterial>>,
    // All four touch `Transform`, so Bevy needs them provably disjoint or
    // it refuses to run the system (B0001). Every fixture-shaped entity
    // carries exactly one of these four marker components, so excluding
    // the other three is both true and the cheapest thing to state.
    mut fixtures: Query<
        (Entity, &Fixture, &mut Transform),
        (Without<FixtureHead>, Without<FixtureBeam>, Without<FixtureSpill>),
    >,
    mut heads: Query<
        (&ChildOf, &mut Transform),
        (With<FixtureHead>, Without<Fixture>, Without<FixtureBeam>, Without<FixtureSpill>),
    >,
    mut beam_q: Query<
        (&FixtureBeam, &mut Transform, &mut Visibility, &MeshMaterial3d<BeamMaterial>),
        (Without<Fixture>, Without<FixtureHead>, Without<FixtureSpill>),
    >,
    mut spill_q: Query<
        (&FixtureSpill, &mut Transform, &mut Visibility, &mut SpotLight),
        (Without<Fixture>, Without<FixtureHead>, Without<FixtureBeam>),
    >,
) {
    let Some(dmx) = dmx else { return };
    let venue = &venue.0;
    let throw = BeamThrow::for_venue(venue);
    let seconds = time.elapsed_secs();

    // One pass to resolve, so the three entity families below all agree
    // on the same frame's DMX rather than re-resolving it each.
    let mut resolved: Vec<Option<Resolved>> = vec![None; venue.fixtures.len()];
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

        let pan_tilt = (live.pan_deg != 0.0 || live.tilt_deg != 0.0).then(|| {
            (
                Quat::from_axis_angle(Vec3::Z, live.pan_deg.to_radians()),
                Quat::from_axis_angle(Vec3::X, live.tilt_deg.to_radians()),
            )
        });

        // A fixture with no colour channel at all (a hazer's plain Dimmer
        // map) still emits — as white, since a hazer genuinely does haze
        // up whatever light is already in the air, and no beam at all
        // would read as broken.
        let emit = (live.dimmer > MIN_VISIBLE_DIMMER).then(|| {
            let color = if live.has_color {
                [
                    live.color[0] * live.dimmer,
                    live.color[1] * live.dimmer,
                    live.color[2] * live.dimmer,
                ]
            } else {
                [live.dimmer; 3]
            };
            LiveEmission { color, beam_angle_deg: f.beam_angle_deg }
        });

        let visual = resolve_fixture(
            f.position.to_vec3(),
            f.orientation(),
            pan_tilt,
            manufacturer,
            model,
            emit,
            &throw,
        );
        resolved[index] = Some(Resolved {
            position: visual.position,
            body_rot: visual.body_rot,
            head_tilt: visual.head_tilt,
            head_pivot: visual.head_pivot,
            beam: visual.beam,
        });
    }

    // Which venue record each fixture root came from, so the head pass
    // below can resolve its parent without querying `fixtures` again
    // while that query is still borrowed.
    let mut index_of_root: std::collections::HashMap<Entity, usize> = std::collections::HashMap::new();
    for (entity, fixture, mut transform) in &mut fixtures {
        index_of_root.insert(entity, fixture.index);
        let Some(Some(r)) = resolved.get(fixture.index) else { continue };
        transform.translation = r.position;
        transform.rotation = r.body_rot;
    }

    for (parent, mut transform) in &mut heads {
        let Some(r) = index_of_root
            .get(&parent.parent())
            .and_then(|i| resolved.get(*i))
            .and_then(|o| o.as_ref())
        else {
            continue;
        };
        // Tilt is relative to the body, which already carries pan, so the
        // head's own rotation is the tilt composed onto the mesh's
        // pre-rotate — the same product the body uses, minus pan.
        transform.translation = r.head_pivot.unwrap_or(Vec3::ZERO);
        if let Some(tilt) = r.head_tilt {
            transform.rotation = tilt;
        }
    }

    for (beam, mut transform, mut visibility, material) in &mut beam_q {
        let lit = resolved.get(beam.fixture).and_then(|o| o.as_ref()).and_then(|r| r.beam.as_ref());
        match lit {
            Some(b) => {
                *visibility = Visibility::Visible;
                *transform = beam_transform(b.origin, b.direction, b.length, b.far_radius);
                if let Some(mut m) = beam_materials.get_mut(&material.0) {
                    m.color = LinearRgba::rgb(b.color[0], b.color[1], b.color[2]);
                    m.direction_angle =
                        Vec4::new(b.direction.x, b.direction.y, b.direction.z, b.half_angle_deg);
                    m.origin_length = Vec4::new(b.origin.x, b.origin.y, b.origin.z, b.length);
                    m.params = Vec4::new(settings.haze, seconds, 0.0, 0.0);
                }
            }
            None => *visibility = Visibility::Hidden,
        }
    }

    for (spill, mut transform, mut visibility, mut light) in &mut spill_q {
        let lit = resolved.get(spill.fixture).and_then(|o| o.as_ref()).and_then(|r| r.beam.as_ref());
        match lit {
            Some(b) => {
                *visibility = Visibility::Visible;
                // A Bevy spot light shines along its entity's -Z, which is
                // exactly this project's own beam axis convention, so the
                // aim is a plain look-along.
                transform.translation = b.origin;
                transform.rotation = Quat::from_rotation_arc(Vec3::NEG_Z, b.direction);
                let outer = b.half_angle_deg.to_radians();
                light.outer_angle = outer;
                light.inner_angle = outer * 0.8;
                light.range = b.length * 1.2;
                light.color = Color::srgb(b.color[0], b.color[1], b.color[2]);
                // Lumens. Scaled by the fixture's own resolved brightness,
                // which is already baked into `color`, so this is only the
                // headroom that makes a lit surface read at all against an
                // otherwise black room.
                light.intensity = 60_000.0;
            }
            None => {
                *visibility = Visibility::Hidden;
                light.intensity = 0.0;
            }
        }
    }
}

#[derive(Clone)]
struct Resolved {
    position: Vec3,
    body_rot: Quat,
    head_tilt: Option<Quat>,
    head_pivot: Option<Vec3>,
    beam: Option<BeamVisual>,
}

/// Applies the ambient dial to Bevy's global ambient light.
pub fn apply_ambient(settings: Res<VizSettings>, mut ambient: ResMut<GlobalAmbientLight>) {
    if settings.is_changed() {
        ambient.brightness = settings.ambient * 200.0;
    }
}
