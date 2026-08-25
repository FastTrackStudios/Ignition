//! *Who* a recipe applies to — an expression, never a frozen list.
//!
//! This is the mechanism, not a detail: because a selection stores the
//! rule rather than the fixtures it happened to match, adding a fixture
//! to a group changes every recipe that selects that group, and
//! re-patching a rig does not invalidate a show. See
//! `docs/domain/cue-building-architecture.md`, Decision 4.
//!
//! A selection is **ordered**, and the order is load-bearing: phase
//! spread across an effect is defined by a fixture's position *in the
//! selection*. That is what `Order` is for, and it is the part no other
//! console in this class can do. Elsewhere, a left-to-right chase means
//! knowing by hand which channel numbers happen to run left to right,
//! and re-deriving that every time the rig moves. Here the rig's real
//! hung XYZ is data we already have, so "left to right" is a property of
//! the room:
//!
//! ```text
//! Order { of: Group("Washers"), by: Axis(X, Asc) }   // left to right
//! Order { of: Group("Pars"),    by: Distance { .. } } // centre-out bloom
//! ```

use crate::group::{self, Group};
use ignition_proto::{ChanId, Placement, Vec3};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// What resolution needs to know about one patched fixture.
///
/// A flat record rather than a callback into the venue loader, so
/// `ignition-core` keeps its no-I/O rule while still being able to
/// answer "which fixtures are tagged `mover`" — a question a
/// `Fn(ChanId) -> Placement` closure cannot be asked.
#[derive(Debug, Clone, PartialEq)]
pub struct FixtureInfo {
    pub chan: ChanId,
    /// `None` for a fixture with no known hung position — spatial
    /// filters and orders skip it rather than guessing at the origin.
    pub placement: Option<Placement>,
    pub manufacturer: String,
    pub model: String,
    pub tags: Vec<String>,
}

/// The patched rig, indexed for lookup.
///
/// Built once by the venue loader and borrowed by every resolve, because
/// resolution now runs every frame (Decision 1) and re-deriving this
/// per frame would spend exactly the budget that decision bought.
#[derive(Debug, Clone, Default)]
pub struct Rig {
    fixtures: Vec<FixtureInfo>,
    by_chan: HashMap<ChanId, usize>,
}

impl Rig {
    pub fn new(fixtures: Vec<FixtureInfo>) -> Self {
        let by_chan = fixtures
            .iter()
            .enumerate()
            .map(|(i, f)| (f.chan, i))
            .collect();
        Self { fixtures, by_chan }
    }

    pub fn get(&self, chan: ChanId) -> Option<&FixtureInfo> {
        self.by_chan.get(&chan).map(|i| &self.fixtures[*i])
    }

    pub fn placement(&self, chan: ChanId) -> Option<Placement> {
        self.get(chan).and_then(|f| f.placement.clone())
    }

    pub fn fixtures(&self) -> &[FixtureInfo] {
        &self.fixtures
    }
}

/// A rig with no fixtures — for tests, and for resolving a selection
/// that only names groups and channels.
pub static EMPTY_RIG: std::sync::LazyLock<Rig> = std::sync::LazyLock::new(Rig::default);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Axis {
    X,
    Y,
    Z,
}

impl Axis {
    fn of(self, v: Vec3) -> f64 {
        match self {
            Axis::X => v.x,
            Axis::Y => v.y,
            Axis::Z => v.z,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Cmp {
    Lt,
    Le,
    Gt,
    Ge,
}

impl Cmp {
    fn test(self, a: f64, b: f64) -> bool {
        match self {
            Cmp::Lt => a < b,
            Cmp::Le => a <= b,
            Cmp::Gt => a > b,
            Cmp::Ge => a >= b,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Dir {
    Asc,
    Desc,
}

/// A predicate on real position.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Where {
    /// One side of an axis-aligned plane. "Stage-left half" is
    /// `Half { axis: X, cmp: Gt, at: 0.0 }`.
    Half { axis: Axis, cmp: Cmp, at: f64 },
    /// Inside an axis-aligned box — a zone of the room.
    Within { min: Vec3, max: Vec3 },
    /// Within `radius` metres of a point.
    Near { at: Vec3, radius: f64 },
}

impl Where {
    fn test(&self, p: Vec3) -> bool {
        match self {
            Where::Half { axis, cmp, at } => cmp.test(axis.of(p), *at),
            Where::Within { min, max } => {
                p.x >= min.x
                    && p.x <= max.x
                    && p.y >= min.y
                    && p.y <= max.y
                    && p.z >= min.z
                    && p.z <= max.z
            }
            Where::Near { at, radius } => {
                let (dx, dy, dz) = (p.x - at.x, p.y - at.y, p.z - at.z);
                (dx * dx + dy * dy + dz * dz) <= radius * radius
            }
        }
    }
}

/// How to order a selection. The half that makes an effect spatial
/// rather than an accident of patch order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Order {
    /// Sorted along an axis. `Axis(X, Asc)` is a left-to-right chase;
    /// `Axis(Y, Desc)` sweeps downstage to upstage.
    Axis(Axis, Dir),
    /// Sorted by distance from a point — a centre-out bloom, or a ripple
    /// away from wherever the singer is standing.
    Distance {
        from: Vec3,
        dir: Dir,
    },
    Reverse,
}

/// Who a recipe applies to.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Selection {
    /// Looked up by name against the venue's real groups. An unknown
    /// name resolves to no fixtures rather than erroring — see
    /// `recipe::unresolved` for how a typo is surfaced without making
    /// the show fail.
    Group(String),
    /// An explicit, inline channel list — for a one-off that does not
    /// warrant naming a reusable group.
    Chans(Vec<ChanId>),
    /// Every fixture carrying this tag, case-insensitively. The venue's
    /// fixtures are already tagged, so this is free data.
    Tag(String),
    /// Every fixture whose manufacturer/model contains this text,
    /// case-insensitively — `"Betopper"`, `"Beam Moving Head"`. Fuzzy
    /// because model strings are vendor prose, unlike group names, which
    /// are operator-authored and exact by convention.
    Model(String),
    Union(Vec<Selection>),
    Intersect(Vec<Selection>),
    Except {
        of: Box<Selection>,
        minus: Box<Selection>,
    },
    /// Keep only the fixtures whose real position passes `filter`. A
    /// fixture with no known placement is dropped, not kept — a spatial
    /// question cannot be answered about it either way, and silently
    /// including it would put an unplaced fixture in a zone it may not
    /// be in.
    Where {
        of: Box<Selection>,
        filter: Where,
    },
    Order {
        of: Box<Selection>,
        by: Order,
    },
}

/// Resolves a selection to its ordered channel list.
pub fn resolve(selection: &Selection, groups: &[Group], rig: &Rig) -> Vec<ChanId> {
    match selection {
        Selection::Chans(chans) => dedup(chans.clone()),
        Selection::Group(name) => group::find(groups, name)
            .map(|g| dedup(g.chans.clone()))
            .unwrap_or_default(),
        Selection::Tag(tag) => rig
            .fixtures()
            .iter()
            .filter(|f| f.tags.iter().any(|t| t.eq_ignore_ascii_case(tag)))
            .map(|f| f.chan)
            .collect(),
        Selection::Model(text) => {
            let needle = text.to_ascii_lowercase();
            rig.fixtures()
                .iter()
                .filter(|f| {
                    format!("{} {}", f.manufacturer, f.model)
                        .to_ascii_lowercase()
                        .contains(&needle)
                })
                .map(|f| f.chan)
                .collect()
        }
        Selection::Union(parts) => {
            dedup(parts.iter().flat_map(|p| resolve(p, groups, rig)).collect())
        }
        Selection::Intersect(parts) => {
            let Some((first, rest)) = parts.split_first() else {
                return Vec::new();
            };
            let others: Vec<Vec<ChanId>> = rest.iter().map(|p| resolve(p, groups, rig)).collect();
            resolve(first, groups, rig)
                .into_iter()
                .filter(|c| others.iter().all(|o| o.contains(c)))
                .collect()
        }
        Selection::Except { of, minus } => {
            let drop = resolve(minus, groups, rig);
            resolve(of, groups, rig)
                .into_iter()
                .filter(|c| !drop.contains(c))
                .collect()
        }
        Selection::Where { of, filter } => resolve(of, groups, rig)
            .into_iter()
            .filter(|c| rig.placement(*c).is_some_and(|p| filter.test(p.position)))
            .collect(),
        Selection::Order { of, by } => {
            let mut chans = resolve(of, groups, rig);
            match by {
                Order::Reverse => chans.reverse(),
                // A fixture with no placement has no position to sort
                // by; `sort_by_key` is stable, so they keep their
                // relative order and land at the end rather than
                // scrambling the fixtures that do have one.
                Order::Axis(axis, dir) => sort_by(&mut chans, *dir, |c| {
                    rig.placement(c).map(|p| axis.of(p.position))
                }),
                Order::Distance { from, dir } => sort_by(&mut chans, *dir, |c| {
                    rig.placement(c).map(|p| {
                        let (dx, dy, dz) = (
                            p.position.x - from.x,
                            p.position.y - from.y,
                            p.position.z - from.z,
                        );
                        dx * dx + dy * dy + dz * dz
                    })
                }),
            }
            chans
        }
    }
}

/// Stable sort by an optional key, `None` last regardless of direction.
fn sort_by(chans: &mut [ChanId], dir: Dir, key: impl Fn(ChanId) -> Option<f64>) {
    chans.sort_by(|a, b| match (key(*a), key(*b)) {
        (Some(x), Some(y)) => {
            let ord = x.partial_cmp(&y).unwrap_or(std::cmp::Ordering::Equal);
            match dir {
                Dir::Asc => ord,
                Dir::Desc => ord.reverse(),
            }
        }
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    });
}

/// First occurrence wins, so a union keeps the order its first branch
/// established — which matters, because that order drives phase spread.
fn dedup(chans: Vec<ChanId>) -> Vec<ChanId> {
    let mut seen = std::collections::HashSet::with_capacity(chans.len());
    chans.into_iter().filter(|c| seen.insert(*c)).collect()
}

/// Every name in `selection` this venue cannot resolve.
pub fn unresolved_names(selection: &Selection, groups: &[Group], rig: &Rig) -> Vec<String> {
    let mut out = Vec::new();
    match selection {
        Selection::Group(name) if group::find(groups, name).is_none() => {
            out.push(format!("no group named {name:?}"));
        }
        Selection::Tag(tag) if resolve(selection, groups, rig).is_empty() => {
            out.push(format!("no fixture tagged {tag:?}"));
        }
        Selection::Model(text) if resolve(selection, groups, rig).is_empty() => {
            out.push(format!("no fixture whose model matches {text:?}"));
        }
        Selection::Union(parts) | Selection::Intersect(parts) => {
            for p in parts {
                out.extend(unresolved_names(p, groups, rig));
            }
        }
        Selection::Except { of, minus } => {
            out.extend(unresolved_names(of, groups, rig));
            out.extend(unresolved_names(minus, groups, rig));
        }
        Selection::Where { of, .. } | Selection::Order { of, .. } => {
            out.extend(unresolved_names(of, groups, rig));
        }
        _ => {}
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use ignition_proto::Quat;

    fn at(chan: ChanId, x: f64, y: f64, z: f64, model: &str, tags: &[&str]) -> FixtureInfo {
        FixtureInfo {
            chan,
            placement: Some(Placement {
                position: Vec3 { x, y, z },
                orientation: Quat {
                    w: 1.0,
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
            }),
            manufacturer: "Uking".to_string(),
            model: model.to_string(),
            tags: tags.iter().map(|t| t.to_string()).collect(),
        }
    }

    /// Patched deliberately out of spatial order, so a test that gets
    /// the right answer cannot have got it by accident.
    fn rig() -> Rig {
        Rig::new(vec![
            at(1, 2.0, 0.0, 3.0, "Par", &["wash"]),
            at(2, -2.0, 0.0, 3.0, "Par", &["wash"]),
            at(3, 0.0, 0.0, 3.0, "Par", &["wash"]),
            at(4, 4.0, 0.0, 0.5, "Beam Moving Head", &["mover"]),
        ])
    }

    fn groups() -> Vec<Group> {
        vec![Group {
            name: "Pars".to_string(),
            chans: vec![1, 2, 3],
        }]
    }

    fn go(sel: Selection) -> Vec<ChanId> {
        resolve(&sel, &groups(), &rig())
    }

    #[test]
    fn a_tag_selects_every_fixture_carrying_it() {
        assert_eq!(go(Selection::Tag("wash".to_string())), vec![1, 2, 3]);
        assert_eq!(go(Selection::Tag("WASH".to_string())), vec![1, 2, 3]);
    }

    #[test]
    fn a_model_match_is_fuzzy_across_manufacturer_and_model() {
        assert_eq!(go(Selection::Model("moving head".to_string())), vec![4]);
        assert_eq!(go(Selection::Model("uking".to_string())).len(), 4);
    }

    #[test]
    fn set_algebra_composes() {
        let all_but_centre = Selection::Except {
            of: Box::new(Selection::Group("Pars".to_string())),
            minus: Box::new(Selection::Chans(vec![3])),
        };
        assert_eq!(go(all_but_centre), vec![1, 2]);

        let both = Selection::Union(vec![
            Selection::Chans(vec![3]),
            Selection::Group("Pars".to_string()),
        ]);
        assert_eq!(go(both), vec![3, 1, 2], "first branch sets the order");

        let overlap = Selection::Intersect(vec![
            Selection::Tag("wash".to_string()),
            Selection::Chans(vec![2, 3, 4]),
        ]);
        assert_eq!(go(overlap), vec![2, 3]);
    }

    #[test]
    fn a_half_space_filter_keeps_one_side_of_the_room() {
        let stage_left = Selection::Where {
            of: Box::new(Selection::Tag("wash".to_string())),
            filter: Where::Half {
                axis: Axis::X,
                cmp: Cmp::Gt,
                at: 0.0,
            },
        };
        assert_eq!(go(stage_left), vec![1]);
    }

    #[test]
    fn a_height_filter_separates_the_ceiling_from_the_floor() {
        let ceiling = Selection::Where {
            of: Box::new(Selection::Union(vec![
                Selection::Tag("wash".to_string()),
                Selection::Tag("mover".to_string()),
            ])),
            filter: Where::Half {
                axis: Axis::Z,
                cmp: Cmp::Gt,
                at: 2.0,
            },
        };
        assert_eq!(go(ceiling), vec![1, 2, 3]);
    }

    /// The point of the whole module: patch order is 1, 2, 3 at x = 2,
    /// -2, 0, so left-to-right is 2, 3, 1 and nothing else.
    #[test]
    fn ordering_by_axis_gives_a_real_left_to_right_chase() {
        let left_to_right = Selection::Order {
            of: Box::new(Selection::Group("Pars".to_string())),
            by: Order::Axis(Axis::X, Dir::Asc),
        };
        assert_eq!(go(left_to_right), vec![2, 3, 1]);

        let right_to_left = Selection::Order {
            of: Box::new(Selection::Group("Pars".to_string())),
            by: Order::Axis(Axis::X, Dir::Desc),
        };
        assert_eq!(go(right_to_left), vec![1, 3, 2]);
    }

    #[test]
    fn ordering_by_distance_gives_a_centre_out_bloom() {
        let centre_out = Selection::Order {
            of: Box::new(Selection::Group("Pars".to_string())),
            by: Order::Distance {
                from: Vec3 {
                    x: 0.0,
                    y: 0.0,
                    z: 3.0,
                },
                dir: Dir::Asc,
            },
        };
        assert_eq!(
            go(centre_out),
            vec![3, 1, 2],
            "centre first, then the two flanks"
        );
    }

    #[test]
    fn filters_and_orders_nest() {
        let sel = Selection::Order {
            of: Box::new(Selection::Where {
                of: Box::new(Selection::Tag("wash".to_string())),
                filter: Where::Half {
                    axis: Axis::X,
                    cmp: Cmp::Le,
                    at: 0.0,
                },
            }),
            by: Order::Axis(Axis::X, Dir::Asc),
        };
        assert_eq!(go(sel), vec![2, 3]);
    }

    /// A fixture with no known position cannot be said to be inside or
    /// outside a zone, so it is dropped rather than assumed.
    #[test]
    fn an_unplaced_fixture_is_dropped_by_a_spatial_filter() {
        let mut fixtures = rig().fixtures().to_vec();
        fixtures.push(FixtureInfo {
            chan: 9,
            placement: None,
            manufacturer: "Uking".to_string(),
            model: "Par".to_string(),
            tags: vec!["wash".to_string()],
        });
        let rig = Rig::new(fixtures);
        let sel = Selection::Where {
            of: Box::new(Selection::Tag("wash".to_string())),
            filter: Where::Half {
                axis: Axis::Z,
                cmp: Cmp::Gt,
                at: 0.0,
            },
        };
        assert_eq!(resolve(&sel, &groups(), &rig), vec![1, 2, 3]);
    }

    /// ...but it still sorts, landing at the end rather than scrambling
    /// the fixtures that do have a position.
    #[test]
    fn an_unplaced_fixture_sorts_last() {
        let mut fixtures = rig().fixtures().to_vec();
        fixtures.push(FixtureInfo {
            chan: 9,
            placement: None,
            manufacturer: "Uking".to_string(),
            model: "Par".to_string(),
            tags: vec!["wash".to_string()],
        });
        let rig = Rig::new(fixtures);
        let sel = Selection::Order {
            of: Box::new(Selection::Tag("wash".to_string())),
            by: Order::Axis(Axis::X, Dir::Asc),
        };
        assert_eq!(resolve(&sel, &groups(), &rig), vec![2, 3, 1, 9]);
    }

    #[test]
    fn a_typo_is_reported_rather_than_silently_selecting_nothing() {
        let sel = Selection::Order {
            of: Box::new(Selection::Tag("mvoer".to_string())),
            by: Order::Reverse,
        };
        assert!(go(sel.clone()).is_empty());
        let problems = unresolved_names(&sel, &groups(), &rig());
        assert_eq!(problems.len(), 1);
        assert!(problems[0].contains("mvoer"), "{problems:?}");
    }
}
