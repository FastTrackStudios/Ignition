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
    pub fn of(self, v: Vec3) -> f64 {
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
// r[impl files.capability-over-name]
// r[impl groups.derived]
pub enum Where {
    /// One side of an axis-aligned plane. "Stage-left half" is
    /// `Half { axis: X, cmp: Gt, at: 0.0 }`.
    Half { axis: Axis, cmp: Cmp, at: f64 },
    /// Inside an axis-aligned box — a zone of the room.
    Within { min: Vec3, max: Vec3 },
    /// Within `radius` metres of a point.
    Near { at: Vec3, radius: f64 },
    /// Fixtures whose **beam lands** inside an axis-aligned box.
    ///
    /// Not where the fixture hangs — where its light arrives. The two
    /// are different questions and confusing them produces a look that
    /// is precisely wrong: selecting the wash fixtures positioned over
    /// the left of the stage and turning everything else off lit the
    /// *centre*, because those fixtures are aimed at the centre like
    /// almost every front wash in every room.
    ///
    /// `Half`/`Within` ask about position and remain right for questions
    /// about the rig — "the fixtures on the left-hand truss". This is
    /// the question to ask about the *stage*: "whatever is lighting
    /// downstage left".
    ///
    /// The beam is projected along the fixture's mount orientation to
    /// `height`, which is where the answer is wanted — faces, not the
    /// floor. A fixture pointing away from that plane is not covering
    /// anything and is excluded.
    // r[impl profile.areas.not-a-focus-point] - fixtures that cover a region, distinct from an aim
    Covers { min: Vec3, max: Vec3, height: f64 },
}

/// Where a fixture's beam axis crosses a horizontal plane.
///
/// The mount orientation aims the beam along its local -Z, which is the
/// same convention the renderer uses (`mount * pan * tilt * -Z`) and the
/// reason identity reads as "hung from the truss, pointing down".
///
/// `None` when the beam never reaches the plane — pointing up, or level
/// enough that it would land somewhere absurd. A fixture that does not
/// cross the plane is not covering anything on it, which is a different
/// statement from covering it at a great distance.
fn beam_landing(placement: &Placement, height: f64) -> Option<Vec3> {
    let q = placement.orientation;
    // Rotate (0, 0, -1) by the quaternion.
    let (x, y, z, w) = (q.x, q.y, q.z, q.w);
    let dir = Vec3 {
        x: -2.0 * (x * z + w * y),
        y: -2.0 * (y * z - w * x),
        z: -(1.0 - 2.0 * (x * x + y * y)),
    };
    let drop = placement.position.z - height;
    // Beam must be travelling toward the plane, and steeply enough that
    // the answer means something: a beam within a few degrees of level
    // lands hundreds of metres away, which is not "covering" anywhere.
    if dir.z >= -1e-3 || drop <= 0.0 {
        return None;
    }
    let t = drop / -dir.z;
    Some(Vec3 {
        x: placement.position.x + dir.x * t,
        y: placement.position.y + dir.y * t,
        z: height,
    })
}

impl Where {
    /// Takes the whole placement, not just the position, because
    /// `Covers` asks where a fixture is *pointing* — a question a point
    /// cannot answer.
    fn test(&self, placement: &Placement) -> bool {
        let p = placement.position;
        match self {
            Where::Covers { min, max, height } => {
                let Some(landing) = beam_landing(placement, *height) else {
                    return false;
                };
                landing.x >= min.x && landing.x <= max.x && landing.y >= min.y && landing.y <= max.y
            }
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
// r[impl groups.order-is-data]
// r[impl groups.one-ordering-authority]
// r[impl profile.spatial-grid-is-derived]
pub enum Order {
    /// Sorted along an axis. `Axis(X, Asc)` is a left-to-right chase;
    /// `Axis(Y, Desc)` sweeps downstage to upstage.
    // r[impl profile.spatial-grid-is-derived]
    // r[impl tricks.grid.from-space] - one axis at a time from real positions
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
// r[impl recipes.template]
// r[impl groups.derived]
pub enum Selection {
    /// Looked up by name against the venue's real groups. An unknown
    /// name resolves to no fixtures rather than erroring — see
    /// `recipe::unresolved` for how a typo is surfaced without making
    /// the show fail.
    Group(String),
    /// A **role** from the profile, resolved through the venue's binding
    /// — `Key`, `Wash`, `Back`.
    ///
    /// The variant that makes a show portable, and the reason recipes
    /// matter. `Group("Center Washers")` names a thing that exists at
    /// exactly one address; `Role("Key")` names a job every venue fills,
    /// and the venue says which of its fixtures fills it. A cue written
    /// against roles plays in Norco, Riverside and Vegas without being
    /// touched.
    ///
    /// An unbound role resolves to no fixtures, like an unknown group —
    /// but unlike an unknown group it is a *reportable* gap, because the
    /// profile said it should be there. See `profile::Profile::gaps`.
    // r[impl profile.resolution-by-role]
    Role(String),
    /// An explicit, inline channel list — for a one-off that does not
    /// warrant naming a reusable group.
    Chans(Vec<ChanId>),
    /// Every fixture carrying this tag, case-insensitively. The venue's
    /// fixtures are already tagged, so this is free data.
    // r[impl files.capability-over-name]
    Tag(String),
    /// Every fixture whose manufacturer/model contains this text,
    /// case-insensitively — `"Betopper"`, `"Beam Moving Head"`. Fuzzy
    /// because model strings are vendor prose, unlike group names, which
    /// are operator-authored and exact by convention.
    // r[impl files.capability-over-name]
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
    /// An explicit grid layout carried through resolution — for a rig
    /// whose logical arrangement is not its physical one: a matrix hung
    /// in a spiral, a strip that snakes.
    ///
    /// `rows` is the layout, outer index Y and inner index X, each entry
    /// a channel. It **overrides** the spatial grid `tricks::Grid`
    /// derives from the rig, and the override is inspectable through
    /// [`layout_of`]. It does not widen the selection: resolution yields
    /// the rows flattened in order, keeping only channels that `of`
    /// resolves to, so a layout naming a fixture the selection does not
    /// include cannot smuggle it in. Channels `of` resolves to that the
    /// layout omits are appended after the laid-out ones, so a partial
    /// layout still lights everything — they land in the grid's last row.
    ///
    /// On disk: `{"Layout":{"of":{"Role":"Wall"},"rows":[[1,2,3],[4,5,6]]}}`.
    // r[impl tricks.grid.explicit-override]
    Layout {
        of: Box<Selection>,
        rows: Vec<Vec<ChanId>>,
    },
}

/// The explicit grid layout a selection carries, if any — outermost
/// wins, so wrapping a laid-out selection in `Order` or `Where` keeps
/// the layout visible.
///
/// This is how "the fact that it is overriding" is inspected: a
/// `Some` here means `tricks::Grid` will be built from these rows and
/// not from the room.
// r[impl tricks.grid.explicit-override] - the override is inspectable
pub fn layout_of(selection: &Selection) -> Option<&Vec<Vec<ChanId>>> {
    match selection {
        Selection::Layout { rows, .. } => Some(rows),
        Selection::Where { of, .. } | Selection::Order { of, .. } => layout_of(of),
        Selection::Except { of, .. } => layout_of(of),
        Selection::Union(parts) | Selection::Intersect(parts) => parts.iter().find_map(layout_of),
        _ => None,
    }
}

/// Resolves a selection to its ordered channel list.
// r[impl groups.resolution-is-live]
// r[impl groups.order-is-stable]
pub fn resolve(selection: &Selection, groups: &[Group], rig: &Rig) -> Vec<ChanId> {
    resolve_with(selection, groups, rig, &())
}

/// What a `Role` resolves to at this venue.
///
/// A trait so `ignition-core` never has to know what a venue *is*: the
/// venue crate owns loading and binding, and this is the one question
/// selection resolution needs answered. `()` implements it as "no roles
/// bound", which keeps every existing caller working and makes a
/// role-free show behave exactly as it did.
// r[impl profile.trait-not-hardcode]
// r[impl profile.resolution-by-role]
pub trait Roles {
    fn role(&self, name: &str) -> Option<&Selection>;
}

impl Roles for () {
    fn role(&self, _: &str) -> Option<&Selection> {
        None
    }
}

// r[impl groups.resolution-is-live]
// r[impl groups.order-is-data]
// r[impl groups.order-is-stable]
// r[impl groups.derived]
// r[impl profile.resolution-by-role]
// r[impl groups.one-ordering-authority] - Order is the only thing that orders
pub fn resolve_with(
    selection: &Selection,
    groups: &[Group],
    rig: &Rig,
    // `dyn` rather than a generic: `Show` holds this behind a reference
    // so a venue can supply its bindings without `Show` — and therefore
    // every function taking one — growing a type parameter that most
    // callers would only ever instantiate as `()`.
    roles: &dyn Roles,
) -> Vec<ChanId> {
    match selection {
        // r[impl profile.resolution-by-role]
        // r[impl profile.unbound-is-visible] - unbound resolves to clearly empty
        // r[impl effects.library.missing-role-is-empty]
        Selection::Role(name) => match roles.role(name) {
            // Resolved against the same machinery, so a role may bind to
            // an expression rather than a group — "washes downstage of
            // the plaster line" is as valid a Key as one named group.
            Some(bound) => resolve_with(bound, groups, rig, roles),
            None => Vec::new(),
        },
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
        Selection::Union(parts) => dedup(
            parts
                .iter()
                .flat_map(|p| resolve_with(p, groups, rig, roles))
                .collect(),
        ),
        Selection::Intersect(parts) => {
            let Some((first, rest)) = parts.split_first() else {
                return Vec::new();
            };
            let others: Vec<Vec<ChanId>> = rest
                .iter()
                .map(|p| resolve_with(p, groups, rig, roles))
                .collect();
            resolve_with(first, groups, rig, roles)
                .into_iter()
                .filter(|c| others.iter().all(|o| o.contains(c)))
                .collect()
        }
        Selection::Except { of, minus } => {
            let drop = resolve_with(minus, groups, rig, roles);
            resolve_with(of, groups, rig, roles)
                .into_iter()
                .filter(|c| !drop.contains(c))
                .collect()
        }
        Selection::Where { of, filter } => resolve_with(of, groups, rig, roles)
            .into_iter()
            .filter(|c| rig.placement(*c).is_some_and(|p| filter.test(&p)))
            .collect(),
        // r[impl groups.order-is-data]
        // r[impl profile.spatial-grid-is-derived]
        Selection::Order { of, by } => {
            let mut chans = resolve_with(of, groups, rig, roles);
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
        // r[impl tricks.grid.explicit-override] - the layout's order wins
        Selection::Layout { of, rows } => {
            let members = resolve_with(of, groups, rig, roles);
            let mut out: Vec<ChanId> = rows
                .iter()
                .flatten()
                .copied()
                .filter(|c| members.contains(c))
                .collect();
            // Anything the layout forgot still belongs to the selection.
            let laid_out = out.clone();
            out.extend(members.iter().copied().filter(|c| !laid_out.contains(c)));
            dedup(out)
        }
    }
}

/// Stable sort by an optional key, `None` last regardless of direction.
// r[impl groups.order-is-stable]
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
// r[impl groups.order-is-stable]
fn dedup(chans: Vec<ChanId>) -> Vec<ChanId> {
    let mut seen = std::collections::HashSet::with_capacity(chans.len());
    chans.into_iter().filter(|c| seen.insert(*c)).collect()
}

/// Every name in `selection` this venue cannot resolve.
pub fn unresolved_names(selection: &Selection, groups: &[Group], rig: &Rig) -> Vec<String> {
    unresolved_names_with(selection, groups, rig, &())
}

/// The same, including whether each **role** is bound.
///
/// A role that no venue binds resolves to no fixtures, exactly like an
/// unknown group — but it is a different *kind* of problem and has to be
/// reported as one. An empty group is usually a typo; an unbound role
/// means this room has not implemented that part of the profile, and the
/// fix is at the venue rather than in the show.
///
/// This is also the check that catches a whole show going dark. A cue
/// list written entirely against roles, opened at a venue with no
/// bindings, produces no light at all and no complaint — every recipe
/// legitimately covers nothing. That happened, and it looked like a
/// renderer fault rather than a missing file.
// r[impl profile.unbound-is-visible]
// r[impl recipes.status.selects-nothing-is-not-an-error] - reported, not fatal
pub fn unresolved_names_with(
    selection: &Selection,
    groups: &[Group],
    rig: &Rig,
    roles: &dyn Roles,
) -> Vec<String> {
    let mut out = Vec::new();
    match selection {
        Selection::Role(name) if roles.role(name).is_none() => {
            out.push(format!("role {name:?} is not bound by this venue"));
        }
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
                out.extend(unresolved_names_with(p, groups, rig, roles));
            }
        }
        Selection::Except { of, minus } => {
            out.extend(unresolved_names_with(of, groups, rig, roles));
            out.extend(unresolved_names_with(minus, groups, rig, roles));
        }
        Selection::Where { of, .. }
        | Selection::Order { of, .. }
        | Selection::Layout { of, .. } => {
            out.extend(unresolved_names_with(of, groups, rig, roles));
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

    // r[verify files.capability-over-name]

    // r[verify groups.derived]

    #[test]
    fn a_tag_selects_every_fixture_carrying_it() {
        assert_eq!(go(Selection::Tag("wash".to_string())), vec![1, 2, 3]);
        assert_eq!(go(Selection::Tag("WASH".to_string())), vec![1, 2, 3]);
    }

    // r[verify files.capability-over-name]

    #[test]
    fn a_model_match_is_fuzzy_across_manufacturer_and_model() {
        assert_eq!(go(Selection::Model("moving head".to_string())), vec![4]);
        assert_eq!(go(Selection::Model("uking".to_string())).len(), 4);
    }

    // r[verify groups.order-is-data]

    // r[verify groups.order-is-stable]

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

    // r[verify groups.derived]

    // r[verify files.capability-over-name]

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

    // r[verify groups.derived]

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
    // r[verify profile.spatial-grid-is-derived]
    // r[verify groups.order-is-data]
    // r[verify groups.one-ordering-authority]
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

    // r[verify groups.order-is-data]

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
    // r[verify groups.order-is-stable]
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

    /// An explicit layout orders the selection its own way, never widens
    /// it, and never loses a member it forgot to mention.
    // r[verify tricks.grid.explicit-override]
    #[test]
    fn a_layout_orders_without_widening_and_is_inspectable() {
        let sel = Selection::Order {
            of: Box::new(Selection::Layout {
                of: Box::new(Selection::Group("Pars".to_string())),
                rows: vec![vec![3, 99], vec![1]],
            }),
            by: Order::Reverse,
        };
        // Rows flattened, 99 excluded (not in the group), 2 appended
        // (in the group, not in the layout); then the outer Reverse.
        assert_eq!(go(sel.clone()), vec![2, 1, 3]);
        assert_eq!(layout_of(&sel), Some(&vec![vec![3, 99], vec![1]]));
        assert_eq!(layout_of(&Selection::Group("Pars".to_string())), None);

        let json = serde_json::to_string(&Selection::Layout {
            of: Box::new(Selection::Chans(vec![1])),
            rows: vec![vec![1]],
        })
        .unwrap();
        assert_eq!(json, r#"{"Layout":{"of":{"Chans":[1]},"rows":[[1]]}}"#);
        let back: Selection = serde_json::from_str(&json).unwrap();
        assert!(matches!(back, Selection::Layout { .. }));
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

    /// An unbound role is reported, and says so in terms of the venue
    /// rather than the show.
    ///
    /// This is the check that would have caught a whole show going dark.
    /// A cue list written entirely against roles, opened at a venue that
    /// binds none of them, lights nothing and complains about nothing —
    /// every recipe legitimately covers no fixtures. It looked like a
    /// renderer fault.
    // r[verify profile.unbound-is-visible]
    #[test]
    fn an_unbound_role_is_reported() {
        struct None_;
        impl Roles for None_ {
            fn role(&self, _: &str) -> Option<&Selection> {
                Option::None
            }
        }
        let problems =
            unresolved_names_with(&Selection::Role("Key".into()), &[], &EMPTY_RIG, &None_);
        assert_eq!(problems.len(), 1);
        assert!(problems[0].contains("Key"), "{problems:?}");
        assert!(
            problems[0].contains("venue"),
            "the report must point at the venue, not the show: {problems:?}"
        );
    }

    /// A bound role is not reported, however it is nested.
    // r[verify profile.resolution-by-role]
    #[test]
    fn a_bound_role_is_quiet_at_any_depth() {
        struct Bound(Selection);
        impl Roles for Bound {
            fn role(&self, name: &str) -> Option<&Selection> {
                (name == "Key").then_some(&self.0)
            }
        }
        let bound = Bound(Selection::Chans(Vec::new()));
        let nested = Selection::Order {
            of: Box::new(Selection::Union(vec![Selection::Role("Key".into())])),
            by: Order::Axis(Axis::X, Dir::Asc),
        };
        assert!(unresolved_names_with(&nested, &[], &EMPTY_RIG, &bound).is_empty());
    }
}
