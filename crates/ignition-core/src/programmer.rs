//! The live layer — what the operator is holding right now.
//!
//! Busking is the primary way this desk is played: pick a group, hit a
//! colour, push a fader. Cue playback sits *underneath* that, not beside
//! it — a cue stack is where a busked look gets recorded to, and what
//! fills in around whatever the operator is not currently touching.
//!
//! Mechanically the programmer is two more layers on top of the cue
//! cascade (`cue.rs`), in the same first-one-wins order:
//!
//! ```text
//!   grand master                 <- scales every intensity, last of all
//!   parks                        <- a value nailed down; above the hand
//!   highlight / lowlight         <- finding a fixture; above even the hand
//!   programmer direct values     <- hit a palette with a group selected
//!   masters and solo             <- per-role limits, not values
//!   flashes, the held look
//!   programmer faders            <- the eight assignable recipes, per page
//!   ---- everything below is the cue player ----
//!   direct values on the cue
//!   recipes on the cue
//! ```
//!
//! Which is why the cascade was worth building properly: busking did not
//! need a new engine, only two more layers and something to own them.
//!
//! Everything the operator punches — a palette, a fader move — arrives
//! over the **program time** (`r[playback.program-time]`), so the
//! programmer keeps a small per-key crossfade and needs to know the show
//! clock. It learns it from `apply_to`, which is called every frame.

use crate::cue::CueValue;
use crate::recipe::{Recipe, RecipeApply, Show, expand_recipe};
use crate::selection::{Selection, resolve};
use crate::step::Speed;
use ignition_proto::{Attribute, ChanId, ColorChannel};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::atomic::{AtomicU32, Ordering};

/// How many assignable faders the surface has.
///
/// Eight because that is what fits under a hand, and because every
/// hardware surface this will ever talk to is a multiple of it.
pub const FADERS: usize = 8;

/// One assignable fader.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Fader {
    pub name: String,
    /// What this fader plays. `None` is an unassigned fader, which is
    /// not the same as one at zero.
    pub recipe: Option<Recipe>,
    /// 0.0–1.0.
    pub level: f32,
    /// A multiple of the recipe's speed master — half, double, ×3 —
    /// applied to this fader only. `1.0` is the master's own tempo.
    ///
    /// `Speed::Scaled` does not exist in `step.rs` at the time of
    /// writing, so this scales the *clock* handed to the recipe, the
    /// same mechanism `Programmer::rate` uses for every fader at once.
    /// The two multiply.
    // r[impl playback.speed-scale] - per-fader clock scale; Speed::Scaled is not yet a variant
    #[serde(default = "one")]
    pub speed_scale: f32,
    /// Further recipes played at the same level — a bundle or a look on
    /// one fader. Empty for the ordinary single-effect fader.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub also: Vec<Recipe>,
    /// Which attribute families this fader may touch. Everything, by
    /// default; a colour fader filtered to colour cannot move a mover
    /// however the library effect was written.
    // r[impl profile.attribute-filter]
    #[serde(default)]
    pub filter: AttrFilter,
    /// Effect parameters applied at fold, by name: `depth` scales the
    /// swing for this fader alone, `bars` sets the loop length, `duty`
    /// sets the first step's share of the cycle. The recipe itself is
    /// never rewritten.
    // r[impl profile.effect-parameters]
    // r[impl playback.effect-parameters]
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, f32>,
    /// A fader that *is* a role's scaling master rather than a recipe —
    /// `FaderSource::Master`. Its level is the master's level.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub master: Option<String>,
}

fn one() -> f32 {
    1.0
}

impl Default for Fader {
    fn default() -> Self {
        Self {
            name: String::new(),
            recipe: None,
            level: 0.0,
            speed_scale: 1.0,
            also: Vec::new(),
            filter: AttrFilter::default(),
            params: BTreeMap::new(),
            master: None,
        }
    }
}

impl Fader {
    /// Every recipe this fader plays, the main one first.
    pub fn recipes(&self) -> impl Iterator<Item = &Recipe> {
        self.recipe.iter().chain(self.also.iter())
    }

    /// `recipe` with this fader's parameters applied — `depth`, `bars`,
    /// `duty` — when it has any; the recipe itself untouched. The same
    /// `recipe::apply_params` a cue's reference goes through, so the
    /// two cannot mean different things.
    // r[impl playback.effect-parameters] - applied to a copy at fold
    pub fn parametrised<'r>(&self, recipe: &'r Recipe) -> std::borrow::Cow<'r, Recipe> {
        if self.params.is_empty() {
            return std::borrow::Cow::Borrowed(recipe);
        }
        let mut out = recipe.clone();
        crate::recipe::apply_params(&mut out, &self.params);
        std::borrow::Cow::Owned(out)
    }

    /// The `depth` parameter, 1 where none is set.
    pub fn depth(&self) -> f32 {
        self.params.get("depth").copied().unwrap_or(1.0).max(0.0)
    }
}

/// Which attribute families a fader may touch.
///
/// Four families, because that is how an operator thinks of a fixture —
/// how bright, what colour, where it points, what the beam is doing —
/// and how a console's filter keys are laid out. A `Custom` attribute
/// belongs to none of them and always passes.
// r[impl profile.attribute-filter]
// r[impl playback.attribute-filter]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttrFilter {
    #[serde(default = "yes")]
    pub intensity: bool,
    #[serde(default = "yes")]
    pub colour: bool,
    #[serde(default = "yes")]
    pub position: bool,
    #[serde(default = "yes")]
    pub beam: bool,
}

fn yes() -> bool {
    true
}

impl Default for AttrFilter {
    fn default() -> Self {
        Self::ALL
    }
}

impl AttrFilter {
    pub const ALL: Self = Self {
        intensity: true,
        colour: true,
        position: true,
        beam: true,
    };

    pub const INTENSITY: Self = Self::only(true, false, false, false);
    pub const COLOUR: Self = Self::only(false, true, false, false);
    pub const POSITION: Self = Self::only(false, false, true, false);
    pub const BEAM: Self = Self::only(false, false, false, true);

    const fn only(intensity: bool, colour: bool, position: bool, beam: bool) -> Self {
        Self {
            intensity,
            colour,
            position,
            beam,
        }
    }

    /// Whether an emit on this attribute passes the filter.
    pub fn admits(&self, attr: &Attribute) -> bool {
        match attr {
            Attribute::Dimmer => self.intensity,
            Attribute::ColorAdd { .. } | Attribute::ColorWheel { .. } => self.colour,
            Attribute::Pan | Attribute::Tilt | Attribute::PanFine | Attribute::TiltFine => {
                self.position
            }
            Attribute::Zoom
            | Attribute::Focus
            | Attribute::Iris
            | Attribute::Strobe
            | Attribute::GoboWheel { .. } => self.beam,
            Attribute::Custom(_) => true,
        }
    }
}

/// How a role master constrains its role's intensity.
///
/// Four modes, per `r[groups.master.modes]`, because "a master at 50%"
/// means four different things and conflating any two of them is how a
/// master at 100% ends up either a no-op or a full-on.
// r[impl groups.master.modes]
// r[impl playback.master-modes]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum MasterMode {
    /// An upper limit: `min(value, level)`. Where a fixture is in two
    /// positive-master roles the *higher* limit applies — MA3's HTP
    /// limiter — so a fixture is only capped by the most generous of
    /// its roles.
    Positive,
    /// An inhibit: `min(value, level)`, and where a fixture is in two
    /// roles the *lowest* wins. This is the one to reach for when a
    /// master has to hold no matter what else the fixture belongs to.
    Negative,
    /// Multiply — a fixture at 50% under a master at 50% outputs 25%.
    /// The default, and what a control that looks like a fader does.
    /// Lowest wins across two roles, per `r[playback.masters-scale]`.
    #[default]
    Scaling,
    /// A hand lift over a running show: the role's dimmer is HTP-merged
    /// with `level`, so the master can *raise* a fixture the show left
    /// dark. Fixtures of the role with no dimmer in the output are
    /// given one at `level`.
    Additive,
}

/// One role master.
// r[impl groups.master]
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Master {
    pub mode: MasterMode,
    /// 0.0–1.0.
    pub level: f32,
}

impl Default for Master {
    fn default() -> Self {
        Self {
            mode: MasterMode::Scaling,
            level: 1.0,
        }
    }
}

/// What a playback key does while it is down.
// r[impl playback.keys]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum KeyAction {
    /// The fader at full while held, back where it was on release.
    Flash,
    /// Latches the fader at full; pressing again unlatches. Release is
    /// ignored.
    Toggle,
    /// This fader at full and every other fader suppressed while held.
    Swap,
    /// This fader to full and every other fader's level set to zero.
    /// Not held — it is a move, not a gesture.
    Kill,
    /// This fader's intensity contribution zeroed while held; every
    /// other attribute it plays carries on.
    Black,
    /// The musical flash: the fader's recipe brought to full over the
    /// program time while held, and back over the same on release. A
    /// flash snaps; a temp *arrives*, so a chorus lift can be a key.
    // r[impl playback.temp-and-pause] - temp
    Temp,
    /// Pauses the Song playback in place; pressing again resumes it.
    // r[impl playback.temp-and-pause] - pause / resume
    Pause,
    /// Loads a cue on the Song playback as the next GO, by the key's
    /// slot index — key 3 loads cue 3.
    Load,
    /// Steps the Song playback back one cue, over its own times.
    // r[impl playback.temp-and-pause] - steppable backwards
    GoBack,
    /// Speed keys on the `Tap` master — see `TapMaster`.
    // r[impl playback.speed-keys]
    Learn,
    HalfSpeed,
    DoubleSpeed,
    ResetSpeed,
}

impl KeyAction {
    /// Whether this key belongs to a fader slot (as opposed to the
    /// transport or the tap master).
    pub fn is_fader_key(self) -> bool {
        matches!(
            self,
            KeyAction::Flash
                | KeyAction::Toggle
                | KeyAction::Swap
                | KeyAction::Kill
                | KeyAction::Black
                | KeyAction::Temp
        )
    }
}

/// The tap-tempo master behind the `Tap` speed master.
///
/// Learn averages rather than takes the last interval, so a hand that
/// is a little early on one beat does not throw every phaser slaved to
/// it. Half and double are *multipliers on the learned tempo*, so
/// tapping again while at half time learns the true tempo and keeps the
/// half; reset drops back to the learned tempo at ×1.
// r[impl playback.speed-keys]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TapMaster {
    /// Show-clock times of the taps in the current run.
    pub taps: Vec<f32>,
    /// The tempo learned from the taps, BPM. `None` until two taps have
    /// landed; the `Tap` master then keeps whatever it had.
    pub learned: Option<f32>,
    /// The multiplier the half/double keys have applied, ×1 at rest.
    pub multiplier: f32,
}

impl Default for TapMaster {
    fn default() -> Self {
        Self {
            taps: Vec::new(),
            learned: None,
            multiplier: 1.0,
        }
    }
}

impl TapMaster {
    /// A gap longer than this starts a new run of taps.
    pub const RESET_GAP: f32 = 2.0;
    /// At most this many intervals are averaged, so the tempo follows a
    /// band that drifts rather than being anchored by its first bar.
    pub const WINDOW: usize = 8;

    /// A tap at show time `now`. The tempo is the average of the last
    /// four to eight intervals; fewer than four are averaged as they
    /// come, so a tempo exists from the second tap.
    // r[impl playback.speed-keys] - learn converges rather than jitters
    pub fn tap(&mut self, now: f32) {
        if let Some(last) = self.taps.last()
            && (now - last > Self::RESET_GAP || now < *last)
        {
            self.taps.clear();
        }
        self.taps.push(now);
        while self.taps.len() > Self::WINDOW + 1 {
            self.taps.remove(0);
        }
        if self.taps.len() >= 2 {
            let intervals = self.taps.len() - 1;
            let span = self.taps[intervals] - self.taps[0];
            if span > 0.0 {
                self.learned = Some(60.0 * intervals as f32 / span);
            }
        }
    }

    // r[impl playback.speed-keys] - half
    pub fn half(&mut self) {
        self.multiplier *= 0.5;
    }

    // r[impl playback.speed-keys] - double
    pub fn double(&mut self) {
        self.multiplier *= 2.0;
    }

    /// Back to the learned tempo at ×1. The learned tempo is kept: reset
    /// is "stop halving", not "forget the song".
    // r[impl playback.speed-keys] - reset to the learned tempo
    pub fn reset(&mut self) {
        self.multiplier = 1.0;
    }

    /// The BPM the `Tap` master should carry, if a tempo has been
    /// learned.
    pub fn bpm(&self) -> Option<f32> {
        self.learned.map(|b| b * self.multiplier)
    }
}

/// A transport request a key made that the programmer cannot carry out
/// itself, because it lands on a playback. Returned by `key_down` and
/// handed to `Playbacks::transport` by the host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    /// Pause if running, resume if paused.
    TogglePause,
    /// Load the cue at this index as the next GO.
    Load(usize),
    GoBack,
}

/// The last show clock the programmer saw, updatable through `&self`.
///
/// Atomic rather than a `Cell` because the programmer lives in a
/// visualizer resource that has to be `Sync`; the bits of an `f32` in a
/// `u32` are the whole trick.
#[derive(Default)]
struct Clock(AtomicU32);

impl Clock {
    fn get(&self) -> f32 {
        f32::from_bits(self.0.load(Ordering::Relaxed))
    }

    fn set(&self, secs: f32) {
        self.0.store(secs.to_bits(), Ordering::Relaxed);
    }
}

impl Clone for Clock {
    fn clone(&self) -> Self {
        Self(AtomicU32::new(self.0.load(Ordering::Relaxed)))
    }
}

impl std::fmt::Debug for Clock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.get())
    }
}

/// One value the operator's hand is holding, mid-crossfade or settled.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Held {
    target: f32,
    /// Where the fade started from. `None` means "from whatever the
    /// layers beneath were producing", resolved at fold time — the
    /// programmer cannot know that at the moment of the punch.
    from: Option<f32>,
    started: f32,
}

/// The live programmer.
#[derive(Debug, Clone, Default)]
// r[impl effects.live-control-on-programmer]
pub struct Programmer {
    /// What the operator currently has selected. Palette hits apply to
    /// this; with nothing selected they do nothing, the same as on a
    /// real desk.
    pub selection: Option<Selection>,
    /// Values the operator set by hand. Top layer — these beat both the
    /// faders and everything the cue stack is doing.
    // r[impl playback.hand-wins]
    values: HashMap<(ChanId, Attribute), Held>,
    /// The same hand, as it was written: every `apply` since the
    /// values were last cleared, target and all — `Role("Wash")` set to
    /// House Blue, not the forty channel floats that became. What a
    /// stored look carries, since a look is recipes against roles.
    // r[impl profile.looks.authored] - the hand remembered as recipes
    applied: Vec<Recipe>,
    /// The eight faders as they are *playing*. Normally a mirror of
    /// `pages[page]`; differs only where a slot is latched to the
    /// previous page's assignment (see `set_page`).
    pub faders: [Fader; FADERS],
    /// Every page of assignments. `faders` plays one of them.
    // r[impl playback.pages]
    pub pages: Vec<[Fader; FADERS]>,
    /// Which page the surface is on.
    pub page: usize,
    /// Slots still playing the previous page's fader because they were
    /// up when the page changed — MA3 calls this a fixed executor
    /// awaiting fader pickup. Cleared when the physical fader is
    /// brought to match the new page's level.
    latched: [bool; FADERS],
    /// Per-slot level glides in flight: where the level came from and
    /// when it left, for the program time.
    level_fades: [Option<(f32, f32)>; FADERS],
    /// Keys currently down, by slot.
    keys_down: [Option<KeyAction>; FADERS],
    /// Slots latched on by a `Toggle` key.
    toggled: [bool; FADERS],
    /// How long a punched value takes to arrive, in beats of `Song`.
    /// Zero snaps.
    // r[impl playback.program-time]
    pub program_time_beats: f32,
    /// The last show clock `apply_to` saw. A punch is stamped with
    /// this, which is how the programmer knows when a fade started
    /// without being handed a clock on every gesture.
    now: Clock,
    /// The Song tempo the last fold ran at, in beats per second — what a
    /// key release between frames measures its temp against.
    last_bps: Clock,
    /// Blind: values are held and shown in `preview_output` but do not
    /// reach `apply_to`.
    // r[impl playback.blind]
    pub blind: bool,
    /// The current selection to open white at full, above everything.
    // r[impl playback.highlight]
    pub highlight: bool,
    /// Everything *not* selected capped at `lowlight_floor`.
    // r[impl playback.highlight] - lowlight
    pub lowlight: bool,
    pub lowlight_floor: f32,
    /// How far every effect swings, 0..=1.
    ///
    /// The control an operator holds for most of a night. Distinct from
    /// a master or a dimmer, and the distinction is the whole point: a
    /// master scales what the fixtures *output*, size scales how far an
    /// effect *swings*. Halving a master makes a chase dimmer overall;
    /// halving size makes it flatter while the look underneath stays
    /// exactly where it was. Conflating them makes an effect impossible
    /// to withdraw from a look without dimming the look too.
    ///
    /// At zero every effect is inert and whatever is beneath shows
    /// through unchanged — which is what makes this a *withdrawal*
    /// rather than a blackout.
    // r[impl recipes.size]
    // r[impl recipes.size.is-not-intensity]
    // r[impl recipes.live-control-is-not-stored] - operator state, not on the recipe
    // r[impl effects.live-control-on-programmer]
    pub size: f32,
    /// A multiplier on every effect's rate, against its speed master.
    ///
    /// The tap master already sets the tempo; this is how one operator
    /// runs the rig at half or double it without a second recipe for
    /// every entry in the library.
    // r[impl recipes.rate]
    // r[impl recipes.live-control-is-not-stored]
    // r[impl effects.live-control-on-programmer]
    pub rate: f32,
    /// Per-role intensity masters, by role name.
    ///
    /// A busking operator's grip on a rig a cue list is otherwise
    /// driving: pull `Movers` down for a ballad without editing a cue.
    /// Each carries a mode, per `r[groups.master.modes]`; the default is
    /// scaling, which is what an operator expects from something that
    /// behaves like a fader.
    // r[impl groups.master]
    // r[impl groups.master.modes]
    // r[impl playback.masters-scale]
    // r[impl playback.master-modes]
    pub masters: BTreeMap<String, Master>,
    /// A role played on its own, everything else pulled down.
    ///
    /// What a solo button does. Not a selection — selection is what the
    /// *next* palette hit lands on, and a solo has to change the output
    /// without changing what is armed, or hitting solo would silently
    /// redirect the operator's next move.
    pub solo: Option<String>,
    /// How far down the un-soloed rig goes. Not to zero by default: a
    /// solo that blacks the room reads as a fault, where one that leaves
    /// a floor under it reads as a decision.
    pub solo_floor: f32,
    /// Bumps fired by hand and still ringing, with the show time each
    /// started.
    ///
    /// Transient rather than a fader, because a flash key is an *event*.
    /// Parked on a fader it would need a release to arrive, and a flash
    /// whose release is dropped — a slipped hand, a lost MIDI note-off —
    /// leaves the rig stuck bright. A one-shot with its own start time
    /// cannot do that.
    // r[impl effects.bump.is-not-held]
    // r[impl triggers.own-clock] - each flash carries its own start time
    flashes: Vec<(Recipe, f32)>,
    /// A look held under a key for exactly as long as the key is down —
    /// the punt look, or a momentary rig drop.
    ///
    /// Not a fader, because a fader is *positioned* and this is *held*:
    /// the operator's hand is the release, and a punt look that stayed
    /// on after the hand left would be a cue, not a punt. Not a flash
    /// either, because a flash has an envelope and retires itself, and
    /// this has to stay for as long as the trouble lasts. One slot, since
    /// a hand holds one key.
    // r[impl effects.bump.is-not-held] - the held look is the thing a bump is not
    // r[impl playback.look-hold] - a look is several recipes held as one
    held: Vec<Recipe>,
    /// Effects a macro fired by name, beside the faders: the name, the
    /// recipe, its weight and when it started. Released by the macro's
    /// release step, never by a fader move.
    // r[impl playback.macro-effects]
    extras: Vec<Extra>,
    /// Every intensity to zero, protected roles aside — what a macro's
    /// blackout step sets and its release clears. Arrives over the
    /// program time like everything else the desk does.
    // r[impl playback.blackout]
    pub blackout: bool,
    /// When `blackout` last changed, for the fade.
    blackout_since: Option<f32>,
    /// Roles the black key, a blackout, a `Safe` held look and the
    /// grand master leave alone — from the profile.
    // r[impl profile.protected-roles]
    // r[impl playback.protected-untouched]
    pub protected: Vec<String>,
    /// Whether the held look is a `Safe` one, which protected roles
    /// pass through. A punt look is not safe in this sense: house lights
    /// under a punt are whatever the punt says.
    held_safe: bool,
    /// Attributes nailed to a value above every playback and the hand.
    ///
    /// A motor screaming on one tilt is fixed at the desk in a second by
    /// parking it. Applied last in the fold — after the hand and after
    /// highlight — because the whole point is that nothing else can
    /// move it. Only the grand master reaches past a park, and only for
    /// intensity.
    /// A `HashMap` rather than a `BTreeMap` only because `Attribute` is
    /// not `Ord`; the fold does not depend on order, since parks never
    /// overlap.
    // r[impl playback.park]
    pub parked: HashMap<(ChanId, Attribute), f32>,
    /// Raw DMX channels parked at a byte, keyed `(universe, channel)`.
    ///
    /// Not applied here: this layer works in attribute space and a raw
    /// channel has none. The DMX stage (the visualizer's encode) writes
    /// these over the encoded frame, *after* encoding — so a parked
    /// channel also ignores the grand master. That is what "park a DMX
    /// channel" has to mean: the value on the wire.
    // r[impl playback.park] - a DMX channel; applied after encode by the host
    pub parked_dmx: BTreeMap<(u16, u16), u8>,
    /// The grand master, 0..=1. Scales every intensity last of all —
    /// after parks of intensity too — so the whole rig comes down with
    /// one hand no matter what is holding a fixture. Nothing but the
    /// dimmer is touched.
    // r[impl playback.grand-master]
    pub grand: f32,
    /// Masters keyed by a selection rather than a role — "the four
    /// movers I just grabbed". Same modes as `masters`, folded together
    /// with them.
    // r[impl playback.selection-master]
    pub selection_masters: Vec<(Selection, Master)>,
    /// The tap master behind the `Tap` speed master.
    // r[impl playback.speed-keys]
    pub tap: TapMaster,
    /// Per-slot temp key state: when the key went down, and — once
    /// released — the level it had reached and when.
    temp_down: [Option<f32>; FADERS],
    temp_release: [Option<(f32, f32)>; FADERS],
}

/// An effect a macro fired by name — see `Programmer::extras`.
#[derive(Debug, Clone, PartialEq)]
struct Extra {
    name: String,
    recipe: Recipe,
    level: f32,
    started: f32,
}

/// How close a latched fader has to come to the new page's level to
/// pick it up. Wide enough that a hand finds it, narrow enough that
/// sweeping through does not.
const PICKUP: f32 = 0.05;

impl Programmer {
    pub fn new() -> Self {
        Self {
            // Effects at full and no masters pulled: a desk that starts
            // with its controls somewhere other than neutral makes every
            // first look a surprise.
            size: 1.0,
            rate: 1.0,
            solo_floor: 0.15,
            lowlight_floor: 0.1,
            grand: 1.0,
            pages: vec![Default::default()],
            ..Self::default()
        }
    }

    // ── parks ────────────────────────────────────────────────────────

    /// Parks one attribute of every fixture in `selection` at `value`.
    // r[impl playback.park]
    pub fn park(&mut self, selection: &Selection, attr: Attribute, value: f32, show: &Show<'_>) {
        for chan in crate::selection::resolve_with(selection, show.groups, show.rig, show.roles) {
            self.parked.insert((chan, attr.clone()), value);
        }
    }

    /// Parks one attribute of one fixture.
    pub fn park_chan(&mut self, chan: ChanId, attr: Attribute, value: f32) {
        self.parked.insert((chan, attr), value);
    }

    /// Unparks one attribute of every fixture in `selection`.
    // r[impl playback.park] - and unpark it
    pub fn unpark(&mut self, selection: &Selection, attr: &Attribute, show: &Show<'_>) {
        for chan in crate::selection::resolve_with(selection, show.groups, show.rig, show.roles) {
            self.parked.remove(&(chan, attr.clone()));
        }
    }

    pub fn unpark_chan(&mut self, chan: ChanId, attr: &Attribute) {
        self.parked.remove(&(chan, attr.clone()));
    }

    /// Parks a raw DMX channel — see `parked_dmx`.
    // r[impl playback.park] - a DMX channel
    pub fn park_dmx(&mut self, universe: u16, channel: u16, value: u8) {
        self.parked_dmx.insert((universe, channel), value);
    }

    pub fn unpark_dmx(&mut self, universe: u16, channel: u16) {
        self.parked_dmx.remove(&(universe, channel));
    }

    /// Writes the DMX parks over an encoded universe. For the DMX
    /// stage to call after encode; `channel` is 1-based as on the wire.
    // r[impl playback.park] - a parked channel is the value on the wire
    pub fn apply_parked_dmx(&self, universe: u16, frame: &mut [u8]) {
        for ((u, channel), value) in &self.parked_dmx {
            if *u == universe
                && let Some(slot) = (*channel as usize)
                    .checked_sub(1)
                    .and_then(|i| frame.get_mut(i))
            {
                *slot = *value;
            }
        }
    }

    // ── masters ──────────────────────────────────────────────────────

    /// Sets the grand master, 0..=1.
    // r[impl playback.grand-master]
    pub fn set_grand(&mut self, level: f32) {
        self.grand = level.clamp(0.0, 1.0);
    }

    /// Sets a master on a selection, replacing one on an equal
    /// selection.
    // r[impl playback.selection-master]
    pub fn set_selection_master(&mut self, selection: Selection, master: Master) {
        match self
            .selection_masters
            .iter_mut()
            .find(|(s, _)| *s == selection)
        {
            Some((_, m)) => *m = master,
            None => self.selection_masters.push((selection, master)),
        }
    }

    pub fn clear_selection_master(&mut self, selection: &Selection) {
        self.selection_masters.retain(|(s, _)| s != selection);
    }

    /// The speed masters as this programmer's tap sees them: `base` with
    /// `Tap` set to the learned tempo, when there is one. The host
    /// builds its `Show::speeds` from this so the `Tap` master a recipe
    /// names is the one the keys drive.
    // r[impl playback.speed-keys] - the Tap master's value comes from the tap master
    pub fn speeds_for(&self, base: &crate::step::SpeedMasters) -> crate::step::SpeedMasters {
        let mut out = base.clone();
        if let Some(bpm) = self.tap.bpm() {
            out.insert("Tap".into(), bpm);
        }
        out
    }

    /// The show clock as of the last fold.
    pub fn now(&self) -> f32 {
        self.now.get()
    }

    /// Stamps the clock without folding — for a caller that punches
    /// values before the first frame, or between frames.
    pub fn set_now(&mut self, secs: f32) {
        self.now.set(secs);
    }

    /// Puts a role on its own until cleared.
    pub fn solo(&mut self, role: &str) {
        self.solo = Some(role.to_string());
    }

    pub fn clear_solo(&mut self) {
        self.solo = None;
    }

    /// Fires a bump by hand.
    ///
    /// `now` is show time, so the envelope runs from this moment rather
    /// than from wherever the shared clock happens to be — the same rule
    /// the cue player applies to one-shots, and for the same reason: an
    /// envelope evaluated against a clock that started hours ago has
    /// already finished before it is seen.
    // r[impl playback.flash-equals-hit] - built by the same `bump::bump` a charted hit uses
    // r[impl effects.bump.one-object]
    // r[impl effects.bump.is-not-held]
    pub fn flash(&mut self, target: Selection, kind: crate::bump::Kind, now: f32) {
        self.flashes
            .push((crate::bump::bump(target, kind, 1.0), now));
        // A cap rather than unbounded growth: `retire_flashes` clears
        // finished ones every frame, so reaching this means something is
        // not calling it, and dropping the oldest is better than growing
        // for the rest of the night.
        while self.flashes.len() > 16 {
            self.flashes.remove(0);
        }
    }

    /// Holds a look at full until `release_hold` — a key that is down.
    pub fn hold(&mut self, recipe: Recipe) {
        self.held = vec![recipe];
        self.held_safe = false;
    }

    /// Holds several recipes as one look — a profile look — latched
    /// until released or replaced. `safe` marks a look protected roles
    /// pass through.
    // r[impl playback.look-hold]
    pub fn hold_look(&mut self, recipes: Vec<Recipe>, safe: bool) {
        self.held = recipes;
        self.held_safe = safe;
    }

    pub fn release_hold(&mut self) {
        self.held.clear();
        self.held_safe = false;
    }

    pub fn is_holding(&self) -> bool {
        !self.held.is_empty()
    }

    /// Plays an effect on the macro layer at `level`, replacing one of
    /// the same name. Its clock starts now, so an envelope runs from
    /// the moment it was fired.
    // r[impl playback.macro-effects]
    pub fn take_effect(&mut self, name: &str, recipe: Recipe, level: f32) {
        let started = self.now.get();
        self.extras.retain(|e| e.name != name);
        self.extras.push(Extra {
            name: name.to_string(),
            recipe,
            level: level.clamp(0.0, 1.0),
            started,
        });
    }

    pub fn release_effect(&mut self, name: &str) {
        self.extras.retain(|e| e.name != name);
    }

    pub fn release_effects(&mut self) {
        self.extras.clear();
    }

    /// The names on the macro layer, in the order they were fired.
    pub fn effects_playing(&self) -> Vec<&str> {
        self.extras.iter().map(|e| e.name.as_str()).collect()
    }

    /// Sets the blackout; the fade runs from now.
    // r[impl playback.blackout]
    pub fn set_blackout(&mut self, on: bool) {
        if self.blackout != on {
            self.blackout = on;
            self.blackout_since = Some(self.now.get());
        }
    }

    /// Lets go of everything a macro can take: the held look, the
    /// macro effects, the blackout. Faders, the hand, parks and masters
    /// are not the macro's to release.
    // r[impl profile.macros.release]
    pub fn release_macro(&mut self) {
        self.release_hold();
        self.release_effects();
        self.set_blackout(false);
    }

    /// The fixtures of every protected role, resolved once per fold.
    fn protected_chans(&self, show: &Show<'_>) -> HashSet<ChanId> {
        self.protected
            .iter()
            .flat_map(|role| {
                crate::selection::resolve_with(
                    &Selection::Role(role.clone()),
                    show.groups,
                    show.rig,
                    show.roles,
                )
            })
            .collect()
    }

    /// Drops bumps whose envelope has run out.
    // r[impl recipes.finished-one-shot-withdraws] - flashes
    pub fn retire_flashes(&mut self, show: &Show<'_>, now: f32) {
        self.flashes
            .retain(|(recipe, started)| recipe.timing.cycles(now - started, show.speeds) < 1.0);
        // A blackout that has fully faded back out has no fade left to
        // run; forgetting when it started is what stops the fold
        // resolving the protected roles every frame for the rest of
        // the night.
        if !self.blackout
            && let Some(since) = self.blackout_since
            && self.progress(show, now - since) >= 1.0
        {
            self.blackout_since = None;
        }
    }

    /// Sets a role's intensity master, 0..=1, keeping its mode (scaling
    /// for a role that had none).
    // r[impl groups.master]
    pub fn set_master(&mut self, role: &str, level: f32) {
        self.masters.entry(role.to_string()).or_default().level = level.clamp(0.0, 1.0);
    }

    /// Sets a role master's mode, keeping its level.
    // r[impl groups.master.modes]
    // r[impl playback.master-modes]
    pub fn set_master_mode(&mut self, role: &str, mode: MasterMode) {
        self.masters.entry(role.to_string()).or_default().mode = mode;
    }

    pub fn select(&mut self, selection: Selection) {
        self.selection = Some(selection);
    }

    pub fn deselect(&mut self) {
        self.selection = None;
    }

    /// Everything the operator has set by hand, as cue values — this is
    /// what "record" writes into a cue. The *target* of any fade still
    /// in flight, because that is what the operator asked for.
    pub fn captured(&self) -> Vec<CueValue> {
        let mut out: Vec<CueValue> = self
            .values
            .iter()
            .map(|((chan, attr), held)| CueValue {
                chan: *chan,
                attr: attr.clone(),
                value: held.target,
            })
            .collect();
        // Stable order so a recorded cue does not churn in git purely
        // because a HashMap iterated differently.
        out.sort_by(|a, b| {
            (a.chan, format!("{:?}", a.attr)).cmp(&(b.chan, format!("{:?}", b.attr)))
        });
        out
    }

    /// The recipes currently on faders, for recording alongside
    /// `captured()`. A fader at zero is still recorded — an operator who
    /// parks a fader down and records expects it back where they left
    /// it.
    pub fn assigned(&self) -> Vec<Recipe> {
        self.faders
            .iter()
            .flat_map(|f| f.recipes().cloned())
            .collect()
    }

    /// Sets one effect parameter on a fader — `depth`, `bars`, `duty`.
    // r[impl profile.effect-parameters]
    pub fn set_param(&mut self, index: usize, name: &str, value: f32) {
        if index >= FADERS {
            return;
        }
        self.faders[index].params.insert(name.to_string(), value);
        if let Some(page) = self.pages.get_mut(self.page) {
            page[index].params.insert(name.to_string(), value);
        }
    }

    /// Clears everything the operator set by hand, leaving the faders.
    ///
    /// Two separate verbs because they are two separate mistakes: "undo
    /// the colour I just hit" and "put the effects away" are never the
    /// same intent in the middle of a song.
    pub fn clear_values(&mut self) {
        self.values.clear();
        self.applied.clear();
    }

    /// What the hand holds, as recipes: a look latched by a key and
    /// every apply since the values were cleared, in order. What
    /// STORE → LOOK writes.
    // r[impl profile.looks.authored] - a look is captured as recipes, not channel floats
    pub fn look_recipes(&self) -> Vec<Recipe> {
        self.held
            .iter()
            .chain(self.applied.iter())
            .cloned()
            .collect()
    }

    pub fn clear_faders(&mut self) {
        self.faders = Default::default();
        self.latched = [false; FADERS];
        self.level_fades = Default::default();
        self.keys_down = [None; FADERS];
        self.toggled = [false; FADERS];
        if let Some(page) = self.pages.get_mut(self.page) {
            *page = Default::default();
        }
    }

    /// Applies a palette (or anything else a recipe can express) to the
    /// current selection, as direct values, arriving over the program
    /// time from wherever the output was.
    ///
    /// Resolved immediately rather than stored as a recipe, because this
    /// is the operator's hand on the desk: what they see now is what
    /// they get, and a template that re-resolved under them mid-song
    /// would be a surprise, not a feature.
    // r[impl playback.program-time] - a palette punch
    pub fn apply(&mut self, apply: RecipeApply, show: &Show<'_>) {
        let Some(selection) = &self.selection else {
            return;
        };
        let now = self.now.get();
        let recipe = Recipe::new(selection.clone(), apply);
        // A later apply on the same target and attribute class replaces
        // the earlier — the hand holds one colour per role, not a
        // history of them.
        let family = |r: &Recipe| {
            r.steps
                .first()
                .and_then(|s| s.apply.first())
                .map(apply_family)
        };
        let same = family(&recipe);
        self.applied
            .retain(|r| !(r.target == recipe.target && family(r) == same));
        self.applied.push(recipe.clone());
        for emit in expand_recipe(&recipe, show, 0.0) {
            let key = (emit.value.chan, emit.value.attr);
            let previous = self.values.get(&key).copied();
            let target = if emit.relative {
                previous.map(|h| h.target).unwrap_or(0.0) + emit.value.value
            } else {
                emit.value.value
            };
            // A fade from a value already held starts from where that
            // fade *is*, not where it was going, so two quick punches
            // do not jump.
            let from = previous.map(|h| self.settle(&h, show, now, None));
            self.values.insert(
                key,
                Held {
                    target,
                    from,
                    started: now,
                },
            );
        }
    }

    /// Where a held value is right now.
    fn settle(&self, held: &Held, show: &Show<'_>, now: f32, under: Option<f32>) -> f32 {
        let progress = self.progress(show, now - held.started);
        if progress >= 1.0 {
            // Exactly the target once arrived — a lerp that lands a
            // float's width off would make "record" and the stage
            // disagree about what the operator set.
            return held.target;
        }
        let from = held.from.or(under).unwrap_or(held.target);
        from + (held.target - from) * progress
    }

    /// How far through the program time `elapsed` seconds is, 0..=1.
    fn progress(&self, show: &Show<'_>, elapsed: f32) -> f32 {
        if self.program_time_beats <= 0.0 {
            return 1.0;
        }
        let bps = Speed::Master("Song".into()).beats_per_second(show.speeds);
        self.last_bps.set(bps);
        if bps <= 0.0 {
            return 1.0;
        }
        (elapsed * bps / self.program_time_beats).clamp(0.0, 1.0)
    }

    /// Releases the current selection's hold on one attribute family —
    /// how an operator takes their hand off without clearing the whole
    /// programmer.
    pub fn release(&mut self, show: &Show<'_>) {
        let Some(selection) = &self.selection else {
            return;
        };
        let chans = resolve(selection, show.groups, show.rig);
        self.values.retain(|(chan, _), _| !chans.contains(chan));
        self.applied.retain(|r| &r.target != selection);
    }

    /// Assigns a fader on the current page, and makes it live. Clears a
    /// latch on that slot: an explicit assignment is not a pickup
    /// problem.
    pub fn set_fader(&mut self, index: usize, fader: Fader) {
        if index >= FADERS {
            return;
        }
        if let Some(page) = self.pages.get_mut(self.page) {
            page[index] = fader.clone();
        }
        self.faders[index] = fader;
        self.latched[index] = false;
        self.level_fades[index] = None;
    }

    /// Moves a fader. The level arrives over the program time.
    ///
    /// On a latched slot the previous page's fader keeps playing at its
    /// old level until this one comes within `PICKUP` of the new page's
    /// level, at which point the new assignment takes over and follows
    /// the hand.
    // r[impl playback.pages] - fader pickup
    // r[impl playback.program-time] - a fader take
    pub fn set_level(&mut self, index: usize, level: f32) {
        if index >= FADERS {
            return;
        }
        let level = level.clamp(0.0, 1.0);
        if self.latched[index] {
            let stored = self.pages.get(self.page).map(|p| p[index].level);
            match stored {
                Some(stored) if (stored - level).abs() <= PICKUP => {
                    self.faders[index] = self.pages[self.page][index].clone();
                    self.latched[index] = false;
                }
                _ => return,
            }
        }
        let now = self.now.get();
        let was = self.faders[index].level;
        if self.program_time_beats > 0.0 && (was - level).abs() > f32::EPSILON {
            self.level_fades[index] = Some((was, now));
        } else {
            self.level_fades[index] = None;
        }
        self.faders[index].level = level;
        if let Some(page) = self.pages.get_mut(self.page) {
            page[index].level = level;
        }
    }

    /// Per-fader speed scale — see `Fader::speed_scale`.
    // r[impl playback.speed-scale]
    pub fn set_speed_scale(&mut self, index: usize, scale: f32) {
        if index >= FADERS {
            return;
        }
        let scale = scale.max(0.0);
        self.faders[index].speed_scale = scale;
        if let Some(page) = self.pages.get_mut(self.page) {
            page[index].speed_scale = scale;
        }
    }

    // ── keys ─────────────────────────────────────────────────────────

    /// A playback key goes down on a fader.
    ///
    /// Speed keys land on the tap master here; transport keys (pause,
    /// load, go back) are returned as a `Transport` for the host to hand
    /// to `Playbacks::transport`, since the programmer does not own the
    /// playbacks.
    // r[impl playback.keys]
    // r[impl playback.speed-keys] - learn / half / double / reset keys
    // r[impl playback.temp-and-pause] - temp, pause, go back keys
    pub fn key_down(&mut self, index: usize, action: KeyAction) -> Option<Transport> {
        match action {
            KeyAction::Learn => {
                self.tap.tap(self.now.get());
                return None;
            }
            KeyAction::HalfSpeed => {
                self.tap.half();
                return None;
            }
            KeyAction::DoubleSpeed => {
                self.tap.double();
                return None;
            }
            KeyAction::ResetSpeed => {
                self.tap.reset();
                return None;
            }
            KeyAction::Pause => return Some(Transport::TogglePause),
            KeyAction::Load => return Some(Transport::Load(index)),
            KeyAction::GoBack => return Some(Transport::GoBack),
            _ => {}
        }
        if index >= FADERS {
            return None;
        }
        match action {
            KeyAction::Toggle => {
                self.toggled[index] = !self.toggled[index];
            }
            KeyAction::Temp => {
                self.keys_down[index] = Some(action);
                self.temp_down[index] = Some(self.now.get());
                self.temp_release[index] = None;
            }
            KeyAction::Kill => {
                for i in 0..FADERS {
                    let level = if i == index { 1.0 } else { 0.0 };
                    self.faders[i].level = level;
                    self.level_fades[i] = None;
                    if let Some(page) = self.pages.get_mut(self.page) {
                        page[i].level = level;
                    }
                }
            }
            KeyAction::Flash | KeyAction::Swap | KeyAction::Black => {
                self.keys_down[index] = Some(action);
            }
            _ => {}
        }
        None
    }

    /// The key on a fader comes up. Toggle and kill ignore this. A temp
    /// starts its way back from wherever it had got to.
    pub fn key_up(&mut self, index: usize) {
        if index >= FADERS {
            return;
        }
        if matches!(self.keys_down[index], Some(KeyAction::Temp))
            && let Some(started) = self.temp_down[index].take()
        {
            let now = self.now.get();
            let reached = self.temp_progress(started, now);
            self.temp_release[index] = Some((reached, now));
        }
        self.keys_down[index] = None;
    }

    /// How far a temp key has lifted its fader: 0 at the press, 1 once
    /// the program time has elapsed. Uses the clock of the last fold,
    /// which is the only one the programmer has between frames.
    fn temp_progress(&self, started: f32, now: f32) -> f32 {
        if self.program_time_beats <= 0.0 {
            return 1.0;
        }
        let bps = self.last_bps.get();
        if bps <= 0.0 {
            return 1.0;
        }
        ((now - started) * bps / self.program_time_beats).clamp(0.0, 1.0)
    }

    pub fn is_toggled(&self, index: usize) -> bool {
        self.toggled.get(index).copied().unwrap_or(false)
    }

    /// Whether a slot is still playing the previous page's fader.
    pub fn is_latched(&self, index: usize) -> bool {
        self.latched.get(index).copied().unwrap_or(false)
    }

    // ── pages ────────────────────────────────────────────────────────

    /// Adds an empty page at the end and returns its index.
    // r[impl playback.pages]
    pub fn add_page(&mut self) -> usize {
        self.pages.push(Default::default());
        self.pages.len() - 1
    }

    pub fn next_page(&mut self) {
        if !self.pages.is_empty() {
            self.set_page((self.page + 1) % self.pages.len());
        }
    }

    pub fn prev_page(&mut self) {
        if !self.pages.is_empty() {
            self.set_page((self.page + self.pages.len() - 1) % self.pages.len());
        }
    }

    /// Changes page. A fader that is up stays live on its old
    /// assignment — latched — until `set_level` brings it to match the
    /// new page's level; a fader at rest takes the new page's fader
    /// immediately.
    // r[impl playback.pages] - a fader that is up stays live
    pub fn set_page(&mut self, page: usize) {
        if page >= self.pages.len() || page == self.page {
            return;
        }
        self.page = page;
        for i in 0..FADERS {
            let live = self.faders[i].level > 0.0 || self.level_fades[i].is_some();
            if live {
                self.latched[i] = true;
            } else {
                self.faders[i] = self.pages[page][i].clone();
                self.latched[i] = false;
                self.level_fades[i] = None;
            }
            // A held key belongs to the hand, not the page; a toggle
            // latch belongs to the slot and is released by the change.
            self.toggled[i] = false;
        }
    }

    // ── the fold ─────────────────────────────────────────────────────

    /// Folds the programmer's layers onto whatever the cue stack
    /// produced. Does nothing when blind — see `preview_output`.
    ///
    /// A fader's level is a **crossfade weight**, not a multiplier. That
    /// one choice makes absolute and relative recipes behave the way an
    /// operator expects from the same control: pushing a colour fader up
    /// fades that colour in over what is underneath, and pushing a
    /// `Delta` pulse up fades the modulation in. A multiplier would
    /// dim the colour toward black instead, which is not what the
    /// control looks like it does.
    // r[impl playback.stack] - layers 1-4 (hand, masters, flashes, faders); triggers and the cue player are folded by the caller
    // r[impl playback.busking-over-show] - everything here lands on top of the cue player's output
    // r[impl playback.hand-wins] - direct values are written last
    // r[impl playback.blind] - a blind programmer does not reach output
    // r[impl recipes.size] - fader swing scaled by `size`
    // r[impl recipes.rate] - fader clock scaled by `rate`
    // r[impl effects.size-scales-the-swing] - size travels on the `Show`, so the expansion scales it
    // r[impl effects.masters.scale] - rate is operator state here, applied to every master through the `Show`
    // r[impl effects.masters.uniform] - faders and the cue player read the same two numbers
    pub fn apply_to(
        &self,
        base: &mut HashMap<(ChanId, Attribute), f32>,
        show: &Show<'_>,
        secs: f32,
    ) {
        self.now.set(secs);
        if self.blind {
            return;
        }
        self.fold(base, show, secs);
    }

    /// What the output *would* be with this programmer folded in, blind
    /// or not — for the visualizer's preview. Does not touch `base` or
    /// the clock.
    // r[impl playback.blind] - held values are shown in the preview
    pub fn preview_output(
        &self,
        base: &HashMap<(ChanId, Attribute), f32>,
        show: &Show<'_>,
        secs: f32,
    ) -> HashMap<(ChanId, Attribute), f32> {
        let mut out = base.clone();
        self.fold(&mut out, show, secs);
        out
    }

    /// A fader's level as it is playing right now: its glide, then any
    /// key on it, then a swap held elsewhere.
    fn playing_level(&self, index: usize, show: &Show<'_>, secs: f32) -> f32 {
        let fader = &self.faders[index];
        let mut level = match self.level_fades[index] {
            Some((from, started)) => {
                from + (fader.level - from) * self.progress(show, secs - started)
            }
            None => fader.level,
        };
        if self.toggled[index] || matches!(self.keys_down[index], Some(KeyAction::Flash)) {
            level = 1.0;
        }
        // A temp lifts the fader toward full over the program time and
        // lets it back down over the same after release; the slot's own
        // level is what it lifts from and returns to.
        // r[impl playback.temp-and-pause] - temp
        if let (Some(KeyAction::Temp), Some(started)) =
            (self.keys_down[index], self.temp_down[index])
        {
            let t = self.progress(show, secs - started);
            level += (1.0 - level) * t;
        } else if let Some((reached, released)) = self.temp_release[index] {
            let t = self.progress(show, secs - released);
            let lift = reached * (1.0 - t);
            level += (1.0 - level) * lift;
        }
        match self.keys_down[index] {
            Some(KeyAction::Swap) => 1.0,
            _ if self
                .keys_down
                .iter()
                .any(|k| matches!(k, Some(KeyAction::Swap))) =>
            {
                0.0
            }
            _ => level,
        }
    }

    /// The show as this programmer's size and rate see it — what every
    /// fader expands through, and what a host should hand its cue
    /// player, so one control reaches both.
    // r[impl effects.masters.scale]
    // r[impl effects.masters.uniform]
    // r[impl recipes.size]
    // r[impl recipes.rate]
    pub fn show_for<'a>(&self, show: &Show<'a>) -> Show<'a> {
        show.scaled(
            show.size * self.size.clamp(0.0, 1.0),
            show.speed_scale * self.rate.max(0.0),
        )
    }

    fn fold(&self, base: &mut HashMap<(ChanId, Attribute), f32>, show: &Show<'_>, secs: f32) {
        // Size and rate are applied where every recipe is expanded, not
        // here, so a fader and a cue-player effect obey the same control
        // the same way.
        let show = &self.show_for(show);
        // Resolved once, and only when something that respects it is
        // in play — a fold with no black key, no blackout, no held look
        // and the grand master at full never asks.
        let black_down = self
            .keys_down
            .iter()
            .any(|k| matches!(k, Some(KeyAction::Black)));
        let blackout_live = self.blackout || self.blackout_since.is_some();
        let wants_protected = !self.protected.is_empty()
            && (black_down || blackout_live || self.held_safe || self.grand < 1.0);
        let protected = if wants_protected {
            self.protected_chans(show)
        } else {
            HashSet::new()
        };

        for (index, fader) in self.faders.iter().enumerate() {
            let level = self.playing_level(index, show, secs);
            if level <= 0.0 {
                continue;
            }
            let black = matches!(self.keys_down[index], Some(KeyAction::Black));
            // The fader's own speed scale stretches the clock the recipe
            // is evaluated at; the programmer's rate reaches it through
            // the show's speed scale, as it reaches every other recipe.
            let at = secs * fader.speed_scale.max(0.0);
            // `depth` rides on the parametrised copy and reaches the
            // expansion as this recipe's own size.
            // r[impl playback.effect-parameters] - depth through the show's size
            for recipe in fader.recipes() {
                let recipe = fader.parametrised(recipe);
                for emit in expand_recipe(&recipe, show, at) {
                    // r[impl playback.attribute-filter] - dropped before the weight, withdraws nothing
                    if !fader.filter.admits(&emit.value.attr) {
                        continue;
                    }
                    if black
                        && emit.value.attr == Attribute::Dimmer
                        && !protected.contains(&emit.value.chan)
                    {
                        continue;
                    }
                    let key = (emit.value.chan, emit.value.attr);
                    let under = base.get(&key).copied().unwrap_or(0.0);
                    // The fader says how much of *this* effect; size (how
                    // much of every effect at once) has already scaled the
                    // swing inside the expansion.
                    let weight = level;
                    let value = if emit.relative {
                        under + emit.value.value * weight
                    } else {
                        under + (emit.value.value - under) * weight
                    };
                    base.insert(key, value);
                }
            }
        }

        // The macro layer: effects fired by name, beside the faders and
        // weighted the same way, each on its own clock from the moment
        // it was fired.
        // r[impl playback.macro-effects]
        for extra in &self.extras {
            if extra.level <= 0.0 {
                continue;
            }
            for emit in expand_recipe(&extra.recipe, show, secs - extra.started) {
                let key = (emit.value.chan, emit.value.attr);
                let under = base.get(&key).copied().unwrap_or(0.0);
                let value = if emit.relative {
                    under + emit.value.value * extra.level
                } else {
                    under + (emit.value.value - under) * extra.level
                };
                base.insert(key, value);
            }
        }

        // The held look lands over the faders at full weight: a punt
        // look is the operator saying "not whatever that was", and it
        // has to beat the effects that were running. Under the masters,
        // like everything else that is not the operator's direct hand.
        // A `Safe` look — a blackout held on a key — leaves the
        // protected roles' intensity where the layers beneath put it.
        // r[impl playback.protected-untouched] - a safe held look, a rig drop
        for recipe in &self.held {
            for emit in expand_recipe(recipe, show, secs) {
                if self.held_safe
                    && emit.value.attr == Attribute::Dimmer
                    && protected.contains(&emit.value.chan)
                {
                    continue;
                }
                let key = (emit.value.chan, emit.value.attr);
                let value = if emit.relative {
                    base.get(&key).copied().unwrap_or(0.0) + emit.value.value
                } else {
                    emit.value.value
                };
                base.insert(key, value);
            }
        }

        // Blackout: every intensity toward zero over the program time,
        // above the faders, the macro layer, the held look and the
        // flashes — under the masters and the hand, like the rest of
        // what is not the operator's own fingers.
        // r[impl playback.blackout]
        // r[impl playback.protected-untouched] - blackout
        if blackout_live {
            let since = self.blackout_since.unwrap_or(secs);
            let t = self.progress(show, secs - since);
            // Fading in toward black, or back out of it.
            let keep = if self.blackout { 1.0 - t } else { t };
            if keep < 1.0 {
                for ((chan, attr), value) in base.iter_mut() {
                    if *attr == Attribute::Dimmer && !protected.contains(chan) {
                        *value *= keep;
                    }
                }
            }
        }

        // Flashes land *before* the masters, so pulling a role down
        // quietens its flashes too — a master an operator has pulled
        // should hold whatever arrives, including their own hand.
        for (recipe, started) in &self.flashes {
            for emit in expand_recipe(recipe, show, secs - started) {
                let key = (emit.value.chan, emit.value.attr);
                let under = base.get(&key).copied().unwrap_or(0.0);
                let value = if emit.relative {
                    under + emit.value.value
                } else {
                    emit.value.value
                };
                base.insert(key, value);
            }
        }

        self.apply_masters(base, show);

        // The operator's hand wins over everything, including their own
        // faders and their own masters — pulling a master down and then
        // setting a level by hand should give the level they set. Each
        // value glides in from what was underneath it at the punch.
        for (key, held) in &self.values {
            let under = base.get(key).copied();
            base.insert(key.clone(), self.settle(held, show, secs, under));
        }

        self.apply_highlight(base, show);

        // A park is the one thing nothing else moves — not a cue, not
        // the hand, not a master. Written after all of them.
        // r[impl playback.park] - above every playback and the programmer
        for (key, value) in &self.parked {
            base.insert(key.clone(), *value);
        }

        // The grand master, last of all: after the parks too, because
        // the one hand that brings the whole rig down has to beat a
        // parked intensity — that is the safety the spec asks for. A
        // parked *DMX channel* is past its reach, being applied on the
        // wire after encode.
        // r[impl playback.grand-master] - every intensity, last, after parks of intensity
        // r[impl playback.protected-untouched] - the grand master too
        let grand = self.grand.clamp(0.0, 1.0);
        if grand < 1.0 {
            for ((chan, attr), value) in base.iter_mut() {
                if *attr == Attribute::Dimmer && !protected.contains(chan) {
                    *value *= grand;
                }
            }
        }
    }

    /// Highlight the selection to open white at full, above everything;
    /// lowlight the rest to the floor.
    // r[impl playback.highlight]
    fn apply_highlight(&self, base: &mut HashMap<(ChanId, Attribute), f32>, show: &Show<'_>) {
        if !(self.highlight || self.lowlight) {
            return;
        }
        let Some(selection) = &self.selection else {
            return;
        };
        let selected: HashSet<ChanId> =
            crate::selection::resolve_with(selection, show.groups, show.rig, show.roles)
                .into_iter()
                .collect();
        if self.lowlight {
            let floor = self.lowlight_floor.clamp(0.0, 1.0);
            for ((chan, attr), value) in base.iter_mut() {
                if *attr == Attribute::Dimmer && !selected.contains(chan) {
                    *value = value.min(floor);
                }
            }
        }
        if self.highlight {
            for chan in selected {
                base.insert((chan, Attribute::Dimmer), 1.0);
                for channel in [ColorChannel::Red, ColorChannel::Green, ColorChannel::Blue] {
                    base.insert((chan, Attribute::ColorAdd { channel }), 1.0);
                }
            }
        }
    }

    /// Constrains intensity per role, and applies a solo.
    ///
    /// Dimmer only, deliberately. A master that scaled pan would drag
    /// every mover toward its home position as it came down, and a
    /// master that scaled colour would desaturate the rig — neither is
    /// what "quieter" means.
    ///
    /// Order on one fixture: scaling multiplies, then the positive and
    /// negative limits cap, then an additive master HTP-merges. Across
    /// two roles: scaling and negative take the lowest, positive the
    /// highest, additive the highest.
    // r[impl playback.masters-scale] - lowest scaling master wins on a shared fixture
    // r[impl groups.master]
    // r[impl groups.master.modes] - scaling, positive, negative, additive
    // r[impl playback.master-modes]
    fn apply_masters(&self, base: &mut HashMap<(ChanId, Attribute), f32>, show: &Show<'_>) {
        // A fader that *is* a master — `Fader::master` — is a scaling
        // master at the fader's level, folded with the named ones.
        let master_faders: Vec<(String, Master)> = self
            .faders
            .iter()
            .filter_map(|f| {
                f.master.as_ref().map(|role| {
                    (
                        role.clone(),
                        Master {
                            mode: MasterMode::Scaling,
                            level: f.level,
                        },
                    )
                })
            })
            .collect();
        if self.masters.is_empty()
            && self.selection_masters.is_empty()
            && self.solo.is_none()
            && master_faders.is_empty()
        {
            return;
        }
        // Resolve each named role once, not once per channel.
        let mut scale: HashMap<ChanId, f32> = HashMap::new();
        let mut positive: HashMap<ChanId, f32> = HashMap::new();
        let mut negative: HashMap<ChanId, f32> = HashMap::new();
        let mut additive: HashMap<ChanId, f32> = HashMap::new();
        let role_chans = |role: &str| {
            crate::selection::resolve_with(
                &Selection::Role(role.to_string()),
                show.groups,
                show.rig,
                show.roles,
            )
        };

        // Role masters and selection masters are the same thing keyed
        // two ways; they fold into one set of per-fixture limits.
        // r[impl playback.selection-master] - same modes, same fold as role masters
        let by_role = self
            .masters
            .iter()
            .map(|(role, master)| (role_chans(role), *master))
            .chain(
                master_faders
                    .iter()
                    .map(|(role, master)| (role_chans(role), *master)),
            );
        let by_selection = self.selection_masters.iter().map(|(selection, master)| {
            (
                crate::selection::resolve_with(selection, show.groups, show.rig, show.roles),
                *master,
            )
        });
        for (chans, master) in by_role.chain(by_selection) {
            let level = master.level.clamp(0.0, 1.0);
            for chan in chans {
                match master.mode {
                    // Lowest wins where a fixture plays two roles — a
                    // head that is both Key and Wash should follow
                    // whichever of them the operator pulled down, or a
                    // master would be defeated by any other role the
                    // fixture happens to belong to.
                    MasterMode::Scaling => {
                        let slot = scale.entry(chan).or_insert(1.0);
                        *slot = slot.min(level);
                    }
                    MasterMode::Negative => {
                        let slot = negative.entry(chan).or_insert(1.0);
                        *slot = slot.min(level);
                    }
                    // The most generous limit applies.
                    MasterMode::Positive => {
                        let slot = positive.entry(chan).or_insert(0.0);
                        *slot = slot.max(level);
                    }
                    MasterMode::Additive => {
                        let slot = additive.entry(chan).or_insert(0.0);
                        *slot = slot.max(level);
                    }
                }
            }
        }

        if let Some(solo) = &self.solo {
            let lit: HashSet<ChanId> = role_chans(solo).into_iter().collect();
            // Everything already carrying a dimmer that is *not* soloed
            // goes down to the floor. Taken from the base rather than
            // from the rig, so a fixture that was dark stays dark
            // instead of being lifted to the floor level.
            for (chan, _) in base.keys().filter(|(_, a)| *a == Attribute::Dimmer) {
                if !lit.contains(chan) {
                    let slot = scale.entry(*chan).or_insert(1.0);
                    *slot = slot.min(self.solo_floor.clamp(0.0, 1.0));
                }
            }
        }

        // An additive master lifts fixtures the show left without a
        // dimmer at all — that is what "a hand lift over a running
        // show" has to mean for a fixture the show is not using.
        for (chan, level) in &additive {
            if *level > 0.0 {
                base.entry((*chan, Attribute::Dimmer)).or_insert(0.0);
            }
        }

        for ((chan, attr), value) in base.iter_mut() {
            if *attr != Attribute::Dimmer {
                continue;
            }
            if let Some(factor) = scale.get(chan) {
                *value *= *factor;
            }
            if let Some(cap) = positive.get(chan) {
                *value = value.min(*cap);
            }
            if let Some(cap) = negative.get(chan) {
                *value = value.min(*cap);
            }
            if let Some(lift) = additive.get(chan) {
                *value = value.max(*lift);
            }
        }
    }

    /// Whether anything is being held. Drives the "clear" affordance —
    /// a desk that always looks armed teaches operators to ignore it.
    pub fn is_active(&self) -> bool {
        !self.values.is_empty()
            || !self.held.is_empty()
            || !self.extras.is_empty()
            || self.blackout
            || self.faders.iter().any(|f| f.level > 0.0)
            || self.toggled.iter().any(|t| *t)
            || self.keys_down.iter().any(|k| k.is_some())
            || !self.parked.is_empty()
            || !self.parked_dmx.is_empty()
    }
}

/// Which of the hand's "one per role" slots an apply occupies —
/// intensity, colour, focus, or anything else.
fn apply_family(apply: &RecipeApply) -> u8 {
    match apply {
        RecipeApply::Dimmer(_) => 0,
        RecipeApply::Color(_) | RecipeApply::Colors { .. } | RecipeApply::Split(_) => 1,
        RecipeApply::FocusPoint(_)
        | RecipeApply::FocusDirection(_)
        | RecipeApply::FocusFan { .. }
        | RecipeApply::FocusKeyframes(_)
        | RecipeApply::FocusDelta(_)
        | RecipeApply::FocusSplay { .. } => 2,
        _ => 3,
    }
}

#[cfg(test)]

mod tests {
    use super::*;
    use crate::group::Group;
    use crate::preset::{ColorPreset, Ref};
    use crate::selection::EMPTY_RIG;
    use crate::step::{Step, Timing};

    fn groups() -> Vec<Group> {
        vec![Group {
            name: "Pars".to_string(),
            chans: vec![1, 2, 3],
        }]
    }

    fn show(groups: &[Group]) -> Show<'_> {
        Show::new(groups, &EMPTY_RIG)
    }

    fn base() -> HashMap<(ChanId, Attribute), f32> {
        HashMap::new()
    }

    #[test]
    fn a_palette_hit_with_nothing_selected_does_nothing() {
        let groups = groups();
        let mut p = Programmer::new();
        p.apply(RecipeApply::Dimmer(1.0), &show(&groups));
        assert!(p.captured().is_empty());
    }

    #[test]
    fn a_palette_hit_applies_to_the_selection() {
        let groups = groups();
        let mut p = Programmer::new();
        p.select(Selection::Group("Pars".into()));
        p.apply(RecipeApply::Dimmer(0.7), &show(&groups));
        let captured = p.captured();
        assert_eq!(captured.len(), 3);
        assert!(captured.iter().all(|v| v.value == 0.7));
    }

    /// The hand remembered as recipes: a held look first, then every
    /// apply since CLEAR, one per role and family, released with the
    /// selection and gone with CLEAR.
    /// r[verify profile.looks.authored]
    #[test]
    fn the_hand_is_captured_as_recipes_for_a_look() {
        let groups = groups();
        let mut p = Programmer::new();
        let held = Recipe::new(Selection::Role("Key".into()), RecipeApply::Dimmer(0.6));
        p.hold_look(vec![held.clone()], false);
        p.select(Selection::Group("Pars".into()));
        p.apply(RecipeApply::Dimmer(0.3), &show(&groups));
        p.apply(RecipeApply::Dimmer(0.7), &show(&groups));
        p.apply(
            RecipeApply::Color(crate::preset::Ref::Named("House Blue".into())),
            &show(&groups),
        );
        let recipes = p.look_recipes();
        assert_eq!(recipes.len(), 3, "held, dimmer, colour");
        assert_eq!(recipes[0], held);
        assert_eq!(recipes[1].target, Selection::Group("Pars".into()));
        assert_eq!(
            recipes[1].steps[0].apply[0],
            RecipeApply::Dimmer(0.7),
            "the later apply of a family replaces the earlier"
        );
        assert!(matches!(
            recipes[2].steps[0].apply[0],
            RecipeApply::Color(_)
        ));

        p.release(&show(&groups));
        assert_eq!(
            p.look_recipes(),
            vec![held],
            "release lets the selection's recipes go"
        );
        p.apply(RecipeApply::Dimmer(0.1), &show(&groups));
        p.clear_values();
        p.release_hold();
        assert!(p.look_recipes().is_empty());
    }

    /// r[verify playback.hand-wins]
    /// r[verify playback.busking-over-show]
    #[test]
    fn the_operators_hand_beats_the_cue_stack() {
        let groups = groups();
        let mut p = Programmer::new();
        p.select(Selection::Chans(vec![1]));
        p.apply(RecipeApply::Dimmer(0.2), &show(&groups));

        let mut out = base();
        out.insert((1, Attribute::Dimmer), 1.0); // the cue says full
        p.apply_to(&mut out, &show(&groups), 0.0);
        assert_eq!(out[&(1, Attribute::Dimmer)], 0.2);
    }

    #[test]
    fn releasing_drops_only_the_selections_hold() {
        let groups = groups();
        let mut p = Programmer::new();
        p.select(Selection::Group("Pars".into()));
        p.apply(RecipeApply::Dimmer(1.0), &show(&groups));
        p.select(Selection::Chans(vec![2]));
        p.release(&show(&groups));
        let held: Vec<ChanId> = p.captured().iter().map(|v| v.chan).collect();
        assert_eq!(held, vec![1, 3]);
    }

    fn colour_fader(level: f32) -> Fader {
        Fader {
            name: "Red".into(),
            recipe: Some(Recipe::new(
                Selection::Chans(vec![1]),
                RecipeApply::Color(Ref::Inline(ColorPreset {
                    name: "Red".into(),
                    red: 1.0,
                    green: 0.0,
                    blue: 0.0,
                    ..Default::default()
                })),
            )),
            level,
            ..Default::default()
        }
    }

    fn dimmer_fader(chan: ChanId, value: f32, level: f32) -> Fader {
        Fader {
            name: "Dim".into(),
            recipe: Some(Recipe::new(
                Selection::Chans(vec![chan]),
                RecipeApply::Dimmer(value),
            )),
            level,
            ..Default::default()
        }
    }

    /// The design decision worth pinning: level crossfades toward the
    /// recipe rather than scaling it. At half, a red fader over a blue
    /// wash reads half-way to red — not half-brightness red.
    #[test]
    fn a_fader_crossfades_toward_its_recipe() {
        let groups = groups();
        let mut p = Programmer::new();
        p.set_fader(0, colour_fader(0.5));

        let mut out = base();
        let red = Attribute::ColorAdd {
            channel: ignition_proto::ColorChannel::Red,
        };
        out.insert((1, red.clone()), 0.0);
        p.apply_to(&mut out, &show(&groups), 0.0);
        assert!((out[&(1, red)] - 0.5).abs() < 0.001);
    }

    #[test]
    fn a_fader_at_zero_contributes_nothing() {
        let groups = groups();
        let mut p = Programmer::new();
        p.set_fader(0, colour_fader(0.0));
        let mut out = base();
        out.insert((1, Attribute::Dimmer), 0.4);
        p.apply_to(&mut out, &show(&groups), 0.0);
        assert_eq!(out[&(1, Attribute::Dimmer)], 0.4);
    }

    /// A relative fader adds rather than crossfading, so a pulse
    /// modulates the colour underneath instead of replacing it.
    #[test]
    fn a_relative_fader_modulates_what_is_underneath() {
        let groups = groups();
        let mut p = Programmer::new();
        p.set_fader(
            0,
            Fader {
                name: "Pulse".into(),
                recipe: Some(Recipe {
                    target: Selection::Chans(vec![1]),
                    steps: vec![Step::new(vec![RecipeApply::Delta(vec![(
                        Attribute::Dimmer,
                        -0.4,
                    )])])],
                    timing: Timing::default(),
                    tricks: Vec::new(),
                    stack: false,
                    ..Default::default()
                }),
                level: 0.5,
                ..Default::default()
            },
        );
        let mut out = base();
        out.insert((1, Attribute::Dimmer), 1.0);
        p.apply_to(&mut out, &show(&groups), 0.0);
        // -0.4 at half weight.
        assert!((out[&(1, Attribute::Dimmer)] - 0.8).abs() < 0.001);
    }

    fn chase(chan: ChanId) -> Recipe {
        Recipe {
            target: Selection::Chans(vec![chan]),
            steps: vec![
                Step::new(vec![RecipeApply::Raw(vec![(Attribute::Dimmer, 0.0)])]),
                Step::new(vec![RecipeApply::Raw(vec![(Attribute::Dimmer, 1.0)])]),
            ],
            timing: Timing {
                speed: Speed::Master("Rate".into()),
                ..Default::default()
            },
            tricks: Vec::new(),
            stack: false,
            ..Default::default()
        }
    }

    fn show_at<'a>(groups: &'a [Group], masters: &'a crate::step::SpeedMasters) -> Show<'a> {
        Show {
            groups,
            palettes: crate::preset::Palettes::EMPTY,
            rig: &EMPTY_RIG,
            speeds: masters,
            roles: &crate::recipe::NO_ROLES,
            ..Show::new(groups, &EMPTY_RIG)
        }
    }

    /// One rate source retiming every fader at once is the thing this
    /// design has that grandMA3 does not — a speed master drives *every*
    /// recipe here, because a phaser is a recipe.
    #[test]
    fn one_speed_master_drives_every_fader() {
        let groups = groups();
        let masters = crate::step::SpeedMasters::from([("Rate".to_string(), 120.0)]);
        let show = show_at(&groups, &masters);
        let mut p = Programmer::new();
        for (i, chan) in [1, 2].into_iter().enumerate() {
            p.set_fader(
                i,
                Fader {
                    name: "Chase".into(),
                    recipe: Some(chase(chan)),
                    level: 1.0,
                    ..Default::default()
                },
            );
        }

        // 120 BPM = 2 cycles/sec: both faders are on step 0 early in a
        // cycle and step 1 late, together, from one master.
        let mut early = base();
        p.apply_to(&mut early, &show, 0.05);
        let mut late = base();
        p.apply_to(&mut late, &show, 0.35);
        assert_eq!(
            early[&(1, Attribute::Dimmer)],
            early[&(2, Attribute::Dimmer)]
        );
        assert_ne!(
            early[&(1, Attribute::Dimmer)],
            late[&(1, Attribute::Dimmer)]
        );
    }

    /// A fader at double speed reaches its second step when the master's
    /// own tempo is still on its first.
    /// r[verify playback.speed-scale]
    #[test]
    fn a_fader_can_run_at_a_multiple_of_its_master() {
        let groups = groups();
        let masters = crate::step::SpeedMasters::from([("Rate".to_string(), 120.0)]);
        let show = show_at(&groups, &masters);
        let mut p = Programmer::new();
        p.set_fader(
            0,
            Fader {
                name: "Chase".into(),
                recipe: Some(chase(1)),
                level: 1.0,
                ..Default::default()
            },
        );
        p.set_fader(
            1,
            Fader {
                name: "Chase x2".into(),
                recipe: Some(chase(2)),
                level: 1.0,
                speed_scale: 2.0,
                ..Default::default()
            },
        );
        // At 2 cycles/sec, 0.2s is 0.4 cycles: step 0 at x1, step 1 at x2.
        let mut out = base();
        p.apply_to(&mut out, &show, 0.2);
        assert_eq!(out[&(1, Attribute::Dimmer)], 0.0);
        assert_eq!(out[&(2, Attribute::Dimmer)], 1.0);
    }

    // ── live control ─────────────────────────────────────────────────

    /// A role master scales that role's fixtures and nothing else.
    /// r[verify groups.master]
    /// r[verify playback.masters-scale]
    #[test]
    fn a_master_scales_only_its_own_role() {
        let groups = groups();
        let venue = roles();
        let show = show_with_roles(&groups, &venue);
        let mut p = Programmer::new();
        p.set_master("Key", 0.5);

        let mut out = HashMap::new();
        out.insert((1, Attribute::Dimmer), 1.0); // Key
        out.insert((9, Attribute::Dimmer), 1.0); // not Key
        p.apply_to(&mut out, &show, 0.0);

        assert!((out[&(1, Attribute::Dimmer)] - 0.5).abs() < 1e-6);
        assert!(
            (out[&(9, Attribute::Dimmer)] - 1.0).abs() < 1e-6,
            "an unrelated fixture moved"
        );
    }

    /// Scaling, not limiting. A fixture at half under a master at half
    /// is a quarter — which is what an operator expects from something
    /// that behaves like a fader, and is the distinction
    /// `r[groups.master.modes]` exists to pin.
    /// r[verify groups.master.modes] - scaling
    /// r[verify playback.masters-scale]
    /// r[verify playback.master-modes] - scaling
    #[test]
    fn a_master_scales_rather_than_limits() {
        let groups = groups();
        let venue = roles();
        let show = show_with_roles(&groups, &venue);
        let mut p = Programmer::new();
        p.set_master("Key", 0.5);

        let mut out = HashMap::new();
        out.insert((1, Attribute::Dimmer), 0.5);
        p.apply_to(&mut out, &show, 0.0);
        assert!((out[&(1, Attribute::Dimmer)] - 0.25).abs() < 1e-6);
    }

    /// A positive master caps: a fixture under the cap is untouched, one
    /// over it is held at the cap.
    /// r[verify groups.master.modes] - limiting
    /// r[verify playback.master-modes] - positive
    #[test]
    fn a_positive_master_limits_rather_than_scales() {
        let groups = groups();
        let venue = roles();
        let show = show_with_roles(&groups, &venue);
        let mut p = Programmer::new();
        p.set_master("Key", 0.6);
        p.set_master_mode("Key", MasterMode::Positive);

        let mut out = HashMap::new();
        out.insert((1, Attribute::Dimmer), 0.5);
        out.insert((2, Attribute::Dimmer), 1.0);
        p.apply_to(&mut out, &show, 0.0);
        assert!(
            (out[&(1, Attribute::Dimmer)] - 0.5).abs() < 1e-6,
            "under the cap"
        );
        assert!(
            (out[&(2, Attribute::Dimmer)] - 0.6).abs() < 1e-6,
            "at the cap"
        );
    }

    /// r[verify playback.master-modes] - negative
    #[test]
    fn a_negative_master_inhibits() {
        let groups = groups();
        let venue = roles();
        let show = show_with_roles(&groups, &venue);
        let mut p = Programmer::new();
        p.set_master("Key", 0.3);
        p.set_master_mode("Key", MasterMode::Negative);

        let mut out = HashMap::new();
        out.insert((1, Attribute::Dimmer), 1.0);
        p.apply_to(&mut out, &show, 0.0);
        assert!((out[&(1, Attribute::Dimmer)] - 0.3).abs() < 1e-6);
    }

    /// An additive master lifts: HTP with the show, so it raises what is
    /// below it, leaves what is above it, and lights what the show left
    /// dark.
    /// r[verify playback.master-modes] - additive
    #[test]
    fn an_additive_master_lifts_by_htp() {
        let groups = groups();
        let venue = roles();
        let show = show_with_roles(&groups, &venue);
        let mut p = Programmer::new();
        p.set_master("Key", 0.5);
        p.set_master_mode("Key", MasterMode::Additive);

        let mut out = HashMap::new();
        out.insert((1, Attribute::Dimmer), 0.2);
        out.insert((2, Attribute::Dimmer), 0.9);
        p.apply_to(&mut out, &show, 0.0);
        assert!((out[&(1, Attribute::Dimmer)] - 0.5).abs() < 1e-6, "lifted");
        assert!(
            (out[&(2, Attribute::Dimmer)] - 0.9).abs() < 1e-6,
            "left alone"
        );
        assert!(
            (out[&(3, Attribute::Dimmer)] - 0.5).abs() < 1e-6,
            "a fixture the show left dark is lit"
        );
    }

    /// A fixture in two roles: under two positive masters the more
    /// generous limit applies; under two negative ones the stricter
    /// does. That is the whole difference between the two.
    /// r[verify playback.master-modes] - two roles
    /// r[verify playback.masters-scale] - lowest applies
    #[test]
    fn positive_takes_the_higher_limit_and_negative_the_lower() {
        let groups = groups();
        let venue = two_roles();
        let show = show_with_roles(&groups, &venue);

        let run = |mode: MasterMode| {
            let mut p = Programmer::new();
            p.set_master("Key", 0.3);
            p.set_master_mode("Key", mode);
            p.set_master("Wash", 0.8);
            p.set_master_mode("Wash", mode);
            let mut out = HashMap::new();
            out.insert((2, Attribute::Dimmer), 1.0); // Key and Wash
            p.apply_to(&mut out, &show, 0.0);
            out[&(2, Attribute::Dimmer)]
        };
        assert!((run(MasterMode::Positive) - 0.8).abs() < 1e-6);
        assert!((run(MasterMode::Negative) - 0.3).abs() < 1e-6);
        assert!(
            (run(MasterMode::Scaling) - 0.3).abs() < 1e-6,
            "the lowest scaling master applies"
        );
    }

    /// Dimmer only. A master that scaled pan would drag every mover
    /// toward its home position on the way down.
    #[test]
    fn a_master_leaves_position_alone() {
        let groups = groups();
        let venue = roles();
        let show = show_with_roles(&groups, &venue);
        let mut p = Programmer::new();
        p.set_master("Key", 0.25);

        let mut out = HashMap::new();
        out.insert((1, Attribute::Pan), 40.0);
        p.apply_to(&mut out, &show, 0.0);
        assert!((out[&(1, Attribute::Pan)] - 40.0).abs() < 1e-6);
    }

    /// Solo pulls everything else down to the floor, and leaves what is
    /// already dark dark — a solo that lifted unlit fixtures to the
    /// floor level would turn a blackout into a dim wash.
    /// r[verify playback.masters-scale] - solo
    #[test]
    fn solo_pulls_down_everything_else() {
        let groups = groups();
        let venue = roles();
        let show = show_with_roles(&groups, &venue);
        let mut p = Programmer::new();
        p.solo("Key");

        let mut out = HashMap::new();
        out.insert((1, Attribute::Dimmer), 1.0); // Key — stays
        out.insert((9, Attribute::Dimmer), 1.0); // other — floored
        p.apply_to(&mut out, &show, 0.0);

        assert!((out[&(1, Attribute::Dimmer)] - 1.0).abs() < 1e-6);
        assert!((out[&(9, Attribute::Dimmer)] - p.solo_floor).abs() < 1e-6);
        assert!(
            p.solo_floor > 0.0,
            "a solo that blacks the room reads as a fault"
        );
    }

    /// Size withdraws an effect without touching what is under it. This
    /// is the difference from a master, and the reason both exist.
    /// r[verify recipes.size]
    /// r[verify recipes.size.is-not-intensity]
    /// r[verify effects.size-scales-the-swing]
    #[test]
    fn size_flattens_the_effect_and_leaves_the_look() {
        let groups = groups();
        let venue = roles();
        let show = show_with_roles(&groups, &venue);

        let chase = |chan: ChanId| Recipe {
            target: Selection::Chans(vec![chan]),
            steps: vec![Step::new(vec![RecipeApply::Delta(vec![(
                Attribute::Dimmer,
                -0.5,
            )])])],
            timing: Timing::default(),
            tricks: Vec::new(),
            stack: false,
            ..Default::default()
        };

        let mut p = Programmer::new();
        p.set_fader(
            0,
            Fader {
                name: "Chase".into(),
                recipe: Some(chase(1)),
                level: 1.0,
                ..Default::default()
            },
        );

        let run = |p: &Programmer| {
            let mut out = HashMap::new();
            out.insert((1, Attribute::Dimmer), 0.8);
            p.apply_to(&mut out, &show, 0.0);
            out[&(1, Attribute::Dimmer)]
        };

        assert!((run(&p) - 0.3).abs() < 1e-6, "full size: 0.8 - 0.5");
        p.size = 0.5;
        assert!((run(&p) - 0.55).abs() < 1e-6, "half size: 0.8 - 0.25");
        p.size = 0.0;
        assert!(
            (run(&p) - 0.8).abs() < 1e-6,
            "at zero the look underneath must show through untouched"
        );
    }

    /// The programmer's rate and size reach a fader's effect through the
    /// show it expands with — the same two numbers a host hands the cue
    /// player — so a master-slaved chase on a fader doubles with rate.
    /// r[verify effects.masters.scale]
    /// r[verify effects.masters.uniform]
    /// r[verify recipes.rate]
    #[test]
    fn rate_and_size_reach_every_recipe_through_the_show() {
        let groups = groups();
        let masters = crate::step::SpeedMasters::from([("Rate".to_string(), 120.0)]);
        let show = show_at(&groups, &masters);
        let mut p = Programmer::new();
        p.set_fader(
            0,
            Fader {
                name: "Chase".into(),
                recipe: Some(chase(1)),
                level: 1.0,
                ..Default::default()
            },
        );
        // 2 cycles/sec: 0.2 s is step 0; at double rate it is step 1.
        let mut out = base();
        p.apply_to(&mut out, &show, 0.2);
        assert_eq!(out[&(1, Attribute::Dimmer)], 0.0);
        p.rate = 2.0;
        let mut out = base();
        p.apply_to(&mut out, &show, 0.2);
        assert_eq!(out[&(1, Attribute::Dimmer)], 1.0);

        p.size = 0.25;
        let scaled = p.show_for(&show);
        assert_eq!(scaled.speed_scale, 2.0);
        assert_eq!(scaled.size, 0.25);
        // Compounds with what the host already set, rather than replacing it.
        let host = show.scaled(0.5, 3.0);
        let scaled = p.show_for(&host);
        assert_eq!(scaled.speed_scale, 6.0);
        assert_eq!(scaled.size, 0.125);
    }

    /// A held look beats the faders while the key is down and is gone
    /// the moment it is released — there is no envelope to wait out.
    #[test]
    fn a_held_look_wins_over_the_faders_and_leaves_on_release() {
        let groups = groups();
        let mut p = Programmer::new();
        p.set_fader(0, colour_fader(1.0));
        p.hold(Recipe::new(
            Selection::Chans(vec![1]),
            RecipeApply::Dimmer(0.6),
        ));

        let mut out = base();
        out.insert((1, Attribute::Dimmer), 0.0);
        p.apply_to(&mut out, &show(&groups), 0.0);
        assert!((out[&(1, Attribute::Dimmer)] - 0.6).abs() < 1e-6);
        assert!(p.is_active());

        p.release_hold();
        let mut out = base();
        out.insert((1, Attribute::Dimmer), 0.0);
        p.apply_to(&mut out, &show(&groups), 0.0);
        assert_eq!(out[&(1, Attribute::Dimmer)], 0.0);
    }

    // ── keys ─────────────────────────────────────────────────────────

    fn dimmer_of(p: &Programmer, groups: &[Group], chan: ChanId) -> f32 {
        let mut out = base();
        p.apply_to(&mut out, &show(groups), 0.0);
        out.get(&(chan, Attribute::Dimmer)).copied().unwrap_or(0.0)
    }

    /// r[verify playback.keys] - flash
    #[test]
    fn flash_holds_the_fader_at_full_and_gives_it_back() {
        let groups = groups();
        let mut p = Programmer::new();
        p.set_fader(0, dimmer_fader(1, 1.0, 0.25));
        assert!((dimmer_of(&p, &groups, 1) - 0.25).abs() < 1e-6);
        p.key_down(0, KeyAction::Flash);
        assert!((dimmer_of(&p, &groups, 1) - 1.0).abs() < 1e-6);
        p.key_up(0);
        assert!((dimmer_of(&p, &groups, 1) - 0.25).abs() < 1e-6);
        assert_eq!(p.faders[0].level, 0.25, "the fader itself never moved");
    }

    /// r[verify playback.keys] - toggle
    #[test]
    fn toggle_latches_until_pressed_again() {
        let groups = groups();
        let mut p = Programmer::new();
        p.set_fader(0, dimmer_fader(1, 1.0, 0.0));
        p.key_down(0, KeyAction::Toggle);
        p.key_up(0);
        assert!(p.is_toggled(0));
        assert!(
            (dimmer_of(&p, &groups, 1) - 1.0).abs() < 1e-6,
            "still on after release"
        );
        p.key_down(0, KeyAction::Toggle);
        assert!(!p.is_toggled(0));
        assert_eq!(dimmer_of(&p, &groups, 1), 0.0);
    }

    /// r[verify playback.keys] - swap
    #[test]
    fn swap_takes_this_fader_to_full_and_suppresses_the_others_while_held() {
        let groups = groups();
        let mut p = Programmer::new();
        p.set_fader(0, dimmer_fader(1, 1.0, 0.5));
        p.set_fader(1, dimmer_fader(2, 1.0, 0.5));
        p.key_down(0, KeyAction::Swap);
        assert!((dimmer_of(&p, &groups, 1) - 1.0).abs() < 1e-6);
        assert_eq!(dimmer_of(&p, &groups, 2), 0.0);
        p.key_up(0);
        assert!((dimmer_of(&p, &groups, 1) - 0.5).abs() < 1e-6);
        assert!(
            (dimmer_of(&p, &groups, 2) - 0.5).abs() < 1e-6,
            "back after release"
        );
    }

    /// r[verify playback.keys] - kill
    #[test]
    fn kill_moves_the_faders_and_stays_moved() {
        let groups = groups();
        let mut p = Programmer::new();
        p.set_fader(0, dimmer_fader(1, 1.0, 0.5));
        p.set_fader(1, dimmer_fader(2, 1.0, 0.5));
        p.key_down(0, KeyAction::Kill);
        p.key_up(0);
        assert_eq!(p.faders[0].level, 1.0);
        assert_eq!(p.faders[1].level, 0.0);
        assert!((dimmer_of(&p, &groups, 1) - 1.0).abs() < 1e-6);
        assert_eq!(dimmer_of(&p, &groups, 2), 0.0);
    }

    /// Black zeroes only the intensity this fader contributes; its colour
    /// carries on, and so does every other fader.
    /// r[verify playback.keys] - black
    #[test]
    fn black_zeroes_this_faders_intensity_only() {
        let groups = groups();
        let mut p = Programmer::new();
        p.set_fader(0, dimmer_fader(1, 1.0, 1.0));
        p.set_fader(1, colour_fader(1.0));
        p.set_fader(2, dimmer_fader(2, 1.0, 1.0));
        p.key_down(0, KeyAction::Black);
        let mut out = base();
        p.apply_to(&mut out, &show(&groups), 0.0);
        assert!(!out.contains_key(&(1, Attribute::Dimmer)));
        let red = Attribute::ColorAdd {
            channel: ColorChannel::Red,
        };
        assert!((out[&(1, red)] - 1.0).abs() < 1e-6, "colour carries on");
        assert!(
            (out[&(2, Attribute::Dimmer)] - 1.0).abs() < 1e-6,
            "other faders carry on"
        );
        p.key_up(0);
        assert!((dimmer_of(&p, &groups, 1) - 1.0).abs() < 1e-6);
    }

    // ── pages ────────────────────────────────────────────────────────

    /// r[verify playback.pages]
    #[test]
    fn a_fader_at_rest_takes_the_new_pages_assignment() {
        let groups = groups();
        let mut p = Programmer::new();
        p.set_fader(0, dimmer_fader(1, 1.0, 0.0));
        let two = p.add_page();
        p.set_page(two);
        p.set_fader(0, dimmer_fader(2, 1.0, 0.0));
        p.set_level(0, 1.0);
        assert!((dimmer_of(&p, &groups, 2) - 1.0).abs() < 1e-6);
        assert_eq!(dimmer_of(&p, &groups, 1), 0.0);

        // Back on page one, fader zero is up on page two's assignment,
        // so it is latched; bringing it to page one's level (zero)
        // picks page one back up.
        p.set_page(0);
        assert!(p.is_latched(0));
        assert!(
            (dimmer_of(&p, &groups, 2) - 1.0).abs() < 1e-6,
            "still playing page two"
        );
        p.set_level(0, 0.5);
        assert!(p.is_latched(0), "sweeping past does not pick up");
        assert!((dimmer_of(&p, &groups, 2) - 1.0).abs() < 1e-6);
        p.set_level(0, 0.0);
        assert!(!p.is_latched(0));
        assert_eq!(dimmer_of(&p, &groups, 2), 0.0);
        assert_eq!(p.faders[0].recipe, p.pages[0][0].recipe);
    }

    /// A fader that is up keeps playing its old assignment across a
    /// page change until it is brought back to match.
    /// r[verify playback.pages] - a fader that is up stays live
    #[test]
    fn a_fader_that_is_up_stays_live_until_picked_up() {
        let groups = groups();
        let mut p = Programmer::new();
        p.set_fader(0, dimmer_fader(1, 1.0, 0.0));
        p.set_level(0, 0.8);
        let two = p.add_page();
        p.set_page(two);
        p.pages[two][0] = dimmer_fader(2, 1.0, 0.8);
        assert!(p.is_latched(0));
        assert!(
            (dimmer_of(&p, &groups, 1) - 0.8).abs() < 1e-6,
            "page one still plays"
        );
        assert_eq!(dimmer_of(&p, &groups, 2), 0.0);
        p.set_level(0, 0.82);
        assert!(!p.is_latched(0), "within pickup of the stored level");
        assert!((dimmer_of(&p, &groups, 2) - 0.82).abs() < 1e-6);
        assert_eq!(dimmer_of(&p, &groups, 1), 0.0);
    }

    #[test]
    fn next_and_prev_wrap() {
        let mut p = Programmer::new();
        p.add_page();
        p.add_page();
        p.next_page();
        assert_eq!(p.page, 1);
        p.next_page();
        p.next_page();
        assert_eq!(p.page, 0);
        p.prev_page();
        assert_eq!(p.page, 2);
    }

    // ── program time ─────────────────────────────────────────────────

    /// A palette punch over two beats at 120 BPM is a one-second fade:
    /// halfway at half a second, arrived at one.
    /// r[verify playback.program-time]
    #[test]
    fn a_palette_punch_is_halfway_at_half_the_program_time() {
        let groups = groups();
        let masters = crate::step::SpeedMasters::from([("Song".to_string(), 120.0)]);
        let show = show_at(&groups, &masters);
        let mut p = Programmer::new();
        p.program_time_beats = 2.0;
        p.set_now(10.0);
        p.select(Selection::Chans(vec![1]));
        p.apply(RecipeApply::Dimmer(1.0), &show);

        let at = |secs: f32| {
            let mut out = base();
            out.insert((1, Attribute::Dimmer), 0.2); // what the show had it at
            p.apply_to(&mut out, &show, secs);
            out[&(1, Attribute::Dimmer)]
        };
        assert!(
            (at(10.0) - 0.2).abs() < 1e-6,
            "starts from what was underneath"
        );
        assert!((at(10.5) - 0.6).abs() < 1e-6, "halfway");
        assert!((at(11.0) - 1.0).abs() < 1e-6, "arrived");
        assert!((at(12.0) - 1.0).abs() < 1e-6, "and stays");
        assert_eq!(p.captured()[0].value, 1.0, "record captures the target");
    }

    /// r[verify playback.program-time] - a fader take
    #[test]
    fn a_fader_take_glides_over_the_program_time() {
        let groups = groups();
        let masters = crate::step::SpeedMasters::from([("Song".to_string(), 120.0)]);
        let show = show_at(&groups, &masters);
        let mut p = Programmer::new();
        p.program_time_beats = 2.0;
        p.set_fader(0, dimmer_fader(1, 1.0, 0.0));
        p.set_level(0, 1.0);
        let at = |secs: f32| {
            let mut out = base();
            p.apply_to(&mut out, &show, secs);
            out[&(1, Attribute::Dimmer)]
        };
        assert!((at(0.5) - 0.5).abs() < 1e-6);
        assert!((at(1.0) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn zero_program_time_snaps() {
        let groups = groups();
        let mut p = Programmer::new();
        p.select(Selection::Chans(vec![1]));
        p.apply(RecipeApply::Dimmer(1.0), &show(&groups));
        assert!((dimmer_of(&p, &groups, 1) - 1.0).abs() < 1e-6);
    }

    // ── blind ────────────────────────────────────────────────────────

    /// r[verify playback.blind]
    #[test]
    fn blind_holds_values_off_the_output_but_in_the_preview() {
        let groups = groups();
        let mut p = Programmer::new();
        p.blind = true;
        p.select(Selection::Chans(vec![1]));
        p.apply(RecipeApply::Dimmer(1.0), &show(&groups));
        p.set_fader(0, dimmer_fader(2, 1.0, 1.0));

        let mut out = base();
        out.insert((1, Attribute::Dimmer), 0.3);
        p.apply_to(&mut out, &show(&groups), 0.0);
        assert!(
            (out[&(1, Attribute::Dimmer)] - 0.3).abs() < 1e-6,
            "the hand did not reach output"
        );
        assert!(!out.contains_key(&(2, Attribute::Dimmer)), "nor the fader");

        let preview = p.preview_output(&out, &show(&groups), 0.0);
        assert!((preview[&(1, Attribute::Dimmer)] - 1.0).abs() < 1e-6);
        assert!((preview[&(2, Attribute::Dimmer)] - 1.0).abs() < 1e-6);
        assert_eq!(p.captured().len(), 1, "still held for record");

        p.blind = false;
        let mut out = base();
        p.apply_to(&mut out, &show(&groups), 0.0);
        assert!(
            (out[&(1, Attribute::Dimmer)] - 1.0).abs() < 1e-6,
            "un-blind and it lands"
        );
    }

    // ── highlight / lowlight ─────────────────────────────────────────

    /// Highlight beats the hand and the masters: it is for finding a
    /// fixture, and a fixture a master has pulled to nothing cannot be
    /// found.
    /// r[verify playback.highlight]
    #[test]
    fn highlight_puts_the_selection_at_open_white_above_everything() {
        let groups = groups();
        let venue = roles();
        let show = show_with_roles(&groups, &venue);
        let mut p = Programmer::new();
        p.set_master("Key", 0.0);
        p.select(Selection::Chans(vec![1]));
        p.apply(RecipeApply::Dimmer(0.2), &show);
        p.highlight = true;

        let mut out = HashMap::new();
        out.insert((1, Attribute::Dimmer), 0.5);
        out.insert((2, Attribute::Dimmer), 0.5);
        p.apply_to(&mut out, &show, 0.0);
        assert!((out[&(1, Attribute::Dimmer)] - 1.0).abs() < 1e-6);
        for channel in [ColorChannel::Red, ColorChannel::Green, ColorChannel::Blue] {
            assert!((out[&(1, Attribute::ColorAdd { channel })] - 1.0).abs() < 1e-6);
        }
        assert_eq!(
            out[&(2, Attribute::Dimmer)],
            0.0,
            "unselected still under its master"
        );
    }

    /// r[verify playback.highlight] - lowlight
    #[test]
    fn lowlight_dims_everything_not_selected_to_the_floor() {
        let groups = groups();
        let mut p = Programmer::new();
        p.select(Selection::Chans(vec![1]));
        p.lowlight = true;

        let mut out = base();
        out.insert((1, Attribute::Dimmer), 0.7);
        out.insert((2, Attribute::Dimmer), 0.7);
        out.insert((3, Attribute::Dimmer), 0.05);
        p.apply_to(&mut out, &show(&groups), 0.0);
        assert!(
            (out[&(1, Attribute::Dimmer)] - 0.7).abs() < 1e-6,
            "selected untouched"
        );
        assert!((out[&(2, Attribute::Dimmer)] - p.lowlight_floor).abs() < 1e-6);
        assert!(
            (out[&(3, Attribute::Dimmer)] - 0.05).abs() < 1e-6,
            "already below the floor"
        );
    }

    // ── parks, grand master, selection masters ───────────────────────

    /// A parked tilt ignores the cue, the hand and the masters.
    /// r[verify playback.park]
    #[test]
    fn a_park_ignores_cues_hand_and_masters() {
        let groups = groups();
        let venue = roles();
        let show = show_with_roles(&groups, &venue);
        let mut p = Programmer::new();
        p.park(&Selection::Chans(vec![1]), Attribute::Tilt, 12.0, &show);
        p.park_chan(1, Attribute::Dimmer, 0.3);
        p.set_master("Key", 0.0);
        p.select(Selection::Chans(vec![1]));
        p.apply(RecipeApply::Dimmer(1.0), &show);
        p.apply(RecipeApply::Raw(vec![(Attribute::Tilt, 90.0)]), &show);
        p.highlight = true;

        let mut out = HashMap::new();
        out.insert((1, Attribute::Tilt), -40.0); // the cue
        out.insert((1, Attribute::Dimmer), 1.0);
        p.apply_to(&mut out, &show, 0.0);
        assert!((out[&(1, Attribute::Tilt)] - 12.0).abs() < 1e-6);
        assert!(
            (out[&(1, Attribute::Dimmer)] - 0.3).abs() < 1e-6,
            "a parked intensity beats the master, the hand and highlight"
        );
        assert!(p.is_active());

        p.unpark(&Selection::Chans(vec![1]), &Attribute::Tilt, &show);
        p.unpark_chan(1, &Attribute::Dimmer);
        let mut out = HashMap::new();
        out.insert((1, Attribute::Tilt), -40.0);
        p.apply_to(&mut out, &show, 0.0);
        assert!(
            (out[&(1, Attribute::Tilt)] - 90.0).abs() < 1e-6,
            "the hand is back"
        );
    }

    /// r[verify playback.park] - a DMX channel, on the wire
    #[test]
    fn a_dmx_park_writes_over_the_encoded_frame() {
        let mut p = Programmer::new();
        p.park_dmx(1, 3, 200);
        p.park_dmx(2, 1, 9);
        let mut frame = [0u8; 4];
        p.apply_parked_dmx(1, &mut frame);
        assert_eq!(frame, [0, 0, 200, 0]);
        p.unpark_dmx(1, 3);
        let mut frame = [0u8; 4];
        p.apply_parked_dmx(1, &mut frame);
        assert_eq!(frame, [0; 4]);
        // Out of range is ignored rather than a panic.
        p.park_dmx(1, 512, 1);
        p.apply_parked_dmx(1, &mut frame);
    }

    /// The grand master scales every intensity last — parked intensity
    /// too — and nothing but intensity.
    /// r[verify playback.grand-master]
    #[test]
    fn the_grand_master_scales_every_intensity_last() {
        let groups = groups();
        let mut p = Programmer::new();
        p.set_grand(0.5);
        p.park_chan(2, Attribute::Dimmer, 1.0);
        p.select(Selection::Chans(vec![3]));
        p.apply(RecipeApply::Dimmer(1.0), &show(&groups));

        let mut out = base();
        out.insert((1, Attribute::Dimmer), 0.8);
        out.insert((1, Attribute::Pan), 40.0);
        p.apply_to(&mut out, &show(&groups), 0.0);
        assert!((out[&(1, Attribute::Dimmer)] - 0.4).abs() < 1e-6, "the cue");
        assert!((out[&(2, Attribute::Dimmer)] - 0.5).abs() < 1e-6, "a park");
        assert!(
            (out[&(3, Attribute::Dimmer)] - 0.5).abs() < 1e-6,
            "the hand"
        );
        assert!((out[&(1, Attribute::Pan)] - 40.0).abs() < 1e-6, "not pan");
    }

    /// r[verify playback.selection-master]
    #[test]
    fn a_selection_master_rides_the_fixtures_in_hand() {
        let groups = groups();
        let venue = roles();
        let show = show_with_roles(&groups, &venue);
        let mut p = Programmer::new();
        p.set_selection_master(
            Selection::Chans(vec![1, 2]),
            Master {
                mode: MasterMode::Scaling,
                level: 0.5,
            },
        );
        let mut out = HashMap::new();
        out.insert((1, Attribute::Dimmer), 1.0);
        out.insert((2, Attribute::Dimmer), 0.4);
        out.insert((3, Attribute::Dimmer), 1.0);
        p.apply_to(&mut out, &show, 0.0);
        assert!((out[&(1, Attribute::Dimmer)] - 0.5).abs() < 1e-6);
        assert!((out[&(2, Attribute::Dimmer)] - 0.2).abs() < 1e-6);
        assert!(
            (out[&(3, Attribute::Dimmer)] - 1.0).abs() < 1e-6,
            "not in hand"
        );

        // Same modes as a role master, and folded with them: the lowest
        // scaling master wins on a shared fixture.
        p.set_master("Key", 0.2);
        let mut out = HashMap::new();
        out.insert((1, Attribute::Dimmer), 1.0);
        p.apply_to(&mut out, &show, 0.0);
        assert!((out[&(1, Attribute::Dimmer)] - 0.2).abs() < 1e-6);

        p.set_selection_master(
            Selection::Chans(vec![1, 2]),
            Master {
                mode: MasterMode::Additive,
                level: 0.9,
            },
        );
        assert_eq!(p.selection_masters.len(), 1, "replaced, not added");
        p.clear_selection_master(&Selection::Chans(vec![1, 2]));
        assert!(p.selection_masters.is_empty());
    }

    // ── speed keys ───────────────────────────────────────────────────

    /// r[verify playback.speed-keys] - learn, half, double, reset
    #[test]
    fn eight_taps_at_120_learn_120_and_half_is_60() {
        let mut tap = TapMaster::default();
        assert_eq!(tap.bpm(), None);
        for i in 0..8 {
            tap.tap(i as f32 * 0.5);
        }
        let bpm = tap.bpm().unwrap();
        assert!((bpm - 120.0).abs() < 0.5, "{bpm}");
        tap.half();
        assert!((tap.bpm().unwrap() - 60.0).abs() < 0.5);
        tap.double();
        tap.double();
        assert!((tap.bpm().unwrap() - 240.0).abs() < 0.5);
        tap.reset();
        assert!((tap.bpm().unwrap() - 120.0).abs() < 0.5);
    }

    /// One early tap moves the average a little, not the tempo a lot.
    /// r[verify playback.speed-keys] - converges rather than jitters
    #[test]
    fn learn_averages_rather_than_takes_the_last_interval() {
        let mut tap = TapMaster::default();
        let mut t = 0.0;
        for i in 0..8 {
            tap.tap(t);
            t += if i == 6 { 0.4 } else { 0.5 };
        }
        let bpm = tap.bpm().unwrap();
        assert!((bpm - 120.0).abs() < 5.0, "{bpm}");
        assert!(bpm > 120.0);
    }

    /// r[verify playback.speed-keys] - a gap starts over
    #[test]
    fn a_two_second_gap_starts_a_new_run() {
        let mut tap = TapMaster::default();
        for i in 0..4 {
            tap.tap(i as f32 * 0.5);
        }
        tap.tap(10.0);
        assert_eq!(tap.taps.len(), 1);
        assert!(
            (tap.bpm().unwrap() - 120.0).abs() < 0.5,
            "the old tempo is kept"
        );
        tap.tap(11.0);
        assert!((tap.bpm().unwrap() - 60.0).abs() < 0.5);
    }

    /// The keys reach the tap master, and its tempo reaches the `Tap`
    /// speed master a recipe names.
    /// r[verify playback.speed-keys]
    #[test]
    fn speed_keys_drive_the_tap_master() {
        let mut p = Programmer::new();
        for i in 0..5 {
            p.set_now(i as f32 * 0.5);
            assert_eq!(p.key_down(0, KeyAction::Learn), None);
        }
        p.key_down(0, KeyAction::HalfSpeed);
        let speeds = p.speeds_for(&crate::step::SpeedMasters::new());
        assert!((speeds["Tap"] - 60.0).abs() < 0.5);
        p.key_down(0, KeyAction::DoubleSpeed);
        p.key_down(0, KeyAction::ResetSpeed);
        let speeds = p.speeds_for(&crate::step::SpeedMasters::from([(
            "Song".to_string(),
            90.0,
        )]));
        assert!((speeds["Tap"] - 120.0).abs() < 0.5);
        assert_eq!(speeds["Song"], 90.0);
        let untouched = Programmer::new().speeds_for(&crate::step::SpeedMasters::new());
        assert!(!untouched.contains_key("Tap"), "no tempo learned, no Tap");
    }

    // ── temp / transport keys ────────────────────────────────────────

    /// r[verify playback.temp-and-pause] - temp
    #[test]
    fn temp_arrives_over_the_program_time_and_leaves_over_the_same() {
        let groups = groups();
        let masters = crate::step::SpeedMasters::from([("Song".to_string(), 120.0)]);
        let show = show_at(&groups, &masters);
        let mut p = Programmer::new();
        p.program_time_beats = 2.0; // one second
        p.set_fader(0, dimmer_fader(1, 1.0, 0.2));
        let at = |p: &Programmer, secs: f32| {
            let mut out = base();
            p.apply_to(&mut out, &show, secs);
            out[&(1, Attribute::Dimmer)]
        };
        assert!((at(&p, 0.0) - 0.2).abs() < 1e-6);
        p.set_now(10.0);
        assert_eq!(p.key_down(0, KeyAction::Temp), None);
        assert!((at(&p, 10.0) - 0.2).abs() < 1e-6, "starts where it was");
        assert!((at(&p, 10.5) - 0.6).abs() < 1e-6, "halfway");
        assert!((at(&p, 11.0) - 1.0).abs() < 1e-6, "full");
        assert!((at(&p, 13.0) - 1.0).abs() < 1e-6, "and held");
        p.key_up(0);
        assert!(
            (at(&p, 13.0) - 1.0).abs() < 1e-6,
            "release starts from full"
        );
        assert!((at(&p, 13.5) - 0.6).abs() < 1e-6);
        assert!((at(&p, 14.0) - 0.2).abs() < 1e-6, "back where it was");
        assert_eq!(p.faders[0].level, 0.2, "the fader itself never moved");
    }

    /// A temp released mid-lift comes back from where it got to, not
    /// from full.
    #[test]
    fn a_temp_released_early_returns_from_where_it_was() {
        let groups = groups();
        let masters = crate::step::SpeedMasters::from([("Song".to_string(), 120.0)]);
        let show = show_at(&groups, &masters);
        let mut p = Programmer::new();
        p.program_time_beats = 2.0;
        p.set_fader(0, dimmer_fader(1, 1.0, 0.0));
        let at = |p: &Programmer, secs: f32| {
            let mut out = base();
            p.apply_to(&mut out, &show, secs);
            out.get(&(1, Attribute::Dimmer)).copied().unwrap_or(0.0)
        };
        p.set_now(0.0);
        p.key_down(0, KeyAction::Temp);
        assert!((at(&p, 0.5) - 0.5).abs() < 1e-6);
        p.key_up(0);
        assert!((at(&p, 0.5) - 0.5).abs() < 1e-6, "no jump at release");
        assert!((at(&p, 1.0) - 0.25).abs() < 1e-6);
        assert!(at(&p, 1.5).abs() < 1e-6);
    }

    /// Transport keys are not fader keys: they come back as requests
    /// for the playbacks, and touch no slot.
    /// r[verify playback.temp-and-pause] - pause / go back keys
    #[test]
    fn transport_keys_are_handed_to_the_playbacks() {
        let mut p = Programmer::new();
        assert_eq!(
            p.key_down(0, KeyAction::Pause),
            Some(Transport::TogglePause)
        );
        assert_eq!(p.key_down(3, KeyAction::Load), Some(Transport::Load(3)));
        assert_eq!(p.key_down(0, KeyAction::GoBack), Some(Transport::GoBack));
        assert!(!p.is_active());
        assert!(!KeyAction::Pause.is_fader_key());
        assert!(KeyAction::Temp.is_fader_key());
    }

    fn roles() -> Bound {
        let mut bound = Bound::default();
        bound
            .0
            .insert("Key".into(), Selection::Chans(vec![1, 2, 3]));
        bound
    }

    fn two_roles() -> Bound {
        let mut bound = roles();
        bound.0.insert("Wash".into(), Selection::Chans(vec![2, 4]));
        bound
    }

    #[derive(Default)]
    struct Bound(BTreeMap<String, Selection>);

    impl crate::selection::Roles for Bound {
        fn role(&self, name: &str) -> Option<&Selection> {
            self.0.get(name)
        }
    }

    fn show_with_roles<'a>(groups: &'a [Group], roles: &'a Bound) -> Show<'a> {
        Show {
            groups,
            palettes: crate::Palettes::EMPTY,
            rig: &EMPTY_RIG,
            speeds: &crate::recipe::NO_SPEEDS,
            roles,
            ..Show::new(groups, &EMPTY_RIG)
        }
    }

    // ── filters, parameters, protection, blackout ────────────────────

    /// A role binding for the tests below: every role is the pars.
    struct AllPars;
    impl crate::selection::Roles for AllPars {
        fn role(&self, _: &str) -> Option<&Selection> {
            static PARS: std::sync::LazyLock<Selection> =
                std::sync::LazyLock::new(|| Selection::Group("Pars".into()));
            Some(&PARS)
        }
    }

    fn colour_and_level(chan: ChanId) -> Recipe {
        let mut r = Recipe::new(Selection::Chans(vec![chan]), RecipeApply::Dimmer(1.0));
        r.steps[0]
            .apply
            .push(RecipeApply::Color(Ref::Inline(ColorPreset {
                name: "Red".into(),
                red: 1.0,
                green: 0.0,
                blue: 0.0,
                ..Default::default()
            })));
        r
    }

    /// r[verify profile.attribute-filter]
    /// r[verify playback.attribute-filter]
    #[test]
    fn a_filtered_fader_drops_emits_outside_the_filter() {
        let groups = groups();
        let mut p = Programmer::new();
        p.set_fader(
            0,
            Fader {
                name: "RED".into(),
                recipe: Some(colour_and_level(1)),
                level: 1.0,
                filter: AttrFilter::COLOUR,
                ..Default::default()
            },
        );
        let red = Attribute::ColorAdd {
            channel: ignition_proto::ColorChannel::Red,
        };
        let mut out = base();
        out.insert((1, Attribute::Dimmer), 0.3);
        p.apply_to(&mut out, &show(&groups), 0.0);
        assert_eq!(out[&(1, red)], 1.0, "the colour lands");
        assert_eq!(
            out[&(1, Attribute::Dimmer)],
            0.3,
            "the level was outside the filter and withdrew nothing"
        );
        assert!(AttrFilter::POSITION.admits(&Attribute::Tilt));
        assert!(!AttrFilter::POSITION.admits(&Attribute::Zoom));
        assert!(AttrFilter::BEAM.admits(&Attribute::Custom("x".into())));
    }

    /// r[verify profile.effect-parameters]
    /// r[verify playback.effect-parameters]
    #[test]
    fn effect_parameters_apply_at_fold_and_leave_the_recipe_alone() {
        let groups = groups();
        let mut p = Programmer::new();
        let pulse = Recipe {
            target: Selection::Chans(vec![1]),
            steps: vec![
                Step::new(vec![RecipeApply::Delta(vec![(Attribute::Dimmer, 0.5)])]),
                Step::new(vec![RecipeApply::Delta(vec![(Attribute::Dimmer, 0.0)])]),
            ],
            timing: Timing {
                speed: crate::step::Speed::Hz(1.0),
                ..Default::default()
            },
            ..Default::default()
        };
        p.set_fader(
            0,
            Fader {
                name: "PULSE".into(),
                recipe: Some(pulse.clone()),
                level: 1.0,
                ..Default::default()
            },
        );
        // Depth halves the swing.
        p.set_param(0, "depth", 0.5);
        let mut out = base();
        out.insert((1, Attribute::Dimmer), 0.2);
        p.apply_to(&mut out, &show(&groups), 0.0);
        assert!(
            (out[&(1, Attribute::Dimmer)] - 0.45).abs() < 0.01,
            "{out:?}"
        );
        // Bars stretches the loop: at 4 bars (16 beats) the first step
        // holds for 8 beats, so 0.3 cycles-worth of seconds is still
        // on step 0 where it would have moved on.
        p.set_param(0, "depth", 1.0);
        p.set_param(0, "bars", 4.0);
        let mut out = base();
        out.insert((1, Attribute::Dimmer), 0.0);
        p.apply_to(&mut out, &show(&groups), 0.6);
        assert!((out[&(1, Attribute::Dimmer)] - 0.5).abs() < 0.01, "{out:?}");
        // Duty gives the first step a tenth of the cycle.
        p.set_param(0, "bars", 0.25);
        p.set_param(0, "duty", 0.1);
        let mut out = base();
        out.insert((1, Attribute::Dimmer), 0.0);
        p.apply_to(&mut out, &show(&groups), 0.2);
        assert!(
            out[&(1, Attribute::Dimmer)].abs() < 0.01,
            "off after the duty: {out:?}"
        );
        // The recipe on the fader is untouched.
        assert_eq!(p.faders[0].recipe.as_ref(), Some(&pulse));
        assert_eq!(p.faders[0].params.get("duty"), Some(&0.1));
    }

    fn lit_base() -> HashMap<(ChanId, Attribute), f32> {
        let mut out = base();
        for chan in [1, 2, 3] {
            out.insert((chan, Attribute::Dimmer), 1.0);
        }
        out
    }

    /// r[verify profile.protected-roles]
    /// r[verify playback.protected-untouched]
    /// r[verify playback.grand-master]
    #[test]
    fn protected_roles_survive_the_grand_master_the_black_key_and_a_safe_look() {
        let groups = groups();
        let show = Show {
            roles: &AllPars,
            ..show(&groups)
        };
        let mut p = Programmer::new();
        p.protected = vec!["House Lights".into()];
        p.set_grand(0.0);
        let mut out = lit_base();
        p.apply_to(&mut out, &show, 0.0);
        assert_eq!(
            out[&(1, Attribute::Dimmer)],
            1.0,
            "the grand master skips the house"
        );

        // A safe held look — the blackout look — leaves them too.
        p.set_grand(1.0);
        p.hold_look(
            vec![Recipe::new(
                Selection::Role("Key".into()),
                RecipeApply::Dimmer(0.0),
            )],
            true,
        );
        let mut out = lit_base();
        p.apply_to(&mut out, &show, 0.0);
        assert_eq!(out[&(1, Attribute::Dimmer)], 1.0);
        // The same look held as a plain (unsafe) look does drop them.
        p.hold(Recipe::new(
            Selection::Role("Key".into()),
            RecipeApply::Dimmer(0.0),
        ));
        let mut out = lit_base();
        p.apply_to(&mut out, &show, 0.0);
        assert_eq!(out[&(1, Attribute::Dimmer)], 0.0);
        p.release_hold();

        // The black key on a fader that lights them keeps lighting them.
        p.set_fader(
            0,
            Fader {
                name: "HOUSE".into(),
                recipe: Some(Recipe::new(
                    Selection::Role("House Lights".into()),
                    RecipeApply::Dimmer(0.8),
                )),
                level: 1.0,
                ..Default::default()
            },
        );
        p.key_down(0, KeyAction::Black);
        let mut out = base();
        p.apply_to(&mut out, &show, 0.0);
        assert!((out[&(1, Attribute::Dimmer)] - 0.8).abs() < 0.01);
        p.key_up(0);

        // And the hand still wins: a direct value reaches a protected role.
        // r[verify playback.hand-wins]
        p.select(Selection::Chans(vec![1]));
        p.apply(RecipeApply::Dimmer(0.25), &show);
        let mut out = lit_base();
        p.apply_to(&mut out, &show, 0.0);
        assert_eq!(out[&(1, Attribute::Dimmer)], 0.25);
    }

    /// r[verify playback.blackout]
    /// r[verify profile.macros.release]
    #[test]
    fn a_blackout_zeroes_every_intensity_but_the_protected_and_releases() {
        let groups = groups();
        let show = show(&groups);
        let mut p = Programmer::new();
        p.set_blackout(true);
        let mut out = lit_base();
        out.insert(
            (
                1,
                Attribute::ColorAdd {
                    channel: ColorChannel::Red,
                },
            ),
            1.0,
        );
        p.apply_to(&mut out, &show, 0.0);
        assert_eq!(out[&(2, Attribute::Dimmer)], 0.0);
        assert_eq!(
            out[&(
                1,
                Attribute::ColorAdd {
                    channel: ColorChannel::Red
                }
            )],
            1.0,
            "only intensity"
        );
        assert!(p.is_active());
        p.release_macro();
        assert!(!p.blackout);
        let mut out = lit_base();
        p.apply_to(&mut out, &show, 0.0);
        assert_eq!(out[&(2, Attribute::Dimmer)], 1.0);

        // Over the program time it fades rather than snaps.
        let mut speeds = crate::step::SpeedMasters::new();
        speeds.insert("Song".into(), 60.0);
        let timed = Show {
            speeds: &speeds,
            ..show
        };
        p.program_time_beats = 2.0;
        p.set_now(0.0);
        p.set_blackout(true);
        let mut out = lit_base();
        p.apply_to(&mut out, &timed, 1.0);
        assert!((out[&(2, Attribute::Dimmer)] - 0.5).abs() < 0.01, "{out:?}");
        // A protected role, resolved by the show, passes through.
        let protected = Show {
            roles: &AllPars,
            ..timed
        };
        p.protected = vec!["House Lights".into()];
        let mut out = lit_base();
        p.apply_to(&mut out, &protected, 5.0);
        assert_eq!(out[&(2, Attribute::Dimmer)], 1.0);
    }

    /// A fader that is a role master scales that role at its level.
    /// r[verify profile.pages]
    #[test]
    fn a_master_fader_scales_its_role() {
        let groups = groups();
        let show = Show {
            roles: &AllPars,
            ..show(&groups)
        };
        let mut p = Programmer::new();
        p.set_fader(
            0,
            Fader {
                name: "MOVERS".into(),
                master: Some("Movers".into()),
                level: 0.5,
                ..Default::default()
            },
        );
        let mut out = lit_base();
        p.apply_to(&mut out, &show, 0.0);
        assert!((out[&(1, Attribute::Dimmer)] - 0.5).abs() < 0.01);
    }

    /// r[verify playback.macro-effects]
    #[test]
    fn macro_effects_play_beside_the_faders_and_release_without_them() {
        let groups = groups();
        let show = show(&groups);
        let mut p = Programmer::new();
        p.set_fader(0, dimmer_fader(2, 0.6, 1.0));
        p.take_effect("lift", dimmer_fader(1, 1.0, 1.0).recipe.unwrap(), 0.5);
        let mut out = base();
        out.insert((1, Attribute::Dimmer), 0.0);
        p.apply_to(&mut out, &show, 0.0);
        assert!(
            (out[&(1, Attribute::Dimmer)] - 0.5).abs() < 0.01,
            "half weight"
        );
        assert!((out[&(2, Attribute::Dimmer)] - 0.6).abs() < 0.01);
        // Firing the same name again replaces rather than stacks.
        p.take_effect("lift", dimmer_fader(1, 1.0, 1.0).recipe.unwrap(), 1.0);
        assert_eq!(p.effects_playing(), vec!["lift"]);
        p.release_effects();
        let mut out = base();
        out.insert((1, Attribute::Dimmer), 0.0);
        p.apply_to(&mut out, &show, 0.0);
        assert_eq!(out[&(1, Attribute::Dimmer)], 0.0);
        assert!(
            (out[&(2, Attribute::Dimmer)] - 0.6).abs() < 0.01,
            "the fader stays"
        );
    }

    /// Size and rate are the operator's, not the recipe's.
    ///
    /// They are live controls: a hand on a wheel while the show runs,
    /// the same two numbers over every effect at once. Stored on the
    /// recipe instead, a library would need a timid spelling and a bold
    /// one of everything to work around their absence — which is the
    /// shape `r[recipes.live-control-is-not-stored]` refuses.
    ///
    /// r[verify effects.live-control-on-programmer]
    #[test]
    fn size_and_rate_live_on_the_programmer_and_not_on_a_recipe() {
        // They are here, and they start neutral.
        let programmer = Programmer::new();
        assert_eq!(programmer.size, 1.0);
        assert_eq!(programmer.rate, 1.0);

        // And nowhere on a recipe: a stored effect that carried its own
        // size would take it to every show that named it.
        let recipe = Recipe::new(Selection::Group("Pars".into()), RecipeApply::Dimmer(1.0));
        let json = serde_json::to_string(&recipe).expect("a recipe serialises");
        for live in ["\"size\"", "\"rate\""] {
            assert!(
                !json.contains(live),
                "a recipe carries {live}, so a library entry could ship one: {json}"
            );
        }
    }
}
