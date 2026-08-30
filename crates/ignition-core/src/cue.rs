//! A fixture-agnostic tracking cue-list engine — the "programming a light
//! show" layer this project has been missing: `dmx.rs` (in `ignition-viz`)
//! can only *read* live DMX from an external console; nothing before this
//! could originate a show of its own. Deliberately has no I/O, no DMX byte
//! encoding, and no fixture/channel-map knowledge — it operates purely in
//! terms of `(ChanId, Attribute) -> f32` targets and fade timing, the same
//! way `ignition-proto`'s `Attribute` already treats a fixture's channel
//! layout as an output-time concern, not something upstream code touches.
//! `ignition-viz::show` is the bridge that turns this engine's output into
//! real DMX bytes for the live visualizer to render.
//!
//! Values are normalized the same way `dmx.rs::ResolvedAttributes` already
//! reads them back out: 0.0-1.0 for `Dimmer`/`ColorAdd`/`Strobe`/`Zoom`/
//! `Focus`/`Iris`, degrees for `Pan`/`Tilt` (-270..270 and -135..135 to
//! match `dmx.rs`'s own byte<->degree formulas, which this module's
//! counterpart in `ignition-viz::show` inverts when encoding to bytes).
//!
//! **Tracking** cue-list semantics, matching how a real console (Eos,
//! grandMA) behaves by default: a cue only needs to list what *changes* —
//! any `(chan, attr)` a cue doesn't mention holds wherever the previous cue
//! left it, rather than snapping back to some default. This is what makes
//! programming a real show practical (a 40-cue song doesn't need every
//! fixture re-stated in every cue, just the deltas), and is why `target`
//! below is cumulative across `go()` calls rather than reset per cue.

use crate::color::Intent;
use crate::focus::resolve_focus_delta;
use crate::music::{Bars, Position, SongMap};
use crate::recipe::{Expansion, Recipe, RecipeRef, Show, expand_recipe, expand_recipe_full};
use crate::step::Ease;
use crate::tricks::Fan;
use ignition_proto::{Attribute, ChanId, Vec3};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// One fixture attribute's value within a cue — `value`'s unit depends on
/// `attr` (see the module doc): 0.0-1.0 for level-like attributes, degrees
/// for `Pan`/`Tilt`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CueValue {
    pub chan: ChanId,
    pub attr: Attribute,
    pub value: f32,
}

/// One step of a show. `fade_secs` is how long the *move into* this cue
/// takes (its own fade time, not the previous cue's) — matches Eos/grandMA
/// convention where a cue's fade time describes arriving at it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
// r[impl cues.shape]
pub struct Cue {
    pub name: String,
    /// This cue's number — what a rehearsal note, a cue sheet and a
    /// call over comms all mean by "cue six". Stable under insertion
    /// and deliberately *not* the index: a cue put between 5 and 6 is
    /// 5.5, and nothing downstream of it renumbers. Absent means "take
    /// the position in the list", which is what every file written
    /// before numbers existed says.
    // r[impl cues.number]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub number: Option<f32>,
    /// Why this cue is the way it is — "wait for the bass drop, MD
    /// holds the bar if the crowd goes". The name is what fits in a
    /// column; this is everything that does not, and it never affects
    /// output.
    // r[impl cues.note]
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
    /// How the list draws this cue. A hundred-cue list is scanned, not
    /// read, and a section boundary the eye can find is worth more than
    /// any column of data. Never affects output.
    // r[impl cues.appearance]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub appearance: Option<Appearance>,
    #[serde(default)]
    // r[impl cues.fade-is-arrival]
    pub fade_secs: f32,
    /// **Layer 1** of the cascade — direct values on the cue. Hand
    /// tweaks and recorded output. Always beat a recipe on the same cue,
    /// which is what makes "override one fixture out of the recipe" a
    /// defined operation rather than a merge.
    #[serde(default)]
    // r[impl recipes.cascade] - layer 1, direct values
    pub values: Vec<CueValue>,
    /// **Layer 2** — recipes on the cue, resolved at output time rather
    /// than flattened at load. Storing the template instead of its
    /// output is the whole point: a recipe still knows it targets a
    /// *group*, so adding a fixture to that group changes what the cue
    /// covers with no re-authoring.
    #[serde(default)]
    // r[impl recipes.cascade] - layer 2, recipes
    // r[impl cues.recipes-not-values]
    // r[impl effects.library.by-name] - a cue may name a library effect instead of copying it
    pub recipes: Vec<RecipeRef>,
    /// Does not track from its predecessor — everything this cue does
    /// not set goes out rather than holding. Required before song
    /// sections can reorder (a chorus recalled after a bridge must not
    /// inherit the bridge's leftovers).
    #[serde(default)]
    // r[impl cues.block]
    // r[impl recipes.blocking-resets]
    pub block: bool,
    /// Where this cue sits in the song, if it belongs to one.
    ///
    /// Optional, and *alongside* list order rather than instead of it.
    /// `at` is what a clock uses; order is what a person pressing GO
    /// uses; both land in the same state. That is the whole of "losing
    /// backing tracks must not mean losing lighting" — see
    /// `docs/domain/musical-time-cues.md`.
    ///
    /// Written as the author meant it — "4 bars into `CH 1`" — and
    /// resolved to a bar by [`CueList::resolve_positions`] against the
    /// song map on load. An absolute bar (what older files say) needs
    /// no map at all.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    // r[impl cues.position]
    // r[impl song.relative-position] - the file carries the relative form
    pub at: Option<Position>,
    /// The bar `at` resolved to, kept in the file so a player with no
    /// song map — an older build, a still with `--bar` — lands where
    /// the author's arrangement had it. Rewritten whenever a map is
    /// given; never authoritative over `at`.
    // r[impl song.relative-position.resolved-on-load] - the cached bar is what runs when no map is given
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved: Option<Bars>,
    /// Per-class fades and delays, in beats. Every class defaults to
    /// `fade_secs`, so a cue that says nothing here fades as it always
    /// did.
    // r[impl cues.timing.per-attribute]
    // r[impl cues.delay]
    #[serde(default, skip_serializing_if = "CueTiming::is_default")]
    pub timing: CueTiming,
    /// A selection within the cue that takes its own timing — one
    /// fixture or a small group on its own fade and delay while the rest
    /// keep the class timing. Resolved against the show at take; a
    /// fixture named by several takes the last.
    // r[impl cues.timing-overrides]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub timing_overrides: Vec<TimingOverride>,
    /// Attributes whose values from before this cue do **not** track
    /// through it on the fixtures it covers, even though it does not
    /// set them. Nothing is faded to rest on purpose; the source is
    /// simply not carried.
    // r[impl cues.break]
    #[serde(default, rename = "break", skip_serializing_if = "Vec::is_empty")]
    pub break_: Vec<Attribute>,
    /// Spread the delay and fade across the fixtures this cue covers, in
    /// the order its first recipe lists them — a static cue that wipes.
    // r[impl cues.fan]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fan: Option<CueFan>,
    /// How fixtures that go from dark to lit *in this cue* are
    /// pre-positioned while the previous cue is still showing.
    // r[impl cues.mib.mode]
    // r[impl cues.mib.timing]
    #[serde(default, skip_serializing_if = "Mib::is_default")]
    pub mib: Mib,
    /// Re-state everything this cue would only track, with this cue's
    /// timing: tracked recipes are re-taken as this cue's own.
    // r[impl cues.assert]
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub assert: bool,
    /// This cue's changes do not track: the cue after it starts from
    /// the state *before* this one.
    // r[impl cues.cue-only]
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub cue_only: bool,
    /// Attributes this cue stops asserting on the fixtures it covers
    /// (on every tracked fixture, when it covers none). They fade to
    /// rest over the class's timing.
    // r[impl cues.release]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub release: Vec<Attribute>,
    /// Crossfade a replaced recipe's values into this cue's over the
    /// fade. The stage stack already resolves both sides live every
    /// frame, so this is the engine's behaviour for every cue; the flag
    /// records the author's intent and is what a stricter "swap at
    /// take" mode would be switched off by.
    // r[impl cues.morph]
    // r[impl effects.morph]
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub morph: bool,
    /// What takes this cue.
    // r[impl cues.trig]
    #[serde(default, skip_serializing_if = "Trig::is_go")]
    pub trig: Trig,
    /// Opaque host commands, handed out by `CuePlayer::drain_commands`
    /// once the cue is taken live.
    // r[impl cues.command]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub commands: Vec<String>,
}

/// A cue's colour in the list, and optionally a short label beside it.
///
/// Drawing only. The colour is a CSS string the surface hands straight
/// to a stylesheet, because the list is the only thing that ever reads
/// it.
// r[impl cues.appearance]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Appearance {
    pub color: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

impl Cue {
    /// Where the clock finds this cue: the bar `at` resolved to, an
    /// absolute `at` that never needed resolving, or a positional
    /// trigger. A relative `at` nobody has resolved is no position.
    // r[impl song.relative-position.resolved-on-load]
    pub fn position(&self) -> Option<Bars> {
        self.resolved
            .or(match &self.at {
                Some(Position::Absolute(bars)) => Some(*bars),
                _ => None,
            })
            .or(match self.trig {
                Trig::At(at) => Some(at),
                _ => None,
            })
    }

    /// Resolves `at` against a song map, caching the bar. `false` when
    /// the map has no such section — the cue keeps whatever bar it had.
    pub fn resolve_position(&mut self, song: &SongMap) -> bool {
        match &self.at {
            None => true,
            Some(position) => match position.resolve(song) {
                Some(bars) => {
                    self.resolved = Some(bars);
                    true
                }
                None => false,
            },
        }
    }

    /// The recipes this cue carries once every library reference is
    /// looked up through `show` — what the player cooks.
    // r[impl effects.library.by-name] - resolved through the show, never stored resolved
    pub fn resolved_recipes(&self, show: &Show<'_>) -> Vec<Recipe> {
        self.recipes.iter().flat_map(|r| r.resolve(show)).collect()
    }
}

/// The four classes cue timing is written against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AttrClass {
    Intensity,
    Colour,
    Position,
    Beam,
}

impl AttrClass {
    // r[impl cues.timing.per-attribute] - attribute to class
    pub fn of(attr: &Attribute) -> Self {
        match attr {
            Attribute::Dimmer => Self::Intensity,
            Attribute::ColorAdd { .. } | Attribute::ColorWheel { .. } => Self::Colour,
            Attribute::Pan | Attribute::Tilt | Attribute::PanFine | Attribute::TiltFine => {
                Self::Position
            }
            Attribute::Zoom
            | Attribute::Focus
            | Attribute::Iris
            | Attribute::Strobe
            | Attribute::GoboWheel { .. }
            | Attribute::Custom(_) => Self::Beam,
        }
    }

    const ALL: [AttrClass; 4] = [Self::Intensity, Self::Colour, Self::Position, Self::Beam];
}

/// A delay per class, in beats, before that class's fade begins.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
// r[impl cues.delay]
pub struct ClassDelays {
    #[serde(default)]
    pub intensity: f32,
    #[serde(default)]
    pub colour: f32,
    #[serde(default)]
    pub position: f32,
    #[serde(default)]
    pub beam: f32,
}

impl ClassDelays {
    pub fn get(&self, class: AttrClass) -> f32 {
        match class {
            AttrClass::Intensity => self.intensity,
            AttrClass::Colour => self.colour,
            AttrClass::Position => self.position,
            AttrClass::Beam => self.beam,
        }
    }
}

/// Per-class fades in **beats**; `None` falls back to the cue's
/// `fade_secs`. `dimmer_out` covers an intensity that is going *down*
/// (or out), `dimmer_in` one coming up.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
// r[impl cues.timing.per-attribute]
pub struct CueTiming {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dimmer_in: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dimmer_out: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub beam: Option<f32>,
    #[serde(default)]
    pub delay: ClassDelays,
    /// The shape of each class's fade — linear, slow, fast, s-curve,
    /// swing — reusing the step ease. Linear unless said otherwise.
    // r[impl cues.transition-curve]
    #[serde(default, skip_serializing_if = "ClassEase::is_default")]
    pub ease: ClassEase,
}

/// An ease per class. A mover that swings into place reads as designed;
/// a straight line reads as generated.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
// r[impl cues.transition-curve]
pub struct ClassEase {
    #[serde(default)]
    pub intensity: Ease,
    #[serde(default)]
    pub colour: Ease,
    #[serde(default)]
    pub position: Ease,
    #[serde(default)]
    pub beam: Ease,
}

impl ClassEase {
    fn is_default(&self) -> bool {
        *self == Self::default()
    }

    pub fn get(&self, class: AttrClass) -> Ease {
        match class {
            AttrClass::Intensity => self.intensity,
            AttrClass::Colour => self.colour,
            AttrClass::Position => self.position,
            AttrClass::Beam => self.beam,
        }
    }

    /// The same ease on every class.
    pub fn all(ease: Ease) -> Self {
        Self {
            intensity: ease,
            colour: ease,
            position: ease,
            beam: ease,
        }
    }
}

/// The named curves a fade is written with, as sugar over `Ease`.
///
/// `Ease` cannot overshoot — its output is clamped to the move — so
/// `Swing` is the pendulum *arrival*: it leaves softly and lands at
/// twice the linear slope, which is what a mover swinging into its
/// mark looks like from the floor. `SCurve` is exactly `Sine`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
// r[impl cues.transition-curve] - the named forms
pub enum CurveName {
    #[default]
    Linear,
    /// Soft departure, linear arrival.
    Slow,
    /// Hard departure, linear arrival.
    Fast,
    /// Soft both ends — `Ease::Sine`.
    SCurve,
    /// Soft departure, hard arrival.
    Swing,
}

impl CurveName {
    pub fn ease(self) -> Ease {
        match self {
            CurveName::Linear => Ease::Linear,
            CurveName::Slow => Ease::Curve {
                accel: -1.0,
                decel: 0.0,
            },
            CurveName::Fast => Ease::Curve {
                accel: 1.0,
                decel: 0.0,
            },
            CurveName::SCurve => Ease::Sine,
            CurveName::Swing => Ease::Curve {
                accel: -1.0,
                decel: 1.0,
            },
        }
    }
}

impl From<CurveName> for Ease {
    fn from(name: CurveName) -> Ease {
        name.ease()
    }
}

/// Timing for a selection within a cue, looked up before the class
/// timing for every fixture it resolves to.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
// r[impl cues.timing-overrides]
pub struct TimingOverride {
    pub selection: crate::selection::Selection,
    pub timing: CueTiming,
}

impl CueTiming {
    fn is_default(&self) -> bool {
        *self == Self::default()
    }

    fn fade_beats(&self, class: AttrClass) -> Option<f32> {
        match class {
            AttrClass::Intensity => self.dimmer_in,
            AttrClass::Colour => self.color,
            AttrClass::Position => self.position,
            AttrClass::Beam => self.beam,
        }
    }
}

/// A delay and a fade spread across the fixtures a cue covers, in
/// beats. A flat `fade` (`0 → 0`, the default) leaves each fixture on
/// its class fade; the delay always adds to the class delay.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
// r[impl cues.fan]
pub struct CueFan {
    #[serde(default)]
    pub delay: Fan,
    #[serde(default)]
    pub fade: Fan,
}

/// When a dark fixture is moved to where the coming cue wants it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
// r[impl cues.mib.mode]
pub enum MibMode {
    /// As soon as the previous cue is taken.
    Early,
    /// With the next take *after* the fixture goes dark — the cue that
    /// darkens it never moves it; the one after does.
    // r[impl cues.mib.preference] - upon-go beside early and late
    UponGo,
    /// Timed to arrive just before this cue, when both cues carry a
    /// position; otherwise the same as `Early`.
    #[default]
    Late,
    None,
}

/// Whether a running effect keeps running on a fixture while it is dark
/// and being pre-positioned.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
// r[impl cues.mib.multistep]
pub enum MultiStep {
    /// The effect keeps its phase with the rest of the rig.
    #[default]
    Running,
    /// The effect is frozen where it was when the fixture went dark, and
    /// resumes when it comes up.
    Paused,
}

/// Move-in-black settings on the cue whose fixtures come up from dark.
///
/// `mode`, `fade_beats`, `delay_beats`, `multistep` are read from the
/// cue the fixture comes up *in*; `preference` and `hold` describe the
/// cue as a dark window — how good a moment it is to move through, and
/// whether its dark fixtures must stay where they are.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
// r[impl cues.mib.mode]
// r[impl cues.mib.timing]
pub struct Mib {
    #[serde(default)]
    pub mode: MibMode,
    /// The pre-position's own fade, in beats.
    #[serde(default = "one_beat")]
    pub fade_beats: f32,
    /// Beats after the previous cue's take before the move starts.
    #[serde(default)]
    pub delay_beats: f32,
    /// How good a moment this cue is to pre-position *into* (0–100).
    /// Among the cues a fixture is dark through, the best-rated one is
    /// where the move happens.
    // r[impl cues.mib.preference]
    #[serde(default = "fifty")]
    pub preference: u8,
    /// The fixtures dark after this cue stay where they are — a light
    /// that must not move while dark, for a reveal.
    // r[impl cues.mib.hold]
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hold: bool,
    /// Whether an effect on a fixture being pre-positioned keeps running.
    // r[impl cues.mib.multistep]
    #[serde(default)]
    pub multistep: MultiStep,
}

fn one_beat() -> f32 {
    1.0
}

fn fifty() -> u8 {
    50
}

impl Default for Mib {
    fn default() -> Self {
        Self {
            mode: MibMode::Late,
            fade_beats: 1.0,
            delay_beats: 0.0,
            preference: 50,
            hold: false,
            multistep: MultiStep::Running,
        }
    }
}

impl Mib {
    fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// What takes a cue.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
// r[impl cues.trig]
pub enum Trig {
    /// The operator.
    #[default]
    Go,
    /// The clock, at a position — the same as `at`.
    At(Bars),
    /// `beats` after the previous cue was taken, on wall time at the
    /// Song tempo, with no transport needed. Follows chain.
    Follow { beats: f32 },
    /// A sound event on a named band, which the host reports by
    /// calling `go` when `pending_sound_trig` names the band it heard.
    Sound { band: String },
}

impl Trig {
    fn is_go(&self) -> bool {
        matches!(self, Trig::Go)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CueList {
    #[serde(default)]
    pub name: String,
    pub cues: Vec<Cue>,
    /// What the song fires, apart from what the operator takes.
    ///
    /// A different kind of thing from a cue — see `docs/spec/triggers.md`
    /// — and stored apart so the GO order holds sections and lifts only.
    // r[impl files.show.triggers]
    // r[impl triggers.are-not-cues]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub triggers: Vec<crate::trigger::Trigger>,
    /// GO on the last cue takes the first.
    // r[impl cues.wrap-and-restart]
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub wrap: bool,
    /// Where the list goes on from when it is re-enabled.
    // r[impl cues.wrap-and-restart]
    #[serde(default, skip_serializing_if = "Restart::is_first")]
    pub restart: Restart,
}

/// How a list restarts when re-enabled.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
// r[impl cues.wrap-and-restart]
pub enum Restart {
    #[default]
    First,
    Current,
    Next,
}

impl Restart {
    fn is_first(&self) -> bool {
        *self == Restart::First
    }
}

/// How an edit to a cue is stored against the cues after it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
// r[impl cues.shield]
pub enum StoreMode {
    /// The edit tracks forward as any value does.
    Track,
    /// The edit stops at the next cue: it restates what the edited
    /// fixtures had before, for every key the edit changed.
    CueOnly,
    /// The edit stops at the first later cue where each edited
    /// fixture's intensity comes up from zero.
    ShieldFromZero,
    /// The edit stops at the first later cue where each edited
    /// fixture's intensity is above zero.
    ShieldAboveZero,
}

impl CueList {
    /// What cue `index` is *called* — its stored number, or its
    /// position in the list for a cue that has never been numbered.
    ///
    /// Every surface that shows a cue number goes through here, so a
    /// file written before numbers existed reads as 1, 2, 3 rather than
    /// as blanks.
    // r[impl cues.number]
    pub fn number_of(&self, index: usize) -> f32 {
        self.cues
            .get(index)
            .and_then(|c| c.number)
            .unwrap_or(index as f32 + 1.0)
    }

    /// The number a cue inserted at `index` should take: halfway
    /// between its neighbours, so inserting between 5 and 6 gives 5.5
    /// and nothing after it renumbers. Appending continues by whole
    /// numbers.
    // r[impl cues.number]
    pub fn number_for_insert(&self, index: usize) -> f32 {
        let before = index.checked_sub(1).map(|i| self.number_of(i));
        let after = (index < self.cues.len()).then(|| self.number_of(index));
        match (before, after) {
            (Some(b), Some(a)) if a > b => (b + a) / 2.0,
            (Some(b), _) => b + 1.0,
            (None, Some(a)) => a - 1.0,
            (None, None) => 1.0,
        }
    }

    /// Stamps every cue with a whole number in list order, discarding
    /// the fractions insertion left behind. A deliberate, destructive
    /// tidy — every note and cue sheet that said "5.5" stops matching —
    /// so nothing calls it but an operator asking for it.
    // r[impl cues.number]
    pub fn renumber(&mut self) {
        for (i, cue) in self.cues.iter_mut().enumerate() {
            cue.number = Some(i as f32 + 1.0);
        }
    }

    /// Replaces cue `index` with `edited`, and for the shielding modes
    /// inserts the compensating restatements that stop the edit at the
    /// cue the mode names — the surgical form of cue-only.
    ///
    /// Works on direct values: the keys whose value the edit changed or
    /// added are restated, in the shielding cue, at what the show had
    /// there before the edit — the old cue's own value, else the last
    /// direct value tracked in from before it. A key nothing earlier set
    /// has nothing to restate, and tracks. Recipe coverage is not
    /// shielded.
    // r[impl cues.shield]
    pub fn store(&mut self, index: usize, edited: Cue, mode: StoreMode) {
        if index >= self.cues.len() {
            self.cues.push(edited);
            return;
        }
        let old = std::mem::replace(&mut self.cues[index], edited);
        if mode == StoreMode::Track {
            return;
        }
        // What the show carried at the end of the old cue, for the keys
        // the edit touched.
        let mut carried = direct_state_through(&self.cues[..index]);
        for v in &old.values {
            carried.insert((v.chan, v.attr.clone()), v.value);
        }
        let changed: Vec<Restated> = self.cues[index]
            .values
            .iter()
            .filter_map(|v| {
                let key = (v.chan, v.attr.clone());
                let was = carried.get(&key).copied()?;
                ((was - v.value).abs() > 1e-6).then_some((key, was))
            })
            .collect();
        if changed.is_empty() {
            return;
        }
        // Intensity as the list carries it, cue by cue after the edit,
        // to find each fixture's shielding cue.
        let mut level = direct_state_through(&self.cues[..=index]);
        let mut restate: HashMap<usize, Vec<Restated>> = HashMap::new();
        let mut pending = changed;
        for j in index + 1..self.cues.len() {
            if pending.is_empty() {
                break;
            }
            let cue = &self.cues[j];
            let before: HashMap<ChanId, f32> = pending
                .iter()
                .map(|(key, _)| {
                    let dim = level
                        .get(&(key.0, Attribute::Dimmer))
                        .copied()
                        .unwrap_or(0.0);
                    (key.0, dim)
                })
                .collect();
            apply_direct_state(&mut level, cue);
            let (kept, stopped): (Vec<_>, Vec<_>) = pending.into_iter().partition(|(key, _)| {
                let now = level
                    .get(&(key.0, Attribute::Dimmer))
                    .copied()
                    .unwrap_or(0.0);
                let was = before[&key.0];
                let stops = match mode {
                    StoreMode::CueOnly => true,
                    StoreMode::ShieldFromZero => was <= 0.0 && now > 0.0,
                    StoreMode::ShieldAboveZero => now > 0.0,
                    StoreMode::Track => false,
                };
                !stops
            });
            pending = kept;
            restate.entry(j).or_default().extend(stopped);
        }
        for (j, keys) in restate {
            let cue = &mut self.cues[j];
            for (key, value) in keys {
                if cue
                    .values
                    .iter()
                    .any(|v| (v.chan, &v.attr) == (key.0, &key.1))
                {
                    continue; // the cue sets it itself; nothing to stop
                }
                cue.values.push(CueValue {
                    chan: key.0,
                    attr: key.1,
                    value,
                });
            }
        }
    }

    /// Resolves every relative position in the list against `song`,
    /// caching the bars the player runs on, and re-sorts the cues so a
    /// cue that moved is found by `seek`. Returns the names whose
    /// section the arrangement no longer has; those keep the bar they
    /// had, rather than landing at bar 1.
    // r[impl song.relative-position.resolved-on-load]
    // r[impl song.relative-position.duplicate-names] - through Position::resolve
    pub fn resolve_positions(&mut self, song: &SongMap) -> Vec<String> {
        let mut unresolved = Vec::new();
        for cue in &mut self.cues {
            if !cue.resolve_position(song) {
                unresolved.push(cue.name.clone());
            }
        }
        for trigger in &mut self.triggers {
            if !trigger.resolve_position(song) {
                unresolved.push(trigger.name.clone());
            }
        }
        self.sort_by_position();
        unresolved
    }

    /// Cues in position order, unpositioned ones keeping their place
    /// relative to each other at the end.
    pub fn sort_by_position(&mut self) {
        self.cues.sort_by(|x, y| {
            x.position()
                .partial_cmp(&y.position())
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    }
}

/// Where a tracked attribute's value comes from.
///
/// Tracking carries the *source*, not a number, because a recipe is a
/// live thing: one left running by a cue three cues back has to keep
/// being asked for its current value, not frozen at whatever it read
/// when it was fired.
#[derive(Debug, Clone, PartialEq)]
// r[impl cues.tracking-carries-sources]
enum Source {
    Direct(f32),
    /// Index into `CuePlayer::active`.
    Recipe(usize),
}

/// The layer stack at one point in the show — what tracking carries
/// forward from cue to cue.
#[derive(Debug, Clone, Default)]
// r[impl cues.tracking]
struct Layers {
    /// Every `(chan, attr)` any cue up to here has set, and where its
    /// value comes from. A cue that does not mention a channel simply
    /// does not touch this, which is what tracking *is*.
    // r[impl cues.tracking]
    // r[impl cues.tracking-carries-sources]
    // r[impl playback.absolute-layer]
    tracked: HashMap<(ChanId, Attribute), Source>,
    /// Which recipe, if any, is *modulating* each attribute. Relative
    /// values do not compete for the slot — they add on top of whatever
    /// won it — so they get their own map. One modulator per attribute,
    /// latest cue wins, same rule as everything else.
    /// Every relative recipe touching a key, in the order taken.
    ///
    /// The **last still-contributing** one wins, which is grandMA3's
    /// rule: "absolute and relative values in multiple parts will use
    /// the value with the highest cue part number". Relative values do
    /// not sum with each other — absolute and relative are separate
    /// layers, and the relative layer adds to the absolute one, but
    /// within the relative layer a later value replaces an earlier.
    ///
    /// A list rather than a single slot, because "last *still
    /// contributing*" needs the ones underneath. A finished one-shot
    /// withdraws (see `contributing`) and the chase beneath it resumes;
    /// with a single slot it had nothing to resume to, which is what
    /// made a hit landing on already-chased fixtures kill that chase for
    /// the rest of the section.
    ///
    /// Growth is bounded by the cascade: a blocking cue starts from
    /// empty layers, so this only accumulates within a section.
    // r[impl recipes.relative-is-a-separate-layer]
    // r[impl recipes.relative-last-wins]
    // r[impl recipes.finished-one-shot-withdraws] - keeps the ones underneath to resume
    modulated: HashMap<(ChanId, Attribute), Vec<usize>>,
    /// Which recipe *aimed* each channel at a room point — the one whose
    /// `FocusPoint`/`FocusFan`/`FocusKeyframes` won the pan/tilt slot.
    /// The source rather than the point, because the point moves: a
    /// fan blends between steps, a marker is moved by a tracker. Cleared
    /// when something later sets pan or tilt without a point, since a
    /// delta against an aim the fixture no longer has is nonsense.
    // r[impl focus.delta] - the aim is tracked beside the angles
    aim: HashMap<ChanId, usize>,
    /// Every relative recipe offering a `FocusDelta` on a channel, in
    /// the order taken — the focus twin of `modulated`, and resolved by
    /// the same rule.
    // r[impl focus.delta]
    // r[impl focus.orbit-in-metres]
    focus_modulated: HashMap<ChanId, Vec<usize>>,
    /// Fixtures whose running effects are held at the show time given,
    /// for as long as this stage's cue is the one showing: a dark
    /// fixture being pre-positioned under a paused multistep. Not
    /// carried to the next cue — the effect resumes with the take that
    /// lights the fixture.
    // r[impl cues.mib.multistep]
    frozen: HashMap<ChanId, f32>,
}

/// A recipe some live stage still points at, and when it was taken.
///
/// One struct rather than two parallel vectors, because the two drifted.
/// `compact` renumbers recipes every GO, and a second vector of
/// timestamps was not renumbered with them — so after the first
/// compaction every one-shot read some *other* recipe's stamp, judged
/// itself long finished, and withdrew before it had been seen. From the
/// stage that was a hit doing nothing while the chase beneath it went on
/// as if nothing had been fired. The timestamp now travels with the
/// recipe it belongs to and cannot be renumbered apart from it.
///
/// r[impl cues.one-shot.stamp-travels-with-recipe]
#[derive(Debug, Clone)]
struct Active {
    recipe: Recipe,
    /// Show time this recipe was taken.
    ///
    /// Only one-shots read it. A looping phaser deliberately runs off
    /// the shared monotonic clock, because that is what keeps two cues
    /// carrying the same chase in phase with each other instead of each
    /// restarting it. A one-shot needs the opposite: an envelope that
    /// has already finished before its cue was taken is not an envelope,
    /// it is a fixture sitting at its end value.
    since: f32,
    /// Seconds each fixture's own start is behind the recipe's, from a
    /// delay fan on the cue that took it. Phase is measured from the
    /// unit's own start, so a wipe and a chase on one selection agree
    /// about which fixture is first. Travels with the recipe so a later
    /// cue tracking it does not jump its phase.
    // r[impl cues.delay-to-phase]
    unit_delay: HashMap<ChanId, f32>,
}

/// How long each key of one stage takes to arrive, in seconds.
#[derive(Debug, Clone, Default)]
// r[impl cues.timing.per-attribute]
// r[impl cues.delay]
struct StageTiming {
    /// The cue's class timing.
    base: ClassTable,
    /// A selection's own class timing, looked up before `base`.
    // r[impl cues.timing-overrides]
    overrides: HashMap<ChanId, ClassTable>,
    /// A fan across the cue's fixtures: `(extra delay, fade override)`.
    // r[impl cues.fan]
    per_chan: HashMap<ChanId, (f32, Option<f32>)>,
    /// Keys with their own timing regardless of class — pre-positions.
    // r[impl cues.mib.timing]
    per_key: HashMap<(ChanId, Attribute), (f32, f32)>,
    /// Keys covered by a recipe that carries its own arrival. A cue's
    /// recipes are its parts, so this is per-part timing; it is keyed
    /// per `(chan, attr)` rather than per channel because two parts of
    /// one cue routinely share a fixture and differ on what they set —
    /// the colour part and the position part of the same mover.
    // r[impl cues.recipe.timing]
    // r[impl cues.parts-are-recipes]
    per_recipe: HashMap<(ChanId, Attribute), ClassTable>,
    /// When the last of the above has arrived.
    span: f32,
}

/// One `CueTiming` in seconds.
#[derive(Debug, Clone, Default)]
struct ClassTable {
    /// `(delay, fade)` per class, indexed by `AttrClass::ALL` order.
    class: [(f32, f32); 4],
    /// The intensity fade for a dimmer going down.
    dimmer_out: f32,
    /// The shape of each class's fade.
    // r[impl cues.transition-curve]
    ease: [Ease; 4],
}

impl ClassTable {
    fn new(timing: &CueTiming, fade_secs: f32, spb: f32) -> Self {
        let mut class = [(0.0, fade_secs); 4];
        let mut ease = [Ease::Linear; 4];
        for (i, c) in AttrClass::ALL.iter().enumerate() {
            let fade = timing.fade_beats(*c).map_or(fade_secs, |b| b * spb);
            class[i] = (timing.delay.get(*c) * spb, fade);
            ease[i] = timing.ease.get(*c);
        }
        let dimmer_out = timing.dimmer_out.map_or(class[0].1, |b| b * spb);
        Self {
            class,
            dimmer_out,
            ease,
        }
    }

    fn longest(&self) -> f32 {
        self.class
            .iter()
            .map(|(d, f)| d + f)
            .fold(self.class[0].0 + self.dimmer_out, f32::max)
    }
}

fn class_index(attr: &Attribute) -> usize {
    let class = AttrClass::of(attr);
    AttrClass::ALL.iter().position(|c| *c == class).unwrap_or(0)
}

impl StageTiming {
    fn from_cue(cue: &Cue, bpm: f32) -> Self {
        let spb = 60.0 / bpm;
        let mut timing = Self {
            base: ClassTable::new(&cue.timing, cue.fade_secs, spb),
            overrides: HashMap::new(),
            per_chan: HashMap::new(),
            per_key: HashMap::new(),
            per_recipe: HashMap::new(),
            span: 0.0,
        };
        timing.recompute_span();
        timing
    }

    fn recompute_span(&mut self) {
        let base = self
            .overrides
            .values()
            .chain(self.per_recipe.values())
            .map(ClassTable::longest)
            .fold(self.base.longest(), f32::max);
        let fanned = self
            .per_chan
            .iter()
            .map(|(chan, (extra, fade))| {
                let table = self.overrides.get(chan).unwrap_or(&self.base);
                let longest = fade.map_or(table.longest(), |f| {
                    table.class.iter().map(|(d, _)| d + f).fold(f, f32::max)
                });
                extra + longest
            })
            .fold(base, f32::max);
        self.span = self
            .per_key
            .values()
            .map(|(d, f)| d + f)
            .fold(fanned, f32::max);
    }

    /// `(delay, fade)` for one key, seconds.
    // r[impl cues.timing-overrides] - override first, then the class
    fn for_key(&self, key: &(ChanId, Attribute), falling: bool) -> (f32, f32) {
        if let Some(own) = self.per_key.get(key) {
            return *own;
        }
        let table = self.table_for(key);
        let index = class_index(&key.1);
        let (mut delay, mut fade) = table.class[index];
        if index == 0 && falling {
            fade = table.dimmer_out;
        }
        if let Some((extra, over)) = self.per_chan.get(&key.0) {
            delay += extra;
            if let Some(f) = over {
                fade = *f;
            }
        }
        (delay, fade)
    }

    /// The curve one key's fade runs on. A pre-position runs straight.
    // r[impl cues.transition-curve]
    fn ease_for(&self, key: &(ChanId, Attribute)) -> Ease {
        if self.per_key.contains_key(key) {
            return Ease::Linear;
        }
        self.table_for(key).ease[class_index(&key.1)]
    }

    /// Which class table one key runs on. The more specific wins: a
    /// named selection's override, then the recipe (the cue part) that
    /// covers the key, then the cue's own.
    // r[impl cues.recipe.timing-precedence]
    fn table_for(&self, key: &(ChanId, Attribute)) -> &ClassTable {
        self.overrides
            .get(&key.0)
            .or_else(|| self.per_recipe.get(key))
            .unwrap_or(&self.base)
    }
}

/// One cue's worth of state, plus how far into its own fade it is.
#[derive(Debug, Clone)]
struct Stage {
    layers: Layers,
    /// The layers this cue was taken *from*, kept only for a cue-only
    /// cue so the next one can start from there instead.
    // r[impl cues.cue-only]
    base: Option<Layers>,
    timing: StageTiming,
    elapsed: f32,
}

/// The time a recipe should be evaluated at.
///
/// Looping phasers get the shared show clock, so two cues carrying the
/// same chase stay in phase rather than each restarting it. One-shots
/// get time since they were taken, because an envelope that finished
/// before its own cue fired never plays at all.
impl CuePlayer {
    /// Whether a recipe still has anything to say.
    ///
    /// A looping phaser always does. A one-shot stops once its envelope
    /// has run out, and *withdrawing* rather than holding is the whole
    /// point: under last-wins, a finished bump that stayed in the layer
    /// would go on winning at its final value forever, which is exactly
    /// how an accent came to kill the chase it landed on for the rest of
    /// a section. Withdrawn, the value underneath resumes.
    /// r[impl recipes.finished-one-shot-withdraws]
    // r[impl playback.transient-withdraws]
    // r[impl playback.release-falls-through] - a withdrawn transient falls through to what is beneath
    fn contributing(&self, id: usize, show: &Show) -> bool {
        let Some(active) = self.active.get(id) else {
            return false;
        };
        if !active.recipe.timing.once {
            return true;
        }
        // `cycles_at` clamps a one-shot just under 1.0 and holds there,
        // so "finished" is that clamp having been reached rather than a
        // separate piece of state to keep in step with it.
        active
            .recipe
            .timing
            .cycles(self.recipe_time(id), show.speeds)
            < 1.0
    }

    /// r[impl recipes.one-shot-clock]
    // r[impl effects.sync.shared-clock] - loops read the shared clock
    fn recipe_time(&self, id: usize) -> f32 {
        match self.active.get(id) {
            Some(active) if active.recipe.timing.once => self.clock - active.since,
            _ => self.clock,
        }
    }
}

impl Stage {
    /// How far the *slowest* thing in this stage has got — 1.0 once
    /// every class, fan and pre-position has arrived.
    fn progress(&self) -> f32 {
        progress_of(self.elapsed, 0.0, self.timing.span)
    }

    /// How far one key has got, by its own class, fan and delay.
    // r[impl cues.timing.per-attribute] - progress is per key
    // r[impl cues.delay] - a delayed key holds until its delay has passed
    // r[impl cues.transition-curve] - shaped by the class's curve
    fn progress_for(&self, key: &(ChanId, Attribute), falling: bool) -> f32 {
        let (delay, fade) = self.timing.for_key(key, falling);
        let t = progress_of(self.elapsed, delay, fade);
        self.timing.ease_for(key).apply(t)
    }
}

fn progress_of(elapsed: f32, delay: f32, fade: f32) -> f32 {
    let into = elapsed - delay;
    if fade > 0.0 {
        (into / fade).clamp(0.0, 1.0)
    } else if into >= 0.0 {
        1.0
    } else {
        0.0
    }
}

/// The Song tempo the player converts beats with.
///
/// Read from `Show.speeds["Song"]`; 120 when the show has no such
/// master, which is the fallback a `Speed::Master` takes too.
fn song_bpm(show: &Show<'_>) -> f32 {
    show.speeds
        .get("Song")
        .copied()
        .filter(|bpm| *bpm > 0.0)
        .unwrap_or(120.0)
}

/// Beats from one position to another.
///
/// Exact when the show carries a tempo map: the count walks the map's
/// points, so a bar is as many beats as its own time signature says,
/// and a signature change between the two positions is counted on
/// both sides of it. Without a map, four to the bar is assumed.
// r[impl song.tempo-map] - beats between positions read the signature per point
fn beats_between(from: Bars, to: Bars, tempo: Option<&crate::music::TempoMap>) -> f32 {
    let Some(map) = tempo else {
        let flat = |p: Bars| (p.bar as f64 - 1.0) * 4.0 + (p.beat - 1.0);
        return (flat(to) - flat(from)).max(0.0) as f32;
    };
    if to <= from {
        return 0.0;
    }
    let per_bar = |p: Bars| map.at(p).time_signature.numerator.max(1) as f64;
    // Flat beat count within one signature's stretch; a segment is
    // measured in the signature that governs it.
    let flat = |p: Bars, n: f64| (p.bar as f64 - 1.0) * n + (p.beat - 1.0);
    // The change points strictly inside (from, to] split the walk.
    let mut cuts: Vec<Bars> = map
        .points()
        .iter()
        .map(|pt| pt.at)
        .filter(|at| *at > from && *at < to)
        .collect();
    cuts.push(to);
    let mut total = 0.0;
    let mut here = from;
    for next in cuts {
        let n = per_bar(here);
        total += (flat(next, n) - flat(here, n)).max(0.0);
        here = next;
    }
    total as f32
}

/// Whether a sustained relative recipe **sums** with the others on its
/// attribute instead of replacing them. Off by default, the spec's rule.
// r[impl effects.relative-stack] - opt-in, default last-wins
// r[impl effects.relative-stack]
fn stacks(recipe: &Recipe) -> bool {
    recipe.stack
}

/// The unique fixtures a cue covers, in its recipes' unit order (the
/// first recipe's fixtures first), then its direct values.
fn covered_chans(cue: &Cue, show: &Show<'_>) -> Vec<ChanId> {
    let mut seen = HashSet::new();
    let mut order = Vec::new();
    for recipe in cue.resolved_recipes(show) {
        for emit in expand_recipe(&recipe, show, 0.0) {
            if seen.insert(emit.value.chan) {
                order.push(emit.value.chan);
            }
        }
    }
    for v in &cue.values {
        if seen.insert(v.chan) {
            order.push(v.chan);
        }
    }
    order
}

/// How far ahead the pre-positioning look-ahead reads.
const MAX_LOOKAHEAD: usize = 32;

/// What one coming cue asks of its fixtures, for pre-positioning: who
/// it lights, and the absolute non-intensity values it wants.
// r[impl cues.mib] - the coming cue's wants
struct CueLook {
    lit: HashSet<ChanId>,
    wanted: Vec<(ChanId, Attribute, f32)>,
}

impl CueLook {
    fn of(cue: &Cue, show: &Show<'_>, clock: f32) -> Self {
        let mut lit = HashSet::new();
        let mut wanted = Vec::new();
        let mut note = |value: &CueValue| {
            if value.attr == Attribute::Dimmer {
                if value.value > 0.0 {
                    lit.insert(value.chan);
                }
            } else {
                wanted.push((value.chan, value.attr.clone(), value.value));
            }
        };
        for recipe in cue.resolved_recipes(show) {
            for emit in expand_recipe(&recipe, show, clock) {
                if !emit.relative {
                    note(&emit.value);
                }
            }
        }
        for v in &cue.values {
            note(v);
        }
        Self { lit, wanted }
    }
}

/// Renumbers every recipe id a layer stack holds.
fn renumber(layers: &mut Layers, map: impl Fn(usize) -> usize) {
    for source in layers.tracked.values_mut() {
        if let Source::Recipe(id) = source {
            *id = map(*id);
        }
    }
    for ids in layers.modulated.values_mut() {
        for id in ids.iter_mut() {
            *id = map(*id);
        }
    }
    for id in layers.aim.values_mut() {
        *id = map(*id);
    }
    for ids in layers.focus_modulated.values_mut() {
        for id in ids.iter_mut() {
            *id = map(*id);
        }
    }
}

/// One key a shield restates, and the value it restates.
type Restated = ((ChanId, Attribute), f32);

/// The direct values a run of cues carries to its end, by the list's
/// own rules — a block starts over, a cue-only cue is skipped.
// r[impl cues.shield] - the tracked state the shield reasons about
fn direct_state_through(cues: &[Cue]) -> HashMap<(ChanId, Attribute), f32> {
    let mut state = HashMap::new();
    for cue in cues {
        if cue.cue_only {
            continue;
        }
        apply_direct_state(&mut state, cue);
    }
    state
}

fn apply_direct_state(state: &mut HashMap<(ChanId, Attribute), f32>, cue: &Cue) {
    if cue.block {
        state.clear();
    }
    for v in &cue.values {
        state.insert((v.chan, v.attr.clone()), v.value);
    }
}

/// How many overlapping fades to carry before the oldest is forced to
/// finish. Only reachable by firing GO faster than fades complete,
/// several times over; the alternative is an unbounded stack.
// r[impl cues.fades-stack] - the bound
const MAX_FADES: usize = 8;

/// Plays a `CueList`, either stepped by `go()` or driven by `seek()`.
///
/// Has no concept of wall-clock time itself (`no_std`-friendly, matching
/// `ignition-core`'s own aspiration — see `lib.rs`): the caller hands it
/// elapsed seconds each tick, and a musical position if there is one,
/// the same way processing crates in the sibling FastTrackStudio tree
/// are always given time as data rather than reading a clock.
pub struct CuePlayer {
    cues: Vec<Cue>,
    /// Index of the cue currently playing/played-into, or `None` before the
    /// first `go()` (nothing has been fired yet — output is empty/blackout).
    current: Option<usize>,
    /// Recipes introduced by cues so far, kept alive as long as any live
    /// stage still points at them. Compacted on every `go()`, so a
    /// three-hour show does not accumulate one entry per recipe per cue.
    active: Vec<Active>,
    /// Oldest first. The last stage is the cue being moved into;
    /// everything before it is a fade still resolving.
    ///
    /// A stack rather than a single remembered snapshot because both
    /// sides of a fade have to keep *resolving*: crossing from one
    /// phaser into another, the outgoing one must keep moving while it
    /// goes. A snapshot freezes it. See
    /// `docs/domain/cue-building-architecture.md`, Decision 6.
    // r[impl cues.fades-stack]
    // r[impl cues.both-sides-resolve-live]
    stack: Vec<Stage>,
    /// Monotonic show time. Unlike a fade's `elapsed` this never resets,
    /// because a phaser free-runs across cues — restarting its cycle on
    /// every GO would make a chase stutter every time an unrelated cue
    /// fired.
    // r[impl effects.sync.shared-clock]
    // r[impl cues.seek-keeps-the-clock]
    clock: f32,
    /// Real seconds, advanced by `tick` only. Follows run on this,
    /// because a follow is "N beats after the operator pressed GO" and
    /// has nothing to do with where a transport is.
    // r[impl cues.trig] - follows run on wall time
    wall: f32,
    /// Wall time of the last live take, which a follow counts from.
    last_take_wall: Option<f32>,
    /// The Song tempo seen at the last take, for follows between takes.
    bpm: f32,
    /// Commands of cues taken live since the host last drained them.
    // r[impl cues.command]
    commands: Vec<String>,
    /// GO on the last cue takes the first.
    // r[impl cues.wrap-and-restart]
    wrap: bool,
    /// Where `enable` goes on from.
    // r[impl cues.wrap-and-restart]
    restart: Restart,
    /// Fades and the follow clock are stopped; the show clock is not.
    // r[impl playback.temp-and-pause]
    paused: bool,
    /// A cue armed by `load`, which the next `go` takes.
    armed: Option<usize>,
    /// Where a disabled list left off, for `enable`.
    resume_from: Option<usize>,
    enabled: bool,
}

impl CuePlayer {
    pub fn new(cues: Vec<Cue>) -> Self {
        Self {
            cues,
            current: None,
            active: Vec::new(),
            stack: Vec::new(),
            clock: 0.0,
            wall: 0.0,
            last_take_wall: None,
            bpm: 120.0,
            commands: Vec::new(),
            wrap: false,
            restart: Restart::First,
            paused: false,
            armed: None,
            resume_from: None,
            enabled: true,
        }
    }

    /// A player on a list, taking its wrap and restart with it.
    // r[impl cues.wrap-and-restart]
    pub fn from_list(list: &CueList) -> Self {
        let mut player = Self::new(list.cues.clone());
        player.wrap = list.wrap;
        player.restart = list.restart;
        player
    }

    /// Swaps the list under a running player without losing its place.
    ///
    /// The current cue index, the armed cue, the show clock, the wall
    /// clock, pause and enable all survive; the tracked state is rebuilt
    /// from the top of the new list up to the current cue, so the stage
    /// shows the *edited* definition at once — no GO. Every cue on the
    /// way is replayed, not performed: a one-shot in the current cue
    /// does not fire again because its values were edited, and a fade
    /// that was in flight lands (an edit is not a crossfade). A current
    /// index the new list no longer has clamps to its last cue; an
    /// empty list leaves the player before its first cue.
    // r[impl studio.program.cue-editing] - the running player takes the stored cue at once
    // r[impl cues.seek-keeps-the-clock] - the clock is not part of the list
    pub fn replace_list(&mut self, list: &CueList, show: &Show<'_>) {
        let current = self.current;
        let armed = self.armed;
        self.cues = list.cues.clone();
        self.wrap = list.wrap;
        self.restart = list.restart;
        self.reset();
        if let Some(index) = current
            && !self.cues.is_empty()
        {
            self.replay_to(index.min(self.cues.len() - 1), show);
        }
        self.armed = armed.filter(|a| *a < self.cues.len());
        self.resume_from = self.resume_from.filter(|r| *r < self.cues.len());
    }

    /// Replaces one cue in place — `replace_list` for a single edit.
    /// Past the end, the cue is appended.
    // r[impl studio.program.cue-editing]
    pub fn reload_cue(&mut self, index: usize, cue: Cue, show: &Show<'_>) {
        let mut list = CueList {
            cues: self.cues.clone(),
            wrap: self.wrap,
            restart: self.restart,
            ..Default::default()
        };
        if index < list.cues.len() {
            list.cues[index] = cue;
        } else {
            list.cues.push(cue);
        }
        self.replace_list(&list, show);
    }

    /// Replays the list from the top through `target`, every cue as a
    /// replay rather than a take, with every fade landed.
    fn replay_to(&mut self, target: usize, show: &Show<'_>) {
        while self.current != Some(target) {
            let before = self.current;
            self.take(show, false);
            if let Some(stage) = self.stack.last_mut() {
                stage.elapsed = stage.timing.span;
            }
            self.collapse();
            if self.current == before {
                break;
            }
        }
    }

    pub fn set_wrap(&mut self, wrap: bool) {
        self.wrap = wrap;
    }

    pub fn set_restart(&mut self, restart: Restart) {
        self.restart = restart;
    }

    pub fn is_paused(&self) -> bool {
        self.paused
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Stops every running fade and the follow clock where they are.
    /// The show clock keeps running, so a phaser stays in phase with
    /// the song while the operator holds.
    // r[impl playback.temp-and-pause]
    pub fn pause(&mut self) {
        self.paused = true;
    }

    /// Fades and follows go on from where `pause` held them.
    // r[impl playback.temp-and-pause]
    pub fn resume(&mut self) {
        self.paused = false;
    }

    /// Arms cue `index` without taking it: the next `go` takes it, from
    /// wherever the show is, with the tracked state that cue would have
    /// had. Out of range clears the arming.
    // r[impl playback.temp-and-pause] - stepping without losing the place
    pub fn load(&mut self, index: usize) {
        self.armed = (index < self.cues.len()).then_some(index);
    }

    /// The cue `load` armed, if any.
    pub fn loaded(&self) -> Option<usize> {
        self.armed
    }

    /// Steps back to the previous cue, crossfading into it with *its*
    /// fade, from wherever the output is now. Before the first cue there
    /// is nothing to go back to and the list clears.
    // r[impl playback.temp-and-pause] - steppable backwards without losing the place
    pub fn go_back(&mut self, show: &Show<'_>) {
        match self.current {
            Some(0) | None => {
                self.reset();
            }
            Some(i) => self.jump_take(i - 1, show),
        }
    }

    /// Takes the list off: output goes empty and where it was is kept
    /// for `enable`.
    // r[impl cues.wrap-and-restart]
    pub fn disable(&mut self) {
        if !self.enabled {
            return;
        }
        self.enabled = false;
        self.resume_from = self.current;
        self.armed = None;
        self.stack.clear();
        self.active.clear();
        self.current = None;
    }

    /// Puts the list back on, at the cue its restart names — the first,
    /// the one it was on, or the one after — taken live with that
    /// cue's fade.
    // r[impl cues.wrap-and-restart]
    pub fn enable(&mut self, show: &Show<'_>) {
        if self.enabled {
            return;
        }
        self.enabled = true;
        if self.cues.is_empty() {
            return;
        }
        let last = self.cues.len() - 1;
        let target = match (self.restart, self.resume_from.take()) {
            (Restart::First, _) | (_, None) => 0,
            (Restart::Current, Some(i)) => i.min(last),
            (Restart::Next, Some(i)) => {
                if i >= last {
                    if self.wrap { 0 } else { last }
                } else {
                    i + 1
                }
            }
        };
        self.jump_take(target, show);
    }

    /// Takes cue `index` from wherever the show is: the layers the cue
    /// would have had are rebuilt from the top of the list, then pushed
    /// as a live stage over the current output, so the crossfade runs
    /// from what is showing rather than from dark.
    fn jump_take(&mut self, index: usize, show: &Show<'_>) {
        if index >= self.cues.len() {
            return;
        }
        let mut scratch = CuePlayer::new(self.cues.clone());
        scratch.clock = self.clock;
        scratch.wall = self.wall;
        if index > 0 {
            scratch.jump_to_end_of(index - 1, show);
        }
        scratch.take(show, true);
        let Some(mut stage) = scratch.stack.pop() else {
            return;
        };
        let offset = self.active.len();
        self.active.extend(scratch.active);
        renumber(&mut stage.layers, |id| id + offset);
        if let Some(base) = &mut stage.base {
            renumber(base, |id| id + offset);
        }
        stage.elapsed = 0.0;
        self.stack.push(stage);
        while self.stack.len() > MAX_FADES {
            self.stack.remove(0);
        }
        self.commands.extend(scratch.commands);
        self.last_take_wall = Some(self.wall);
        self.bpm = scratch.bpm;
        self.current = Some(index);
        self.collapse();
        self.compact();
    }

    /// Host commands from every cue taken live since the last call.
    // r[impl cues.command]
    pub fn drain_commands(&mut self) -> Vec<String> {
        std::mem::take(&mut self.commands)
    }

    /// The band the next cue is waiting to hear, if its trigger is a
    /// sound. The host listens, and calls `go` when it lands.
    // r[impl cues.trig] - sound
    pub fn pending_sound_trig(&self) -> Option<&str> {
        let next = self.next_index()?;
        if self.current == Some(next) {
            return None;
        }
        match &self.cues[next].trig {
            Trig::Sound { band } => Some(band.as_str()),
            _ => None,
        }
    }

    /// Wall seconds until the next cue follows on its own, if it is a
    /// follow. Zero or less means it is due.
    fn follow_due_in(&self) -> Option<f32> {
        let next = self.next_index()?;
        if self.current == Some(next) {
            return None;
        }
        let Trig::Follow { beats } = self.cues[next].trig else {
            return None;
        };
        let since = self.last_take_wall?;
        Some(since + beats * 60.0 / self.bpm - self.wall)
    }

    /// How far into its arrival the standing cue is, 0 to 1.
    ///
    /// One means everything it set has landed — which is what the list
    /// draws as a filled row. A cue with no fade is arrived the moment
    /// it is taken, so it reports 1 rather than dividing by zero.
    // r[impl studio.cuelist.live-state]
    pub fn fade_progress(&self) -> f32 {
        match self.stack.last() {
            Some(stage) if stage.timing.span > 0.0 => {
                (stage.elapsed / stage.timing.span).clamp(0.0, 1.0)
            }
            _ => 1.0,
        }
    }

    /// Wall seconds until the next cue takes *itself* — a follow
    /// counting down. `None` for a cue somebody has to press GO on,
    /// which is most of them.
    // r[impl studio.cuelist.live-state]
    // r[impl cues.trig] - a follow is the trigger an operator cannot see coming
    pub fn next_in(&self) -> Option<f32> {
        self.follow_due_in().map(|s| s.max(0.0))
    }

    /// Which cue a GO would take next, for the list to count down on.
    pub fn next_cue(&self) -> Option<usize> {
        self.next_index()
    }

    /// Advances fades *and* takes any follow that has come due. Follows
    /// chain: a run of follow cues is taken one after another at their
    /// own spacing, each counted from the moment the previous was due
    /// rather than from the frame it was noticed, so they do not drift.
    // r[impl cues.trig] - follow, chained
    pub fn tick_with(&mut self, dt_secs: f32, show: &Show<'_>) {
        self.tick(dt_secs);
        while let Some(late) = self.follow_due_in()
            && late <= 0.0
        {
            let due = self.wall + late;
            self.take(show, true);
            self.last_take_wall = Some(due);
            // The take's own fade has already been running for `-late`.
            if let Some(stage) = self.stack.last_mut() {
                stage.elapsed = -late;
            }
        }
        self.collapse();
    }

    /// Advances playback into the next cue, if there is one. No-op at the
    /// end of the list (matches a real console's "GO" on the last cue —
    /// nothing to advance into, stays put).
    /// The index `go` would move to next.
    // r[impl cues.wrap-and-restart] - past the last cue, the first when wrapping
    fn next_index(&self) -> Option<usize> {
        if let Some(armed) = self.armed {
            return Some(armed);
        }
        match self.current {
            Some(i) if i + 1 < self.cues.len() => Some(i + 1),
            Some(_) if self.wrap => Some(0),
            Some(i) => Some(i),
            None => (!self.cues.is_empty()).then_some(0),
        }
    }

    // r[impl song.two-ways] - GO by hand; `seek` is the clocked way onto the same list
    // r[impl cues.wrap-and-restart] - wraps when the list says so
    pub fn go(&mut self, show: &Show<'_>) {
        if let Some(armed) = self.armed.take() {
            if self.current.map(|c| c + 1) == Some(armed) {
                self.take(show, true);
            } else {
                self.jump_take(armed, show);
            }
            return;
        }
        self.take(show, true)
    }

    /// How far in the past a replayed one-shot is stamped.
    ///
    /// Long enough that any envelope at any tempo has finished, so
    /// `contributing` drops it immediately. A plain number rather than
    /// an infinity, which would make `clock - started` produce a NaN the
    /// first time somebody did arithmetic on it.
    const LONG_FINISHED: f32 = 3600.0;

    // r[impl recipes.cook-fixes-coverage] - cooking fixes which (chan, attr) a cue owns
    // r[impl cues.tracking]
    // r[impl recipes.cascade]
    // r[impl cues.fades-stack]
    fn take(&mut self, show: &Show<'_>, live: bool) {
        let mut next = self.current.map_or(0, |i| i + 1);
        if next >= self.cues.len() {
            // r[impl cues.wrap-and-restart] - GO on the last cue takes the first
            if self.wrap && !self.cues.is_empty() {
                next = 0;
            } else {
                return;
            }
        }
        let cue = self.cues[next].clone();
        let bpm = song_bpm(show);
        self.bpm = bpm;
        let spb = 60.0 / bpm;

        // Tracking: start from wherever the show already is, unless this
        // cue blocks. A cue-only predecessor is skipped over: the show
        // "already is" wherever it was before that cue.
        // r[impl cues.block]
        // r[impl recipes.blocking-resets]
        // r[impl cues.tracking] - otherwise start from where the show already is
        // r[impl cues.cue-only] - the next cue starts from before it
        let before = self
            .stack
            .last()
            .map(|s| s.base.clone().unwrap_or_else(|| s.layers.clone()))
            .unwrap_or_default();
        let mut layers = if cue.block {
            Layers::default()
        } else {
            before.clone()
        };
        // A pause on a running effect belongs to the dark window of the
        // cue that was showing; this take is a new window.
        layers.frozen.clear();

        // Break: for these attributes, what came before does not track
        // through this cue on the fixtures it covers. The source is
        // dropped, not released — nothing is faded to rest on purpose;
        // the cue simply does not carry it.
        // r[impl cues.break]
        if !cue.break_.is_empty() {
            let covered: HashSet<ChanId> = covered_chans(&cue, show).into_iter().collect();
            let broken =
                |key: &(ChanId, Attribute)| cue.break_.contains(&key.1) && covered.contains(&key.0);
            layers.tracked.retain(|key, _| !broken(key));
            layers.modulated.retain(|key, _| !broken(key));
            let focus_broken =
                |chan: ChanId| broken(&(chan, Attribute::Pan)) || broken(&(chan, Attribute::Tilt));
            layers.aim.retain(|chan, _| !focus_broken(*chan));
            layers
                .focus_modulated
                .retain(|chan, _| !focus_broken(*chan));
        }

        // Assert: what this cue would only track is re-taken as its
        // own. Recipe sources get a fresh entry stamped now, so a
        // one-shot plays again and a phaser is this cue's to fade.
        // r[impl cues.assert]
        if cue.assert && !cue.block {
            let mut retaken: HashMap<usize, usize> = HashMap::new();
            let mut retake = |old: usize, active: &mut Vec<Active>, clock: f32| -> usize {
                *retaken.entry(old).or_insert_with(|| {
                    let mut fresh = active[old].clone();
                    fresh.since = if live {
                        clock
                    } else {
                        clock - Self::LONG_FINISHED
                    };
                    active.push(fresh);
                    active.len() - 1
                })
            };
            for source in layers.tracked.values_mut() {
                if let Source::Recipe(id) = source {
                    *id = retake(*id, &mut self.active, self.clock);
                }
            }
            for ids in layers.modulated.values_mut() {
                for id in ids.iter_mut() {
                    *id = retake(*id, &mut self.active, self.clock);
                }
            }
            for id in layers.aim.values_mut() {
                *id = retake(*id, &mut self.active, self.clock);
            }
            for ids in layers.focus_modulated.values_mut() {
                for id in ids.iter_mut() {
                    *id = retake(*id, &mut self.active, self.clock);
                }
            }
        }

        // The fan is laid across the fixtures the cue covers, in the
        // order its first recipe lists them (then the rest, then the
        // direct values). Each fixture's delay is remembered on the
        // recipes taken here, so their phase runs from that start.
        // r[impl cues.fan]
        // r[impl cues.delay-to-phase]
        let mut timing = StageTiming::from_cue(&cue, bpm);
        // A selection's own timing, resolved now against this show; a
        // fixture several name takes the last.
        // r[impl cues.timing-overrides]
        for over in &cue.timing_overrides {
            let table = ClassTable::new(&over.timing, cue.fade_secs, spb);
            for chan in
                crate::selection::resolve_with(&over.selection, show.groups, show.rig, show.roles)
            {
                timing.overrides.insert(chan, table.clone());
            }
        }
        let mut unit_delay: HashMap<ChanId, f32> = HashMap::new();
        if let Some(fan) = cue.fan {
            let order = covered_chans(&cue, show);
            let count = order.len();
            for (i, chan) in order.into_iter().enumerate() {
                let delay = fan.delay.at(i, count) * spb;
                let fade = (!fan.fade.is_flat() || fan.fade.from > 0.0)
                    .then(|| fan.fade.at(i, count) * spb);
                timing.per_chan.insert(chan, (delay, fade));
                if delay > 0.0 {
                    unit_delay.insert(chan, delay);
                }
            }
        }

        // Cook: resolving a recipe now is what establishes *which*
        // (chan, attr) pairs it covers, so tracking knows what it owns.
        // The values it produces are re-resolved every frame — cooking
        // fixes coverage, not output.
        // References are looked up now, through this show's library —
        // the same moment a group name is resolved.
        // Every key this cue itself sets, for a same-position merge.
        let mut owned: HashSet<(ChanId, Attribute)> = HashSet::new();
        // r[impl effects.library.by-name] - resolved at take
        for recipe in cue.resolved_recipes(show) {
            let id = self.active.len();
            // A replayed one-shot is stamped as long finished, so it
            // contributes nothing and the value beneath it stands.
            // Looping recipes are unaffected: they run on the shared
            // clock and do not read this at all.
            //
            // r[impl cues.replay-does-not-perform]
            let since = if live || !recipe.timing.once {
                self.clock
            } else {
                self.clock - Self::LONG_FINISHED
            };
            self.active.push(Active {
                recipe: recipe.clone(),
                since,
                unit_delay: unit_delay.clone(),
            });
            let expansion = expand_recipe_full(&recipe, show, self.recipe_time(id));
            let aimed: HashSet<ChanId> = expansion.focus_points.iter().map(|(c, _)| *c).collect();
            // A recipe carrying its own arrival *is* a cue part: every
            // key it covers takes its fade, delay and ease instead of
            // the cue's. Later parts overwrite earlier ones on a shared
            // key, so the same rule decides a key's timing as decides
            // its value.
            // r[impl cues.recipe.timing]
            // r[impl cues.recipe.timing-precedence]
            // r[impl cues.parts-are-recipes]
            let part_table = recipe
                .cue_timing
                .map(|t| ClassTable::new(&t, cue.fade_secs, spb));
            for emit in expansion.emits {
                let key = (emit.value.chan, emit.value.attr);
                if let Some(table) = &part_table {
                    timing.per_recipe.insert(key.clone(), table.clone());
                }
                owned.insert(key.clone());
                // r[impl recipes.relative-is-a-separate-layer]
                if emit.relative {
                    let slot = layers.modulated.entry(key).or_default();
                    // A recipe re-taken by a later cue moves to the end
                    // rather than stacking on itself — re-stating a
                    // chase should not double it.
                    // r[impl recipes.relative-last-wins] - later take moves to the end
                    slot.retain(|existing| *existing != id);
                    slot.push(id);
                } else {
                    // Pan or tilt set without a point — an orientation,
                    // a raw value — is an aim the player cannot offset.
                    // It also takes the slot outright: the later of a
                    // point and an orientation on one fixture wins, and
                    // the two are never averaged.
                    // r[impl focus.point-beats-orientation] - the later take owns pan/tilt outright
                    // r[impl focus.delta] - a fixture with no point aim ignores the delta
                    if matches!(key.1, Attribute::Pan | Attribute::Tilt) && !aimed.contains(&key.0)
                    {
                        layers.aim.remove(&key.0);
                    }
                    // r[impl recipes.absolute-last-wins]
                    // r[impl playback.absolute-layer]
                    // r[impl cues.tracking-carries-sources]
                    layers.tracked.insert(key, Source::Recipe(id));
                }
            }
            // r[impl focus.delta] - the aim is tracked beside the angles
            for chan in aimed {
                layers.aim.insert(chan, id);
            }
            // r[impl focus.delta] - a lone delta is tracked against the aim, last-wins per class
            for d in expansion.focus_deltas {
                let slot = layers.focus_modulated.entry(d.chan).or_default();
                slot.retain(|existing| *existing != id);
                slot.push(id);
            }
        }
        // Layer 1 last, so a direct value on this cue beats a recipe on
        // the same cue. The cascade is an ordering, not a merge.
        // r[impl recipes.cascade] - a direct value on the cue beats a recipe on it
        // r[impl playback.absolute-layer]
        for v in &cue.values {
            owned.insert((v.chan, v.attr.clone()));
            if matches!(v.attr, Attribute::Pan | Attribute::Tilt) {
                layers.aim.remove(&v.chan);
            }
            layers
                .tracked
                .insert((v.chan, v.attr.clone()), Source::Direct(v.value));
        }

        // Release: stop asserting an attribute on the fixtures this cue
        // covers (every tracked fixture when it covers none). The key
        // leaves the layers, so `blend` fades it to rest and whatever
        // is beneath this player shows through.
        // r[impl cues.release]
        // r[impl playback.release-falls-through]
        if !cue.release.is_empty() {
            let covered = covered_chans(&cue, show);
            let everyone = covered.is_empty();
            let covered: HashSet<ChanId> = covered.into_iter().collect();
            let released = |key: &(ChanId, Attribute)| {
                cue.release.contains(&key.1) && (everyone || covered.contains(&key.0))
            };
            layers.tracked.retain(|key, _| !released(key));
            layers.modulated.retain(|key, _| !released(key));
            let focus_released = |chan: ChanId| {
                released(&(chan, Attribute::Pan)) || released(&(chan, Attribute::Tilt))
            };
            layers.aim.retain(|chan, _| !focus_released(*chan));
            layers
                .focus_modulated
                .retain(|chan, _| !focus_released(*chan));
        }

        // Move in black: a fixture dark after this cue and lit by the
        // next is sent to the next cue's position, colour and beam now,
        // on the pre-position's own timing, so the move is never seen.
        // Intensity is never touched. Only the absolute layer is
        // pre-applied; a modulation on a dark fixture is nothing to see.
        // r[impl cues.mib]
        // r[impl cues.mib.mode]
        // r[impl cues.mib.timing]
        //
        // The look-ahead: for each fixture dark after this cue, the
        // first later cue that lights it is the one whose values are
        // wanted, and the cues between are the dark window. The window
        // rates its cues by preference, and the move happens in the
        // best-rated one — this take only if this cue is it.
        // r[impl cues.mib]
        // r[impl cues.mib.mode]
        // r[impl cues.mib.timing]
        // r[impl cues.mib.preference] - the best-rated dark window
        // r[impl cues.mib.hold] - a holding cue moves nothing
        if !cue.mib.hold && next + 1 < self.cues.len() {
            let now = self.resolve(&layers, show, &HashSet::new());
            let dark_now = |chan: ChanId| {
                now.get(&(chan, Attribute::Dimmer))
                    .is_none_or(|v| *v <= 0.0)
            };
            // Upon-go moves with the take *after* the fixture went dark:
            // it must have been dark before this cue as well.
            let dark_before: HashSet<ChanId> = if cue.block {
                HashSet::new()
            } else {
                let was = self.resolve(&before, show, &HashSet::new());
                was.iter()
                    .filter(|((_, attr), v)| *attr == Attribute::Dimmer && **v <= 0.0)
                    .map(|((chan, _), _)| *chan)
                    .collect()
            };
            let looks: Vec<CueLook> = self.cues[next + 1..]
                .iter()
                .take(MAX_LOOKAHEAD)
                .map(|c| CueLook::of(c, show, self.clock))
                .collect();
            let mut moved: HashSet<ChanId> = HashSet::new();
            for (ahead, look) in looks.iter().enumerate() {
                let coming = &self.cues[next + 1 + ahead];
                if coming.mib.mode == MibMode::None {
                    // This cue lights its fixtures in the light; they
                    // are no longer dark for anything after it.
                    moved.extend(look.lit.iter().copied());
                    continue;
                }
                // The window this fixture is dark through: this cue and
                // every one before `coming`. The best-rated is where the
                // move happens; a tie goes to the earliest.
                let window =
                    std::iter::once(&cue).chain(self.cues[next + 1..next + 1 + ahead].iter());
                let best = window
                    .enumerate()
                    .max_by(|(ia, a), (ib, b)| {
                        a.mib.preference.cmp(&b.mib.preference).then(ib.cmp(ia))
                    })
                    .map(|(i, _)| i);
                let fade = coming.mib.fade_beats.max(0.0) * spb;
                let mut delay = coming.mib.delay_beats.max(0.0) * spb;
                if coming.mib.mode == MibMode::Late
                    && let (Some(here), Some(there)) = (cue.position(), coming.position())
                {
                    // As late as its own fade allows: finish on the coming
                    // cue's downbeat, never earlier than its own delay.
                    let gap = beats_between(here, there, show.tempo) * spb;
                    delay = delay.max(gap - fade);
                }
                for (chan, attr, value) in &look.wanted {
                    let chan = *chan;
                    if !look.lit.contains(&chan) || moved.contains(&chan) || !dark_now(chan) {
                        continue;
                    }
                    // r[impl cues.mib.preference] - upon-go: the take after the one that darkened it
                    if coming.mib.mode == MibMode::UponGo && !dark_before.contains(&chan) {
                        continue;
                    }
                    if best != Some(0) {
                        continue; // a better-rated cue in the window will
                    }
                    let key = (chan, attr.clone());
                    if now.get(&key).is_some_and(|v| (*v - value).abs() < 1e-6) {
                        continue; // already there
                    }
                    timing.per_key.insert(key.clone(), (delay, fade));
                    layers.tracked.insert(key, Source::Direct(*value));
                    // r[impl cues.mib.multistep] - paused holds the effect's phase while dark
                    if coming.mib.multistep == MultiStep::Paused {
                        layers.frozen.entry(chan).or_insert(self.clock);
                    }
                }
                // Whatever this cue lights, later cues see lit.
                moved.extend(look.lit.iter().copied());
            }
        }
        timing.recompute_span();

        if live {
            self.commands.extend(cue.commands.iter().cloned());
            self.last_take_wall = Some(self.wall);
        }

        // Cues at one position are one frame's work. A zero-fade accent
        // on the downbeat of a section otherwise arrives instantly and
        // collapses the section's fade beneath it. So it joins the
        // section's stage: its own keys take its own timing, measured
        // from now, and everything else keeps fading as it was.
        // r[impl cues.same-position-is-one-take]
        let same_position = next > 0
            && cue.position().is_some()
            && self.cues[next - 1].position() == cue.position()
            && self.current == Some(next - 1);
        if same_position && let Some(last) = self.stack.last_mut() {
            let elapsed = last.elapsed;
            for key in owned {
                let (delay, fade) = timing.for_key(&key, false);
                last.timing.per_key.insert(key, (delay + elapsed, fade));
            }
            for (key, (delay, fade)) in timing.per_key {
                last.timing.per_key.insert(key, (delay + elapsed, fade));
            }
            last.timing.recompute_span();
            last.layers = layers;
            if cue.cue_only {
                last.base = Some(before);
            }
        } else {
            self.stack.push(Stage {
                layers,
                base: cue.cue_only.then_some(before),
                timing,
                elapsed: 0.0,
            });
        }
        // r[impl cues.fades-stack] - the oldest is forced to finish past the bound
        while self.stack.len() > MAX_FADES {
            self.stack.remove(0);
        }
        self.collapse();
        self.compact();
        self.current = Some(next);
    }

    /// Jumps straight to the end of cue `index`'s fade (as if `go()` had
    /// been called `index + 1` times and enough time had passed for each
    /// fade to finish) — for headless/automated testing and snapshotting a
    /// specific point in a show without stepping through every cue with
    /// real elapsed time. Out-of-range `index` clamps to the last cue.
    pub fn jump_to_end_of(&mut self, index: usize, show: &Show<'_>) {
        let target = index.min(self.cues.len().saturating_sub(1));
        while self.current != Some(target) {
            let before = self.current;
            // Every cue but the one being landed on is *replayed*, not
            // taken. Replaying rebuilds the tracked state a live show
            // would have arrived at; it must not re-perform the
            // transient events along the way.
            //
            // This was the bug behind a figure that appeared to do
            // nothing. Reaching `fig 0 · 2/3` replays `1/3`, and `go`
            // stamps every one-shot with the clock *now* — so the first
            // third's cut re-fired at the same instant as the second's.
            // By the third hit all three zones were flashing together,
            // which is the whole rig, which reads as nothing happening
            // at all. The cutout was working perfectly; it was being
            // played three times at once.
            let live = self.next_index() == Some(target);
            self.take(show, live);
            if let Some(stage) = self.stack.last_mut() {
                stage.elapsed = stage.timing.span;
            }
            self.collapse();
            if self.current == before {
                break; // ran off the end of the list
            }
        }
    }

    /// The index of the last cue at or before a musical position.
    ///
    /// Cues without an `at` are invisible to this: a list can mix
    /// positioned cues with hand-only ones, and the unpositioned ones
    /// simply never come up under a clock.
    // r[impl cues.unpositioned-are-invisible-to-seek]
    // r[impl cues.position]
    pub fn index_at(&self, position: Bars) -> Option<usize> {
        self.cues
            .iter()
            .enumerate()
            .rev()
            .find(|(_, cue)| cue.position().is_some_and(|at| at <= position))
            .map(|(i, _)| i)
    }

    /// Puts playback where a musical position says it should be.
    ///
    /// This — not `go` — is what makes a synced show survive being
    /// looped, restarted, or jumped around in. An event-driven player
    /// that has been *told* to fire cues 1..9 has no answer to "we went
    /// back to bar 22" except replaying its own history. Asking instead
    /// "what is the state at bar 22" has one answer, and it is the same
    /// answer however you arrived.
    ///
    /// Cheap to call every frame: it returns immediately unless the
    /// position implies a different cue than the one already current.
    /// Seeking *backwards* rebuilds from the top of the list, because
    /// tracking means a cue's state is the sum of everything before it.
    // r[impl cues.seek]
    // r[impl cues.seek-is-cheap-when-still]
    // r[impl cues.position]
    // r[impl cues.unpositioned-are-invisible-to-seek] - still applied when replayed past
    // r[impl song.two-ways] - the clocked way; `go` is the hand way onto the same list
    pub fn seek(&mut self, position: Bars, show: &Show<'_>) {
        let Some(target) = self.index_at(position) else {
            // Before the first positioned cue — nothing has happened yet.
            if self.current.is_some() {
                self.reset();
            }
            return;
        };
        if self.current == Some(target) {
            return;
        }
        if self.current.is_none_or(|current| current > target) {
            self.reset();
        }
        self.jump_to_end_of(target, show);
    }

    /// Back to before the first cue, keeping the show clock — a phaser
    /// that is running should not restart because the operator jumped
    /// the transport.
    // r[impl cues.seek-keeps-the-clock]
    fn reset(&mut self) {
        self.current = None;
        self.stack.clear();
        self.active.clear();
    }

    // r[impl cues.fade-is-wall-time] - fades advance on real elapsed seconds
    // r[impl playback.temp-and-pause] - paused, fades and the follow clock hold; the show clock runs
    pub fn tick(&mut self, dt_secs: f32) {
        self.clock += dt_secs;
        if self.paused {
            return;
        }
        self.wall += dt_secs;
        for stage in &mut self.stack {
            stage.elapsed += dt_secs;
        }
        self.collapse();
    }

    /// Advances the show clock without advancing any fade — for
    /// snapshotting a running phaser at a chosen moment.
    pub fn advance_clock(&mut self, secs: f32) {
        self.clock += secs;
    }

    pub fn clock(&self) -> f32 {
        self.clock
    }

    /// Drives the show clock from an external timeline — the song.
    ///
    /// Free-running, the clock starts at zero when the app does and has
    /// no relationship to the music: an effect written as "one cycle per
    /// bar" then runs at the right *rate* with an arbitrary *phase*, so
    /// a snare pulse written on beats two and four lands on whatever
    /// beat the app happened to launch on. And because it advances every
    /// frame regardless, the pulse went on flashing after the song was
    /// stopped.
    ///
    /// Handing it the song's own position fixes both at once, and a
    /// third thing for free: a scrub moves the clock with it, so effects
    /// arrive already in the phase they would have had if the song had
    /// played there.
    ///
    /// Fades are deliberately untouched — a two-second fade is two
    /// *real* seconds whether or not a song is playing, so `tick` still
    /// owns `Stage::elapsed`.
    // r[impl effects.sync.follows-the-song]
    // r[impl cues.fade-is-wall-time] - fades are untouched by the transport clock
    // r[impl cues.seek-keeps-the-clock] - the transport drives the clock, not the seek
    pub fn set_clock(&mut self, secs: f32) {
        self.clock = secs;
    }

    /// Drops fades that have finished. Once a stage is fully arrived,
    /// everything under it contributes nothing.
    // r[impl cues.arrived-collapses]
    fn collapse(&mut self) {
        if let Some(last) = self.stack.iter().rposition(|s| s.progress() >= 1.0)
            && last > 0
        {
            self.stack.drain(..last);
        }
    }

    /// Drops recipes no live stage points at any more, renumbering the
    /// rest.
    ///
    /// Without this, `active` gains an entry per recipe per `go()` for
    /// the length of the show and never gives one back — invisible at
    /// eleven cues, a real leak over a three-hour service.
    fn compact(&mut self) {
        let referenced = |layers: &Layers| -> Vec<usize> {
            layers
                .tracked
                .values()
                .filter_map(|s| match s {
                    Source::Recipe(id) => Some(*id),
                    Source::Direct(_) => None,
                })
                .chain(layers.modulated.values().flatten().copied())
                .chain(layers.aim.values().copied())
                .chain(layers.focus_modulated.values().flatten().copied())
                .collect()
        };
        let live: HashSet<usize> = self
            .stack
            .iter()
            .flat_map(|stage| {
                let mut ids = referenced(&stage.layers);
                if let Some(base) = &stage.base {
                    ids.extend(referenced(base));
                }
                ids
            })
            .collect();
        if live.len() == self.active.len() {
            return;
        }
        let mut remap = HashMap::with_capacity(live.len());
        let mut kept = Vec::with_capacity(live.len());
        for (id, recipe) in self.active.drain(..).enumerate() {
            if live.contains(&id) {
                remap.insert(id, kept.len());
                kept.push(recipe);
            }
        }
        self.active = kept;
        for stage in &mut self.stack {
            renumber(&mut stage.layers, |id| remap[&id]);
            if let Some(base) = &mut stage.base {
                renumber(base, |id| remap[&id]);
            }
        }
    }

    pub fn current_index(&self) -> Option<usize> {
        self.current
    }

    pub fn current_name(&self) -> Option<&str> {
        self.current
            .and_then(|i| self.cues.get(i))
            .map(|c| c.name.as_str())
    }

    pub fn cues(&self) -> &[Cue] {
        &self.cues
    }

    /// The `(chan, attr) -> value` output right now.
    ///
    /// Every stage in the stack resolves live and they are folded oldest
    /// to newest, so a phaser being faded *out of* keeps moving while it
    /// goes, exactly like the one being faded into.
    ///
    /// Deliberately a pure function of (stack, `show`, clock) with
    /// nothing cached between calls. That is affordable at this rig's
    /// scale and it is what keeps a memoisation layer a legal
    /// optimisation later rather than a rewrite — see
    /// `docs/domain/cue-building-architecture.md`, Decision 1.
    // r[impl cues.both-sides-resolve-live]
    // r[impl cues.fades-stack] - the fold of every live fade, oldest first
    // r[impl playback.output-is-pure]
    // r[impl recipes.cook-fixes-coverage] - values re-resolved every frame
    pub fn output(&self, show: &Show<'_>) -> HashMap<(ChanId, Attribute), f32> {
        self.output_under(show, &HashSet::new())
    }

    /// Output with the sustained modulators on `transient` keys held off.
    ///
    /// A transient outranks a sustained effect whichever layer it lives
    /// in. Hits ring on the trigger bus, above this player, and they add
    /// to what it produces — so a chase still swinging underneath would
    /// otherwise eat the hit: a reveal on a fixture the chase had at
    /// −0.9 landed at a third of its level. The bus says which keys are
    /// ringing; those keys resolve as though the chase were not there,
    /// and it resumes at its own phase the moment the hit withdraws.
    ///
    /// r[impl playback.transient-over-sustained] - across layers
    pub fn output_under(
        &self,
        show: &Show<'_>,
        transient: &HashSet<(ChanId, Attribute)>,
    ) -> HashMap<(ChanId, Attribute), f32> {
        self.fold(show, transient, &HashMap::new()).0
    }

    /// Output over a floor of defaults — the player's half of
    /// `r[playback.defaults]`. A key nothing has set, or that a cue
    /// released, reads its default rather than nothing; a released key
    /// fades *to* its default over the class's timing; and before the
    /// first cue the output *is* the defaults map — the list's implicit
    /// cue zero.
    // r[impl playback.defaults] - the floor a released attribute falls to, and cue zero
    pub fn output_with_defaults(
        &self,
        show: &Show<'_>,
        defaults: &HashMap<(ChanId, Attribute), f32>,
    ) -> HashMap<(ChanId, Attribute), f32> {
        self.fold(show, &HashSet::new(), defaults).0
    }

    /// The colour intent each fixture's winning colour source carries,
    /// for a host to solve emitters against — never the RGB triple
    /// re-derived from three floats. Mid-fade between two intents the
    /// chromaticity and luminance crossfade in xyY; settled, the intent
    /// is exactly what the preset stored. A fixture whose colour is a
    /// direct value has no intent here and is solved from its triple.
    // r[impl color.intent-to-output] - the intent survives to output
    pub fn output_intents(&self, show: &Show<'_>) -> HashMap<ChanId, Intent> {
        self.fold(show, &HashSet::new(), &HashMap::new()).1
    }

    /// The fold every output is: each stage in the stack resolved live
    /// and crossfaded over the one before, with `defaults` as the
    /// ground the first stage rises from and a leaving key falls to.
    fn fold(
        &self,
        show: &Show<'_>,
        transient: &HashSet<(ChanId, Attribute)>,
        defaults: &HashMap<(ChanId, Attribute), f32>,
    ) -> (HashMap<(ChanId, Attribute), f32>, HashMap<ChanId, Intent>) {
        let mut out: HashMap<(ChanId, Attribute), f32> = defaults.clone();
        let mut aims: HashMap<ChanId, Vec3> = HashMap::new();
        let mut intents: HashMap<ChanId, Intent> = HashMap::new();
        for stage in &self.stack {
            let resolved = self.resolve_with_aims(&stage.layers, show, transient);
            let (target, target_aims, target_intents) =
                (resolved.values, resolved.aims, resolved.intents);
            let mut next = blend(&out, &target, stage, defaults);
            intents = blend_intents(&intents, &target_intents, stage);
            // A beam moving between two *points* travels the straight
            // line between them: the point is interpolated in the room
            // and solved per frame, instead of pan and tilt each
            // interpolating on their own and the beam bowing across the
            // stage. A fixture only one side aims at a point falls back
            // to the angle crossfade `blend` already did.
            // r[impl focus.straight-line]
            // r[impl focus.resolve-at-output] - solved per frame, mid-fade
            let mut next_aims = HashMap::with_capacity(target_aims.len());
            for (chan, to) in target_aims {
                let point = match aims.get(&chan) {
                    Some(from) => {
                        let t = stage.progress_for(&(chan, Attribute::Pan), false);
                        let p = crate::focus::v_add(
                            *from,
                            crate::focus::v_scale(
                                crate::focus::v_add(to, crate::focus::v_scale(*from, -1.0)),
                                t as f64,
                            ),
                        );
                        if let Some(place) = show.rig.placement(chan) {
                            let (pan, tilt) = crate::focus::pan_tilt_deg_to_point(
                                place.position,
                                place.orientation,
                                p,
                            );
                            next.insert((chan, Attribute::Pan), pan);
                            next.insert((chan, Attribute::Tilt), tilt);
                        }
                        p
                    }
                    None => to,
                };
                next_aims.insert(chan, point);
            }
            out = next;
            aims = next_aims;
        }
        (out, intents)
    }

    /// One recipe's current values, keyed by `(chan, attr)`.
    ///
    /// A recipe taken with a delay fan is evaluated once per distinct
    /// delay, each fixture reading the time since *its own* start, so
    /// the phase spread and the wipe agree about who is first.
    // r[impl cues.delay-to-phase]
    fn expand_active(
        &self,
        id: usize,
        show: &Show<'_>,
        frozen: &HashMap<ChanId, f32>,
    ) -> ActiveOut {
        let active = &self.active[id];
        let secs = self.recipe_time(id);
        let collect = |at: f32| ActiveOut::from(expand_recipe_full(&active.recipe, show, at));
        let mut out = if active.unit_delay.is_empty() || !active.recipe.is_phaser() {
            collect(secs)
        } else {
            let mut out = collect(secs);
            out.retain(|chan| !active.unit_delay.contains_key(&chan));
            let mut delays: Vec<f32> = active.unit_delay.values().copied().collect();
            delays.sort_by(|a, b| a.total_cmp(b));
            delays.dedup();
            for delay in delays {
                let mut late = collect(secs - delay);
                late.retain(|chan| active.unit_delay.get(&chan) == Some(&delay));
                out.extend(late);
            }
            out
        };
        // A fixture whose effect is paused while dark reads the recipe
        // at the moment it was frozen, its own delay still honoured.
        // r[impl cues.mib.multistep] - paused holds the phase
        if active.recipe.is_phaser() && frozen.keys().any(|chan| out.covers(*chan)) {
            let mut times: Vec<(ChanId, f32)> = frozen
                .iter()
                .filter(|(chan, _)| out.covers(**chan))
                .map(|(chan, at)| {
                    let own = active.unit_delay.get(chan).copied().unwrap_or(0.0);
                    (*chan, secs - (self.clock - at) - own)
                })
                .collect();
            times.sort_by(|a, b| a.1.total_cmp(&b.1));
            for (chan, at) in times {
                out.retain(|c| c != chan);
                let mut held = collect(at);
                held.retain(|c| c == chan);
                out.extend(held);
            }
        }
        out
    }

    /// One stage's layer stack, resolved through the cascade.
    // r[impl recipes.cascade]
    // r[impl recipes.relative-is-a-separate-layer]
    // r[impl playback.relative-classes]
    // r[impl cues.tracking-carries-sources]
    // r[impl playback.output-is-pure]
    fn resolve(
        &self,
        layers: &Layers,
        show: &Show<'_>,
        transient: &HashSet<(ChanId, Attribute)>,
    ) -> HashMap<(ChanId, Attribute), f32> {
        self.resolve_with_aims(layers, show, transient).values
    }

    /// `resolve`, plus the room point each aimed channel is looking at
    /// this frame (delta folded in) — what the fade needs to move a beam
    /// in a straight line rather than bow its angles.
    // r[impl focus.straight-line] - the aim is resolved beside the angles so the fade can interpolate it
    fn resolve_with_aims(
        &self,
        layers: &Layers,
        show: &Show<'_>,
        transient: &HashSet<(ChanId, Attribute)>,
    ) -> Resolved {
        let mut aims: HashMap<ChanId, Vec3> = HashMap::new();
        let mut intents: HashMap<ChanId, Intent> = HashMap::new();
        // Resolve only the recipes something still points at, once each,
        // rather than once per attribute they cover.
        let mut resolved: HashMap<usize, ActiveOut> = HashMap::new();
        let referenced = layers
            .tracked
            .values()
            .filter_map(|s| match s {
                Source::Recipe(id) => Some(*id),
                Source::Direct(_) => None,
            })
            .chain(layers.modulated.values().flatten().copied())
            .chain(layers.aim.values().copied())
            .chain(layers.focus_modulated.values().flatten().copied());
        for id in referenced {
            resolved
                .entry(id)
                .or_insert_with(|| self.expand_active(id, show, &layers.frozen));
        }

        // The intent the winning colour source carries, per fixture.
        // r[impl color.intent-to-output] - read off the winning source, never the triple
        for (key, source) in &layers.tracked {
            if AttrClass::of(&key.1) != AttrClass::Colour {
                continue;
            }
            if let Source::Recipe(id) = source
                && let Some(intent) = resolved[id].intents.get(&key.0)
            {
                intents.insert(key.0, intent.clone());
            }
        }

        // r[impl playback.relative-on-unset-attribute]
        // Every key something has set *or* is modulating. A relative
        // value on an attribute nothing set absolutely applies to the
        // attribute's rest value rather than being dropped: a cutout
        // that lifts a layer the section left dark has to be able to.
        let keys: HashSet<&(ChanId, Attribute)> = layers
            .tracked
            .keys()
            .chain(layers.modulated.keys())
            .collect();
        let mut out = HashMap::with_capacity(keys.len());
        for key in keys {
            let value = match layers.tracked.get(key) {
                Some(Source::Direct(v)) => *v,
                // A recipe whose selection has since stopped covering
                // this channel simply contributes nothing, rather than
                // holding a stale value — the tolerance the rest of
                // recipe resolution already has.
                Some(Source::Recipe(id)) => match resolved[id].values.get(key) {
                    Some(v) => *v,
                    None => continue,
                },
                None => 0.0,
            };
            // Modulation is applied *after* the cascade has picked a
            // winner, not as another competitor for the slot. That is
            // what "-40% dimmer, and the colour is not my business"
            // means mechanically.
            //
            // r[impl playback.transient-over-sustained]
            // r[impl playback.relative-last-wins-within-class]
            // A transient (one-shot) outranks every sustained (looping)
            // modulator for as long as it contributes, whichever was
            // taken later; within a class the last one taken wins —
            // MA3's rule, not a sum. A bump does replace the chase
            // under it for as long as it runs, which is what a stab
            // should do; the chase comes back when the bump withdraws.
            let ringing = transient.contains(key);
            let contributing: Vec<usize> = layers
                .modulated
                .get(key)
                .into_iter()
                .flatten()
                .copied()
                .filter(|id| self.contributing(*id, show))
                // A key a trigger is ringing on: sustained modulators
                // step aside for the duration.
                .filter(|id| !ringing || self.active[*id].recipe.timing.once)
                .collect();
            let value_of = |id: &usize| resolved[id].values.get(key).copied().unwrap_or(0.0);
            let modulation = modulation_of(&contributing, value_of, |id| self.kind_of(*id));
            if !layers.tracked.contains_key(key) && modulation == 0.0 {
                // Nothing set it and nothing is moving it: it is at rest,
                // which is the same as absent.
                continue;
            }
            // r[impl recipes.relative-is-a-separate-layer] - relative added to absolute at output
            // r[impl recipes.cascade] - one absolute plus one relative per attribute
            out.insert(key.clone(), value + modulation);
        }

        // Focus deltas: metres against the aim, not degrees against the
        // angles. A channel something aimed at a point, with a relative
        // recipe offering a delta for it, is re-solved from (point +
        // delta) through its own placement — so the same metre circle
        // is a different set of angles for every head, and the same
        // circle on the floor at every venue. The delta is picked by
        // the rule the values use: a transient wins, else the last
        // sustained one, plus every stacking one.
        // r[impl focus.delta] - added to the aim before the solve, never to the angles
        // r[impl focus.orbit-in-metres] - each fixture solves its own angles per frame
        // r[impl effects.relative-stack] - stacking focus deltas sum in metres
        for (chan, aim) in &layers.aim {
            let Some(point) = resolved.get(aim).and_then(|r| r.points.get(chan).copied()) else {
                continue;
            };
            aims.insert(*chan, point);
            let contributing: Vec<usize> = layers
                .focus_modulated
                .get(chan)
                .into_iter()
                .flatten()
                .copied()
                .filter(|id| self.contributing(*id, show))
                .filter(|id| resolved[id].deltas.contains_key(chan))
                .collect();
            if contributing.is_empty() {
                continue;
            }
            let delta = modulation_of(
                &contributing,
                |id| resolved[id].deltas.get(chan).copied().unwrap_or(Vec3::REST),
                |id| self.kind_of(*id),
            );
            if let Some((pan, tilt)) = resolve_focus_delta(*chan, point, delta, show.rig) {
                out.insert((*chan, Attribute::Pan), pan);
                out.insert((*chan, Attribute::Tilt), tilt);
                aims.insert(*chan, crate::focus::v_add(point, delta));
            }
        }
        // A direct value on any colour channel is the triple winning
        // the fixture's colour outright; no intent describes it.
        for (key, source) in &layers.tracked {
            if AttrClass::of(&key.1) == AttrClass::Colour && matches!(source, Source::Direct(_)) {
                intents.remove(&key.0);
            }
        }
        Resolved {
            values: out,
            aims,
            intents,
        }
    }

    /// The show as it looks right now, as direct values a host can record
    /// into a cue — every running effect sampled at this instant and
    /// frozen. Playback is untouched: the effects keep running; this is
    /// a copy of one moment of them. The deliberate verb the spec asks
    /// for, so a moment out of a generative effect is grabbed on purpose
    /// and never by recalling a value over it.
    // r[impl effects.stomp.freeze-is-explicit] - an explicit verb, not a side effect of a take
    // r[impl effects.stomp] - the running effect is left running; only the copy is static
    /// Cooks the cue's recipes into direct values on the cue itself,
    /// sampled at this instant — `Overwrite` replaces its direct values,
    /// `Merge` keeps the ones the recipes do not cover. The recipes are
    /// removed either way; the cue is static from here.
    // r[impl cues.cook-merge]
    // r[impl effects.stomp.freeze-is-explicit] - an explicit verb
    pub fn freeze_into(&self, cue: &mut Cue, show: &Show<'_>, mode: CookMode) {
        // The keys the cue's own recipes cover, sampled from the live
        // output where the player has them (a modulation on top is part
        // of the moment), else from the recipe itself.
        let mut cooked: Vec<CueValue> = Vec::new();
        let mut seen: HashSet<(ChanId, Attribute)> = HashSet::new();
        let live = self.output(show);
        for recipe in cue.resolved_recipes(show) {
            for emit in expand_recipe(&recipe, show, self.clock) {
                let key = (emit.value.chan, emit.value.attr.clone());
                if !seen.insert(key.clone()) {
                    continue;
                }
                let value = live.get(&key).copied().unwrap_or(emit.value.value);
                cooked.push(CueValue {
                    chan: key.0,
                    attr: key.1,
                    value,
                });
            }
        }
        cooked.sort_by(|a, b| {
            (a.chan, format!("{:?}", a.attr)).cmp(&(b.chan, format!("{:?}", b.attr)))
        });
        match mode {
            CookMode::Overwrite => cue.values = cooked,
            CookMode::Merge => {
                cue.values
                    .retain(|v| !seen.contains(&(v.chan, v.attr.clone())));
                cue.values.extend(cooked);
            }
        }
        cue.recipes.clear();
    }

    pub fn freeze(&self, show: &Show<'_>) -> Vec<CueValue> {
        let mut out: Vec<CueValue> = self
            .output(show)
            .into_iter()
            .map(|((chan, attr), value)| CueValue { chan, attr, value })
            .collect();
        out.sort_by(|a, b| {
            (a.chan, format!("{:?}", a.attr)).cmp(&(b.chan, format!("{:?}", b.attr)))
        });
        out
    }
}

/// One layer stack resolved: the values, the room point each aimed
/// channel looks at, and the colour intent each fixture's winning
/// colour source carries.
struct Resolved {
    values: HashMap<(ChanId, Attribute), f32>,
    aims: HashMap<ChanId, Vec3>,
    intents: HashMap<ChanId, Intent>,
}

/// One recipe's current output, keyed for the cascade: its values, the
/// room point it aimed each channel at, the metre offset it offers
/// each channel, and the colour intent behind its colour values.
#[derive(Debug, Default)]
struct ActiveOut {
    values: HashMap<(ChanId, Attribute), f32>,
    points: HashMap<ChanId, Vec3>,
    deltas: HashMap<ChanId, Vec3>,
    // r[impl color.intent-to-output] - kept beside the triple through the cascade
    intents: HashMap<ChanId, Intent>,
}

impl From<Expansion> for ActiveOut {
    fn from(e: Expansion) -> Self {
        let mut intents = HashMap::new();
        for emit in &e.emits {
            if let Some(intent) = &emit.intent {
                intents.insert(emit.value.chan, intent.clone());
            }
        }
        Self {
            values: e
                .emits
                .into_iter()
                .map(|e| ((e.value.chan, e.value.attr), e.value.value))
                .collect(),
            points: e.focus_points.into_iter().collect(),
            deltas: e
                .focus_deltas
                .into_iter()
                .map(|d| (d.chan, d.delta))
                .collect(),
            intents,
        }
    }
}

impl ActiveOut {
    fn retain(&mut self, mut keep: impl FnMut(ChanId) -> bool) {
        self.values.retain(|key, _| keep(key.0));
        self.points.retain(|chan, _| keep(*chan));
        self.deltas.retain(|chan, _| keep(*chan));
        self.intents.retain(|chan, _| keep(*chan));
    }

    fn extend(&mut self, other: ActiveOut) {
        self.values.extend(other.values);
        self.points.extend(other.points);
        self.deltas.extend(other.deltas);
        self.intents.extend(other.intents);
    }

    fn covers(&self, chan: ChanId) -> bool {
        self.values.keys().any(|key| key.0 == chan)
            || self.points.contains_key(&chan)
            || self.deltas.contains_key(&chan)
    }
}

/// How `freeze_into` writes the cooked values onto the cue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
// r[impl cues.cook-merge]
pub enum CookMode {
    /// The cooked values are the cue's direct values; whatever it had
    /// is gone.
    Overwrite,
    /// The cooked values land over the cue's direct values, keeping
    /// every direct value the recipes do not cover.
    Merge,
}

/// Something with a zero, for summing stacked relative values.
trait Rest: Copy {
    const REST: Self;
    fn plus(self, other: Self) -> Self;
}

impl Rest for f32 {
    const REST: f32 = 0.0;
    fn plus(self, other: f32) -> f32 {
        self + other
    }
}

impl Rest for Vec3 {
    const REST: Vec3 = Vec3 {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };
    fn plus(self, other: Vec3) -> Vec3 {
        crate::focus::v_add(self, other)
    }
}

/// The relative value that wins a slot among the recipes still
/// contributing to it, in the order they were taken.
///
/// A transient (one-shot) outranks every sustained (looping) modulator
/// for as long as it contributes, whichever was taken later; among the
/// sustained, the last non-stacking one wins and every stacking one
/// adds to it — MA3's rule, with the stack as the one opt-in.
// r[impl recipes.relative-last-wins]
// r[impl playback.transient-over-sustained]
// r[impl playback.relative-last-wins-within-class]
// r[impl playback.transient-withdraws] - a withdrawn transient is filtered out by the caller
// r[impl effects.relative-stack] - stacking sustained modulators sum
fn modulation_of<T: Rest>(
    contributing: &[usize],
    value_of: impl Fn(&usize) -> T,
    kind_of: impl Fn(&usize) -> Kind,
) -> T {
    let transient_winner = contributing.iter().rev().find(|id| kind_of(id).once);
    match transient_winner {
        Some(id) => value_of(id),
        None => {
            let last = contributing
                .iter()
                .rev()
                .find(|id| !kind_of(id).stacks)
                .map(&value_of)
                .unwrap_or(T::REST);
            contributing
                .iter()
                .filter(|id| kind_of(id).stacks)
                .map(&value_of)
                .fold(last, T::plus)
        }
    }
}

/// What `modulation_of` needs to know about a recipe.
#[derive(Clone, Copy)]
struct Kind {
    once: bool,
    stacks: bool,
}

impl CuePlayer {
    fn kind_of(&self, id: usize) -> Kind {
        let recipe = &self.active[id].recipe;
        Kind {
            once: recipe.timing.once,
            stacks: stacks(recipe),
        }
    }
}

/// Crossfades two resolved frames.
///
/// A key only `next` has fades in from 0 — the fixture coming up from
/// off, the same as a real desk's first cue on a previously-untouched
/// channel. A key only `prev` has fades *out* to 0, which is how a
/// `block` cue takes back what it does not set instead of snapping it
/// dark.
// r[impl cues.both-sides-resolve-live] - crossfade of two resolved frames
// r[impl cues.fade-is-arrival]
// r[impl cues.block] - what a blocking cue does not set fades out over its own time
// r[impl playback.release-falls-through] - a key only the outgoing side has fades to rest
// r[impl cues.timing.per-attribute] - each key crossfades on its own class timing
// r[impl cues.delay]
// r[impl playback.defaults] - a key rises from, and falls to, its default
fn blend(
    prev: &HashMap<(ChanId, Attribute), f32>,
    next: &HashMap<(ChanId, Attribute), f32>,
    stage: &Stage,
    defaults: &HashMap<(ChanId, Attribute), f32>,
) -> HashMap<(ChanId, Attribute), f32> {
    let mut out = HashMap::with_capacity(next.len().max(prev.len()));
    let rest = |key: &(ChanId, Attribute)| defaults.get(key).copied();
    for (key, target) in next {
        let from = prev.get(key).copied().or_else(|| rest(key)).unwrap_or(0.0);
        let t = stage.progress_for(key, *target < from);
        out.insert(key.clone(), from + (target - from) * t);
    }
    for (key, leaving) in prev {
        if !next.contains_key(key) {
            let floor = rest(key);
            let t = stage.progress_for(key, true);
            if t < 1.0 {
                let to = floor.unwrap_or(0.0);
                out.insert(key.clone(), leaving + (to - leaving) * t);
            } else if let Some(floor) = floor {
                out.insert(key.clone(), floor);
            }
        }
    }
    out
}

/// Crossfades two frames of colour intents. Both sides present and
/// expressible in xyY, the chromaticity and luminance interpolate; at
/// either end the stored intent passes through untouched, so a settled
/// `Cct` is still a `Cct`. A gel with no table entry cannot be blended
/// and switches at half-way.
// r[impl color.intent-to-output] - faded in xyY, never as three floats
fn blend_intents(
    prev: &HashMap<ChanId, Intent>,
    next: &HashMap<ChanId, Intent>,
    stage: &Stage,
) -> HashMap<ChanId, Intent> {
    let colour_key = |chan: ChanId| {
        (
            chan,
            Attribute::ColorAdd {
                channel: ignition_proto::ColorChannel::Red,
            },
        )
    };
    let mut out = HashMap::with_capacity(next.len().max(prev.len()));
    for (chan, to) in next {
        let t = stage.progress_for(&colour_key(*chan), false);
        let blended = match prev.get(chan) {
            Some(from) if t < 1.0 && *from != *to => {
                if t <= 0.0 {
                    from.clone()
                } else {
                    match (from.xyy(), to.xyy()) {
                        (Some(a), Some(b)) => Intent::Xy {
                            x: a.x + (b.x - a.x) * t,
                            y: a.y + (b.y - a.y) * t,
                            luminance: a.luminance + (b.luminance - a.luminance) * t,
                        },
                        _ if t < 0.5 => from.clone(),
                        _ => to.clone(),
                    }
                }
            }
            _ => to.clone(),
        };
        out.insert(*chan, blended);
    }
    for (chan, leaving) in prev {
        if !next.contains_key(chan) && stage.progress_for(&colour_key(*chan), true) < 1.0 {
            out.insert(*chan, leaving.clone());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cue(name: &str, fade_secs: f32, values: Vec<(ChanId, Attribute, f32)>) -> Cue {
        Cue {
            name: name.to_string(),
            fade_secs,
            values: values
                .into_iter()
                .map(|(chan, attr, value)| CueValue { chan, attr, value })
                .collect(),
            ..Default::default()
        }
    }

    /// These tests exercise tracking and fades, none of which need a
    /// venue — the layer-2 path has its own tests below.
    fn bare() -> Show<'static> {
        Show::new(&[], &crate::selection::EMPTY_RIG)
    }

    #[test]
    fn before_the_first_go_output_is_empty() {
        let player = CuePlayer::new(vec![cue("Cue 1", 1.0, vec![(1, Attribute::Dimmer, 1.0)])]);
        assert!(player.output(&bare()).is_empty());
        assert_eq!(player.current_index(), None);
    }

    #[test]
    fn a_zero_fade_cue_snaps_immediately() {
        let mut player = CuePlayer::new(vec![cue("Cue 1", 0.0, vec![(1, Attribute::Dimmer, 1.0)])]);
        player.go(&bare());
        assert_eq!(
            player.output(&bare()).get(&(1, Attribute::Dimmer)),
            Some(&1.0)
        );
    }

    // r[verify cues.fade-is-arrival]

    // r[verify cues.fade-is-wall-time]

    #[test]
    fn a_fade_interpolates_over_elapsed_time() {
        let mut player = CuePlayer::new(vec![cue("Cue 1", 2.0, vec![(1, Attribute::Dimmer, 1.0)])]);
        player.go(&bare());
        player.tick(1.0); // halfway through a 2s fade
        let v = *player.output(&bare()).get(&(1, Attribute::Dimmer)).unwrap();
        assert!(
            (v - 0.5).abs() < 0.001,
            "expected ~0.5 halfway through the fade, got {v}"
        );
        player.tick(1.0); // now fully elapsed
        assert!((player.output(&bare()).get(&(1, Attribute::Dimmer)).unwrap() - 1.0).abs() < 0.001);
    }

    /// A store swaps the list under the player: the place, the armed
    /// cue and the clocks stay; the stage shows the edited cue without
    /// a GO, and tracking is rebuilt from the new list.
    /// r[verify studio.program.cue-editing]
    /// r[verify cues.seek-keeps-the-clock]
    #[test]
    fn replacing_the_list_keeps_the_place_and_shows_the_edit() {
        let mut list = CueList {
            cues: vec![
                cue(
                    "Cue 1",
                    0.0,
                    vec![(1, Attribute::Dimmer, 1.0), (2, Attribute::Dimmer, 0.5)],
                ),
                cue("Cue 2", 0.0, vec![(1, Attribute::Dimmer, 0.2)]),
                cue("Cue 3", 0.0, vec![(1, Attribute::Dimmer, 0.9)]),
            ],
            ..Default::default()
        };
        let mut player = CuePlayer::from_list(&list);
        player.go(&bare());
        player.go(&bare());
        player.tick(3.5);
        player.load(2);
        player.pause();
        assert_eq!(player.current_index(), Some(1));

        // Edit the current cue and the one before it.
        list.cues[1].values = vec![CueValue {
            chan: 1,
            attr: Attribute::Dimmer,
            value: 0.7,
        }];
        list.cues[0].values[1].value = 0.3;
        player.replace_list(&list, &bare());

        assert_eq!(player.current_index(), Some(1), "the place is kept");
        assert_eq!(player.loaded(), Some(2), "the armed cue is kept");
        assert!(player.is_paused(), "pause is kept");
        assert!((player.clock() - 3.5).abs() < 1e-6, "the clock is kept");
        let out = player.output(&bare());
        assert!(
            (out[&(1, Attribute::Dimmer)] - 0.7).abs() < 1e-6,
            "the edited cue shows"
        );
        assert!(
            (out[&(2, Attribute::Dimmer)] - 0.3).abs() < 1e-6,
            "tracking is rebuilt from the edited list"
        );
        assert_eq!(player.current_name(), Some("Cue 2"));

        // A new cue appended past the end plays from the same place.
        player.resume();
        player.reload_cue(
            3,
            cue("Cue 4", 0.0, vec![(1, Attribute::Dimmer, 0.1)]),
            &bare(),
        );
        assert_eq!(player.cues().len(), 4);
        assert_eq!(player.current_index(), Some(1));
        player.go(&bare()); // the armed cue 2
        player.go(&bare());
        assert_eq!(player.current_name(), Some("Cue 4"));

        // A list shorter than the place clamps; an empty one unwinds.
        list.cues.truncate(1);
        player.replace_list(&list, &bare());
        assert_eq!(player.current_index(), Some(0));
        assert_eq!(player.loaded(), None);
        list.cues.clear();
        player.replace_list(&list, &bare());
        assert_eq!(player.current_index(), None);
        assert!(player.output(&bare()).is_empty());
    }

    /// Replacing the list replays, never performs: a one-shot in the
    /// current cue does not fire again because the cue was edited.
    /// r[verify cues.replay-does-not-perform]
    #[test]
    fn replacing_the_list_does_not_refire_a_one_shot() {
        let mut once = Recipe::new(Selection::Chans(vec![1]), RecipeApply::Dimmer(1.0));
        once.timing.once = true;
        let list = CueList {
            cues: vec![Cue {
                name: "Hit".into(),
                recipes: vec![once.into()],
                ..Default::default()
            }],
            ..Default::default()
        };
        let mut player = CuePlayer::from_list(&list);
        player.go(&bare());
        player.tick(30.0);
        let before = player.output(&bare());
        player.replace_list(&list, &bare());
        assert_eq!(player.output(&bare()), before);
    }

    // r[verify cues.tracking]

    #[test]
    fn a_channel_not_mentioned_in_the_next_cue_holds_its_value_tracking() {
        let mut player = CuePlayer::new(vec![
            cue(
                "Cue 1",
                0.0,
                vec![(1, Attribute::Dimmer, 1.0), (2, Attribute::Dimmer, 0.5)],
            ),
            // Cue 2 only touches channel 1 — channel 2 should still read 0.5.
            cue("Cue 2", 0.0, vec![(1, Attribute::Dimmer, 0.2)]),
        ]);
        player.go(&bare());
        player.go(&bare());
        let out = player.output(&bare());
        assert!((out.get(&(1, Attribute::Dimmer)).unwrap() - 0.2).abs() < 0.001);
        assert!(
            (out.get(&(2, Attribute::Dimmer)).unwrap() - 0.5).abs() < 0.001,
            "untouched channel should track forward"
        );
    }

    #[test]
    fn go_on_the_last_cue_is_a_no_op() {
        let mut player = CuePlayer::new(vec![cue(
            "Only Cue",
            0.0,
            vec![(1, Attribute::Dimmer, 1.0)],
        )]);
        player.go(&bare());
        assert_eq!(player.current_index(), Some(0));
        player.go(&bare()); // no cue 1 to advance into
        assert_eq!(player.current_index(), Some(0));
        assert_eq!(
            player.output(&bare()).get(&(1, Attribute::Dimmer)),
            Some(&1.0)
        );
    }

    // r[verify cues.fades-stack]

    #[test]
    fn refiring_go_mid_fade_chains_from_the_actual_current_position_not_the_prior_target() {
        let mut player = CuePlayer::new(vec![
            cue("Cue 1", 4.0, vec![(1, Attribute::Dimmer, 1.0)]),
            cue("Cue 2", 4.0, vec![(1, Attribute::Dimmer, 0.0)]),
        ]);
        player.go(&bare());
        player.tick(2.0); // halfway into Cue 1's fade -> dimmer at ~0.5
        player.go(&bare()); // fire Cue 2 before Cue 1 finished
        let v = *player.output(&bare()).get(&(1, Attribute::Dimmer)).unwrap();
        assert!(
            (v - 0.5).abs() < 0.01,
            "should start Cue 2's fade from the actual mid-fade value, got {v}"
        );
    }

    // r[verify cues.seek]

    // r[verify cues.tracking]

    #[test]
    fn jump_to_end_of_resolves_every_cue_up_to_and_including_the_target() {
        let mut player = CuePlayer::new(vec![
            cue("Cue 1", 1.0, vec![(1, Attribute::Dimmer, 1.0)]),
            cue("Cue 2", 1.0, vec![(2, Attribute::Dimmer, 1.0)]),
            cue("Cue 3", 1.0, vec![(1, Attribute::Dimmer, 0.0)]),
        ]);
        player.jump_to_end_of(1, &bare());
        assert_eq!(player.current_index(), Some(1));
        let out = player.output(&bare());
        assert_eq!(
            out.get(&(1, Attribute::Dimmer)),
            Some(&1.0),
            "Cue 1's value should have fully resolved"
        );
        assert_eq!(
            out.get(&(2, Attribute::Dimmer)),
            Some(&1.0),
            "Cue 2's own value should have fully resolved"
        );
    }

    // -----------------------------------------------------------------
    // The cascade and recipe tracking
    // -----------------------------------------------------------------

    use crate::group::Group;
    use crate::recipe::RecipeApply;
    use crate::selection::Selection;

    fn pars() -> Vec<Group> {
        vec![Group {
            name: "Pars".to_string(),
            chans: vec![1, 2, 3],
        }]
    }

    fn recipe_cue(name: &str, level: f32) -> Cue {
        Cue {
            name: name.to_string(),
            recipes: vec![
                Recipe::new(
                    Selection::Group("Pars".to_string()),
                    RecipeApply::Dimmer(level),
                )
                .into(),
            ],
            ..Default::default()
        }
    }

    /// Relative values do not sum — the last one still contributing
    /// wins, which is grandMA3's rule for both absolute and relative
    /// data in multiple parts.
    // r[verify recipes.relative-last-wins]
    // r[verify playback.relative-last-wins-within-class]
    // r[verify recipes.relative-is-a-separate-layer]
    #[test]
    fn the_last_relative_value_wins_rather_than_summing() {
        let groups = pars();
        let show = Show::new(&groups, &crate::selection::EMPTY_RIG);
        let mut player = CuePlayer::new(vec![look_with_modulation(0.3, 0.2), accent(0.5, false)]);
        player.go(&show);
        let base = player.output(&show)[&(1, Attribute::Dimmer)];
        assert!(
            (base - 0.5).abs() < 1e-4,
            "look alone should be 0.3+0.2: {base}"
        );

        player.go(&show);
        let both = player.output(&show)[&(1, Attribute::Dimmer)];
        // 0.3 absolute + 0.5 relative. The look's own 0.2 is replaced,
        // not added to: 0.8, not 1.0.
        assert!((both - 0.8).abs() < 1e-4, "expected last-wins 0.8: {both}");
    }

    /// ...and a finished one-shot **withdraws**, so what was underneath
    /// comes back.
    ///
    /// This is the "fig 0 did nothing" report, pinned. Under last-wins a
    /// finished bump that stayed in the layer would go on winning at its
    /// final value forever — which for an envelope ending at zero meant
    /// the accent silently killed the chase it landed on for the rest of
    /// the section, and looked from the stage like the hit doing nothing
    /// at all.
    // r[verify recipes.finished-one-shot-withdraws]
    // r[verify playback.transient-withdraws]
    // r[verify playback.release-falls-through]
    #[test]
    fn a_finished_one_shot_withdraws_and_the_chase_returns() {
        let groups = pars();
        let show = Show::new(&groups, &crate::selection::EMPTY_RIG);
        let mut player = CuePlayer::new(vec![look_with_modulation(0.3, 0.2), accent(0.5, true)]);
        player.go(&show);
        player.go(&show);

        let during = player.output(&show)[&(1, Attribute::Dimmer)];
        assert!(during > 0.7, "the bump never landed: {during}");

        // Long past the one-shot's single cycle.
        player.tick(10.0);
        let after = player.output(&show)[&(1, Attribute::Dimmer)];
        assert!(
            (after - 0.5).abs() < 1e-4,
            "the chase did not come back after the bump finished: {after}"
        );
    }

    /// A transient outranks a sustained modulator whichever was taken
    /// later. A section that re-states its chase after a figure was
    /// fired must not outrank every hit for the rest of the section.
    ///
    /// r[verify playback.transient-over-sustained]
    // r[verify playback.relative-classes]
    // r[verify playback.transient-withdraws]
    #[test]
    fn a_hit_outranks_a_chase_restated_after_it() {
        let groups = pars();
        let show = Show::new(&groups, &crate::selection::EMPTY_RIG);
        let mut player = CuePlayer::new(vec![
            recipe_cue("Look", 0.3),
            accent(0.5, true),
            // A later, non-blocking cue re-stating a steady modulation.
            Cue {
                name: "· wider".to_string(),
                recipes: vec![flat_delta(0.1, false).into()],
                ..Default::default()
            },
        ]);
        player.go(&show);
        player.go(&show);
        player.go(&show);
        let during = player.output(&show)[&(1, Attribute::Dimmer)];
        assert!(
            (during - 0.8).abs() < 1e-4,
            "the chase outranked the hit: {during}"
        );
        player.tick(10.0);
        let after = player.output(&show)[&(1, Attribute::Dimmer)];
        assert!(
            (after - 0.4).abs() < 1e-4,
            "the chase did not resume: {after}"
        );
    }

    /// A relative value on an attribute nothing set applies to the rest
    /// value rather than vanishing, so a cutout can lift a layer the
    /// section left dark.
    ///
    /// r[verify playback.relative-on-unset-attribute]
    #[test]
    fn a_relative_value_on_an_unset_attribute_lifts_from_rest() {
        let groups = pars();
        let show = Show::new(&groups, &crate::selection::EMPTY_RIG);
        let mut player = CuePlayer::new(vec![accent(0.5, true)]);
        player.go(&show);
        let during = player.output(&show)[&(1, Attribute::Dimmer)];
        assert!((during - 0.5).abs() < 1e-4, "{during}");
        player.tick(10.0);
        assert!(
            !player.output(&show).contains_key(&(1, Attribute::Dimmer)),
            "withdrawn from rest is absent"
        );
    }

    /// A look at `level`, with a steady relative `lift` on top of it.
    fn look_with_modulation(level: f32, lift: f32) -> Cue {
        Cue {
            name: "Look".to_string(),
            recipes: vec![
                Recipe::new(
                    Selection::Group("Pars".to_string()),
                    RecipeApply::Dimmer(level),
                )
                .into(),
                flat_delta(lift, false).into(),
            ],
            block: true,
            ..Default::default()
        }
    }

    fn accent(lift: f32, once: bool) -> Cue {
        Cue {
            name: "· hit".to_string(),
            recipes: vec![flat_delta(lift, once).into()],
            block: false,
            ..Default::default()
        }
    }

    fn flat_delta(v: f32, once: bool) -> Recipe {
        use crate::step::{Speed, Step, Timing};
        Recipe {
            target: Selection::Group("Pars".to_string()),
            steps: vec![Step::new(vec![RecipeApply::Delta(vec![(
                Attribute::Dimmer,
                v,
            )])])],
            timing: Timing {
                speed: Speed::Bpm(60.0),
                once,
                ..Default::default()
            },
            tricks: Vec::new(),
            stack: false,
            ..Default::default()
        }
    }

    /// A bump releases itself, which is the whole reason `once` exists.
    /// Before it, a flash needed two cues — one to lift and one to put
    /// out a beat later — so half the cue names in a show were "… out"
    /// and the list said nothing a reader wanted to know.
    // r[verify recipes.one-shot]
    // r[verify effects.once]
    // r[verify recipes.one-shot-clock]
    #[test]
    fn a_one_shot_bump_releases_itself() {
        use crate::step::{Speed, Step, Timing};
        let groups = pars();
        let show = Show::new(&groups, &crate::selection::EMPTY_RIG);

        let up = Step::new(vec![RecipeApply::Delta(vec![(Attribute::Dimmer, 0.6)])]);
        let down = Step::new(vec![RecipeApply::Delta(vec![(Attribute::Dimmer, 0.0)])]);
        let bump = Cue {
            name: "· hit".to_string(),
            recipes: vec![
                Recipe {
                    target: Selection::Group("Pars".to_string()),
                    steps: vec![up, down],
                    timing: Timing {
                        speed: Speed::Bpm(60.0),
                        measure: 1.0,
                        once: true,
                        ..Default::default()
                    },
                    tricks: Vec::new(),
                    stack: false,
                    ..Default::default()
                }
                .into(),
            ],
            ..Default::default()
        };

        // A Delta modulates a tracked value, so there has to be a look
        // for it to sit on — which is exactly how the show runs it: a
        // section sets the level, the accent lifts it.
        let mut player = CuePlayer::new(vec![recipe_cue("Look", 0.3), bump]);
        player.go(&show);
        player.go(&show);
        let peak = player
            .output(&show)
            .get(&(1, Attribute::Dimmer))
            .copied()
            .unwrap_or_default();
        assert!(peak > 0.5, "the bump never lifted: {peak}");

        // A second later the envelope has run out and holds at zero.
        player.tick(1.0);
        let after = player
            .output(&show)
            .get(&(1, Attribute::Dimmer))
            .copied()
            .unwrap_or_default();
        assert!(
            (after - 0.3).abs() < 0.05,
            "the bump did not release to the look: {after}"
        );

        // And stays there rather than looping round for another flash.
        player.tick(5.0);
        let later = player
            .output(&show)
            .get(&(1, Attribute::Dimmer))
            .copied()
            .unwrap_or_default();
        assert!((later - 0.3).abs() < 0.05, "the bump looped: {later}");
    }

    /// A looping chase carried by two cues stays in phase across the
    /// cut between them.
    ///
    /// This is MA3's Sync. A loop reads the shared show clock, so
    /// taking the next cue does not restart the chase — an operator
    /// running sections during a chase should see the sections change
    /// underneath it, not the chase stutter back to its first step.
    /// `r[recipes.one-shot-clock]` is the same seam from the other
    /// side: a one-shot, and only a one-shot, gets its own start.
    ///
    /// r[verify effects.sync.shared-clock]
    #[test]
    fn a_chase_carried_by_two_cues_does_not_restart_on_the_cut() {
        use crate::step::{Speed, Step, Timing};
        let groups = pars();
        let show = Show::new(&groups, &crate::selection::EMPTY_RIG);

        // A slow two-step loop, so a restart would be unmistakable.
        let chase = || Recipe {
            target: Selection::Group("Pars".to_string()),
            steps: vec![
                Step::new(vec![RecipeApply::Dimmer(1.0)]),
                Step::new(vec![RecipeApply::Dimmer(0.0)]),
            ],
            timing: Timing {
                speed: Speed::Bpm(60.0),
                measure: 4.0,
                once: false,
                ..Default::default()
            },
            tricks: Vec::new(),
            stack: false,
            ..Default::default()
        };
        let cue = |name: &str| Cue {
            name: name.to_string(),
            recipes: vec![chase().into()],
            ..Default::default()
        };

        let mut player = CuePlayer::new(vec![cue("A"), cue("B")]);
        player.go(&show);
        player.tick(1.5);
        let before = player
            .output(&show)
            .get(&(1, Attribute::Dimmer))
            .copied()
            .unwrap_or_default();

        // The cut. The same chase is now carried by the next cue.
        player.go(&show);
        let after = player
            .output(&show)
            .get(&(1, Attribute::Dimmer))
            .copied()
            .unwrap_or_default();

        assert!(
            (before - after).abs() < 0.05,
            "the chase jumped across the cut: {before} then {after} — a restart, not a sync"
        );
    }

    // r[verify groups.resolution-is-live]

    // r[verify cues.recipes-not-values]

    #[test]
    fn a_recipe_on_a_cue_resolves_at_output_time() {
        let groups = pars();
        let show = Show::new(&groups, &crate::selection::EMPTY_RIG);
        let mut player = CuePlayer::new(vec![recipe_cue("Wash", 0.8)]);
        player.go(&show);
        let out = player.output(&show);
        assert_eq!(out.len(), 3);
        assert_eq!(out.get(&(2, Attribute::Dimmer)), Some(&0.8));
    }

    /// The point of storing the template: the same cue covers a fixture
    /// the group did not have when the show loaded.
    // r[verify recipes.template]
    // r[verify recipes.cook-fixes-coverage]
    #[test]
    fn a_recipe_covers_a_fixture_added_to_its_group_after_loading() {
        let mut player = CuePlayer::new(vec![recipe_cue("Wash", 1.0)]);
        let before = pars();
        player.go(&Show::new(&before, &crate::selection::EMPTY_RIG));

        let after = vec![Group {
            name: "Pars".to_string(),
            chans: vec![1, 2, 3, 4],
        }];
        let grown = Show::new(&after, &crate::selection::EMPTY_RIG);
        // Re-firing is the cook that picks up the new fixture.
        let mut player2 = CuePlayer::new(vec![recipe_cue("Wash", 1.0)]);
        player2.go(&grown);
        assert_eq!(player2.output(&grown).len(), 4);
        // ...and the original player is unchanged, because coverage is
        // fixed at cook time rather than drifting under a running cue.
        assert_eq!(player.output(&grown).len(), 3);
    }

    /// Layer 1 beats layer 2 *within one cue* — the ordering is what
    /// makes "override one fixture out of the recipe" defined.
    // r[verify recipes.cascade]
    // r[verify playback.absolute-layer]
    #[test]
    fn a_direct_value_beats_a_recipe_on_the_same_cue() {
        let groups = pars();
        let show = Show::new(&groups, &crate::selection::EMPTY_RIG);
        let mut cue = recipe_cue("Wash", 0.8);
        cue.values = vec![CueValue {
            chan: 2,
            attr: Attribute::Dimmer,
            value: 0.1,
        }];
        let mut player = CuePlayer::new(vec![cue]);
        player.go(&show);
        let out = player.output(&show);
        assert_eq!(out.get(&(1, Attribute::Dimmer)), Some(&0.8));
        assert_eq!(
            out.get(&(2, Attribute::Dimmer)),
            Some(&0.1),
            "the tweak wins"
        );
        assert_eq!(out.get(&(3, Attribute::Dimmer)), Some(&0.8));
    }

    // r[verify recipes.absolute-last-wins]

    // r[verify playback.absolute-layer]

    #[test]
    fn a_later_cues_recipe_supersedes_an_earlier_ones() {
        let groups = pars();
        let show = Show::new(&groups, &crate::selection::EMPTY_RIG);
        let mut player = CuePlayer::new(vec![recipe_cue("A", 0.2), recipe_cue("B", 0.9)]);
        player.go(&show);
        player.go(&show);
        assert_eq!(
            player.output(&show).get(&(1, Attribute::Dimmer)),
            Some(&0.9)
        );
    }

    // r[verify cues.tracking]

    // r[verify cues.tracking-carries-sources]

    #[test]
    fn a_recipe_tracks_forward_through_a_cue_that_does_not_mention_it() {
        let groups = pars();
        let show = Show::new(&groups, &crate::selection::EMPTY_RIG);
        let mut player = CuePlayer::new(vec![
            recipe_cue("Wash", 0.7),
            cue("Something Else", 0.0, vec![(9, Attribute::Dimmer, 1.0)]),
        ]);
        player.go(&show);
        player.go(&show);
        assert_eq!(
            player.output(&show).get(&(1, Attribute::Dimmer)),
            Some(&0.7)
        );
    }

    // r[verify cues.block]

    // r[verify recipes.blocking-resets]

    #[test]
    fn a_block_cue_drops_everything_it_does_not_set() {
        let groups = pars();
        let show = Show::new(&groups, &crate::selection::EMPTY_RIG);
        let mut blocked = cue("Fresh", 0.0, vec![(9, Attribute::Dimmer, 1.0)]);
        blocked.block = true;
        let mut player = CuePlayer::new(vec![recipe_cue("Wash", 0.7), blocked]);
        player.go(&show);
        player.go(&show);
        let out = player.output(&show);
        assert!(!out.contains_key(&(1, Attribute::Dimmer)), "{out:?}");
        assert_eq!(out.get(&(9, Attribute::Dimmer)), Some(&1.0));
    }

    /// Without compaction `active` gains an entry per recipe per `go()`
    /// and never gives one back.
    #[test]
    fn superseded_recipes_are_dropped_rather_than_accumulating() {
        let groups = pars();
        let show = Show::new(&groups, &crate::selection::EMPTY_RIG);
        let cues: Vec<Cue> = (0..20)
            .map(|i| recipe_cue("Wash", i as f32 / 20.0))
            .collect();
        let mut player = CuePlayer::new(cues);
        for _ in 0..20 {
            player.go(&show);
        }
        assert_eq!(
            player.active.len(),
            1,
            "only the newest recipe is still tracked"
        );
    }

    // -----------------------------------------------------------------
    // Both sides of a fade resolve live
    // -----------------------------------------------------------------

    use crate::step::{Speed, Step, Timing};

    /// A two-step phaser on channel 1 swinging between `lo` and `hi`
    /// once per second.
    fn chase(lo: f32, hi: f32) -> Recipe {
        let at = |v: f32| Step::new(vec![RecipeApply::Raw(vec![(Attribute::Dimmer, v)])]);
        Recipe {
            target: Selection::Chans(vec![1]),
            steps: vec![at(lo), at(hi)],
            timing: Timing {
                speed: Speed::Hz(1.0),
                ..Default::default()
            },
            tricks: Vec::new(),
            stack: false,
            ..Default::default()
        }
    }

    /// The bug a snapshot `from` has: crossing from one phaser into
    /// another, the outgoing one must keep moving while it goes. Held
    /// halfway through the fade, the output has to change as the clock
    /// advances even though the fade position does not.
    // r[verify cues.both-sides-resolve-live]
    #[test]
    fn a_phaser_being_faded_out_of_keeps_moving() {
        let show = bare();
        let mut player = CuePlayer::new(vec![
            Cue {
                name: "A".into(),
                recipes: vec![chase(0.0, 1.0).into()],
                ..Default::default()
            },
            Cue {
                name: "B".into(),
                fade_secs: 10.0,
                // Static, so anything that moves in the output can only
                // have come from the outgoing phaser.
                values: vec![CueValue {
                    chan: 1,
                    attr: Attribute::Dimmer,
                    value: 0.5,
                }],
                ..Default::default()
            },
        ]);
        player.go(&show);
        player.go(&show);

        // Advance the clock a quarter-cycle at a time, but read the
        // output at the same fade position each time by rewinding it.
        let sample = |p: &mut CuePlayer, dt: f32| {
            p.tick(dt);
            for stage in &mut p.stack {
                stage.elapsed = 1.0; // a tenth of the way into the fade
            }
            *p.output(&show).get(&(1, Attribute::Dimmer)).unwrap()
        };
        let first = sample(&mut player, 0.0);
        let later = sample(&mut player, 0.5);
        assert!(
            (first - later).abs() > 0.05,
            "the outgoing phaser froze: {first} then {later}"
        );
    }

    /// The incoming side moves too, which the snapshot model already
    /// got right — pinned so a future change cannot lose it.
    // r[verify cues.both-sides-resolve-live]
    #[test]
    fn a_phaser_being_faded_into_moves_while_it_arrives() {
        let show = bare();
        let mut player = CuePlayer::new(vec![
            cue("Static", 0.0, vec![(1, Attribute::Dimmer, 0.0)]),
            Cue {
                name: "Chase".into(),
                fade_secs: 10.0,
                recipes: vec![chase(0.0, 1.0).into()],
                ..Default::default()
            },
        ]);
        player.go(&show);
        player.go(&show);
        let sample = |p: &mut CuePlayer, dt: f32| {
            p.tick(dt);
            for stage in &mut p.stack {
                stage.elapsed = 1.0;
            }
            *p.output(&show).get(&(1, Attribute::Dimmer)).unwrap()
        };
        let first = sample(&mut player, 0.0);
        let later = sample(&mut player, 0.5);
        assert!((first - later).abs() > 0.01, "{first} then {later}");
    }

    /// Overlapping fades compose rather than the newest simply
    /// discarding the one in flight.
    // r[verify cues.fades-stack]
    // r[verify cues.arrived-collapses]
    #[test]
    fn firing_go_mid_fade_stacks_the_fades() {
        let show = bare();
        let mut player = CuePlayer::new(vec![
            cue("A", 0.0, vec![(1, Attribute::Dimmer, 1.0)]),
            cue("B", 4.0, vec![(1, Attribute::Dimmer, 0.0)]),
            cue("C", 4.0, vec![(1, Attribute::Dimmer, 1.0)]),
        ]);
        player.go(&show);
        player.go(&show);
        player.tick(2.0); // halfway to 0, so ~0.5
        let mid = *player.output(&show).get(&(1, Attribute::Dimmer)).unwrap();
        assert!((mid - 0.5).abs() < 0.01, "{mid}");

        player.go(&show); // back up to 1.0, from ~0.5, while B still runs
        assert_eq!(player.stack.len(), 3, "B's fade is still in flight");
        player.tick(4.0);
        let done = *player.output(&show).get(&(1, Attribute::Dimmer)).unwrap();
        assert!((done - 1.0).abs() < 0.01, "{done}");
        assert_eq!(player.stack.len(), 1, "finished fades collapse");
    }

    // r[verify cues.fades-stack]

    #[test]
    fn spamming_go_does_not_grow_the_stack_without_bound() {
        let show = bare();
        let cues: Vec<Cue> = (0..40)
            .map(|i| cue(&format!("C{i}"), 100.0, vec![(1, Attribute::Dimmer, 1.0)]))
            .collect();
        let mut player = CuePlayer::new(cues);
        for _ in 0..40 {
            player.go(&show);
        }
        assert!(player.stack.len() <= MAX_FADES, "{}", player.stack.len());
    }

    // -----------------------------------------------------------------
    // Musical position
    // -----------------------------------------------------------------

    /// A cue at a bar, setting one channel to a recognisable level.
    fn at_bar(bar: u32, level: f32) -> Cue {
        Cue {
            name: format!("bar {bar}"),
            values: vec![CueValue {
                chan: 1,
                attr: Attribute::Dimmer,
                value: level,
            }],
            at: Some(Bars::bar(bar).into()),
            ..Default::default()
        }
    }

    fn song() -> Vec<Cue> {
        vec![
            at_bar(1, 0.1),
            at_bar(11, 0.2),
            at_bar(23, 0.3),
            at_bar(31, 0.4),
        ]
    }

    fn level(player: &CuePlayer, show: &Show<'_>) -> Option<f32> {
        player.output(show).get(&(1, Attribute::Dimmer)).copied()
    }

    // r[verify cues.seek]

    // r[verify cues.position]

    #[test]
    fn seeking_lands_on_the_last_cue_at_or_before_the_position() {
        let show = bare();
        let mut player = CuePlayer::new(song());
        player.seek(Bars::bar(23), &show);
        assert_eq!(level(&player, &show), Some(0.3));
        // Mid-section stays on the section's cue.
        player.seek(Bars::new(27, 2.5), &show);
        assert_eq!(level(&player, &show), Some(0.3));
    }

    /// The point of position-addressing: a jump backwards has to land in
    /// the same state as arriving there forwards. An event-driven player
    /// cannot do this without replaying its own history.
    // r[verify cues.seek]
    /// r[verify song.two-ways]
    #[test]
    fn seeking_backwards_lands_where_playing_forwards_would() {
        let show = bare();
        let mut forwards = CuePlayer::new(song());
        forwards.seek(Bars::bar(11), &show);
        let expected = level(&forwards, &show);

        let mut jumped = CuePlayer::new(song());
        jumped.seek(Bars::bar(31), &show);
        jumped.seek(Bars::bar(11), &show);
        assert_eq!(level(&jumped, &show), expected);
    }

    /// Looping a section is the same motion, repeated — the case a live
    /// rehearsal spends all afternoon in.
    // r[verify cues.seek]
    #[test]
    fn looping_a_section_is_stable() {
        let show = bare();
        let mut player = CuePlayer::new(song());
        for _ in 0..3 {
            for bar in [23u32, 26, 29, 23] {
                player.seek(Bars::bar(bar), &show);
                assert_eq!(level(&player, &show), Some(0.3), "bar {bar}");
            }
        }
    }

    // r[verify cues.seek]

    #[test]
    fn before_the_first_positioned_cue_nothing_is_lit() {
        let show = bare();
        let mut player = CuePlayer::new(vec![at_bar(11, 0.2), at_bar(23, 0.3)]);
        player.seek(Bars::bar(23), &show);
        assert!(level(&player, &show).is_some());
        player.seek(Bars::bar(1), &show);
        assert!(
            player.output(&show).is_empty(),
            "rewinding past the show clears it"
        );
    }

    /// A list can mix positioned cues with hand-only ones; the clock
    /// simply never picks the unpositioned ones up.
    // r[verify cues.unpositioned-are-invisible-to-seek]
    #[test]
    fn cues_without_a_position_are_invisible_to_seeking() {
        let show = bare();
        let mut cues = song();
        cues.insert(2, cue("Hand only", 0.0, vec![(1, Attribute::Dimmer, 0.99)]));
        let mut player = CuePlayer::new(cues);
        player.seek(Bars::bar(23), &show);
        // Bar 23's cue is now at index 3, and the hand-only one at 2 was
        // passed through on the way — tracking still applied it, which
        // is correct: seeking replays the list, it does not skip it.
        assert_eq!(level(&player, &show), Some(0.3));
        assert_eq!(player.index_at(Bars::bar(23)), Some(3));
    }

    /// Seeking every frame is the normal case, so it has to be free when
    /// nothing changed.
    // r[verify cues.seek-is-cheap-when-still]
    #[test]
    fn seeking_within_the_same_cue_does_not_refire_it() {
        let show = bare();
        let mut player = CuePlayer::new(song());
        player.seek(Bars::bar(23), &show);
        let stack = player.stack.len();
        for beat in 1..16 {
            player.seek(Bars::new(23 + beat / 4, (beat % 4) as f64 + 1.0), &show);
        }
        assert_eq!(
            player.stack.len(),
            stack,
            "a re-seek inside a cue restacked it"
        );
    }

    /// The show clock is not the transport: a phaser mid-cycle should
    /// not restart because somebody moved the playhead.
    // r[verify cues.seek-keeps-the-clock]
    #[test]
    fn seeking_does_not_disturb_the_show_clock() {
        let show = bare();
        let mut player = CuePlayer::new(song());
        player.tick(4.0);
        player.seek(Bars::bar(31), &show);
        player.seek(Bars::bar(1), &show);
        assert!((player.clock() - 4.0).abs() < 1e-6);
    }

    // -----------------------------------------------------------------
    // Wave 1: per-attribute timing, fans, MIB, assert/cue-only/release,
    // morph, trig, commands, relative stacking.
    // -----------------------------------------------------------------

    use crate::step::SpeedMasters;

    fn red() -> Attribute {
        Attribute::ColorAdd {
            channel: ignition_proto::ColorChannel::Red,
        }
    }

    /// A show whose Song master runs at 120 BPM — one beat is half a
    /// second, so beat arithmetic in these tests is legible.
    fn show_120() -> (Vec<Group>, SpeedMasters) {
        (pars(), SpeedMasters::from([("Song".to_string(), 120.0)]))
    }

    fn with_speeds<'a>(groups: &'a [Group], speeds: &'a SpeedMasters) -> Show<'a> {
        let mut show = Show::new(groups, &crate::selection::EMPTY_RIG);
        show.speeds = speeds;
        show
    }

    fn timed(name: &str, values: Vec<(ChanId, Attribute, f32)>, timing: CueTiming) -> Cue {
        Cue {
            timing,
            ..cue(name, 4.0, values)
        }
    }

    fn get(player: &CuePlayer, show: &Show<'_>, chan: ChanId, attr: Attribute) -> f32 {
        player
            .output(show)
            .get(&(chan, attr))
            .copied()
            .unwrap_or_default()
    }

    /// The scalar fade stays the default for every class, so a cue
    /// with no `timing` fades exactly as it did.
    // r[verify cues.timing.per-attribute]
    #[test]
    fn old_cue_json_still_loads_and_fades_as_before() {
        let cue: Cue =
            serde_json::from_str(r#"{"name":"Old","fade_secs":2.0,"values":[]}"#).unwrap();
        assert_eq!(cue.timing, CueTiming::default());
        assert_eq!(cue.mib, Mib::default());
        assert_eq!(cue.trig, Trig::Go);
        assert!(!cue.assert && !cue.cue_only && !cue.morph);
        assert!(cue.release.is_empty() && cue.commands.is_empty());
        assert!(cue.fan.is_none());
        // And a default cue serialises without any of the new keys.
        let json = serde_json::to_string(&Cue {
            name: "Plain".into(),
            ..Default::default()
        })
        .unwrap();
        for key in [
            "timing",
            "mib",
            "trig",
            "assert",
            "cue_only",
            "release",
            "morph",
            "fan",
            "number",
            "note",
            "appearance",
        ] {
            assert!(!json.contains(key), "{key} leaked into {json}");
        }
    }

    /// The list needs to draw what the player is doing: how far into
    /// its arrival the standing cue is, and how long until a follow
    /// takes itself.
    // r[verify studio.cuelist.live-state]
    #[test]
    fn the_player_reports_its_fade_and_its_next_take() {
        let (groups, speeds) = show_120();
        let show = with_speeds(&groups, &speeds);
        let mut player = CuePlayer::new(vec![
            cue("Up", 4.0, vec![(1, Attribute::Dimmer, 1.0)]),
            Cue {
                trig: Trig::Follow { beats: 4.0 }, // two seconds
                ..cue("Follows", 1.0, vec![(1, Attribute::Dimmer, 0.5)])
            },
        ]);
        // Nothing taken: nothing arriving, and the next cue is the
        // first one, which nobody counts down to.
        assert_eq!(player.fade_progress(), 1.0);
        assert_eq!(player.next_cue(), Some(0));
        assert!(player.next_in().is_none(), "cue 1 waits for a hand");

        player.go(&show);
        player.tick_with(1.0, &show);
        let fade = player.fade_progress();
        assert!(
            (fade - 0.25).abs() < 1e-3,
            "a second into a four-second arrival: {fade}"
        );
        // The follow is counting itself down from two seconds.
        let left = player.next_in().expect("a follow counts down");
        assert!((left - 1.0).abs() < 1e-3, "one second left of two: {left}");

        player.tick_with(3.0, &show);
        assert_eq!(player.current_index(), Some(1), "the follow took itself");
        assert_eq!(player.fade_progress(), 1.0, "and has arrived");
    }

    /// A cue inserted between 5 and 6 is 5.5, and nothing after it
    /// renumbers — which is the whole point of a number that is not an
    /// index.
    // r[verify cues.number]
    #[test]
    fn a_cue_number_survives_insertion() {
        let mut list = CueList {
            cues: (0..6).map(|i| cue(&format!("c{i}"), 1.0, vec![])).collect(),
            ..Default::default()
        };
        // Unnumbered, a cue is called by its place in the list.
        assert_eq!(list.number_of(0), 1.0);
        assert_eq!(list.number_of(5), 6.0);
        list.renumber();
        let between = list.number_for_insert(5);
        assert_eq!(between, 5.5);
        let mut inserted = cue("new", 1.0, vec![]);
        inserted.number = Some(between);
        list.cues.insert(5, inserted);
        // Everything that was called six is still called six.
        assert_eq!(list.number_of(5), 5.5);
        assert_eq!(list.number_of(6), 6.0);
        // Appending continues by whole numbers; inserting at the top
        // goes below the first.
        assert_eq!(list.number_for_insert(list.cues.len()), 7.0);
        assert_eq!(list.number_for_insert(0), 0.0);
    }

    /// The identity fields are drawing and documentation only, and a
    /// file that has never heard of them loads unchanged.
    // r[verify cues.note]
    // r[verify cues.appearance]
    // r[verify cues.recipe.name]
    #[test]
    fn notes_numbers_and_appearance_round_trip_without_touching_output() {
        let mut part = Recipe::new(
            Selection::Group("Pars".to_string()),
            RecipeApply::Dimmer(1.0),
        );
        part.name = Some("pars up".into());
        part.note = Some("MD holds the bar if the crowd goes".into());
        part.cue_timing = Some(CueTiming {
            position: Some(4.0),
            ..Default::default()
        });
        let written = Cue {
            name: "Chorus".into(),
            number: Some(5.5),
            note: "wait for the bass drop".into(),
            appearance: Some(Appearance {
                color: "#c0392b".into(),
                label: Some("CH".into()),
            }),
            recipes: vec![RecipeRef::Inline(part)],
            ..Default::default()
        };
        let json = serde_json::to_string(&written).unwrap();
        let back: Cue = serde_json::from_str(&json).unwrap();
        assert_eq!(back, written, "{json}");
        let RecipeRef::Inline(r) = &back.recipes[0] else {
            panic!("the part came back as something else");
        };
        assert_eq!(r.name.as_deref(), Some("pars up"));
        assert_eq!(r.cue_timing.unwrap().position, Some(4.0));

        // A reference can be a named, separately-timed part too.
        let referenced = RecipeRef::named("chase")
            .part("movers swing in")
            .arriving(CueTiming {
                position: Some(8.0),
                ..Default::default()
            });
        let back: RecipeRef =
            serde_json::from_str(&serde_json::to_string(&referenced).unwrap()).unwrap();
        assert_eq!(back, referenced);

        // And a recipe written before any of this loads as it did.
        let old: Recipe =
            serde_json::from_str(r#"{"target":{"Group":"Pars"},"apply":{"Dimmer":1.0}}"#).unwrap();
        assert!(old.name.is_none() && old.note.is_none() && old.cue_timing.is_none());
    }

    /// One recipe on a cue, carrying its own arrival. A cue's recipes
    /// are its parts, and this is the whole reason parts exist: the
    /// part arrives on its own fade, not the cue's.
    // r[verify cues.recipe.timing]
    // r[verify cues.parts-are-recipes]
    #[test]
    fn a_recipes_own_timing_beats_the_cues() {
        let (groups, speeds) = show_120();
        let show = with_speeds(&groups, &speeds);
        let mut part = Recipe::new(
            Selection::Group("Pars".to_string()),
            RecipeApply::Dimmer(1.0),
        );
        // One beat is half a second here; the cue's own fade is four.
        part.cue_timing = Some(CueTiming {
            dimmer_in: Some(1.0),
            ..Default::default()
        });
        part.name = Some("pars up".into());
        let mut player = CuePlayer::new(vec![Cue {
            name: "Part".into(),
            fade_secs: 4.0,
            recipes: vec![RecipeRef::Inline(part)],
            ..Default::default()
        }]);
        player.go(&show);
        player.tick(0.5);
        let level = get(&player, &show, 1, Attribute::Dimmer);
        assert!(
            (level - 1.0).abs() < 1e-3,
            "the part's own half-second fade has arrived, not the cue's four: {level}"
        );
    }

    /// Two parts covering one key: the later one's timing wins, the
    /// same rule that decides its value.
    // r[verify cues.recipe.timing-precedence]
    #[test]
    fn the_later_part_owns_the_timing_of_a_shared_key() {
        let (groups, speeds) = show_120();
        let show = with_speeds(&groups, &speeds);
        let part = |beats: f32| {
            let mut r = Recipe::new(
                Selection::Group("Pars".to_string()),
                RecipeApply::Dimmer(1.0),
            );
            r.cue_timing = Some(CueTiming {
                dimmer_in: Some(beats),
                ..Default::default()
            });
            RecipeRef::Inline(r)
        };
        let mut player = CuePlayer::new(vec![Cue {
            name: "Both".into(),
            fade_secs: 4.0,
            // Eight beats (4 s) first, one beat (0.5 s) second.
            recipes: vec![part(8.0), part(1.0)],
            ..Default::default()
        }]);
        player.go(&show);
        player.tick(0.5);
        let level = get(&player, &show, 1, Attribute::Dimmer);
        assert!(
            (level - 1.0).abs() < 1e-3,
            "the second part's half-second fade wins: {level}"
        );
    }

    /// A named selection is more specific than a part, so its override
    /// wins over the part's own timing.
    // r[verify cues.recipe.timing-precedence]
    #[test]
    fn a_named_override_beats_a_parts_own_timing() {
        let (groups, speeds) = show_120();
        let show = with_speeds(&groups, &speeds);
        let mut part = Recipe::new(
            Selection::Group("Pars".to_string()),
            RecipeApply::Dimmer(1.0),
        );
        part.cue_timing = Some(CueTiming {
            dimmer_in: Some(1.0), // half a second
            ..Default::default()
        });
        let mut player = CuePlayer::new(vec![Cue {
            name: "Exception".into(),
            fade_secs: 4.0,
            recipes: vec![RecipeRef::Inline(part)],
            timing_overrides: vec![TimingOverride {
                selection: Selection::Chans(vec![1]),
                timing: CueTiming {
                    dimmer_in: Some(8.0), // four seconds
                    ..Default::default()
                },
            }],
            ..Default::default()
        }]);
        player.go(&show);
        player.tick(0.5);
        // Chan 1 was named, so it takes the slow override; 2 and 3 keep
        // the part's own fast one.
        let named = get(&player, &show, 1, Attribute::Dimmer);
        let rest = get(&player, &show, 2, Attribute::Dimmer);
        assert!(
            (named - 0.125).abs() < 1e-3,
            "the named fixture is an eighth into four seconds: {named}"
        );
        assert!(
            (rest - 1.0).abs() < 1e-3,
            "the rest arrived on the part's own fade: {rest}"
        );
    }

    /// The dimmer goes out faster than it came up.
    // r[verify cues.timing.per-attribute]
    #[test]
    fn dimmer_out_can_be_faster_than_dimmer_in() {
        let (groups, speeds) = show_120();
        let show = with_speeds(&groups, &speeds);
        let timing = CueTiming {
            dimmer_in: Some(8.0),  // 4 s
            dimmer_out: Some(2.0), // 1 s
            ..Default::default()
        };
        let mut player = CuePlayer::new(vec![
            timed("Up", vec![(1, Attribute::Dimmer, 1.0)], timing),
            timed("Down", vec![(1, Attribute::Dimmer, 0.0)], timing),
        ]);
        player.go(&show);
        player.tick(1.0);
        let up = get(&player, &show, 1, Attribute::Dimmer);
        assert!((up - 0.25).abs() < 1e-3, "a second into a 4 s in: {up}");
        player.tick(3.0);
        player.go(&show);
        player.tick(0.5);
        let down = get(&player, &show, 1, Attribute::Dimmer);
        assert!(
            (down - 0.5).abs() < 1e-3,
            "half a second into a 1 s out: {down}"
        );
    }

    /// Colour snaps while the dimmer is still fading.
    // r[verify cues.timing.per-attribute]
    #[test]
    fn colour_snaps_while_the_dimmer_fades() {
        let (groups, speeds) = show_120();
        let show = with_speeds(&groups, &speeds);
        let mut player = CuePlayer::new(vec![
            cue(
                "Blue",
                0.0,
                vec![(1, Attribute::Dimmer, 0.0), (1, red(), 0.0)],
            ),
            timed(
                "Red",
                vec![(1, Attribute::Dimmer, 1.0), (1, red(), 1.0)],
                CueTiming {
                    color: Some(0.0),
                    ..Default::default()
                },
            ),
        ]);
        player.go(&show);
        player.go(&show);
        player.tick(1.0); // a quarter of the 4 s scalar fade
        let colour = get(&player, &show, 1, red());
        let dimmer = get(&player, &show, 1, Attribute::Dimmer);
        assert!((colour - 1.0).abs() < 1e-6, "colour did not snap: {colour}");
        assert!(
            (dimmer - 0.25).abs() < 1e-3,
            "dimmer did not fade: {dimmer}"
        );
    }

    /// A delayed class holds its old value until the delay has passed,
    /// then fades over its own time.
    // r[verify cues.delay]
    #[test]
    fn a_delay_holds_before_the_fade_begins() {
        let (groups, speeds) = show_120();
        let show = with_speeds(&groups, &speeds);
        let mut player = CuePlayer::new(vec![
            cue("Dark", 0.0, vec![(1, Attribute::Dimmer, 0.0)]),
            timed(
                "Up",
                vec![(1, Attribute::Dimmer, 1.0)],
                CueTiming {
                    dimmer_in: Some(2.0), // 1 s
                    delay: ClassDelays {
                        intensity: 2.0, // 1 s
                        ..Default::default()
                    },
                    ..Default::default()
                },
            ),
        ]);
        player.go(&show);
        player.go(&show);
        player.tick(0.9);
        assert!(
            get(&player, &show, 1, Attribute::Dimmer).abs() < 1e-6,
            "moved during its delay"
        );
        player.tick(0.6); // 0.5 s into the fade
        let v = get(&player, &show, 1, Attribute::Dimmer);
        assert!((v - 0.5).abs() < 1e-3, "{v}");
        player.tick(1.0);
        assert!((get(&player, &show, 1, Attribute::Dimmer) - 1.0).abs() < 1e-6);
    }

    /// A delay fan across the selection wipes: every fixture arrives at
    /// the same value, each later than the one before.
    // r[verify cues.fan]
    #[test]
    fn a_delay_fan_wipes_across_the_selection() {
        let (groups, speeds) = show_120();
        let show = with_speeds(&groups, &speeds);
        let mut player = CuePlayer::new(vec![Cue {
            fade_secs: 0.0,
            fan: Some(CueFan {
                delay: Fan::new(0.0, 4.0), // 0 s, 1 s, 2 s across three pars
                fade: Fan::default(),
            }),
            ..recipe_cue("Wipe", 1.0)
        }]);
        player.go(&show);
        player.tick(0.5);
        let out = player.output(&show);
        assert_eq!(out.get(&(1, Attribute::Dimmer)), Some(&1.0));
        assert_eq!(
            out.get(&(2, Attribute::Dimmer)).copied().unwrap_or(0.0),
            0.0
        );
        assert_eq!(
            out.get(&(3, Attribute::Dimmer)).copied().unwrap_or(0.0),
            0.0
        );
        player.tick(1.0);
        let out = player.output(&show);
        assert_eq!(out.get(&(2, Attribute::Dimmer)), Some(&1.0));
        assert_eq!(
            out.get(&(3, Attribute::Dimmer)).copied().unwrap_or(0.0),
            0.0
        );
        player.tick(1.0);
        assert_eq!(
            player.output(&show).get(&(3, Attribute::Dimmer)),
            Some(&1.0)
        );
    }

    /// A fade fan gives depth in time: the front snaps, the back drifts.
    // r[verify cues.fan]
    #[test]
    fn a_fade_fan_snaps_the_front_and_drifts_the_back() {
        let (groups, speeds) = show_120();
        let show = with_speeds(&groups, &speeds);
        let mut player = CuePlayer::new(vec![Cue {
            fan: Some(CueFan {
                delay: Fan::default(),
                fade: Fan::new(0.0, 4.0), // 0 s, 1 s, 2 s
            }),
            ..recipe_cue("Depth", 1.0)
        }]);
        player.go(&show);
        player.tick(0.5);
        let out = player.output(&show);
        assert_eq!(out.get(&(1, Attribute::Dimmer)), Some(&1.0));
        let mid = out[&(2, Attribute::Dimmer)];
        let back = out[&(3, Attribute::Dimmer)];
        assert!((mid - 0.5).abs() < 1e-3, "{mid}");
        assert!((back - 0.25).abs() < 1e-3, "{back}");
    }

    /// With a delay fan, each fixture's phase is measured from its own
    /// start: the delayed fixture reads the phaser where the first one
    /// read it that long ago.
    // r[verify cues.delay-to-phase]
    #[test]
    fn a_delay_fan_offsets_each_units_phase_by_its_delay() {
        let (groups, speeds) = show_120();
        let show = with_speeds(&groups, &speeds);
        let at = |v: f32| Step {
            transition: 1.0, // always moving, so time is visible
            ..Step::new(vec![RecipeApply::Raw(vec![(Attribute::Dimmer, v)])])
        };
        let ramp = Recipe {
            target: Selection::Group("Pars".into()),
            steps: vec![at(0.0), at(1.0)],
            timing: Timing {
                speed: Speed::Hz(0.1), // a ten-second cycle, lockstep
                ..Default::default()
            },
            tricks: Vec::new(),
            stack: false,
            ..Default::default()
        };
        let mut player = CuePlayer::new(vec![Cue {
            name: "Ramp".into(),
            recipes: vec![ramp.into()],
            fan: Some(CueFan {
                delay: Fan::new(0.0, 4.0), // 0 s, 1 s, 2 s
                fade: Fan::default(),
            }),
            ..Default::default()
        }]);
        player.go(&show);
        // Stamp the fade as finished so only the phaser is in play.
        player.tick(3.0);
        let out = player.output(&show);
        let first = out[&(1, Attribute::Dimmer)];
        let third = out[&(3, Attribute::Dimmer)];
        // The third par started two seconds later, so it is where the
        // first was two seconds ago.
        player.advance_clock(-2.0);
        let first_two_ago = player.output(&show)[&(1, Attribute::Dimmer)];
        assert!(
            (third - first_two_ago).abs() < 1e-3,
            "phase not from own start: now {first}, third {third}, first two seconds ago {first_two_ago}"
        );
        assert!((first - third).abs() > 0.05, "no offset at all");
    }

    fn mover_cues(mode: MibMode) -> Vec<Cue> {
        vec![
            cue(
                "Dark",
                0.0,
                vec![(1, Attribute::Dimmer, 0.0), (1, Attribute::Pan, 0.0)],
            ),
            Cue {
                mib: Mib {
                    mode,
                    fade_beats: 2.0, // 1 s
                    delay_beats: 0.0,
                    ..Default::default()
                },
                ..cue(
                    "Aimed",
                    4.0,
                    vec![(1, Attribute::Dimmer, 1.0), (1, Attribute::Pan, 90.0)],
                )
            },
        ]
    }

    /// A mover dark in cue 1 and aimed elsewhere in cue 2 is already
    /// there when cue 2 takes, and its intensity was never touched.
    // r[verify cues.mib]
    // r[verify cues.mib.timing]
    #[test]
    fn a_dark_mover_is_pre_positioned_for_the_coming_cue() {
        let (groups, speeds) = show_120();
        let show = with_speeds(&groups, &speeds);
        let mut player = CuePlayer::new(mover_cues(MibMode::Early));
        player.go(&show);
        player.tick(0.5); // halfway through the pre-position's own fade
        let pan = get(&player, &show, 1, Attribute::Pan);
        assert!(
            (pan - 45.0).abs() < 1e-3,
            "pre-position not on its own fade: {pan}"
        );
        assert!(get(&player, &show, 1, Attribute::Dimmer).abs() < 1e-6);
        player.tick(1.0);
        assert!((get(&player, &show, 1, Attribute::Pan) - 90.0).abs() < 1e-3);
        assert!(
            get(&player, &show, 1, Attribute::Dimmer).abs() < 1e-6,
            "intensity was touched"
        );
        player.go(&show);
        player.tick(2.0); // halfway into cue 2's 4 s fade
        assert!(
            (get(&player, &show, 1, Attribute::Pan) - 90.0).abs() < 1e-3,
            "the move was visible after the dimmer came up"
        );
        let dimmer = get(&player, &show, 1, Attribute::Dimmer);
        assert!((dimmer - 0.5).abs() < 1e-3, "{dimmer}");
    }

    /// `None` disables it: the swing happens in the light.
    // r[verify cues.mib.mode]
    #[test]
    fn mib_none_leaves_the_mover_where_it_was() {
        let (groups, speeds) = show_120();
        let show = with_speeds(&groups, &speeds);
        let mut player = CuePlayer::new(mover_cues(MibMode::None));
        player.go(&show);
        player.tick(2.0);
        assert!(get(&player, &show, 1, Attribute::Pan).abs() < 1e-6);
        player.go(&show);
        player.tick(2.0);
        let pan = get(&player, &show, 1, Attribute::Pan);
        assert!((pan - 45.0).abs() < 1e-3, "{pan}");
    }

    /// `Late` waits: with both cues positioned, the move lands on the
    /// coming cue's downbeat rather than as soon as the fixture is dark.
    // r[verify cues.mib.mode]
    #[test]
    fn mib_late_arrives_just_before_the_coming_cue() {
        let (groups, speeds) = show_120();
        let show = with_speeds(&groups, &speeds);
        let mut cues = mover_cues(MibMode::Late);
        cues[0].at = Some(Bars::bar(1).into());
        cues[1].at = Some(Bars::bar(2).into()); // four beats = 2 s later
        let mut player = CuePlayer::new(cues);
        player.go(&show);
        player.tick(0.9);
        assert!(
            get(&player, &show, 1, Attribute::Pan).abs() < 1e-6,
            "moved early"
        );
        player.tick(0.6); // 1.5 s: halfway through a 1 s fade ending at 2 s
        let pan = get(&player, &show, 1, Attribute::Pan);
        assert!((pan - 45.0).abs() < 1e-3, "{pan}");
        player.tick(0.5);
        assert!((get(&player, &show, 1, Attribute::Pan) - 90.0).abs() < 1e-3);
    }

    /// An asserting cue re-takes what it would only track: a one-shot
    /// tracked from the previous cue plays again with this cue.
    // r[verify cues.assert]
    #[test]
    fn an_asserting_cue_retakes_what_it_tracks() {
        let groups = pars();
        let show = Show::new(&groups, &crate::selection::EMPTY_RIG);
        let quiet = |assert: bool| Cue {
            name: "· wider".into(),
            assert,
            ..Default::default()
        };
        for (assert, expect) in [(false, 0.3), (true, 0.8)] {
            let mut player = CuePlayer::new(vec![
                recipe_cue("Look", 0.3),
                accent(0.5, true),
                quiet(assert),
            ]);
            player.go(&show);
            player.go(&show);
            player.tick(10.0); // the bump has long withdrawn
            player.go(&show);
            let v = get(&player, &show, 1, Attribute::Dimmer);
            assert!((v - expect).abs() < 1e-3, "assert={assert}: {v}");
        }
    }

    /// A cue-only cue's changes do not track: the cue after it starts
    /// from the state before it.
    // r[verify cues.cue-only]
    #[test]
    fn a_cue_only_cue_does_not_track_forward() {
        let show = bare();
        let mut player = CuePlayer::new(vec![
            cue(
                "Base",
                0.0,
                vec![(1, Attribute::Dimmer, 0.2), (2, Attribute::Dimmer, 0.2)],
            ),
            Cue {
                cue_only: true,
                ..cue("Fix", 0.0, vec![(2, Attribute::Dimmer, 1.0)])
            },
            cue("Next", 0.0, vec![(1, Attribute::Dimmer, 0.5)]),
        ]);
        player.go(&show);
        player.go(&show);
        assert_eq!(
            get(&player, &show, 2, Attribute::Dimmer),
            1.0,
            "the fix itself shows"
        );
        player.go(&show);
        assert!(
            (get(&player, &show, 2, Attribute::Dimmer) - 0.2).abs() < 1e-6,
            "the fix tracked into the next cue"
        );
        assert!((get(&player, &show, 1, Attribute::Dimmer) - 0.5).abs() < 1e-6);
    }

    /// A released attribute leaves the layers and fades to rest.
    // r[verify cues.release]
    // r[verify playback.release-falls-through]
    #[test]
    fn a_released_attribute_falls_through_to_rest() {
        let show = bare();
        let mut player = CuePlayer::new(vec![
            cue(
                "Lit",
                0.0,
                vec![(1, Attribute::Dimmer, 1.0), (2, Attribute::Dimmer, 1.0)],
            ),
            Cue {
                release: vec![Attribute::Dimmer],
                ..cue("Let go", 2.0, vec![(1, Attribute::Pan, 10.0)])
            },
        ]);
        player.go(&show);
        player.go(&show);
        player.tick(1.0);
        let out = player.output(&show);
        let one = out.get(&(1, Attribute::Dimmer)).copied().unwrap_or(0.0);
        assert!(
            (one - 0.5).abs() < 1e-3,
            "released dimmer fades to rest: {one}"
        );
        assert_eq!(
            out.get(&(2, Attribute::Dimmer)),
            Some(&1.0),
            "an uncovered fixture keeps its value"
        );
        player.tick(1.0);
        assert!(
            !player.output(&show).contains_key(&(1, Attribute::Dimmer)),
            "still held after release"
        );
    }

    /// A circle crossfading into a figure-eight is halfway between the
    /// two at half fade — not either one.
    // r[verify cues.morph]
    // r[verify effects.morph]
    #[test]
    fn a_morph_crossfades_recipe_values_rather_than_snapping() {
        let show = bare();
        let pan = |a: f32, b: f32, hz: f32| Recipe {
            target: Selection::Chans(vec![1]),
            steps: vec![
                Step::new(vec![RecipeApply::Raw(vec![(Attribute::Pan, a)])]),
                Step::new(vec![RecipeApply::Raw(vec![(Attribute::Pan, b)])]),
            ],
            timing: Timing {
                speed: Speed::Hz(hz),
                ..Default::default()
            },
            tricks: Vec::new(),
            stack: false,
            ..Default::default()
        };
        let circle = Cue {
            name: "Circle".into(),
            recipes: vec![pan(-90.0, 90.0, 0.25).into()],
            ..Default::default()
        };
        let eight = Cue {
            name: "Eight".into(),
            fade_secs: 2.0,
            morph: true,
            recipes: vec![pan(-30.0, 30.0, 0.5).into()],
            ..Default::default()
        };
        let alone = |c: Cue| {
            let mut p = CuePlayer::new(vec![c]);
            p.go(&show);
            p.tick(1.7);
            get(&p, &show, 1, Attribute::Pan)
        };
        let a = alone(circle.clone());
        let b = alone(Cue {
            fade_secs: 0.0, // its own fade-in must not colour the baseline
            ..eight.clone()
        });
        let mut player = CuePlayer::new(vec![circle, eight]);
        player.go(&show);
        player.tick(0.7);
        player.go(&show);
        player.tick(1.0); // half of the 2 s fade, clock at 1.7 like the others
        let v = get(&player, &show, 1, Attribute::Pan);
        assert!(
            (v - (a + b) / 2.0).abs() < 1e-2,
            "expected midway between {a} and {b}: {v}"
        );
        assert!(
            (v - a).abs() > 1.0 && (v - b).abs() > 1.0,
            "snapped: {v} (a={a}, b={b})"
        );
    }

    /// Follows chain on wall time at the Song tempo, with no transport.
    // r[verify cues.trig]
    #[test]
    fn follow_cues_take_themselves_in_turn() {
        let (groups, speeds) = show_120();
        let show = with_speeds(&groups, &speeds);
        let follow = |name: &str, level: f32| Cue {
            trig: Trig::Follow { beats: 2.0 }, // 1 s
            ..cue(name, 0.0, vec![(1, Attribute::Dimmer, level)])
        };
        let mut player = CuePlayer::new(vec![
            cue("A", 0.0, vec![(1, Attribute::Dimmer, 0.1)]),
            follow("B", 0.2),
            follow("C", 0.3),
            cue("D", 0.0, vec![(1, Attribute::Dimmer, 0.4)]),
        ]);
        player.go(&show);
        player.tick_with(0.9, &show);
        assert_eq!(player.current_index(), Some(0), "followed early");
        player.tick_with(0.2, &show);
        assert_eq!(player.current_index(), Some(1));
        player.tick_with(0.5, &show);
        assert_eq!(player.current_index(), Some(1));
        // Counted from when B was *due*, not noticed: C lands at 2.0 s.
        player.tick_with(0.45, &show);
        assert_eq!(player.current_index(), Some(2));
        player.tick_with(5.0, &show);
        assert_eq!(player.current_index(), Some(2), "a GO cue took itself");
        assert!(player.pending_sound_trig().is_none());
    }

    /// One long frame takes a whole chain of follows that came due in it.
    // r[verify cues.trig]
    #[test]
    fn a_chain_of_follows_survives_a_long_frame() {
        let (groups, speeds) = show_120();
        let show = with_speeds(&groups, &speeds);
        let follow = |name: &str| Cue {
            trig: Trig::Follow { beats: 2.0 },
            ..cue(name, 0.0, vec![])
        };
        let mut player = CuePlayer::new(vec![cue("A", 0.0, vec![]), follow("B"), follow("C")]);
        player.go(&show);
        player.tick_with(2.5, &show);
        assert_eq!(player.current_index(), Some(2));
    }

    /// A sound trigger is exposed for the host; a positional trigger is
    /// visible to the clock like `at`.
    // r[verify cues.trig]
    #[test]
    fn sound_and_positional_trigs_are_exposed() {
        let show = bare();
        let mut player = CuePlayer::new(vec![
            cue("A", 0.0, vec![]),
            Cue {
                trig: Trig::Sound {
                    band: "kick".into(),
                },
                ..cue("B", 0.0, vec![])
            },
            Cue {
                trig: Trig::At(Bars::bar(9)),
                ..cue("C", 0.0, vec![])
            },
        ]);
        assert_eq!(player.pending_sound_trig(), None);
        player.go(&show);
        assert_eq!(player.pending_sound_trig(), Some("kick"));
        player.go(&show);
        assert_eq!(player.pending_sound_trig(), None);
        assert_eq!(player.index_at(Bars::bar(9)), Some(2));
    }

    /// Commands ride along with the cue and are handed out once.
    // r[verify cues.command]
    #[test]
    fn commands_of_taken_cues_are_drained_once() {
        let show = bare();
        let mut player = CuePlayer::new(vec![
            Cue {
                commands: vec!["osc /clip/1 start".into()],
                ..cue("A", 0.0, vec![])
            },
            Cue {
                commands: vec!["macro 3".into()],
                ..cue("B", 0.0, vec![])
            },
        ]);
        assert!(player.drain_commands().is_empty());
        player.go(&show);
        player.go(&show);
        assert_eq!(
            player.drain_commands(),
            vec!["osc /clip/1 start".to_string(), "macro 3".to_string()]
        );
        assert!(player.drain_commands().is_empty());
        // Replaying to a position rebuilds state without re-running
        // the host's side effects.
        let mut replay = CuePlayer::new(player.cues().to_vec());
        replay.jump_to_end_of(1, &show);
        assert_eq!(replay.drain_commands(), vec!["macro 3".to_string()]);
    }

    /// Two stacking sustained modulators sum instead of the later one
    /// winning.
    // r[verify effects.relative-stack]
    #[test]
    fn stacking_relative_recipes_sum() {
        let groups = pars();
        let show = Show::new(&groups, &crate::selection::EMPTY_RIG);
        let mut look = look_with_modulation(0.3, 0.2);
        if let RecipeRef::Inline(r) = &mut look.recipes[1] {
            r.stack = true;
        }
        let mut lift = accent(0.1, false);
        if let RecipeRef::Inline(r) = &mut lift.recipes[0] {
            r.stack = true;
        }
        let mut player = CuePlayer::new(vec![look, lift]);
        player.go(&show);
        player.go(&show);
        let v = get(&player, &show, 1, Attribute::Dimmer);
        assert!(
            (v - 0.6).abs() < 1e-4,
            "expected 0.3 + 0.2 + 0.1 with both stacking: {v}"
        );
    }
}

#[cfg(test)]
mod wave2_tests {
    use super::*;
    use crate::focus::pan_tilt_deg_to_point;
    use crate::music::{Section, TempoMap};
    use crate::preset::Ref;
    use crate::recipe::RecipeApply;
    use crate::selection::{FixtureInfo, Rig, Selection};
    use crate::step::{Speed, Step, Timing};
    use ignition_proto::{Placement, Quat};

    fn v(x: f64, y: f64, z: f64) -> Vec3 {
        Vec3 { x, y, z }
    }

    /// One mover, chan 1, hung at `trim` metres straight over the origin.
    fn hung_at(trim: f64) -> Rig {
        Rig::new(vec![FixtureInfo {
            chan: 1,
            placement: Some(Placement {
                position: v(0.0, 0.0, trim),
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

    fn aim(point: Vec3) -> Cue {
        Cue {
            name: "aim".into(),
            recipes: vec![
                Recipe::new(
                    Selection::Chans(vec![1]),
                    RecipeApply::FocusPoint(Ref::Inline(point)),
                )
                .into(),
            ],
            ..Default::default()
        }
    }

    /// A `room circle`-style recipe: a path in metres, blended between
    /// steps, one cycle a second.
    fn orbit() -> Recipe {
        let step = |x: f64, y: f64| Step {
            apply: vec![RecipeApply::FocusDelta(v(x, y, 0.0))],
            width: 1.0,
            transition: 1.0,
            ..Step::new(Vec::new())
        };
        Recipe {
            target: Selection::Chans(vec![1]),
            steps: vec![
                step(2.0, 0.0),
                step(0.0, 2.0),
                step(-2.0, 0.0),
                step(0.0, -2.0),
            ],
            timing: Timing {
                speed: Speed::Hz(1.0),
                ..Default::default()
            },
            tricks: Vec::new(),
            stack: false,
            ..Default::default()
        }
    }

    fn orbit_cue() -> Cue {
        Cue {
            name: "· orbit".into(),
            recipes: vec![orbit().into()],
            ..Default::default()
        }
    }

    fn pan_tilt(player: &CuePlayer, show: &Show<'_>) -> Option<(f32, f32)> {
        let out = player.output(show);
        Some((
            *out.get(&(1, Attribute::Pan))?,
            *out.get(&(1, Attribute::Tilt))?,
        ))
    }

    /// The delta the orbit offers at `secs`, per the recipe contract.
    fn delta_at(secs: f32, rig: &Rig) -> Vec3 {
        expand_recipe_full(&orbit(), &Show::new(&[], rig), secs).focus_deltas[0].delta
    }

    /// r[verify focus.delta]
    /// r[verify focus.orbit-in-metres]
    #[test]
    fn a_metre_orbit_on_an_aimed_fixture_moves_its_pan_and_tilt_over_time() {
        let rig = hung_at(5.0);
        let show = Show::new(&[], &rig);
        let mut player = CuePlayer::new(vec![aim(v(0.0, 0.0, 0.0)), orbit_cue()]);
        player.go(&show);
        let still = pan_tilt(&player, &show).unwrap();
        player.go(&show);
        let a = pan_tilt(&player, &show).unwrap();
        player.advance_clock(0.25);
        let b = pan_tilt(&player, &show).unwrap();
        assert_ne!(a, still, "the orbit moved the aim off the point");
        assert_ne!(a, b, "and it keeps moving");
        // Solved from the point plus the delta, not from the angles.
        let expect = |secs: f32| {
            pan_tilt_deg_to_point(
                v(0.0, 0.0, 5.0),
                Quat {
                    w: 1.0,
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
                crate::focus::v_add(v(0.0, 0.0, 0.0), delta_at(secs, &rig)),
            )
        };
        assert_eq!(a, expect(0.0));
        assert_eq!(b, expect(0.25));
    }

    /// r[verify focus.delta] - a fixture with no point aim ignores the delta
    #[test]
    fn a_metre_orbit_does_nothing_to_a_fixture_aimed_by_raw_angles() {
        let rig = hung_at(5.0);
        let show = Show::new(&[], &rig);
        let raw = Cue {
            name: "raw".into(),
            recipes: vec![
                Recipe::new(
                    Selection::Chans(vec![1]),
                    RecipeApply::Raw(vec![(Attribute::Pan, 12.0), (Attribute::Tilt, 34.0)]),
                )
                .into(),
            ],
            ..Default::default()
        };
        let mut player = CuePlayer::new(vec![raw, orbit_cue()]);
        player.go(&show);
        player.go(&show);
        assert_eq!(pan_tilt(&player, &show), Some((12.0, 34.0)));
        player.advance_clock(0.3);
        assert_eq!(pan_tilt(&player, &show), Some((12.0, 34.0)));
        // And a later cue that re-aims by angle drops the earlier point.
        let mut player = CuePlayer::new(vec![
            aim(v(0.0, 0.0, 0.0)),
            Cue {
                name: "by hand".into(),
                values: vec![
                    CueValue {
                        chan: 1,
                        attr: Attribute::Pan,
                        value: -5.0,
                    },
                    CueValue {
                        chan: 1,
                        attr: Attribute::Tilt,
                        value: 40.0,
                    },
                ],
                ..Default::default()
            },
            orbit_cue(),
        ]);
        player.jump_to_end_of(2, &show);
        assert_eq!(pan_tilt(&player, &show), Some((-5.0, 40.0)));
    }

    /// r[verify focus.orbit-in-metres] - the same floor circle at two trim heights
    #[test]
    fn two_rigs_at_different_trims_trace_the_same_floor_circle() {
        let floor = v(0.0, 0.0, 0.0);
        for secs in [0.0f32, 0.1, 0.4, 0.7] {
            let mut points = Vec::new();
            for trim in [5.0, 8.0] {
                let rig = hung_at(trim);
                let show = Show::new(&[], &rig);
                let mut player = CuePlayer::new(vec![aim(floor), orbit_cue()]);
                player.jump_to_end_of(1, &show);
                player.advance_clock(secs);
                let got = pan_tilt(&player, &show).unwrap();
                // The point this rig is looking at: the one whose solved
                // angles match the output.
                let point = crate::focus::v_add(floor, delta_at(secs, &rig));
                let want = pan_tilt_deg_to_point(
                    v(0.0, 0.0, trim),
                    Quat {
                        w: 1.0,
                        x: 0.0,
                        y: 0.0,
                        z: 0.0,
                    },
                    point,
                );
                assert_eq!(got, want, "trim {trim} at {secs}s");
                points.push((point, got));
            }
            // Same room point, different angles.
            assert_eq!(points[0].0, points[1].0);
            assert_ne!(points[0].1, points[1].1);
        }
    }

    // -----------------------------------------------------------------
    // Library by name
    // -----------------------------------------------------------------

    fn flat(level: f32) -> Recipe {
        Recipe::new(Selection::Chans(vec![1, 2]), RecipeApply::Dimmer(level))
    }

    fn library() -> BTreeMap<String, Recipe> {
        BTreeMap::from([
            ("flat".to_string(), flat(0.5)),
            ("half".to_string(), flat(0.25)),
        ])
    }

    fn bundles() -> BTreeMap<String, crate::profile::Bundle> {
        BTreeMap::from([(
            "both".to_string(),
            crate::profile::Bundle {
                name: "both".into(),
                family: "test".into(),
                about: String::new(),
                recipes: vec!["flat".into(), "half".into()],
            },
        )])
    }

    fn with_library<'a>(
        library: &'a BTreeMap<String, Recipe>,
        bundles: &'a BTreeMap<String, crate::profile::Bundle>,
    ) -> Show<'a> {
        let mut show = Show::new(&[], &crate::selection::EMPTY_RIG);
        show.library = library;
        show.bundles = bundles;
        show
    }

    use std::collections::BTreeMap;

    /// r[verify effects.library.by-name]
    #[test]
    fn a_cue_names_a_library_effect_and_the_player_resolves_it_at_take() {
        let (lib, bun) = (library(), bundles());
        let show = with_library(&lib, &bun);
        let mut player = CuePlayer::new(vec![Cue {
            name: "named".into(),
            recipes: vec![RecipeRef::named("flat").on(Selection::Chans(vec![3]))],
            ..Default::default()
        }]);
        player.go(&show);
        let out = player.output(&show);
        assert_eq!(out.get(&(3, Attribute::Dimmer)), Some(&0.5), "retargeted");
        assert!(out.get(&(1, Attribute::Dimmer)).is_none());
        assert!(crate::recipe::unresolved(player.cues(), &show).is_empty());
    }

    /// r[verify effects.bundle]
    #[test]
    fn a_bundle_takes_every_member() {
        let (lib, bun) = (library(), bundles());
        let show = with_library(&lib, &bun);
        let cue = Cue {
            name: "bundled".into(),
            recipes: vec![RecipeRef::Bundle {
                bundle: "both".into(),
                target: None,
            }],
            ..Default::default()
        };
        assert_eq!(cue.resolved_recipes(&show).len(), 2);
        let mut player = CuePlayer::new(vec![cue]);
        player.go(&show);
        // Last member wins the slot, as two inline recipes would.
        assert_eq!(
            player.output(&show).get(&(1, Attribute::Dimmer)),
            Some(&0.25)
        );
    }

    /// r[verify effects.library.by-name] - unknown names are reported, not fatal
    #[test]
    fn an_unknown_effect_or_bundle_is_reported_by_name() {
        let (lib, bun) = (library(), bundles());
        let show = with_library(&lib, &bun);
        let cues = vec![Cue {
            name: "typo".into(),
            recipes: vec![
                RecipeRef::named("flta"),
                RecipeRef::Bundle {
                    bundle: "bth".into(),
                    target: None,
                },
            ],
            ..Default::default()
        }];
        let problems = crate::recipe::unresolved(&cues, &show);
        assert_eq!(
            problems,
            vec![
                "cue \"typo\": no bundle \"bth\"".to_string(),
                "cue \"typo\": no library effect \"flta\"".to_string(),
            ]
        );
        assert_eq!(
            crate::recipe::cook_cue(&cues[0], &show, 0.0).status(),
            crate::recipe::Status::Failed
        );
        let mut player = CuePlayer::new(cues);
        player.go(&show);
        assert!(
            player.output(&show).is_empty(),
            "nothing resolves, nothing breaks"
        );
    }

    /// r[verify effects.library.by-name] - the JSON shapes
    /// r[verify files.additive-evolution]
    #[test]
    fn inline_named_and_bundle_references_load_from_json() {
        let json = r#"{
            "name": "mixed",
            "recipes": [
                {"target": {"Chans": [1]}, "apply": {"Dimmer": 1.0}},
                {"effect": "circle", "bars": 8.0},
                {"effect": "chase", "target": {"Role": "Wash"}, "tricks": [{"Group": 2}]},
                {"bundle": "intro reveal"}
            ]
        }"#;
        let cue: Cue = serde_json::from_str(json).unwrap();
        assert!(matches!(cue.recipes[0], RecipeRef::Inline(_)));
        assert!(matches!(
            &cue.recipes[1],
            RecipeRef::Named { effect, bars: Some(b), .. } if effect == "circle" && *b == 8.0
        ));
        assert!(matches!(
            &cue.recipes[2],
            RecipeRef::Named { tricks: Some(t), .. } if t.len() == 1
        ));
        assert!(
            matches!(&cue.recipes[3], RecipeRef::Bundle { bundle, .. } if bundle == "intro reveal")
        );
        // Round trip keeps the terse spellings.
        let back = serde_json::to_string(&cue).unwrap();
        assert!(back.contains(r#"{"effect":"circle","bars":8.0}"#), "{back}");
        assert!(back.contains(r#"{"bundle":"intro reveal"}"#), "{back}");
        assert_eq!(serde_json::from_str::<Cue>(&back).unwrap(), cue);
    }

    // -----------------------------------------------------------------
    // Relative positions in the file
    // -----------------------------------------------------------------

    fn song(chorus_at: u32) -> SongMap {
        SongMap {
            name: "t".into(),
            tempo: TempoMap::default(),
            sections: vec![
                Section {
                    name: "VS".into(),
                    start: Bars::bar(1),
                    bars: (chorus_at - 1) as f64,
                },
                Section {
                    name: "CH".into(),
                    start: Bars::bar(chorus_at),
                    bars: 8.0,
                },
            ],
        }
    }

    fn lit(name: &str, at: Position) -> Cue {
        Cue {
            name: name.into(),
            at: Some(at),
            recipes: vec![flat(1.0).into()],
            ..Default::default()
        }
    }

    /// r[verify song.relative-position]
    /// r[verify song.relative-position.resolved-on-load]
    #[test]
    fn a_relative_position_resolves_against_the_map_it_is_loaded_with() {
        let mut list = CueList {
            name: "t".into(),
            cues: vec![
                lit("CH", Position::at("CH", 0)),
                lit("VS", Position::at("VS", 0)),
                lit("· lift", Position::at("CH", 4)),
            ],
            triggers: vec![crate::trigger::Trigger {
                at: Position::last_bar("VS"),
                resolved: None,
                recipe: flat(0.1),
                name: "stab".into(),
                hold: false,
            }],
            ..Default::default()
        };
        // Unresolved, the relative cues are invisible to the clock.
        assert_eq!(list.cues[0].position(), None);
        assert_eq!(list.triggers[0].bars(), None);

        assert!(list.resolve_positions(&song(9)).is_empty());
        let names: Vec<&str> = list.cues.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["VS", "CH", "· lift"], "re-sorted by position");
        assert_eq!(list.cues[1].position(), Some(Bars::bar(9)));
        assert_eq!(list.cues[2].position(), Some(Bars::bar(13)));
        assert_eq!(list.triggers[0].bars(), Some(Bars::bar(8)));

        // The same file against a longer verse: everything moves.
        assert!(list.resolve_positions(&song(11)).is_empty());
        assert_eq!(list.cues[1].position(), Some(Bars::bar(11)));
        assert_eq!(list.cues[2].position(), Some(Bars::bar(15)));
        assert_eq!(list.triggers[0].bars(), Some(Bars::bar(10)));

        // A section the map lacks is named, and the cue keeps its bar.
        list.cues.push(lit("BR", Position::at("BR", 0)));
        let unresolved = list.resolve_positions(&song(11));
        assert_eq!(unresolved, ["BR"]);

        // The player seeks by the resolved bars.
        let show = Show::new(&[], &crate::selection::EMPTY_RIG);
        let mut player = CuePlayer::new(list.cues.clone());
        player.seek(Bars::bar(12), &show);
        assert_eq!(player.current_name(), Some("CH"));
    }

    /// r[verify song.relative-position.resolved-on-load] - no map, absolute still runs
    /// r[verify files.additive-evolution] - the old JSON still loads
    #[test]
    fn absolute_positions_and_cached_bars_need_no_map() {
        let old: Cue =
            serde_json::from_str(r#"{"name": "old", "at": {"bar": 22, "beat": 1.0}}"#).unwrap();
        assert_eq!(old.at, Some(Position::Absolute(Bars::bar(22))));
        assert_eq!(old.position(), Some(Bars::bar(22)));

        let new: Cue = serde_json::from_str(
            r#"{"name": "new", "at": {"section": "CH 1", "bars": 4}, "resolved": {"bar": 27}}"#,
        )
        .unwrap();
        assert_eq!(new.at, Some(Position::at("CH 1", 4)));
        assert_eq!(new.position(), Some(Bars::bar(27)), "the cached bar stands");

        let trigger: crate::trigger::Trigger = serde_json::from_str(
            r#"{"at": {"bar": 3, "beat": 2.5}, "name": "hit",
                "recipe": {"target": {"Chans": [1]}, "apply": {"Dimmer": 1.0}}}"#,
        )
        .unwrap();
        assert_eq!(trigger.bars(), Some(Bars::new(3, 2.5)));

        let mut player = CuePlayer::new(vec![old, new]);
        let show = Show::new(&[], &crate::selection::EMPTY_RIG);
        player.seek(Bars::bar(30), &show);
        assert_eq!(player.current_name(), Some("new"));
    }

    /// Late MIB counts the gap to the coming cue in the song's own
    /// signature: two bars of 3/4 are six beats, not eight, and a map
    /// that changes signature is counted stretch by stretch.
    // r[verify song.tempo-map] - beats between positions honour the signature
    #[test]
    fn beats_between_reads_the_tempo_map() {
        use crate::music::{TempoMap, TempoPoint, TimeSignature};
        let three = TimeSignature {
            numerator: 3,
            denominator: 4,
        };
        let waltz = TempoMap::constant(120.0, three);
        assert_eq!(beats_between(Bars::bar(1), Bars::bar(3), Some(&waltz)), 6.0);
        assert_eq!(beats_between(Bars::bar(1), Bars::bar(3), None), 8.0);
        assert_eq!(beats_between(Bars::bar(3), Bars::bar(1), Some(&waltz)), 0.0);

        // Two bars of 4/4, then 3/4 from bar 3: 8 + 3 beats to bar 4.
        let mixed = TempoMap::new(vec![
            TempoPoint {
                at: Bars::START,
                bpm: 120.0,
                time_signature: TimeSignature::default(),
            },
            TempoPoint {
                at: Bars::bar(3),
                bpm: 120.0,
                time_signature: three,
            },
        ]);
        assert_eq!(
            beats_between(Bars::bar(1), Bars::bar(4), Some(&mixed)),
            11.0
        );
        assert_eq!(
            beats_between(Bars::new(3, 2.0), Bars::new(4, 1.5), Some(&mixed)),
            2.5
        );
    }
}

#[cfg(test)]
mod focus_and_position_tests {
    use super::*;
    use crate::focus::pan_tilt_deg_to_point;
    use crate::group::Group;
    use crate::music::Bars;
    use crate::preset::Ref;
    use crate::recipe::RecipeApply;
    use crate::selection::{FixtureInfo, Rig, Selection};
    use crate::step::{Speed, Step, Timing};
    use ignition_proto::{Placement, Quat};

    fn v(x: f64, y: f64, z: f64) -> Vec3 {
        Vec3 { x, y, z }
    }

    const MOUNT: Quat = Quat {
        w: 1.0,
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };

    /// One head five metres up over the origin.
    fn rig() -> Rig {
        Rig::new(vec![FixtureInfo {
            chan: 1,
            placement: Some(Placement {
                position: v(0.0, 0.0, 5.0),
                orientation: MOUNT,
            }),
            manufacturer: String::new(),
            model: String::new(),
            tags: Vec::new(),
        }])
    }

    fn focus_cue(name: &str, fade_secs: f32, apply: RecipeApply) -> Cue {
        Cue {
            name: name.into(),
            fade_secs,
            recipes: vec![Recipe::new(Selection::Chans(vec![1]), apply).into()],
            ..Default::default()
        }
    }

    fn pan_tilt(out: &HashMap<(ChanId, Attribute), f32>) -> (f32, f32) {
        (out[&(1, Attribute::Pan)], out[&(1, Attribute::Tilt)])
    }

    /// A point and an orientation on one fixture: whichever cue came
    /// later owns pan and tilt outright, in either order, never an
    /// average of the two.
    /// r[verify focus.point-beats-orientation]
    #[test]
    fn the_later_of_a_point_and_an_orientation_wins_outright() {
        let rig = rig();
        let show = Show::new(&[], &rig);
        let point = RecipeApply::FocusPoint(Ref::Inline(v(0.0, 0.0, 0.0)));
        let out_over_the_crowd = RecipeApply::FocusDirection(v(0.0, 1.0, -1.0));
        let want_point = pan_tilt_deg_to_point(v(0.0, 0.0, 5.0), MOUNT, v(0.0, 0.0, 0.0));
        let want_dir = crate::focus::pan_tilt_deg_along(MOUNT, v(0.0, 1.0, -1.0));
        assert!((want_dir.1 - 45.0).abs() < 0.5);

        let mut player = CuePlayer::new(vec![
            focus_cue("Point", 0.0, point.clone()),
            focus_cue("Out", 0.0, out_over_the_crowd.clone()),
        ]);
        player.go(&show);
        assert_eq!(pan_tilt(&player.output(&show)), want_point);
        player.go(&show);
        assert_eq!(
            pan_tilt(&player.output(&show)),
            want_dir,
            "the orientation, not a blend"
        );

        let mut player = CuePlayer::new(vec![
            focus_cue("Out", 0.0, out_over_the_crowd),
            focus_cue("Point", 0.0, point),
        ]);
        player.go(&show);
        player.go(&show);
        assert_eq!(
            pan_tilt(&player.output(&show)),
            want_point,
            "the point, not a blend"
        );
    }

    /// Between two points the beam crosses the room in a straight line:
    /// halfway through the fade it aims at the midpoint of the two
    /// points, which is not what interpolating tilt would give.
    /// r[verify focus.straight-line]
    #[test]
    fn a_fade_between_two_points_aims_at_the_midpoint_halfway() {
        let rig = rig();
        let show = Show::new(&[], &rig);
        let a = v(-3.0, 0.0, 0.0);
        let b = v(3.0, 0.0, 0.0);
        let mut player = CuePlayer::new(vec![
            focus_cue("A", 0.0, RecipeApply::FocusPoint(Ref::Inline(a))),
            focus_cue("B", 2.0, RecipeApply::FocusPoint(Ref::Inline(b))),
        ]);
        player.go(&show);
        player.go(&show);
        player.tick(1.0);
        let (pan, tilt) = pan_tilt(&player.output(&show));
        let (_, want_tilt) = pan_tilt_deg_to_point(v(0.0, 0.0, 5.0), MOUNT, v(0.0, 0.0, 0.0));
        assert!(
            (tilt - want_tilt).abs() < 0.5,
            "midpoint is straight down, tilt {tilt}; a tilt crossfade would sit near 31"
        );
        let _ = pan;
        player.tick(1.0);
        let (pan_b, tilt_b) = pan_tilt_deg_to_point(v(0.0, 0.0, 5.0), MOUNT, b);
        let (pan, tilt) = pan_tilt(&player.output(&show));
        assert!(
            (pan - pan_b).abs() < 0.5 && (tilt - tilt_b).abs() < 0.5,
            "arrives at B"
        );
    }

    /// A zero-fade accent on the same downbeat as a section does not cut
    /// the section's fade short: the two are one take.
    /// r[verify cues.same-position-is-one-take]
    #[test]
    fn an_accent_at_the_sections_position_does_not_truncate_its_fade() {
        let show = Show::new(&[], &crate::selection::EMPTY_RIG);
        let section = Cue {
            name: "Verse".into(),
            fade_secs: 2.0,
            values: vec![CueValue {
                chan: 1,
                attr: Attribute::Dimmer,
                value: 1.0,
            }],
            at: Some(Bars::bar(5).into()),
            ..Default::default()
        };
        let accent = Cue {
            name: "Downbeat".into(),
            fade_secs: 0.0,
            values: vec![CueValue {
                chan: 2,
                attr: Attribute::Dimmer,
                value: 1.0,
            }],
            at: Some(Bars::bar(5).into()),
            ..Default::default()
        };
        let mut player = CuePlayer::new(vec![section, accent]);
        player.go(&show);
        player.go(&show);
        let out = player.output(&show);
        assert_eq!(out[&(2, Attribute::Dimmer)], 1.0, "the accent snapped");
        assert!(
            out[&(1, Attribute::Dimmer)].abs() < 1e-6,
            "the section has only begun"
        );
        player.tick(1.0);
        let mid = player.output(&show)[&(1, Attribute::Dimmer)];
        assert!(
            (mid - 0.5).abs() < 1e-3,
            "halfway through its two seconds, got {mid}"
        );
        player.tick(1.0);
        assert!((player.output(&show)[&(1, Attribute::Dimmer)] - 1.0).abs() < 1e-6);

        // Seeking there lands both, arrived.
        let mut seeker = CuePlayer::new(player.cues().to_vec());
        seeker.seek(Bars::bar(6), &show);
        let out = seeker.output(&show);
        assert_eq!(out[&(1, Attribute::Dimmer)], 1.0);
        assert_eq!(out[&(2, Attribute::Dimmer)], 1.0);
    }

    /// Freezing is a verb: it hands back the running effect's values at
    /// this instant as direct values, and the effect keeps running.
    /// r[verify effects.stomp.freeze-is-explicit]
    /// r[verify effects.stomp]
    #[test]
    fn freeze_samples_the_running_effect_and_leaves_it_running() {
        let groups = vec![Group {
            name: "Pars".into(),
            chans: vec![1, 2],
        }];
        let show = Show::new(&groups, &crate::selection::EMPTY_RIG);
        let chase = Recipe {
            target: Selection::Group("Pars".into()),
            steps: vec![
                Step::new(vec![RecipeApply::Raw(vec![(Attribute::Dimmer, 0.2)])]),
                Step::new(vec![RecipeApply::Raw(vec![(Attribute::Dimmer, 0.9)])]),
            ],
            timing: Timing {
                speed: Speed::Hz(1.0),
                phase_spread_deg: 360.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut player = CuePlayer::new(vec![Cue {
            name: "Chase".into(),
            recipes: vec![chase.into()],
            ..Default::default()
        }]);
        player.go(&show);
        player.advance_clock(0.1);
        let before = player.output(&show);
        let frozen = player.freeze(&show);
        assert_eq!(frozen.len(), 2);
        for value in &frozen {
            assert_eq!(before[&(value.chan, value.attr.clone())], value.value);
        }
        assert_ne!(
            frozen[0].value, frozen[1].value,
            "a moment of a spread chase"
        );
        assert_eq!(player.output(&show), before, "freezing changed nothing");
        player.advance_clock(0.5);
        assert_ne!(
            player.output(&show)[&(1, Attribute::Dimmer)],
            frozen[0].value,
            "the effect went on running"
        );
    }
}

/// Arrival curves and the finer timing — `docs/spec/cues.md`, plus the
/// player halves of pause, defaults and colour intent.
#[cfg(test)]
mod arrival_tests {
    use super::*;
    use crate::group::Group;
    use crate::preset::{ColorPreset, Palettes, Ref};
    use crate::recipe::RecipeApply;
    use crate::selection::Selection;
    use crate::step::{SpeedMasters, Step, Timing};

    fn cue(name: &str, fade_secs: f32, values: Vec<(ChanId, Attribute, f32)>) -> Cue {
        Cue {
            name: name.to_string(),
            fade_secs,
            values: values
                .into_iter()
                .map(|(chan, attr, value)| CueValue { chan, attr, value })
                .collect(),
            ..Default::default()
        }
    }

    fn red() -> Attribute {
        Attribute::ColorAdd {
            channel: ignition_proto::ColorChannel::Red,
        }
    }

    fn bare() -> Show<'static> {
        Show::new(&[], &crate::selection::EMPTY_RIG)
    }

    fn show_120() -> SpeedMasters {
        SpeedMasters::from([("Song".to_string(), 120.0)])
    }

    fn with_speeds(speeds: &SpeedMasters) -> Show<'_> {
        let mut show = Show::new(&[], &crate::selection::EMPTY_RIG);
        show.speeds = speeds;
        show
    }

    fn get(player: &CuePlayer, show: &Show<'_>, chan: ChanId, attr: Attribute) -> f32 {
        player
            .output(show)
            .get(&(chan, attr))
            .copied()
            .unwrap_or_default()
    }

    // ----- transition curves -----

    /// r[verify cues.transition-curve]
    #[test]
    fn a_class_fades_on_its_own_curve_and_the_others_stay_linear() {
        let show = bare();
        let mut player = CuePlayer::new(vec![Cue {
            timing: CueTiming {
                ease: ClassEase {
                    position: CurveName::SCurve.ease(),
                    ..Default::default()
                },
                ..Default::default()
            },
            ..cue(
                "Swing in",
                4.0,
                vec![(1, Attribute::Dimmer, 1.0), (1, Attribute::Pan, 100.0)],
            )
        }]);
        player.go(&show);
        player.tick(1.0); // a quarter in
        let dimmer = get(&player, &show, 1, Attribute::Dimmer);
        let pan = get(&player, &show, 1, Attribute::Pan);
        assert!(
            (dimmer - 0.25).abs() < 1e-3,
            "intensity should stay linear: {dimmer}"
        );
        assert!(pan < 20.0 && pan > 0.0, "an s-curve leaves softly: {pan}");
        player.tick(1.0); // halfway — every symmetric curve crosses here
        let pan = get(&player, &show, 1, Attribute::Pan);
        assert!((pan - 50.0).abs() < 1e-2, "{pan}");
        player.tick(2.0);
        assert!((get(&player, &show, 1, Attribute::Pan) - 100.0).abs() < 1e-3);
    }

    /// Swing leaves softly and arrives hard: still well short of the
    /// mark three quarters of the way through, and exactly on it at
    /// the end — the pendulum landing, without turning back.
    /// r[verify cues.transition-curve]
    #[test]
    fn swing_arrives_hard_and_settles_exactly() {
        let swing = CurveName::Swing.ease();
        let mut last = 0.0;
        for i in 0..=100 {
            let t = i as f32 / 100.0;
            let v = swing.apply(t);
            assert!(v >= last - 1e-6, "swing turned back at {t}: {v} < {last}");
            last = v;
        }
        assert!(swing.apply(0.75) < 0.65, "{}", swing.apply(0.75));
        assert!((swing.apply(1.0) - 1.0).abs() < 1e-6);
        assert_eq!(CurveName::SCurve.ease(), Ease::Sine);
        assert_eq!(CurveName::Linear.ease(), Ease::Linear);
        // Sugar round-trips as a name in the file.
        assert_eq!(
            serde_json::to_string(&CurveName::Swing).unwrap(),
            "\"swing\""
        );
    }

    // ----- timing overrides -----

    /// r[verify cues.timing-overrides]
    #[test]
    fn a_selection_takes_its_own_timing_while_the_rest_keep_the_class() {
        let speeds = show_120();
        let show = with_speeds(&speeds);
        let mut player = CuePlayer::new(vec![Cue {
            timing_overrides: vec![TimingOverride {
                selection: Selection::Chans(vec![1]),
                timing: CueTiming {
                    dimmer_in: Some(1.0), // one beat: half a second
                    ..Default::default()
                },
            }],
            ..cue(
                "Up",
                4.0,
                vec![(1, Attribute::Dimmer, 1.0), (2, Attribute::Dimmer, 1.0)],
            )
        }]);
        player.go(&show);
        player.tick(0.5);
        let one = get(&player, &show, 1, Attribute::Dimmer);
        let two = get(&player, &show, 2, Attribute::Dimmer);
        assert!(
            (one - 1.0).abs() < 1e-3,
            "the override did not apply: {one}"
        );
        assert!(
            (two - 0.125).abs() < 1e-3,
            "the class timing was lost: {two}"
        );
        let json = serde_json::to_string(&player.cues()[0]).unwrap();
        assert!(json.contains("timing_overrides"), "{json}");
    }

    // ----- break -----

    /// r[verify cues.break]
    #[test]
    fn a_break_stops_an_attribute_tracking_through_a_cue_that_does_not_set_it() {
        let show = bare();
        let mut player = CuePlayer::new(vec![
            cue(
                "Aimed",
                0.0,
                vec![(1, Attribute::Dimmer, 1.0), (1, Attribute::Pan, 90.0)],
            ),
            Cue {
                break_: vec![Attribute::Pan],
                ..cue("Level only", 0.0, vec![(1, Attribute::Dimmer, 0.5)])
            },
            cue("Later", 0.0, vec![(1, Attribute::Dimmer, 0.7)]),
        ]);
        player.go(&show);
        player.go(&show);
        let out = player.output(&show);
        assert!(
            !out.contains_key(&(1, Attribute::Pan)),
            "pan tracked through the break"
        );
        assert!((out[&(1, Attribute::Dimmer)] - 0.5).abs() < 1e-6);
        player.go(&show);
        assert!(!player.output(&show).contains_key(&(1, Attribute::Pan)));
        let json = serde_json::to_string(&player.cues()[1]).unwrap();
        assert!(json.contains(r#""break":["Pan"]"#), "{json}");
    }

    // ----- shield -----

    /// Pre-positioning is off, so the pan read at each cue is the
    /// tracked value and not a move in black toward the restatement.
    fn reveal_list() -> CueList {
        let still = |name: &str, values| Cue {
            mib: Mib {
                mode: MibMode::None,
                ..Default::default()
            },
            ..cue(name, 0.0, values)
        };
        CueList {
            cues: vec![
                still(
                    "Lit",
                    vec![(1, Attribute::Dimmer, 1.0), (1, Attribute::Pan, 0.0)],
                ),
                still("Dark", vec![(1, Attribute::Dimmer, 0.0)]),
                still("Still dark", vec![(2, Attribute::Dimmer, 1.0)]),
                still("Back up", vec![(1, Attribute::Dimmer, 1.0)]),
            ],
            ..Default::default()
        }
    }

    fn pan_after(list: &CueList, index: usize) -> f32 {
        let show = bare();
        let mut player = CuePlayer::from_list(list);
        player.jump_to_end_of(index, &show);
        get(&player, &show, 1, Attribute::Pan)
    }

    /// r[verify cues.shield]
    #[test]
    fn shield_from_zero_stops_the_edit_where_the_intensity_comes_up() {
        let mut list = reveal_list();
        let mut edited = list.cues[0].clone();
        edited.values[1].value = 50.0;
        list.store(0, edited, StoreMode::ShieldFromZero);
        assert!((pan_after(&list, 0) - 50.0).abs() < 1e-6);
        assert!(
            (pan_after(&list, 1) - 50.0).abs() < 1e-6,
            "tracks while dark"
        );
        assert!(
            (pan_after(&list, 2) - 50.0).abs() < 1e-6,
            "tracks while dark"
        );
        assert!(
            (pan_after(&list, 3)).abs() < 1e-6,
            "stops where it comes up"
        );
        assert!(
            list.cues[3]
                .values
                .iter()
                .any(|v| v.attr == Attribute::Pan && v.value == 0.0)
        );
        assert!(!list.cues[1].values.iter().any(|v| v.attr == Attribute::Pan));
    }

    /// r[verify cues.shield]
    #[test]
    fn shield_above_zero_stops_the_edit_at_the_next_lit_cue() {
        let mut list = reveal_list();
        // The fixture stays lit through cue 1 here.
        list.cues[1].values.clear();
        let mut edited = list.cues[0].clone();
        edited.values[1].value = 50.0;
        list.store(0, edited, StoreMode::ShieldAboveZero);
        assert!((pan_after(&list, 0) - 50.0).abs() < 1e-6);
        assert!(
            (pan_after(&list, 1)).abs() < 1e-6,
            "lit at cue 1: the edit stops there"
        );
        // Track leaves it to track.
        let mut list = reveal_list();
        let mut edited = list.cues[0].clone();
        edited.values[1].value = 50.0;
        list.store(0, edited, StoreMode::Track);
        assert!((pan_after(&list, 3) - 50.0).abs() < 1e-6);
        // Cue-only stops at the very next cue, lit or not.
        let mut list = reveal_list();
        let mut edited = list.cues[0].clone();
        edited.values[1].value = 50.0;
        list.store(0, edited, StoreMode::CueOnly);
        assert!((pan_after(&list, 1)).abs() < 1e-6);
    }

    // ----- wrap and restart -----

    fn three() -> CueList {
        CueList {
            cues: vec![
                cue("A", 0.0, vec![(1, Attribute::Dimmer, 0.1)]),
                cue("B", 0.0, vec![(1, Attribute::Dimmer, 0.2)]),
                cue("C", 0.0, vec![(1, Attribute::Dimmer, 0.3)]),
            ],
            wrap: true,
            ..Default::default()
        }
    }

    /// r[verify cues.wrap-and-restart]
    #[test]
    fn a_wrapping_list_goes_from_its_last_cue_to_its_first() {
        let show = bare();
        let mut player = CuePlayer::from_list(&three());
        for _ in 0..3 {
            player.go(&show);
        }
        assert_eq!(player.current_index(), Some(2));
        player.go(&show);
        assert_eq!(player.current_index(), Some(0));
        assert!((get(&player, &show, 1, Attribute::Dimmer) - 0.1).abs() < 1e-6);
        let mut plain = CuePlayer::new(three().cues);
        for _ in 0..4 {
            plain.go(&show);
        }
        assert_eq!(plain.current_index(), Some(2), "wrap is opt-in");
        let json = serde_json::to_string(&three()).unwrap();
        assert!(
            json.contains(r#""wrap":true"#) && !json.contains("restart"),
            "{json}"
        );
    }

    /// r[verify cues.wrap-and-restart]
    #[test]
    fn a_disabled_list_re_enables_where_its_restart_says() {
        let show = bare();
        for (restart, expect, level) in [
            (Restart::First, 0, 0.1),
            (Restart::Current, 1, 0.2),
            (Restart::Next, 2, 0.3),
        ] {
            let mut list = three();
            list.restart = restart;
            let mut player = CuePlayer::from_list(&list);
            player.go(&show);
            player.go(&show);
            player.disable();
            assert!(player.output(&show).is_empty(), "disabled but showing");
            assert!(!player.is_enabled());
            player.enable(&show);
            assert_eq!(player.current_index(), Some(expect), "{restart:?}");
            let got = get(&player, &show, 1, Attribute::Dimmer);
            assert!((got - level).abs() < 1e-6, "{restart:?}: {got}");
        }
        // Next off the last cue wraps when the list does.
        let mut list = three();
        list.restart = Restart::Next;
        let mut player = CuePlayer::from_list(&list);
        for _ in 0..3 {
            player.go(&show);
        }
        player.disable();
        player.enable(&show);
        assert_eq!(player.current_index(), Some(0));
    }

    // ----- move in black -----

    fn mover(mib: Mib) -> Vec<Cue> {
        vec![
            cue(
                "Dark",
                0.0,
                vec![(1, Attribute::Dimmer, 0.0), (1, Attribute::Pan, 0.0)],
            ),
            cue("Other", 0.0, vec![(2, Attribute::Dimmer, 1.0)]),
            Cue {
                mib,
                ..cue(
                    "Aimed",
                    0.0,
                    vec![(1, Attribute::Dimmer, 1.0), (1, Attribute::Pan, 90.0)],
                )
            },
        ]
    }

    fn early() -> Mib {
        Mib {
            mode: MibMode::Early,
            fade_beats: 0.0,
            ..Default::default()
        }
    }

    /// The look-ahead reaches past an intervening cue, and moves in the
    /// best-rated dark window: with the second cue preferred, the move
    /// waits for it.
    /// r[verify cues.mib.preference]
    #[test]
    fn the_move_happens_in_the_best_rated_dark_window() {
        let speeds = show_120();
        let show = with_speeds(&speeds);
        let mut player = CuePlayer::new(mover(early()));
        player.go(&show);
        assert!(
            (get(&player, &show, 1, Attribute::Pan) - 90.0).abs() < 1e-3,
            "flat preference: moves at once"
        );

        let mut cues = mover(early());
        cues[1].mib.preference = 90;
        let mut player = CuePlayer::new(cues);
        player.go(&show);
        assert!(
            get(&player, &show, 1, Attribute::Pan).abs() < 1e-6,
            "moved in the worse window"
        );
        player.go(&show);
        assert!((get(&player, &show, 1, Attribute::Pan) - 90.0).abs() < 1e-3);
        assert!(get(&player, &show, 1, Attribute::Dimmer).abs() < 1e-6);
        player.go(&show);
        assert!((get(&player, &show, 1, Attribute::Dimmer) - 1.0).abs() < 1e-6);
    }

    /// r[verify cues.mib.preference]
    #[test]
    fn upon_go_moves_with_the_take_after_the_fixture_went_dark() {
        let speeds = show_120();
        let show = with_speeds(&speeds);
        let mut player = CuePlayer::new(mover(Mib {
            mode: MibMode::UponGo,
            ..early()
        }));
        player.go(&show); // goes dark here: no move yet
        assert!(get(&player, &show, 1, Attribute::Pan).abs() < 1e-6);
        player.go(&show); // the next take moves it
        assert!((get(&player, &show, 1, Attribute::Pan) - 90.0).abs() < 1e-3);
    }

    /// r[verify cues.mib.hold]
    #[test]
    fn a_holding_cue_never_pre_positions_its_dark_fixtures() {
        let speeds = show_120();
        let show = with_speeds(&speeds);
        let mut cues = mover(early());
        cues[0].mib.hold = true;
        let mut player = CuePlayer::new(cues);
        player.go(&show);
        assert!(
            get(&player, &show, 1, Attribute::Pan).abs() < 1e-6,
            "moved under hold"
        );
        player.go(&show); // the next window does not hold
        assert!((get(&player, &show, 1, Attribute::Pan) - 90.0).abs() < 1e-3);
        let json = serde_json::to_string(&player.cues()[0]).unwrap();
        assert!(json.contains(r#""hold":true"#), "{json}");
    }

    /// A colour chase on channel 1, once a second.
    fn red_chase() -> Recipe {
        Recipe {
            target: Selection::Chans(vec![1]),
            steps: vec![
                Step::new(vec![RecipeApply::Raw(vec![(red(), 0.0)])]),
                Step::new(vec![RecipeApply::Raw(vec![(red(), 1.0)])]),
            ],
            timing: Timing {
                speed: crate::step::Speed::Hz(1.0),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    /// r[verify cues.mib.multistep]
    #[test]
    fn a_paused_multistep_holds_the_effect_while_the_fixture_is_dark() {
        let speeds = show_120();
        let show = with_speeds(&speeds);
        for (multistep, expect_still) in [(MultiStep::Running, false), (MultiStep::Paused, true)] {
            let mut cues = mover(Mib {
                multistep,
                ..early()
            });
            cues.remove(1);
            cues[0].recipes = vec![red_chase().into()];
            let mut player = CuePlayer::new(cues);
            player.go(&show);
            player.tick(0.2); // step 0
            let a = get(&player, &show, 1, red());
            player.tick(0.5); // 0.7 s: step 1, unless held
            let b = get(&player, &show, 1, red());
            assert_eq!(
                (a - b).abs() < 1e-6,
                expect_still,
                "{multistep:?}: {a} then {b}"
            );
            // Coming up, the effect runs again either way.
            player.go(&show);
            player.tick(0.3); // 1.0 s: step 0
            let c = get(&player, &show, 1, red());
            player.tick(0.5); // 1.5 s: step 1
            let d = get(&player, &show, 1, red());
            assert!((c - d).abs() > 1e-6, "{multistep:?}: still frozen once lit");
        }
    }

    // ----- cook merge -----

    /// r[verify cues.cook-merge]
    #[test]
    fn cooking_merges_or_overwrites_the_direct_values() {
        let groups = vec![Group {
            name: "Pars".into(),
            chans: vec![1, 2],
        }];
        let show = Show::new(&groups, &crate::selection::EMPTY_RIG);
        let template = Cue {
            recipes: vec![
                Recipe::new(Selection::Group("Pars".into()), RecipeApply::Dimmer(0.5)).into(),
            ],
            ..cue(
                "Look",
                0.0,
                vec![(1, Attribute::Pan, 30.0), (1, Attribute::Dimmer, 0.9)],
            )
        };
        let mut player = CuePlayer::new(vec![template.clone()]);
        player.go(&show);
        let mut merged = template.clone();
        player.freeze_into(&mut merged, &show, CookMode::Merge);
        assert!(merged.recipes.is_empty());
        let find = |c: &Cue, chan, attr: Attribute| {
            c.values
                .iter()
                .find(|v| v.chan == chan && v.attr == attr)
                .map(|v| v.value)
        };
        assert_eq!(
            find(&merged, 1, Attribute::Pan),
            Some(30.0),
            "merge lost a direct value"
        );
        assert_eq!(
            find(&merged, 1, Attribute::Dimmer),
            Some(0.9),
            "the direct value still wins"
        );
        assert_eq!(find(&merged, 2, Attribute::Dimmer), Some(0.5));
        let mut over = template.clone();
        player.freeze_into(&mut over, &show, CookMode::Overwrite);
        assert_eq!(find(&over, 1, Attribute::Pan), None);
        assert_eq!(find(&over, 2, Attribute::Dimmer), Some(0.5));
        assert_eq!(over.values.len(), 2);
    }

    // ----- pause, resume, go back, load -----

    /// r[verify playback.temp-and-pause]
    #[test]
    fn pause_holds_the_fades_and_the_follow_clock_but_not_the_show_clock() {
        let speeds = show_120();
        let show = with_speeds(&speeds);
        let mut player = CuePlayer::new(vec![
            cue("Up", 2.0, vec![(1, Attribute::Dimmer, 1.0)]),
            Cue {
                trig: Trig::Follow { beats: 2.0 }, // one second
                ..cue("Follow", 2.0, vec![(2, Attribute::Dimmer, 1.0)])
            },
        ]);
        player.go(&show);
        player.tick_with(0.5, &show);
        let clock = player.clock();
        player.pause();
        player.tick_with(2.0, &show);
        assert!(
            (get(&player, &show, 1, Attribute::Dimmer) - 0.25).abs() < 1e-3,
            "the fade moved"
        );
        assert_eq!(
            player.current_index(),
            Some(0),
            "the follow fired while paused"
        );
        assert!(
            (player.clock() - clock - 2.0).abs() < 1e-6,
            "the show clock stopped"
        );
        player.resume();
        player.tick_with(0.5, &show);
        assert!((get(&player, &show, 1, Attribute::Dimmer) - 0.5).abs() < 1e-3);
        assert_eq!(player.current_index(), Some(1), "the follow was due at 1 s");
    }

    /// r[verify playback.temp-and-pause]
    #[test]
    fn go_back_crossfades_into_the_previous_cue_with_its_own_fade() {
        let show = bare();
        let mut player = CuePlayer::new(vec![
            cue(
                "Bright",
                2.0,
                vec![(1, Attribute::Dimmer, 1.0), (2, Attribute::Dimmer, 0.4)],
            ),
            cue("Dim", 0.0, vec![(1, Attribute::Dimmer, 0.2)]),
        ]);
        player.go(&show);
        player.go(&show);
        player.tick(5.0);
        assert!((get(&player, &show, 1, Attribute::Dimmer) - 0.2).abs() < 1e-6);
        player.go_back(&show);
        assert_eq!(player.current_index(), Some(0));
        player.tick(1.0); // halfway through "Bright"'s two seconds, from 0.2
        assert!((get(&player, &show, 1, Attribute::Dimmer) - 0.6).abs() < 1e-3);
        assert!(
            (get(&player, &show, 2, Attribute::Dimmer) - 0.4).abs() < 1e-3,
            "tracked state was lost"
        );
        player.go_back(&show);
        assert_eq!(player.current_index(), None);
        assert!(player.output(&show).is_empty());
    }

    /// r[verify playback.temp-and-pause]
    #[test]
    fn load_arms_a_cue_that_the_next_go_takes_with_its_tracked_state() {
        let show = bare();
        let mut player = CuePlayer::new(vec![
            cue("A", 0.0, vec![(1, Attribute::Dimmer, 0.1)]),
            cue("B", 0.0, vec![(2, Attribute::Dimmer, 0.2)]),
            cue("C", 0.0, vec![(3, Attribute::Dimmer, 0.3)]),
        ]);
        player.load(2);
        assert_eq!(player.loaded(), Some(2));
        assert!(player.output(&show).is_empty(), "load must not take");
        player.go(&show);
        assert_eq!(player.current_index(), Some(2));
        assert_eq!(player.loaded(), None);
        let out = player.output(&show);
        assert!(
            (out[&(1, Attribute::Dimmer)] - 0.1).abs() < 1e-6,
            "A did not track in"
        );
        assert!((out[&(2, Attribute::Dimmer)] - 0.2).abs() < 1e-6);
        assert!((out[&(3, Attribute::Dimmer)] - 0.3).abs() < 1e-6);
        player.go(&show);
        assert_eq!(player.current_index(), Some(2), "past the end without wrap");
    }

    // ----- intent to output -----

    fn warm_and_cool() -> Palettes {
        Palettes {
            colors: vec![
                ColorPreset::from_intent(
                    "Warm",
                    Intent::Cct {
                        kelvin: 3200.0,
                        tint: 0.0,
                    },
                )
                .unwrap(),
                ColorPreset::from_intent(
                    "Cool",
                    Intent::Cct {
                        kelvin: 6500.0,
                        tint: 0.0,
                    },
                )
                .unwrap(),
            ],
            ..Default::default()
        }
    }

    fn colour_cue(name: &str, fade_secs: f32, preset: &str) -> Cue {
        Cue {
            name: name.into(),
            fade_secs,
            recipes: vec![
                Recipe::new(
                    Selection::Chans(vec![1]),
                    RecipeApply::Color(Ref::Named(preset.into())),
                )
                .into(),
            ],
            ..Default::default()
        }
    }

    /// r[verify color.intent-to-output]
    #[test]
    fn the_winning_colour_source_hands_its_intent_to_output() {
        let palettes = warm_and_cool();
        let mut show = bare();
        show.palettes = &palettes;
        let mut player = CuePlayer::new(vec![
            colour_cue("Warm", 0.0, "Warm"),
            colour_cue("Cool", 2.0, "Cool"),
            cue("Direct", 0.0, vec![(1, red(), 1.0)]),
        ]);
        assert!(player.output_intents(&show).is_empty());
        player.go(&show);
        assert_eq!(
            player.output_intents(&show).get(&1),
            Some(&Intent::Cct {
                kelvin: 3200.0,
                tint: 0.0
            }),
            "settled, the preset's own intent"
        );
        player.go(&show);
        player.tick(1.0);
        let warm = Intent::Cct {
            kelvin: 3200.0,
            tint: 0.0,
        }
        .xyy()
        .unwrap();
        let cool = Intent::Cct {
            kelvin: 6500.0,
            tint: 0.0,
        }
        .xyy()
        .unwrap();
        match player.output_intents(&show).get(&1) {
            Some(Intent::Xy { x, .. }) => {
                let mid = (warm.x + cool.x) / 2.0;
                assert!((x - mid).abs() < 1e-3, "mid-fade in xyY: {x} vs {mid}");
            }
            other => panic!("expected an xyY crossfade, got {other:?}"),
        }
        player.tick(1.0);
        assert!(matches!(
            player.output_intents(&show).get(&1),
            Some(Intent::Cct { kelvin, .. }) if (*kelvin - 6500.0).abs() < 1e-3
        ));
        player.go(&show);
        assert!(
            !player.output_intents(&show).contains_key(&1),
            "a direct triple carries no intent"
        );
    }

    // ----- defaults floor -----

    /// r[verify playback.defaults]
    #[test]
    fn a_released_or_unset_key_reads_its_default_and_cue_zero_is_the_defaults() {
        let show = bare();
        let defaults = HashMap::from([((1, Attribute::Zoom), 0.5), ((1, Attribute::Dimmer), 0.0)]);
        let mut player = CuePlayer::new(vec![
            cue("Wide", 2.0, vec![(1, Attribute::Zoom, 1.0)]),
            Cue {
                release: vec![Attribute::Zoom],
                ..cue("Let go", 2.0, vec![(1, Attribute::Dimmer, 1.0)])
            },
        ]);
        let zero = player.output_with_defaults(&show, &defaults);
        assert_eq!(
            zero.get(&(1, Attribute::Zoom)),
            Some(&0.5),
            "cue zero is the defaults"
        );
        assert!(player.output(&show).is_empty(), "plain output is untouched");
        player.go(&show);
        player.tick(1.0);
        let z = player.output_with_defaults(&show, &defaults)[&(1, Attribute::Zoom)];
        assert!(
            (z - 0.75).abs() < 1e-3,
            "rises from the default, not from zero: {z}"
        );
        player.tick(1.0);
        player.go(&show);
        player.tick(1.0);
        let z = player.output_with_defaults(&show, &defaults)[&(1, Attribute::Zoom)];
        assert!((z - 0.75).abs() < 1e-3, "falls toward the default: {z}");
        player.tick(1.0);
        let out = player.output_with_defaults(&show, &defaults);
        assert_eq!(
            out.get(&(1, Attribute::Zoom)),
            Some(&0.5),
            "lands on the default"
        );
        assert!(!player.output(&show).contains_key(&(1, Attribute::Zoom)));
    }

    // ----- file shape -----

    #[test]
    fn the_new_fields_default_and_stay_out_of_the_file() {
        let cue = Cue::default();
        let json = serde_json::to_string(&cue).unwrap();
        for absent in [
            "timing_overrides",
            "break",
            "ease",
            "hold",
            "multistep",
            "preference",
        ] {
            assert!(!json.contains(absent), "{absent} in {json}");
        }
        let back: Cue = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cue);
        let mib: Mib = serde_json::from_str(r#"{"mode":"Early"}"#).unwrap();
        assert_eq!(mib.preference, 50);
        assert_eq!(mib.multistep, MultiStep::Running);
        assert!(!mib.hold);
        let list: CueList = serde_json::from_str(r#"{"cues":[]}"#).unwrap();
        assert!(!list.wrap);
        assert_eq!(list.restart, Restart::First);
        let timing: CueTiming = serde_json::from_str(r#"{"ease":{"position":"Sine"}}"#).unwrap();
        assert_eq!(timing.ease.position, Ease::Sine);
        assert_eq!(timing.ease.intensity, Ease::Linear);
    }
}
