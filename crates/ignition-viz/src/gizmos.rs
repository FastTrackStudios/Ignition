//! The programmer's overlays: what the Program view draws over the room
//! that the Live view does not.
//!
//! Four of them, each its own `GizmoConfigGroup` so it can be switched
//! off without touching the others: the venue's focus points, the
//! selected fixtures' beam axes, the outline of a group the operator is
//! hovering in the Library, and the DMX address over every fixture. All
//! four hang off one [`ProgramOverlays`] resource, and all four go dark
//! together when `program` is off — Live is an operator's view of the
//! stage, not a programmer's view of the rig.
//!
//! Lines and spheres are `bevy_gizmos`, immediate-mode, redrawn every
//! frame from the venue and the selection. The labels are the one thing
//! gizmos cannot draw: they are `bevy_ui` text nodes, one per fixture
//! and one per focus point, moved every frame to where the camera says
//! their point projects — the same mechanism the operator overlay uses,
//! so they land on the studio's texture and the snapshot's image alike.

// r[impl studio.program.pick-and-gizmos] - focus points, beams, groups and labels, each switchable

use crate::picking::{SelectedFixtures, address_of};
use crate::spawn::{BeamEmitter, Fixture, VenueRes};
use bevy::gizmos::AppGizmoBuilder;
use bevy::gizmos::prelude::{GizmoConfigGroup, GizmoConfigStore};
use bevy::prelude::*;

/// Which overlays draw. `program` is the master: the Program view sets
/// it, the Live view clears it, and the others only count while it is on.
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "each flag is an independent overlay toggle, set from a different UI \
              control (the Program/Live switch, the Library's group hover, a \
              per-overlay checkbox) — folding them into an enum would force those \
              independent controls to agree on a shared shape they don't share"
)]
pub struct ProgramOverlays {
    pub program: bool,
    pub focus: bool,
    pub beams: bool,
    pub groups: bool,
    pub labels: bool,
}

impl Default for ProgramOverlays {
    fn default() -> Self {
        Self {
            program: true,
            focus: true,
            beams: true,
            groups: true,
            labels: false,
        }
    }
}

impl ProgramOverlays {
    /// Whether each gizmo group draws: `[focus, beams, groups]`.
    #[must_use]
    pub const fn enabled(&self) -> [bool; 3] {
        [
            self.program && self.focus,
            self.program && self.beams,
            self.program && self.groups,
        ]
    }

    #[must_use]
    pub const fn labels_on(&self) -> bool {
        self.program && self.labels
    }
}

/// The group whose fixtures are outlined — what the Library sets while a
/// group tile is under the pointer.
#[derive(Resource, Clone, Debug, Default, PartialEq, Eq)]
pub struct HighlightGroup(pub Option<String>);

#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct FocusGizmos;

#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct BeamGizmos;

#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct GroupGizmos;

/// A label over a fixture, by venue index.
#[derive(Component)]
pub struct FixtureLabel(pub usize);

/// A label beside a focus point, by palette index.
#[derive(Component)]
pub struct FocusLabel(pub usize);

pub const FOCUS_COLOR: Color = Color::srgb(1.0, 0.55, 0.15);
pub const BEAM_COLOR: Color = Color::srgb(0.3, 0.8, 1.0);
pub const GROUP_COLOR: Color = Color::srgb(0.6, 1.0, 0.4);

/// How far a beam axis is drawn when it never meets the floor.
const BEAM_REACH: f32 = 25.0;
const FOCUS_RADIUS: f32 = 0.15;
/// Air around a group's fixtures before the box is drawn.
const GROUP_PAD: f32 = 0.3;
/// Each pair is one edge of a box face, wound around it; `(3, 0)` closes
/// the loop back to the start without a modulo on the loop counter.
const BOX_EDGES: [(usize, usize); 4] = [(0, 1), (1, 2), (2, 3), (3, 0)];

pub struct ProgramGizmosPlugin;

impl Plugin for ProgramGizmosPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ProgramOverlays>()
            .init_resource::<HighlightGroup>()
            .init_gizmo_group::<FocusGizmos>()
            .init_gizmo_group::<BeamGizmos>()
            .init_gizmo_group::<GroupGizmos>()
            .add_systems(Startup, spawn_focus_labels)
            .add_systems(
                Update,
                (
                    apply_overlay_config.run_if(resource_changed::<ProgramOverlays>),
                    draw_focus_points,
                    draw_beam_axes,
                    draw_group_outline,
                    spawn_fixture_labels,
                ),
            )
            .add_systems(
                PostUpdate,
                place_labels.after(bevy::transform::TransformSystems::Propagate),
            );
    }
}

/// Pushes the resource into the three gizmo groups' `enabled` flags.
pub fn apply_overlays(overlays: &ProgramOverlays, store: &mut GizmoConfigStore) {
    let [focus, beams, groups] = overlays.enabled();
    store.config_mut::<FocusGizmos>().0.enabled = focus;
    store.config_mut::<BeamGizmos>().0.enabled = beams;
    store.config_mut::<GroupGizmos>().0.enabled = groups;
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "Res<T> is a Bevy SystemParam and must be taken by value for this to run \
              as a system; the fn only borrows the resource it wraps"
)]
fn apply_overlay_config(overlays: Res<ProgramOverlays>, mut store: ResMut<GizmoConfigStore>) {
    apply_overlays(&overlays, &mut store);
}

const fn point(v: &ignition_proto::Vec3) -> Vec3 {
    Vec3::new(
        crate::num::f32_of_f64(v.x),
        crate::num::f32_of_f64(v.y),
        crate::num::f32_of_f64(v.z),
    )
}

/// A small sphere on every focus point of the venue's palette.
#[expect(
    clippy::needless_pass_by_value,
    reason = "Res<T> is a Bevy SystemParam and must be taken by value for this to run \
              as a system; the fn only borrows the resource it wraps"
)]
#[expect(
    clippy::arithmetic_side_effects,
    reason = "Vec3 arithmetic over f32 components — floating-point ops cannot \
              overflow or panic; clippy just doesn't special-case bevy_math's \
              operator overloads the way it does bare f32/f64"
)]
fn draw_focus_points(mut gizmos: Gizmos<FocusGizmos>, venue: Res<VenueRes>) {
    for preset in &venue.0.palettes.focus {
        let at = point(&preset.target);
        gizmos.sphere(Isometry3d::from_translation(at), FOCUS_RADIUS, FOCUS_COLOR);
        // A cross on the floor under it, so a point in the air still
        // reads as a place on the stage.
        let floor = Vec3::new(at.x, 0.0, at.z);
        gizmos.line(
            floor - Vec3::X * FOCUS_RADIUS,
            floor + Vec3::X * FOCUS_RADIUS,
            FOCUS_COLOR,
        );
        gizmos.line(
            floor - Vec3::Z * FOCUS_RADIUS,
            floor + Vec3::Z * FOCUS_RADIUS,
            FOCUS_COLOR,
        );
    }
}

/// Where a beam from `origin` along `dir` lands: the floor if it is
/// heading down, else `BEAM_REACH` out.
#[must_use]
#[expect(
    clippy::arithmetic_side_effects,
    reason = "Vec3/f32 arithmetic — floating-point ops cannot overflow or panic; \
              clippy just doesn't special-case bevy_math's operator overloads the \
              way it does bare f32/f64"
)]
pub fn beam_end(origin: Vec3, dir: Vec3) -> Vec3 {
    if dir.y < -1e-4 {
        let t = -origin.y / dir.y;
        if t > 0.0 && t < BEAM_REACH {
            return origin + dir * t;
        }
    }
    origin + dir * BEAM_REACH
}

/// The optical axis of every selected fixture, from the lens to where
/// it lands, with a ring where it lands.
#[expect(
    clippy::needless_pass_by_value,
    reason = "Res<T> is a Bevy SystemParam and must be taken by value for this to run \
              as a system; the fn only borrows the resource it wraps"
)]
fn draw_beam_axes(
    mut gizmos: Gizmos<BeamGizmos>,
    selected: Res<SelectedFixtures>,
    emitters: Query<(&BeamEmitter, &GlobalTransform)>,
) {
    if selected.0.is_empty() {
        return;
    }
    for (emitter, transform) in &emitters {
        if !selected.contains(emitter.fixture) {
            continue;
        }
        let origin = transform.translation();
        let dir = transform.forward().as_vec3();
        let end = beam_end(origin, dir);
        gizmos.line(origin, end, BEAM_COLOR);
        let up = if dir.y.abs() > 0.99 { Vec3::X } else { Vec3::Y };
        let rot = Quat::from_rotation_arc(Vec3::Z, dir.cross(up).cross(dir).normalize_or(Vec3::Y));
        gizmos.circle(Isometry3d::new(end, rot), 0.2, BEAM_COLOR);
    }
}

/// The axis-aligned box around a set of points, padded.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "Vec3/f32 arithmetic — floating-point ops cannot overflow or panic; \
              clippy just doesn't special-case bevy_math's operator overloads the \
              way it does bare f32/f64"
)]
pub fn bounds_of(points: impl IntoIterator<Item = Vec3>) -> Option<(Vec3, Vec3)> {
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    let mut any = false;
    for p in points {
        min = min.min(p);
        max = max.max(p);
        any = true;
    }
    any.then_some((min - Vec3::splat(GROUP_PAD), max + Vec3::splat(GROUP_PAD)))
}

fn draw_box(gizmos: &mut Gizmos<GroupGizmos>, min: Vec3, max: Vec3, color: Color) {
    let c = |x: f32, y: f32, z: f32| Vec3::new(x, y, z);
    let bottom = [
        c(min.x, min.y, min.z),
        c(max.x, min.y, min.z),
        c(max.x, min.y, max.z),
        c(min.x, min.y, max.z),
    ];
    let top = [
        c(min.x, max.y, min.z),
        c(max.x, max.y, min.z),
        c(max.x, max.y, max.z),
        c(min.x, max.y, max.z),
    ];
    for (i, j) in BOX_EDGES {
        // `bottom`/`top` are always length 4 and `EDGES` only ever names
        // indices 0..=3, so every `get` here succeeds; `get` over `[]` is
        // still the house idiom for reading out of an array by index.
        let (Some(&b_i), Some(&b_j), Some(&t_i), Some(&t_j)) =
            (bottom.get(i), bottom.get(j), top.get(i), top.get(j))
        else {
            continue;
        };
        gizmos.line(b_i, b_j, color);
        gizmos.line(t_i, t_j, color);
        gizmos.line(b_i, t_i, color);
    }
}

/// A box around the highlighted group's fixtures.
#[expect(
    clippy::needless_pass_by_value,
    reason = "Res<T> is a Bevy SystemParam and must be taken by value for this to run \
              as a system; the fn only borrows the resources it wraps"
)]
fn draw_group_outline(
    mut gizmos: Gizmos<GroupGizmos>,
    highlight: Res<HighlightGroup>,
    venue: Res<VenueRes>,
    fixtures: Query<(&Fixture, &GlobalTransform)>,
) {
    let Some(name) = highlight.0.as_deref() else {
        return;
    };
    let Some(group) = venue.0.groups().into_iter().find(|g| g.name == name) else {
        return;
    };
    let points = fixtures.iter().filter_map(|(fixture, transform)| {
        let record = venue.0.fixtures.get(fixture.index)?;
        record
            .chan
            .is_some_and(|c| group.chans.contains(&c))
            .then(|| transform.translation())
    });
    if let Some((min, max)) = bounds_of(points) {
        draw_box(&mut gizmos, min, max, GROUP_COLOR);
    }
}

fn label_bundle(text: String, color: Color) -> impl Bundle {
    (
        Text::new(text),
        TextFont {
            font_size: bevy::text::FontSize::Px(11.0),
            ..default()
        },
        TextColor(color),
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.5)),
        Node {
            position_type: PositionType::Absolute,
            padding: UiRect::axes(Val::Px(3.0), Val::Px(1.0)),
            ..default()
        },
        Visibility::Hidden,
    )
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "Res<T> is a Bevy SystemParam and must be taken by value for this to run \
              as a system; the fn only borrows the resource it wraps"
)]
fn spawn_focus_labels(mut commands: Commands, venue: Res<VenueRes>) {
    for (i, preset) in venue.0.palettes.focus.iter().enumerate() {
        commands.spawn((
            label_bundle(preset.name.clone(), FOCUS_COLOR),
            FocusLabel(i),
        ));
    }
}

/// One label per fixture, spawned the first frame the fixture exists.
#[expect(
    clippy::needless_pass_by_value,
    reason = "Res<T> is a Bevy SystemParam and must be taken by value for this to run \
              as a system; the fn only borrows the resource it wraps"
)]
fn spawn_fixture_labels(
    mut commands: Commands,
    venue: Res<VenueRes>,
    fixtures: Query<&Fixture>,
    labelled: Query<&FixtureLabel>,
) {
    let have: std::collections::HashSet<usize> = labelled.iter().map(|l| l.0).collect();
    for fixture in &fixtures {
        if have.contains(&fixture.index) {
            continue;
        }
        let Some(record) = venue.0.fixtures.get(fixture.index) else {
            continue;
        };
        let text = match (record.chan, address_of(record)) {
            (Some(chan), addr) if !addr.is_empty() => format!("{chan} @ {addr}"),
            (Some(chan), _) => format!("{chan}"),
            (None, addr) => addr,
        };
        commands.spawn((
            label_bundle(text, Color::WHITE),
            FixtureLabel(fixture.index),
        ));
    }
}

/// The camera the labels project through — the scene camera.
type SceneCamera = (
    With<Camera3d>,
    With<crate::camera::MainCamera>,
    Without<crate::haze::HazeCamera>,
);

/// Moves every label to its point's place on screen, or hides it.
#[expect(
    clippy::needless_pass_by_value,
    reason = "Res<T> is a Bevy SystemParam and must be taken by value for this to run \
              as a system; the fn only borrows the resources it wraps"
)]
#[expect(
    clippy::arithmetic_side_effects,
    reason = "Vec3/f32 arithmetic — floating-point ops cannot overflow or panic; \
              clippy just doesn't special-case bevy_math's operator overloads the \
              way it does bare f32/f64"
)]
fn place_labels(
    mut commands: Commands,
    overlays: Res<ProgramOverlays>,
    venue: Res<VenueRes>,
    camera: Query<(Entity, &Camera, &GlobalTransform), SceneCamera>,
    fixtures: Query<(&Fixture, &GlobalTransform)>,
    mut labels: Query<(
        Entity,
        Option<&FixtureLabel>,
        Option<&FocusLabel>,
        &mut Node,
        &mut Visibility,
        Has<UiTargetCamera>,
    )>,
) {
    let Ok((camera_entity, camera, camera_transform)) = camera.single() else {
        return;
    };
    let show_fixtures = overlays.labels_on();
    let show_focus = overlays.program && overlays.focus;
    for (entity, fixture_label, focus_label, mut node, mut visibility, targeted) in &mut labels {
        if !targeted {
            commands
                .entity(entity)
                .insert(UiTargetCamera(camera_entity));
        }
        let world = match (fixture_label, focus_label) {
            (Some(label), _) if show_fixtures => fixtures
                .iter()
                .find(|(f, _)| f.index == label.0)
                .map(|(_, t)| t.translation() + Vec3::Y * 0.25),
            (_, Some(label)) if show_focus => venue
                .0
                .palettes
                .focus
                .get(label.0)
                .map(|p| point(&p.target) + Vec3::Y * (FOCUS_RADIUS + 0.05)),
            _ => None,
        };
        let on_screen = world.and_then(|w| camera.world_to_viewport(camera_transform, w).ok());
        match on_screen {
            Some(at) => {
                node.left = Val::Px(at.x);
                node.top = Val::Px(at.y);
                if *visibility != Visibility::Inherited {
                    *visibility = Visibility::Inherited;
                }
            }
            None => {
                if *visibility != Visibility::Hidden {
                    *visibility = Visibility::Hidden;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::gizmos::prelude::GizmoConfig;

    /// r[verify studio.program.pick-and-gizmos] - each overlay toggles on its own, and Live turns them all off
    #[test]
    fn overlay_toggles_reach_the_gizmo_groups() {
        let mut store = GizmoConfigStore::default();
        store.insert(GizmoConfig::default(), FocusGizmos);
        store.insert(GizmoConfig::default(), BeamGizmos);
        store.insert(GizmoConfig::default(), GroupGizmos);

        let mut overlays = ProgramOverlays::default();
        apply_overlays(&overlays, &mut store);
        assert!(store.config::<FocusGizmos>().0.enabled);
        assert!(store.config::<BeamGizmos>().0.enabled);
        assert!(store.config::<GroupGizmos>().0.enabled);
        assert!(!overlays.labels_on(), "labels are off until asked for");

        overlays.beams = false;
        apply_overlays(&overlays, &mut store);
        assert!(store.config::<FocusGizmos>().0.enabled);
        assert!(
            !store.config::<BeamGizmos>().0.enabled,
            "one off, the others stay"
        );

        overlays.labels = true;
        assert!(overlays.labels_on());
        overlays.program = false;
        apply_overlays(&overlays, &mut store);
        assert!(
            !store.config::<FocusGizmos>().0.enabled,
            "Live: everything off"
        );
        assert!(!store.config::<GroupGizmos>().0.enabled);
        assert!(!overlays.labels_on());
    }

    #[test]
    fn a_beam_lands_on_the_floor_or_runs_out() {
        let down = beam_end(Vec3::new(0.0, 4.0, 0.0), Vec3::NEG_Y);
        assert!((down - Vec3::ZERO).length() < 1e-4);
        let up = beam_end(Vec3::new(0.0, 4.0, 0.0), Vec3::Y);
        assert!((up.y - (4.0 + BEAM_REACH)).abs() < 1e-4);
    }

    #[test]
    fn group_bounds_pad_the_fixtures_and_need_at_least_one() {
        assert!(bounds_of([]).is_none());
        let (min, max) = bounds_of([Vec3::ZERO, Vec3::new(2.0, 1.0, 0.0)]).unwrap();
        assert_eq!(min, Vec3::splat(-GROUP_PAD));
        assert_eq!(max, Vec3::new(2.0, 1.0, 0.0) + Vec3::splat(GROUP_PAD));
    }
}
