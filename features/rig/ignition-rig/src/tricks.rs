//! Tricks — addressing part of a selection.
//!
//! grandMA3 calls this MAtricks. The name is shorter and carries nobody
//! else's initials; the model is theirs, minus the parts that are
//! console ergonomics rather than ideas.
//!
//! The whole design fits in one sentence: **a Trick is a selection**.
//! `Block(3)` applied to a wash is a selection, so it can be ordered,
//! filtered, unioned, and Tricked again. Built instead as a parameter
//! that effects understand, it would be a second and weaker selection
//! language that static looks could not use — and half of what makes a
//! rig read as programmed rather than chased is a *static* cue with a
//! spread on it.
//!
//! The other thing that matters is that every Trick is a **proportion**.
//! `Group(2)` is odds and evens whether the rig has forty fixtures or
//! four, and a fan across a selection is across however many there are.
//! That is what lets one show play at three venues: it never says "the
//! third mover", so it never breaks on a rig that has two.
//!
//! **The grid.** A selection is also a three-axis grid ([`Grid`]),
//! derived from where the fixtures actually hang (`tricks.grid.from-space`)
//! or from an explicit layout the selection carries
//! (`tricks.grid.explicit-override`). Every Trick is one-dimensional and
//! stays that way; [`Trick::OnAxis`] applies it along one axis of the
//! grid — to each line of cells along Y, say — and a bare Trick keeps its
//! old meaning over the grid's X order with the rows concatenated. An axis
//! nothing varies along has size 1, so a Trick on Y over a single truss
//! is the no-op `tricks.grid.degenerate-axes` asks for rather than a
//! selection of nothing. [`apply_all_grid`] is the entry point; the 1-D
//! [`apply_all`] remains for callers with no rig in hand.
//!
//! grandMA3's *Invert X* — flipping the axis — is `Reverse` (on that
//! axis); its *Invert style* — flipping the sign of a relative value on
//! some of the units — is [`Trick::Invert`], which is a different thing
//! and is why both names are here.

use crate::ChanId;
use crate::selection::{Axis, Rig};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A way of cutting a selection into parts, or of reordering it.
///
/// Applied to an ordered list of channels and returning one — never
/// returning "sub-selections" as a distinct type, because a sub-
/// selection that is not a selection is exactly the second language this
/// is built to avoid.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
// r[impl tricks.composable]
// r[impl tricks.on-the-recipe]
// r[impl tricks.grid] - every Trick is expressible per axis via OnAxis
pub enum Trick {
    /// Contiguous runs of `n`, each acting as one unit. Ten fixtures at
    /// `Block(2)` is five units: (1,2) (3,4) (5,6) (7,8) (9,10).
    // r[impl tricks.block]
    Block(usize),
    /// Round-robin into `n` parts. Ten at `Group(2)` is (1,3,5,7,9) and
    /// (2,4,6,8,10).
    ///
    /// Not a variant of `Block`, and the difference is the point:
    /// `Block(5)` and `Group(2)` both cut ten fixtures into two fives,
    /// and one is halves of the rig while the other is odds and evens.
    // r[impl tricks.group]
    // r[impl tricks.block-is-not-group]
    Group(usize),
    /// Divide into `n` parts and mirror alternate ones, so a symmetric
    /// rig runs from one definition and opens outward from centre.
    // r[impl tricks.wings]
    Wings(usize),
    /// Reorder pseudo-randomly. The same seed over the same count always
    /// gives the same order — a random look that cannot be recalled has
    /// been discovered, not programmed.
    // r[impl tricks.shuffle]
    Shuffle(u32),
    /// Rotate the selection by `n`, so a pattern moves along the rig
    /// without being re-authored.
    // r[impl tricks.shift]
    Shift(isize),
    /// Reverse. The cheap half of `Wings`, and worth having alone.
    ///
    /// Also grandMA3's *Invert X*: in one dimension, inverting the axis
    /// is reversing the list, so there is no separate `Invert` variant
    /// to drift away from this one.
    // r[impl tricks.shift] - the other reordering; Invert X in 1-D
    Reverse,
    /// Fold the selection about its centre so values come out symmetric:
    /// the outermost pair becomes one unit, the next pair the next unit,
    /// and so on inward. A spread across the result runs from the ends
    /// to the middle on both sides at once — a symmetric fan from one
    /// from/to.
    ///
    /// This is a fold of *units*, not of fixtures, which is what lets it
    /// compose: `Block(2)` then `Mirror` pairs the outermost pairs,
    /// `Wings(2)` then `Mirror` pairs each wing's ends with the other's.
    ///
    /// An odd count leaves a centre unit with nothing to pair with. It is
    /// kept as its own unit, last — never folded onto itself (which would
    /// count it twice) and never dropped. It therefore takes the spread's
    /// `to` alone, which is the 1-D reading of grandMA3's *edge fixture*:
    /// it holds the centre of the symmetric shape rather than following
    /// one side of it.
    // r[impl tricks.mirror]
    // r[impl tricks.mirror.odd-centre]
    Mirror,
    /// Flip the sign of relative values — pan, tilt, both, or every
    /// attribute — on the **second half** of the units.
    ///
    /// Mirror changes *which* unit takes a value; this changes the value.
    /// It never reorders anything: `apply` is the identity, and the
    /// recipe asks [`inverted`] which units it marked. The rule is the
    /// second half of the units *as the preceding Tricks left them*
    /// (an odd count leaves the centre unit alone), which is what makes
    /// it compose:
    ///
    /// - `Invert(Pan)` alone: the far half of the rig circles the other
    ///   way from the near half.
    /// - `Group(2)` then `Invert(Pan)`: two units, odds and evens, and
    ///   the evens invert — "odd movers circle the other way".
    /// - `Wings(2)` then `Invert(Pan)`: the second wing inverts.
    /// - `Block(2)` then `Invert(Tilt)`: the last half of the pairs.
    ///
    /// Where it sits in the chain does not matter — every Invert in a
    /// chain is read against the final units — because a Trick that
    /// reorders after it would otherwise scramble which half was meant.
    // r[impl tricks.invert]
    // r[impl effects.invert]
    Invert(InvertStyle),
    /// The inner Trick applied along one axis of the selection's
    /// [`Grid`] — to every line of cells running along that axis,
    /// independently, with the other two coordinates left alone.
    ///
    /// `OnAxis(Y, Wings(2))` on a two-truss rig mirrors the upper truss
    /// against the lower; `OnAxis(X, Block(2))` pairs neighbours along
    /// every truss; `OnAxis(Y, Shuffle(7))` permutes whole rows and
    /// leaves every column's order intact, which is what
    /// `tricks.shuffle.axes` means by not disturbing the other axes.
    ///
    /// A bare Trick (no `OnAxis`) acts on the grid's X order with the
    /// rows concatenated — today's one-dimensional meaning — and after it
    /// the grid is one-dimensional: the units it produced, in order,
    /// along X. That is a definition rather than a limitation: `Block(2)`
    /// then `OnAxis(Y, …)` is asking for Y after Y has been folded away,
    /// and the answer is the no-op the degenerate-axes rule prescribes.
    /// Put the per-axis Tricks first.
    ///
    /// Through the 1-D [`apply_all`] — no grid — `OnAxis(X, t)` is `t`
    /// and `OnAxis(Y | Z, _)` is inert, because a list has only an X.
    ///
    /// On disk: `{"OnAxis":["Y",{"Wings":2}]}`.
    // r[impl tricks.grid]
    // r[impl tricks.grid.degenerate-axes]
    // r[impl tricks.shuffle.axes]
    OnAxis(Axis, Box<Trick>),
}

/// Which relative values [`Trick::Invert`] flips.
///
/// On disk as `{"Invert":"Pan"}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
// r[impl tricks.invert]
// r[impl effects.invert]
pub enum InvertStyle {
    Pan,
    Tilt,
    PanTilt,
    All,
}

impl InvertStyle {
    /// True when this style covers `attr`.
    pub fn covers(self, attr: &crate::Attribute) -> bool {
        use crate::Attribute;
        match self {
            InvertStyle::Pan => *attr == Attribute::Pan,
            InvertStyle::Tilt => *attr == Attribute::Tilt,
            InvertStyle::PanTilt => matches!(attr, Attribute::Pan | Attribute::Tilt),
            InvertStyle::All => true,
        }
    }
}

/// Which units of `count` a chain of Tricks inverts, and in what style:
/// `None` for a unit left alone. The second half of the units, per
/// [`Trick::Invert`]; several Inverts widen the style rather than
/// cancel.
// r[impl tricks.invert]
// r[impl effects.invert]
pub fn inverted(tricks: &[Trick], count: usize) -> Vec<Option<InvertStyle>> {
    let style = tricks
        .iter()
        .filter_map(|t| match t.on_axis() {
            // A list has only an X; an Invert on Y or Z has nothing to
            // split and marks nobody.
            (Axis::X, Trick::Invert(s)) => Some(*s),
            _ => None,
        })
        .fold(None, |acc, s| Some(widen(acc, s)));
    let first = count.div_ceil(2);
    (0..count)
        .map(|i| if i >= first { style } else { None })
        .collect()
}

/// Two Invert styles on the same unit widen rather than cancel: Pan
/// then Tilt is PanTilt, and All swallows everything.
// r[impl tricks.invert]
fn widen(acc: Option<InvertStyle>, s: InvertStyle) -> InvertStyle {
    match (acc, s) {
        (_, InvertStyle::All) | (Some(InvertStyle::All), _) => InvertStyle::All,
        (None, s) => s,
        (Some(a), b) if a == b => a,
        _ => InvertStyle::PanTilt,
    }
}

/// A cut selection: the units, in order, each of one or more channels.
///
/// A unit is what everything downstream sees. Spreading a phase across a
/// blocked selection gives one phase per *unit*, so a pair moves
/// together — which is the entire purpose of blocking.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
// r[impl tricks.spread.blocks-are-units]
// r[impl tricks.block] - units are what downstream sees
pub struct Units(pub Vec<Vec<ChanId>>);

impl Units {
    /// Every channel, flattened back into selection order.
    pub fn flat(&self) -> Vec<ChanId> {
        self.0.iter().flatten().copied().collect()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// One channel per unit, in order — for a Trick applied to something
    /// that wants fixtures rather than groups of them.
    pub fn leaders(&self) -> Vec<ChanId> {
        self.0.iter().filter_map(|u| u.first().copied()).collect()
    }
}

/// Every channel as its own unit — a selection nobody has cut yet.
pub fn units_of(chans: &[ChanId]) -> Units {
    Units(chans.iter().map(|c| vec![*c]).collect())
}

impl Trick {
    /// Which axis this Trick runs along, and the 1-D Trick that runs
    /// there. A bare Trick is on X; nested `OnAxis` resolves to the
    /// innermost axis.
    pub fn on_axis(&self) -> (Axis, &Trick) {
        match self {
            Trick::OnAxis(axis, inner) => {
                let (deeper, t) = inner.on_axis();
                if matches!(**inner, Trick::OnAxis(..)) {
                    (deeper, t)
                } else {
                    (*axis, t)
                }
            }
            t => (Axis::X, t),
        }
    }

    /// Applies this Trick to already-cut units.
    ///
    /// Taking and returning `Units` is what makes Tricks chain:
    /// `Block(2)` then `Group(2)` is five pairs dealt into two combs,
    /// and neither needs to know the other happened.
    ///
    /// This is the one-dimensional reading. `OnAxis(X, t)` is `t`;
    /// `OnAxis(Y | Z, _)` is inert, because a list has no other axis to
    /// address — the degenerate-axes rule. [`apply_all_grid`] is where Y
    /// and Z mean something.
    // r[impl tricks.composable]
    // r[impl tricks.grid.degenerate-axes] - Y/Z on a list is a no-op
    pub fn apply(&self, units: Units) -> Units {
        match self {
            Trick::Block(n) => block(units, *n),
            Trick::Group(n) => group(units, *n),
            Trick::Wings(n) => wings(units, *n),
            Trick::Shuffle(seed) => shuffle(units, *seed),
            Trick::Shift(n) => shift(units, *n),
            Trick::Reverse => {
                let mut units = units;
                units.0.reverse();
                units
            }
            Trick::Mirror => mirror(units),
            // Changes values, not order — see `inverted`.
            Trick::Invert(_) => units,
            Trick::OnAxis(..) => match self.on_axis() {
                (Axis::X, inner) => inner.apply(units),
                _ => units,
            },
        }
    }
}

/// Applies a chain of Tricks, left to right.
// r[impl tricks.composable]
pub fn apply_all(chans: &[ChanId], tricks: &[Trick]) -> Units {
    tricks
        .iter()
        .fold(units_of(chans), |units, trick| trick.apply(units))
}

/// Which room axes the grid's X, Y and Z are read from, and how close
/// two positions must be to share a row.
///
/// The default is the obvious one — grid X is room X and so on — and
/// exists as a type so a rig hung diagonally, or a wall where "across"
/// is the room's Z, can say so without the grid growing a rotation.
///
/// `tolerance` is in metres. Fixtures whose coordinate along an axis is
/// within it of the first fixture in a bin share that bin: a truss that
/// sags a few centimetres is still one row, and two trusses a metre
/// apart are two. 0.35 m is under half the closest spacing pars are
/// hung at in practice and over any sag a truss survives.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
// r[impl tricks.grid.from-space]
pub struct GridAxes {
    pub x: Axis,
    pub y: Axis,
    pub z: Axis,
    pub tolerance: f64,
}

impl Default for GridAxes {
    fn default() -> Self {
        Self {
            x: Axis::X,
            y: Axis::Y,
            z: Axis::Z,
            tolerance: 0.35,
        }
    }
}

/// One fixture's place in a [`Grid`]: integer coordinates, 0-based,
/// each below the grid's `size` on that axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    pub chan: ChanId,
    pub x: usize,
    pub y: usize,
    pub z: usize,
}

/// A selection as a three-axis grid.
///
/// Derived from the rig ([`Grid::from_rig`]) or declared
/// ([`Grid::explicit`]); [`Grid::for_selection`] picks between them the
/// way the spec says to. Cells are kept in **selection order** — the
/// grid adds coordinates to a selection, it does not reorder it — and
/// [`apply_all_grid`] is what turns them into units.
///
/// An axis nothing varies along has `size` 1 there, so a single truss
/// is `[n, 1, 1]` and a Trick on its Y addresses one row of everything:
/// a no-op, not an empty selection.
#[derive(Debug, Clone, PartialEq, Eq)]
// r[impl tricks.grid]
// r[impl tricks.grid.degenerate-axes]
pub struct Grid {
    pub cells: Vec<Cell>,
    /// Rows along X, Y and Z. Every axis is at least 1 unless there are
    /// no cells at all.
    pub size: [usize; 3],
    explicit: bool,
}

impl Grid {
    /// The grid the room implies: each axis binned into rows of
    /// fixtures whose real coordinate is within `axes.tolerance` of one
    /// another, indexed ascending along that axis.
    ///
    /// Fixtures the rig has no placement for cannot be located and sit
    /// at index 0 on every axis — the same rule the spatial `Order`
    /// uses, applied to a grid: they are kept, never dropped, and they
    /// never invent a row.
    // r[impl tricks.grid.from-space]
    // r[impl tricks.grid.degenerate-axes]
    pub fn from_rig(chans: &[ChanId], rig: &Rig, axes: GridAxes) -> Self {
        let coord = |axis: Axis| -> Vec<usize> { bin(chans, rig, axis, axes.tolerance) };
        let (xs, ys, zs) = (coord(axes.x), coord(axes.y), coord(axes.z));
        let cells = chans
            .iter()
            .enumerate()
            .map(|(i, &chan)| Cell {
                chan,
                x: xs[i],
                y: ys[i],
                z: zs[i],
            })
            .collect();
        Self {
            cells,
            size: [rows(&xs), rows(&ys), rows(&zs)],
            explicit: false,
        }
    }

    /// The room's grid, keeping the **selection's order along a row**.
    ///
    /// `from_rig` sorts every axis by coordinate, which is right for a
    /// layout and wrong for an effect: a chase on `Order::Axis(X, Desc)`
    /// would silently run the other way. So Y and Z are binned from the
    /// room — a truss is a row, a second truss above it is a layer —
    /// and X is the fixture's rank *within* its row in the order the
    /// selection produced. One truss is `[n, 1, 1]` in exactly today's
    /// order; two trusses at different heights are `[n, 1, 2]`; a
    /// two-deep, two-high rig is `[n, 2, 2]`. An axis along which
    /// nothing varies has size 1 — a Trick on it is a no-op, not a
    /// selection of nothing.
    // r[impl tricks.grid.from-space]
    // r[impl tricks.grid.degenerate-axes]
    // r[impl tricks.grid] - the third axis
    pub fn from_rig_in_order(chans: &[ChanId], rig: &Rig, axes: GridAxes) -> Self {
        let ys = bin(chans, rig, axes.y, axes.tolerance);
        let zs = bin(chans, rig, axes.z, axes.tolerance);
        let mut rank: std::collections::HashMap<(usize, usize), usize> =
            std::collections::HashMap::new();
        let mut cells = Vec::with_capacity(chans.len());
        for (i, &chan) in chans.iter().enumerate() {
            if cells.iter().any(|c: &Cell| c.chan == chan) {
                continue;
            }
            let slot = rank.entry((ys[i], zs[i])).or_insert(0);
            cells.push(Cell {
                chan,
                x: *slot,
                y: ys[i],
                z: zs[i],
            });
            *slot += 1;
        }
        let width = rank.values().copied().max().unwrap_or(0);
        Self {
            cells,
            size: [width.max(1), rows(&ys), rows(&zs)],
            explicit: false,
        }
    }

    /// A declared layout: `rows[y][x]`, Z always 0. Ragged rows are
    /// allowed and the grid is as wide as the widest.
    ///
    /// Overrides whatever the room would have said, and says so through
    /// [`Grid::is_override`].
    // r[impl tricks.grid.explicit-override]
    pub fn explicit(rows: Vec<Vec<ChanId>>) -> Self {
        let mut cells = Vec::new();
        for (y, row) in rows.iter().enumerate() {
            for (x, &chan) in row.iter().enumerate() {
                if !cells.iter().any(|c: &Cell| c.chan == chan) {
                    cells.push(Cell { chan, x, y, z: 0 });
                }
            }
        }
        let width = rows.iter().map(Vec::len).max().unwrap_or(0);
        let height = rows.iter().filter(|r| !r.is_empty()).count();
        Self {
            cells,
            size: [width.max(1), height.max(1), 1],
            explicit: true,
        }
    }

    /// The grid for a resolved selection: the explicit layout when the
    /// selection carries one (`selection::layout_of`), the room's
    /// otherwise. `chans` is the selection as `resolve_with` returned
    /// it; a layout is trimmed to those channels, and channels the
    /// layout omits are added as one extra row at the end so they are
    /// still addressed.
    ///
    /// This is the call recipe.rs makes — see [`apply_all_grid`].
    // r[impl tricks.grid.explicit-override] - the explicit layout wins
    // r[impl tricks.grid.from-space] - the room is the default
    pub fn for_selection(
        chans: &[ChanId],
        layout: Option<&Vec<Vec<ChanId>>>,
        rig: &Rig,
        axes: GridAxes,
    ) -> Self {
        match layout {
            None => Self::from_rig(chans, rig, axes),
            Some(rows) => {
                let mut rows: Vec<Vec<ChanId>> = rows
                    .iter()
                    .map(|r| r.iter().copied().filter(|c| chans.contains(c)).collect())
                    .collect();
                let missing: Vec<ChanId> = chans
                    .iter()
                    .copied()
                    .filter(|c| !rows.iter().flatten().any(|r| r == c))
                    .collect();
                if !missing.is_empty() {
                    rows.push(missing);
                }
                Self::explicit(rows)
            }
        }
    }

    /// True when this grid came from a declared layout rather than the
    /// room — the inspectable half of `tricks.grid.explicit-override`.
    // r[impl tricks.grid.explicit-override] - the override is inspectable
    pub fn is_override(&self) -> bool {
        self.explicit
    }

    pub fn len(&self) -> usize {
        self.cells.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }
}

/// Bin `chans` along one room axis: sorted by coordinate, a new row
/// starts whenever a fixture is more than `tolerance` from the first
/// fixture of the current row. Returns one row index per input channel,
/// in input order. Unplaced fixtures get 0.
// r[impl tricks.grid.from-space]
fn bin(chans: &[ChanId], rig: &Rig, axis: Axis, tolerance: f64) -> Vec<usize> {
    let mut placed: Vec<(usize, f64)> = chans
        .iter()
        .enumerate()
        .filter_map(|(i, &c)| rig.placement(c).map(|p| (i, axis.of(p.position))))
        .collect();
    placed.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
    let mut out = vec![0; chans.len()];
    let mut row = 0;
    let mut start = f64::NAN;
    for (i, v) in placed {
        if start.is_nan() {
            start = v;
        } else if (v - start).abs() > tolerance {
            row += 1;
            start = v;
        }
        out[i] = row;
    }
    out
}

fn rows(coords: &[usize]) -> usize {
    coords.iter().max().map_or(0, |m| m + 1).max(1)
}

/// Where a unit sits in the grid of units, after the Tricks.
///
/// `count` is the size of the *unit* grid on each axis, which is what a
/// spread is over: `x / count[0]` is the unit's fraction along X in the
/// same sense `Timing::spread_fraction`'s `index / count` is. A unit on
/// a degenerate axis has coordinate 0 of 1 there and contributes nothing
/// to a spread on that axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
// r[impl tricks.grid]
// r[impl effects.phase.spread] - the position a per-axis spread lands on
pub struct UnitPos {
    pub x: usize,
    pub y: usize,
    pub z: usize,
    pub count: [usize; 3],
}

impl UnitPos {
    /// This unit's fraction along `axis`, 0 at the leading end and
    /// `i / count` after — the 3-D twin of `Timing::spread_fraction`.
    pub fn fraction(&self, axis: Axis) -> f32 {
        let (i, n) = match axis {
            Axis::X => (self.x, self.count[0]),
            Axis::Y => (self.y, self.count[1]),
            Axis::Z => (self.z, self.count[2]),
        };
        if n > 1 { i as f32 / n as f32 } else { 0.0 }
    }
}

/// The result of [`apply_all_grid`]: the units, and where each sits.
///
/// `units.0[i]` is at `pos[i]`. The order is row-major over the unit
/// grid — X fastest, then Y, then Z — so `units` read as a plain list is
/// "every row of the bottom layer left to right, then the next row",
/// and on a one-truss rig is exactly what [`apply_all`] returns.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
// r[impl tricks.grid]
// r[impl tricks.spread.blocks-are-units]
pub struct GridUnits {
    pub units: Units,
    pub pos: Vec<UnitPos>,
    /// Size of the unit grid on each axis.
    pub count: [usize; 3],
}

impl GridUnits {
    pub fn len(&self) -> usize {
        self.units.len()
    }

    pub fn is_empty(&self) -> bool {
        self.units.is_empty()
    }
}

/// A unit while the chain is being applied: channels plus coordinates.
#[derive(Debug, Clone)]
struct Placed {
    chans: Vec<ChanId>,
    pos: [usize; 3],
}

fn axis_index(axis: Axis) -> usize {
    match axis {
        Axis::X => 0,
        Axis::Y => 1,
        Axis::Z => 2,
    }
}

/// Row-major: Z slowest, X fastest; ties keep their existing order.
fn sort_row_major(units: &mut [Placed]) {
    units.sort_by_key(|u| (u.pos[2], u.pos[1], u.pos[0]));
}

/// Applies a chain of Tricks to a grid, left to right.
///
/// **Wave-3 wiring for `recipe.rs`** (`expand_recipe_full`), replacing
/// the `apply_all` / `inverted` / `spread_fraction` trio:
///
/// ```text
/// let chans  = selection::resolve_with(&recipe.target, show.groups, show.rig, show.roles);
/// let layout = selection::layout_of(&recipe.target);
/// let grid   = tricks::Grid::for_selection(&chans, layout, show.rig, tricks::GridAxes::default());
/// let gu     = tricks::apply_all_grid(&recipe.tricks, &grid);
/// let inverts = tricks::inverted_grid(&recipe.tricks, &gu);
/// let count  = gu.len();
/// for (index, unit) in gu.units.0.iter().enumerate() {
///     let pos = gu.pos[index];
///     // phaser:  recipe.timing.cycles_at_pos(secs, &pos, show.speeds)
///     //          (== cycles_at(secs, index, count, ..) when only phase_spread_deg is set)
///     // Build:   u >= recipe.timing.build_fraction_3d(&pos)
///     // invert:  inverts[index]
///     // Slot { index, count } and everything per-fixture stay as they are.
/// }
/// ```
///
/// Nothing about the 1-D path changes meaning: on a rig where nothing
/// varies along Y or Z the grid is `[n, 1, 1]`, `gu.units` is what
/// `apply_all` returns, `gu.pos[i].fraction(X)` is `spread_fraction(i, n)`
/// and `cycles_at_pos` equals `cycles_at`. Only `OnAxis` Tricks and the
/// new `phase_spread_y_deg` / `phase_spread_z_deg` see the difference.
///
/// Semantics, per Trick in the chain:
///
/// - `OnAxis(a, t)`: every line of units running along `a` (one per
///   distinct pair of the other two coordinates) is sorted along `a`,
///   `t` is applied to it as a 1-D chain, and the results are put back
///   at coordinates `0..m` along `a`. The other coordinates are
///   untouched — a Shuffle on Y permutes rows and every column keeps its
///   order (`tricks.shuffle.axes`). Lines may differ in length; the unit
///   grid is as long as the longest.
/// - a bare Trick: the units are flattened row-major and `t` applied to
///   that list; the result is one-dimensional along X.
/// - `Invert(_)` never moves anything; see [`inverted_grid`].
///
/// The result is sorted row-major (X fastest) whatever the chain did.
// r[impl tricks.grid]
// r[impl tricks.grid.degenerate-axes]
// r[impl tricks.shuffle.axes]
// r[impl tricks.composable]
// r[impl tricks.spread.blocks-are-units]
pub fn apply_all_grid(tricks: &[Trick], grid: &Grid) -> GridUnits {
    let mut units: Vec<Placed> = grid
        .cells
        .iter()
        .map(|c| Placed {
            chans: vec![c.chan],
            pos: [c.x, c.y, c.z],
        })
        .collect();
    let mut count = grid.size;
    for trick in tricks {
        match trick {
            Trick::Invert(_) => {}
            Trick::OnAxis(..) => {
                let (axis, inner) = trick.on_axis();
                if matches!(inner, Trick::Invert(_)) {
                    continue;
                }
                (units, count) = along(units, count, axis, inner);
            }
            bare => {
                sort_row_major(&mut units);
                let out = bare.apply(Units(units.into_iter().map(|u| u.chans).collect()));
                units = out
                    .0
                    .into_iter()
                    .enumerate()
                    .map(|(x, chans)| Placed {
                        chans,
                        pos: [x, 0, 0],
                    })
                    .collect();
                count = [units.len().max(1), 1, 1];
            }
        }
    }
    sort_row_major(&mut units);
    let pos = units
        .iter()
        .map(|u| UnitPos {
            x: u.pos[0],
            y: u.pos[1],
            z: u.pos[2],
            count,
        })
        .collect();
    GridUnits {
        units: Units(units.into_iter().map(|u| u.chans).collect()),
        pos,
        count,
    }
}

/// One Trick along one axis: see [`apply_all_grid`].
fn along(
    units: Vec<Placed>,
    mut count: [usize; 3],
    axis: Axis,
    trick: &Trick,
) -> (Vec<Placed>, [usize; 3]) {
    let a = axis_index(axis);
    let (o1, o2) = ((a + 1) % 3, (a + 2) % 3);
    // BTreeMap so lines come back in a fixed order; the row-major sort
    // at the end makes the final order independent of it anyway.
    let mut lines: BTreeMap<(usize, usize), Vec<Placed>> = BTreeMap::new();
    for u in units {
        lines.entry((u.pos[o1], u.pos[o2])).or_default().push(u);
    }
    let mut out = Vec::new();
    let mut longest = 1;
    for ((c1, c2), mut line) in lines {
        line.sort_by_key(|u| u.pos[a]);
        let cut = trick.apply(Units(line.into_iter().map(|u| u.chans).collect()));
        longest = longest.max(cut.len());
        for (i, chans) in cut.0.into_iter().enumerate() {
            let mut pos = [0; 3];
            pos[a] = i;
            pos[o1] = c1;
            pos[o2] = c2;
            out.push(Placed { chans, pos });
        }
    }
    count[a] = longest;
    (out, count)
}

/// Which units of a grid result a chain inverts, and in what style —
/// the grid twin of [`inverted`], indexed like `gu.units`.
///
/// A bare `Invert` marks the second half of the units in row-major
/// order; `OnAxis(a, Invert(_))` marks the units in the far half along
/// `a` (`pos[a] >= ceil(count[a] / 2)`), so `OnAxis(Y, Invert(Pan))` on
/// two trusses has the upper truss circling the other way, and on one
/// truss marks nobody — the degenerate axis has no far half.
// r[impl tricks.invert]
// r[impl effects.invert]
// r[impl tricks.grid.degenerate-axes]
pub fn inverted_grid(tricks: &[Trick], gu: &GridUnits) -> Vec<Option<InvertStyle>> {
    let n = gu.len();
    let mut out: Vec<Option<InvertStyle>> = vec![None; n];
    for trick in tricks {
        let (axis, inner) = trick.on_axis();
        let Trick::Invert(style) = inner else {
            continue;
        };
        let bare = !matches!(trick, Trick::OnAxis(..));
        for (i, slot) in out.iter_mut().enumerate() {
            let far = if bare {
                i >= n.div_ceil(2)
            } else {
                let p = gu.pos[i];
                let (c, total) = match axis {
                    Axis::X => (p.x, p.count[0]),
                    Axis::Y => (p.y, p.count[1]),
                    Axis::Z => (p.z, p.count[2]),
                };
                total > 1 && c >= total.div_ceil(2)
            };
            if far {
                *slot = Some(widen(*slot, *style));
            }
        }
    }
    out
}

// r[impl tricks.block]
// r[impl tricks.block-is-not-group]
fn block(units: Units, n: usize) -> Units {
    // `Block(0)` and `Block(1)` are both "leave it alone" rather than an
    // error: a Trick chain is data, often generated, and a degenerate
    // value should be inert instead of taking the show down.
    if n <= 1 {
        return units;
    }
    Units(
        units
            .0
            .chunks(n)
            .map(|chunk| chunk.iter().flatten().copied().collect())
            .collect(),
    )
}

// r[impl tricks.group]
// r[impl tricks.block-is-not-group]
fn group(units: Units, n: usize) -> Units {
    if n <= 1 {
        return units;
    }
    let mut out: Vec<Vec<ChanId>> = vec![Vec::new(); n];
    for (i, unit) in units.0.into_iter().enumerate() {
        out[i % n].extend(unit);
    }
    // A group that caught nothing is dropped rather than kept as an
    // empty unit: `Group(4)` over three fixtures is three groups, and a
    // fourth empty one would take a quarter of every phase spread and
    // light nothing with it.
    out.retain(|g| !g.is_empty());
    Units(out)
}

// r[impl tricks.wings]
fn wings(units: Units, n: usize) -> Units {
    if n <= 1 {
        return units;
    }
    let total = units.0.len();
    if total == 0 {
        return units;
    }
    // Ceiling division, so the last wing is the short one rather than
    // there being an extra wing holding the remainder.
    let per = total.div_ceil(n);
    let mut out = Vec::with_capacity(total);
    for (index, wing) in units.0.chunks(per).enumerate() {
        let mut wing: Vec<Vec<ChanId>> = wing.to_vec();
        // Alternate wings run outward from centre, which is what makes
        // one definition symmetric.
        if index % 2 == 1 {
            wing.reverse();
        }
        out.extend(wing);
    }
    Units(out)
}

// r[impl tricks.shift]
fn shift(units: Units, n: isize) -> Units {
    let len = units.0.len();
    if len == 0 || n == 0 {
        return units;
    }
    // `rem_euclid` so a negative shift rotates the other way rather than
    // panicking on a negative index.
    let by = n.rem_euclid(len as isize) as usize;
    let mut out = units.0;
    out.rotate_left(by);
    Units(out)
}

// r[impl tricks.mirror]
// r[impl tricks.mirror.odd-centre]
fn mirror(units: Units) -> Units {
    let n = units.0.len();
    if n < 2 {
        return units;
    }
    let mut src = units.0;
    let mut out = Vec::with_capacity(n.div_ceil(2));
    // Fold outside in: (first, last), (second, second-last)...
    while src.len() >= 2 {
        let last = src.pop().expect("len >= 2");
        let mut first = src.remove(0);
        first.extend(last);
        out.push(first);
    }
    // The odd centre. Explicit, so nobody has to reason about whether
    // the arithmetic paired it with itself.
    if let Some(centre) = src.pop() {
        out.push(centre);
    }
    Units(out)
}

/// Deterministic shuffle.
///
/// A hand-rolled xorshift rather than the `rand` crate, for one reason:
/// reproducibility must survive a dependency bump. `rand`'s generators
/// are explicitly allowed to change their output between versions, so a
/// show whose look came from a seed would quietly become a different
/// show. This will produce the same order in ten years.
// r[impl tricks.shuffle]
// r[impl effects.play.random] - a seeded reorder, not a play mode
fn shuffle(units: Units, seed: u32) -> Units {
    if seed == 0 || units.0.len() < 2 {
        return units;
    }
    let mut out = units.0;
    // Seeded so that seed alone determines the order; `| 1` because
    // xorshift is stuck at zero.
    let mut state = (u64::from(seed) << 16 | 0x9E37).max(1);
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    // Fisher-Yates, back to front.
    for i in (1..out.len()).rev() {
        let j = (next() % (i as u64 + 1)) as usize;
        out.swap(i, j);
    }
    Units(out)
}

/// Distributes a value across units — MAtricks' from/to layers.
///
/// Deliberately free of any notion of effect. This is how a value meets
/// a selection, so a static cue with a delay fan and a running chase
/// with the same fan share one mechanism; built into the phaser, the
/// static case would be unreachable.
// r[impl tricks.spread]
// r[impl tricks.spread.not-an-effect]
// r[impl tricks.spread.blocks-are-units] - indexed by unit
pub fn spread(from: f32, to: f32, index: usize, count: usize) -> f32 {
    if count <= 1 {
        return from;
    }
    let t = index as f32 / (count - 1) as f32;
    from + (to - from) * t
}

/// A from/to pair — a value to spread across a selection.
///
/// One type for every attribute that fans, so phase, delay, fade and
/// speed cannot each grow their own slightly different from/to rule.
/// The recipe side owns *which* attribute a `Fan` drives (see
/// `recipe::Timing`); this owns only how the two numbers meet the units.
/// `Fan { from: 0.0, to: 0.0 }` is "everyone together", which is why it
/// is the default.
///
/// `shape` and `curve` are grandMA3's Align modes: *where* along the
/// selection each end lands, and *how* the values run between them. On
/// disk both are optional, so `{"from":0,"to":1}` keeps meaning the
/// straight line it always did.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
// r[impl tricks.spread.attributes]
// r[impl tricks.spread.not-an-effect]
// r[impl tricks.fan.shapes]
// r[impl effects.align]
pub struct Fan {
    pub from: f32,
    pub to: f32,
    #[serde(default, skip_serializing_if = "FanShape::is_default")]
    pub shape: FanShape,
    #[serde(default, skip_serializing_if = "Curve::is_default")]
    pub curve: Curve,
}

/// Where the ends of a spread land — MA3's Align `<`, `>`, `<>`, `><`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
// r[impl tricks.fan.shapes]
// r[impl effects.align]
pub enum FanShape {
    /// First unit takes `from`, last takes `to`.
    #[default]
    Linear,
    /// First unit fixed at `from`; the rest step toward `to` as if one
    /// more unit would reach it. Adding a fixture never moves the first.
    FromFirst,
    /// Last unit fixed at `to`; the rest step back from it the same way.
    FromLast,
    /// Both ends take `from`, the centre takes `to` — a zoom that opens
    /// from the middle.
    CentreOut,
    /// Both ends take `to`, the centre takes `from` — the tilt "V".
    EndsIn,
}

impl FanShape {
    fn is_default(&self) -> bool {
        *self == FanShape::Linear
    }

    /// Maps a unit's position `t` (0 first … 1 last) to where it sits
    /// between `from` (0) and `to` (1) under this shape.
    pub fn place(self, index: usize, count: usize) -> f32 {
        let n = count.max(1) as f32;
        let t = if count > 1 {
            index as f32 / (count - 1) as f32
        } else {
            0.0
        };
        match self {
            FanShape::Linear => t,
            FanShape::FromFirst => index as f32 / n,
            FanShape::FromLast => (index as f32 + 1.0) / n,
            FanShape::CentreOut => 1.0 - (2.0 * t - 1.0).abs(),
            FanShape::EndsIn => (2.0 * t - 1.0).abs(),
        }
    }
}

/// How values run between the ends of a spread.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
// r[impl tricks.fan.shapes]
// r[impl effects.align]
pub enum Curve {
    #[default]
    Linear,
    /// Eased at both ends.
    Sine,
    /// Slow to leave `from`, then quick.
    Slow,
    /// Quick to leave `from`, then slow into `to`.
    Fast,
}

impl Curve {
    fn is_default(&self) -> bool {
        *self == Curve::Linear
    }

    /// Reshapes a 0…1 position.
    pub fn apply(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Curve::Linear => t,
            Curve::Sine => (1.0 - (std::f32::consts::PI * t).cos()) / 2.0,
            Curve::Slow => t * t,
            Curve::Fast => 1.0 - (1.0 - t) * (1.0 - t),
        }
    }
}

impl Fan {
    pub const fn new(from: f32, to: f32) -> Self {
        Self {
            from,
            to,
            shape: FanShape::Linear,
            curve: Curve::Linear,
        }
    }

    /// The same numbers with a shape and a curve.
    pub const fn shaped(from: f32, to: f32, shape: FanShape, curve: Curve) -> Self {
        Self {
            from,
            to,
            shape,
            curve,
        }
    }

    /// The value for unit `index` of `count`.
    // r[impl tricks.spread]
    // r[impl tricks.fan.shapes]
    pub fn at(self, index: usize, count: usize) -> f32 {
        if count <= 1 {
            return self.from;
        }
        let t = self.curve.apply(self.shape.place(index, count));
        self.from + (self.to - self.from) * t
    }

    /// One value per unit, in selection order.
    // r[impl tricks.spread.blocks-are-units]
    pub fn over(self, units: &Units) -> Vec<f32> {
        (0..units.len()).map(|i| self.at(i, units.len())).collect()
    }

    /// True when nothing is spread and every unit takes `from`.
    pub fn is_flat(self) -> bool {
        self.from == self.to
    }

    /// More than two keyframes along the selection, sharing this fan's
    /// shape and curve. `from` and `to` are ignored — the points are the
    /// values.
    // r[impl tricks.keyframes]
    pub fn keyframes(self, points: Vec<f32>) -> Keyframes {
        Keyframes {
            points,
            shape: self.shape,
            curve: self.curve,
        }
    }
}

/// Several values placed evenly along the selection, each unit
/// interpolated between the two nearest — MA3's MAgic presets, on any
/// scalar. Two points is a `Fan`; five is the whole truss shaped by hand
/// once and landing on any count.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
// r[impl tricks.keyframes]
// r[impl focus.magic]
pub struct Keyframes {
    pub points: Vec<f32>,
    #[serde(default, skip_serializing_if = "FanShape::is_default")]
    pub shape: FanShape,
    #[serde(default, skip_serializing_if = "Curve::is_default")]
    pub curve: Curve,
}

impl Keyframes {
    /// Which pair of keyframes unit `index` of `count` falls between,
    /// and how far along that pair — with the curve applied *within*
    /// the pair, so each segment eases the same way. Returns
    /// `(lower, upper, fraction)` into a list of `n` points; `None` when
    /// there are no points.
    // r[impl tricks.keyframes]
    pub fn segment(
        shape: FanShape,
        curve: Curve,
        n: usize,
        index: usize,
        count: usize,
    ) -> Option<(usize, usize, f32)> {
        if n == 0 {
            return None;
        }
        if n == 1 || count <= 1 {
            return Some((0, 0, 0.0));
        }
        let t = shape.place(index, count) * (n - 1) as f32;
        let lo = (t.floor() as usize).min(n - 1);
        let hi = (lo + 1).min(n - 1);
        Some((lo, hi, curve.apply(t - lo as f32)))
    }

    /// The value for unit `index` of `count`.
    pub fn at(&self, index: usize, count: usize) -> f32 {
        match Self::segment(self.shape, self.curve, self.points.len(), index, count) {
            Some((lo, hi, f)) => self.points[lo] + (self.points[hi] - self.points[lo]) * f,
            None => 0.0,
        }
    }

    /// One value per unit, in selection order.
    pub fn over(&self, units: &Units) -> Vec<f32> {
        (0..units.len()).map(|i| self.at(i, units.len())).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ten() -> Vec<ChanId> {
        (1..=10).collect()
    }

    use crate::selection::FixtureInfo;
    use ignition_proto::{Placement, Quat, Vec3};

    fn fixture(chan: ChanId, x: f64, y: f64, z: f64) -> FixtureInfo {
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
            manufacturer: "Uking".into(),
            model: "Par".into(),
            tags: Vec::new(),
        }
    }

    /// Two trusses of eight, a metre apart in X and 3 m apart in Y.
    /// Channels 1–8 downstage, 9–16 upstage; both sag a few centimetres
    /// so a naive equality test would split every truss into eight rows.
    fn two_trusses() -> Rig {
        let mut f = Vec::new();
        for i in 0..8 {
            let x = i as f64 - 3.5;
            f.push(fixture(i + 1, x, 0.0 + 0.01 * i as f64, 4.0));
            f.push(fixture(i + 9, x, 3.0 - 0.02 * i as f64, 4.0));
        }
        Rig::new(f)
    }

    fn lone_truss() -> Rig {
        Rig::new((0..8).map(|i| fixture(i + 1, i as f64, 0.0, 4.0)).collect())
    }

    fn matrix4() -> Rig {
        let mut f = Vec::new();
        for y in 0..4 {
            for x in 0..4 {
                f.push(fixture(y * 4 + x + 1, x as f64, y as f64, 0.0));
            }
        }
        Rig::new(f)
    }

    /// Grid axes derive from where fixtures hang, whatever order they
    /// were patched or selected in — a re-patch cannot change the look.
    // r[verify tricks.grid]
    // r[verify tricks.grid.from-space]
    #[test]
    fn a_two_truss_rig_is_an_8_by_2_grid() {
        let rig = two_trusses();
        // Selected upstage first and right to left, deliberately.
        let chans: Vec<ChanId> = (9..=16).rev().chain((1..=8).rev()).collect();
        let grid = Grid::from_rig(&chans, &rig, GridAxes::default());
        assert_eq!(grid.size, [8, 2, 1]);
        assert!(!grid.is_override());
        let cell = |c| grid.cells.iter().find(|k| k.chan == c).copied().unwrap();
        assert_eq!((cell(1).x, cell(1).y, cell(1).z), (0, 0, 0));
        assert_eq!((cell(8).x, cell(8).y), (7, 0));
        assert_eq!((cell(9).x, cell(9).y), (0, 1));
        assert_eq!((cell(16).x, cell(16).y), (7, 1));

        // No Tricks: row-major, left to right, downstage truss first.
        let gu = apply_all_grid(&[], &grid);
        assert_eq!(gu.units.flat(), (1..=16).collect::<Vec<_>>());
        assert_eq!(gu.count, [8, 2, 1]);
        assert_eq!(
            gu.pos[9],
            UnitPos {
                x: 1,
                y: 1,
                z: 0,
                count: [8, 2, 1]
            }
        );
    }

    /// A line of pars is one row on Y and Z, and a Trick there is a
    /// no-op — not a selection of nothing.
    // r[verify tricks.grid.degenerate-axes]
    #[test]
    fn a_lone_truss_is_8_by_1_and_y_tricks_are_inert() {
        let rig = lone_truss();
        let chans: Vec<ChanId> = (1..=8).collect();
        let grid = Grid::from_rig(&chans, &rig, GridAxes::default());
        assert_eq!(grid.size, [8, 1, 1]);
        for trick in [
            Trick::OnAxis(Axis::Y, Box::new(Trick::Wings(2))),
            Trick::OnAxis(Axis::Y, Box::new(Trick::Reverse)),
            Trick::OnAxis(Axis::Z, Box::new(Trick::Shuffle(3))),
            Trick::OnAxis(Axis::Y, Box::new(Trick::Block(4))),
        ] {
            let gu = apply_all_grid(std::slice::from_ref(&trick), &grid);
            assert_eq!(gu.units.flat(), chans, "{trick:?}");
            assert_eq!(gu.len(), 8, "{trick:?}");
            assert_eq!(gu.count, [8, 1, 1], "{trick:?}");
            // The 1-D path agrees.
            assert_eq!(
                apply_all(&chans, std::slice::from_ref(&trick)).flat(),
                chans
            );
        }
        // Invert on Y marks nobody on one truss.
        let gu = apply_all_grid(&[], &grid);
        let inv = inverted_grid(
            &[Trick::OnAxis(
                Axis::Y,
                Box::new(Trick::Invert(InvertStyle::Pan)),
            )],
            &gu,
        );
        assert!(inv.iter().all(Option::is_none));
    }

    /// The same Trick on a different axis is a different look: Wings on
    /// Y mirrors the trusses against each other, Block on X pairs
    /// neighbours along each truss.
    // r[verify tricks.grid]
    // r[verify tricks.wings]
    // r[verify tricks.block]
    #[test]
    fn y_wings_mirror_rows_and_x_block_pairs_columns() {
        let rig = two_trusses();
        let chans: Vec<ChanId> = (1..=16).collect();
        let grid = Grid::from_rig(&chans, &rig, GridAxes::default());

        let wings = apply_all_grid(&[Trick::OnAxis(Axis::Y, Box::new(Trick::Wings(2)))], &grid);
        // Two rows, so each wing is one unit deep and the second wing
        // reversed is itself: Y positions are unchanged but the spread
        // now reads 0 for both rows — mirrored about the gap. The
        // observable is the unit grid: still 8 × 2, every column intact.
        assert_eq!(wings.count, [8, 2, 1]);
        assert_eq!(wings.units.flat(), chans);

        // With four rows the mirroring shows: rows 2,3 come back as 3,2.
        let rig4 = matrix4();
        let all: Vec<ChanId> = (1..=16).collect();
        let g4 = Grid::from_rig(&all, &rig4, GridAxes::default());
        let w4 = apply_all_grid(&[Trick::OnAxis(Axis::Y, Box::new(Trick::Wings(2)))], &g4);
        let row = |y: usize| -> Vec<ChanId> {
            w4.units
                .0
                .iter()
                .zip(&w4.pos)
                .filter(|(_, p)| p.y == y)
                .flat_map(|(u, _)| u.iter().copied())
                .collect()
        };
        assert_eq!(row(0), vec![1, 2, 3, 4]);
        assert_eq!(row(1), vec![5, 6, 7, 8]);
        assert_eq!(row(2), vec![13, 14, 15, 16], "second wing runs outward");
        assert_eq!(row(3), vec![9, 10, 11, 12]);

        let block = apply_all_grid(&[Trick::OnAxis(Axis::X, Box::new(Trick::Block(2)))], &grid);
        assert_eq!(block.count, [4, 2, 1]);
        assert_eq!(block.len(), 8);
        assert_eq!(block.units.0[0], vec![1, 2]);
        assert_eq!(block.units.0[3], vec![7, 8]);
        assert_eq!(
            block.units.0[4],
            vec![9, 10],
            "the upstage truss is paired too"
        );
        assert_eq!(
            block.pos[4],
            UnitPos {
                x: 0,
                y: 1,
                z: 0,
                count: [4, 2, 1]
            }
        );

        // Y-Block groups the trusses into one unit per column.
        let column = apply_all_grid(&[Trick::OnAxis(Axis::Y, Box::new(Trick::Block(2)))], &grid);
        assert_eq!(column.count, [8, 1, 1]);
        assert_eq!(column.units.0[0], vec![1, 9]);
    }

    /// Shuffling Y permutes whole rows; every column keeps its order.
    /// Scrambling everything is the explicit choice of a bare Shuffle.
    // r[verify tricks.shuffle.axes]
    #[test]
    fn shuffle_on_one_axis_leaves_the_other_alone() {
        let rig = matrix4();
        let all: Vec<ChanId> = (1..=16).collect();
        let grid = Grid::from_rig(&all, &rig, GridAxes::default());
        let gu = apply_all_grid(
            &[Trick::OnAxis(Axis::Y, Box::new(Trick::Shuffle(5)))],
            &grid,
        );
        assert_eq!(gu.count, [4, 4, 1]);
        // Each unit's X is still the X it hung at.
        for (u, p) in gu.units.0.iter().zip(&gu.pos) {
            assert_eq!(p.x, ((u[0] - 1) % 4) as usize, "{u:?} at {p:?}");
        }
        // Rows moved as rows: the four units at any Y share an original row.
        for y in 0..4 {
            let rows: std::collections::BTreeSet<ChanId> = gu
                .units
                .0
                .iter()
                .zip(&gu.pos)
                .filter(|(_, p)| p.y == y)
                .map(|(u, _)| (u[0] - 1) / 4)
                .collect();
            assert_eq!(rows.len(), 1, "row {y} mixes source rows");
        }
        // And something actually moved.
        assert_ne!(gu.units.flat(), all);

        // A bare Shuffle scrambles the flattened list and the grid
        // collapses to 1-D.
        let scrambled = apply_all_grid(&[Trick::Shuffle(5)], &grid);
        assert_eq!(scrambled.count, [16, 1, 1]);
        assert_eq!(
            scrambled.units.flat(),
            apply_all(&all, &[Trick::Shuffle(5)]).flat()
        );
    }

    /// A declared layout beats the room, is inspectable, and never loses
    /// a fixture it forgot to mention.
    // r[verify tricks.grid.explicit-override]
    #[test]
    fn an_explicit_layout_overrides_the_room() {
        let rig = lone_truss();
        let chans: Vec<ChanId> = (1..=8).collect();
        // A snake: the room says one row of eight, the layout says two
        // rows of four with the second running backwards.
        let layout = vec![vec![1, 2, 3, 4], vec![8, 7, 6, 5]];
        let grid = Grid::for_selection(&chans, Some(&layout), &rig, GridAxes::default());
        assert!(grid.is_override());
        assert_eq!(grid.size, [4, 2, 1]);
        let gu = apply_all_grid(&[], &grid);
        assert_eq!(gu.units.flat(), vec![1, 2, 3, 4, 8, 7, 6, 5]);

        // Trimmed to the selection; the forgotten one lands in a last row.
        let partial = vec![vec![1, 2, 99], vec![3, 4]];
        let grid = Grid::for_selection(&[1, 2, 3, 4, 5], Some(&partial), &rig, GridAxes::default());
        assert_eq!(grid.size, [2, 3, 1]);
        assert_eq!(apply_all_grid(&[], &grid).units.flat(), vec![1, 2, 3, 4, 5]);

        let spatial = Grid::for_selection(&chans, None, &rig, GridAxes::default());
        assert!(!spatial.is_override());
        assert_eq!(spatial.size, [8, 1, 1]);
    }

    /// On a one-truss rig the grid path is the list path, unit for unit,
    /// so wave-3 wiring changes nothing an existing show can see.
    // r[verify tricks.composable]
    // r[verify tricks.grid.degenerate-axes]
    #[test]
    fn the_grid_path_agrees_with_the_list_path_on_one_truss() {
        let rig = lone_truss();
        let chans: Vec<ChanId> = (1..=8).collect();
        let grid = Grid::from_rig(&chans, &rig, GridAxes::default());
        for tricks in [
            vec![Trick::Block(2)],
            vec![Trick::Group(3), Trick::Reverse],
            vec![Trick::Wings(2), Trick::Mirror],
            vec![Trick::Shift(3), Trick::Shuffle(9)],
            vec![Trick::Group(2), Trick::Invert(InvertStyle::Pan)],
        ] {
            let gu = apply_all_grid(&tricks, &grid);
            let list = apply_all(&chans, &tricks);
            assert_eq!(gu.units, list, "{tricks:?}");
            assert_eq!(
                inverted_grid(&tricks, &gu),
                inverted(&tricks, list.len()),
                "{tricks:?}"
            );
            for (i, p) in gu.pos.iter().enumerate() {
                assert_eq!((p.x, p.count[0]), (i, list.len()));
            }
        }
    }

    /// Invert per axis: the upstage truss circles the other way.
    // r[verify tricks.invert]
    #[test]
    fn invert_on_y_marks_the_far_truss() {
        let rig = two_trusses();
        let chans: Vec<ChanId> = (1..=16).collect();
        let grid = Grid::from_rig(&chans, &rig, GridAxes::default());
        let tricks = [Trick::OnAxis(
            Axis::Y,
            Box::new(Trick::Invert(InvertStyle::Pan)),
        )];
        let gu = apply_all_grid(&tricks, &grid);
        let inv = inverted_grid(&tricks, &gu);
        for (u, i) in gu.units.0.iter().zip(&inv) {
            let expect = if u[0] > 8 {
                Some(InvertStyle::Pan)
            } else {
                None
            };
            assert_eq!(*i, expect, "{u:?}");
        }
    }

    // r[verify tricks.grid]
    #[test]
    fn on_axis_json_shape() {
        let t = Trick::OnAxis(Axis::Y, Box::new(Trick::Wings(2)));
        let json = serde_json::to_string(&t).unwrap();
        assert_eq!(json, r#"{"OnAxis":["Y",{"Wings":2}]}"#);
        let back: Trick = serde_json::from_str(&json).unwrap();
        assert_eq!(back, t);
        assert_eq!(t.on_axis(), (Axis::Y, &Trick::Wings(2)));
    }

    // r[verify tricks.block]

    #[test]
    fn block_makes_contiguous_runs() {
        let units = apply_all(&ten(), &[Trick::Block(2)]);
        assert_eq!(units.len(), 5);
        assert_eq!(units.0[0], vec![1, 2]);
        assert_eq!(units.0[4], vec![9, 10]);
    }

    // r[verify tricks.group]

    #[test]
    fn group_deals_round_robin() {
        let units = apply_all(&ten(), &[Trick::Group(2)]);
        assert_eq!(units.len(), 2);
        assert_eq!(units.0[0], vec![1, 3, 5, 7, 9]);
        assert_eq!(units.0[1], vec![2, 4, 6, 8, 10]);
    }

    /// The distinction the whole model rests on. Both cut ten fixtures
    /// into two fives, and they look nothing alike — halves of the rig
    /// against odds and evens. Implementing one as a flag on the other
    /// puts half the available looks out of reach.
    // r[verify tricks.block-is-not-group]
    #[test]
    fn block_and_group_are_different_operations() {
        let blocked = apply_all(&ten(), &[Trick::Block(5)]);
        let grouped = apply_all(&ten(), &[Trick::Group(2)]);
        assert_eq!(blocked.len(), grouped.len(), "both give two units");
        assert_eq!(blocked.0[0], vec![1, 2, 3, 4, 5]);
        assert_eq!(grouped.0[0], vec![1, 3, 5, 7, 9]);
        assert_ne!(blocked.0[0], grouped.0[0]);
    }

    /// Tricks chain, because each takes and returns the same thing.
    // r[verify tricks.composable]
    #[test]
    fn tricks_compose() {
        // Five pairs, dealt into two combs.
        let units = apply_all(&ten(), &[Trick::Block(2), Trick::Group(2)]);
        assert_eq!(units.len(), 2);
        assert_eq!(units.0[0], vec![1, 2, 5, 6, 9, 10]);
        assert_eq!(units.0[1], vec![3, 4, 7, 8]);
    }

    /// No fixture is invented or lost, whatever the chain.
    // r[verify tricks.composable]
    #[test]
    fn every_trick_preserves_the_fixtures() {
        for tricks in [
            vec![Trick::Block(3)],
            vec![Trick::Group(3)],
            vec![Trick::Wings(2)],
            vec![Trick::Shuffle(7)],
            vec![Trick::Shift(3)],
            vec![Trick::Reverse],
            vec![Trick::Mirror],
            vec![Trick::Wings(2), Trick::Mirror],
            vec![Trick::Block(2), Trick::Group(2), Trick::Shuffle(9)],
            vec![Trick::Group(2), Trick::Invert(InvertStyle::Pan)],
        ] {
            let mut got = apply_all(&ten(), &tricks).flat();
            got.sort();
            assert_eq!(got, ten(), "{tricks:?} changed the fixture set");
        }
    }

    /// A group that caught nothing is dropped. Kept, it would take a
    /// share of every phase spread and light nothing with it.
    #[test]
    fn an_empty_group_is_dropped() {
        let units = apply_all(&[1, 2, 3], &[Trick::Group(5)]);
        assert_eq!(units.len(), 3);
        assert!(units.0.iter().all(|u| !u.is_empty()));
    }

    /// Degenerate values are inert rather than fatal. Trick chains are
    /// data and often generated; a zero should not take the show down.
    #[test]
    fn degenerate_tricks_are_inert() {
        for trick in [
            Trick::Block(0),
            Trick::Block(1),
            Trick::Group(1),
            Trick::Shuffle(0),
        ] {
            assert_eq!(
                apply_all(&ten(), std::slice::from_ref(&trick)).flat(),
                ten(),
                "{trick:?}"
            );
        }
    }

    /// The same seed always gives the same order, and different seeds
    /// generally do not. Reproducibility is what makes a shuffled look
    /// programmable rather than merely discovered.
    // r[verify tricks.shuffle]
    #[test]
    fn shuffle_is_reproducible() {
        let a = apply_all(&ten(), &[Trick::Shuffle(42)]).flat();
        let b = apply_all(&ten(), &[Trick::Shuffle(42)]).flat();
        let c = apply_all(&ten(), &[Trick::Shuffle(43)]).flat();
        assert_eq!(a, b);
        assert_ne!(a, c, "two seeds gave the same order");
        assert_ne!(a, ten(), "the shuffle did not shuffle");
    }

    // r[verify tricks.shift]

    #[test]
    fn shift_rotates_both_ways() {
        assert_eq!(
            apply_all(&[1, 2, 3, 4], &[Trick::Shift(1)]).flat(),
            vec![2, 3, 4, 1]
        );
        assert_eq!(
            apply_all(&[1, 2, 3, 4], &[Trick::Shift(-1)]).flat(),
            vec![4, 1, 2, 3]
        );
    }

    /// Wings mirror alternate parts, so a symmetric look runs from one
    /// definition.
    // r[verify tricks.wings]
    #[test]
    fn wings_mirror_alternate_parts() {
        let units = apply_all(&[1, 2, 3, 4, 5, 6], &[Trick::Wings(2)]).flat();
        assert_eq!(units, vec![1, 2, 3, 6, 5, 4]);
    }

    /// Spread is a proportion, which is what makes a show portable: the
    /// ends are the ends however many fixtures there are.
    // r[verify tricks.spread]
    #[test]
    fn spread_hits_both_ends_at_any_size() {
        for count in [2, 3, 8, 47] {
            assert!((spread(0.0, 1.0, 0, count) - 0.0).abs() < 1e-6);
            assert!((spread(0.0, 1.0, count - 1, count) - 1.0).abs() < 1e-6);
        }
        // A selection of one takes the start rather than dividing by zero.
        assert!((spread(0.25, 1.0, 0, 1) - 0.25).abs() < 1e-6);
    }

    /// Mirror folds the ends together, so one from/to gives a fan that
    /// is the same on both sides.
    // r[verify tricks.mirror]
    #[test]
    fn mirror_folds_outside_in() {
        let units = apply_all(&[1, 2, 3, 4, 5, 6], &[Trick::Mirror]);
        assert_eq!(units.0, vec![vec![1, 6], vec![2, 5], vec![3, 4]]);
        // The ends share the start of the spread, the middle its end.
        let fan = Fan::new(0.0, 1.0).over(&units);
        assert_eq!(fan, vec![0.0, 0.5, 1.0]);
    }

    /// The odd centre unit is its own unit, last, once. Not paired with
    /// itself, not lost.
    // r[verify tricks.mirror.odd-centre]
    #[test]
    fn mirror_keeps_the_odd_centre_alone() {
        let units = apply_all(&[1, 2, 3, 4, 5], &[Trick::Mirror]);
        assert_eq!(units.0, vec![vec![1, 5], vec![2, 4], vec![3]]);
        assert_eq!(
            units.flat().len(),
            5,
            "the centre was double-counted or dropped"
        );
        // A selection of one is its own centre.
        assert_eq!(apply_all(&[7], &[Trick::Mirror]).0, vec![vec![7]]);
    }

    /// Mirror composes with what is already in force rather than
    /// mirroring the raw fixtures underneath it.
    // r[verify tricks.mirror]
    #[test]
    fn mirror_composes_with_block_and_wings() {
        let blocked = apply_all(&[1, 2, 3, 4, 5, 6, 7, 8], &[Trick::Block(2), Trick::Mirror]);
        assert_eq!(blocked.0, vec![vec![1, 2, 7, 8], vec![3, 4, 5, 6]]);
        // Wings(2) orders 1 2 3 6 5 4; mirroring that pairs each wing's
        // outer end with the other's inner end.
        let winged = apply_all(&[1, 2, 3, 4, 5, 6], &[Trick::Wings(2), Trick::Mirror]);
        assert_eq!(winged.0, vec![vec![1, 4], vec![2, 5], vec![3, 6]]);
    }

    /// One `Fan` serves every attribute that spreads; the numbers are
    /// the same whichever attribute they land on.
    // r[verify tricks.spread.attributes]
    #[test]
    fn fan_is_attribute_agnostic() {
        let units = apply_all(&(1..=5).collect::<Vec<_>>(), &[]);
        let phase = Fan::new(0.0, 360.0).over(&units);
        let delay = Fan::new(0.0, 2.0).over(&units);
        assert_eq!(phase[2], 180.0);
        assert_eq!(delay[2], 1.0);
        assert!(Fan::default().is_flat());
        assert_eq!(Fan::new(0.3, 0.3).at(4, 9), 0.3);
    }

    /// Existing JSON keeps loading, and the new variant round-trips in
    /// the same shape as `Reverse`.
    // r[verify tricks.shared-or-inline]
    #[test]
    fn tricks_json_is_stable() {
        let json = r#"[{"Block":2},{"Group":2},{"Wings":2},{"Shuffle":7},{"Shift":-1},"Reverse","Mirror"]"#;
        let tricks: Vec<Trick> = serde_json::from_str(json).unwrap();
        assert_eq!(tricks.len(), 7);
        assert_eq!(tricks[6], Trick::Mirror);
        assert_eq!(serde_json::to_string(&tricks).unwrap(), json);
    }

    /// The second half inverts, read against the final units, so it
    /// composes with Group (odds against evens), Wings (the second
    /// wing) and Block (the last pairs).
    /// r[verify tricks.invert]
    /// r[verify effects.invert]
    #[test]
    fn invert_marks_the_second_half_of_the_units() {
        let units = apply_all(&ten(), &[Trick::Group(2), Trick::Invert(InvertStyle::Pan)]);
        assert_eq!(units.len(), 2, "invert does not reorder");
        assert_eq!(
            inverted(&[Trick::Group(2), Trick::Invert(InvertStyle::Pan)], 2),
            vec![None, Some(InvertStyle::Pan)]
        );
        let wings = [Trick::Wings(2), Trick::Invert(InvertStyle::Tilt)];
        let marks = inverted(&wings, apply_all(&ten(), &wings).len());
        assert_eq!(&marks[..5], &[None; 5]);
        assert!(marks[5..].iter().all(|m| *m == Some(InvertStyle::Tilt)));
        // An odd count leaves the centre alone.
        assert_eq!(
            inverted(&[Trick::Invert(InvertStyle::All)], 3),
            vec![None, None, Some(InvertStyle::All)]
        );
        // Two inverts widen the style.
        assert_eq!(
            inverted(
                &[
                    Trick::Invert(InvertStyle::Pan),
                    Trick::Invert(InvertStyle::Tilt)
                ],
                2
            )[1],
            Some(InvertStyle::PanTilt)
        );
        assert!(inverted(&[Trick::Group(2)], 2).iter().all(Option::is_none));
        assert!(InvertStyle::Pan.covers(&crate::Attribute::Pan));
        assert!(!InvertStyle::Pan.covers(&crate::Attribute::Tilt));
        assert!(InvertStyle::All.covers(&crate::Attribute::Dimmer));
    }

    /// `{"Invert":"Pan"}` on disk, beside the others.
    /// r[verify tricks.invert]
    #[test]
    fn invert_json_shape() {
        let json = r#"[{"Invert":"Pan"},{"Invert":"PanTilt"}]"#;
        let tricks: Vec<Trick> = serde_json::from_str(json).unwrap();
        assert_eq!(tricks[0], Trick::Invert(InvertStyle::Pan));
        assert_eq!(serde_json::to_string(&tricks).unwrap(), json);
    }

    /// The Align shapes: linear hits both ends; first-fixed and
    /// last-fixed hold one end and never reach the other; centre-out
    /// and ends-in are symmetric.
    /// r[verify tricks.fan.shapes]
    /// r[verify effects.align]
    #[test]
    fn fan_shapes_place_the_ends_where_align_says() {
        let five = apply_all(&(1..=5).collect::<Vec<_>>(), &[]);
        let at = |shape| Fan::shaped(0.0, 1.0, shape, Curve::Linear).over(&five);
        assert_eq!(at(FanShape::Linear), vec![0.0, 0.25, 0.5, 0.75, 1.0]);
        assert_eq!(at(FanShape::FromFirst), vec![0.0, 0.2, 0.4, 0.6, 0.8]);
        assert_eq!(at(FanShape::FromLast), vec![0.2, 0.4, 0.6, 0.8, 1.0]);
        assert_eq!(at(FanShape::CentreOut), vec![0.0, 0.5, 1.0, 0.5, 0.0]);
        assert_eq!(at(FanShape::EndsIn), vec![1.0, 0.5, 0.0, 0.5, 1.0]);
        // A selection of one takes `from`, whatever the shape.
        assert_eq!(
            Fan::shaped(0.3, 1.0, FanShape::EndsIn, Curve::Sine).at(0, 1),
            0.3
        );
    }

    /// Curves keep the ends and bend the middle the way their name says.
    /// r[verify tricks.fan.shapes]
    /// r[verify effects.align]
    #[test]
    fn fan_curves_bend_the_middle_and_keep_the_ends() {
        for curve in [Curve::Linear, Curve::Sine, Curve::Slow, Curve::Fast] {
            let fan = Fan::shaped(0.0, 1.0, FanShape::Linear, curve);
            assert_eq!(fan.at(0, 5), 0.0, "{curve:?}");
            assert!((fan.at(4, 5) - 1.0).abs() < 1e-6, "{curve:?}");
        }
        let mid = |curve| Fan::shaped(0.0, 1.0, FanShape::Linear, curve).at(1, 5);
        assert!(mid(Curve::Slow) < 0.25, "slow starts slow");
        assert!(mid(Curve::Fast) > 0.25, "fast starts fast");
        assert!((mid(Curve::Sine) - 0.1464).abs() < 1e-3);
        assert_eq!(
            Fan::shaped(0.0, 1.0, FanShape::Linear, Curve::Sine).at(2, 5),
            0.5,
            "sine is symmetric"
        );
    }

    /// Old JSON without shape/curve loads as the straight line, and the
    /// straight line saves without them.
    /// r[verify tricks.fan.shapes]
    #[test]
    fn fan_json_defaults_keep_the_old_shape() {
        let fan: Fan = serde_json::from_str(r#"{"from":0.0,"to":1.0}"#).unwrap();
        assert_eq!(fan, Fan::new(0.0, 1.0));
        assert_eq!(
            serde_json::to_string(&fan).unwrap(),
            r#"{"from":0.0,"to":1.0}"#
        );
        let shaped: Fan =
            serde_json::from_str(r#"{"from":0.0,"to":1.0,"shape":"CentreOut","curve":"Sine"}"#)
                .unwrap();
        assert_eq!(shaped.shape, FanShape::CentreOut);
        assert_eq!(shaped.curve, Curve::Sine);
    }

    /// Keyframes land on the selection: three points over five units
    /// put the middle point on the middle unit and interpolate between.
    /// r[verify tricks.keyframes]
    /// r[verify focus.magic]
    #[test]
    fn keyframes_interpolate_along_the_selection() {
        let five = apply_all(&(1..=5).collect::<Vec<_>>(), &[]);
        let k = Fan::new(0.0, 0.0).keyframes(vec![0.0, 10.0, 0.0]);
        assert_eq!(k.over(&five), vec![0.0, 5.0, 10.0, 5.0, 0.0]);
        // Five hand-set points land exactly on five units and spread
        // over twelve without per-fixture values.
        let k = Fan::new(0.0, 0.0).keyframes(vec![1.0, 2.0, 3.0, 4.0, 5.0]);
        assert_eq!(k.over(&five), vec![1.0, 2.0, 3.0, 4.0, 5.0]);
        let twelve = apply_all(&(1..=12).collect::<Vec<_>>(), &[]);
        let v = k.over(&twelve);
        assert_eq!(v.len(), 12);
        assert_eq!(v[0], 1.0);
        assert!((v[11] - 5.0).abs() < 1e-6);
        assert!(v.windows(2).all(|w| w[1] >= w[0]), "monotone: {v:?}");
        // The curve eases each segment, not the whole run.
        let eased =
            Fan::shaped(0.0, 0.0, FanShape::Linear, Curve::Slow).keyframes(vec![0.0, 1.0, 2.0]);
        let v = eased.over(&apply_all(&(1..=9).collect::<Vec<_>>(), &[]));
        assert_eq!(v[4], 1.0, "a keyframe is hit exactly");
        assert!(
            v[1] < 0.25 && v[5] < 1.25,
            "each segment starts slow: {v:?}"
        );
        // Degenerate lists are inert.
        assert_eq!(Fan::default().keyframes(vec![7.0]).at(3, 5), 7.0);
        assert_eq!(Fan::default().keyframes(Vec::new()).at(0, 5), 0.0);
    }

    /// Every named set in the shipped profile applies to a rig without
    /// panicking, and the ones with a statable unit count have it.
    // r[verify tricks.shared-or-inline]
    #[test]
    fn profile_named_tricks_apply_to_twelve() {
        let text = include_str!("../../../../data/profiles/ignition.ig-profile");
        let profile: serde_json::Value = serde_json::from_str(text).unwrap();
        let named: std::collections::BTreeMap<String, Vec<Trick>> =
            serde_json::from_value(profile["tricks"].clone()).unwrap();
        assert!(
            named.len() >= 18,
            "the named library shrank: {}",
            named.len()
        );
        let twelve: Vec<ChanId> = (1..=12).collect();
        let expected = [
            ("odds", 2),
            ("evens", 2),
            ("pairs", 6),
            ("triples", 4),
            ("quads", 3),
            ("thirds", 3),
            ("quarters", 4),
            ("mirror", 6),
            ("fan from centre", 6),
            ("paired odds", 2),
            ("mirrored pairs", 3),
            ("scatter", 12),
            ("centre out", 12),
            ("ends in", 12),
            ("reverse", 12),
            ("shifted", 12),
        ];
        for (name, count) in expected {
            let tricks = named
                .get(name)
                .unwrap_or_else(|| panic!("profile has no {name:?}"));
            assert_eq!(apply_all(&twelve, tricks).len(), count, "{name}");
        }
        for (name, tricks) in &named {
            let mut flat = apply_all(&twelve, tricks).flat();
            flat.sort();
            assert_eq!(flat, twelve, "{name} lost or invented a fixture");
        }
        // The odds really are the odds.
        assert_eq!(
            apply_all(&twelve, &named["odds"]).0[0],
            vec![1, 3, 5, 7, 9, 11]
        );
        assert_eq!(
            apply_all(&twelve, &named["evens"]).0[0],
            vec![2, 4, 6, 8, 10, 12]
        );
    }
    /// Two trusses at different heights are two layers, each in the
    /// selection's own order; the same rig at one height is one layer.
    // r[verify tricks.grid.from-space]
    // r[verify tricks.grid.degenerate-axes]
    #[test]
    fn a_lower_and_an_upper_truss_are_two_layers_in_selection_order() {
        use crate::selection::{FixtureInfo, Rig};
        use ignition_proto::{Placement, Quat, Vec3};
        let head = |chan: u32, x: f64, z: f64| FixtureInfo {
            chan,
            placement: Some(Placement {
                position: Vec3 { x, y: 0.0, z },
                orientation: Quat {
                    w: 1.0,
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
            }),
            manufacturer: String::new(),
            model: String::new(),
            tags: Vec::new(),
        };
        let mut fixtures = Vec::new();
        for i in 0..4u32 {
            fixtures.push(head(i + 1, i as f64, 3.0));
            fixtures.push(head(i + 11, i as f64, 6.0));
        }
        let rig = Rig::new(fixtures);
        // Selected right-to-left on purpose: the order must survive.
        let chans = vec![4, 3, 2, 1, 14, 13, 12, 11];
        let grid = Grid::from_rig_in_order(&chans, &rig, GridAxes::default());
        assert_eq!(grid.size, [4, 1, 2]);
        assert!(!grid.is_override());
        let cell = |chan| grid.cells.iter().find(|c| c.chan == chan).unwrap();
        assert_eq!(
            (cell(4).x, cell(4).z),
            (0, 0),
            "first selected is first along the row"
        );
        assert_eq!((cell(1).x, cell(1).z), (3, 0));
        assert_eq!((cell(14).x, cell(14).z), (0, 1));
        assert_eq!((cell(11).x, cell(11).z), (3, 1));
        // A Z trick reaches the upper truss; on one truss it is a no-op.
        let gu = apply_all_grid(&[Trick::OnAxis(Axis::Z, Box::new(Trick::Reverse))], &grid);
        assert_eq!(gu.count, [4, 1, 2]);
        let one = Grid::from_rig_in_order(&chans[..4], &rig, GridAxes::default());
        assert_eq!(one.size, [4, 1, 1]);
        let flat = apply_all_grid(&[Trick::OnAxis(Axis::Z, Box::new(Trick::Wings(2)))], &one);
        assert_eq!(
            flat.units.0.len(),
            4,
            "a Z trick on one truss changes nothing"
        );
    }

    /// One mechanism spreads a static look and a running chase alike.
    ///
    /// This is the requirement the rest of the file exists to protect.
    /// Implemented as a *kind of effect*, a fan would be unavailable to
    /// a look that is not running — so the same trick on a one-step
    /// recipe and on a phaser has to cut the selection into the same
    /// units.
    ///
    /// r[verify tricks.spread.not-an-effect]
    #[test]
    fn a_fan_cuts_a_still_look_and_a_chase_the_same_way() {
        let chans = vec![1, 2, 3, 4, 5, 6];
        let trick = [Trick::Block(2)];

        // `apply_all` knows nothing about steps, speeds or whether
        // anything is running: it is a property of how a value meets a
        // selection, which is the whole claim.
        let units = apply_all(&chans, &trick);
        assert_eq!(
            units.0,
            vec![vec![1, 2], vec![3, 4], vec![5, 6]],
            "the fan did not cut the selection into units"
        );

        // The same call is what a phaser gets; there is no second path
        // for a moving one to take.
        let again = apply_all(&chans, &trick);
        assert_eq!(units.0, again.0);
    }
}
