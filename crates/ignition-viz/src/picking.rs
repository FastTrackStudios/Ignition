//! Picking fixtures in the viewport: click to select, hover to see.
//!
//! Built on `bevy_picking`'s mesh backend rather than a raycast of our
//! own. Every entity `spawn.rs` marks with [`Fixture`] gets a
//! [`Pickable`] and three observers — click, over, out — attached by a
//! system that runs after the spawn, so this module never touches the
//! spawn itself. The fixture's body meshes are children of that root and
//! pointer events bubble up to it, which is what makes one observer per
//! fixture enough however many meshes its profile came with.
//!
//! The viewport is not always a window. In the studio the visualizer is
//! a texture inside a Blitz widget, and Bevy sees no window, no winit and
//! no mouse. The host feeds pointer samples into [`HostPointer`] and two
//! systems turn them into what `bevy_picking` wants: a custom
//! `PointerId` fed `PointerInput` messages, and — because the stock ray
//! builder only knows how to look up a *window's* camera — a ray per
//! camera whose target is the pointer's texture, added to the `RayMap`
//! the way its docs say a render-to-texture setup should.
//!
//! What a click means is engine business. This module only writes a
//! [`SelectionRequest`]; who applies it depends on the [`SelectionRoute`]
//! — the standalone binary applies it to the `Playback` in the world,
//! and the studio takes it and sends it through the same `Command::Select`
//! every other surface uses, so the programmer's header, the remote and
//! the viewport all agree.

// r[impl studio.program.pick-and-gizmos] - click a fixture to select it, hover to see it

use crate::playback::Playback;
use crate::spawn::{Fixture, FixtureBody, VenueRes, update_fixture_bodies};
use bevy::camera::RenderTarget;
use bevy::picking::backend::ray::{RayId, RayMap};
use bevy::picking::mesh_picking::MeshPickingPlugin;
use bevy::picking::pointer::{
    Location, PointerAction, PointerButton, PointerId, PointerInput, PointerLocation,
};
use bevy::picking::{Pickable, PickingSystems};
use bevy::prelude::*;
use ignition_core::Selection;
use ignition_proto::ChanId;
use uuid::Uuid;

/// The one pointer a host feeds. A fixed id, so the pointer entity is
/// spawned once and the messages always find it.
const HOST_POINTER: Uuid = Uuid::from_u128(0x1971_0ac7_5e1e_c710_4f1c_7e2e_5b0e_a11d);

/// The host's pointer, in texture pixels of the camera's render target.
/// `None` when the pointer is not over the viewport.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq)]
pub struct HostPointer {
    pub position: Option<Vec2>,
    /// Primary button down.
    pub primary: bool,
    /// The modifiers as of the last sample — `Pointer<Click>` carries
    /// none, so the observer reads them from here.
    pub shift: bool,
    pub ctrl: bool,
}

/// Which fixture the pointer is over, for the surface.
#[derive(Resource, Clone, Debug, Default, PartialEq, Eq)]
pub struct Hovered(pub Option<HoveredFixture>);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HoveredFixture {
    /// Index into the venue's fixtures.
    pub index: usize,
    pub name: String,
    /// `universe.start` from the patch, or empty for an unpatched one.
    pub address: String,
}

/// The programmer's selection, as fixture indices — resolved from the
/// `Playback` once per frame so the observers and the gizmos can ask
/// "is this one selected" without resolving a `Selection` themselves.
#[derive(Resource, Clone, Debug, Default, PartialEq, Eq)]
pub struct SelectedFixtures(pub Vec<usize>);

impl SelectedFixtures {
    #[must_use]
    pub fn contains(&self, index: usize) -> bool {
        self.0.contains(&index)
    }
}

/// A selection the viewport wants made: the channels to select, or an
/// empty list to deselect. Written by the click observer, taken by
/// whoever the [`SelectionRoute`] says.
#[derive(Resource, Clone, Debug, Default, PartialEq, Eq)]
pub struct SelectionRequest(pub Option<Vec<ChanId>>);

impl SelectionRequest {
    pub const fn take(&mut self) -> Option<Vec<ChanId>> {
        self.0.take()
    }
}

/// Who turns a [`SelectionRequest`] into a programmer selection.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SelectionRoute {
    /// The viz applies it to the `Playback` in its own world — the
    /// standalone binary.
    #[default]
    InWorld,
    /// A host takes it and sends `Command::Select` — the studio.
    Host,
}

/// Marks a fixture root whose observers are attached.
#[derive(Component)]
pub struct PickTarget;

/// Picking for the rig: `MeshPickingPlugin` plus the fixture observers,
/// the host pointer and the selection plumbing.
pub struct FixturePickingPlugin;

impl Plugin for FixturePickingPlugin {
    fn build(&self, app: &mut App) {
        // A pointer event climbs to the fixture root through
        // `PointerTraversal`, whose query asks for `Option<&Window>`; a
        // world that has never registered `Window` fails that query and
        // the climb stops at the mesh. Any windowed app has it; the
        // embedded one is not left to chance.
        app.world_mut().register_component::<Window>();
        app.add_plugins(MeshPickingPlugin)
            .init_resource::<HostPointer>()
            .init_resource::<Hovered>()
            .init_resource::<SelectedFixtures>()
            .init_resource::<SelectionRequest>()
            .init_resource::<SelectionRoute>()
            .add_systems(Startup, spawn_host_pointer)
            .add_systems(First, feed_host_pointer.in_set(PickingSystems::Input))
            .add_systems(
                PreUpdate,
                host_pointer_rays
                    .in_set(PickingSystems::ProcessInput)
                    .after(RayMap::repopulate),
            )
            .add_systems(
                Update,
                (
                    attach_pickables,
                    sync_selection,
                    apply_selection_request.after(sync_selection),
                ),
            )
            .add_systems(Update, tint_bodies.after(update_fixture_bodies));
    }
}

/// The pointer entity `bevy_picking` routes the host's samples to.
/// `PointerId` requires its location and press state, so this is the
/// whole spawn.
fn spawn_host_pointer(mut commands: Commands) {
    commands.spawn((PointerId::Custom(HOST_POINTER), Name::new("host pointer")));
}

/// The camera a pointer picks through: the scene camera, not the haze
/// pass's own.
type SceneCamera = (
    With<Camera3d>,
    With<crate::camera::MainCamera>,
    Without<crate::haze::HazeCamera>,
);

/// Turns the host's latest sample into `PointerInput` messages — a move
/// when the position changed, a press or release when the button did.
#[expect(
    clippy::needless_pass_by_value,
    reason = "Res<T> is a Bevy SystemParam and must be taken by value for this to run \
              as a system; the fn only reads/copies what it wraps"
)]
#[expect(
    clippy::arithmetic_side_effects,
    reason = "Vec2 subtraction is float, component-wise, and cannot panic or overflow"
)]
fn feed_host_pointer(
    pointer: Res<HostPointer>,
    mut last: Local<HostPointer>,
    cameras: Query<&RenderTarget, SceneCamera>,
    mut out: MessageWriter<PointerInput>,
) {
    if *pointer == *last {
        return;
    }
    let Some(target) = cameras
        .iter()
        .next()
        .and_then(|target| target.normalize(None))
    else {
        return;
    };
    let id = PointerId::Custom(HOST_POINTER);
    let location = |position: Vec2| Location {
        target: target.clone(),
        position,
    };
    if let Some(position) = pointer.position
        && last.position != Some(position)
    {
        let delta = last.position.map_or(Vec2::ZERO, |p| position - p);
        out.write(PointerInput::new(
            id,
            location(position),
            PointerAction::Move { delta },
        ));
    }
    if pointer.primary != last.primary
        && let Some(position) = pointer.position.or(last.position)
    {
        let action = if pointer.primary {
            PointerAction::Press(PointerButton::Primary)
        } else {
            PointerAction::Release(PointerButton::Primary)
        };
        out.write(PointerInput::new(id, location(position), action));
    }
    *last = *pointer;
}

/// A ray per scene camera rendering to the pointer's texture.
///
/// `RayMap::repopulate` asks every pointer whether it is inside a
/// camera's viewport, and answers "no" for everything when there is no
/// primary window — which, embedded, there never is. Its own docs point
/// at the fix: add the ray yourself. `viewport_to_world` does the
/// projection, so this is a lookup and an insert, not a raycast.
fn host_pointer_rays(
    pointers: Query<(&PointerId, &PointerLocation)>,
    cameras: Query<(Entity, &Camera, &RenderTarget, &GlobalTransform), SceneCamera>,
    mut rays: ResMut<RayMap>,
) {
    for (id, location) in &pointers {
        if !id.is_custom() {
            continue;
        }
        let Some(location) = &location.location else {
            continue;
        };
        for (entity, camera, target, transform) in &cameras {
            if target.normalize(None).as_ref() != Some(&location.target) {
                continue;
            }
            if let Ok(ray) = camera.viewport_to_world(transform, location.position) {
                rays.map.insert(RayId::new(entity, *id), ray);
            }
        }
    }
}

/// Fixtures no one has seen yet — `spawn.rs` is the only spawner, so this
/// runs once per fixture on its first frame.
fn attach_pickables(
    mut commands: Commands,
    fresh: Query<Entity, (With<Fixture>, Without<PickTarget>)>,
) {
    for root in &fresh {
        commands
            .entity(root)
            .insert((PickTarget, Pickable::default()))
            .observe(
                move |_: On<Pointer<Over>>,
                      mut hovered: ResMut<Hovered>,
                      venue: Res<VenueRes>,
                      fixtures: Query<&Fixture>| {
                    if let Ok(fixture) = fixtures.get(root) {
                        hovered.0 = hovered_of(&venue.0, fixture.index);
                    }
                },
            )
            .observe(
                move |_: On<Pointer<Out>>,
                      mut hovered: ResMut<Hovered>,
                      fixtures: Query<&Fixture>| {
                    if let Ok(fixture) = fixtures.get(root)
                        && hovered.0.as_ref().is_some_and(|h| h.index == fixture.index)
                    {
                        hovered.0 = None;
                    }
                },
            )
            .observe(
                move |click: On<Pointer<Click>>,
                      pointer: Res<HostPointer>,
                      keys: Option<Res<ButtonInput<KeyCode>>>,
                      selected: Res<SelectedFixtures>,
                      venue: Res<VenueRes>,
                      mut request: ResMut<SelectionRequest>,
                      fixtures: Query<&Fixture>| {
                    if click.button != PointerButton::Primary {
                        return;
                    }
                    let Ok(fixture) = fixtures.get(root) else {
                        return;
                    };
                    let (shift, ctrl) = modifiers(&pointer, keys.as_deref());
                    let next = next_selection(&selected.0, fixture.index, shift, ctrl);
                    request.0 = Some(chans_of(&venue.0, &next));
                },
            );
    }
}

/// Shift and ctrl from wherever this viz gets its keys: the host's
/// sample when embedded, the window's keyboard otherwise.
fn modifiers(pointer: &HostPointer, keys: Option<&ButtonInput<KeyCode>>) -> (bool, bool) {
    let from_keys = |keys: &ButtonInput<KeyCode>, a, b| keys.pressed(a) || keys.pressed(b);
    let shift = pointer.shift
        || keys.is_some_and(|k| from_keys(k, KeyCode::ShiftLeft, KeyCode::ShiftRight));
    let ctrl = pointer.ctrl
        || keys.is_some_and(|k| from_keys(k, KeyCode::ControlLeft, KeyCode::ControlRight));
    (shift, ctrl)
}

/// What a click on `hit` makes of `current`: plain replaces, shift adds,
/// ctrl toggles. Ctrl on the only selected fixture leaves nothing
/// selected, which is what a request with no channels means.
#[must_use]
pub fn next_selection(current: &[usize], hit: usize, shift: bool, ctrl: bool) -> Vec<usize> {
    let mut next: Vec<usize> = if shift || ctrl {
        current.to_vec()
    } else {
        Vec::new()
    };
    if ctrl && let Some(at) = next.iter().position(|&i| i == hit) {
        next.remove(at);
    } else if !next.contains(&hit) {
        next.push(hit);
    }
    next
}

fn hovered_of(venue: &crate::venue::Venue, index: usize) -> Option<HoveredFixture> {
    let f = venue.fixtures.get(index)?;
    Some(HoveredFixture {
        index,
        name: f.name.clone(),
        address: address_of(f),
    })
}

/// `universe.start`, the way a patch sheet writes it.
#[must_use]
pub fn address_of(f: &crate::venue::FixtureRecord) -> String {
    f.dmx_address().map_or_else(String::new, |a| {
        format!("{}.{}", a.universe, a.start_channel)
    })
}

/// The patched channels of these fixtures, in the order given.
fn chans_of(venue: &crate::venue::Venue, indices: &[usize]) -> Vec<ChanId> {
    indices
        .iter()
        .filter_map(|&i| venue.fixtures.get(i).and_then(|f| f.chan))
        .collect()
}

/// The programmer's selection, back as fixture indices.
#[expect(
    clippy::needless_pass_by_value,
    reason = "Res<T>/Option<Res<T>>/ResMut<T> are Bevy SystemParams and must be taken \
              by value for this to run as a system; the fn only borrows what they wrap"
)]
fn sync_selection(
    playback: Option<Res<Playback>>,
    venue: Res<VenueRes>,
    mut selected: ResMut<SelectedFixtures>,
) {
    // Binding `playback` and `selection` from the same `and_then` chain,
    // rather than re-deriving `selection` from `playback` a second time,
    // so there is no second `Option` to unwrap.
    let next: Vec<usize> = playback
        .as_ref()
        .and_then(|p| p.programmer.selection.as_ref().map(|s| (p, s)))
        .map(|(playback, selection)| {
            let chans =
                ignition_core::selection::resolve(selection, &playback.groups, &playback.rig);
            venue
                .0
                .fixtures
                .iter()
                .enumerate()
                .filter(|(_, f)| f.chan.is_some_and(|c| chans.contains(&c)))
                .map(|(i, _)| i)
                .collect()
        })
        .unwrap_or_default();
    if selected.0 != next {
        selected.0 = next;
    }
}

/// The in-world route: a request becomes the programmer's selection.
#[expect(
    clippy::needless_pass_by_value,
    reason = "Res<T>/ResMut<T>/Option<ResMut<T>> are Bevy SystemParams and must be \
              taken by value for this to run as a system; the fn only borrows what \
              they wrap"
)]
fn apply_selection_request(
    route: Res<SelectionRoute>,
    mut request: ResMut<SelectionRequest>,
    playback: Option<ResMut<Playback>>,
) {
    if *route != SelectionRoute::InWorld {
        return;
    }
    let Some(chans) = request.take() else { return };
    let Some(mut playback) = playback else { return };
    apply_chans(&mut playback.programmer, chans);
}

/// A channel list as the programmer takes it; none is a deselect.
pub fn apply_chans(programmer: &mut ignition_core::Programmer, chans: Vec<ChanId>) {
    if chans.is_empty() {
        programmer.deselect();
    } else {
        programmer.select(Selection::Chans(chans));
    }
}

/// What a hovered body adds to its emissive, and a selected one — on top of
/// whatever `update_fixture_bodies` decided this frame, which is why this
/// runs after it.
///
/// Warm for the hover, cool for the selection, hot enough for bloom to
/// notice in the studio's dark room.
pub const HOVER_TINT: LinearRgba = LinearRgba::rgb(1.2, 0.9, 0.25);
pub const SELECT_TINT: LinearRgba = LinearRgba::rgb(0.2, 0.7, 1.4);

#[expect(
    clippy::needless_pass_by_value,
    reason = "Res<T>/ResMut<T> are Bevy SystemParams and must be taken by value for \
              this to run as a system; the fn only borrows what they wrap"
)]
#[expect(
    clippy::arithmetic_side_effects,
    reason = "LinearRgba += is float, component-wise, and cannot panic or overflow"
)]
fn tint_bodies(
    hovered: Res<Hovered>,
    selected: Res<SelectedFixtures>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    bodies: Query<(&Fixture, &FixtureBody)>,
) {
    let hovered = hovered.0.as_ref().map(|h| h.index);
    for (fixture, body) in &bodies {
        let is_hovered = hovered == Some(fixture.index);
        let is_selected = selected.contains(fixture.index);
        if !is_hovered && !is_selected {
            continue;
        }
        let tint = if is_hovered { HOVER_TINT } else { SELECT_TINT };
        // The body material and every GLB part the fixture owns — a par
        // is its file's meshes, and they only tint through their
        // per-fixture clones (`FixtureBody::parts`).
        for handle in body.tintable() {
            if let Some(mut material) = materials.get_mut(handle) {
                material.emissive += tint;
            }
        }
    }
}

/// `1-8,12` → the channels it names, for `--select`.
///
/// # Errors
///
/// If `text` isn't a comma-separated list of channel numbers and/or
/// `lo-hi` ranges.
pub fn parse_chan_ranges(text: &str) -> anyhow::Result<Vec<ChanId>> {
    let mut out = Vec::new();
    for part in text.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        match part.split_once('-') {
            Some((a, b)) => {
                let (a, b): (ChanId, ChanId) = (a.trim().parse()?, b.trim().parse()?);
                anyhow::ensure!(a <= b, "--select range {part} runs backwards");
                out.extend(a..=b);
            }
            None => out.push(part.parse()?),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::MinimalPlugins;
    use bevy::ecs::observer::Observer;

    /// r[verify studio.program.pick-and-gizmos] - every fixture root gets a pickable and its observers
    #[test]
    fn every_fixture_becomes_pickable_with_three_observers() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .init_resource::<Hovered>()
            .init_resource::<SelectedFixtures>()
            .init_resource::<SelectionRequest>()
            .init_resource::<HostPointer>()
            .add_systems(Update, attach_pickables);
        for i in 0..5 {
            app.world_mut().spawn(Fixture {
                index: i,
                base_rot: Quat::IDENTITY,
            });
        }
        app.world_mut().spawn(Name::new("not a fixture"));
        app.update();
        app.update();
        let world = app.world_mut();
        let pickable = world
            .query_filtered::<Entity, (With<Fixture>, With<Pickable>, With<PickTarget>)>()
            .iter(world)
            .count();
        assert_eq!(pickable, 5, "every fixture root is pickable");
        let observers = world.query::<&Observer>().iter(world).count();
        assert_eq!(observers, 15, "over, out and click on each");
        let stray = world
            .query_filtered::<Entity, (Without<Fixture>, With<Pickable>)>()
            .iter(world)
            .count();
        assert_eq!(stray, 0, "nothing but fixtures is touched");
    }

    /// A click on a mesh two levels under the root — where a GLB scene's
    /// meshes sit — reaches the root's observer, so a par whose meshes
    /// are the loader's picks the same as a mover whose meshes are ours.
    /// r[verify studio.program.pick-and-gizmos] - clicks bubble from scene meshes to the fixture root
    #[test]
    fn a_click_on_a_nested_scene_mesh_bubbles_to_the_fixture() {
        use bevy::camera::{ImageRenderTarget, NormalizedRenderTarget};
        use bevy::picking::backend::HitData;

        #[derive(Resource, Default)]
        struct Clicked(Vec<usize>);

        let mut app = App::new();
        app.add_plugins(MinimalPlugins).init_resource::<Clicked>();
        // What `FixturePickingPlugin::build` does for the app: without
        // it the traversal query fails and the climb stops at the leaf.
        app.world_mut().register_component::<Window>();
        let mut roots = Vec::new();
        for (index, depth) in [(0usize, 1usize), (1, 3)] {
            let root = app
                .world_mut()
                .spawn((
                    Fixture {
                        index,
                        base_rot: Quat::IDENTITY,
                    },
                    Pickable::default(),
                ))
                .observe(move |_: On<Pointer<Click>>, mut clicked: ResMut<Clicked>| {
                    clicked.0.push(index);
                })
                .id();
            // A primitive part is one level down; a GLB scene's meshes
            // are three: root -> node -> gltf -> mesh.
            let mut leaf = root;
            for _ in 0..depth {
                leaf = app.world_mut().spawn(ChildOf(leaf)).id();
            }
            roots.push(leaf);
        }
        let location = Location {
            target: NormalizedRenderTarget::Image(ImageRenderTarget {
                handle: Handle::default(),
                scale_factor: 1.0,
            }),
            position: Vec2::ZERO,
        };
        for &leaf in &roots {
            let click = Click {
                button: PointerButton::Primary,
                hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
                duration: std::time::Duration::ZERO,
                count: 1,
            };
            app.world_mut().trigger(Pointer::new(
                PointerId::Custom(HOST_POINTER),
                location.clone(),
                click,
                leaf,
            ));
        }
        assert_eq!(
            app.world().resource::<Clicked>().0,
            vec![0, 1],
            "both the primitive and the GLB-depth mesh reach their root"
        );
    }

    /// r[verify studio.program.pick-and-gizmos] - plain, shift and ctrl clicks
    #[test]
    fn click_modifiers_replace_add_and_toggle() {
        assert_eq!(
            next_selection(&[1, 2], 7, false, false),
            vec![7],
            "plain replaces"
        );
        assert_eq!(
            next_selection(&[1, 2], 7, true, false),
            vec![1, 2, 7],
            "shift adds"
        );
        assert_eq!(
            next_selection(&[1, 2], 2, true, false),
            vec![1, 2],
            "shift never doubles"
        );
        assert_eq!(
            next_selection(&[1, 2], 7, false, true),
            vec![1, 2, 7],
            "ctrl adds a new one"
        );
        assert_eq!(
            next_selection(&[1, 2], 1, false, true),
            vec![2],
            "ctrl removes a selected one"
        );
        assert!(
            next_selection(&[1], 1, false, true).is_empty(),
            "ctrl on the last clears"
        );
    }

    #[test]
    fn a_request_reaches_the_programmer_as_channels() {
        let mut programmer = ignition_core::Programmer::default();
        apply_chans(&mut programmer, vec![3, 4]);
        assert_eq!(programmer.selection, Some(Selection::Chans(vec![3, 4])));
        apply_chans(&mut programmer, vec![]);
        assert_eq!(programmer.selection, None);
    }

    #[test]
    fn select_ranges_parse() {
        assert_eq!(parse_chan_ranges("1-3, 7").unwrap(), vec![1, 2, 3, 7]);
        assert!(parse_chan_ranges("5-2").is_err());
        assert!(parse_chan_ranges("x").is_err());
    }
}
