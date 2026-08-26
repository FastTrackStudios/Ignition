//! Recipes — this project's foundation for building cues, the same role
//! grandMA3's Preset system plays: a `Recipe` pairs a *target* (a `Group`
//! or an explicit channel list) with an *apply* (a `Dimmer` level, a
//! `Color`, a `FocusPoint`, or a `Raw` attribute list).
//!
//! A recipe is **stored into** a `Cue` and resolved at output time, not
//! flattened into values when the show loads. That distinction is the
//! whole feature: a stored recipe still knows it targets a *group*, so
//! adding a fixture to that group changes what every cue using it covers
//! with no re-authoring, and the recipe stays something an editor can
//! show and change. `expand_recipe` is the resolver `CuePlayer` calls;
//! see `docs/domain/cue-building-architecture.md` for why resolution
//! lives there rather than at load.
//!
//! grandMA3 also has "Phaser" recipes — effect generators (waveforms
//! driving an attribute across a group's fixtures with per-fixture phase
//! offset) — which are a genuinely different kind of thing (a continuous
//! function of time, not a fixed target state) and are **not** built here;
//! this module is the static-cue half of the roadmap the operator laid
//! out (`docs/research/lighting-console-landscape.md`'s cue-list Slice),
//! Phasers are the deliberately deferred next slice.

use crate::cue::{Cue, CueValue};
use crate::focus::{pan_tilt_deg_along, pan_tilt_deg_to_point};
use crate::group::Group;
use crate::preset::{ColorPreset, Palettes, Ref};
use crate::selection::{Rig, Selection, resolve, unresolved_names};
use crate::step::{Play, Speed, SpeedMasters, Step, Timing, Waveform, locate};
use ignition_proto::{Attribute, ChanId, ColorChannel};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// What to apply to the target.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum RecipeApply {
    Dimmer(f32),
    /// A pooled colour by name (`"House Blue"`), or one written inline.
    /// A name the venue's palette does not carry resolves to nothing and
    /// the recipe is skipped, same tolerance as an unknown group.
    Color(Ref<ColorPreset>),
    /// A real XYZ room location — resolved per-fixture via each target
    /// fixture's actual `Placement` (see `focus.rs`), not a single shared
    /// pan/tilt value. Fixtures with no known `Placement` (an unpatched or
    /// unrecognized channel) are silently skipped, same tolerance as the
    /// rest of this module.
    FocusPoint(Ref<ignition_proto::Vec3>),
    /// A shared world-space *direction* rather than a shared point, so
    /// every fixture in the group ends up beam-parallel with the others
    /// instead of converging. Not expressible as a `FocusPoint` at any
    /// finite distance. Fixtures with no known `Placement` are skipped,
    /// same as `FocusPoint`.
    FocusDirection(ignition_proto::Vec3),
    /// Escape hatch for anything not modelled as its own `RecipeApply`
    /// variant yet — the same role `Attribute::Custom` plays one level
    /// down.
    Raw(Vec<(Attribute, f32)>),
    /// Adds to whatever a lower layer already set instead of replacing
    /// it.
    ///
    /// This is what makes a phaser composable. An intensity chase over a
    /// coloured wash used to have to restate the colour, because the
    /// effect's output overwrote everything it touched. A `Delta` says
    /// "−40% dimmer" and what is underneath is simply not its business —
    /// which is MA3's absolute/relative split, and the reason a phaser
    /// can live in the same cue as the look it modulates.
    Delta(Vec<(Attribute, f32)>),
}

/// A parametric template: who, what, and — with more than one step —
/// when.
///
/// One step is a static look. Two or more is a phaser. That is the whole
/// distinction; there is no separate effect type. See
/// `docs/domain/cue-building-architecture.md`, Decision 3.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(from = "RecipeWire", into = "RecipeWire")]
pub struct Recipe {
    pub target: Selection,
    pub steps: Vec<Step>,
    pub timing: Timing,
    /// How the target is cut before the steps meet it.
    ///
    /// Inline on the recipe rather than a separate stage, because that
    /// is what a Trick *is*: grandMA3 carries them as columns on the
    /// recipe line, and it is why one recipe there covers looks that
    /// need a dozen here. `Block(2)` makes the phase spread land per
    /// pair; `Group(2)` makes it land on odds and evens.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tricks: Vec<crate::tricks::Trick>,
}

impl Recipe {
    /// The common case: one thing applied to one selection, no timing.
    pub fn new(target: Selection, apply: RecipeApply) -> Self {
        Self {
            target,
            steps: vec![Step::new(vec![apply])],
            timing: Timing::default(),
            tricks: Vec::new(),
        }
    }

    /// True when this recipe is a phaser rather than a static look.
    pub fn is_phaser(&self) -> bool {
        self.steps.len() > 1
    }
}

/// The on-disk shape, which offers three spellings of the same thing.
///
/// `apply` is the terse one-step form every show file in this repo was
/// written in, and it keeps working unchanged. `waveform` is the
/// ergonomic spelling of a periodic phaser — "sine" is a worse thing to
/// say as a step table than as the word. `steps` is the general form
/// both of the others expand into, so the runtime only ever sees one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct RecipeWire {
    target: Selection,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    apply: Option<RecipeApply>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    waveform: Option<WaveformWire>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    steps: Vec<Step>,
    #[serde(default, skip_serializing_if = "is_default_timing")]
    timing: Timing,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    tricks: Vec<crate::tricks::Trick>,
}

fn is_default_timing(t: &Timing) -> bool {
    *t == Timing::default()
}

/// `{"shape": "Sine", "attr": "Dimmer", "base": 0.5, "size": 0.5}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct WaveformWire {
    shape: Waveform,
    attr: Attribute,
    base: f32,
    size: f32,
    /// Modulate what a lower layer already set rather than replacing it
    /// — see `RecipeApply::Delta`.
    #[serde(default)]
    relative: bool,
}

impl From<RecipeWire> for Recipe {
    fn from(w: RecipeWire) -> Self {
        let steps = if !w.steps.is_empty() {
            w.steps
        } else if let Some(wave) = w.waveform {
            wave.shape
                .steps(wave.attr, wave.base, wave.size, wave.relative)
        } else if let Some(apply) = w.apply {
            vec![Step::new(vec![apply])]
        } else {
            Vec::new()
        };
        Self {
            target: w.target,
            steps,
            timing: w.timing,
            tricks: w.tricks,
        }
    }
}

impl From<Recipe> for RecipeWire {
    fn from(r: Recipe) -> Self {
        // Round-trip back to the terse spelling when that is all it is,
        // so re-saving a hand-written show does not explode it into
        // step tables nobody asked for.
        let terse = match r.steps.as_slice() {
            [step] if step.apply.len() == 1 && step.transition == 0.0 => {
                Some(step.apply[0].clone())
            }
            _ => None,
        };
        Self {
            target: r.target,
            apply: terse.clone(),
            waveform: None,
            steps: if terse.is_some() { Vec::new() } else { r.steps },
            timing: r.timing,
            tricks: r.tricks,
        }
    }
}

/// Everything expanding a recipe needs to know about the room: who the
/// groups are, what the palettes mean, and where each fixture is hung.
///
/// Bundled into one struct rather than passed as three parallel
/// arguments because every one of them is a property of the *venue*, they
/// are always supplied together, and effects will want the same set.
/// `rig` is flat records rather than a venue-loader callback, so
/// `ignition-core` keeps its no-I/O rule while still being able to answer
/// "which fixtures are tagged `mover`" — a question a
/// `Fn(ChanId) -> Placement` closure cannot be asked, and the reason
/// `Selection::Tag`/`Model` are possible at all.
pub struct Show<'a> {
    pub groups: &'a [Group],
    pub palettes: &'a Palettes,
    pub rig: &'a Rig,
    /// Named tempo sources every phaser in the show can slave to.
    pub speeds: &'a SpeedMasters,
    /// What the venue binds each profile role to.
    ///
    /// The seam that makes a cue portable: a recipe targeting
    /// `Role("Key")` resolves to whatever fixtures *this* room calls its
    /// key light. Defaults to no bindings, so a show written against
    /// venue group names behaves exactly as it did.
    pub roles: &'a dyn crate::selection::Roles,
}

/// No role bindings — every `Selection::Role` resolves to nothing.
pub static NO_ROLES: () = ();

/// No tempo sources — every `Speed::Master` resolves to stopped.
pub static NO_SPEEDS: std::sync::LazyLock<SpeedMasters> =
    std::sync::LazyLock::new(SpeedMasters::new);

impl<'a> Show<'a> {
    /// A show with no palettes — for tests and for a venue that has not
    /// been given a palette file yet.
    pub fn new(groups: &'a [Group], rig: &'a Rig) -> Self {
        Self {
            groups,
            palettes: Palettes::EMPTY,
            rig,
            speeds: &NO_SPEEDS,
            roles: &NO_ROLES,
        }
    }
}

/// One resolved value, and whether it replaces what is underneath or
/// adds to it.
#[derive(Debug, Clone, PartialEq)]
pub struct Emit {
    pub value: CueValue,
    /// `true` for a `RecipeApply::Delta` — the caller adds this on top
    /// of whatever won the cascade rather than letting it compete.
    pub relative: bool,
}

/// Resolves one apply, for one channel, into concrete attribute values.
fn apply_values(
    apply: &RecipeApply,
    chan: ChanId,
    show: &Show<'_>,
) -> (Vec<(Attribute, f32)>, bool) {
    match apply {
        RecipeApply::Dimmer(value) => (vec![(Attribute::Dimmer, *value)], false),
        RecipeApply::Color(reference) => match show.palettes.resolve_color(reference) {
            Some(c) => (
                vec![
                    (
                        Attribute::ColorAdd {
                            channel: ColorChannel::Red,
                        },
                        c.red,
                    ),
                    (
                        Attribute::ColorAdd {
                            channel: ColorChannel::Green,
                        },
                        c.green,
                    ),
                    (
                        Attribute::ColorAdd {
                            channel: ColorChannel::Blue,
                        },
                        c.blue,
                    ),
                ],
                false,
            ),
            None => (Vec::new(), false),
        },
        RecipeApply::FocusPoint(reference) => {
            let pair = show
                .palettes
                .resolve_focus(reference)
                .zip(show.rig.placement(chan))
                .map(|(target, p)| pan_tilt_deg_to_point(p.position, p.orientation, target));
            match pair {
                Some((pan, tilt)) => (vec![(Attribute::Pan, pan), (Attribute::Tilt, tilt)], false),
                None => (Vec::new(), false),
            }
        }
        RecipeApply::FocusDirection(dir) => match show.rig.placement(chan) {
            Some(p) => {
                let (pan, tilt) = pan_tilt_deg_along(p.orientation, *dir);
                (vec![(Attribute::Pan, pan), (Attribute::Tilt, tilt)], false)
            }
            None => (Vec::new(), false),
        },
        RecipeApply::Raw(values) => (values.clone(), false),
        RecipeApply::Delta(values) => (values.clone(), true),
    }
}

/// Everything one step sets for one channel, keyed so two steps can be
/// interpolated attribute by attribute.
fn step_values(
    step: &Step,
    chan: ChanId,
    show: &Show<'_>,
) -> std::collections::HashMap<(Attribute, bool), f32> {
    let mut out = std::collections::HashMap::new();
    for apply in &step.apply {
        let (pairs, relative) = apply_values(apply, chan, show);
        for (attr, value) in pairs {
            out.insert((attr, relative), value);
        }
    }
    out
}

/// Resolves a recipe at `secs` into the values it produces right now.
///
/// For a one-step recipe `secs` is ignored and this is a pure template
/// expansion. For a phaser it is the show clock: each fixture's cycle
/// position comes from its index in the selection (phase spread) and the
/// recipe's speed, and the step either side of that position is blended
/// according to the step's transition and ease.
pub fn expand_recipe(recipe: &Recipe, show: &Show<'_>, secs: f32) -> Vec<Emit> {
    let mut out = Vec::new();
    if recipe.steps.is_empty() {
        return out;
    }
    // Roles first, so a recipe targeting `Key` finds whatever this venue
    // calls its key light, then Tricks, so the phase spread lands on the
    // *units* the Tricks made rather than on raw fixtures.
    let chans = crate::selection::resolve_with(&recipe.target, show.groups, show.rig, show.roles);
    let units = crate::tricks::apply_all(&chans, &recipe.tricks);
    let count = units.len();
    let phaser = recipe.is_phaser();

    // `Negative` inverts each attribute about its own swing, so the
    // extremes have to be known before any fixture is resolved. Computed
    // once rather than per fixture — the step table is the same for all
    // of them.
    let swing = (recipe.timing.direction == Play::Negative).then(|| swing_of(recipe, show));

    for (index, unit) in units.0.iter().enumerate() {
        let (prev, cur, blend) = if !phaser {
            (0, 0, 1.0)
        } else if recipe.timing.direction == Play::Build {
            // Build is not a phase shift of a shared waveform: a fixture
            // arrives when the cycle passes it and *stays* until the
            // wrap, so the selection fills up and then resets. That is a
            // threshold against the fixture's own position in the
            // selection, not a position in the step list — which is why
            // it is a mode rather than something a step table can say.
            let cycles = recipe.timing.cycles(secs, show.speeds);
            let u = cycles - cycles.floor();
            let arrived = u >= recipe.timing.spread_fraction(index, count);
            let last = recipe.steps.len() - 1;
            if arrived {
                (last, last, 1.0)
            } else {
                (0, 0, 1.0)
            }
        } else {
            let cycles = recipe.timing.cycles_at(secs, index, count, show.speeds);
            locate(&recipe.steps, cycles)
        };

        // Phase was decided per *unit* above; values are still resolved
        // per fixture, because a step can say something fixture-relative
        // — a focus point is a different pan and tilt for every head
        // pointing at it. So a blocked pair shares a moment and still
        // aims individually, which is what blocking is supposed to mean.
        for chan in unit.iter().copied() {
            let to = step_values(&recipe.steps[cur], chan, show);
            // Resolving the outgoing step is only worth it mid-transition.
            let from = if blend < 1.0 && prev != cur {
                step_values(&recipe.steps[prev], chan, show)
            } else {
                Default::default()
            };

            for (key, target) in &to {
                // An attribute the outgoing step did not set has nothing
                // to move away from, so it takes this step's value.
                let value = match from.get(key) {
                    Some(start) => start + (target - start) * blend,
                    None => *target,
                };
                let value = match &swing {
                    // Reflect about the middle of this attribute's own
                    // range: the fixture that would have been brightest
                    // is now the dark one travelling across the rig.
                    Some(swing) => match swing.get(&key.0) {
                        Some((lo, hi)) => lo + hi - value,
                        None => value,
                    },
                    None => value,
                };
                out.push(Emit {
                    value: CueValue {
                        chan,
                        attr: key.0.clone(),
                        value,
                    },
                    relative: key.1,
                });
            }
        }
    }
    out
}

/// The lowest and highest value each attribute takes across the step
/// table, which is what `Play::Negative` reflects about.
///
/// Resolved against the first channel of the selection: the values that
/// vary per fixture are focus points, and inverting a *position* is not
/// what negative means to anyone.
fn swing_of(recipe: &Recipe, show: &Show<'_>) -> HashMap<Attribute, (f32, f32)> {
    let chan = resolve(&recipe.target, show.groups, show.rig)
        .first()
        .copied()
        .unwrap_or(0);
    let mut swing: HashMap<Attribute, (f32, f32)> = HashMap::new();
    for step in &recipe.steps {
        for ((attr, _), value) in step_values(step, chan, show) {
            let entry = swing.entry(attr).or_insert((value, value));
            entry.0 = entry.0.min(value);
            entry.1 = entry.1.max(value);
        }
    }
    swing
}

/// Every name in `cues` that this venue cannot resolve, as readable
/// one-liners.
///
/// Expansion deliberately treats an unknown group or palette entry as
/// "no fixtures" rather than an error, which is the right runtime
/// behaviour — a show should not go dark because one cue names a group
/// this room does not have. But it is a miserable *authoring* behaviour:
/// a typo'd group name is a cue that silently does nothing, and the only
/// symptom is lights that never come on. So the tolerance stays and the
/// diagnosis is reported separately, for the loader to print.
pub fn unresolved(cues: &[Cue], show: &Show<'_>) -> Vec<String> {
    let mut out = Vec::new();
    for cue in cues {
        for recipe in &cue.recipes {
            for problem in unresolved_names(&recipe.target, show.groups, show.rig) {
                out.push(format!("cue {:?}: {problem}", cue.name));
            }
            if let Speed::Master(name) = &recipe.timing.speed
                && !show.speeds.contains_key(name)
            {
                out.push(format!("cue {:?}: no speed master {name:?}", cue.name));
            }
            for apply in recipe.steps.iter().flat_map(|s| &s.apply) {
                match apply {
                    RecipeApply::Color(Ref::Named(name)) if show.palettes.color(name).is_none() => {
                        out.push(format!("cue {:?}: no colour palette {:?}", cue.name, name));
                    }
                    RecipeApply::FocusPoint(Ref::Named(name))
                        if show.palettes.focus(name).is_none() =>
                    {
                        out.push(format!("cue {:?}: no focus palette {:?}", cue.name, name));
                    }
                    _ => {}
                }
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

// ---------------------------------------------------------------------
// Cooked status
// ---------------------------------------------------------------------

/// What one recipe resolved to.
///
/// grandMA3 shows this as a coloured pot beside every recipe in the cue
/// sheet, and it is worth stealing outright: it answers "is this cue's
/// content actually going to do what I think it does" at a glance. That
/// is a small feature with an outsized effect on trust, and trust in
/// what the desk is about to do is most of what an operator is buying.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cook {
    /// Resolved to this many fixtures — MA3's green pot.
    Ok(usize),
    /// Resolved to nothing: a group this room lacks, a spatial filter
    /// that excluded everything, a palette name that does not exist.
    /// MA3's red pot. Not an error — the show still runs — but almost
    /// always a mistake, and invisible without this.
    Empty,
}

/// A whole cue's cooked state.
#[derive(Debug, Clone, PartialEq)]
pub struct CueCook {
    pub name: String,
    pub recipes: Vec<Cook>,
    /// How many direct (layer 1) values the cue carries.
    pub direct: usize,
}

/// The one-glance verdict, matching MA3's pot colours.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Every recipe resolved, and nothing but recipes — green.
    Cooked,
    /// At least one recipe resolved to nothing — red.
    Failed,
    /// Recipe output *and* hand-placed direct values — orange. Not a
    /// problem, but worth knowing: part of this cue will not follow a
    /// rig change the way the rest of it will.
    Mixed,
    /// Direct values only. Nothing generative to go wrong.
    Direct,
    /// Sets nothing at all — a blackout, or a mistake.
    Empty,
}

impl CueCook {
    pub fn status(&self) -> Status {
        if self.recipes.contains(&Cook::Empty) {
            Status::Failed
        } else if self.recipes.is_empty() {
            if self.direct == 0 {
                Status::Empty
            } else {
                Status::Direct
            }
        } else if self.direct > 0 {
            Status::Mixed
        } else {
            Status::Cooked
        }
    }

    /// A compact marker for a cue sheet or a status line.
    ///
    /// MA3 shows these as coloured pots; this is the monochrome port.
    /// Drawing them needs a real font — Bevy's built-in default is a
    /// subset with none of these glyphs, which is why `flake.nix`
    /// supplies DejaVu and `ignition-viz/build.rs` embeds it.
    pub fn marker(&self) -> char {
        match self.status() {
            Status::Cooked => '\u{25cf}', // ● full
            Status::Failed => '\u{2716}', // ✖ failed
            Status::Mixed => '\u{25d0}',  // ◐ half
            Status::Direct => '\u{25cb}', // ○ empty
            Status::Empty => '\u{00b7}',  // · nothing
        }
    }
}

/// Cooks one cue without firing it — how a cue sheet shows status for
/// cues that have not played yet.
pub fn cook_cue(cue: &Cue, show: &Show<'_>, secs: f32) -> CueCook {
    CueCook {
        name: cue.name.clone(),
        recipes: cue
            .recipes
            .iter()
            .map(|r| {
                // Count fixtures, not emitted values: one recipe can set
                // three colour channels per fixture, and "3 fixtures" is
                // what an operator wants to read.
                let emits = expand_recipe(r, show, secs);
                let fixtures: std::collections::HashSet<ChanId> =
                    emits.iter().map(|e| e.value.chan).collect();
                if fixtures.is_empty() {
                    Cook::Empty
                } else {
                    Cook::Ok(fixtures.len())
                }
            })
            .collect(),
        direct: cue.values.len(),
    }
}

pub fn cook_list(cues: &[Cue], show: &Show<'_>, secs: f32) -> Vec<CueCook> {
    cues.iter().map(|c| cook_cue(c, show, secs)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::selection::{FixtureInfo, Rig};
    use crate::step::{Ease, Speed};
    use ignition_proto::{Placement, Quat, Vec3};

    fn groups() -> Vec<Group> {
        vec![Group {
            name: "Pars".to_string(),
            chans: vec![1, 2, 3],
        }]
    }

    fn bare<'a>(groups: &'a [Group]) -> Show<'a> {
        Show::new(groups, &crate::selection::EMPTY_RIG)
    }

    fn palettes() -> Palettes {
        Palettes {
            colors: vec![ColorPreset {
                name: "House Blue".to_string(),
                red: 0.1,
                green: 0.2,
                blue: 1.0,
            }],
            focus: vec![crate::preset::FocusPointPreset {
                name: "Drums".to_string(),
                target: Vec3 {
                    x: 1.0,
                    y: 2.0,
                    z: 3.0,
                },
            }],
        }
    }

    fn dimmer_of(emits: &[Emit], chan: ChanId) -> Option<f32> {
        emits
            .iter()
            .find(|e| e.value.chan == chan && e.value.attr == Attribute::Dimmer)
            .map(|e| e.value.value)
    }

    #[test]
    fn a_group_target_resolves_to_its_real_channels() {
        let recipe = Recipe::new(
            Selection::Group("Pars".to_string()),
            RecipeApply::Dimmer(0.8),
        );
        let groups = groups();
        let emits = expand_recipe(&recipe, &bare(&groups), 0.0);
        assert_eq!(emits.len(), 3);
        assert!(emits.iter().all(|e| e.value.attr == Attribute::Dimmer));
        assert_eq!(dimmer_of(&emits, 2), Some(0.8));
    }

    #[test]
    fn an_unknown_group_name_resolves_to_no_fixtures_not_an_error() {
        let recipe = Recipe::new(
            Selection::Group("Nonexistent".to_string()),
            RecipeApply::Dimmer(1.0),
        );
        let groups = groups();
        assert!(expand_recipe(&recipe, &bare(&groups), 0.0).is_empty());
    }

    #[test]
    fn a_color_recipe_emits_red_green_blue_per_channel() {
        let recipe = Recipe::new(
            Selection::Chans(vec![5]),
            RecipeApply::Color(Ref::Inline(ColorPreset {
                name: "Amber".to_string(),
                red: 1.0,
                green: 0.5,
                blue: 0.0,
            })),
        );
        let emits = expand_recipe(&recipe, &bare(&[]), 0.0);
        assert_eq!(emits.len(), 3);
        let find = |c: ColorChannel| {
            emits
                .iter()
                .find(|e| e.value.attr == Attribute::ColorAdd { channel: c })
                .map(|e| e.value.value)
        };
        assert_eq!(find(ColorChannel::Red), Some(1.0));
        assert_eq!(find(ColorChannel::Green), Some(0.5));
        assert_eq!(find(ColorChannel::Blue), Some(0.0));
    }

    fn one_fixture_at(z: f64) -> Rig {
        Rig::new(vec![FixtureInfo {
            chan: 7,
            placement: Some(Placement {
                position: Vec3 { x: 0.0, y: 0.0, z },
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
        }])
    }

    #[test]
    fn a_focus_point_recipe_resolves_real_pan_tilt_from_the_fixtures_placement() {
        let recipe = Recipe::new(
            Selection::Chans(vec![7]),
            // Straight below the fixture.
            RecipeApply::FocusPoint(Ref::Inline(Vec3 {
                x: 0.0,
                y: 0.0,
                z: -5.0,
            })),
        );
        let rig = one_fixture_at(5.0);
        let emits = expand_recipe(&recipe, &Show::new(&[], &rig), 0.0);
        let get = |a: Attribute| {
            emits
                .iter()
                .find(|e| e.value.attr == a)
                .map(|e| e.value.value)
                .unwrap()
        };
        assert!(get(Attribute::Pan).abs() < 0.5);
        assert!(get(Attribute::Tilt).abs() < 0.5);
    }

    #[test]
    fn a_focus_point_recipe_skips_a_channel_with_no_known_placement() {
        let recipe = Recipe::new(
            Selection::Chans(vec![99]),
            RecipeApply::FocusPoint(Ref::Inline(Vec3 {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            })),
        );
        assert!(expand_recipe(&recipe, &bare(&[]), 0.0).is_empty());
    }

    #[test]
    fn a_named_colour_resolves_against_the_venues_palette() {
        let recipe = Recipe::new(
            Selection::Chans(vec![5]),
            RecipeApply::Color(Ref::Named("House Blue".to_string())),
        );
        let pool = palettes();
        let show = Show {
            groups: &[],
            palettes: &pool,
            rig: &crate::selection::EMPTY_RIG,
            speeds: &NO_SPEEDS,
            roles: &crate::recipe::NO_ROLES,
        };
        let emits = expand_recipe(&recipe, &show, 0.0);
        assert_eq!(emits.len(), 3);
        assert!(emits.iter().any(|e| {
            e.value.attr
                == Attribute::ColorAdd {
                    channel: ColorChannel::Blue,
                }
                && e.value.value == 1.0
        }));
    }

    /// The runtime must not go dark over a typo, but the loader has to be
    /// able to say so — the split `unresolved` exists for.
    #[test]
    fn an_unknown_palette_name_is_skipped_but_reported() {
        let cue = Cue {
            name: "Oops".to_string(),
            recipes: vec![Recipe::new(
                Selection::Chans(vec![5]),
                RecipeApply::Color(Ref::Named("Chartreuse".to_string())),
            )],
            ..Default::default()
        };
        let pool = palettes();
        let show = Show {
            groups: &[],
            palettes: &pool,
            rig: &crate::selection::EMPTY_RIG,
            speeds: &NO_SPEEDS,
            roles: &crate::recipe::NO_ROLES,
        };
        assert!(
            cue.recipes
                .iter()
                .all(|r| expand_recipe(r, &show, 0.0).is_empty())
        );
        let problems = unresolved(std::slice::from_ref(&cue), &show);
        assert_eq!(problems.len(), 1);
        assert!(problems[0].contains("Chartreuse"), "{problems:?}");
    }

    // -----------------------------------------------------------------
    // Steps, phasers and relative values
    // -----------------------------------------------------------------

    fn phaser(lo: f32, hi: f32, relative: bool) -> Recipe {
        let make = |v: f32| {
            let pair = vec![(Attribute::Dimmer, v)];
            vec![if relative {
                RecipeApply::Delta(pair)
            } else {
                RecipeApply::Raw(pair)
            }]
        };
        Recipe {
            target: Selection::Group("Pars".to_string()),
            steps: vec![Step::new(make(lo)), Step::new(make(hi))],
            timing: Timing {
                speed: Speed::Hz(1.0),
                ..Default::default()
            },
            tricks: Vec::new(),
        }
    }

    #[test]
    fn one_step_is_static_and_two_is_a_phaser() {
        assert!(!Recipe::new(Selection::Chans(vec![1]), RecipeApply::Dimmer(1.0)).is_phaser());
        assert!(phaser(0.0, 1.0, false).is_phaser());
    }

    #[test]
    fn a_phaser_moves_through_its_steps_over_time() {
        let recipe = phaser(0.0, 1.0, false);
        let groups = groups();
        let show = bare(&groups);
        // One cycle per second, two snapping steps: the first half of
        // each second is step 0, the second half step 1.
        assert_eq!(dimmer_of(&expand_recipe(&recipe, &show, 0.1), 1), Some(0.0));
        assert_eq!(dimmer_of(&expand_recipe(&recipe, &show, 0.6), 1), Some(1.0));
        assert_eq!(dimmer_of(&expand_recipe(&recipe, &show, 1.1), 1), Some(0.0));
    }

    /// The reason `Order` in `selection.rs` matters: spread walks the
    /// selection in order, so three fixtures a third of a cycle apart
    /// are never all on the same step.
    #[test]
    fn phase_spread_puts_each_fixture_at_a_different_point() {
        let mut recipe = phaser(0.0, 1.0, false);
        recipe.timing.phase_spread_deg = 360.0;
        let groups = groups();
        let emits = expand_recipe(&recipe, &bare(&groups), 0.1);
        assert_eq!(dimmer_of(&emits, 1), Some(0.0), "index 0, no offset");
        assert_eq!(dimmer_of(&emits, 2), Some(0.0), "index 1, a third round");
        assert_eq!(dimmer_of(&emits, 3), Some(1.0), "index 2, two thirds round");
    }

    #[test]
    fn a_transition_interpolates_rather_than_snapping() {
        let mut recipe = phaser(0.0, 1.0, false);
        for step in &mut recipe.steps {
            step.transition = 1.0;
            step.ease = Ease::Linear;
        }
        let groups = groups();
        let show = bare(&groups);
        // Step 0 owns the first half-cycle and transitions *into* its
        // own value from step 1's across the whole slice. So at 0.25 —
        // halfway through that slice — the value is halfway between
        // step 1's 1.0 and step 0's 0.0.
        let mid = dimmer_of(&expand_recipe(&recipe, &show, 0.25), 1).unwrap();
        assert!((mid - 0.5).abs() < 0.01, "{mid}");
        let early = dimmer_of(&expand_recipe(&recipe, &show, 0.125), 1).unwrap();
        assert!((early - 0.75).abs() < 0.01, "{early}");
    }

    /// The claim `Waveform` rests on: two eased steps really do trace a
    /// sine, so it can be sugar rather than a parallel engine.
    #[test]
    fn a_sine_waveform_traces_a_real_sine() {
        let recipe = Recipe {
            target: Selection::Chans(vec![1]),
            steps: Waveform::Sine.steps(Attribute::Dimmer, 0.5, 0.5, false),
            timing: Timing {
                speed: Speed::Hz(1.0),
                ..Default::default()
            },
            tricks: Vec::new(),
        };
        let show = bare(&[]);
        let at = |t: f32| dimmer_of(&expand_recipe(&recipe, &show, t), 1).unwrap();

        // Starts at the bottom of the swing, peaks at the half-cycle.
        assert!(at(0.0).abs() < 0.01, "{}", at(0.0));
        assert!((at(0.25) - 0.5).abs() < 0.01, "{}", at(0.25));
        assert!((at(0.5) - 1.0).abs() < 0.01, "{}", at(0.5));
        assert!((at(0.75) - 0.5).abs() < 0.01, "{}", at(0.75));
        // ...and it is a curve, not a triangle: an eighth of the way in,
        // a triangle would read 0.25.
        assert!(at(0.125) < 0.2, "{}", at(0.125));
    }

    /// A `Delta` is flagged rather than emitted as an ordinary value, so
    /// the player adds it on top of the cascade's winner instead of
    /// letting it compete for the slot.
    #[test]
    fn a_delta_is_marked_relative() {
        let groups = groups();
        let emits = expand_recipe(&phaser(-0.4, 0.0, true), &bare(&groups), 0.1);
        assert!(!emits.is_empty());
        assert!(emits.iter().all(|e| e.relative));
        assert_eq!(dimmer_of(&emits, 1), Some(-0.4));
    }

    #[test]
    fn a_static_recipe_ignores_the_clock() {
        let recipe = Recipe::new(Selection::Chans(vec![1]), RecipeApply::Dimmer(0.42));
        let show = bare(&[]);
        for t in [0.0, 1.7, 99.0] {
            assert_eq!(dimmer_of(&expand_recipe(&recipe, &show, t), 1), Some(0.42));
        }
    }

    // -----------------------------------------------------------------
    // The on-disk shapes
    // -----------------------------------------------------------------

    /// Every show file in this repo is written in the terse one-step
    /// spelling; it has to keep parsing.
    #[test]
    fn the_pre_steps_spelling_still_parses() {
        let json = r#"{"target":{"Group":"Pars"},"apply":{"Dimmer":0.8}}"#;
        let recipe: Recipe = serde_json::from_str(json).unwrap();
        assert_eq!(recipe.steps.len(), 1);
        assert!(!recipe.is_phaser());
        assert_eq!(recipe.steps[0].apply, vec![RecipeApply::Dimmer(0.8)]);
    }

    /// ...and round-trips back to it, so re-saving a hand-written show
    /// does not explode it into step tables nobody asked for.
    #[test]
    fn a_one_step_recipe_round_trips_to_the_terse_spelling() {
        let recipe = Recipe::new(Selection::Chans(vec![1]), RecipeApply::Dimmer(0.5));
        let json = serde_json::to_string(&recipe).unwrap();
        assert!(json.contains("\"apply\""), "{json}");
        assert!(!json.contains("\"steps\""), "{json}");
        assert_eq!(serde_json::from_str::<Recipe>(&json).unwrap(), recipe);
    }

    #[test]
    fn the_waveform_spelling_expands_to_steps() {
        let json = r#"{
            "target": {"Group": "Pars"},
            "waveform": {"shape": "Sine", "attr": "Dimmer", "base": 0.5, "size": 0.5},
            "timing": {"speed": {"Bpm": 120.0}, "phase_spread_deg": 360.0}
        }"#;
        let recipe: Recipe = serde_json::from_str(json).unwrap();
        assert!(recipe.is_phaser());
        assert_eq!(recipe.steps.len(), 2);
        assert_eq!(recipe.timing.speed, Speed::Bpm(120.0));
        assert!(recipe.steps.iter().all(|s| s.ease == Ease::Sine));
    }

    /// A speed master that is not wired up is reported, because a
    /// frozen phaser is otherwise indistinguishable from a slow one.
    #[test]
    fn an_unwired_speed_master_is_reported() {
        let mut recipe = phaser(0.0, 1.0, false);
        recipe.timing.speed = Speed::Master("Song".to_string());
        let cue = Cue {
            name: "Chase".to_string(),
            recipes: vec![recipe],
            ..Default::default()
        };
        let groups = groups();
        let problems = unresolved(std::slice::from_ref(&cue), &bare(&groups));
        assert_eq!(problems.len(), 1);
        assert!(problems[0].contains("Song"), "{problems:?}");
    }

    // -----------------------------------------------------------------
    // Cooked status
    // -----------------------------------------------------------------

    #[test]
    fn a_recipe_that_resolves_reports_its_fixture_count() {
        let cue = Cue {
            name: "Wash".into(),
            recipes: vec![Recipe::new(
                Selection::Group("Pars".to_string()),
                RecipeApply::Color(Ref::Inline(ColorPreset {
                    name: "Red".into(),
                    red: 1.0,
                    green: 0.0,
                    blue: 0.0,
                })),
            )],
            ..Default::default()
        };
        let groups = groups();
        let cook = cook_cue(&cue, &bare(&groups), 0.0);
        // Three fixtures, not nine values — a colour sets three
        // channels each and "9" would be a lie.
        assert_eq!(cook.recipes, vec![Cook::Ok(3)]);
        assert_eq!(cook.status(), Status::Cooked);
    }

    #[test]
    fn a_recipe_that_selects_nothing_reports_failed() {
        let cue = Cue {
            name: "Typo".into(),
            recipes: vec![Recipe::new(
                Selection::Group("Prs".to_string()),
                RecipeApply::Dimmer(1.0),
            )],
            ..Default::default()
        };
        let groups = groups();
        let cook = cook_cue(&cue, &bare(&groups), 0.0);
        assert_eq!(cook.recipes, vec![Cook::Empty]);
        assert_eq!(cook.status(), Status::Failed);
    }

    #[test]
    fn the_pot_colours_distinguish_the_five_cases() {
        let groups = groups();
        let show = bare(&groups);
        let with = |recipes: Vec<Recipe>, values: Vec<CueValue>| {
            cook_cue(
                &Cue {
                    name: "x".into(),
                    recipes,
                    values,
                    ..Default::default()
                },
                &show,
                0.0,
            )
            .status()
        };
        let good = || {
            Recipe::new(
                Selection::Group("Pars".to_string()),
                RecipeApply::Dimmer(1.0),
            )
        };
        let bad = || {
            Recipe::new(
                Selection::Group("Nope".to_string()),
                RecipeApply::Dimmer(1.0),
            )
        };
        let direct = || CueValue {
            chan: 1,
            attr: Attribute::Dimmer,
            value: 1.0,
        };

        assert_eq!(with(vec![good()], vec![]), Status::Cooked);
        assert_eq!(with(vec![bad()], vec![]), Status::Failed);
        assert_eq!(with(vec![good()], vec![direct()]), Status::Mixed);
        assert_eq!(with(vec![], vec![direct()]), Status::Direct);
        assert_eq!(with(vec![], vec![]), Status::Empty);
        // A failure beats everything else — an operator needs to see the
        // broken one, not an average.
        assert_eq!(with(vec![good(), bad()], vec![direct()]), Status::Failed);
    }

    // -----------------------------------------------------------------
    // How a step list is played — Eos's effect "attributes"
    // -----------------------------------------------------------------

    /// A snapping two-step chase over four fixtures, one cycle per
    /// second, spread right around the selection.
    fn spread_chase(play: Play) -> Recipe {
        let at = |v: f32| Step::new(vec![RecipeApply::Raw(vec![(Attribute::Dimmer, v)])]);
        Recipe {
            target: Selection::Chans(vec![1, 2, 3, 4]),
            steps: vec![at(0.0), at(1.0)],
            timing: Timing {
                speed: Speed::Hz(1.0),
                phase_spread_deg: 360.0,
                direction: play,
                ..Default::default()
            },
            tricks: Vec::new(),
        }
    }

    fn levels(recipe: &Recipe, secs: f32) -> Vec<f32> {
        let show = bare(&[]);
        let emits = expand_recipe(recipe, &show, secs);
        (1..=4)
            .map(|chan| dimmer_of(&emits, chan).unwrap_or(f32::NAN))
            .collect()
    }

    /// Reverse runs the wave the other way along the selection.
    ///
    /// Not a plain list reversal, and the reason is worth stating:
    /// reverse gives fixture `i` the phase `1 - i/N`, which is `-i/N`
    /// once it wraps — so it matches *forward* fixture `(N - i) % N`.
    /// Fixture 0 maps to itself, because a whole cycle of offset is no
    /// offset.
    #[test]
    fn reverse_mirrors_the_selection() {
        let forward = levels(&spread_chase(Play::Forward), 0.1);
        let reversed = levels(&spread_chase(Play::Reverse), 0.1);
        let n = forward.len();
        for i in 0..n {
            assert_eq!(reversed[i], forward[(n - i) % n], "at {i}: {reversed:?}");
        }
    }

    /// A chase with a narrow active step: one fixture lit, travelling.
    /// The 50/50 two-step above has no gap to invert — half on, half
    /// off, inverted is still half and half.
    fn gap_chase(play: Play) -> Recipe {
        let at = |v: f32| Step::new(vec![RecipeApply::Raw(vec![(Attribute::Dimmer, v)])]);
        Recipe {
            target: Selection::Chans(vec![1, 2, 3, 4]),
            steps: vec![at(1.0), at(0.0), at(0.0), at(0.0)],
            timing: Timing {
                speed: Speed::Hz(1.0),
                phase_spread_deg: 360.0,
                direction: play,
                ..Default::default()
            },
            tricks: Vec::new(),
        }
    }

    /// The one Eos's own docs single out — "the one most people never
    /// build": fixtures sit at the top of the swing and the travelling
    /// point is the one that drops out.
    #[test]
    fn negative_is_a_dark_gap_travelling_across_the_rig() {
        for t in [0.1f32, 0.4, 0.7] {
            let forward = levels(&gap_chase(Play::Forward), t);
            let negative = levels(&gap_chase(Play::Negative), t);
            for (f, n) in forward.iter().zip(&negative) {
                assert!((f + n - 1.0).abs() < 1e-5, "{forward:?} vs {negative:?}");
            }
            // One lit point travelling becomes one dark point
            // travelling — not simply "everything dimmer".
            assert_eq!(
                forward.iter().filter(|v| **v > 0.5).count(),
                1,
                "{forward:?}"
            );
            assert_eq!(
                negative.iter().filter(|v| **v > 0.5).count(),
                3,
                "{negative:?}"
            );
        }
    }

    /// Build fills the selection up and resets, rather than moving one
    /// point along it.
    #[test]
    fn build_accumulates_then_resets() {
        let recipe = spread_chase(Play::Build);
        // A quarter into the cycle only the leading fixture has arrived;
        // by the end they all have.
        let early = levels(&recipe, 0.05);
        assert_eq!(early.iter().filter(|v| **v > 0.5).count(), 1, "{early:?}");
        let late = levels(&recipe, 0.95);
        assert_eq!(late.iter().filter(|v| **v > 0.5).count(), 4, "{late:?}");
        // ...and the wrap starts over rather than staying full.
        let wrapped = levels(&recipe, 1.05);
        assert_eq!(
            wrapped.iter().filter(|v| **v > 0.5).count(),
            1,
            "{wrapped:?}"
        );
    }

    /// Bounce runs the list out and back inside one cycle, so the value
    /// at three-quarters matches the value at a quarter.
    #[test]
    fn bounce_runs_out_and_back() {
        let mut recipe = spread_chase(Play::Bounce);
        recipe.timing.phase_spread_deg = 0.0;
        for step in &mut recipe.steps {
            step.transition = 1.0;
        }
        let quarter = levels(&recipe, 0.25)[0];
        let three_quarters = levels(&recipe, 0.75)[0];
        assert!(
            (quarter - three_quarters).abs() < 1e-5,
            "{quarter} vs {three_quarters}"
        );
    }

    /// Every mode has to stay position-addressed: the same clock reading
    /// gives the same output, or seeking would not land where playing
    /// did. Build in particular is a threshold, and a threshold that
    /// drifted would be invisible until a rehearsal.
    #[test]
    fn every_play_mode_is_a_pure_function_of_the_clock() {
        for play in [
            Play::Forward,
            Play::Reverse,
            Play::Bounce,
            Play::Build,
            Play::Negative,
        ] {
            let recipe = spread_chase(play);
            for t in [0.13f32, 0.5, 0.87, 2.31] {
                assert_eq!(levels(&recipe, t), levels(&recipe, t), "{play:?} at {t}");
            }
        }
    }
}
