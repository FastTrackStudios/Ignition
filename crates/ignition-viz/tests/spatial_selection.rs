//! Spatial selection against the real Norco rig.
//!
//! The unit tests in `ignition_core::selection` use a four-fixture
//! fiction; these run the same expressions over 71 real fixtures with
//! their real surveyed positions, which is the only way to catch a unit
//! or axis-convention mistake that a hand-built fixture list would agree
//! with.

use ignition_core::{Axis, Cmp, Dir, Order, Selection, Where};
use ignition_viz::Venue;

fn venue() -> Venue {
    Venue::load(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../data/venues/norco"
    ))
    .expect("the Norco venue extract")
}

/// The whole point of Decision 4: a chase that runs left to right
/// because the room says so, not because someone hand-listed channels in
/// the right order.
/// r[verify profile.spatial-grid-is-derived] - the order comes from surveyed positions, not a hand-built list
/// r[verify groups.derived]
#[test]
fn ordering_the_ceiling_washers_by_x_is_monotonic_in_real_space() {
    let venue = venue();
    let (groups, rig) = (venue.groups(), venue.rig());

    let ceiling = Selection::Where {
        of: Box::new(Selection::Tag("Luminaire_LED_Wash".into())),
        filter: Where::Half {
            axis: Axis::Z,
            cmp: Cmp::Gt,
            at: 2.0,
        },
    };
    let left_to_right = Selection::Order {
        of: Box::new(ceiling.clone()),
        by: Order::Axis(Axis::X, Dir::Asc),
    };

    let chans = ignition_core::selection::resolve(&left_to_right, &groups, &rig);
    assert!(
        chans.len() > 40,
        "expected the ceiling rig, got {}",
        chans.len()
    );

    let xs: Vec<f64> = chans
        .iter()
        .map(|c| {
            rig.placement(*c)
                .expect("a ceiling fixture is placed")
                .position
                .x
        })
        .collect();
    assert!(
        xs.windows(2).all(|w| w[0] <= w[1]),
        "not sorted left to right: {xs:?}"
    );

    // ...and it is a real reordering, not the patch order by luck.
    let unsorted = ignition_core::selection::resolve(&ceiling, &groups, &rig);
    assert_ne!(chans, unsorted, "the sort did nothing — check the axis");

    // Descending is *not* the exact reverse of ascending, and should not
    // be: this rig is symmetric, so many fixtures share an X, and a
    // stable sort keeps tied fixtures in their original relative order
    // whichever way it runs. Fixtures at the same X land at the same
    // phase anyway, so the distinction is invisible on stage — but
    // asserting `reverse()` here would be asserting a bug.
    let reversed = ignition_core::selection::resolve(
        &Selection::Order {
            of: Box::new(ceiling),
            by: Order::Axis(Axis::X, Dir::Desc),
        },
        &groups,
        &rig,
    );
    let xs: Vec<f64> = reversed
        .iter()
        .map(|c| rig.placement(*c).unwrap().position.x)
        .collect();
    assert!(
        xs.windows(2).all(|w| w[0] >= w[1]),
        "not sorted right to left: {xs:?}"
    );
}

/// A height filter has to separate the truss from the floor package, or
/// every "ceiling" recipe quietly includes the floor movers.
/// r[verify groups.derived]
/// r[verify files.capability-over-name] - "above 2 m" without naming a group
#[test]
fn a_height_filter_separates_the_truss_from_the_floor_package() {
    let venue = venue();
    let (groups, rig) = (venue.groups(), venue.rig());
    let all = Selection::Chans(rig.fixtures().iter().map(|f| f.chan).collect());

    let high = ignition_core::selection::resolve(
        &Selection::Where {
            of: Box::new(all.clone()),
            filter: Where::Half {
                axis: Axis::Z,
                cmp: Cmp::Gt,
                at: 2.0,
            },
        },
        &groups,
        &rig,
    );
    let low = ignition_core::selection::resolve(
        &Selection::Where {
            of: Box::new(all),
            filter: Where::Half {
                axis: Axis::Z,
                cmp: Cmp::Le,
                at: 2.0,
            },
        },
        &groups,
        &rig,
    );

    assert!(
        !high.is_empty() && !low.is_empty(),
        "{} / {}",
        high.len(),
        low.len()
    );
    assert!(high.iter().all(|c| !low.contains(c)), "the halves overlap");
    for chan in &low {
        let z = rig.placement(*chan).unwrap().position.z;
        assert!(z <= 2.0, "chan {chan} at z={z} is not floor-level");
    }
}

/// Model matching has to find the movers without anyone naming a group,
/// which is what makes a recipe survive being carried to another rig.
/// r[verify files.capability-over-name]
#[test]
fn a_model_selector_finds_the_real_moving_heads() {
    let venue = venue();
    let (groups, rig) = (venue.groups(), venue.rig());
    let movers =
        ignition_core::selection::resolve(&Selection::Model("Moving Head".into()), &groups, &rig);
    assert_eq!(movers.len(), 9, "5 Betopper beams + 4 Riukoe: {movers:?}");
}
