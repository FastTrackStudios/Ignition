//! What the UI says to the visualizer.
//!
//! Dioxus components and the Bevy app live on the same thread but in
//! different ownership worlds — the app is inside a Blitz widget the
//! components cannot reach. A channel is the seam: components send, the
//! widget drains before it steps. That also means every control is a
//! *message*, which is what a hardware surface or a remote will send
//! later without the UI having to change shape.

use ignition_core::{Attribute, Class, FADERS, Fader, KeyAction, Recipe, Selection};

/// `Deselect` and `Section` have no control on the surface yet.
///
/// Kept rather than deleted because this enum is a *wire contract*: the
/// widget handles both, and the surface that will send them — a clear
/// key, a rehearsal jump — is UI work rather than a change here.
/// Deleting them would mean re-deriving the handler when the control
/// arrives, which is how a protocol loses the pieces nobody happened to
/// need on the day.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum Command {
    /// Pick the fixtures the next palette hit lands on.
    Select(Selection),
    Deselect,
    /// A colour palette entry, by name.
    Color(String),
    /// A colour split — a named multi-colour palette entry, by name.
    Split(String),
    /// A focus palette entry, by name.
    Focus(String),
    /// Intensity for the current selection.
    Dimmer(f32),
    /// Take the operator's hand off the current selection.
    Release,
    ClearValues,
    Fader(usize, Box<Fader>),
    /// Assign a fader on a page that need not be the current one — how
    /// the bank's second page is loaded before anyone has turned to it.
    /// Pages that do not exist yet are created up to `page`.
    FaderOnPage {
        page: usize,
        index: usize,
        fader: Box<Fader>,
    },
    Level(usize, f32),
    /// A playback key on a fader: flash, toggle, swap, kill or black.
    /// `down` is the hand going on or coming off — toggle and kill
    /// ignore the release, the others are held.
    Key {
        index: usize,
        action: KeyAction,
        down: bool,
    },
    /// Turn the fader bank to another page. A fader that is up stays
    /// live on its old assignment until it is brought back to match.
    Page(PageMove),
    /// How long a punched value takes to arrive, in beats. Zero snaps.
    ProgramTime(f32),
    /// Blind: the programmer's values are held and previewed in the
    /// viewport but do not reach output.
    Blind(bool),
    /// The selection to open white at full, above every layer.
    Highlight(bool),
    /// Everything *not* selected capped low.
    Lowlight(bool),
    /// The `Tap` master, in BPM — what the sound-in's beat detector
    /// drives, and what a tap key would. Distinct from `Rate`, which is
    /// the surface's own master the bank is slaved to.
    Tap(f32),
    /// Band levels from the audio input, 0..=1 each. Stored on the
    /// playback so a recipe can be driven by the kick later.
    SoundLevels {
        low: f32,
        mid: f32,
        high: f32,
    },
    /// The master rate every fader's phaser is slaved to, in BPM.
    Rate(f32),
    /// How far every effect swings, 0..=1 — the control an operator
    /// holds for most of a night. Distinct from a master: size flattens
    /// the effect, a master dims the fixtures.
    Size(f32),
    /// A multiplier on every effect's rate against its speed master.
    EffectRate(f32),
    /// A role's intensity master, 0..=1.
    Master(String, f32),
    /// Play one role on its own; `None` clears.
    Solo(Option<String>),
    /// Fire a bump by hand — a flash key.
    ///
    /// Carries the same shape a charted hit fires, so the two are one
    /// gesture arriving from two places rather than two features that
    /// happen to look alike.
    ///
    /// A selection rather than a role, because a stab is on the whole
    /// rig and a blinder hit is on one role — the same key shape wants
    /// both.
    Flash(Selection, ignition_core::BumpKind),
    /// Hold a look at full for as long as a key is down; `None` is the
    /// key coming up. The punt look and the rig drop — the two keys an
    /// operator reaches for when the *show* is wrong, which is why they
    /// are held rather than fired: the hand is the release.
    Hold(Option<Box<Recipe>>),
    /// GO on the song list.
    Go,
    /// GO on the look list — the list the operator steps through by
    /// hand, beneath the song's.
    LookGo,
    Cue(usize),
    /// Transport. The song is loaded once at startup; these move it.
    Play,
    Stop,
    /// Locate to a section by name — how a rehearsal loops one part.
    Section(String),
    /// Move the playhead to a fraction of the song, 0..=1 — what
    /// dragging the progress bar sends.
    Scrub(f32),
    /// Move the playhead to a musical position — what clicking a hit in
    /// the list sends, since a hit is not a cue and has no index to GO.
    Locate(ignition_core::Bars),
    /// The grand master, 0..=1: every intensity scaled last of all, so
    /// the whole rig comes down with one hand and nothing else moves.
    // r[impl playback.grand-master] - the surface's GM fader
    Grand(f32),
    /// One playback's own intensity master, by class — the song list
    /// pulled under the look list without touching either's content.
    // r[impl playback.playback-master] - the SONG and LOOK faders
    PlaybackMaster(Class, f32),
    /// Nail `attrs` of every fixture in `selection` at the value the
    /// programmer is holding for it, above every playback and the hand.
    /// A fixture the hand is not holding on an attribute is left alone:
    /// a park is "keep it where I put it", and nothing was put.
    // r[impl playback.park] - park from the selection
    Park {
        selection: Selection,
        attrs: Vec<Attribute>,
    },
    // r[impl playback.park] - and unpark it
    Unpark {
        selection: Selection,
        attrs: Vec<Attribute>,
    },
    /// A speed key on the `Tap` master: a learn tap, half, double or a
    /// reset to the learned tempo. Its own command rather than a
    /// `Key` on a slot because it lands on no fader.
    // r[impl playback.speed-keys]
    Speed(SpeedKey),
    /// How long the sound-in's band levels take to settle, in seconds,
    /// 0–2. Zero is the raw meter; two seconds is a swell.
    // r[impl playback.sound-as-value] - the sound fade
    SoundFade(f32),
    /// Whether lit fixture housings glow their own colour in the
    /// viewport. A render option, not a show value: it never reaches
    /// the engine or the wire.
    // r[impl viz.body-glow] - the studio's switch
    BodyGlow(bool),
    /// DMX out of the socket, on or off. The engine keeps running
    /// either way; this is the switch on the transmitter.
    // r[impl dmx.output-toggle] - the surface's OUTPUT key
    Output(bool),
    /// Run a profile macro by name — DROP, BUILD 8, BREAKDOWN, END.
    /// One runs at a time; a new one replaces the running one.
    // r[impl playback.macro-runner] - the surface's MACRO keys
    Macro(String),
    /// Take a profile look by name, latched on the programmer's held
    /// layer until `None` releases it or another look replaces it.
    // r[impl playback.look-hold] - the surface's LOOK keys
    Look(Option<String>),
    /// One effect parameter on a bank fader — `depth`, `bars`, `duty`
    /// — as the page declared it. The second thin control beside a
    /// fader's level.
    // r[impl profile.effect-parameters] - the surface's param control
    Param {
        index: usize,
        name: String,
        value: f32,
    },
}

/// The keys beside RATE. `Tap` is a learn tap — averaged, so a hand a
/// little early on one beat does not throw every phaser slaved to it.
// r[impl playback.speed-keys]
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpeedKey {
    Tap,
    Half,
    Double,
    Reset,
}

impl SpeedKey {
    /// The engine's key for this one.
    pub fn action(self) -> KeyAction {
        match self {
            SpeedKey::Tap => KeyAction::Learn,
            SpeedKey::Half => KeyAction::HalfSpeed,
            SpeedKey::Double => KeyAction::DoubleSpeed,
            SpeedKey::Reset => KeyAction::ResetSpeed,
        }
    }
}

/// Where a page turn goes. `Set` has no on-screen control — a remote's
/// `{"page": n}` binding sends it — and is kept for the same wire-
/// contract reason `Deselect` is.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageMove {
    Next,
    Prev,
    Set(usize),
}

pub type Sender = std::sync::mpsc::Sender<Command>;
pub type Receiver = std::sync::mpsc::Receiver<Command>;

pub fn channel() -> (Sender, Receiver) {
    std::sync::mpsc::channel()
}

/// Where the show actually is — the widget's answer back to the UI.
///
/// The commands above are one-way, and that was fine while every change
/// came from a click: the sidebar could just remember what it had sent.
/// The moment the *song* drives the cues, that memory becomes a second
/// source of truth and starts lying — it went on highlighting the last
/// cue clicked while the transport had moved several cues past it. So
/// the player's own state comes back, and the UI renders that rather
/// than anything it believes about itself.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Playhead {
    /// Index of the cue the player is actually standing on.
    pub cue: Option<usize>,
    /// Seconds into the song, and how long it is — the progress bar.
    pub secs: f32,
    pub length: f32,
    pub playing: bool,
    /// Index into the show's triggers of the hit that fired last, while
    /// it is still ringing. `None` between hits.
    pub hit: Option<usize>,
    /// Which page the fader bank is on, and how many there are.
    pub page: usize,
    pub pages: usize,
    /// Slots still playing the previous page's fader because they were
    /// up when the page turned — drawn as latched until the hand brings
    /// the physical fader back to match.
    pub latched: [bool; FADERS],
    /// Slots latched on by a toggle key.
    pub toggled: [bool; FADERS],
    /// Whether the programmer is blind, so the surface can say so —
    /// the viewport is showing a preview, not the output.
    pub blind: bool,
    /// The `Tap` master as the engine has it — what the sound-in
    /// detector last decided, or the default.
    pub tap_bpm: f32,
    /// The audio input's band levels, for the meters — smoothed by the
    /// sound fade, so the meters show what the recipes are reading.
    pub sound: [f32; 3],
    /// The grand master as the engine holds it.
    pub grand: f32,
    /// The song and look playbacks' own masters, in that order.
    pub playback_masters: [f32; 2],
    /// How many fixture attributes are parked.
    pub parked: usize,
    /// The multiplier the half/double keys have on the `Tap` master.
    pub tap_multiplier: f32,
    /// Whether the song playback is paused.
    pub paused: bool,
    /// Every bank fader's level as the engine has it — what a motorised
    /// surface is told after a page turn, and what a screen surface
    /// draws.
    // r[impl playback.remote-feedback] - fader positions travel back
    pub levels: [f32; FADERS],
    /// The transmitter's state — sending, which universes, at what
    /// rate, and any socket error — as the engine sees it.
    // r[impl dmx.output-toggle] - the transmit state travels back to the surface
    pub output: ignition_viz::OutputSummary,
}

impl Playhead {
    /// The song playback's master, for the surface.
    pub fn song_master(&self) -> f32 {
        self.playback_masters[0]
    }

    /// The look playback's master.
    pub fn look_master(&self) -> f32 {
        self.playback_masters[1]
    }
}

impl Playhead {
    /// How far through, 0..=1. A zero-length song reports 0 rather than
    /// NaN, which would otherwise reach a CSS percentage and lay the bar
    /// out at a width no one can explain.
    pub fn fraction(&self) -> f32 {
        if self.length > 0.0 {
            (self.secs / self.length).clamp(0.0, 1.0)
        } else {
            0.0
        }
    }
}

/// The latest playhead, written by the widget and read by the UI.
///
/// A watch channel rather than a queue: the UI wants the *current*
/// value, and a backlog of stale positions is worse than useless — it
/// would render the song's history one frame at a time, always behind.
pub type StateTx = tokio::sync::watch::Sender<Playhead>;
pub type StateRx = tokio::sync::watch::Receiver<Playhead>;

pub fn state_channel() -> (StateTx, StateRx) {
    tokio::sync::watch::channel(Playhead::default())
}
