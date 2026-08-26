//! Move-in-black and per-class timing on a generated list.
//!
//! The engine can pre-position a mover while it is dark, but only when
//! a cue asks for it. Both generators build their list and then run
//! [`set_mib`] over it, so every cue that re-aims a mover is flagged and
//! the flag is derived from the recipes rather than typed per cue.

use std::collections::{BTreeMap, BTreeSet};

use ignition_core::cue::{Mib, MibMode};
use ignition_core::{CueList, RecipeApply, RecipeRef, Selection};

use crate::generate::{Kind, kind_of};

/// Preference for a section boundary — the best moment to move into.
const SECTION_PREFERENCE: u8 = 80;
/// Preference for a lift inside a section — a moment, but a busy one.
const LIFT_PREFERENCE: u8 = 30;
/// Beats the pre-position itself takes.
const MIB_FADE_BEATS: f32 = 2.0;

/// The names a selection is built from — roles, groups, models, tags.
///
/// A fan over `movers_lr()` and an aim at `movers()` are the same
/// fixtures under different orderings, so the pass keys on the leaves
/// rather than on the whole selection.
fn leaves(selection: &Selection, out: &mut Vec<String>) {
    match selection {
        Selection::Group(n) | Selection::Role(n) | Selection::Tag(n) | Selection::Model(n) => {
            out.push(n.clone())
        }
        Selection::Union(v) | Selection::Intersect(v) => v.iter().for_each(|s| leaves(s, out)),
        Selection::Except { .. } | Selection::Where { .. } | Selection::Order { .. } => {
            let Ok(json) = serde_json::to_value(selection) else {
                return;
            };
            // Nested selections live under `of`, whatever the variant.
            fn walk(v: &serde_json::Value, out: &mut Vec<String>) {
                if let Some(of) = v.get("of")
                    && let Ok(sel) = serde_json::from_value::<Selection>(of.clone())
                {
                    leaves(&sel, out);
                } else if let Some(obj) = v.as_object() {
                    obj.values().for_each(|v| walk(v, out));
                }
            }
            walk(&json, out);
        }
        _ => {}
    }
}

fn is_focus(apply: &RecipeApply) -> bool {
    matches!(
        apply,
        RecipeApply::FocusPoint(_)
            | RecipeApply::FocusDirection(_)
            | RecipeApply::FocusFan { .. }
            | RecipeApply::FocusKeyframes(_)
            | RecipeApply::FocusDelta(_)
            | RecipeApply::FocusSplay { .. }
            | RecipeApply::FocusPerFixture { .. }
            | RecipeApply::FocusAxes { .. }
            | RecipeApply::FocusRelative { .. }
    )
}

/// What one cue does to the fixtures that ever get aimed: the focus it
/// sets per leaf, and the leaves it sends to zero without aiming.
#[derive(Default)]
struct Moves {
    aims: BTreeMap<String, String>,
    darkens: BTreeSet<String>,
}

fn moves(recipes: &[RecipeRef]) -> Moves {
    let mut m = Moves::default();
    for recipe in recipes.iter().filter_map(RecipeRef::inline) {
        let mut names = Vec::new();
        leaves(&recipe.target, &mut names);
        for apply in recipe.steps.iter().flat_map(|s| s.apply.iter()) {
            if is_focus(apply) {
                let key = serde_json::to_string(apply).unwrap_or_default();
                for n in &names {
                    m.aims.insert(n.clone(), key.clone());
                }
            } else if matches!(apply, RecipeApply::Dimmer(v) if *v <= 0.0) {
                m.darkens.extend(names.iter().cloned());
            }
        }
    }
    m.darkens.retain(|n| !m.aims.contains_key(n));
    m
}

/// Does this cue aim movers somewhere the previous aiming cue did not?
pub fn reaims(list: &CueList, index: usize) -> bool {
    let mut last: BTreeMap<String, String> = BTreeMap::new();
    for cue in &list.cues[..index] {
        last.extend(moves(&cue.recipes).aims);
    }
    moves(&list.cues[index].recipes)
        .aims
        .iter()
        .any(|(leaf, focus)| last.get(leaf) != Some(focus))
}

/// Flags pre-positioning on every cue that re-aims a mover.
///
/// A cue whose focus for a fixture differs from the last cue that aimed
/// it gets `Early` — the mover swings as soon as it is dark — with a
/// two-beat fade. Where the previous mover cue already sent them to
/// zero, that dark window is the move and the mode is left at its
/// default for the engine to place. Section cues are rated a good
/// moment to move into; lifts a poor one.
// r[impl cues.generator-emits-mib]
// r[impl cues.mib.mode] - early where the fixture is not already parked dark
// r[impl cues.mib.timing] - the pre-position's own fade
// r[impl cues.mib.preference] - sections over lifts
pub fn set_mib(list: &mut CueList) {
    let mut last: BTreeMap<String, String> = BTreeMap::new();
    let mut dark: BTreeSet<String> = BTreeSet::new();
    for cue in &mut list.cues {
        let m = moves(&cue.recipes);
        let changed: Vec<&String> = m
            .aims
            .iter()
            .filter(|(leaf, focus)| last.get(*leaf) != Some(*focus))
            .map(|(leaf, _)| leaf)
            .collect();
        if !changed.is_empty() {
            let parked = changed.iter().all(|leaf| dark.contains(*leaf));
            cue.mib = Mib {
                mode: if parked {
                    MibMode::default()
                } else {
                    MibMode::Early
                },
                fade_beats: MIB_FADE_BEATS,
                ..Default::default()
            };
            set_preference(
                &mut cue.mib,
                if cue.block {
                    SECTION_PREFERENCE
                } else {
                    LIFT_PREFERENCE
                },
            );
        }
        for leaf in m.aims.keys() {
            dark.remove(leaf);
        }
        dark.extend(m.darkens.iter().filter(|l| last.contains_key(*l)).cloned());
        last.extend(m.aims);
    }
}

fn set_preference(mib: &mut Mib, preference: u8) {
    mib.preference = preference;
}

/// Per-class timing from the design guide, by section kind.
///
/// Colour snaps into a chorus; movers drift over a bar in a verse or
/// bridge. Only section cues are touched — a lift's timing is its own.
// r[impl cues.timing.per-attribute] - generator defaults by section kind
pub fn set_class_timing(list: &mut CueList) {
    for cue in list.cues.iter_mut().filter(|c| c.block) {
        match kind_of(&cue.name) {
            Kind::Chorus => cue.timing.color = Some(0.0),
            Kind::Verse | Kind::Bridge => cue.timing.position = Some(4.0),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ignition_core::preset::Ref;
    use ignition_core::{Cue, Recipe};

    fn movers() -> Selection {
        Selection::Role("Movers".into())
    }
    fn aim(focus: &str) -> RecipeRef {
        Recipe::new(movers(), RecipeApply::FocusPoint(Ref::Named(focus.into()))).into()
    }
    fn dark() -> RecipeRef {
        Recipe::new(movers(), RecipeApply::Dimmer(0.0)).into()
    }
    fn cue(name: &str, recipes: Vec<RecipeRef>) -> Cue {
        Cue {
            name: name.into(),
            recipes,
            block: !name.starts_with('·'),
            ..Default::default()
        }
    }
    fn list(cues: Vec<Cue>) -> CueList {
        CueList {
            name: "t".into(),
            cues,
            triggers: Vec::new(),
            ..Default::default()
        }
    }

    /// r[verify cues.generator-emits-mib]
    /// r[verify cues.mib.mode]
    #[test]
    fn a_reaim_is_early_a_repeat_is_nothing_and_a_parked_move_is_left_to_the_engine() {
        let mut l = list(vec![
            cue("VS 1", vec![aim("Band")]),
            cue("· lift", vec![]),
            cue("PRE", vec![aim("Band")]),
            cue("· PRE lift", vec![aim("Sky")]),
            cue("CH 1", vec![aim("House")]),
            cue("BR", vec![dark()]),
            cue("CH 2", vec![aim("Drums")]),
        ]);
        set_mib(&mut l);
        let modes: Vec<_> = l.cues.iter().map(|c| c.mib).collect();
        assert_eq!(modes[0].mode, MibMode::Early);
        assert_eq!(modes[0].fade_beats, MIB_FADE_BEATS);
        assert_eq!(modes[1], Mib::default(), "a lift that does not aim");
        assert_eq!(modes[2], Mib::default(), "same focus as before");
        assert_eq!(modes[3].preference, LIFT_PREFERENCE, "a lift that re-aims");
        assert_eq!(modes[4].mode, MibMode::Early);
        assert_eq!(modes[5], Mib::default(), "going dark is not a move");
        assert_eq!(
            modes[6].mode,
            MibMode::default(),
            "parked dark: engine chooses"
        );
        assert_eq!(modes[6].fade_beats, MIB_FADE_BEATS);
        assert_eq!(modes[0].preference, SECTION_PREFERENCE);
        assert!(reaims(&l, 6));
        assert!(!reaims(&l, 2));
    }

    /// r[verify cues.timing.per-attribute]
    #[test]
    fn choruses_snap_colour_and_verses_drift_position() {
        let mut l = list(vec![
            cue("VS 1", vec![]),
            cue("CH 1", vec![]),
            cue("· x", vec![]),
        ]);
        set_class_timing(&mut l);
        assert_eq!(l.cues[0].timing.position, Some(4.0));
        assert_eq!(l.cues[0].timing.color, None);
        assert_eq!(l.cues[1].timing.color, Some(0.0));
        assert_eq!(l.cues[2].timing, Default::default());
    }
}
