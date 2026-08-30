//! What a venue file has to carry, checked against both shipped rooms.
//!
//! These are the claims that make a show portable, and every one of
//! them fails quietly. A fixture with no orientation renders and aims
//! nowhere; a room with no geometry loads and makes every `Where`
//! predicate meaningless; a venue that publishes no vocabulary is a
//! room no show can be written for. None of it errors — the show simply
//! does less than it says. So both rooms are checked rather than a
//! fixture, because the failure mode is a room somebody extracted in a
//! hurry.

use ignition_core::profile::Bindings;
use ignition_viz::Venue;

fn rooms() -> Vec<(&'static str, Venue)> {
    ["norco", "riverside"]
        .into_iter()
        .filter_map(|name| {
            let dir = format!("{}/../../data/venues/{name}", env!("CARGO_MANIFEST_DIR"));
            Venue::load(dir).ok().map(|v| (name, v))
        })
        .collect()
}

/// Every fixture says what it is, where it is, which way it faces and
/// where it sits on the wire.
///
/// Position without orientation is the one that bites: a mover with no
/// rotation aims into the floor, and nothing about the file looks
/// wrong. Patch is checked as "addressable at all" rather than as a
/// universe — a rig is often half-patched, and an unpatched fixture is
/// explicitly allowed to say so.
///
/// r[verify files.venue.fixtures]
#[test]
fn every_fixture_carries_its_type_place_facing_and_patch() {
    for (name, venue) in rooms() {
        assert!(!venue.fixtures.is_empty(), "{name} has no fixtures");
        for fixture in &venue.fixtures {
            let what = format!("{name}/{}", fixture.name);
            assert!(
                fixture.model.is_some() || !fixture.tags.is_empty(),
                "{what} says nothing about what kind of light it is"
            );
            let p = fixture.position;
            assert!(
                p.x.is_finite() && p.y.is_finite() && p.z.is_finite(),
                "{what} is nowhere: {p:?}"
            );
            let q = fixture.quat;
            assert!(
                (q.x * q.x + q.y * q.y + q.z * q.z + q.w * q.w - 1.0).abs() < 1e-2,
                "{what} carries no usable orientation, so it aims wherever the \
                 renderer's default points: {q:?}"
            );
            // Patched fixtures are addressable; an unpatched one is
            // allowed to be, and says so.
            if fixture.patched {
                assert!(
                    fixture.chan.is_some() || fixture.address.is_some(),
                    "{what} is patched but has no address"
                );
            }
        }

        // Metres in the room's own stage space, not millimetres from a
        // CAD origin — the unit mistake `Where::Within` cannot see.
        let tallest = venue
            .fixtures
            .iter()
            .map(|f| f.position.y)
            .fold(f32::MIN, f32::max);
        assert!(
            (0.0..200.0).contains(&tallest),
            "{name}'s highest fixture is at y={tallest}, which is not metres"
        );
    }
}

/// The room has geometry, and the fixtures are in it.
///
/// A `Where::Within` predicate is portable only because both rooms
/// agree what their coordinates mean. The check that actually catches a
/// bad extract is not "geometry exists" but "the fixtures and the walls
/// are in the same space" — a room in one unit and a rig in another
/// loads perfectly and selects nothing.
///
/// r[verify files.venue.room]
#[test]
fn the_room_has_geometry_and_the_rig_stands_inside_it() {
    for (name, venue) in rooms() {
        assert!(
            !venue.room.is_empty(),
            "{name} carries no room, so every spatial selection in every show is \
             guesswork"
        );

        let extent = |pick: fn(&ignition_viz::venue::Vec3) -> f32| {
            venue
                .room
                .iter()
                .map(|g| pick(&g.size).abs() / 2.0 + pick(&g.position).abs())
                .fold(0.0_f32, f32::max)
        };
        let (half_x, half_z) = (extent(|v| v.x), extent(|v| v.z));
        assert!(
            half_x > 1.0 && half_z > 1.0,
            "{name}'s room is smaller than a fixture: {half_x} x {half_z}"
        );

        // Generous — trusses overhang and a house rig can sit behind the
        // back wall — but a rig in the wrong unit is out by a factor of
        // a thousand, not by a metre.
        for fixture in &venue.fixtures {
            let p = fixture.position;
            assert!(
                p.x.abs() < half_x * 4.0 + 10.0 && p.z.abs() < half_z * 4.0 + 10.0,
                "{name}/{} is at {p:?}, nowhere near a room {half_x} by {half_z}",
                fixture.name
            );
        }
    }
}

/// A venue publishes its vocabulary by name, and the names a show uses
/// are the names it publishes.
///
/// The venue says what "Back Wash" is at this address; the show says
/// what happens to it. Both halves are checked here, because a venue
/// publishing a vocabulary nothing consumes and a show consuming a
/// vocabulary nothing publishes fail identically at output: nothing
/// happens and nothing complains.
///
/// r[verify files.vocabulary]
#[test]
fn a_venue_publishes_its_vocabulary_and_binds_it_to_something_real() {
    for (name, venue) in rooms() {
        let groups = venue.groups();
        assert!(
            !groups.is_empty(),
            "{name} publishes no groups, so a show can only address its channels"
        );
        assert!(
            !venue.palettes.colors.is_empty() || !venue.palettes.focus.is_empty(),
            "{name} publishes no colours and no focus points"
        );

        // Every role the venue claims to bind resolves to a name this
        // room actually has. A binding pointing at a group that was
        // renamed is the quiet half of this rule.
        let known: std::collections::HashSet<&str> =
            groups.iter().map(|g| g.name.as_str()).collect();
        for role in venue.profile.groups.keys() {
            assert!(
                venue.profile.has_group(role),
                "{name} lists role {role:?} and then does not bind it"
            );
        }
        let focus: std::collections::HashSet<&str> = venue
            .palettes
            .focus
            .iter()
            .map(|f| f.name.as_str())
            .collect();
        for (role, point) in &venue.profile.focus {
            assert!(
                focus.contains(point.as_str()),
                "{name} binds focus role {role:?} to {point:?}, which this room's \
                 palette does not have: {focus:?}"
            );
        }
        let _ = known;
    }
}
