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
use crate::focus::{pan_tilt_deg_along, pan_tilt_deg_to_point, v_add, v_scale};
use crate::group::Group;
use crate::preset::{ColorPreset, ColorSplit, Palettes, Ref};
use crate::programmer::AttrFilter;
use crate::selection::{Rig, Selection, resolve};
use crate::step::{Play, Speed, SpeedMasters, Step, Timing, Waveform, locate};
use ignition_proto::{Attribute, ChanId, ColorChannel, Vec3};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

/// What to apply to the target.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum RecipeApply {
    Dimmer(f32),
    /// A pooled colour by name (`"House Blue"`), or one written inline.
    /// A name the venue's palette does not carry resolves to nothing and
    /// the recipe is skipped, same tolerance as an unknown group.
    // r[impl color.recall-by-reference]
    Color(Ref<ColorPreset>),
    /// Several colours and a rule for laying them across the selection —
    /// a rainbow, a two-tone split, a warm-to-cool wash — as *one* thing
    /// a cue can name, instead of five hand-written single-colour
    /// recipes that have to be kept in step. The rule is declared rather
    /// than inferred from the count, so adding a colour never silently
    /// changes the look. A name the palette lacks fails the whole apply,
    /// not just that entry: a gradient with a hole in it is a worse
    /// surprise than no gradient.
    // r[impl color.multi]
    // r[impl color.multi.distribution]
    Colors {
        colors: Vec<Ref<ColorPreset>>,
        distribute: Distribute,
    },
    /// A palette split by name (`"Fire"`), or one written inline — the
    /// same colours-plus-distribution as `Colors`, but recallable as one
    /// object, so editing the split in the palette changes every cue
    /// that names it. Expands exactly as `Colors` does: per unit after
    /// Tricks, in the selection's order.
    // r[impl color.multi]
    // r[impl color.recall-by-reference] - a cue stores the split's name
    // r[impl color.embedding] - the palette resolves nested splits
    Split(Ref<ColorSplit>),
    /// A real XYZ room location — resolved per-fixture via each target
    /// fixture's actual `Placement` (see `focus.rs`), not a single shared
    /// pan/tilt value. Fixtures with no known `Placement` (an unpatched or
    /// unrecognized channel) are silently skipped, same tolerance as the
    /// rest of this module.
    // r[impl focus.point]
    // r[impl focus.two-kinds] - point
    FocusPoint(Ref<ignition_proto::Vec3>),
    /// A shared world-space *direction* rather than a shared point, so
    /// every fixture in the group ends up beam-parallel with the others
    /// instead of converging. Not expressible as a `FocusPoint` at any
    /// finite distance. Fixtures with no known `Placement` are skipped,
    /// same as `FocusPoint`.
    // r[impl focus.orientation]
    // r[impl focus.two-kinds] - orientation
    FocusDirection(ignition_proto::Vec3),
    /// A focus *pattern*: the aim point walks from `from` to `to` across
    /// the selection, so the first unit looks at one end and the last at
    /// the other. Interpolating the *point* rather than the pan/tilt is
    /// what keeps it a room fact — each fixture still resolves its own
    /// angles through its `Placement`, exactly as `FocusPoint` does, so a
    /// re-hung head lands on the same line. The direction of the fan is
    /// the selection's order; the apply carries none of its own.
    // r[impl focus.pattern]
    // r[impl focus.pattern.fan]
    // r[impl focus.pattern.order-is-the-selection]
    FocusFan {
        from: Ref<ignition_proto::Vec3>,
        to: Ref<ignition_proto::Vec3>,
    },
    /// Up to five (or any number of) aims placed evenly along the
    /// selection, every unit's point interpolated between the two
    /// nearest — MA3's MAgic presets, and the general form of
    /// `FocusFan`. Five hand-set positions across a truss land on twelve
    /// or twenty movers with no per-fixture values. Each unit's point is
    /// then solved through its own `Placement`, as `FocusPoint` does.
    // r[impl focus.magic]
    // r[impl tricks.keyframes] - the focus form
    // r[impl focus.pattern.order-is-the-selection]
    FocusKeyframes(Vec<Ref<ignition_proto::Vec3>>),
    /// A **relative offset in metres** to the point the fixture is aimed
    /// at — added to the point *before* it is solved to pan/tilt, so the
    /// same metre is a different angle for every head and the same shape
    /// in the room at every venue.
    ///
    /// Two ways it meets a point. In the same step as a `FocusPoint`,
    /// `FocusFan` or `FocusKeyframes` it is folded in here and the step
    /// emits absolute pan/tilt — this is how the metre orbits in the
    /// library draw a circle round `Drums`. Alone, the step cannot know
    /// where the cascade has the fixture aimed, so it is handed to the
    /// player as a [`FocusDeltaEmit`] to apply against its own aim; a
    /// fixture with no point aim ignores it. Several in one step sum.
    // r[impl focus.delta]
    // r[impl focus.orbit-in-metres]
    FocusDelta(ignition_proto::Vec3),
    /// A **splay**: every fixture leans outward from `origin` (a named
    /// focus marker, or the room's zero) along `axis`, by
    /// `degrees_per_metre` for each metre of its own real offset. An
    /// orientation per fixture derived from where it hangs, so the rig
    /// opens away from centre at any venue without re-authoring, and a
    /// re-hung head takes the angle its new place implies.
    // r[impl focus.pattern]
    // r[impl focus.pattern.parallel-out]
    // r[impl focus.resolve-at-output]
    FocusSplay {
        axis: crate::selection::Axis,
        degrees_per_metre: f32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        origin: Option<String>,
    },
    /// An explicit aim **per fixture** — each mover its own point,
    /// authored deliberately. A fixture with no entry falls back in the
    /// order colour scoping does: its own value, then its model's, then
    /// the shared `default`; with none of those it is left alone.
    // r[impl focus.pattern]
    // r[impl focus.pattern.per-fixture]
    FocusPerFixture {
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        aims: BTreeMap<ChanId, ignition_proto::Vec3>,
        /// Per `manufacturer model`, for a fixture with no own entry.
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        models: BTreeMap<String, ignition_proto::Vec3>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        default: Option<ignition_proto::Vec3>,
    },
    /// A point that constrains only some axes. An axis left `None` is
    /// taken from the fixture's *own* position — "this height and this
    /// place across the stage, at whatever depth you are" lights a line
    /// rather than a point. Never defaulted to zero. Optionally
    /// relative to a named origin marker.
    // r[impl focus.partial-axes]
    // r[impl focus.relative-origin]
    FocusAxes {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        x: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        y: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        z: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        origin: Option<String>,
    },
    /// A point expressed as metres **from a named origin** — a focus
    /// marker (`Vocal`, `Drums`) resolved through the show, so moving
    /// the marker moves every aim written against it.
    // r[impl focus.relative-origin]
    // r[impl focus.marker-moving] - resolved through `Show::focus`, so an override moves it
    FocusRelative {
        origin: String,
        offset: ignition_proto::Vec3,
    },
    /// A per-unit random generator — fire, candle, an electrical fault,
    /// a sparkle whose density is a fader. See [`Random`]: a pure
    /// function of (seed, unit, time), so a seek and a replay agree.
    /// Relative (`Delta` semantics) unless `absolute`.
    // r[impl effects.random]
    Random(Random),
    /// A sound band level as a value: `attr` sits at
    /// `low + level × (high − low)`, where `level` is the show's
    /// smoothed level of `band` (`0..=1`). Silence is `low`; full is
    /// `high`. `relative` makes it a `Delta` on top of the look rather
    /// than a replacement — the bass *lifting* the blinders over
    /// whatever the cue set. The no-chart busking case. The smoothing
    /// (sound fade) is the host's, handed in as [`SoundLevels`], so the
    /// same recipe on the same levels is the same value everywhere.
    // r[impl playback.sound-as-value]
    Sound {
        band: Band,
        attr: Attribute,
        low: f32,
        high: f32,
        #[serde(default)]
        relative: bool,
    },
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
    // r[impl effects.modulates-with-delta]
    // r[impl recipes.relative-leaves-colour-alone]
    Delta(Vec<(Attribute, f32)>),
    /// A canvas picture driving an attribute across the selection's
    /// grid: the content's colour at each unit's `(u, v)` position — X
    /// and Y fractions of the unit grid — mapped by `channel` onto the
    /// attribute it names. A wipe that crosses the TVs crosses a row of
    /// movers' tilt the same way, because both are addressed by grid
    /// position rather than by index.
    // r[impl canvas.bitmap-channels]
    // r[impl canvas.on-the-stack]
    Canvas {
        recipe: crate::canvas::CanvasRecipe,
        channel: crate::canvas::BitmapChannel,
    },
}

pub use crate::preset::Distribute;

/// A random level generator, per unit, as a pure function of time.
///
/// Every unit gets its own phase and its own rate (from `seed` and its
/// index in the selection), and re-rolls a target level in
/// `[low, high]` — widened by up to `level_var` either way — once per
/// *period*, where a period is one cycle of the recipe's `Timing`. Each
/// period ramps from the level it left over `attack`, holds, and falls
/// back toward `low` over `decay` (both fractions of the period, so a
/// candle is a long attack and a spark is none). `speed_var` spreads the
/// per-unit rate about the recipe's, so no two units ever agree on the
/// beat.
///
/// This is what a shuffled step table is not: there every unit runs the
/// same shape at a different phase, and a room full of candles all
/// guttering identically reads as a machine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
// r[impl effects.random]
// r[impl effects.sync.pure-function] - (seed, unit, time) and nothing else
pub struct Random {
    pub attr: Attribute,
    pub low: f32,
    pub high: f32,
    /// How far outside `[low, high]` a rolled level may land, either way.
    #[serde(default)]
    pub level_var: f32,
    /// ±fraction of the recipe's rate each unit's own rate may differ by.
    #[serde(default)]
    pub speed_var: f32,
    /// Fraction of a period spent rising to the new level.
    #[serde(default)]
    pub attack: f32,
    /// Fraction of a period spent falling back to `low` at the end.
    #[serde(default)]
    pub decay: f32,
    #[serde(default)]
    pub seed: u32,
    /// Replace what is underneath rather than add to it.
    #[serde(default)]
    pub absolute: bool,
    /// How much of a period each unit's phase may be offset by, `0..=1`
    /// — MA3's *phase variance*. `1.0` (the default, and what every
    /// file before this field got) scatters the units across the whole
    /// period; `0.0` runs them in lock-step, every unit changing level
    /// on the same frame with its own rolled level.
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub phase_var: f32,
    /// The proportion of each period the unit is *on* — at its rolled
    /// level — before it sits at `low` for the rest: MA3's *ratio*.
    /// `1.0` (the default) never drops; `0.25` is a sparkle. Attack and
    /// decay are fractions of the on-portion, so a decay still lands on
    /// `low` at the moment the unit goes off.
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub ratio: f32,
    /// ± spread on `ratio` per unit, so one candle burns longer than
    /// the next.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub ratio_var: f32,
    /// Each unit starts its level sequence at a different, seeded point
    /// — MA3's *random start* — so two units at the same phase still
    /// roll different levels in the same order the file was written.
    /// Off, every unit's sequence begins at period zero.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub random_start: bool,
    /// Let the sound's band level drive `high`: the range becomes
    /// `[low, low + level × (high − low)]`, so a sparkle's brightness
    /// breathes with the music and sits at `low` in silence. The host
    /// smooths the level; see [`SoundLevels`].
    // r[impl playback.sound-as-value] - a generator's range
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub high_from_band: Option<Band>,
}

fn one() -> f32 {
    1.0
}

fn is_one(v: &f32) -> bool {
    *v == 1.0
}

fn is_zero(v: &f32) -> bool {
    *v == 0.0
}

impl Default for Random {
    /// A full-range dimmer sparkle with nothing varied: the fields a
    /// file may omit at the values omitting them gives.
    fn default() -> Self {
        Self {
            attr: Attribute::Dimmer,
            low: 0.0,
            high: 1.0,
            level_var: 0.0,
            speed_var: 0.0,
            attack: 0.0,
            decay: 0.0,
            seed: 0,
            absolute: false,
            phase_var: 1.0,
            ratio: 1.0,
            ratio_var: 0.0,
            random_start: false,
            high_from_band: None,
        }
    }
}

impl Random {
    /// A uniform in `[0, 1)` from the seed, the unit and a period
    /// counter. A hand-rolled mix (splitmix64's finaliser) for the same
    /// reason `Trick::Shuffle` has one: it must give the same answer in
    /// ten years.
    fn roll(&self, unit: usize, k: i64, salt: u64) -> f32 {
        let mut z = (u64::from(self.seed) << 32)
            ^ (unit as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
            ^ (k as u64).wrapping_mul(0xBF58_476D_1CE4_E5B9)
            ^ salt.wrapping_mul(0x94D0_49BB_1331_11EB);
        z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        (z >> 40) as f32 / (1u64 << 24) as f32
    }

    /// The level rolled for period `k` of `unit`.
    fn level(&self, unit: usize, k: i64) -> f32 {
        let base = self.low + (self.high - self.low) * self.roll(unit, k, 1);
        base + (self.roll(unit, k, 2) * 2.0 - 1.0) * self.level_var
    }

    /// This generator with its range breathing on the sound: `high`
    /// pulled toward `low` by the band's level. Itself when no band is
    /// named.
    // r[impl playback.sound-as-value] - a generator's range
    pub fn heard(&self, sound: &SoundLevels) -> std::borrow::Cow<'_, Random> {
        use std::borrow::Cow;
        match self.high_from_band {
            None => Cow::Borrowed(self),
            Some(band) => Cow::Owned(Random {
                high: self.low + (self.high - self.low) * sound.level(band),
                high_from_band: None,
                ..self.clone()
            }),
        }
    }

    /// The value for `unit` when the recipe's clock reads `cycles`.
    ///
    /// Still a pure function of (seed, unit, time): phase variance,
    /// ratio variance and random start are all rolled from the seed
    /// and the unit, never from a clock or a counter.
    // r[impl effects.random]
    // r[impl effects.sync.pure-function]
    pub fn at(&self, unit: usize, cycles: f32) -> f32 {
        let rate = 1.0 + (self.roll(unit, 0, 3) * 2.0 - 1.0) * self.speed_var;
        let phase = self.roll(unit, 0, 4) * self.phase_var.clamp(0.0, 1.0);
        // A seeded whole-period offset so this unit reads a different
        // stretch of its level sequence.
        let start = if self.random_start {
            (self.roll(unit, 0, 6) * 4096.0).floor()
        } else {
            0.0
        };
        let local = cycles * rate.max(0.01) + phase + start;
        let k = local.floor();
        let frac = local - k;
        let k = k as i64;
        // The on-portion of this period for this unit; outside it the
        // unit sits at `low`.
        let ratio =
            (self.ratio + (self.roll(unit, 0, 5) * 2.0 - 1.0) * self.ratio_var).clamp(0.0, 1.0);
        if ratio <= 0.0 || frac >= ratio {
            return self.low;
        }
        let frac = frac / ratio;
        let target = self.level(unit, k);
        let attack = self.attack.clamp(0.0, 1.0);
        let decay = self.decay.clamp(0.0, 1.0 - attack);
        // Where the last period left off: at `low` if it decayed or
        // switched off, else on its own level.
        let prev = if decay > 0.0 || ratio < 1.0 {
            self.low
        } else {
            self.level(unit, k - 1)
        };
        if frac < attack {
            prev + (target - prev) * (frac / attack)
        } else if frac >= 1.0 - decay {
            target + (self.low - target) * ((frac - (1.0 - decay)) / decay)
        } else {
            target
        }
    }
}

/// Where one unit sits in the selection after Tricks: `index` of
/// `count`. Colour distribution and focus fans are a function of this,
/// which is why it travels alongside the channel rather than being
/// recomputed from the rig.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Slot {
    pub index: usize,
    pub count: usize,
}

impl Slot {
    /// The lone slot — for a value that does not vary across the selection.
    pub const ONLY: Slot = Slot { index: 0, count: 1 };

    /// 0 on the first unit, 1 on the last, evenly spaced between; 0 for a
    /// selection of one, so a single fixture takes the *start* rather
    /// than dividing by zero.
    // r[impl tricks.spread]
    fn fraction(self) -> f32 {
        if self.count > 1 {
            self.index as f32 / (self.count - 1) as f32
        } else {
            0.0
        }
    }
}

/// A parametric template: who, what, and — with more than one step —
/// when.
///
/// One step is a static look. Two or more is a phaser. That is the whole
/// distinction; there is no separate effect type. See
/// `docs/domain/cue-building-architecture.md`, Decision 3.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(from = "RecipeWire", into = "RecipeWire")]
// r[impl recipes.template]
// r[impl recipes.steps-are-the-switch]
// r[impl recipes.selection-owns-order] - no direction field of its own
pub struct Recipe {
    pub target: Selection,
    pub steps: Vec<Step>,
    /// What this piece of the cue is called, for the list to label its
    /// sub-row with. A cue's recipes are its parts, and a part with a
    /// name — "movers swing in" beside "house to half" — is the
    /// difference between a cue somebody can read and one opaque row.
    // r[impl cues.recipe.name]
    // r[impl cues.parts-are-recipes]
    pub name: Option<String>,
    /// Why this piece is the way it is. Never affects output.
    // r[impl cues.recipe.name]
    pub note: Option<String>,
    /// This piece's **own** fade, delay and ease per class, overriding
    /// the cue's for every key it covers. Distinct from `timing`, which
    /// is the effect's rate; this is the arrival. It is the whole
    /// reason a console needs cue parts — a cue-wide class fade cannot
    /// say "*these* movers over five seconds, everything else over
    /// one", and a per-piece one can.
    // r[impl cues.recipe.timing]
    pub cue_timing: Option<crate::cue::CueTiming>,
    // r[impl recipes.timing-in-musical-terms] - rate against a named master via Speed::Master
    pub timing: Timing,
    // r[impl tricks.on-the-recipe]
    // r[impl recipes.tricks] - inline form; a shared Tricks reference is not built
    /// How the target is cut before the steps meet it.
    ///
    /// Inline on the recipe rather than a separate stage, because that
    /// is what a Trick *is*: grandMA3 carries them as columns on the
    /// recipe line, and it is why one recipe there covers looks that
    /// need a dozen here. `Block(2)` makes the phase spread land per
    /// pair; `Group(2)` makes it land on odds and evens.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tricks: Vec<crate::tricks::Trick>,
    /// Sum with the other sustained relative recipes on an attribute
    /// instead of replacing them — a slow tilt wave under a fast shiver.
    /// Off by default so nothing already authored changes.
    // r[impl effects.relative-stack]
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub stack: bool,
    /// Off means the line contributes nothing but stays in the file, so
    /// an A/B keeps the thing being compared against. Defaults to on
    /// and is only written when off.
    // r[impl recipes.enabled]
    pub enabled: bool,
    /// A shared, named Tricks chain from the profile's `tricks` pool,
    /// applied *before* the inline `tricks`. One edit to the pool is a
    /// rig-wide layout change; the inline chain is how one recipe
    /// deviates from it.
    // r[impl tricks.shared-or-inline] - the reference half
    // r[impl recipes.tricks] - by reference
    pub tricks_ref: Option<String>,
    /// How far this recipe's relative values and swings go — the
    /// `depth` effect parameter, multiplied into the show's size at
    /// expansion for this recipe alone. `1.0` is as authored. Set by
    /// `apply_params` on a resolved copy; a library recipe is never
    /// written with anything else.
    // r[impl profile.effect-parameters] - depth, on the resolved copy
    // r[impl playback.effect-parameters]
    pub depth: f32,
    /// Which attribute families this recipe may touch. Everything by
    /// default; a `Named` reference with a `filter` narrows the
    /// resolved copy so a rainbow on the bars owns colour while a strip
    /// chase owns intensity. Applied at expansion: an emit outside the
    /// filter is dropped and withdraws nothing.
    // r[impl profile.attribute-filter] - on a cue's reference
    // r[impl playback.attribute-filter]
    pub filter: AttrFilter,
}

impl Default for Recipe {
    fn default() -> Self {
        Self {
            target: Selection::Chans(Vec::new()),
            steps: Vec::new(),
            name: None,
            note: None,
            cue_timing: None,
            timing: Timing::default(),
            tricks: Vec::new(),
            stack: false,
            enabled: true,
            tricks_ref: None,
            depth: 1.0,
            filter: AttrFilter::ALL,
        }
    }
}

impl Recipe {
    /// The common case: one thing applied to one selection, no timing.
    pub fn new(target: Selection, apply: RecipeApply) -> Self {
        Self {
            target,
            steps: vec![Step::new(vec![apply])],
            ..Default::default()
        }
    }

    /// The Tricks that actually cut this recipe's target: the shared
    /// chain it names (if the show has it), then its own inline ones.
    // r[impl tricks.shared-or-inline] - shared first, inline deviates after
    // r[impl recipes.tricks]
    pub fn effective_tricks(&self, show: &Show<'_>) -> Vec<crate::tricks::Trick> {
        let mut out: Vec<crate::tricks::Trick> = self
            .tricks_ref
            .as_ref()
            .and_then(|name| show.named_tricks.get(name))
            .cloned()
            .unwrap_or_default();
        out.extend(self.tricks.iter().cloned());
        out
    }

    /// True when this recipe is a phaser rather than a static look.
    // r[impl recipes.steps-are-the-switch]
    pub fn is_phaser(&self) -> bool {
        self.steps.len() > 1
    }
}

/// What a cue's `recipes` list holds: a recipe written in place, a
/// library effect by name, or a bundle of them.
///
/// The library is the one place a chase is defined, so a show that
/// copied its steps would be a show that stopped following the library
/// the day it was saved. `Named` and `Bundle` are resolved through the
/// `Show` when the cue is taken, the same moment a group name is; a name
/// the library lacks resolves to nothing and `unresolved` says so.
///
/// Untagged, and the reference forms tried first: an inline recipe has
/// no `effect` or `bundle` key, and a reference has no `apply`, so
/// every show file written before references existed still loads.
// r[impl effects.library.by-name] - a cue stores the effect's name
// r[impl effects.bundle] - a cue stores the bundle's name
// r[impl files.additive-evolution] - the inline spelling is unchanged
// r[impl profile.looks] - a cue may take a look by name
// r[impl profile.effect-parameters] - a cue's reference carries the same params a fader does
// r[impl profile.attribute-filter] - and the same filter
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RecipeRef {
    /// A library effect, retargeted and retimed for this cue. `bars`
    /// overrides the loop length (one bar is four beats of the measure)
    /// and `tricks` replaces the effect's own. `params` are the effect
    /// parameters a page fader exposes — `depth`, `bars`, `duty` —
    /// applied to the resolved copy exactly as a fader applies them;
    /// `filter` narrows the copy to attribute families; `speed`
    /// replaces the effect's own master — the show-side spelling of
    /// speed routing, since a cue has no family table to route by.
    Named {
        effect: String,
        /// This part's label and note, and its own arrival — the same
        /// three a `Recipe` carries, so a library effect can be a named,
        /// separately-timed part of a cue without being inlined.
        // r[impl cues.recipe.name]
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        note: Option<String>,
        // r[impl cues.recipe.timing]
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cue_timing: Option<crate::cue::CueTiming>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target: Option<Selection>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        bars: Option<f32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tricks: Option<Vec<crate::tricks::Trick>>,
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        params: BTreeMap<String, f32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        filter: Option<AttrFilter>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        speed: Option<Speed>,
    },
    /// Every effect in a library bundle, each retargeted the same way.
    Bundle {
        bundle: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target: Option<Selection>,
    },
    /// A profile look by name — `{"look": "verse bed"}` — every recipe
    /// it carries, resolved through the show's looks. A section cue
    /// opens on the look and states its own recipes on top.
    Look {
        look: String,
    },
    Inline(Recipe),
}

impl From<Recipe> for RecipeRef {
    fn from(recipe: Recipe) -> Self {
        RecipeRef::Inline(recipe)
    }
}

impl RecipeRef {
    /// The recipe, when it is written in place.
    pub fn inline(&self) -> Option<&Recipe> {
        match self {
            RecipeRef::Inline(r) => Some(r),
            _ => None,
        }
    }

    /// A library effect by name, as shipped.
    pub fn named(effect: &str) -> Self {
        RecipeRef::Named {
            effect: effect.to_string(),
            name: None,
            note: None,
            cue_timing: None,
            target: None,
            bars: None,
            tricks: None,
            params: BTreeMap::new(),
            filter: None,
            speed: None,
        }
    }

    /// A profile look by name.
    // r[impl profile.looks]
    pub fn look(name: &str) -> Self {
        RecipeRef::Look {
            look: name.to_string(),
        }
    }

    /// This reference retargeted — a `Named` or `Bundle`; an inline
    /// recipe or a look is returned unchanged.
    pub fn on(self, selection: Selection) -> Self {
        match self {
            RecipeRef::Named {
                target: _,
                effect,
                name,
                note,
                cue_timing,
                bars,
                tricks,
                params,
                filter,
                speed,
            } => RecipeRef::Named {
                effect,
                name,
                note,
                cue_timing,
                target: Some(selection),
                bars,
                tricks,
                params,
                filter,
                speed,
            },
            RecipeRef::Bundle { bundle, .. } => RecipeRef::Bundle {
                bundle,
                target: Some(selection),
            },
            other => other,
        }
    }

    /// A `Named` reference with one effect parameter set — `depth`,
    /// `bars`, `duty`. Anything else is returned unchanged.
    // r[impl profile.effect-parameters]
    pub fn with_param(self, name: &str, value: f32) -> Self {
        match self {
            RecipeRef::Named {
                effect,
                name: part,
                note,
                cue_timing,
                target,
                bars,
                tricks,
                mut params,
                filter,
                speed,
            } => {
                params.insert(name.to_string(), value);
                RecipeRef::Named {
                    effect,
                    name: part,
                    note,
                    cue_timing,
                    target,
                    bars,
                    tricks,
                    params,
                    filter,
                    speed,
                }
            }
            other => other,
        }
    }

    /// A `Named` reference narrowed to attribute families.
    // r[impl profile.attribute-filter]
    pub fn filtered(self, filter: AttrFilter) -> Self {
        match self {
            RecipeRef::Named {
                effect,
                name,
                note,
                cue_timing,
                target,
                bars,
                tricks,
                params,
                speed,
                ..
            } => RecipeRef::Named {
                effect,
                name,
                note,
                cue_timing,
                target,
                bars,
                tricks,
                params,
                filter: Some(filter),
                speed,
            },
            other => other,
        }
    }

    /// This reference as a named part of a cue. A `Named` reference
    /// takes the label; anything else is returned unchanged.
    // r[impl cues.recipe.name]
    pub fn part(self, label: &str) -> Self {
        match self {
            RecipeRef::Named {
                effect,
                note,
                cue_timing,
                target,
                bars,
                tricks,
                params,
                filter,
                speed,
                ..
            } => RecipeRef::Named {
                effect,
                name: Some(label.to_string()),
                note,
                cue_timing,
                target,
                bars,
                tricks,
                params,
                filter,
                speed,
            },
            RecipeRef::Inline(mut r) => {
                r.name = Some(label.to_string());
                RecipeRef::Inline(r)
            }
            other => other,
        }
    }

    /// This reference with its own arrival — the part's fade, delay and
    /// ease, overriding the cue's for everything it covers.
    // r[impl cues.recipe.timing]
    pub fn arriving(self, timing: crate::cue::CueTiming) -> Self {
        match self {
            RecipeRef::Named {
                effect,
                name,
                note,
                target,
                bars,
                tricks,
                params,
                filter,
                speed,
                ..
            } => RecipeRef::Named {
                effect,
                name,
                note,
                cue_timing: Some(timing),
                target,
                bars,
                tricks,
                params,
                filter,
                speed,
            },
            RecipeRef::Inline(mut r) => {
                r.cue_timing = Some(timing);
                RecipeRef::Inline(r)
            }
            other => other,
        }
    }

    /// A `Named` reference at an explicit speed.
    pub fn at(self, speed: Speed) -> Self {
        match self {
            RecipeRef::Named {
                effect,
                name,
                note,
                cue_timing,
                target,
                bars,
                tricks,
                params,
                filter,
                ..
            } => RecipeRef::Named {
                effect,
                name,
                note,
                cue_timing,
                target,
                bars,
                tricks,
                params,
                filter,
                speed: Some(speed),
            },
            other => other,
        }
    }

    /// A `Named` reference with its tricks replaced.
    pub fn tricked(self, tricks: Vec<crate::tricks::Trick>) -> Self {
        match self {
            RecipeRef::Named {
                effect,
                name,
                note,
                cue_timing,
                target,
                bars,
                params,
                filter,
                speed,
                ..
            } => RecipeRef::Named {
                effect,
                name,
                note,
                cue_timing,
                target,
                bars,
                tricks: Some(tricks),
                params,
                filter,
                speed,
            },
            other => other,
        }
    }

    /// The recipes this stands for in `show`: one for an inline or a
    /// named effect, one per member for a bundle, none for a name the
    /// library does not carry.
    // r[impl effects.library.by-name] - resolved through the show at take
    // r[impl effects.bundle] - every member, in the bundle's order
    pub fn resolve(&self, show: &Show<'_>) -> Vec<Recipe> {
        let retarget = |name: &str, target: &Option<Selection>| -> Option<Recipe> {
            let mut r = show.library.get(name)?.clone();
            if let Some(t) = target {
                r.target = t.clone();
            }
            Some(r)
        };
        match self {
            RecipeRef::Inline(recipe) => vec![recipe.clone()],
            RecipeRef::Named {
                effect,
                name,
                note,
                cue_timing,
                target,
                bars,
                tricks,
                params,
                filter,
                speed,
            } => retarget(effect, target)
                .map(|mut r| {
                    if let Some(b) = bars {
                        r.timing.measure = b * 4.0;
                    }
                    if let Some(t) = tricks {
                        r.tricks = t.clone();
                    }
                    // r[impl playback.effect-parameters] - the one place params land, shared with the fader
                    apply_params(&mut r, params);
                    if let Some(f) = filter {
                        r.filter = f.clone();
                    }
                    if let Some(s) = speed {
                        r.timing.speed = s.clone();
                    }
                    // The reference's own part label and arrival win
                    // over whatever the library effect was authored
                    // with — the cue is where a part is named and
                    // timed, not the library.
                    // r[impl cues.recipe.name]
                    // r[impl cues.recipe.timing]
                    if name.is_some() {
                        r.name = name.clone();
                    }
                    if note.is_some() {
                        r.note = note.clone();
                    }
                    if cue_timing.is_some() {
                        r.cue_timing = *cue_timing;
                    }
                    r
                })
                .into_iter()
                .collect(),
            // A look is static by contract, so a look inside a look is
            // not followed: one level, never a cycle.
            // r[impl profile.looks] - resolved like any other reference
            RecipeRef::Look { look } => show
                .looks
                .get(look)
                .map(|l| {
                    l.recipes
                        .iter()
                        .filter(|r| !matches!(r, RecipeRef::Look { .. }))
                        .flat_map(|r| r.resolve(show))
                        .collect()
                })
                .unwrap_or_default(),
            RecipeRef::Bundle { bundle, target } => show
                .bundles
                .get(bundle)
                .map(|b| {
                    b.recipes
                        .iter()
                        .filter_map(|name| retarget(name, target))
                        .collect()
                })
                .unwrap_or_default(),
        }
    }

    /// The names this reference needs that `show` does not have.
    // r[impl effects.library.by-name] - an unknown name is reported, never fatal
    pub fn missing(&self, show: &Show<'_>) -> Vec<String> {
        match self {
            RecipeRef::Inline(_) => Vec::new(),
            RecipeRef::Named { effect, .. } => (!show.library.contains_key(effect))
                .then(|| format!("no library effect {effect:?}"))
                .into_iter()
                .collect(),
            RecipeRef::Bundle { bundle, .. } => match show.bundles.get(bundle) {
                None => vec![format!("no bundle {bundle:?}")],
                Some(b) => b
                    .recipes
                    .iter()
                    .filter(|name| !show.library.contains_key(*name))
                    .map(|name| format!("no library effect {name:?} (in bundle {bundle:?})"))
                    .collect(),
            },
            // r[impl profile.looks] - an unknown look is reported, never fatal
            RecipeRef::Look { look } => match show.looks.get(look) {
                None => vec![format!("no look {look:?}")],
                Some(l) => l
                    .recipes
                    .iter()
                    .filter(|r| !matches!(r, RecipeRef::Look { .. }))
                    .flat_map(|r| r.missing(show))
                    .map(|problem| format!("{problem} (in look {look:?})"))
                    .collect(),
            },
        }
    }
}

/// Applies effect parameters to a recipe — the one place `depth`,
/// `bars` and `duty` are given their meaning, so a fader and a cue's
/// reference cannot drift. `depth` is how far the relative values and
/// swings go; `bars` the loop length in bars; `duty` the first step's
/// share of the cycle. Call it on a copy: the library recipe is never
/// rewritten. Unknown names are ignored — a parameter the engine does
/// not know is one a later engine may.
// r[impl profile.effect-parameters]
// r[impl playback.effect-parameters] - depth, bars, duty; applied to a copy
pub fn apply_params(recipe: &mut Recipe, params: &BTreeMap<String, f32>) {
    if let Some(depth) = params.get("depth") {
        recipe.depth = depth.max(0.0);
    }
    if let Some(bars) = params.get("bars")
        && *bars > 0.0
    {
        recipe.timing.measure = bars * 4.0;
    }
    if let Some(duty) = params.get("duty")
        && recipe.steps.len() > 1
    {
        let duty = duty.clamp(0.01, 0.99);
        let rest = (1.0 - duty) / (recipe.steps.len() - 1) as f32;
        for (i, step) in recipe.steps.iter_mut().enumerate() {
            step.width = if i == 0 { duty } else { rest };
        }
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
// r[impl effects.waveform.is-sugar] - the waveform spelling on disk
struct RecipeWire {
    target: Selection,
    /// `"name": "movers swing in"` on disk — the part's label.
    // r[impl cues.recipe.name]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    note: Option<String>,
    /// `"cue_timing": {"position": 4.0}` on disk — this part's own
    /// arrival, in beats.
    // r[impl cues.recipe.timing]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cue_timing: Option<crate::cue::CueTiming>,
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
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    stack: bool,
    // r[impl recipes.enabled] - `"enabled": false` on disk; absent means on
    #[serde(default = "yes", skip_serializing_if = "Clone::clone")]
    enabled: bool,
    // r[impl tricks.shared-or-inline] - `"tricks_ref": "Name"` on disk
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tricks_ref: Option<String>,
    /// `"depth": 0.5` on disk; absent is 1.
    // r[impl profile.effect-parameters]
    #[serde(default = "one", skip_serializing_if = "is_one")]
    depth: f32,
    /// `"filter": {"intensity": false, ...}` on disk; absent is all.
    // r[impl profile.attribute-filter]
    #[serde(default, skip_serializing_if = "is_all")]
    filter: AttrFilter,
}

fn yes() -> bool {
    true
}

fn is_all(f: &AttrFilter) -> bool {
    *f == AttrFilter::ALL
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

// r[impl effects.waveform.is-sugar] - expands to steps on load
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
            name: w.name,
            note: w.note,
            cue_timing: w.cue_timing,
            timing: w.timing,
            tricks: w.tricks,
            stack: w.stack,
            enabled: w.enabled,
            tricks_ref: w.tricks_ref,
            depth: w.depth,
            filter: w.filter,
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
            name: r.name,
            note: r.note,
            cue_timing: r.cue_timing,
            apply: terse.clone(),
            waveform: None,
            steps: if terse.is_some() { Vec::new() } else { r.steps },
            timing: r.timing,
            tricks: r.tricks,
            stack: r.stack,
            enabled: r.enabled,
            tricks_ref: r.tricks_ref,
            depth: r.depth,
            filter: r.filter,
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
#[derive(Clone, Copy)]
pub struct Show<'a> {
    pub groups: &'a [Group],
    pub palettes: &'a Palettes,
    pub rig: &'a Rig,
    /// Named tempo sources every phaser in the show can slave to.
    // r[impl effects.masters.registry]
    pub speeds: &'a SpeedMasters,
    /// What the venue binds each profile role to.
    ///
    /// The seam that makes a cue portable: a recipe targeting
    /// `Role("Key")` resolves to whatever fixtures *this* room calls its
    /// key light. Defaults to no bindings, so a show written against
    /// venue group names behaves exactly as it did.
    // r[impl profile.resolution-by-role]
    // r[impl profile.trait-not-hardcode]
    pub roles: &'a dyn crate::selection::Roles,
    /// The effects library a cue's `RecipeRef::Named` resolves through
    /// — the profile's `effects`, or whatever the host loaded in its
    /// place. Empty by default, so a show of inline recipes needs none.
    // r[impl effects.library.by-name]
    pub library: &'a BTreeMap<String, Recipe>,
    /// Named bundles of library effects, for `RecipeRef::Bundle`.
    // r[impl effects.bundle]
    pub bundles: &'a BTreeMap<String, crate::profile::Bundle>,
    /// The profile's looks, for `RecipeRef::Look`. Empty by default,
    /// so a show that names one against a host that passed none is
    /// reported by `unresolved` rather than lit wrong.
    // r[impl profile.looks]
    pub looks: &'a BTreeMap<String, crate::profile::Look>,
    /// Where a named focus marker is *right now*, when a tracker or an
    /// operator has moved it off the palette's value. Consulted before
    /// the palette by every focus lookup, so a host can move `Vocal`
    /// per frame without rewriting the venue's palette.
    // r[impl focus.marker-moving]
    pub focus_overrides: &'a HashMap<String, Vec3>,
    /// The song's tempo map, when a transport or a loaded song map has
    /// one. Cue timing that counts beats between positions reads the
    /// time signature from here; without it four to the bar is assumed.
    // r[impl song.tempo-map] - the show carries the map the player counts beats with
    pub tempo: Option<&'a crate::music::TempoMap>,
    /// The room's declared box, from the venue, for reporting a focus
    /// that has left it. `None` is unbounded.
    // r[impl focus.stage-space] - the venue's, not a constant
    pub stage: Option<&'a crate::focus::StageSpace>,
    /// Shared Tricks chains by name — the profile's `tricks` pool — for
    /// `Recipe::tricks_ref`.
    // r[impl tricks.shared-or-inline]
    pub named_tricks: &'a BTreeMap<String, Vec<crate::tricks::Trick>>,
    /// The operator's size: scales every relative value and every focus
    /// delta at expansion, and an absolute phaser's swing about its
    /// base. `1.0` is as authored; `0.0` is the look with no effect.
    // r[impl effects.size-scales-the-swing]
    // r[impl recipes.size]
    pub size: f32,
    /// The operator's speed scale: a multiplier over every speed
    /// master, so everything slaved to any master runs faster or slower
    /// together. `1.0` is the masters as set.
    // r[impl effects.masters.scale]
    // r[impl effects.masters.uniform] - one multiplier, one code path
    pub speed_scale: f32,
    /// The smoothed sound band levels right now, from the host. Zeros
    /// when nothing is listening.
    // r[impl playback.sound-as-value]
    pub sound: SoundLevels,
}

/// No shared Tricks — every `tricks_ref` resolves to nothing.
pub static NO_NAMED_TRICKS: std::sync::LazyLock<BTreeMap<String, Vec<crate::tricks::Trick>>> =
    std::sync::LazyLock::new(BTreeMap::new);

/// No role bindings — every `Selection::Role` resolves to nothing.
pub static NO_ROLES: () = ();

/// No library — every `RecipeRef::Named` resolves to nothing.
pub static NO_LIBRARY: std::sync::LazyLock<BTreeMap<String, Recipe>> =
    std::sync::LazyLock::new(BTreeMap::new);

/// No bundles.
pub static NO_BUNDLES: std::sync::LazyLock<BTreeMap<String, crate::profile::Bundle>> =
    std::sync::LazyLock::new(BTreeMap::new);

/// No looks — every `RecipeRef::Look` resolves to nothing.
pub static NO_LOOKS: std::sync::LazyLock<BTreeMap<String, crate::profile::Look>> =
    std::sync::LazyLock::new(BTreeMap::new);

/// No moved markers — every focus name reads from the palette.
pub static NO_FOCUS_OVERRIDES: std::sync::LazyLock<HashMap<String, Vec3>> =
    std::sync::LazyLock::new(HashMap::new);

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
            library: &NO_LIBRARY,
            bundles: &NO_BUNDLES,
            looks: &NO_LOOKS,
            focus_overrides: &NO_FOCUS_OVERRIDES,
            tempo: None,
            stage: None,
            named_tricks: &NO_NAMED_TRICKS,
            size: 1.0,
            speed_scale: 1.0,
            sound: SoundLevels::default(),
        }
    }

    /// This show with the host's smoothed sound levels for this frame.
    // r[impl playback.sound-as-value]
    pub fn with_sound(&self, sound: SoundLevels) -> Show<'a> {
        Show { sound, ..*self }
    }

    /// This show with the operator's size and speed scale applied — what
    /// the programmer hands the faders and what a host hands the cue
    /// player, so one control reaches every recipe the same way.
    // r[impl effects.masters.scale] - operator state, applied to the show
    // r[impl effects.masters.uniform]
    pub fn scaled(&self, size: f32, speed_scale: f32) -> Show<'a> {
        Show {
            size,
            speed_scale,
            ..*self
        }
    }

    /// The speed masters as this show's scale sees them.
    fn scaled_speeds(&self) -> std::borrow::Cow<'a, SpeedMasters> {
        use std::borrow::Cow;
        if (self.speed_scale - 1.0).abs() < 1e-9 || self.speed_scale <= 0.0 {
            Cow::Borrowed(self.speeds)
        } else {
            Cow::Owned(
                self.speeds
                    .iter()
                    .map(|(k, v)| (k.clone(), v * self.speed_scale))
                    .collect(),
            )
        }
    }

    /// Where a focus reference points right now: a moved marker first,
    /// then the palette. The one lookup every focus apply goes through,
    /// so everything expressed against `Vocal` moves in the same frame.
    // r[impl focus.marker-moving] - the override is read before the palette
    pub fn focus(&self, r: &Ref<Vec3>) -> Option<Vec3> {
        if let Ref::Named(name) = r
            && let Some(moved) = self.focus_overrides.get(name)
        {
            return Some(*moved);
        }
        self.palettes.resolve_focus(r)
    }

    /// A named origin marker, or the room's zero when none is named;
    /// `None` when the name means nothing here.
    // r[impl focus.relative-origin]
    fn origin(&self, name: Option<&str>) -> Option<Vec3> {
        match name {
            None => Some(Vec3 {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            }),
            Some(n) => self.focus(&Ref::Named(n.to_string())),
        }
    }

    /// Whether a focus name means anything here — moved or in the palette.
    fn has_focus(&self, name: &str) -> bool {
        self.focus_overrides.contains_key(name) || self.palettes.focus(name).is_some()
    }

    /// `manufacturer model` of a fixture, for a colour preset's scope.
    fn model_of(&self, chan: ChanId) -> Option<String> {
        self.rig
            .get(chan)
            .map(|f| format!("{} {}", f.manufacturer, f.model).trim().to_string())
    }
}

/// One resolved value, and whether it replaces what is underneath or
/// adds to it.
#[derive(Debug, Clone, PartialEq)]
// r[impl recipes.relative-is-a-separate-layer] - flags relative so the player keeps it apart
pub struct Emit {
    pub value: CueValue,
    /// `true` for a `RecipeApply::Delta` — the caller adds this on top
    /// of whatever won the cascade rather than letting it compete.
    pub relative: bool,
    /// The device-independent colour this value is one channel of, for
    /// a colour emit (`ColorAdd{Red,Green,Blue}` from a `Color`,
    /// `Colors` or `Split`): the preset's stored `Intent`, or
    /// `Intent::Rgb` of the resolved triple when it stores none. The
    /// player carries it to output so a fixture's emitters are solved
    /// against *this*, never against the triple re-derived from three
    /// floats. Every other emit carries `None`. Blending between two
    /// steps of a colour phaser carries the intent of the **nearer**
    /// step (the outgoing one below half-way, the incoming one from
    /// half-way): a blend of two intents has no single meaning, and the
    /// triple beside it is already interpolated for anything that wants
    /// the in-between.
    // r[impl color.intent-to-output] - the intent rides beside the triple
    pub intent: Option<crate::color::Intent>,
}

/// The show's smoothed sound band levels, `0..=1` each, for
/// `RecipeApply::Sound` and `Random::high_from_band`.
///
/// Smoothing (the "sound fade") is the **host's** job: whatever
/// analyses audio hands the *already smoothed* levels in `Show::sound`
/// every frame, so a kick reads as a lift and not as noise, and so a
/// recipe stays a pure function of what it is handed. Zeros mean no
/// sound — every sound-driven value sits at its `low`.
// r[impl playback.sound-as-value] - the levels the recipe reads
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct SoundLevels {
    pub low: f32,
    pub mid: f32,
    pub high: f32,
}

impl SoundLevels {
    /// The level of one band, clamped to `0..=1`.
    pub fn level(&self, band: Band) -> f32 {
        match band {
            Band::Low => self.low,
            Band::Mid => self.mid,
            Band::High => self.high,
        }
        .clamp(0.0, 1.0)
    }
}

/// One of the three sound bands a recipe can read.
// r[impl playback.sound-as-value]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Band {
    Low,
    Mid,
    High,
}

/// A `RecipeApply::FocusDelta` the recipe could not resolve on its own,
/// because the step it sits in names no point: the player owns the aim.
///
/// **Contract for the cue player.** For each `chan` here, if the
/// cascade currently aims that fixture at a *point* (a `FocusPoint`,
/// `FocusFan` or `FocusKeyframes` won pan/tilt for it), call
/// `focus::resolve_focus_delta(chan, point, delta, rig)` and write the
/// returned pan/tilt as the fixture's *absolute* pan and tilt for this
/// frame — after the absolute layer, before relative `Emit`s are added.
/// If the fixture has no point aim (an orientation, or nothing), ignore
/// the delta (`r[focus.delta]`). Two sustained recipes both emitting a
/// delta for one channel follow the same rule as relative values: last
/// wins unless the recipe stacks. Deltas are already blended between
/// steps and inverted per `Trick::Invert`, so a path in metres arrives
/// here as one point per frame.
#[derive(Debug, Clone, PartialEq)]
// r[impl focus.delta]
// r[impl focus.orbit-in-metres]
pub struct FocusDeltaEmit {
    pub chan: ChanId,
    /// Metres, in the room.
    pub delta: Vec3,
}

/// Everything a recipe resolves to: attribute values, and the focus
/// deltas only the player can place. `expand_recipe` is the first half.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Expansion {
    pub emits: Vec<Emit>,
    pub focus_deltas: Vec<FocusDeltaEmit>,
    /// The room point each channel is aimed at, for every channel a
    /// `FocusPoint`, `FocusFan` or `FocusKeyframes` in this recipe
    /// solved. The player keeps it beside the solved pan/tilt so a
    /// later `FocusDelta` has something to be relative to.
    // r[impl focus.delta] - the aim the player tracks
    pub focus_points: Vec<(ChanId, Vec3)>,
}

/// The preset's value for one fixture — its scope walked in the spec's
/// fallback order — as a preset with that triple, so the distribution
/// code below still blends presets.
// r[impl color.scope.fallback-order] - resolved per fixture at expansion
// r[impl color.scope.selective]
// r[impl color.scope.global]
fn scoped(c: &ColorPreset, chan: ChanId, model: Option<&str>) -> ColorPreset {
    let rgb = c.resolve_for(chan, model);
    ColorPreset {
        red: rgb.red,
        green: rgb.green,
        blue: rgb.blue,
        ..c.clone()
    }
}

/// The three additive colour attributes a colour resolves to.
fn rgb_values(c: &ColorPreset) -> Vec<(Attribute, f32)> {
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
    ]
}

/// Picks (or blends) the colour that a unit at `slot` takes from a
/// resolved list, per `Distribute`.
///
/// Works on `Slot` — the unit's place after Tricks — rather than on a
/// channel, so a `Block(2)` gives one colour per pair and a `Group(2)`
/// paints odds against evens, with no colour-specific ordering logic.
// r[impl color.multi.distribution]
// r[impl color.multi.order]
// r[impl tricks.spread.blocks-are-units]
fn distribute_color(colors: &[ColorPreset], distribute: Distribute, slot: Slot) -> ColorPreset {
    let n = colors.len();
    match distribute {
        Distribute::Cycle => colors[slot.index % n].clone(),
        Distribute::Block => {
            // Integer division so the runs are contiguous and as even as
            // the counts allow; the last colour absorbs any remainder.
            let run = (slot.index * n / slot.count.max(1)).min(n - 1);
            colors[run].clone()
        }
        Distribute::Spread => {
            // r[impl tricks.spread]
            let t = slot.fraction() * (n - 1) as f32;
            let lo = (t.floor() as usize).min(n - 1);
            let hi = (lo + 1).min(n - 1);
            let f = t - lo as f32;
            let (a, b) = (&colors[lo], &colors[hi]);
            ColorPreset {
                name: String::new(),
                red: a.red + (b.red - a.red) * f,
                green: a.green + (b.green - a.green) * f,
                blue: a.blue + (b.blue - a.blue) * f,
                ..Default::default()
            }
        }
    }
}

/// The point `fraction` of the way from `from` to `to`.
fn lerp_vec3(from: ignition_proto::Vec3, to: ignition_proto::Vec3, f: f32) -> ignition_proto::Vec3 {
    let f = f as f64;
    ignition_proto::Vec3 {
        x: from.x + (to.x - from.x) * f,
        y: from.y + (to.y - from.y) * f,
        z: from.z + (to.z - from.z) * f,
    }
}

/// What one apply resolved to for one channel: attribute values, a
/// room point to aim at, and/or a room offset to add to that point.
#[derive(Default)]
struct Resolved {
    values: Vec<(Attribute, f32)>,
    relative: bool,
    point: Option<Vec3>,
    delta: Option<Vec3>,
    /// The intent behind a colour's `values`, for the player.
    intent: Option<crate::color::Intent>,
}

impl Resolved {
    fn values(values: Vec<(Attribute, f32)>, relative: bool) -> Self {
        Self {
            values,
            relative,
            ..Default::default()
        }
    }

    /// A colour: its triple as values, its intent beside them. A
    /// preset with no stored intent (a pre-intent file, or a `Spread`
    /// blend of two) carries `Intent::Rgb` of its triple.
    // r[impl color.intent-to-output]
    fn colour(c: &ColorPreset) -> Self {
        Self {
            values: rgb_values(c),
            intent: Some(c.intent()),
            ..Default::default()
        }
    }

    fn point(point: Option<Vec3>) -> Self {
        Self {
            point,
            ..Default::default()
        }
    }
}

/// The unit's place in time and in the selection — what a generator
/// needs beyond the channel.
#[derive(Clone, Copy)]
struct Clock {
    /// The recipe's own cycle count at this instant.
    cycles: f32,
    /// Seconds on the show clock, for an apply that carries its own
    /// timing (a canvas recipe).
    secs: f32,
    /// Where this unit sits in the unit grid, 0..=1 on X and Y — the
    /// canvas coordinate. A degenerate axis reads the picture's middle.
    uv: (f32, f32),
}

impl Clock {
    /// The canvas coordinate of a unit: the *centre* of its cell,
    /// `(x + 0.5) / count`, the way a pixel samples a picture. Cell
    /// centres rather than `x / (count - 1)` because the content wraps:
    /// on a ring the two ends of a row would be the same point, and a
    /// wipe could never tell the first fixture from the last. An axis
    /// nothing varies along is one cell, so it reads the middle.
    // r[impl canvas.grid] - the unit grid is the canvas
    fn uv_of(pos: &crate::tricks::UnitPos) -> (f32, f32) {
        let f = |i: usize, n: usize| (i as f32 + 0.5) / n.max(1) as f32;
        (f(pos.x, pos.count[0]), f(pos.y, pos.count[1]))
    }
}

/// Resolves one apply, for one channel sitting at `slot` in the
/// selection, into concrete attribute values.
fn apply_values(
    apply: &RecipeApply,
    chan: ChanId,
    slot: Slot,
    clock: Clock,
    show: &Show<'_>,
) -> Resolved {
    match apply {
        RecipeApply::Dimmer(value) => Resolved::values(vec![(Attribute::Dimmer, *value)], false),
        // r[impl color.recall-by-reference] - resolved from the palette at output time
        // r[impl color.scope.fallback-order] - the preset meets this fixture through its scope
        RecipeApply::Color(reference) => match show.palettes.resolve_color(reference) {
            Some(c) => Resolved::colour(&scoped(&c, chan, show.model_of(chan).as_deref())),
            None => Resolved::default(),
        },
        // r[impl color.multi]
        // r[impl color.recall-by-reference] - every entry resolved from the palette at output time
        RecipeApply::Colors { colors, distribute } => {
            // All-or-nothing: one missing name empties the whole apply,
            // so `unresolved` is the only place a hole shows up.
            let model = show.model_of(chan);
            let resolved: Option<Vec<ColorPreset>> = colors
                .iter()
                .map(|r| {
                    show.palettes
                        .resolve_color(r)
                        .map(|c| scoped(&c, chan, model.as_deref()))
                })
                .collect();
            match resolved {
                Some(list) if !list.is_empty() => {
                    Resolved::colour(&distribute_color(&list, *distribute, slot))
                }
                _ => Resolved::default(),
            }
        }
        // r[impl color.multi]
        // r[impl color.recall-by-reference] - the split and its members resolved at output time
        // r[impl color.embedding]
        RecipeApply::Split(reference) => match show.palettes.resolve_split(reference) {
            Some((list, distribute)) => {
                let model = show.model_of(chan);
                let list: Vec<ColorPreset> = list
                    .iter()
                    .map(|c| scoped(c, chan, model.as_deref()))
                    .collect();
                Resolved::colour(&distribute_color(&list, distribute, slot))
            }
            None => Resolved::default(),
        },
        // r[impl focus.point]
        // r[impl focus.resolve-at-output] - the point is kept until the step is solved
        // r[impl focus.marker-moving] - read through the show, so a moved marker wins
        RecipeApply::FocusPoint(reference) => Resolved::point(show.focus(reference)),
        // r[impl focus.orientation]
        // r[impl focus.resolve-at-output]
        RecipeApply::FocusDirection(dir) => match show.rig.placement(chan) {
            Some(p) => {
                let (pan, tilt) = pan_tilt_deg_along(p.orientation, *dir);
                Resolved::values(vec![(Attribute::Pan, pan), (Attribute::Tilt, tilt)], false)
            }
            None => Resolved::default(),
        },
        // r[impl focus.pattern]
        // r[impl focus.pattern.fan]
        // r[impl focus.pattern.order-is-the-selection] - the slot is the selection's order after Tricks
        // r[impl focus.resolve-at-output]
        // r[impl tricks.spread.blocks-are-units]
        RecipeApply::FocusFan { from, to } => Resolved::point(
            show.focus(from)
                .zip(show.focus(to))
                .map(|(a, b)| lerp_vec3(a, b, slot.fraction())),
        ),
        // r[impl focus.magic]
        // r[impl tricks.keyframes]
        // r[impl focus.pattern.order-is-the-selection]
        RecipeApply::FocusKeyframes(points) => {
            use crate::tricks::{Curve, FanShape, Keyframes};
            let resolved: Option<Vec<Vec3>> = points.iter().map(|r| show.focus(r)).collect();
            Resolved::point(resolved.and_then(|list| {
                Keyframes::segment(
                    FanShape::Linear,
                    Curve::Linear,
                    list.len(),
                    slot.index,
                    slot.count,
                )
                .map(|(lo, hi, f)| lerp_vec3(list[lo], list[hi], f))
            }))
        }
        // r[impl focus.delta]
        RecipeApply::FocusDelta(delta) => Resolved {
            delta: Some(*delta),
            ..Default::default()
        },
        // An orientation per fixture from its real offset: outward from
        // the origin along the axis, by so many degrees a metre.
        // r[impl focus.pattern.parallel-out]
        // r[impl focus.resolve-at-output] - the fixture's live position decides its lean
        RecipeApply::FocusSplay {
            axis,
            degrees_per_metre,
            origin,
        } => match (show.rig.placement(chan), show.origin(origin.as_deref())) {
            (Some(p), Some(o)) => {
                let offset = v_add(p.position, v_scale(o, -1.0));
                let dir = crate::focus::splay_direction(*axis, offset, *degrees_per_metre);
                let (pan, tilt) = pan_tilt_deg_along(p.orientation, dir);
                Resolved::values(vec![(Attribute::Pan, pan), (Attribute::Tilt, tilt)], false)
            }
            _ => Resolved::default(),
        },
        // r[impl focus.pattern.per-fixture] - own, then model, then shared
        RecipeApply::FocusPerFixture {
            aims,
            models,
            default,
        } => Resolved::point(
            aims.get(&chan)
                .copied()
                .or_else(|| show.model_of(chan).and_then(|m| models.get(&m).copied()))
                .or(*default),
        ),
        // A constrained axis is an offset from the origin; a free one is
        // the fixture's own coordinate, never zero.
        // r[impl focus.partial-axes]
        // r[impl focus.relative-origin]
        RecipeApply::FocusAxes { x, y, z, origin } => {
            match (show.rig.placement(chan), show.origin(origin.as_deref())) {
                (Some(p), Some(o)) => Resolved::point(Some(Vec3 {
                    x: x.map_or(p.position.x, |v| v + o.x),
                    y: y.map_or(p.position.y, |v| v + o.y),
                    z: z.map_or(p.position.z, |v| v + o.z),
                })),
                _ => Resolved::default(),
            }
        }
        // r[impl focus.relative-origin] - metres from a marker, read live
        RecipeApply::FocusRelative { origin, offset } => Resolved::point(
            show.focus(&Ref::Named(origin.clone()))
                .map(|o| v_add(o, *offset)),
        ),
        // r[impl effects.random]
        RecipeApply::Random(random) => {
            let random = random.heard(&show.sound);
            Resolved::values(
                vec![(random.attr.clone(), random.at(slot.index, clock.cycles))],
                !random.absolute,
            )
        }
        // r[impl playback.sound-as-value] - the band's smoothed level, as a value
        RecipeApply::Sound {
            band,
            attr,
            low,
            high,
            relative,
        } => Resolved::values(
            vec![(attr.clone(), low + (high - low) * show.sound.level(*band))],
            *relative,
        ),
        RecipeApply::Raw(values) => Resolved::values(values.clone(), false),
        // r[impl effects.modulates-with-delta]
        RecipeApply::Delta(values) => Resolved::values(values.clone(), true),
        // The picture is sampled where this unit sits in the grid, on
        // the canvas recipe's own clock, and the channel says which
        // attribute the colour becomes.
        // r[impl canvas.bitmap-channels] - sampled at the unit's grid position
        // r[impl canvas.on-the-stack] - emitted like any other value, absolute or relative
        RecipeApply::Canvas { recipe, channel } => {
            let cycles = recipe.cycles_at(clock.secs, show.speeds);
            let (u, v) = clock.uv;
            let values = crate::canvas::sample_for_grid(recipe, channel, &[(chan, u, v)], cycles)
                .into_iter()
                .map(|(_, attr, value)| (attr, value))
                .collect();
            Resolved::values(values, channel.relative)
        }
    }
}

/// Everything one step sets for one channel, keyed so two steps can be
/// interpolated attribute by attribute — plus the focus delta it could
/// not fold into a point, for the player.
#[derive(Default)]
struct StepValues {
    values: HashMap<(Attribute, bool), f32>,
    focus_delta: Option<Vec3>,
    /// The room point the step aimed this channel at, delta folded in.
    point: Option<Vec3>,
    /// The intent of the last colour apply in the step — the one whose
    /// triple won `values`, since a later apply overwrites an earlier.
    intent: Option<crate::color::Intent>,
}

fn step_values(step: &Step, chan: ChanId, slot: Slot, clock: Clock, show: &Show<'_>) -> StepValues {
    let mut out = StepValues::default();
    let mut point = None;
    let mut delta: Option<Vec3> = None;
    for apply in &step.apply {
        let r = apply_values(apply, chan, slot, clock, show);
        for (attr, value) in r.values {
            out.values.insert((attr, r.relative), value);
        }
        if r.intent.is_some() {
            out.intent = r.intent;
        }
        if r.point.is_some() {
            point = r.point;
        }
        if let Some(d) = r.delta {
            delta = Some(delta.map_or(d, |acc| v_add(acc, d)));
        }
    }
    match (point, delta) {
        // A point in this step: the delta folds into it and the step
        // emits absolute pan/tilt, solved per fixture.
        // r[impl focus.delta] - folded into a point named in the same step
        // r[impl focus.point]
        // r[impl focus.resolve-at-output]
        (Some(p), delta) => {
            if let Some(place) = show.rig.placement(chan) {
                let target = delta.map_or(p, |d| v_add(p, d));
                let (pan, tilt) = pan_tilt_deg_to_point(place.position, place.orientation, target);
                out.values.insert((Attribute::Pan, false), pan);
                out.values.insert((Attribute::Tilt, false), tilt);
                out.point = Some(target);
            }
        }
        // No point here: the cascade owns the aim, so the player places
        // the delta against it.
        // r[impl focus.delta] - handed to the player against the cascade's aim
        (None, Some(d)) => out.focus_delta = Some(d),
        (None, None) => {}
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
// r[impl recipes.template]
// r[impl groups.resolution-is-live]
// r[impl recipes.cook-fixes-coverage] - values re-resolved on every call
// r[impl effects.sync.pure-function]
// r[impl recipes.status.selects-nothing-is-not-an-error] - an empty selection yields no emits, not an error
// r[impl default.optional-is-not-second-class] - an unbound optional layer lights nothing and runs on
pub fn expand_recipe(recipe: &Recipe, show: &Show<'_>, secs: f32) -> Vec<Emit> {
    expand_recipe_full(recipe, show, secs).emits
}

/// `expand_recipe`, plus the focus deltas the player has to place — see
/// [`FocusDeltaEmit`] for what to do with them.
// r[impl focus.delta]
// r[impl effects.sync.pure-function]
// r[impl recipes.enabled] - a disabled recipe expands to nothing
// r[impl effects.masters.scale] - the show's speed scale multiplies every master here
// r[impl effects.masters.uniform] - every recipe, cue-player or fader, passes through this one place
// r[impl effects.size-scales-the-swing] - relative values, focus deltas and absolute swings scaled at output
pub fn expand_recipe_full(recipe: &Recipe, show: &Show<'_>, secs: f32) -> Expansion {
    let mut out = Expansion::default();
    if recipe.steps.is_empty() || !recipe.enabled {
        return out;
    }
    // Speed scale: the masters as the operator has scaled them. Hz and
    // BPM recipes are not slaved to anything and are not touched.
    let speeds = show.scaled_speeds();
    // `depth` is this recipe's own size — the same scaling size does
    // to every recipe, for this one alone — so it rides in through the
    // one door size uses.
    // r[impl playback.effect-parameters] - depth through the show's size
    let show = &Show {
        speeds: &speeds,
        size: show.size * recipe.depth.max(0.0),
        ..*show
    };
    let size = show.size.max(0.0);
    let tricks = recipe.effective_tricks(show);
    // Roles first, so a recipe targeting `Key` finds whatever this venue
    // calls its key light, then Tricks, so the phase spread lands on the
    // *units* the Tricks made rather than on raw fixtures.
    let chans = crate::selection::resolve_with(&recipe.target, show.groups, show.rig, show.roles);
    // The selection on its grid: the layout the selection declares, or
    // the rows the room implies. On a rig where nothing varies along Y
    // or Z this is `[n, 1, 1]` and everything below reads exactly as
    // the one-dimensional path did.
    // r[impl tricks.grid]
    // r[impl tricks.grid.from-space]
    // r[impl tricks.grid.explicit-override]
    // r[impl tricks.grid.degenerate-axes]
    let layout = crate::selection::layout_of(&recipe.target);
    let grid = selection_grid(&chans, layout, show.rig, crate::tricks::GridAxes::default());
    // r[impl effects.phase.spread] - spread lands on Trick units
    // r[impl tricks.spread.blocks-are-units]
    // r[impl effects.phase.in-selection-order]
    let gu = crate::tricks::apply_all_grid(&tricks, &grid);
    let count = gu.len();
    let phaser = recipe.is_phaser();
    // Which units flip their relative values, and which attributes.
    // r[impl tricks.invert]
    // r[impl effects.invert]
    let inverts = crate::tricks::inverted_grid(&tricks, &gu);
    // The recipe's own clock, for generators that are a function of time
    // rather than a place in the step table.
    let cycles_now = recipe.timing.cycles(secs, show.speeds);

    // `Negative` inverts each attribute about its own swing, so the
    // extremes have to be known before any fixture is resolved. Computed
    // once rather than per fixture — the step table is the same for all
    // of them.
    // r[impl effects.play] - Negative
    let negative = recipe.timing.direction == Play::Negative;
    // Size scales an absolute phaser about the middle of its own swing,
    // which needs the same extremes.
    let scaled_absolute = phaser && (size - 1.0).abs() > 1e-6;
    let swing = (negative || scaled_absolute).then(|| swing_of(recipe, show));

    for (index, unit) in gu.units.0.iter().enumerate() {
        let pos = gu.pos[index];
        let clock = Clock {
            cycles: cycles_now,
            secs,
            uv: Clock::uv_of(&pos),
        };
        let (prev, cur, blend) = if !phaser {
            (0, 0, 1.0)
        // r[impl effects.play] - Build
        // r[impl effects.play.build-is-a-mode]
        } else if recipe.timing.direction == Play::Build {
            // Build is not a phase shift of a shared waveform: a fixture
            // arrives when the cycle passes it and *stays* until the
            // wrap, so the selection fills up and then resets. That is a
            // threshold against the fixture's own position in the
            // selection, not a position in the step list — which is why
            // it is a mode rather than something a step table can say.
            let u = cycles_now - cycles_now.floor();
            // r[impl effects.phase.spread] - per axis, for Build
            let arrived = u >= recipe.timing.build_fraction_3d(&pos);
            let last = recipe.steps.len() - 1;
            if arrived {
                (last, last, 1.0)
            } else {
                (0, 0, 1.0)
            }
        } else {
            // r[impl effects.phase.spread] - per axis
            let cycles = recipe.timing.cycles_at_pos(secs, &pos, show.speeds);
            locate(&recipe.steps, cycles)
        };

        // Phase was decided per *unit* above; values are still resolved
        // per fixture, because a step can say something fixture-relative
        // — a focus point is a different pan and tilt for every head
        // pointing at it. So a blocked pair shares a moment and still
        // aims individually, which is what blocking is supposed to mean.
        // r[impl effects.phase.values-per-fixture]
        // Distributed values (a colour spread, a focus fan) are likewise
        // decided per unit: both fixtures of a blocked pair take the
        // same slot, which is what "blocks are units" means for them.
        // r[impl tricks.spread.blocks-are-units]
        // r[impl color.multi.order]
        let slot = Slot { index, count };
        let invert = inverts.get(index).copied().flatten();
        for chan in unit.iter().copied() {
            let to = step_values(&recipe.steps[cur], chan, slot, clock, show);
            // Resolving the outgoing step is only worth it mid-transition.
            let from = if blend < 1.0 && prev != cur {
                step_values(&recipe.steps[prev], chan, slot, clock, show)
            } else {
                Default::default()
            };

            // A path in metres blends the same way the angles would, so
            // sixteen points round the drummer is a circle, not a
            // sixteen-gon; an inverted unit runs the mirror-image path.
            // r[impl focus.orbit-in-metres] - the delta interpolates between steps
            // r[impl effects.interpolate]
            if let Some(target) = to.focus_delta {
                let mut delta = match from.focus_delta {
                    Some(start) => v_add(
                        start,
                        v_scale(v_add(target, v_scale(start, -1.0)), blend as f64),
                    ),
                    None => target,
                };
                if let Some(style) = invert {
                    // r[impl effects.invert] - a delta in metres flips the axes its style names
                    if style.covers(&Attribute::Pan) {
                        delta.x = -delta.x;
                    }
                    if style.covers(&Attribute::Tilt) {
                        delta.y = -delta.y;
                    }
                }
                // r[impl effects.size-scales-the-swing] - a metre offset shrinks about zero
                let delta = v_scale(delta, size as f64);
                out.focus_deltas.push(FocusDeltaEmit { chan, delta });
            }

            // The aim itself travels to the player beside the angles it
            // solved to, blended between steps the way the angles are.
            // r[impl focus.delta] - the player learns the aim, not only the angles
            if let Some(target) = to.point {
                let point = match from.point {
                    Some(start) => v_add(
                        start,
                        v_scale(v_add(target, v_scale(start, -1.0)), blend as f64),
                    ),
                    None => target,
                };
                out.focus_points.push((chan, point));
            }

            // The nearer step's intent rides on the colour emits.
            // r[impl color.intent-to-output] - carried, not re-derived from the blended triple
            let intent = if blend < 0.5 && from.intent.is_some() {
                &from.intent
            } else {
                &to.intent
            };
            // r[impl effects.interpolate]
            for (key, target) in &to.values {
                // An attribute the outgoing step did not set has nothing
                // to move away from, so it takes this step's value.
                let value = match from.values.get(key) {
                    Some(start) => start + (target - start) * blend,
                    None => *target,
                };
                // r[impl tricks.invert] - the sign of a relative value, per unit and style
                // r[impl effects.invert]
                let value = match invert {
                    Some(style) if key.1 && style.covers(&key.0) => -value,
                    _ => value,
                };
                let value = match &swing {
                    // Reflect about the middle of this attribute's own
                    // range: the fixture that would have been brightest
                    // is now the dark one travelling across the rig.
                    Some(swing) if negative => match swing.get(&key.0) {
                        Some((lo, hi)) => lo + hi - value,
                        None => value,
                    },
                    _ => value,
                };
                // Size. A relative value swings about zero, so it simply
                // scales; an absolute phaser swings about the middle of
                // its own range, which stays put. At zero a Delta is
                // absent and an absolute one sits at its base.
                // r[impl effects.size-scales-the-swing]
                // r[impl recipes.size]
                let value = if key.1 {
                    value * size
                } else if scaled_absolute {
                    match swing.as_ref().and_then(|s| s.get(&key.0)) {
                        Some((lo, hi)) => {
                            let mid = (lo + hi) / 2.0;
                            mid + (value - mid) * size
                        }
                        None => value,
                    }
                } else {
                    value
                };
                out.emits.push(Emit {
                    value: CueValue {
                        chan,
                        attr: key.0.clone(),
                        value,
                    },
                    relative: key.1,
                    intent: if is_colour(&key.0) {
                        intent.clone()
                    } else {
                        None
                    },
                });
            }
        }
    }
    // The recipe's own attribute filter: an emit outside it is dropped
    // here, before any layer weighs it, so it withdraws nothing. A
    // position filter that is off drops the focus work too — a
    // colour-only rainbow aims nothing.
    // r[impl playback.attribute-filter] - per emit, at expansion
    if recipe.filter != AttrFilter::ALL {
        out.emits.retain(|e| recipe.filter.admits(&e.value.attr));
        if !recipe.filter.position {
            out.focus_deltas.clear();
            out.focus_points.clear();
        }
    }
    out
}

/// Whether an attribute is one channel of a colour — the emits an
/// `Intent` rides beside.
fn is_colour(attr: &Attribute) -> bool {
    matches!(attr, Attribute::ColorAdd { .. })
}

/// The grid a recipe's selection is expanded on.
///
/// A declared layout is taken as it is. Otherwise the room supplies
/// the *rows* — the selection binned along `axes.y`, nearest the
/// downstage edge first — and the selection's own order supplies the
/// position along each row. That second half is deliberate: the
/// selection's `Order` is the one ordering authority, and a right-to-
/// left chase (`Order::Axis(X, Desc)`) has to stay right-to-left, which
/// a grid that re-sorted every row by real X would undo. A fixture the
/// rig cannot place sits in the first row, never dropped; a rig with no
/// placements at all is therefore one row in selection order, exactly
/// the one-dimensional path. Z is not binned yet: a selection is at
/// most rows of a wall.
// r[impl tricks.grid.from-space] - rows from the room, order from the selection
// r[impl tricks.grid.explicit-override]
// r[impl tricks.grid.degenerate-axes] - one row is `[n, 1, 1`]
// r[impl effects.phase.in-selection-order] - the X spread walks the selection
// r[impl groups.one-ordering-authority]
fn selection_grid(
    chans: &[ChanId],
    layout: Option<&Vec<Vec<ChanId>>>,
    rig: &Rig,
    axes: crate::tricks::GridAxes,
) -> crate::tricks::Grid {
    use crate::tricks::Grid;
    if layout.is_some() {
        return Grid::for_selection(chans, layout, rig, axes);
    }
    // The room, binned on Y and Z, in the selection's own order along
    // each row — see `Grid::from_rig_in_order` for why not `from_rig`.
    // r[impl tricks.grid.from-space]
    Grid::from_rig_in_order(chans, rig, axes)
}

/// The lowest and highest value each attribute takes across the step
/// table, which is what `Play::Negative` reflects about.
///
/// Resolved against the first channel of the selection: the values that
/// vary per fixture are focus points, and inverting a *position* is not
/// what negative means to anyone.
// r[impl effects.play] - Negative reflects about each attribute's own swing
fn swing_of(recipe: &Recipe, show: &Show<'_>) -> HashMap<Attribute, (f32, f32)> {
    let chan = resolve(&recipe.target, show.groups, show.rig)
        .first()
        .copied()
        .unwrap_or(0);
    let mut swing: HashMap<Attribute, (f32, f32)> = HashMap::new();
    for step in &recipe.steps {
        let clock = Clock {
            cycles: 0.0,
            secs: 0.0,
            uv: (0.5, 0.5),
        };
        for ((attr, _), value) in step_values(step, chan, Slot::ONLY, clock, show).values {
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
// r[impl color.unresolved-is-visible]
// r[impl effects.masters.unknown] - reported against every cue that asked
// r[impl profile.unbound-is-visible]
// r[impl recipes.status.selects-nothing-is-not-an-error] - reported, never fatal
pub fn unresolved(cues: &[Cue], show: &Show<'_>) -> Vec<String> {
    let mut out = Vec::new();
    for cue in cues {
        // r[impl effects.library.by-name] - an unknown effect or bundle name is reported by cue
        for problem in cue.recipes.iter().flat_map(|r| r.missing(show)) {
            out.push(format!("cue {:?}: {problem}", cue.name));
        }
        for recipe in cue.resolved_recipes(show) {
            for problem in crate::selection::unresolved_names_with(
                &recipe.target,
                show.groups,
                show.rig,
                show.roles,
            ) {
                out.push(format!("cue {:?}: {problem}", cue.name));
            }
            // r[impl effects.masters.unknown] - the report
            if let Speed::Master(name) = &recipe.timing.speed
                && !show.speeds.contains_key(name)
            {
                out.push(format!("cue {:?}: no speed master {name:?}", cue.name));
            }
            // r[impl tricks.shared-or-inline] - an unknown shared name is reported, never fatal
            if let Some(name) = &recipe.tricks_ref
                && !show.named_tricks.contains_key(name)
            {
                out.push(format!("cue {:?}: no shared tricks {name:?}", cue.name));
            }
            // A point off the stage, and a fixture that cannot reach one.
            // Reported, not refused: the show still aims there.
            // r[impl focus.stage-space] - a point outside the room is reported
            // r[impl focus.unreachable] - reportable per fixture
            for problem in focus_problems(&recipe, show) {
                out.push(format!("cue {:?}: {problem}", cue.name));
            }
            for apply in recipe.steps.iter().flat_map(|s| &s.apply) {
                match apply {
                    RecipeApply::Color(Ref::Named(name)) if show.palettes.color(name).is_none() => {
                        out.push(format!("cue {:?}: no colour palette {:?}", cue.name, name));
                    }
                    RecipeApply::FocusPoint(Ref::Named(name)) if !show.has_focus(name) => {
                        out.push(format!("cue {:?}: no focus palette {:?}", cue.name, name));
                    }
                    // r[impl focus.relative-origin] - a missing origin is reported by name
                    RecipeApply::FocusRelative { origin, .. } if !show.has_focus(origin) => {
                        out.push(format!("cue {:?}: no focus origin {:?}", cue.name, origin));
                    }
                    RecipeApply::FocusSplay {
                        origin: Some(origin),
                        ..
                    }
                    | RecipeApply::FocusAxes {
                        origin: Some(origin),
                        ..
                    } if !show.has_focus(origin) => {
                        out.push(format!("cue {:?}: no focus origin {:?}", cue.name, origin));
                    }
                    // r[impl color.unresolved-is-visible] - every missing entry of a multi-colour, by name
                    RecipeApply::Colors { colors, .. } => {
                        for name in colors.iter().filter_map(|c| match c {
                            Ref::Named(name) if show.palettes.color(name).is_none() => Some(name),
                            _ => None,
                        }) {
                            out.push(format!("cue {:?}: no colour palette {:?}", cue.name, name));
                        }
                    }
                    // r[impl color.unresolved-is-visible] - a missing split, a missing member, a cycle, by cue
                    RecipeApply::Split(reference) => {
                        for problem in show.palettes.split_problems(reference) {
                            out.push(format!("cue {:?}: {problem}", cue.name));
                        }
                    }
                    RecipeApply::FocusKeyframes(points) => {
                        for name in points.iter().filter_map(|c| match c {
                            Ref::Named(name) if !show.has_focus(name) => Some(name),
                            _ => None,
                        }) {
                            out.push(format!("cue {:?}: no focus palette {:?}", cue.name, name));
                        }
                    }
                    RecipeApply::FocusFan { from, to } => {
                        for name in [from, to].into_iter().filter_map(|c| match c {
                            Ref::Named(name) if !show.has_focus(name) => Some(name),
                            _ => None,
                        }) {
                            out.push(format!("cue {:?}: no focus palette {:?}", cue.name, name));
                        }
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

/// Where a recipe's points fall short of the room: a point outside the
/// declared stage space, and a fixture whose pan or tilt travel cannot
/// reach the point it was given. Both are solved anyway — the report
/// is the whole feature.
// r[impl focus.stage-space]
// r[impl focus.unreachable] - the report; the clamp is in `focus::reachable`
fn focus_problems(recipe: &Recipe, show: &Show<'_>) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen_off_stage: Vec<ChanId> = Vec::new();
    let range = crate::focus::PanTiltRange::default();
    let expansion = expand_recipe_full(recipe, show, 0.0);
    for (chan, point) in expansion.focus_points {
        if let Some(stage) = show.stage
            && !stage.contains(point)
            && !seen_off_stage.contains(&chan)
        {
            seen_off_stage.push(chan);
            out.push(format!(
                "fixture {chan} aimed at ({:.2}, {:.2}, {:.2}), outside the stage space",
                point.x, point.y, point.z
            ));
        }
        if let Some(p) = show.rig.placement(chan) {
            let (_, reach) =
                crate::focus::pan_tilt_deg_to_point_within(p.position, p.orientation, point, range);
            if reach == crate::focus::Reach::Clamped {
                out.push(format!(
                    "fixture {chan} cannot reach ({:.2}, {:.2}, {:.2}); clamped to its range",
                    point.x, point.y, point.z
                ));
            }
        }
    }
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
// r[impl recipes.status]
pub enum Cook {
    /// Resolved to this many fixtures — MA3's green pot.
    Ok(usize),
    /// Resolved to nothing: a group this room lacks, a spatial filter
    /// that excluded everything, a palette name that does not exist.
    /// MA3's red pot. Not an error — the show still runs — but almost
    /// always a mistake, and invisible without this.
    Empty,
    /// Switched off by its `enabled` flag: contributes nothing, on
    /// purpose. Not a failure — the line is still in the file.
    // r[impl recipes.enabled] - cook reports "disabled"
    Disabled,
}

/// A whole cue's cooked state.
#[derive(Debug, Clone, PartialEq)]
// r[impl recipes.status]
// r[impl cues.cooked-status]
pub struct CueCook {
    pub name: String,
    pub recipes: Vec<Cook>,
    /// How many direct (layer 1) values the cue carries.
    pub direct: usize,
}

/// The one-glance verdict, matching MA3's pot colours.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
// r[impl recipes.status]
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
    // r[impl recipes.status]
    // r[impl recipes.status.selects-nothing-is-not-an-error] - Empty is a status, not a failure to load
    // r[impl recipes.enabled] - a disabled line is neither a failure nor a recipe that ran
    pub fn status(&self) -> Status {
        let live = self
            .recipes
            .iter()
            .filter(|c| **c != Cook::Disabled)
            .count();
        if self.recipes.contains(&Cook::Empty) {
            Status::Failed
        } else if live == 0 {
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
// r[impl recipes.status.visible-per-cue]
// r[impl cues.cooked-status]
pub fn cook_cue(cue: &Cue, show: &Show<'_>, secs: f32) -> CueCook {
    CueCook {
        name: cue.name.clone(),
        // A reference that names nothing this show has is one recipe
        // that resolves to nothing — the same red pot as a bad group.
        // r[impl effects.library.by-name] - a missing name cooks Empty
        recipes: cue
            .recipes
            .iter()
            .flat_map(|r| {
                let resolved = r.resolve(show);
                if resolved.is_empty() {
                    vec![None]
                } else {
                    resolved.into_iter().map(Some).collect()
                }
            })
            .map(|r| {
                let Some(r) = r else {
                    return Cook::Empty;
                };
                // r[impl recipes.enabled]
                if !r.enabled {
                    return Cook::Disabled;
                }
                // Count fixtures, not emitted values: one recipe can set
                // three colour channels per fixture, and "3 fixtures" is
                // what an operator wants to read.
                let emits = expand_recipe(&r, show, secs);
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

// r[impl recipes.status.visible-per-cue]
// r[impl cues.cooked-status]
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
                ..Default::default()
            }],
            splits: Vec::new(),
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

    // r[verify recipes.template]

    // r[verify groups.resolution-is-live]

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

    // r[verify recipes.status.selects-nothing-is-not-an-error]

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
                ..Default::default()
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

    // r[verify focus.point]

    // r[verify focus.resolve-at-output]

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

    // r[verify color.recall-by-reference]

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
            ..Show::new(&[], &crate::selection::EMPTY_RIG)
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
    // r[verify color.unresolved-is-visible]
    #[test]
    fn an_unknown_palette_name_is_skipped_but_reported() {
        let cue = Cue {
            name: "Oops".to_string(),
            recipes: vec![
                Recipe::new(
                    Selection::Chans(vec![5]),
                    RecipeApply::Color(Ref::Named("Chartreuse".to_string())),
                )
                .into(),
            ],
            ..Default::default()
        };
        let pool = palettes();
        let show = Show {
            groups: &[],
            palettes: &pool,
            rig: &crate::selection::EMPTY_RIG,
            speeds: &NO_SPEEDS,
            roles: &crate::recipe::NO_ROLES,
            ..Show::new(&[], &crate::selection::EMPTY_RIG)
        };
        assert!(
            cue.recipes
                .iter()
                .all(|r| expand_recipe(r.inline().unwrap(), &show, 0.0).is_empty())
        );
        let problems = unresolved(std::slice::from_ref(&cue), &show);
        assert_eq!(problems.len(), 1);
        assert!(problems[0].contains("Chartreuse"), "{problems:?}");
    }

    // -----------------------------------------------------------------
    // Multi-colour presets and focus fans
    // -----------------------------------------------------------------

    fn rgb(name: &str, red: f32, green: f32, blue: f32) -> Ref<ColorPreset> {
        Ref::Inline(ColorPreset {
            name: name.to_string(),
            red,
            green,
            blue,
            ..Default::default()
        })
    }

    fn red_of(emits: &[Emit], chan: ChanId) -> Option<f32> {
        emits
            .iter()
            .find(|e| {
                e.value.chan == chan
                    && e.value.attr
                        == Attribute::ColorAdd {
                            channel: ColorChannel::Red,
                        }
            })
            .map(|e| e.value.value)
    }

    fn red_to_blue(chans: Vec<ChanId>, distribute: Distribute) -> Recipe {
        Recipe::new(
            Selection::Chans(chans),
            RecipeApply::Colors {
                colors: vec![rgb("Red", 1.0, 0.0, 0.0), rgb("Blue", 0.0, 0.0, 1.0)],
                distribute,
            },
        )
    }

    /// The gradient case: ends take the ends, the middle is the mix.
    // r[verify color.multi]
    // r[verify color.multi.distribution]
    // r[verify tricks.spread]
    #[test]
    fn spread_over_three_puts_the_midpoint_colour_on_the_middle_fixture() {
        let emits = expand_recipe(
            &red_to_blue(vec![1, 2, 3], Distribute::Spread),
            &bare(&[]),
            0.0,
        );
        assert_eq!(red_of(&emits, 1), Some(1.0));
        assert_eq!(red_of(&emits, 2), Some(0.5));
        assert_eq!(red_of(&emits, 3), Some(0.0));
    }

    // r[verify color.multi.distribution]
    #[test]
    fn cycle_over_four_with_two_colours_alternates() {
        let emits = expand_recipe(
            &red_to_blue(vec![1, 2, 3, 4], Distribute::Cycle),
            &bare(&[]),
            0.0,
        );
        assert_eq!(red_of(&emits, 1), Some(1.0));
        assert_eq!(red_of(&emits, 2), Some(0.0));
        assert_eq!(red_of(&emits, 3), Some(1.0));
        assert_eq!(red_of(&emits, 4), Some(0.0));
    }

    // r[verify color.multi.distribution]
    #[test]
    fn block_over_four_with_two_colours_gives_halves() {
        let emits = expand_recipe(
            &red_to_blue(vec![1, 2, 3, 4], Distribute::Block),
            &bare(&[]),
            0.0,
        );
        assert_eq!(red_of(&emits, 1), Some(1.0));
        assert_eq!(red_of(&emits, 2), Some(1.0));
        assert_eq!(red_of(&emits, 3), Some(0.0));
        assert_eq!(red_of(&emits, 4), Some(0.0));
    }

    /// Distribution walks the *units* the Tricks made, and in the
    /// selection's order: a blocked pair shares one colour, and the
    /// gradient runs pair to pair.
    // r[verify color.multi.order]
    // r[verify tricks.spread.blocks-are-units]
    #[test]
    fn a_block_trick_plus_spread_gives_pairs_sharing_a_colour() {
        let mut recipe = red_to_blue(vec![1, 2, 3, 4, 5, 6], Distribute::Spread);
        recipe.tricks = vec![crate::tricks::Trick::Block(2)];
        let emits = expand_recipe(&recipe, &bare(&[]), 0.0);
        assert_eq!(red_of(&emits, 1), Some(1.0));
        assert_eq!(red_of(&emits, 2), Some(1.0));
        assert_eq!(red_of(&emits, 3), Some(0.5));
        assert_eq!(red_of(&emits, 4), Some(0.5));
        assert_eq!(red_of(&emits, 5), Some(0.0));
        assert_eq!(red_of(&emits, 6), Some(0.0));
    }

    /// Direction comes from the selection's order, and the recipe has
    /// no second opinion about it.
    ///
    /// "Left to right" is an X-ordered selection. Two authorities on
    /// direction is how a cue ends up with its chase running one way
    /// and its colour spread running the other — so the same recipe,
    /// against the same fixtures ordered the other way, must lead from
    /// the other end.
    ///
    /// r[verify recipes.selection-owns-order]
    #[test]
    fn reordering_the_selection_turns_the_chase_around() {
        use crate::selection::{Dir, FixtureInfo, Order, Rig};
        use crate::selection::Axis;
        use ignition_proto::{Placement, Quat, Vec3};

        let at = |chan, x| FixtureInfo {
            chan,
            placement: Some(Placement {
                position: Vec3 { x, y: 0.0, z: 0.0 },
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
        let rig = Rig::new(vec![at(1, -2.0), at(2, 2.0)]);
        let groups: Vec<Group> = Vec::new();
        let show = Show::new(&groups, &rig);

        // One chase, spread across the selection, ordered two ways.
        let chase = |by: Order| Recipe {
            target: Selection::Order {
                of: Box::new(Selection::Chans(vec![1, 2])),
                by,
            },
            steps: vec![
                Step::new(vec![RecipeApply::Dimmer(1.0)]),
                Step::new(vec![RecipeApply::Dimmer(0.0)]),
            ],
            timing: crate::step::Timing {
                speed: crate::step::Speed::Bpm(60.0),
                measure: 1.0,
                // A full cycle across two fixtures, so the leader is at
                // one end of the wave and the follower at the other.
                phase_spread_deg: 360.0,
                ..Default::default()
            },
            tricks: Vec::new(),
            stack: false,
            ..Default::default()
        };

        let level = |emits: &[Emit], chan| {
            emits
                .iter()
                .find(|e| e.value.chan == chan && e.value.attr == Attribute::Dimmer)
                .map(|e| e.value.value)
        };

        let rightward = expand_recipe(&chase(Order::Axis(Axis::X, Dir::Asc)), &show, 0.0);
        let leftward = expand_recipe(&chase(Order::Axis(Axis::X, Dir::Desc)), &show, 0.0);

        // Whoever leads swaps, and the recipe itself never said so.
        assert_eq!(level(&rightward, 1), level(&leftward, 2));
        assert_eq!(level(&rightward, 2), level(&leftward, 1));
        assert_ne!(
            level(&rightward, 1),
            level(&rightward, 2),
            "the spread put both fixtures at the same point, so this proves nothing"
        );
    }

    /// A cue's status can be read before it is taken.
    ///
    /// "Will this cue do what I think" should be answerable by looking
    /// rather than by running the show — a generator can be wrong, and
    /// finding out during the set is the expensive way. Cooking a cue
    /// touches nothing: no player, no clock, nothing fired.
    ///
    /// r[verify recipes.status.visible-per-cue]
    #[test]
    fn a_cue_can_be_read_before_it_is_ever_taken() {
        let groups = vec![Group {
            name: "Pars".to_string(),
            chans: vec![1, 2, 3],
        }];
        let show = bare(&groups);

        let cue = Cue {
            name: "Wash".into(),
            recipes: vec![
                Recipe::new(
                    Selection::Group("Pars".to_string()),
                    RecipeApply::Dimmer(0.8),
                )
                .into(),
                // And one that names a group this show has never heard
                // of, which is the case worth seeing early.
                Recipe::new(
                    Selection::Group("Nothing Here".to_string()),
                    RecipeApply::Dimmer(0.8),
                )
                .into(),
            ],
            ..Default::default()
        };

        let cooked = cook_cue(&cue, &show, 0.0);
        assert_eq!(cooked.name, "Wash");
        assert_eq!(cooked.recipes.len(), 2, "a cue's recipes are all reported");
        assert!(
            cooked
                .recipes
                .iter()
                .any(|c| matches!(c, Cook::Ok(n) if *n > 0)),
            "the resolving recipe reported no fixtures: {:?}",
            cooked.recipes
        );
        assert!(
            cooked.recipes.contains(&Cook::Empty),
            "the recipe naming an unknown group looked fine, which is the \
             failure this affordance exists to catch: {:?}",
            cooked.recipes
        );
    }

    /// A relative recipe touches the attributes it names and no others.
    ///
    /// A chase that dims says how much to take away; what colour the
    /// fixture is doing at the time is not its business. This is what
    /// makes an effect layerable over a look instead of replacing it —
    /// and it fails invisibly, since a chase that quietly zeroed the
    /// colour looks like a colour that was never set.
    ///
    /// r[verify recipes.relative-leaves-colour-alone]
    #[test]
    fn a_relative_dimmer_chase_says_nothing_about_colour() {
        let recipe = Recipe {
            target: Selection::Chans(vec![1]),
            steps: vec![
                Step::new(vec![RecipeApply::Delta(vec![(Attribute::Dimmer, -0.4)])]),
                Step::new(vec![RecipeApply::Delta(vec![(Attribute::Dimmer, 0.0)])]),
            ],
            timing: crate::step::Timing {
                speed: crate::step::Speed::Bpm(60.0),
                measure: 1.0,
                ..Default::default()
            },
            tricks: Vec::new(),
            stack: false,
            ..Default::default()
        };

        for at in [0.0, 0.25, 0.5, 0.75] {
            let emits = expand_recipe(&recipe, &bare(&[]), at);
            assert!(!emits.is_empty(), "the chase emitted nothing at {at}");
            for Emit { value, .. } in &emits {
                assert_eq!(
                    value.attr,
                    Attribute::Dimmer,
                    "a dimmer chase spoke about {:?} at {at}",
                    value.attr
                );
            }
        }
    }

    /// A blocked pair shares a moment and still aims individually.
    ///
    /// That is what blocking is supposed to mean: phase is decided per
    /// *unit*, but the value is still resolved per fixture, because a
    /// step can say something fixture-relative. A focus point is a
    /// different pan and tilt for every head aiming at it — two heads
    /// hung apart, told to light the same spot, must not be handed the
    /// same angles.
    ///
    /// r[verify effects.phase.values-per-fixture]
    #[test]
    fn a_blocked_pair_shares_a_moment_and_still_aims_individually() {
        use crate::selection::{FixtureInfo, Rig};
        use ignition_proto::{Placement, Quat, Vec3};

        let head = |chan, x| FixtureInfo {
            chan,
            placement: Some(Placement {
                position: Vec3 { x, y: 0.0, z: 5.0 },
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
        // Hung four metres apart, so a shared point is genuinely a
        // different angle for each.
        let rig = Rig::new(vec![head(1, -2.0), head(2, 2.0)]);
        let groups: Vec<Group> = Vec::new();
        let show = Show::new(&groups, &rig);

        let mut recipe = Recipe::new(
            Selection::Chans(vec![1, 2]),
            RecipeApply::FocusPoint(Ref::Inline(Vec3 {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            })),
        );
        // One unit, so the two share a phase slot.
        recipe.tricks = vec![crate::tricks::Trick::Block(2)];

        let emits = expand_recipe(&recipe, &show, 0.0);
        let pan_of = |chan| {
            emits
                .iter()
                .find(|e| e.value.chan == chan && e.value.attr == Attribute::Pan)
                .map(|e| e.value.value)
        };
        let (left, right) = (pan_of(1), pan_of(2));
        assert!(left.is_some() && right.is_some(), "the pair did not aim");
        assert!(
            (left.unwrap() - right.unwrap()).abs() > 1e-3,
            "blocked heads were handed the same pan ({left:?}) — blocking shared the \
             value, not just the moment"
        );
    }

    /// One missing name empties the whole apply, and each missing name
    /// is reported — the same split single `Color` gets.
    // r[verify color.unresolved-is-visible]
    // r[verify color.multi]
    #[test]
    fn a_multi_colour_with_unknown_names_is_skipped_and_each_is_reported() {
        let cue = Cue {
            name: "Rainbow".to_string(),
            recipes: vec![
                Recipe::new(
                    Selection::Chans(vec![1, 2]),
                    RecipeApply::Colors {
                        colors: vec![
                            Ref::Named("House Blue".to_string()),
                            Ref::Named("Chartreuse".to_string()),
                            Ref::Named("Puce".to_string()),
                        ],
                        distribute: Distribute::Cycle,
                    },
                )
                .into(),
            ],
            ..Default::default()
        };
        let pool = palettes();
        let show = Show {
            groups: &[],
            palettes: &pool,
            rig: &crate::selection::EMPTY_RIG,
            speeds: &NO_SPEEDS,
            roles: &crate::recipe::NO_ROLES,
            ..Show::new(&[], &crate::selection::EMPTY_RIG)
        };
        assert!(expand_recipe(cue.recipes[0].inline().unwrap(), &show, 0.0).is_empty());
        let problems = unresolved(std::slice::from_ref(&cue), &show);
        assert_eq!(problems.len(), 2, "{problems:?}");
        assert!(problems.iter().any(|p| p.contains("Chartreuse")));
        assert!(problems.iter().any(|p| p.contains("Puce")));
    }

    /// The on-disk spelling stays a plain word.
    // r[verify color.multi.distribution]
    #[test]
    fn a_multi_colour_round_trips_through_json() {
        let recipe = red_to_blue(vec![1], Distribute::Spread);
        let json = serde_json::to_string(&recipe).unwrap();
        assert!(json.contains(r#""distribute":"spread""#), "{json}");
        let back: Recipe = serde_json::from_str(&json).unwrap();
        assert_eq!(back, recipe);
    }

    fn split_palettes() -> Palettes {
        let red = rgb("Red", 1.0, 0.0, 0.0);
        let blue = rgb("Blue", 0.0, 0.0, 1.0);
        let (Ref::Inline(red), Ref::Inline(blue)) = (red, blue) else {
            unreachable!()
        };
        let split = |name: &str, members: &[&str], distribute| ColorSplit {
            name: name.to_string(),
            colors: members.iter().map(|m| Ref::Named(m.to_string())).collect(),
            distribute,
        };
        Palettes {
            colors: vec![red, blue],
            splits: vec![
                split("Red/Blue", &["Red", "Blue"], Distribute::Spread),
                split("Holey", &["Red", "Puce"], Distribute::Cycle),
                // Nests one level: the outer's colours are the inner's.
                split("Outer", &["Red/Blue"], Distribute::Block),
                // A cycle: each names the other.
                split("Ping", &["Pong"], Distribute::Cycle),
                split("Pong", &["Ping"], Distribute::Cycle),
                split("Selfish", &["Selfish"], Distribute::Cycle),
            ],
            focus: Vec::new(),
        }
    }

    fn split_show(pool: &Palettes) -> Show<'_> {
        Show {
            groups: &[],
            palettes: pool,
            rig: &crate::selection::EMPTY_RIG,
            speeds: &NO_SPEEDS,
            roles: &crate::recipe::NO_ROLES,
            ..Show::new(&[], &crate::selection::EMPTY_RIG)
        }
    }

    fn split_cue(name: &str, chans: Vec<ChanId>) -> Cue {
        Cue {
            name: name.to_string(),
            recipes: vec![
                Recipe::new(
                    Selection::Chans(chans),
                    RecipeApply::Split(Ref::Named(name.to_string())),
                )
                .into(),
            ],
            ..Default::default()
        }
    }

    /// Naming a split gives exactly what writing its colours inline
    /// would: the gradient lands on the same fixtures with the same mix.
    // r[verify color.multi]
    // r[verify color.recall-by-reference]
    #[test]
    fn a_named_split_distributes_like_inline_colours() {
        let pool = split_palettes();
        let show = split_show(&pool);
        let named = expand_recipe(
            split_cue("Red/Blue", vec![1, 2, 3]).recipes[0]
                .inline()
                .unwrap(),
            &show,
            0.0,
        );
        let inline = expand_recipe(&red_to_blue(vec![1, 2, 3], Distribute::Spread), &show, 0.0);
        // Emit order within a channel is not part of the contract.
        let key = |e: &Emit| (e.value.chan, format!("{:?}", e.value.attr));
        let sorted = |mut v: Vec<Emit>| {
            v.sort_by_key(key);
            v
        };
        assert_eq!(sorted(named.clone()), sorted(inline));
        assert_eq!(red_of(&named, 2), Some(0.5));
        assert!(unresolved(&[split_cue("Red/Blue", vec![1])], &show).is_empty());
    }

    /// A split with a hole is skipped whole and the hole is named, with
    /// the cue and the split it sits in.
    // r[verify color.unresolved-is-visible]
    #[test]
    fn a_split_with_a_missing_member_is_skipped_and_reported_by_cue() {
        let pool = split_palettes();
        let show = split_show(&pool);
        let cue = split_cue("Holey", vec![1, 2]);
        assert!(expand_recipe(cue.recipes[0].inline().unwrap(), &show, 0.0).is_empty());
        let problems = unresolved(std::slice::from_ref(&cue), &show);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("\"Holey\"") && problems[0].contains("\"Puce\""));
        let missing = unresolved(&[split_cue("Nope", vec![1])], &show);
        assert_eq!(
            missing,
            vec!["cue \"Nope\": no colour split \"Nope\"".to_string()]
        );
    }

    /// One split may be made of another; the inner colours are spliced in
    /// flat and the outer's own distribution rules.
    // r[verify color.embedding]
    #[test]
    fn a_split_made_of_a_split_takes_the_inner_colours_and_its_own_rule() {
        let pool = split_palettes();
        let show = split_show(&pool);
        let emits = expand_recipe(
            split_cue("Outer", vec![1, 2, 3, 4]).recipes[0]
                .inline()
                .unwrap(),
            &show,
            0.0,
        );
        assert_eq!(red_of(&emits, 2), Some(1.0));
        assert_eq!(red_of(&emits, 3), Some(0.0));
    }

    /// A cycle is a report, not a stack overflow — direct or mutual.
    // r[verify color.embedding]
    // r[verify color.unresolved-is-visible]
    #[test]
    fn a_cyclic_split_is_reported_not_recursed() {
        let pool = split_palettes();
        let show = split_show(&pool);
        for name in ["Ping", "Selfish"] {
            let cue = split_cue(name, vec![1]);
            assert!(expand_recipe(cue.recipes[0].inline().unwrap(), &show, 0.0).is_empty());
            let problems = unresolved(std::slice::from_ref(&cue), &show);
            assert_eq!(problems.len(), 1, "{problems:?}");
            assert!(problems[0].contains("refers to itself"), "{problems:?}");
        }
    }

    /// A chain deeper than `MAX_SPLIT_DEPTH` stops with a report rather
    /// than following it to the bottom.
    // r[verify color.embedding]
    #[test]
    fn nesting_is_bounded_at_max_depth() {
        let mut pool = split_palettes();
        let n = crate::preset::MAX_SPLIT_DEPTH + 3;
        for i in 0..n {
            pool.splits.push(ColorSplit {
                name: format!("L{i}"),
                colors: vec![Ref::Named(if i + 1 < n {
                    format!("L{}", i + 1)
                } else {
                    "Red".to_string()
                })],
                distribute: Distribute::Cycle,
            });
        }
        let show = split_show(&pool);
        let cue = split_cue("L0", vec![1]);
        assert!(expand_recipe(cue.recipes[0].inline().unwrap(), &show, 0.0).is_empty());
        let problems = unresolved(std::slice::from_ref(&cue), &show);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("deeper than"), "{problems:?}");
        // Just short of the bound still resolves.
        let short = split_cue(&format!("L{}", n - crate::preset::MAX_SPLIT_DEPTH), vec![1]);
        assert_eq!(
            red_of(
                &expand_recipe(short.recipes[0].inline().unwrap(), &show, 0.0),
                1
            ),
            Some(1.0)
        );
    }

    /// A named split is a bare string on disk; an inline one is the
    /// object; both come back as written, and so does a palette.
    // r[verify color.multi]
    // r[verify color.recall-by-reference]
    #[test]
    fn a_split_round_trips_through_json() {
        let named = Recipe::new(
            Selection::Chans(vec![1]),
            RecipeApply::Split(Ref::Named("Fire".to_string())),
        );
        let json = serde_json::to_string(&named).unwrap();
        assert!(json.contains(r#""Split":"Fire""#), "{json}");
        assert_eq!(serde_json::from_str::<Recipe>(&json).unwrap(), named);

        let inline = Recipe::new(
            Selection::Chans(vec![1]),
            RecipeApply::Split(Ref::Inline(ColorSplit {
                name: "Ad hoc".to_string(),
                colors: vec![Ref::Named("Red".to_string()), rgb("Blue", 0.0, 0.0, 1.0)],
                distribute: Distribute::Block,
            })),
        );
        let json = serde_json::to_string(&inline).unwrap();
        assert!(json.contains(r#""distribute":"block""#), "{json}");
        assert_eq!(serde_json::from_str::<Recipe>(&json).unwrap(), inline);

        let pool = split_palettes();
        let json = serde_json::to_string(&pool).unwrap();
        assert_eq!(serde_json::from_str::<Palettes>(&json).unwrap(), pool);
        // A palette file from before splits existed still parses.
        let old: Palettes = serde_json::from_str(r#"{"colors":[],"focus":[]}"#).unwrap();
        assert!(old.splits.is_empty());
    }

    /// Three heads in a row along X, all at the same height, with the
    /// fan running from below the first to below the last.
    fn three_in_a_line() -> Rig {
        Rig::new(
            [1, 2, 3]
                .into_iter()
                .map(|chan| FixtureInfo {
                    chan,
                    placement: Some(Placement {
                        position: Vec3 {
                            x: 0.0,
                            y: 0.0,
                            z: 5.0,
                        },
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
                })
                .collect(),
        )
    }

    fn pan_of(emits: &[Emit], chan: ChanId) -> Option<f32> {
        emits
            .iter()
            .find(|e| e.value.chan == chan && e.value.attr == Attribute::Pan)
            .map(|e| e.value.value)
    }

    // r[verify focus.pattern]
    // r[verify focus.pattern.fan]
    // r[verify focus.pattern.order-is-the-selection]
    #[test]
    fn a_fan_over_three_fixtures_pans_the_middle_one_between_the_ends() {
        let recipe = Recipe::new(
            Selection::Chans(vec![1, 2, 3]),
            RecipeApply::FocusFan {
                from: Ref::Inline(Vec3 {
                    x: -4.0,
                    y: 3.0,
                    z: 0.0,
                }),
                to: Ref::Inline(Vec3 {
                    x: 4.0,
                    y: 3.0,
                    z: 0.0,
                }),
            },
        );
        let rig = three_in_a_line();
        let emits = expand_recipe(&recipe, &Show::new(&[], &rig), 0.0);
        let (a, b, c) = (
            pan_of(&emits, 1).unwrap(),
            pan_of(&emits, 2).unwrap(),
            pan_of(&emits, 3).unwrap(),
        );
        assert!((a - c).abs() > 10.0, "the ends should differ: {a} {c}");
        let (lo, hi) = (a.min(c), a.max(c));
        assert!(lo < b && b < hi, "middle {b} not between {lo} and {hi}");
        // The aim is symmetric about the fixtures, so the middle head
        // looks straight ahead.
        assert!(b.abs() < 0.5, "{b}");

        // Reversing the selection reverses the fan: the order is the
        // selection's, not the apply's.
        let mut flipped = recipe.clone();
        flipped.target = Selection::Chans(vec![3, 2, 1]);
        let emits = expand_recipe(&flipped, &Show::new(&[], &rig), 0.0);
        assert!((pan_of(&emits, 3).unwrap() - a).abs() < 0.01);
    }

    // r[verify focus.pattern.fan]
    #[test]
    fn a_fan_with_an_unknown_endpoint_is_skipped_and_reported() {
        let cue = Cue {
            name: "Fan".to_string(),
            recipes: vec![
                Recipe::new(
                    Selection::Chans(vec![7]),
                    RecipeApply::FocusFan {
                        from: Ref::Named("Drums".to_string()),
                        to: Ref::Named("Nowhere".to_string()),
                    },
                )
                .into(),
            ],
            ..Default::default()
        };
        let pool = palettes();
        let rig = one_fixture_at(5.0);
        let show = Show {
            groups: &[],
            palettes: &pool,
            rig: &rig,
            speeds: &NO_SPEEDS,
            roles: &crate::recipe::NO_ROLES,
            ..Show::new(&[], &rig)
        };
        assert!(expand_recipe(cue.recipes[0].inline().unwrap(), &show, 0.0).is_empty());
        let problems = unresolved(std::slice::from_ref(&cue), &show);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("Nowhere"));
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
            stack: false,
            ..Default::default()
        }
    }

    // r[verify recipes.steps-are-the-switch]

    #[test]
    fn one_step_is_static_and_two_is_a_phaser() {
        assert!(!Recipe::new(Selection::Chans(vec![1]), RecipeApply::Dimmer(1.0)).is_phaser());
        assert!(phaser(0.0, 1.0, false).is_phaser());
    }

    // r[verify effects.step]

    // r[verify effects.step.transition]

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
    // r[verify effects.phase.spread]
    // r[verify effects.phase.in-selection-order]
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

    // r[verify effects.step.transition]

    // r[verify effects.interpolate]

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
    // r[verify effects.waveform.is-sugar]
    // r[verify effects.waveform.starts-low]
    // r[verify effects.step.ease]
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
            stack: false,
            ..Default::default()
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
    // r[verify effects.modulates-with-delta]
    #[test]
    fn a_delta_is_marked_relative() {
        let groups = groups();
        let emits = expand_recipe(&phaser(-0.4, 0.0, true), &bare(&groups), 0.1);
        assert!(!emits.is_empty());
        assert!(emits.iter().all(|e| e.relative));
        assert_eq!(dimmer_of(&emits, 1), Some(-0.4));
    }

    // r[verify recipes.steps-are-the-switch]

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

    // r[verify effects.waveform.is-sugar]

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
    // r[verify effects.masters.unknown]
    #[test]
    fn an_unwired_speed_master_is_reported() {
        let mut recipe = phaser(0.0, 1.0, false);
        recipe.timing.speed = Speed::Master("Song".to_string());
        let cue = Cue {
            name: "Chase".to_string(),
            recipes: vec![recipe.into()],
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

    // r[verify recipes.status]

    // r[verify cues.cooked-status]

    #[test]
    fn a_recipe_that_resolves_reports_its_fixture_count() {
        let cue = Cue {
            name: "Wash".into(),
            recipes: vec![
                Recipe::new(
                    Selection::Group("Pars".to_string()),
                    RecipeApply::Color(Ref::Inline(ColorPreset {
                        name: "Red".into(),
                        red: 1.0,
                        green: 0.0,
                        blue: 0.0,
                        ..Default::default()
                    })),
                )
                .into(),
            ],
            ..Default::default()
        };
        let groups = groups();
        let cook = cook_cue(&cue, &bare(&groups), 0.0);
        // Three fixtures, not nine values — a colour sets three
        // channels each and "9" would be a lie.
        assert_eq!(cook.recipes, vec![Cook::Ok(3)]);
        assert_eq!(cook.status(), Status::Cooked);
    }

    // r[verify recipes.status.selects-nothing-is-not-an-error]

    // r[verify recipes.status]

    #[test]
    fn a_recipe_that_selects_nothing_reports_failed() {
        let cue = Cue {
            name: "Typo".into(),
            recipes: vec![
                Recipe::new(
                    Selection::Group("Prs".to_string()),
                    RecipeApply::Dimmer(1.0),
                )
                .into(),
            ],
            ..Default::default()
        };
        let groups = groups();
        let cook = cook_cue(&cue, &bare(&groups), 0.0);
        assert_eq!(cook.recipes, vec![Cook::Empty]);
        assert_eq!(cook.status(), Status::Failed);
    }

    // r[verify recipes.status]

    #[test]
    fn the_pot_colours_distinguish_the_five_cases() {
        let groups = groups();
        let show = bare(&groups);
        let with = |recipes: Vec<Recipe>, values: Vec<CueValue>| {
            cook_cue(
                &Cue {
                    name: "x".into(),
                    recipes: recipes.into_iter().map(Into::into).collect(),
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
            stack: false,
            ..Default::default()
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
    // r[verify effects.play]
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
            stack: false,
            ..Default::default()
        }
    }

    /// The one Eos's own docs single out — "the one most people never
    /// build": fixtures sit at the top of the swing and the travelling
    /// point is the one that drops out.
    // r[verify effects.play]
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
    // r[verify effects.play]
    // r[verify effects.play.build-is-a-mode]
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
    // r[verify effects.play]
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
    // r[verify effects.sync.pure-function]
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

    // -----------------------------------------------------------------
    // Shapes in the room: focus deltas, keyframes, invert, random
    // -----------------------------------------------------------------

    fn v(x: f64, y: f64, z: f64) -> Vec3 {
        Vec3 { x, y, z }
    }

    /// `n` heads on a truss at z = 5, one metre apart along x.
    fn truss(n: usize) -> Rig {
        Rig::new(
            (0..n)
                .map(|i| FixtureInfo {
                    chan: (i + 1) as ChanId,
                    placement: Some(Placement {
                        position: v(i as f64, 0.0, 5.0),
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
                })
                .collect(),
        )
    }

    fn angles(emits: &[Emit], chan: ChanId) -> Option<(f32, f32, bool)> {
        let get = |a: Attribute| {
            emits
                .iter()
                .find(|e| e.value.chan == chan && e.value.attr == a)
                .map(|e| (e.value.value, e.relative))
        };
        let (pan, rel) = get(Attribute::Pan)?;
        let (tilt, _) = get(Attribute::Tilt)?;
        Some((pan, tilt, rel))
    }

    fn one_step(applies: Vec<RecipeApply>, chans: Vec<ChanId>) -> Recipe {
        Recipe {
            target: Selection::Chans(chans),
            steps: vec![Step::new(applies)],
            timing: Timing::default(),
            tricks: Vec::new(),
            stack: false,
            ..Default::default()
        }
    }

    /// A delta in the same step as a point is folded in before solving:
    /// the step emits the absolute angles for `point + delta`, and two
    /// deltas sum.
    /// r[verify focus.delta]
    #[test]
    fn a_delta_beside_a_point_offsets_the_point_before_solving() {
        let rig = truss(1);
        let show = Show::new(&[], &rig);
        let below = v(0.0, 0.0, 0.0);
        let direct = expand_recipe(
            &one_step(
                vec![RecipeApply::FocusPoint(Ref::Inline(v(2.0, 1.0, 0.0)))],
                vec![1],
            ),
            &show,
            0.0,
        );
        let offset = expand_recipe_full(
            &one_step(
                vec![
                    RecipeApply::FocusPoint(Ref::Inline(below)),
                    RecipeApply::FocusDelta(v(2.0, 0.0, 0.0)),
                    RecipeApply::FocusDelta(v(0.0, 1.0, 0.0)),
                ],
                vec![1],
            ),
            &show,
            0.0,
        );
        assert_eq!(angles(&direct, 1), angles(&offset.emits, 1));
        assert_eq!(angles(&offset.emits, 1).unwrap().2, false, "absolute");
        assert!(
            offset.focus_deltas.is_empty(),
            "folded in, nothing left for the player"
        );
        // Straight below plus two metres over is a real tilt.
        assert!(angles(&offset.emits, 1).unwrap().1 > 20.0);
    }

    /// A delta with no point in its step cannot be solved here: it goes
    /// to the player as metres, and no pan/tilt is emitted for it.
    /// r[verify focus.delta]
    #[test]
    fn a_delta_alone_is_handed_to_the_player_in_metres() {
        let rig = truss(2);
        let show = Show::new(&[], &rig);
        let out = expand_recipe_full(
            &one_step(vec![RecipeApply::FocusDelta(v(0.5, 0.0, 0.0))], vec![1, 2]),
            &show,
            0.0,
        );
        assert!(
            out.emits.is_empty(),
            "no angles without an aim: {:?}",
            out.emits
        );
        assert_eq!(out.focus_deltas.len(), 2);
        assert_eq!(
            out.focus_deltas[1],
            FocusDeltaEmit {
                chan: 2,
                delta: v(0.5, 0.0, 0.0)
            }
        );
        // The player's half of the contract solves it per fixture.
        let (pan1, tilt1) =
            crate::focus::resolve_focus_delta(1, v(0.0, 0.0, 0.0), v(0.5, 0.0, 0.0), &rig).unwrap();
        let (pan2, tilt2) =
            crate::focus::resolve_focus_delta(2, v(0.0, 0.0, 0.0), v(0.5, 0.0, 0.0), &rig).unwrap();
        assert!(
            (pan1, tilt1) != (pan2, tilt2),
            "the same metre is a different angle per head"
        );
    }

    /// A path in metres is a path: between two steps the delta is the
    /// blend of the two, so a sixteen-point table is a curve. The same
    /// path at a second venue is the same metres.
    /// r[verify focus.orbit-in-metres]
    /// r[verify focus.delta]
    #[test]
    fn a_metre_path_blends_between_steps_and_is_the_same_at_every_venue() {
        let step = |x: f64| Step {
            apply: vec![RecipeApply::FocusDelta(v(x, 0.0, 0.0))],
            width: 1.0,
            transition: 1.0,
            ..Step::new(Vec::new())
        };
        let recipe = Recipe {
            target: Selection::Chans(vec![1]),
            steps: vec![step(0.0), step(2.0)],
            timing: Timing {
                speed: Speed::Hz(1.0),
                ..Default::default()
            },
            tricks: Vec::new(),
            stack: false,
            ..Default::default()
        };
        let here = truss(1);
        // A quarter cycle in: half way through the first step's transition
        // toward... the step model puts the first step's *arrival* at its
        // start, so read two instants and check the delta moved smoothly.
        let at = |rig: &Rig, secs: f32| {
            expand_recipe_full(&recipe, &Show::new(&[], rig), secs).focus_deltas[0]
                .delta
                .x
        };
        let a = at(&here, 0.0);
        let b = at(&here, 0.25);
        let c = at(&here, 0.5);
        assert!(a != c, "the path moves: {a} {c}");
        assert!(
            (b - (a + c) / 2.0).abs() < 1e-4,
            "a quarter in is the midpoint: {a} {b} {c}"
        );
        // A venue with the heads elsewhere gets the same metres.
        let elsewhere = Rig::new(
            truss(1)
                .fixtures()
                .iter()
                .map(|f| FixtureInfo {
                    placement: Some(Placement {
                        position: v(-4.0, 3.0, 7.0),
                        ..f.placement.clone().unwrap()
                    }),
                    ..f.clone()
                })
                .collect(),
        );
        assert_eq!(at(&here, 0.25), at(&elsewhere, 0.25));
    }

    /// Keyframes: three aims over five heads put the middle head on the
    /// middle aim and the second head half way to it, each solved
    /// through its own placement.
    /// r[verify focus.magic]
    /// r[verify tricks.keyframes]
    #[test]
    fn focus_keyframes_interpolate_aims_along_the_selection() {
        let rig = truss(5);
        let show = Show::new(&[], &rig);
        let points = vec![v(0.0, 0.0, 0.0), v(2.0, 0.0, 0.0), v(4.0, 4.0, 0.0)];
        let key = expand_recipe(
            &one_step(
                vec![RecipeApply::FocusKeyframes(
                    points.iter().cloned().map(Ref::Inline).collect(),
                )],
                (1..=5).collect(),
            ),
            &show,
            0.0,
        );
        let want = |chan: ChanId, target: Vec3| {
            angles(
                &expand_recipe(
                    &one_step(
                        vec![RecipeApply::FocusPoint(Ref::Inline(target))],
                        vec![chan],
                    ),
                    &show,
                    0.0,
                ),
                chan,
            )
        };
        assert_eq!(angles(&key, 1), want(1, points[0]));
        assert_eq!(angles(&key, 2), want(2, v(1.0, 0.0, 0.0)));
        assert_eq!(angles(&key, 3), want(3, points[1]));
        assert_eq!(angles(&key, 4), want(4, v(3.0, 2.0, 0.0)));
        assert_eq!(angles(&key, 5), want(5, points[2]));
        // A missing name empties the apply and is reported.
        let cue = Cue {
            name: "Truss".into(),
            recipes: vec![
                one_step(
                    vec![RecipeApply::FocusKeyframes(vec![Ref::Named(
                        "Nowhere".into(),
                    )])],
                    vec![1],
                )
                .into(),
            ],
            ..Default::default()
        };
        assert!(expand_recipe(cue.recipes[0].inline().unwrap(), &show, 0.0).is_empty());
        assert!(unresolved(&[cue], &show)[0].contains("Nowhere"));
    }

    fn pan_tilt_deltas(emits: &[Emit], chan: ChanId) -> (f32, f32) {
        let get = |a: Attribute| {
            emits
                .iter()
                .find(|e| e.value.chan == chan && e.value.attr == a && e.relative)
                .map(|e| e.value.value)
                .unwrap()
        };
        (get(Attribute::Pan), get(Attribute::Tilt))
    }

    /// `Group(2)` then `Invert(Pan)`: the evens' pan delta flips, their
    /// tilt does not, and the odds are untouched. An absolute value is
    /// never inverted.
    /// r[verify tricks.invert]
    /// r[verify effects.invert]
    #[test]
    fn invert_flips_relative_values_on_the_marked_units() {
        use crate::tricks::{InvertStyle, Trick};
        let recipe = Recipe {
            target: Selection::Chans(vec![1, 2, 3, 4]),
            steps: vec![Step::new(vec![
                RecipeApply::Delta(vec![(Attribute::Pan, 10.0), (Attribute::Tilt, 5.0)]),
                RecipeApply::Dimmer(0.7),
            ])],
            timing: Timing::default(),
            tricks: vec![Trick::Group(2), Trick::Invert(InvertStyle::Pan)],
            stack: false,
            ..Default::default()
        };
        let emits = expand_recipe(&recipe, &bare(&[]), 0.0);
        assert_eq!(pan_tilt_deltas(&emits, 1), (10.0, 5.0));
        assert_eq!(pan_tilt_deltas(&emits, 3), (10.0, 5.0));
        assert_eq!(pan_tilt_deltas(&emits, 2), (-10.0, 5.0));
        assert_eq!(pan_tilt_deltas(&emits, 4), (-10.0, 5.0));
        assert!(
            emits
                .iter()
                .all(|e| e.value.attr != Attribute::Dimmer || e.value.value == 0.7)
        );
        // `All` flips every relative attribute; a delta in metres flips
        // the axes its style names.
        let all = Recipe {
            tricks: vec![Trick::Invert(InvertStyle::All)],
            steps: vec![Step::new(vec![
                RecipeApply::Delta(vec![(Attribute::Dimmer, -0.2)]),
                RecipeApply::FocusDelta(v(1.0, 2.0, 3.0)),
            ])],
            ..recipe
        };
        let out = expand_recipe_full(&all, &bare(&[]), 0.0);
        assert_eq!(dimmer_of(&out.emits, 1), Some(-0.2));
        assert_eq!(dimmer_of(&out.emits, 3), Some(0.2));
        assert_eq!(out.focus_deltas[0].delta, v(1.0, 2.0, 3.0));
        assert_eq!(out.focus_deltas[2].delta, v(-1.0, -2.0, 3.0));
    }

    fn flicker(seed: u32, absolute: bool) -> Random {
        Random {
            attr: Attribute::Dimmer,
            low: -0.3,
            high: 0.2,
            level_var: 0.05,
            speed_var: 0.3,
            attack: 0.2,
            decay: 0.3,
            seed,
            absolute,
            ..Default::default()
        }
    }

    /// The generator is a pure function of (seed, unit, time): the same
    /// question twice gets the same answer, units differ from each
    /// other, seeds differ from each other, and every level stays in
    /// the declared range.
    /// r[verify effects.random]
    /// r[verify effects.sync.pure-function]
    #[test]
    fn random_is_deterministic_per_seed_unit_and_time() {
        let r = flicker(7, false);
        let samples = |unit: usize, seed: u32| -> Vec<f32> {
            let r = flicker(seed, false);
            (0..200).map(|i| r.at(unit, i as f32 * 0.137)).collect()
        };
        assert_eq!(samples(0, 7), samples(0, 7));
        assert_ne!(
            samples(0, 7),
            samples(1, 7),
            "two units ran the same flicker"
        );
        assert_ne!(
            samples(0, 7),
            samples(0, 8),
            "two seeds ran the same flicker"
        );
        for unit in 0..6 {
            for i in 0..500 {
                let level = r.at(unit, i as f32 * 0.0731);
                assert!((-0.35..=0.25).contains(&level), "unit {unit}: {level}");
            }
        }
        // It moves.
        assert!(samples(0, 7).iter().any(|&x| x > 0.0) && samples(0, 7).iter().any(|&x| x < 0.0));
    }

    /// Through a recipe: relative by default, absolute on request, and
    /// the same at a seek as on the way there.
    /// r[verify effects.random]
    #[test]
    fn random_in_a_recipe_is_relative_by_default_and_seekable() {
        let recipe = |absolute: bool| Recipe {
            target: Selection::Chans(vec![1, 2, 3]),
            steps: vec![Step::new(vec![RecipeApply::Random(flicker(3, absolute))])],
            timing: Timing {
                speed: Speed::Hz(2.0),
                ..Default::default()
            },
            tricks: Vec::new(),
            stack: false,
            ..Default::default()
        };
        let rel = expand_recipe(&recipe(false), &bare(&[]), 1.3);
        assert_eq!(rel.len(), 3);
        assert!(
            rel.iter()
                .all(|e| e.relative && e.value.attr == Attribute::Dimmer)
        );
        let abs = expand_recipe(&recipe(true), &bare(&[]), 1.3);
        assert!(abs.iter().all(|e| !e.relative));
        assert_eq!(
            dimmer_of(&rel, 2),
            dimmer_of(&abs, 2),
            "absolute changes the layer, not the number"
        );
        let again = expand_recipe(&recipe(false), &bare(&[]), 1.3);
        assert_eq!(rel, again);
        let later = expand_recipe(&recipe(false), &bare(&[]), 9.7);
        assert_ne!(dimmer_of(&rel, 1), dimmer_of(&later, 1));
    }

    /// The on-disk shapes of everything new here.
    /// r[verify focus.delta]
    /// r[verify focus.magic]
    /// r[verify effects.random]
    #[test]
    fn room_shape_applies_round_trip_as_json() {
        let applies = vec![
            RecipeApply::FocusDelta(v(1.0, 0.0, -0.5)),
            RecipeApply::FocusKeyframes(vec![
                Ref::Named("Drums".into()),
                Ref::Inline(v(0.0, 1.0, 0.0)),
            ]),
            RecipeApply::Random(flicker(11, false)),
        ];
        let json = serde_json::to_string(&applies).unwrap();
        assert!(
            json.contains(r#"{"FocusDelta":{"x":1.0,"y":0.0,"z":-0.5}}"#),
            "{json}"
        );
        assert!(
            json.contains(r#"{"FocusKeyframes":["Drums",{"x":0.0,"y":1.0,"z":0.0}]}"#),
            "{json}"
        );
        assert!(json.contains(r#"{"Random":{"attr":"Dimmer","low":-0.3,"high":0.2,"level_var":0.05,"speed_var":0.3,"attack":0.2,"decay":0.3,"seed":11,"absolute":false}}"#), "{json}");
        let back: Vec<RecipeApply> = serde_json::from_str(&json).unwrap();
        assert_eq!(back, applies);
        // The generator's knobs all default.
        let terse: RecipeApply =
            serde_json::from_str(r#"{"Random":{"attr":"Dimmer","low":0.0,"high":1.0}}"#).unwrap();
        assert!(matches!(
            terse,
            RecipeApply::Random(Random {
                seed: 0,
                absolute: false,
                ..
            })
        ));
    }
}

#[cfg(test)]
mod scope_and_marker_tests {
    use super::*;
    use crate::color::Rgb;
    use crate::preset::{FocusPointPreset, Scope};
    use crate::selection::{FixtureInfo, Rig};
    use ignition_proto::{Placement, Quat};

    fn v(x: f64, y: f64, z: f64) -> Vec3 {
        Vec3 { x, y, z }
    }

    fn rig() -> Rig {
        let fixture = |chan: ChanId, model: &str| FixtureInfo {
            chan,
            placement: Some(Placement {
                position: v(0.0, 0.0, 5.0),
                orientation: Quat {
                    w: 1.0,
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
            }),
            manufacturer: "Uking".into(),
            model: model.into(),
            tags: Vec::new(),
        };
        Rig::new(vec![
            fixture(1, "Par"),
            fixture(2, "Par"),
            fixture(3, "Beam"),
        ])
    }

    fn red_of(emits: &[Emit], chan: ChanId) -> f32 {
        emits
            .iter()
            .find(|e| {
                e.value.chan == chan
                    && e.value.attr
                        == Attribute::ColorAdd {
                            channel: ColorChannel::Red,
                        }
            })
            .map(|e| e.value.value)
            .unwrap_or(-1.0)
    }

    fn palette(preset: ColorPreset) -> Palettes {
        Palettes {
            colors: vec![preset],
            splits: Vec::new(),
            focus: Vec::new(),
        }
    }

    fn colour_cue(apply: RecipeApply) -> Recipe {
        Recipe::new(Selection::Chans(vec![1, 2, 3]), apply)
    }

    /// r[verify color.scope.selective]
    /// r[verify color.scope.fallback-order]
    #[test]
    fn a_selective_preset_lands_per_fixture_and_falls_back_for_the_rest() {
        let mut preset = ColorPreset::rgb("Rainbow", 0.5, 0.5, 0.5);
        preset.scope = Scope::Selective(BTreeMap::from([(1, Rgb::new(1.0, 0.0, 0.0))]));
        let palettes = palette(preset);
        let rig = rig();
        let mut show = Show::new(&[], &rig);
        show.palettes = &palettes;
        let emits = expand_recipe(
            &colour_cue(RecipeApply::Color(Ref::Named("Rainbow".into()))),
            &show,
            0.0,
        );
        assert_eq!(red_of(&emits, 1), 1.0, "its own selective value");
        assert_eq!(red_of(&emits, 2), 0.5, "the universal triple");
        // Through a multi-colour apply and a split the same way.
        let emits = expand_recipe(
            &colour_cue(RecipeApply::Colors {
                colors: vec![Ref::Named("Rainbow".into())],
                distribute: Distribute::Cycle,
            }),
            &show,
            0.0,
        );
        assert_eq!(red_of(&emits, 1), 1.0);
        assert_eq!(red_of(&emits, 3), 0.5);
        let emits = expand_recipe(
            &colour_cue(RecipeApply::Split(Ref::Inline(ColorSplit {
                name: "one".into(),
                colors: vec![Ref::Named("Rainbow".into())],
                distribute: Distribute::Block,
            }))),
            &show,
            0.0,
        );
        assert_eq!(red_of(&emits, 1), 1.0);
        assert_eq!(red_of(&emits, 2), 0.5);
    }

    /// r[verify color.scope.global]
    #[test]
    fn a_global_preset_lands_by_fixture_type() {
        let mut preset = ColorPreset::rgb("Amber", 1.0, 0.5, 0.1);
        preset.scope = Scope::Global(BTreeMap::from([(
            "uking par".to_string(),
            Rgb::new(0.9, 0.6, 0.0),
        )]));
        let palettes = palette(preset);
        let rig = rig();
        let mut show = Show::new(&[], &rig);
        show.palettes = &palettes;
        let emits = expand_recipe(
            &colour_cue(RecipeApply::Color(Ref::Named("Amber".into()))),
            &show,
            0.0,
        );
        assert_eq!(red_of(&emits, 1), 0.9, "the par's type value");
        assert_eq!(red_of(&emits, 2), 0.9);
        assert_eq!(red_of(&emits, 3), 1.0, "the beam takes the universal");
    }

    fn angles(emits: &[Emit], chan: ChanId) -> (f32, f32) {
        let get = |a: Attribute| {
            emits
                .iter()
                .find(|e| e.value.chan == chan && e.value.attr == a)
                .map(|e| e.value.value)
                .unwrap()
        };
        (get(Attribute::Pan), get(Attribute::Tilt))
    }

    /// r[verify focus.marker-moving]
    #[test]
    fn a_recipe_aimed_at_vocal_follows_the_moved_marker() {
        let palettes = Palettes {
            colors: Vec::new(),
            splits: Vec::new(),
            focus: vec![FocusPointPreset {
                name: "Vocal".into(),
                target: v(0.0, 0.0, 0.0),
            }],
        };
        let rig = rig();
        let mut show = Show::new(&[], &rig);
        show.palettes = &palettes;
        let recipe = Recipe::new(
            Selection::Chans(vec![1]),
            RecipeApply::FocusPoint(Ref::Named("Vocal".into())),
        );
        let at_palette = angles(&expand_recipe(&recipe, &show, 0.0), 1);
        assert_eq!(at_palette.1, 0.0, "straight down");

        // The singer walks two metres stage left: the host moves the
        // marker per frame without touching the palette.
        let moved = HashMap::from([("Vocal".to_string(), v(2.0, 0.0, 0.0))]);
        show.focus_overrides = &moved;
        let followed = angles(&expand_recipe(&recipe, &show, 0.0), 1);
        assert_ne!(followed, at_palette);
        assert!(
            followed.1 > 20.0,
            "tilted out to the new spot: {followed:?}"
        );
        assert_eq!(palettes.focus("Vocal").unwrap().target, v(0.0, 0.0, 0.0));
        // A fan and a keyframe set read the same marker.
        let fan = Recipe::new(
            Selection::Chans(vec![1]),
            RecipeApply::FocusFan {
                from: Ref::Named("Vocal".into()),
                to: Ref::Named("Vocal".into()),
            },
        );
        assert_eq!(angles(&expand_recipe(&fan, &show, 0.0), 1), followed);
        // And an override for a name the palette lacks still resolves.
        let cue = Cue {
            name: "x".into(),
            recipes: vec![
                Recipe::new(
                    Selection::Chans(vec![1]),
                    RecipeApply::FocusPoint(Ref::Named("Vocal".into())),
                )
                .into(),
            ],
            ..Default::default()
        };
        assert!(unresolved(&[cue], &show).is_empty());

        // The durable form: move the palette's own marker.
        let mut palettes = palettes;
        palettes.set_focus("Vocal", v(2.0, 0.0, 0.0));
        let show = Show {
            palettes: &palettes,
            ..Show::new(&[], &rig)
        };
        assert_eq!(angles(&expand_recipe(&recipe, &show, 0.0), 1), followed);
        palettes.set_focus("Drums", v(0.0, 3.0, 0.0));
        assert_eq!(palettes.focus("Drums").unwrap().target, v(0.0, 3.0, 0.0));
    }
}

#[cfg(test)]
mod grid_tests {
    use super::*;
    use crate::canvas::{BitmapChannel, CanvasRecipe, Procedural, Quantity, Travel};
    use crate::selection::{Axis, FixtureInfo, Rig};
    use crate::tricks::{GridAxes, Trick, apply_all, apply_all_grid, inverted};
    use ignition_proto::{Placement, Quat, Vec3};

    fn fixture(chan: ChanId, x: f64, y: f64) -> FixtureInfo {
        FixtureInfo {
            chan,
            placement: Some(Placement {
                position: Vec3 { x, y, z: 5.0 },
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
        }
    }

    /// `n` heads on one truss, a metre apart along X.
    fn truss(n: usize) -> Rig {
        Rig::new(
            (0..n)
                .map(|i| fixture(i as ChanId + 1, i as f64, 0.0))
                .collect(),
        )
    }

    /// A 4 × 4 matrix, channel `y * 4 + x + 1` at `(x, y)`.
    fn matrix4() -> Rig {
        let mut f = Vec::new();
        for y in 0..4 {
            for x in 0..4 {
                f.push(fixture((y * 4 + x + 1) as ChanId, x as f64, y as f64));
            }
        }
        Rig::new(f)
    }

    fn chan_at(x: usize, y: usize) -> ChanId {
        (y * 4 + x + 1) as ChanId
    }

    fn dimmer_of(emits: &[Emit], chan: ChanId) -> Option<f32> {
        emits
            .iter()
            .find(|e| e.value.chan == chan && e.value.attr == Attribute::Dimmer)
            .map(|e| e.value.value)
    }

    /// Four steps of dimmer, so a unit's step index is its phase.
    fn four_step(tricks: Vec<Trick>, timing: Timing) -> Recipe {
        let mut r = Recipe::new(
            Selection::Chans((1..=16).collect()),
            RecipeApply::Dimmer(0.1),
        );
        r.steps = (1..=4)
            .map(|i| Step::new(vec![RecipeApply::Dimmer(i as f32 / 10.0)]))
            .collect();
        r.tricks = tricks;
        r.timing = timing;
        r
    }

    /// On a one-truss rig the grid is `[n, 1, 1]` and every quantity
    /// the expansion reads off it — units, inverts, the phase clock,
    /// the Build threshold — is exactly what the one-dimensional
    /// functions return. Checked for every library effect, with its
    /// own Tricks and timing, so today's output is reproduced verbatim.
    // r[verify tricks.grid.degenerate-axes]
    // r[verify tricks.grid]
    // r[verify effects.phase.spread]
    #[test]
    fn a_one_truss_grid_reproduces_the_one_dimensional_path() {
        let rig = truss(8);
        let chans: Vec<ChanId> = (1..=8).collect();
        let show = Show::new(&[], &rig);
        let library = crate::effects::library();
        assert!(!library.is_empty());
        for (name, effect) in &library {
            let mut recipe = effect.clone();
            recipe.target = Selection::Chans(chans.clone());
            let grid = selection_grid(&chans, None, &rig, GridAxes::default());
            assert_eq!(grid.size, [8, 1, 1], "{name}");
            let gu = apply_all_grid(&recipe.tricks, &grid);
            let units = apply_all(&chans, &recipe.tricks);
            assert_eq!(gu.units, units, "{name}: units");
            assert_eq!(
                crate::tricks::inverted_grid(&recipe.tricks, &gu),
                inverted(&recipe.tricks, units.len()),
                "{name}: inverts"
            );
            let count = units.len();
            for (index, pos) in gu.pos.iter().enumerate() {
                assert_eq!(
                    recipe.timing.build_fraction_3d(pos),
                    recipe.timing.spread_fraction(index, count),
                    "{name}: build threshold at {index}"
                );
                for secs in [0.0, 0.37, 1.9, 7.25] {
                    let old = recipe.timing.cycles_at(secs, index, count, show.speeds);
                    let new = recipe.timing.cycles_at_pos(secs, pos, show.speeds);
                    assert!(
                        (old - new).abs() < 1e-6,
                        "{name}: clock at {index}, {secs}s"
                    );
                }
            }
            // And the expansion itself runs, unit for unit.
            let emits = expand_recipe(&recipe, &show, 0.37);
            let touched: std::collections::BTreeSet<ChanId> =
                emits.iter().map(|e| e.value.chan).collect();
            assert!(
                touched.is_empty() || touched.iter().all(|c| chans.contains(c)),
                "{name}"
            );
        }
    }

    /// The selection's order is the order along a row: a right-to-left
    /// selection chases right to left, and fixtures the rig cannot
    /// place still form one row in selection order.
    // r[verify effects.phase.in-selection-order]
    // r[verify groups.one-ordering-authority]
    // r[verify tricks.grid.degenerate-axes]
    #[test]
    fn the_selection_order_is_the_order_along_a_row() {
        let rig = truss(4);
        let backwards = selection_grid(&[4, 3, 2, 1], None, &rig, GridAxes::default());
        assert_eq!(backwards.size, [4, 1, 1]);
        let xs: Vec<(ChanId, usize)> = backwards.cells.iter().map(|c| (c.chan, c.x)).collect();
        assert_eq!(xs, vec![(4, 0), (3, 1), (2, 2), (1, 3)]);

        let unplaced = selection_grid(
            &[7, 9, 8],
            None,
            &crate::selection::EMPTY_RIG,
            GridAxes::default(),
        );
        assert_eq!(unplaced.size, [3, 1, 1]);
        assert_eq!(apply_all_grid(&[], &unplaced).units.flat(), vec![7, 9, 8]);

        // Two rows from the room, each in selection order.
        let two = selection_grid(
            &[8, 7, 6, 5, 1, 2, 3, 4],
            None,
            &matrix4(),
            GridAxes::default(),
        );
        assert_eq!(two.size, [4, 2, 1]);
        let row = |y: usize| -> Vec<ChanId> {
            two.cells
                .iter()
                .filter(|c| c.y == y)
                .map(|c| c.chan)
                .collect()
        };
        assert_eq!(row(0), vec![1, 2, 3, 4]);
        assert_eq!(row(1), vec![8, 7, 6, 5]);
    }

    /// Two trusses at different heights: `OnAxis(Z, Invert(Pan))` runs
    /// the upper truss the other way and leaves the lower alone.
    // r[verify tricks.grid.from-space]
    // r[verify tricks.invert]
    #[test]
    fn a_z_invert_reverses_the_upper_truss() {
        use crate::selection::{FixtureInfo, Rig};
        use crate::tricks::{InvertStyle, Trick};
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
        let rig = Rig::new(vec![
            head(1, 0.0, 3.0),
            head(2, 1.0, 3.0),
            head(11, 0.0, 6.0),
            head(12, 1.0, 6.0),
        ]);
        let recipe = Recipe {
            target: Selection::Chans(vec![1, 2, 11, 12]),
            steps: vec![
                Step::new(vec![RecipeApply::Delta(vec![(Attribute::Pan, 20.0)])]),
                Step::new(vec![RecipeApply::Delta(vec![(Attribute::Pan, -20.0)])]),
            ],
            timing: Timing {
                speed: Speed::Hz(1.0),
                ..Default::default()
            },
            tricks: vec![Trick::OnAxis(
                crate::selection::Axis::Z,
                Box::new(Trick::Invert(InvertStyle::Pan)),
            )],
            ..Default::default()
        };
        let show = Show::new(&[], &rig);
        let emits = expand_recipe(&recipe, &show, 0.1);
        let pan = |chan| {
            emits
                .iter()
                .find(|e| e.value.chan == chan && e.value.attr == Attribute::Pan)
                .map(|e| e.value.value)
                .unwrap()
        };
        assert_eq!(pan(1), pan(2), "the lower truss agrees with itself");
        assert_eq!(pan(11), pan(12), "so does the upper");
        assert_eq!(pan(1), -pan(11), "and the upper runs the other way");
        assert!(pan(1).abs() > 1.0);
    }

    /// `OnAxis(Y, Wings(2))` on a four-row matrix: the top two rows
    /// come back in the other order, so a Y spread reads outward from
    /// the middle — rows 0 and 3 share a phase with the same neighbour.
    // r[verify tricks.grid]
    // r[verify tricks.wings]
    // r[verify effects.phase.spread]
    #[test]
    fn y_wings_mirror_the_rows_of_a_matrix() {
        let rig = matrix4();
        let show = Show::new(&[], &rig);
        let timing = Timing {
            phase_spread_y_deg: 360.0,
            ..Default::default()
        };
        let plain = expand_recipe(&four_step(vec![], timing.clone()), &show, 0.0);
        let wings = expand_recipe(
            &four_step(
                vec![Trick::OnAxis(Axis::Y, Box::new(Trick::Wings(2)))],
                timing,
            ),
            &show,
            0.0,
        );
        // Without the Trick a row's step is its row index.
        for y in 0..4 {
            assert_eq!(
                dimmer_of(&plain, chan_at(0, y)),
                Some((y + 1) as f32 / 10.0)
            );
            assert_eq!(
                dimmer_of(&plain, chan_at(3, y)),
                Some((y + 1) as f32 / 10.0)
            );
        }
        // With it the lower wing is unchanged and the upper wing runs
        // outward: row 3 sits where row 2 was, and row 2 where row 3.
        for x in 0..4 {
            assert_eq!(dimmer_of(&wings, chan_at(x, 0)), Some(0.1));
            assert_eq!(dimmer_of(&wings, chan_at(x, 1)), Some(0.2));
            assert_eq!(dimmer_of(&wings, chan_at(x, 3)), Some(0.3));
            assert_eq!(dimmer_of(&wings, chan_at(x, 2)), Some(0.4));
        }
    }

    /// Equal X and Y spreads on a matrix make a diagonal: every unit on
    /// an anti-diagonal (`x + y` constant) is at the same step, and the
    /// step climbs along either axis.
    // r[verify effects.phase.spread]
    // r[verify tricks.grid]
    #[test]
    fn a_y_spread_makes_a_diagonal_across_a_matrix() {
        let rig = matrix4();
        let show = Show::new(&[], &rig);
        let timing = Timing {
            phase_spread_deg: 360.0,
            phase_spread_y_deg: 360.0,
            ..Default::default()
        };
        let emits = expand_recipe(&four_step(vec![], timing), &show, 0.0);
        let at = |x, y| dimmer_of(&emits, chan_at(x, y)).unwrap();
        assert_eq!(at(0, 0), 0.1);
        assert_eq!(at(1, 0), 0.2);
        assert_eq!(at(0, 1), 0.2);
        assert_eq!(at(2, 0), 0.3);
        assert_eq!(at(1, 1), 0.3);
        assert_eq!(at(0, 2), 0.3);
        assert_eq!(at(3, 0), 0.4);
        assert_eq!(at(2, 1), 0.4);
        assert_eq!(at(1, 2), 0.4);
        assert_eq!(at(0, 3), 0.4);
        // Past the anti-diagonal the phase wraps back round.
        assert_eq!(at(2, 2), 0.1);
        assert_eq!(at(3, 3), 0.3);
        // The same rig with only an X spread is one step per column, no
        // matter the row — the 1-D behaviour, untouched.
        let flat = expand_recipe(
            &four_step(
                vec![],
                Timing {
                    phase_spread_deg: 360.0,
                    ..Default::default()
                },
            ),
            &show,
            0.0,
        );
        for y in 0..4 {
            for x in 0..4 {
                assert_eq!(dimmer_of(&flat, chan_at(x, y)), Some((x + 1) as f32 / 10.0));
            }
        }
    }

    /// A canvas wipe read as dimmer across a 4 × 1 row: the brightest
    /// fixture walks along the row as the clock advances, because each
    /// unit samples the picture at its own grid position.
    // r[verify canvas.bitmap-channels]
    // r[verify canvas.on-the-stack]
    // r[verify canvas.grid]
    #[test]
    fn a_wipe_across_a_row_advances_the_brightest_fixture_with_time() {
        let rig = truss(4);
        let show = Show::new(&[], &rig);
        let canvas = CanvasRecipe {
            source: Procedural::Wipe {
                color: [1.0; 3],
                width: 0.5,
                direction: Travel::Horizontal,
            },
            // One crossing per second.
            timing: Timing {
                speed: Speed::Hz(1.0),
                ..Default::default()
            },
        };
        let channel = BitmapChannel {
            canvas: "row".into(),
            quantity: Quantity::Brightness,
            attr: Attribute::Dimmer,
            low: 0.0,
            high: 1.0,
            relative: false,
        };
        let recipe = Recipe::new(
            Selection::Chans(vec![1, 2, 3, 4]),
            RecipeApply::Canvas {
                recipe: canvas,
                channel: channel.clone(),
            },
        );
        let brightest = |secs: f32| {
            let emits = expand_recipe(&recipe, &show, secs);
            assert_eq!(emits.len(), 4);
            assert!(emits.iter().all(|e| !e.relative));
            emits
                .iter()
                .max_by(|a, b| a.value.value.total_cmp(&b.value.value))
                .map(|e| {
                    assert!(e.value.value > 0.99, "{}", e.value.value);
                    e.value.chan
                })
                .unwrap()
        };
        // Cell centres: the bar sits square on a fixture every quarter
        // cycle, an eighth in.
        assert_eq!(brightest(0.125), 1);
        assert_eq!(brightest(0.375), 2);
        assert_eq!(brightest(0.625), 3);
        assert_eq!(brightest(0.875), 4);
        assert_eq!(brightest(1.125), 1, "and wraps");

        // The channel can say its value is an offset instead.
        let mut relative = recipe.clone();
        relative.steps[0].apply = vec![RecipeApply::Canvas {
            recipe: match &recipe.steps[0].apply[0] {
                RecipeApply::Canvas { recipe, .. } => recipe.clone(),
                _ => unreachable!(),
            },
            channel: BitmapChannel {
                relative: true,
                ..channel
            },
        }];
        let emits = expand_recipe(&relative, &show, 0.0);
        assert_eq!(emits.len(), 4);
        assert!(emits.iter().all(|e| e.relative));
    }
}

#[cfg(test)]
mod pattern_and_control_tests {
    use super::*;
    use crate::preset::FocusPointPreset;
    use crate::selection::{Axis, FixtureInfo, Rig};
    use crate::step::{Speed, Timing};
    use ignition_proto::{Placement, Quat};

    fn v(x: f64, y: f64, z: f64) -> Vec3 {
        Vec3 { x, y, z }
    }

    fn fixture(chan: ChanId, x: f64, model: &str) -> FixtureInfo {
        FixtureInfo {
            chan,
            placement: Some(Placement {
                position: v(x, 0.0, 5.0),
                orientation: Quat {
                    w: 1.0,
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
            }),
            manufacturer: String::new(),
            model: model.to_string(),
            tags: Vec::new(),
        }
    }

    /// Three heads at x = -2, 0, 2, five metres up.
    fn truss() -> Rig {
        Rig::new(vec![
            fixture(1, -2.0, "A"),
            fixture(2, 0.0, "B"),
            fixture(3, 2.0, "B"),
        ])
    }

    fn one_step(apply: RecipeApply) -> Recipe {
        Recipe::new(Selection::Chans(vec![1, 2, 3]), apply)
    }

    fn angles(emits: &[Emit], chan: ChanId) -> Option<(f32, f32)> {
        let get = |attr: Attribute| {
            emits
                .iter()
                .find(|e| e.value.chan == chan && e.value.attr == attr)
                .map(|e| e.value.value)
        };
        get(Attribute::Pan).zip(get(Attribute::Tilt))
    }

    fn cue_of(recipe: Recipe) -> Cue {
        Cue {
            name: "c".into(),
            recipes: vec![recipe.into()],
            ..Default::default()
        }
    }

    /// A splay leans each head outward by its real offset: the centre
    /// head points down, the wings lean opposite ways by equal amounts,
    /// and re-hanging a head changes only its own lean.
    /// r[verify focus.pattern.parallel-out]
    /// r[verify focus.resolve-at-output]
    #[test]
    fn a_splay_opens_the_rig_from_its_real_positions() {
        let rig = truss();
        let show = Show::new(&[], &rig);
        let emits = expand_recipe(
            &one_step(RecipeApply::FocusSplay {
                axis: Axis::X,
                degrees_per_metre: 10.0,
                origin: None,
            }),
            &show,
            0.0,
        );
        let (_, t_centre) = angles(&emits, 2).unwrap();
        let (p_left, t_left) = angles(&emits, 1).unwrap();
        let (p_right, t_right) = angles(&emits, 3).unwrap();
        assert!(t_centre.abs() < 0.5, "centre points down, got {t_centre}");
        assert!((t_left - 20.0).abs() < 0.5, "2 m × 10°/m, got {t_left}");
        assert!((t_right - 20.0).abs() < 0.5);
        assert!(
            (p_left - p_right).abs() > 170.0,
            "wings lean opposite ways: {p_left} vs {p_right}"
        );

        // Re-hang the right head a metre further out: only it changes.
        let moved = Rig::new(vec![
            fixture(1, -2.0, "A"),
            fixture(2, 0.0, "B"),
            fixture(3, 3.0, "B"),
        ]);
        let again = expand_recipe(
            &one_step(RecipeApply::FocusSplay {
                axis: Axis::X,
                degrees_per_metre: 10.0,
                origin: None,
            }),
            &Show::new(&[], &moved),
            0.0,
        );
        assert_eq!(angles(&again, 1), angles(&emits, 1));
        assert!((angles(&again, 3).unwrap().1 - 30.0).abs() < 0.5);
    }

    /// Own entry, then the model's, then the shared default, then
    /// nothing at all.
    /// r[verify focus.pattern.per-fixture]
    #[test]
    fn per_fixture_aims_fall_back_own_then_model_then_shared() {
        let rig = truss();
        let show = Show::new(&[], &rig);
        let apply = |default: Option<Vec3>| RecipeApply::FocusPerFixture {
            aims: BTreeMap::from([(1, v(-2.0, 0.0, 0.0))]),
            models: BTreeMap::from([("B".to_string(), v(0.0, 3.0, 0.0))]),
            default,
        };
        let ex = expand_recipe_full(&one_step(apply(Some(v(9.0, 9.0, 0.0)))), &show, 0.0);
        let point = |chan| {
            ex.focus_points
                .iter()
                .find(|(c, _)| *c == chan)
                .map(|(_, p)| *p)
        };
        assert_eq!(point(1), Some(v(-2.0, 0.0, 0.0)), "own value");
        assert_eq!(point(2), Some(v(0.0, 3.0, 0.0)), "model value");
        assert_eq!(point(3), Some(v(0.0, 3.0, 0.0)));

        let rig = Rig::new(vec![fixture(1, -2.0, "A"), fixture(2, 0.0, "Z")]);
        let show = Show::new(&[], &rig);
        let ex = expand_recipe_full(&one_step(apply(Some(v(9.0, 9.0, 0.0)))), &show, 0.0);
        let p2 = ex
            .focus_points
            .iter()
            .find(|(c, _)| *c == 2)
            .map(|(_, p)| *p);
        assert_eq!(p2, Some(v(9.0, 9.0, 0.0)), "shared default");
        let ex = expand_recipe_full(&one_step(apply(None)), &show, 0.0);
        assert!(
            !ex.focus_points.iter().any(|(c, _)| *c == 2),
            "no entry anywhere: left alone"
        );
    }

    /// An unconstrained axis is the fixture's own coordinate, so fixing
    /// Y and Z lights a line across the stage: each head aims straight
    /// downstage of itself, none of them at x = 0.
    /// r[verify focus.partial-axes]
    #[test]
    fn a_free_axis_is_the_fixtures_own_not_zero() {
        let rig = truss();
        let show = Show::new(&[], &rig);
        let ex = expand_recipe_full(
            &one_step(RecipeApply::FocusAxes {
                x: None,
                y: Some(3.0),
                z: Some(1.0),
                origin: None,
            }),
            &show,
            0.0,
        );
        for (chan, x) in [(1, -2.0), (2, 0.0), (3, 2.0)] {
            let p = ex.focus_points.iter().find(|(c, _)| *c == chan).unwrap().1;
            assert_eq!(p, v(x, 3.0, 1.0));
        }
        let emits = ex.emits;
        // Every head sees the same geometry relative to itself, so the
        // angles agree — a line, not a point.
        assert_eq!(angles(&emits, 1), angles(&emits, 3));
    }

    /// A focus written against a marker moves when the marker moves —
    /// through the palette or a run-time override — for both the
    /// relative point and the constrained axes.
    /// r[verify focus.relative-origin]
    /// r[verify focus.marker-moving]
    #[test]
    fn moving_the_origin_moves_every_focus_written_against_it() {
        let rig = truss();
        let palettes = Palettes {
            focus: vec![FocusPointPreset {
                name: "Riser".into(),
                target: v(1.0, 2.0, 0.5),
            }],
            ..Default::default()
        };
        let show = Show {
            palettes: &palettes,
            ..Show::new(&[], &rig)
        };
        let relative = one_step(RecipeApply::FocusRelative {
            origin: "Riser".into(),
            offset: v(0.0, -1.0, 0.0),
        });
        let axes = one_step(RecipeApply::FocusAxes {
            x: Some(0.5),
            y: None,
            z: Some(0.0),
            origin: Some("Riser".into()),
        });
        let point_of = |ex: &Expansion| ex.focus_points.iter().find(|(c, _)| *c == 2).unwrap().1;
        assert_eq!(
            point_of(&expand_recipe_full(&relative, &show, 0.0)),
            v(1.0, 1.0, 0.5)
        );
        assert_eq!(
            point_of(&expand_recipe_full(&axes, &show, 0.0)),
            v(1.5, 0.0, 0.5)
        );

        let moved = HashMap::from([("Riser".to_string(), v(3.0, 2.0, 0.5))]);
        let show = Show {
            focus_overrides: &moved,
            ..show
        };
        assert_eq!(
            point_of(&expand_recipe_full(&relative, &show, 0.0)),
            v(3.0, 1.0, 0.5)
        );
        assert_eq!(
            point_of(&expand_recipe_full(&axes, &show, 0.0)),
            v(3.5, 0.0, 0.5)
        );

        // A marker that does not exist is reported, not guessed at.
        let bare = Show::new(&[], &rig);
        let problems = unresolved(&[cue_of(relative), cue_of(axes)], &bare);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("no focus origin \"Riser\""));
    }

    /// No apply carries a unit field: metres and degrees are the only
    /// units, and nothing on disk can say otherwise.
    /// r[verify focus.units]
    #[test]
    fn no_focus_apply_carries_a_unit_flag() {
        let applies = vec![
            RecipeApply::FocusPoint(Ref::Inline(v(0.0, 1.0, 2.0))),
            RecipeApply::FocusDirection(v(0.0, 1.0, -1.0)),
            RecipeApply::FocusFan {
                from: Ref::Inline(v(0.0, 0.0, 0.0)),
                to: Ref::Inline(v(1.0, 0.0, 0.0)),
            },
            RecipeApply::FocusKeyframes(vec![Ref::Inline(v(0.0, 0.0, 0.0))]),
            RecipeApply::FocusDelta(v(0.5, 0.0, 0.0)),
            RecipeApply::FocusSplay {
                axis: Axis::X,
                degrees_per_metre: 12.0,
                origin: Some("Drums".into()),
            },
            RecipeApply::FocusPerFixture {
                aims: BTreeMap::from([(1, v(0.0, 0.0, 0.0))]),
                models: BTreeMap::new(),
                default: None,
            },
            RecipeApply::FocusAxes {
                x: Some(1.0),
                y: None,
                z: None,
                origin: None,
            },
            RecipeApply::FocusRelative {
                origin: "Vocal".into(),
                offset: v(0.0, 0.0, 0.0),
            },
        ];
        fn keys(value: &serde_json::Value, out: &mut Vec<String>) {
            match value {
                serde_json::Value::Object(map) => {
                    for (k, v) in map {
                        out.push(k.to_lowercase());
                        keys(v, out);
                    }
                }
                serde_json::Value::Array(list) => list.iter().for_each(|v| keys(v, out)),
                _ => {}
            }
        }
        for apply in applies {
            let json = serde_json::to_value(&apply).unwrap();
            let mut found = Vec::new();
            keys(&json, &mut found);
            assert!(
                !found
                    .iter()
                    .any(|k| k.contains("unit") || k == "metric" || k == "imperial"),
                "{json}"
            );
            let back: RecipeApply = serde_json::from_value(json).unwrap();
            assert_eq!(back, apply);
        }
    }

    /// A disabled recipe stays in the file and does nothing; cooking
    /// says "disabled" rather than "failed".
    /// r[verify recipes.enabled]
    #[test]
    fn a_disabled_recipe_contributes_nothing_and_cooks_disabled() {
        let groups = vec![Group {
            name: "Pars".into(),
            chans: vec![1, 2],
        }];
        let show = Show::new(&groups, &crate::selection::EMPTY_RIG);
        let mut recipe = Recipe::new(Selection::Group("Pars".into()), RecipeApply::Dimmer(1.0));
        assert!(!expand_recipe(&recipe, &show, 0.0).is_empty());
        let json = serde_json::to_string(&recipe).unwrap();
        assert!(
            !json.contains("enabled"),
            "on is the default and unwritten: {json}"
        );

        recipe.enabled = false;
        assert!(expand_recipe(&recipe, &show, 0.0).is_empty());
        let json = serde_json::to_string(&recipe).unwrap();
        assert!(json.contains(r#""enabled":false"#), "{json}");
        let back: Recipe = serde_json::from_str(&json).unwrap();
        assert!(!back.enabled, "survives the file");
        assert_eq!(
            back.steps, recipe.steps,
            "the line is still there to A/B against"
        );

        let cook = cook_cue(&cue_of(recipe), &show, 0.0);
        assert_eq!(cook.recipes, vec![Cook::Disabled]);
        assert_eq!(cook.status(), Status::Empty, "not Failed");
    }

    /// A shared Tricks chain by name lands before the inline one, and an
    /// unknown name is reported.
    /// r[verify tricks.shared-or-inline]
    /// r[verify recipes.tricks]
    #[test]
    fn shared_tricks_by_name_come_before_the_inline_ones() {
        use crate::tricks::Trick;
        let groups = vec![Group {
            name: "Pars".into(),
            chans: vec![1, 2, 3, 4],
        }];
        let pool = BTreeMap::from([("Pairs".to_string(), vec![Trick::Block(2)])]);
        let show = Show {
            named_tricks: &pool,
            ..Show::new(&groups, &crate::selection::EMPTY_RIG)
        };
        let mut recipe = Recipe::new(
            Selection::Group("Pars".into()),
            RecipeApply::Colors {
                colors: vec![
                    Ref::Inline(ColorPreset {
                        red: 1.0,
                        ..Default::default()
                    }),
                    Ref::Inline(ColorPreset {
                        blue: 1.0,
                        ..Default::default()
                    }),
                ],
                distribute: Distribute::Cycle,
            },
        );
        recipe.tricks_ref = Some("Pairs".into());
        recipe.tricks = vec![Trick::Reverse];
        assert_eq!(
            recipe.effective_tricks(&show),
            vec![Trick::Block(2), Trick::Reverse]
        );
        let red = |emits: &[Emit], chan: ChanId| {
            emits
                .iter()
                .find(|e| {
                    e.value.chan == chan
                        && e.value.attr
                            == Attribute::ColorAdd {
                                channel: ColorChannel::Red,
                            }
                })
                .map(|e| e.value.value)
                .unwrap()
        };
        let emits = expand_recipe(&recipe, &show, 0.0);
        // Blocked in pairs, then reversed: 3-4 is the first (red) pair.
        assert_eq!(red(&emits, 3), 1.0);
        assert_eq!(red(&emits, 4), 1.0);
        assert_eq!(red(&emits, 1), 0.0);
        assert_eq!(red(&emits, 2), 0.0);

        let json = serde_json::to_string(&recipe).unwrap();
        assert!(json.contains(r#""tricks_ref":"Pairs""#), "{json}");
        let back: Recipe = serde_json::from_str(&json).unwrap();
        assert_eq!(back.tricks_ref.as_deref(), Some("Pairs"));

        recipe.tricks_ref = Some("Nope".into());
        let problems = unresolved(&[cue_of(recipe)], &show);
        assert!(
            problems
                .iter()
                .any(|p| p.contains("no shared tricks \"Nope\"")),
            "{problems:?}"
        );
    }

    /// Size scales a relative swing about zero and an absolute phaser's
    /// swing about its base, at expansion, for every recipe alike; a
    /// focus delta in metres shrinks the same way.
    /// r[verify effects.size-scales-the-swing]
    /// r[verify effects.masters.uniform]
    #[test]
    fn size_scales_every_swing_at_expansion() {
        let groups = vec![Group {
            name: "Pars".into(),
            chans: vec![1],
        }];
        let rig = truss();
        let base = Show::new(&groups, &rig);
        let delta = Recipe::new(
            Selection::Group("Pars".into()),
            RecipeApply::Delta(vec![(Attribute::Dimmer, -0.5)]),
        );
        let wave = Recipe {
            target: Selection::Group("Pars".into()),
            steps: Waveform::Sine.steps(Attribute::Dimmer, 0.6, 0.4, false),
            timing: Timing {
                speed: Speed::Hz(1.0),
                ..Default::default()
            },
            ..Default::default()
        };
        let orbit = Recipe::new(
            Selection::Chans(vec![1]),
            RecipeApply::FocusDelta(v(2.0, 0.0, 0.0)),
        );
        let dimmer =
            |show: &Show<'_>, r: &Recipe, at: f32| expand_recipe(r, show, at)[0].value.value;

        let half = base.scaled(0.5, 1.0);
        assert!((dimmer(&base, &delta, 0.0) + 0.5).abs() < 1e-6);
        assert!((dimmer(&half, &delta, 0.0) + 0.25).abs() < 1e-6);
        assert!(
            (dimmer(&base.scaled(0.0, 1.0), &delta, 0.0)).abs() < 1e-6,
            "absent at zero"
        );

        // The sine starts at 0.2 (a cycle begins at the low end) — at half
        // size 0.4, at zero size its base 0.6; the base never moves.
        assert!((dimmer(&base, &wave, 0.0) - 0.2).abs() < 1e-6);
        assert!((dimmer(&half, &wave, 0.0) - 0.4).abs() < 1e-6);
        assert!((dimmer(&base.scaled(0.0, 1.0), &wave, 0.0) - 0.6).abs() < 1e-6);
        assert!(
            (dimmer(&half, &wave, 0.5) - 0.8).abs() < 1e-6,
            "high half of a half-size swing"
        );

        let metres = |show: &Show<'_>| expand_recipe_full(&orbit, show, 0.0).focus_deltas[0].delta;
        assert_eq!(metres(&base), v(2.0, 0.0, 0.0));
        assert_eq!(metres(&half), v(1.0, 0.0, 0.0));
    }

    /// One speed scale multiplies every master and touches nothing that
    /// is not slaved to one.
    /// r[verify effects.masters.scale]
    /// r[verify effects.masters.uniform]
    #[test]
    fn the_speed_scale_multiplies_every_master_alike() {
        let groups = vec![Group {
            name: "Pars".into(),
            chans: vec![1],
        }];
        let masters = SpeedMasters::from([("Song".to_string(), 60.0), ("Tap".to_string(), 60.0)]);
        let show = Show {
            speeds: &masters,
            ..Show::new(&groups, &crate::selection::EMPTY_RIG)
        };
        let chase = |speed: Speed| Recipe {
            target: Selection::Group("Pars".into()),
            steps: vec![
                Step::new(vec![RecipeApply::Raw(vec![(Attribute::Dimmer, 0.0)])]),
                Step::new(vec![RecipeApply::Raw(vec![(Attribute::Dimmer, 1.0)])]),
            ],
            timing: Timing {
                speed,
                ..Default::default()
            },
            ..Default::default()
        };
        let at =
            |show: &Show<'_>, r: &Recipe, secs: f32| expand_recipe(r, show, secs)[0].value.value;
        let song = chase(Speed::Master("Song".into()));
        let tap = chase(Speed::Scaled {
            master: "Tap".into(),
            scale: 1.0,
        });
        let hz = chase(Speed::Hz(1.0));
        // 60 BPM = one cycle a second: 0.3 s is still step 0.
        assert_eq!(at(&show, &song, 0.3), 0.0);
        assert_eq!(at(&show, &tap, 0.3), 0.0);
        let double = show.scaled(1.0, 2.0);
        assert_eq!(at(&double, &song, 0.3), 1.0, "0.6 cycles at double speed");
        assert_eq!(at(&double, &tap, 0.3), 1.0);
        assert_eq!(
            at(&double, &hz, 0.3),
            at(&show, &hz, 0.3),
            "Hz is not slaved"
        );
    }

    /// Random is a seeded reorder of the selection, not a play mode: the
    /// same seed gives the same order tomorrow and another seed another.
    /// r[verify effects.play.random]
    #[test]
    fn random_is_a_seeded_shuffle_of_the_selection() {
        use crate::tricks::Trick;
        let groups = vec![Group {
            name: "Pars".into(),
            chans: (1..=8).collect(),
        }];
        let show = Show::new(&groups, &crate::selection::EMPTY_RIG);
        let colours = |seed: u32| {
            let mut r = Recipe::new(
                Selection::Group("Pars".into()),
                RecipeApply::Colors {
                    colors: (0..8)
                        .map(|i| {
                            Ref::Inline(ColorPreset {
                                red: i as f32 / 8.0,
                                ..Default::default()
                            })
                        })
                        .collect(),
                    distribute: Distribute::Cycle,
                },
            );
            r.tricks = vec![Trick::Shuffle(seed)];
            let emits = expand_recipe(&r, &show, 0.0);
            let mut reds: Vec<(ChanId, f32)> = emits
                .iter()
                .filter(|e| {
                    e.value.attr
                        == Attribute::ColorAdd {
                            channel: ColorChannel::Red,
                        }
                })
                .map(|e| (e.value.chan, e.value.value))
                .collect();
            reds.sort_by_key(|(c, _)| *c);
            reds
        };
        assert_eq!(colours(7), colours(7), "recallable");
        assert_ne!(colours(7), colours(8));
        let mut ordered: Vec<f32> = colours(7).into_iter().map(|(_, r)| r).collect();
        ordered.sort_by(|a, b| a.total_cmp(b));
        assert_eq!(
            ordered,
            (0..8).map(|i| i as f32 / 8.0).collect::<Vec<_>>(),
            "a reorder, nothing lost"
        );
        assert!(
            !matches!(Timing::default().direction, Play::Build),
            "no Random play mode exists"
        );
    }

    /// A point outside the declared stage space, and a fixture that
    /// cannot reach its point, are both reported — and both still aim.
    /// r[verify focus.stage-space]
    /// r[verify focus.unreachable]
    #[test]
    fn off_stage_and_unreachable_aims_are_reported_not_refused() {
        let rig = truss();
        let stage = crate::focus::StageSpace {
            origin: v(-5.0, -5.0, 0.0),
            extent: v(10.0, 10.0, 8.0),
        };
        let show = Show {
            stage: Some(&stage),
            ..Show::new(&[], &rig)
        };
        let inside = one_step(RecipeApply::FocusPoint(Ref::Inline(v(0.0, 0.0, 0.0))));
        assert!(unresolved(&[cue_of(inside)], &show).is_empty());
        let outside = one_step(RecipeApply::FocusPoint(Ref::Inline(v(0.0, 40.0, 0.0))));
        let problems = unresolved(&[cue_of(outside.clone())], &show);
        assert!(
            problems
                .iter()
                .any(|p| p.contains("outside the stage space")),
            "{problems:?}"
        );
        assert_eq!(
            cook_cue(&cue_of(outside), &show, 0.0).recipes,
            vec![Cook::Ok(3)],
            "still aims there"
        );

        // Above the heads, behind them: the tilt would need to exceed
        // the 135° a 270° yoke allows.
        let above = one_step(RecipeApply::FocusPoint(Ref::Inline(v(0.0, -0.5, 9.0))));
        let problems = unresolved(&[cue_of(above.clone())], &Show::new(&[], &rig));
        assert!(
            problems.iter().any(|p| p.contains("cannot reach")),
            "{problems:?}"
        );
        assert!(
            !problems.iter().any(|p| p.contains("stage space")),
            "no stage declared"
        );
        assert_eq!(
            cook_cue(&cue_of(above), &Show::new(&[], &rig), 0.0).recipes,
            vec![Cook::Ok(3)]
        );
    }
}

#[cfg(test)]
mod intent_and_sound_tests {
    use super::*;
    use crate::color::{Intent, Rgb};
    use crate::selection::FixtureInfo;

    fn rig() -> Rig {
        let fixture = |chan: ChanId| FixtureInfo {
            chan,
            placement: None,
            manufacturer: "Uking".into(),
            model: "Par".into(),
            tags: Vec::new(),
        };
        Rig::new(vec![fixture(1), fixture(2), fixture(3), fixture(4)])
    }

    fn palette(presets: Vec<ColorPreset>) -> Palettes {
        Palettes {
            colors: presets,
            splits: Vec::new(),
            focus: Vec::new(),
        }
    }

    fn colour_emits(emits: &[Emit], chan: ChanId) -> Vec<&Emit> {
        emits
            .iter()
            .filter(|e| e.value.chan == chan && matches!(e.value.attr, Attribute::ColorAdd { .. }))
            .collect()
    }

    /// r[verify color.intent-to-output]
    #[test]
    fn a_cct_preset_carries_its_intent_on_every_colour_emit() {
        let warm = ColorPreset::from_intent(
            "Warm",
            Intent::Cct {
                kelvin: 3200.0,
                tint: 0.0,
            },
        )
        .unwrap();
        let palettes = palette(vec![warm]);
        let rig = rig();
        let mut show = Show::new(&[], &rig);
        show.palettes = &palettes;
        let recipe = Recipe::new(
            Selection::Chans(vec![1, 2]),
            RecipeApply::Color(Ref::Named("Warm".into())),
        );
        let emits = expand_recipe(&recipe, &show, 0.0);
        let colour = colour_emits(&emits, 1);
        assert_eq!(colour.len(), 3);
        assert!(colour.iter().all(|e| matches!(
            e.intent,
            Some(Intent::Cct { kelvin, .. }) if (kelvin - 3200.0).abs() < 1e-6
        )));
        // A dimmer emit carries none.
        let dim = Recipe::new(Selection::Chans(vec![1]), RecipeApply::Dimmer(0.5));
        assert!(
            expand_recipe(&dim, &show, 0.0)
                .iter()
                .all(|e| e.intent.is_none())
        );
    }

    /// r[verify color.intent-to-output]
    #[test]
    fn an_rgb_only_preset_carries_its_triple_as_the_intent() {
        let palettes = palette(vec![
            ColorPreset::rgb("Red", 1.0, 0.0, 0.0),
            ColorPreset::rgb("Blue", 0.0, 0.0, 1.0),
        ]);
        let rig = rig();
        let mut show = Show::new(&[], &rig);
        show.palettes = &palettes;
        let single = Recipe::new(
            Selection::Chans(vec![1]),
            RecipeApply::Color(Ref::Named("Red".into())),
        );
        let emits = expand_recipe(&single, &show, 0.0);
        assert!(
            colour_emits(&emits, 1)
                .iter()
                .all(|e| e.intent == Some(Intent::Rgb(Rgb::new(1.0, 0.0, 0.0))))
        );
        // Through `Colors` too, each unit with the intent of the colour
        // it was dealt.
        let multi = Recipe::new(
            Selection::Chans(vec![1, 2]),
            RecipeApply::Colors {
                colors: vec![Ref::Named("Red".into()), Ref::Named("Blue".into())],
                distribute: Distribute::Cycle,
            },
        );
        let emits = expand_recipe(&multi, &show, 0.0);
        assert!(
            colour_emits(&emits, 2)
                .iter()
                .all(|e| e.intent == Some(Intent::Rgb(Rgb::new(0.0, 0.0, 1.0))))
        );
        // Through a split as well.
        let split = Recipe::new(
            Selection::Chans(vec![1, 2]),
            RecipeApply::Split(Ref::Inline(ColorSplit {
                name: String::new(),
                colors: vec![Ref::Named("Blue".into()), Ref::Named("Red".into())],
                distribute: Distribute::Cycle,
            })),
        );
        let emits = expand_recipe(&split, &show, 0.0);
        assert!(
            colour_emits(&emits, 1)
                .iter()
                .all(|e| e.intent == Some(Intent::Rgb(Rgb::new(0.0, 0.0, 1.0))))
        );
    }

    /// r[verify color.intent-to-output] - the nearer step's intent between two
    #[test]
    fn a_colour_phaser_carries_the_nearer_steps_intent() {
        let warm = ColorPreset::from_intent(
            "Warm",
            Intent::Cct {
                kelvin: 3200.0,
                tint: 0.0,
            },
        )
        .unwrap();
        let palettes = palette(vec![warm, ColorPreset::rgb("Red", 1.0, 0.0, 0.0)]);
        let rig = rig();
        let mut show = Show::new(&[], &rig);
        show.palettes = &palettes;
        let recipe = Recipe {
            target: Selection::Chans(vec![1]),
            steps: vec![
                Step::new(vec![RecipeApply::Color(Ref::Named("Warm".into()))]),
                Step::new(vec![RecipeApply::Color(Ref::Named("Red".into()))]),
            ],
            timing: Timing {
                speed: Speed::Hz(1.0),
                ..Default::default()
            },
            ..Default::default()
        };
        // Sample the whole cycle: every colour emit carries one of the
        // two intents, never nothing and never a blend.
        for i in 0..20 {
            let emits = expand_recipe(&recipe, &show, i as f32 / 20.0);
            for e in colour_emits(&emits, 1) {
                assert!(
                    matches!(e.intent, Some(Intent::Cct { .. }) | Some(Intent::Rgb(_))),
                    "{:?}",
                    e.intent
                );
            }
        }
    }

    fn sound_show<'a>(rig: &'a Rig, low: f32) -> Show<'a> {
        Show::new(&[], rig).with_sound(SoundLevels {
            low,
            mid: 0.0,
            high: 0.0,
        })
    }

    fn dimmer_of(emits: &[Emit], chan: ChanId) -> Option<&Emit> {
        emits
            .iter()
            .find(|e| e.value.chan == chan && e.value.attr == Attribute::Dimmer)
    }

    /// r[verify playback.sound-as-value]
    #[test]
    fn a_sound_apply_sits_between_low_and_high_on_the_bands_level() {
        let rig = rig();
        let recipe = Recipe::new(
            Selection::Chans(vec![1]),
            RecipeApply::Sound {
                band: Band::Low,
                attr: Attribute::Dimmer,
                low: 0.2,
                high: 1.0,
                relative: false,
            },
        );
        let silent = expand_recipe(&recipe, &sound_show(&rig, 0.0), 0.0);
        assert_eq!(dimmer_of(&silent, 1).unwrap().value.value, 0.2);
        let half = expand_recipe(&recipe, &sound_show(&rig, 0.5), 0.0);
        assert!((dimmer_of(&half, 1).unwrap().value.value - 0.6).abs() < 1e-6);
        let loud = expand_recipe(&recipe, &sound_show(&rig, 1.0), 0.0);
        assert_eq!(dimmer_of(&loud, 1).unwrap().value.value, 1.0);
        assert!(!dimmer_of(&loud, 1).unwrap().relative);
        // A different band reads its own level: mid is silent here.
        let mid = Recipe::new(
            Selection::Chans(vec![1]),
            RecipeApply::Sound {
                band: Band::Mid,
                attr: Attribute::Dimmer,
                low: 0.0,
                high: 1.0,
                relative: true,
            },
        );
        let e = expand_recipe(&mid, &sound_show(&rig, 1.0), 0.0);
        let e = dimmer_of(&e, 1).unwrap();
        assert_eq!(e.value.value, 0.0);
        assert!(e.relative, "a relative sound value is a delta");
    }

    /// r[verify playback.sound-as-value] - a generator's range
    #[test]
    fn a_generators_high_breathes_with_the_band() {
        let rig = rig();
        let random = Random {
            attr: Attribute::Dimmer,
            low: 0.0,
            high: 1.0,
            absolute: true,
            high_from_band: Some(Band::Low),
            ..Default::default()
        };
        let recipe = Recipe {
            target: Selection::Chans(vec![1, 2, 3, 4]),
            steps: vec![Step::new(vec![RecipeApply::Random(random.clone())])],
            timing: Timing {
                speed: Speed::Hz(2.0),
                ..Default::default()
            },
            ..Default::default()
        };
        let silent = expand_recipe(&recipe, &sound_show(&rig, 0.0), 0.3);
        assert!(
            silent.iter().all(|e| e.value.value == 0.0),
            "silence is low"
        );
        let loud = expand_recipe(&recipe, &sound_show(&rig, 1.0), 0.3);
        let plain = Recipe {
            steps: vec![Step::new(vec![RecipeApply::Random(Random {
                high_from_band: None,
                ..random
            })])],
            ..recipe.clone()
        };
        let unheard = expand_recipe(&plain, &sound_show(&rig, 0.0), 0.3);
        assert_eq!(loud, unheard, "full level is the range as written");
        let half = expand_recipe(&recipe, &sound_show(&rig, 0.5), 0.3);
        for (a, b) in half.iter().zip(&loud) {
            assert!((a.value.value - b.value.value * 0.5).abs() < 1e-6);
        }
    }

    /// r[verify playback.sound-as-value] - the wire shape
    #[test]
    fn sound_applies_round_trip_as_json() {
        let apply = RecipeApply::Sound {
            band: Band::Low,
            attr: Attribute::Dimmer,
            low: 0.0,
            high: 1.0,
            relative: true,
        };
        let json = serde_json::to_string(&apply).unwrap();
        assert_eq!(
            json,
            r#"{"Sound":{"band":"Low","attr":"Dimmer","low":0.0,"high":1.0,"relative":true}}"#
        );
        let back: RecipeApply = serde_json::from_str(&json).unwrap();
        assert_eq!(back, apply);
        let terse: RecipeApply = serde_json::from_str(
            r#"{"Sound":{"band":"High","attr":"Dimmer","low":0.0,"high":0.5}}"#,
        )
        .unwrap();
        assert!(matches!(
            terse,
            RecipeApply::Sound {
                relative: false,
                band: Band::High,
                ..
            }
        ));
        let r: Random = serde_json::from_str(
            r#"{"attr":"Dimmer","low":0.0,"high":1.0,"high_from_band":"Mid"}"#,
        )
        .unwrap();
        assert_eq!(r.high_from_band, Some(Band::Mid));
    }

    /// r[verify effects.random] - phase variance, ratio, random start
    #[test]
    fn random_extras_keep_determinism_and_default_to_the_old_shape() {
        let base = Random {
            attr: Attribute::Dimmer,
            low: 0.0,
            high: 1.0,
            absolute: true,
            seed: 7,
            ..Default::default()
        };
        // Defaults: phase fully varied, ratio 1, no random start — the
        // on-disk shape of a terse file is the pre-field behaviour.
        let terse: Random =
            serde_json::from_str(r#"{"attr":"Dimmer","low":0.0,"high":1.0}"#).unwrap();
        assert_eq!(terse.phase_var, 1.0);
        assert_eq!(terse.ratio, 1.0);
        assert_eq!(terse.ratio_var, 0.0);
        assert!(!terse.random_start);
        let json = serde_json::to_string(&terse).unwrap();
        assert!(!json.contains("high_from_band"), "{json}");

        // Ratio 1.0 never drops to low: every sample sits at its rolled
        // level, which with level_var 0 is in [low, high] and, over a
        // long run, above low.
        let samples: Vec<f32> = (0..400).map(|i| base.at(3, i as f32 * 0.137)).collect();
        assert!(samples.iter().all(|v| (0.0..=1.0).contains(v)));
        assert!(samples.iter().filter(|v| **v > 0.0).count() > 380);

        // A ratio of 0.25 spends about three quarters of its time at low.
        let sparse = Random {
            ratio: 0.25,
            ..base.clone()
        };
        let off = (0..4000)
            .filter(|i| sparse.at(3, *i as f32 * 0.00731) == 0.0)
            .count();
        assert!((2600..3400).contains(&off), "{off}");

        // Pure function of (seed, unit, t): same inputs, same answer,
        // for every combination of the new fields.
        for r in [
            Random {
                phase_var: 0.0,
                ..base.clone()
            },
            Random {
                ratio: 0.5,
                ratio_var: 0.3,
                ..base.clone()
            },
            Random {
                random_start: true,
                ..base.clone()
            },
        ] {
            for t in [0.0, 0.4, 2.7, 91.3] {
                assert_eq!(r.at(2, t), r.at(2, t));
            }
        }

        // Phase variance 0: every unit changes level on the same frame.
        let locked = Random {
            phase_var: 0.0,
            ..base.clone()
        };
        let before: Vec<f32> = (0..4).map(|u| locked.at(u, 0.99)).collect();
        let after: Vec<f32> = (0..4).map(|u| locked.at(u, 1.01)).collect();
        assert!(before.iter().zip(&after).all(|(a, b)| a != b));
        // And they are still their own levels, not one shared roll.
        assert!(after.iter().any(|v| *v != after[0]));

        // Random start: a unit reads a different stretch of its sequence.
        let started = Random {
            random_start: true,
            ..base.clone()
        };
        assert!((0..8).any(|u| started.at(u, 0.5) != base.at(u, 0.5)));
    }
}

/// The busking features a cue's reference can carry: a look by name,
/// the fader's effect parameters, an attribute filter — resolved the
/// way a page fader resolves them, so a cue and a key mean one thing.
#[cfg(test)]
mod busking_refs {
    use super::*;
    use crate::group::Group;
    use crate::profile::{Look, LookKind};
    use crate::programmer::{AttrFilter, Fader};

    fn groups() -> Vec<Group> {
        vec![Group {
            name: "Pars".to_string(),
            chans: vec![1, 2],
        }]
    }

    fn pars() -> Selection {
        Selection::Group("Pars".into())
    }

    fn pulse() -> Recipe {
        Recipe {
            target: pars(),
            steps: vec![
                Step::new(vec![RecipeApply::Delta(vec![(Attribute::Dimmer, 0.5)])]),
                Step::new(vec![RecipeApply::Delta(vec![(Attribute::Dimmer, 0.0)])]),
            ],
            timing: Timing {
                speed: Speed::Hz(1.0),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    fn library() -> BTreeMap<String, Recipe> {
        let mut lib = BTreeMap::new();
        lib.insert("pulse".to_string(), pulse());
        lib.insert(
            "lit and aimed".to_string(),
            Recipe::new(
                pars(),
                RecipeApply::Raw(vec![(Attribute::Dimmer, 1.0), (Attribute::Pan, 0.5)]),
            ),
        );
        lib
    }

    fn looks() -> BTreeMap<String, Look> {
        let mut looks = BTreeMap::new();
        looks.insert(
            "bed".to_string(),
            Look {
                kind: LookKind::Bed,
                about: String::new(),
                recipes: vec![
                    RecipeRef::Inline(Recipe::new(pars(), RecipeApply::Dimmer(0.4))),
                    RecipeRef::named("pulse"),
                    // A look inside a look is not followed.
                    RecipeRef::look("bed"),
                ],
            },
        );
        looks
    }

    fn show<'a>(
        groups: &'a [Group],
        library: &'a BTreeMap<String, Recipe>,
        looks: &'a BTreeMap<String, Look>,
    ) -> Show<'a> {
        Show {
            library,
            looks,
            ..Show::new(groups, &crate::selection::EMPTY_RIG)
        }
    }

    /// r[verify profile.looks]
    #[test]
    fn a_cue_takes_a_look_by_name_and_an_unknown_one_is_reported() {
        let (groups, library, looks) = (groups(), library(), looks());
        let show = show(&groups, &library, &looks);
        let taken = RecipeRef::look("bed").resolve(&show);
        assert_eq!(
            taken.len(),
            2,
            "the static recipe and the named one, no recursion"
        );
        assert_eq!(taken[1], pulse());
        assert!(RecipeRef::look("bed").missing(&show).is_empty());
        assert_eq!(
            RecipeRef::look("nope").missing(&show),
            vec!["no look \"nope\"".to_string()]
        );
        assert!(RecipeRef::look("nope").resolve(&show).is_empty());
        let cue = Cue {
            name: "VS 1".into(),
            recipes: vec![RecipeRef::look("nope")],
            ..Default::default()
        };
        assert_eq!(
            unresolved(&[cue], &show),
            vec!["cue \"VS 1\": no look \"nope\"".to_string()]
        );
        // A show that was handed no looks reports the same.
        let none = Show::new(&groups, &crate::selection::EMPTY_RIG);
        assert_eq!(RecipeRef::look("bed").missing(&none).len(), 1);
        // On disk it is `{"look": "bed"}`, beside `{"effect": …}`.
        let json = serde_json::to_string(&RecipeRef::look("bed")).unwrap();
        assert_eq!(json, r#"{"look":"bed"}"#);
        let back: RecipeRef = serde_json::from_str(&json).unwrap();
        assert_eq!(back, RecipeRef::look("bed"));
    }

    /// r[verify profile.effect-parameters]
    /// r[verify playback.effect-parameters]
    #[test]
    fn params_on_a_reference_mean_what_they_mean_on_a_fader() {
        let (groups, library, looks) = (groups(), library(), looks());
        let show = show(&groups, &library, &looks);
        let reference = RecipeRef::named("pulse")
            .with_param("depth", 0.5)
            .with_param("bars", 2.0)
            .with_param("duty", 0.1);
        let resolved = reference.resolve(&show).remove(0);
        assert_eq!(resolved.depth, 0.5);
        assert_eq!(resolved.timing.measure, 8.0);
        assert!((resolved.steps[0].width - 0.1).abs() < 1e-6);
        assert!((resolved.steps[1].width - 0.9).abs() < 1e-6);
        // The library recipe is untouched.
        assert_eq!(library["pulse"], pulse());
        // A fader with the same params plays the same copy.
        let fader = Fader {
            recipe: Some(pulse()),
            params: BTreeMap::from([
                ("depth".to_string(), 0.5),
                ("bars".to_string(), 2.0),
                ("duty".to_string(), 0.1),
            ]),
            ..Default::default()
        };
        assert_eq!(fader.parametrised(&pulse()).into_owned(), resolved);
        // Depth halves the swing at expansion: a +0.5 delta lands as +0.25.
        let emits = expand_recipe(&resolved, &show, 0.0);
        assert!(!emits.is_empty());
        for emit in &emits {
            assert!(emit.relative);
            assert!((emit.value.value - 0.25).abs() < 1e-6, "{emit:?}");
        }
        // On disk: `"params": {"bars": 2.0, …}`; a params-free reference
        // is written exactly as it always was.
        let json = serde_json::to_string(&reference).unwrap();
        assert!(
            json.contains(r#""params":{"bars":2.0,"depth":0.5,"duty":0.1}"#),
            "{json}"
        );
        assert_eq!(
            serde_json::to_string(&RecipeRef::named("pulse")).unwrap(),
            r#"{"effect":"pulse"}"#
        );
        let back: RecipeRef = serde_json::from_str(&json).unwrap();
        assert_eq!(back, reference);
    }

    /// r[verify profile.attribute-filter]
    /// r[verify playback.attribute-filter]
    #[test]
    fn a_filtered_reference_drops_every_emit_outside_the_filter() {
        let (groups, library, looks) = (groups(), library(), looks());
        let show = show(&groups, &library, &looks);
        let colour_only = RecipeRef::named("lit and aimed")
            .filtered(AttrFilter::INTENSITY)
            .resolve(&show)
            .remove(0);
        assert_eq!(colour_only.filter, AttrFilter::INTENSITY);
        let emits = expand_recipe(&colour_only, &show, 0.0);
        assert_eq!(emits.len(), 2, "one dimmer per par, no pan: {emits:?}");
        assert!(emits.iter().all(|e| e.value.attr == Attribute::Dimmer));
        // Unfiltered, the same effect pans.
        let both = RecipeRef::named("lit and aimed").resolve(&show).remove(0);
        assert_eq!(expand_recipe(&both, &show, 0.0).len(), 4);
        // On disk and back.
        let json =
            serde_json::to_string(&RecipeRef::named("lit and aimed").filtered(AttrFilter::COLOUR))
                .unwrap();
        assert!(
            json.contains(
                r#""filter":{"intensity":false,"colour":true,"position":false,"beam":false}"#
            ),
            "{json}"
        );
        let back: RecipeRef = serde_json::from_str(&json).unwrap();
        assert!(
            matches!(back, RecipeRef::Named { filter: Some(f), .. } if f == AttrFilter::COLOUR)
        );
        // A filtered inline recipe round-trips its filter and depth too;
        // an unfiltered one writes neither.
        let mut r = pulse();
        r.filter = AttrFilter::COLOUR;
        r.depth = 0.5;
        let json = serde_json::to_string(&r).unwrap();
        assert!(
            json.contains(r#""depth":0.5"#) && json.contains(r#""filter""#),
            "{json}"
        );
        assert_eq!(serde_json::from_str::<Recipe>(&json).unwrap(), r);
        let plain = serde_json::to_string(&pulse()).unwrap();
        assert!(
            !plain.contains("depth") && !plain.contains("filter"),
            "{plain}"
        );
    }
}
