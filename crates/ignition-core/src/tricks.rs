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

use crate::ChanId;
use serde::{Deserialize, Serialize};

/// A way of cutting a selection into parts, or of reordering it.
///
/// Applied to an ordered list of channels and returning one — never
/// returning "sub-selections" as a distinct type, because a sub-
/// selection that is not a selection is exactly the second language this
/// is built to avoid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Trick {
    /// Contiguous runs of `n`, each acting as one unit. Ten fixtures at
    /// `Block(2)` is five units: (1,2) (3,4) (5,6) (7,8) (9,10).
    Block(usize),
    /// Round-robin into `n` parts. Ten at `Group(2)` is (1,3,5,7,9) and
    /// (2,4,6,8,10).
    ///
    /// Not a variant of `Block`, and the difference is the point:
    /// `Block(5)` and `Group(2)` both cut ten fixtures into two fives,
    /// and one is halves of the rig while the other is odds and evens.
    Group(usize),
    /// Divide into `n` parts and mirror alternate ones, so a symmetric
    /// rig runs from one definition and opens outward from centre.
    Wings(usize),
    /// Reorder pseudo-randomly. The same seed over the same count always
    /// gives the same order — a random look that cannot be recalled has
    /// been discovered, not programmed.
    Shuffle(u32),
    /// Rotate the selection by `n`, so a pattern moves along the rig
    /// without being re-authored.
    Shift(isize),
    /// Reverse. The cheap half of `Wings`, and worth having alone.
    Reverse,
}

/// A cut selection: the units, in order, each of one or more channels.
///
/// A unit is what everything downstream sees. Spreading a phase across a
/// blocked selection gives one phase per *unit*, so a pair moves
/// together — which is the entire purpose of blocking.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
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
    /// Applies this Trick to already-cut units.
    ///
    /// Taking and returning `Units` is what makes Tricks chain:
    /// `Block(2)` then `Group(2)` is five pairs dealt into two combs,
    /// and neither needs to know the other happened.
    pub fn apply(self, units: Units) -> Units {
        match self {
            Trick::Block(n) => block(units, n),
            Trick::Group(n) => group(units, n),
            Trick::Wings(n) => wings(units, n),
            Trick::Shuffle(seed) => shuffle(units, seed),
            Trick::Shift(n) => shift(units, n),
            Trick::Reverse => {
                let mut units = units;
                units.0.reverse();
                units
            }
        }
    }
}

/// Applies a chain of Tricks, left to right.
pub fn apply_all(chans: &[ChanId], tricks: &[Trick]) -> Units {
    tricks
        .iter()
        .fold(units_of(chans), |units, trick| trick.apply(units))
}

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

/// Deterministic shuffle.
///
/// A hand-rolled xorshift rather than the `rand` crate, for one reason:
/// reproducibility must survive a dependency bump. `rand`'s generators
/// are explicitly allowed to change their output between versions, so a
/// show whose look came from a seed would quietly become a different
/// show. This will produce the same order in ten years.
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
pub fn spread(from: f32, to: f32, index: usize, count: usize) -> f32 {
    if count <= 1 {
        return from;
    }
    let t = index as f32 / (count - 1) as f32;
    from + (to - from) * t
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ten() -> Vec<ChanId> {
        (1..=10).collect()
    }

    #[test]
    fn block_makes_contiguous_runs() {
        let units = apply_all(&ten(), &[Trick::Block(2)]);
        assert_eq!(units.len(), 5);
        assert_eq!(units.0[0], vec![1, 2]);
        assert_eq!(units.0[4], vec![9, 10]);
    }

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
    #[test]
    fn tricks_compose() {
        // Five pairs, dealt into two combs.
        let units = apply_all(&ten(), &[Trick::Block(2), Trick::Group(2)]);
        assert_eq!(units.len(), 2);
        assert_eq!(units.0[0], vec![1, 2, 5, 6, 9, 10]);
        assert_eq!(units.0[1], vec![3, 4, 7, 8]);
    }

    /// No fixture is invented or lost, whatever the chain.
    #[test]
    fn every_trick_preserves_the_fixtures() {
        for tricks in [
            vec![Trick::Block(3)],
            vec![Trick::Group(3)],
            vec![Trick::Wings(2)],
            vec![Trick::Shuffle(7)],
            vec![Trick::Shift(3)],
            vec![Trick::Reverse],
            vec![Trick::Block(2), Trick::Group(2), Trick::Shuffle(9)],
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
        for trick in [Trick::Block(0), Trick::Block(1), Trick::Group(1), Trick::Shuffle(0)] {
            assert_eq!(apply_all(&ten(), &[trick]).flat(), ten(), "{trick:?}");
        }
    }

    /// The same seed always gives the same order, and different seeds
    /// generally do not. Reproducibility is what makes a shuffled look
    /// programmable rather than merely discovered.
    #[test]
    fn shuffle_is_reproducible() {
        let a = apply_all(&ten(), &[Trick::Shuffle(42)]).flat();
        let b = apply_all(&ten(), &[Trick::Shuffle(42)]).flat();
        let c = apply_all(&ten(), &[Trick::Shuffle(43)]).flat();
        assert_eq!(a, b);
        assert_ne!(a, c, "two seeds gave the same order");
        assert_ne!(a, ten(), "the shuffle did not shuffle");
    }

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
    #[test]
    fn wings_mirror_alternate_parts() {
        let units = apply_all(&[1, 2, 3, 4, 5, 6], &[Trick::Wings(2)]).flat();
        assert_eq!(units, vec![1, 2, 3, 6, 5, 4]);
    }

    /// Spread is a proportion, which is what makes a show portable: the
    /// ends are the ends however many fixtures there are.
    #[test]
    fn spread_hits_both_ends_at_any_size() {
        for count in [2, 3, 8, 47] {
            assert!((spread(0.0, 1.0, 0, count) - 0.0).abs() < 1e-6);
            assert!((spread(0.0, 1.0, count - 1, count) - 1.0).abs() < 1e-6);
        }
        // A selection of one takes the start rather than dividing by zero.
        assert!((spread(0.25, 1.0, 0, 1) - 0.25).abs() < 1e-6);
    }
}
