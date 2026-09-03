//! The Live surface as a library: the same components on the studio's
//! desk and on an iPad in Safari.
//!
//! The split is at the transport. A component here never owns a
//! channel or a socket; it calls [`send`] and reads [`use_playhead`],
//! and the *host* — the studio binary or the web app — decides what
//! those are wired to. On the desktop that is the mpsc into the Blitz
//! widget and the tokio watch out of it; in the browser it is one
//! WebSocket carrying the same two types as JSON. Nothing in the
//! view knows which (`r[studio.touch.ipad]`, `r[studio.one-truth]`).

// r[impl studio.touch.ipad] - one set of components, native and wasm
// r[impl studio.one-truth] - the playhead is fed, never computed here

pub mod cameras;
pub mod command;
pub mod cuelist;
pub mod desk;
pub mod faders;
pub mod fixtures;
pub mod library;
pub mod live;
mod numeric;
pub mod operators;
pub mod panes;
pub mod patch;
pub mod pointer;
pub mod program;

pub use cuelist::CueList;
pub use fixtures::{ChannelRow, ModeRow, TypeLibrary, TypeRow};
pub use patch::{Conflict, Occupancy, PatchRow, PatchSheet};

pub use command::{Command, PageMove, Playhead, SpeedKey};
use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

// ── The transport seam ───────────────────────────────────────────────

/// Where commands go. The host implements this once and installs it.
///
/// `Send + Sync` because the studio installs it before any window
/// exists and every window's components call it; the browser is one
/// thread and satisfies the bound with a `futures` channel sender.
pub trait Bridge: Send + Sync {
    fn send(&self, command: Command);
}

impl<F: Fn(Command) + Send + Sync> Bridge for F {
    fn send(&self, command: Command) {
        self(command);
    }
}

static BRIDGE: std::sync::OnceLock<Box<dyn Bridge>> = std::sync::OnceLock::new();

/// Install the host's transport. Once per process; a second call is
/// ignored, which is what a hot-reloaded root wants.
pub fn install(bridge: impl Bridge + 'static) {
    let _ = BRIDGE.set(Box::new(bridge));
}

/// A free function rather than a captured closure.
///
/// Every control needs to send, closures in `rsx!` are `FnMut`, and a
/// captured sender cannot be moved out of one more than once. Before
/// `install`, a command is dropped — there is nothing to send it to.
pub fn send(command: Command) {
    if let Some(bridge) = BRIDGE.get() {
        bridge.send(command);
    }
}

/// The playhead the host feeds, as a signal in context.
///
/// The host owns the signal and writes it whenever the engine's state
/// moves — from a tokio watch on the desktop, from a WebSocket frame
/// in the browser. Components read it; the derived hooks below narrow
/// it so a fader column does not re-render on every tick of the song
/// clock.
#[derive(Clone, Copy)]
pub struct PlayheadFeed(pub Signal<Playhead>);

/// The whole playhead. Every read of this re-renders with the song
/// clock; prefer [`use_desk`] or [`use_current_cue`] for a control that
/// only cares about a slice.
#[must_use]
pub fn use_playhead() -> Signal<Playhead> {
    use_context::<PlayheadFeed>().0
}

/// Only the cue the player is standing on. A memo, so the cue list —
/// a hundred-odd rows that only care which one is lit — diffs only
/// when the cue actually changes.
#[must_use]
pub fn use_current_cue() -> Memo<Option<usize>> {
    let playhead = use_playhead();
    use_memo(move || playhead().cue)
}

/// The standing cue's fade, 0 to 1, and which cue is next with how long
/// until it takes itself.
///
/// Narrowed from the playhead for the same reason `use_current_cue` is:
/// a hundred rows should not re-render because the song clock moved.
// r[impl studio.cuelist.live-state]
#[must_use]
pub fn use_cue_progress() -> Memo<(f32, Option<usize>, Option<f32>)> {
    let playhead = use_playhead();
    use_memo(move || {
        let p = playhead();
        // A countdown is quantised to a tenth, so the memo settles
        // between frames instead of diffing sixty times a second.
        let next_in = p.next_in.map(|s| (s * 10.0).round() / 10.0);
        (((p.cue_fade * 100.0).round() / 100.0), p.next_cue, next_in)
    })
}

/// Which hit is ringing, for the list.
#[must_use]
pub fn use_ringing_hit() -> Memo<Option<usize>> {
    let playhead = use_playhead();
    use_memo(move || playhead().hit)
}

/// The desk's own state — page, latches, blind — narrowed from the
/// playhead for the same reason `use_current_cue` is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Desk {
    pub page: usize,
    pub pages: usize,
    pub latched: [bool; ignition_core::FADERS],
    pub toggled: [bool; ignition_core::FADERS],
    pub blind: bool,
    pub tap_bpm: f32,
    pub tap_multiplier: f32,
    pub sound: [f32; 3],
    pub parked: usize,
    pub paused: bool,
}

impl Desk {
    #[must_use]
    pub fn of(playhead: &Playhead) -> Self {
        Self {
            page: playhead.page,
            pages: playhead.pages.max(1),
            latched: playhead.latched,
            toggled: playhead.toggled,
            blind: playhead.blind,
            tap_bpm: playhead.tap_bpm,
            tap_multiplier: playhead.tap_multiplier,
            sound: playhead.sound,
            parked: playhead.parked,
            paused: playhead.paused,
        }
    }
}

#[must_use]
pub fn use_desk() -> Memo<Desk> {
    let playhead = use_playhead();
    use_memo(move || Desk::of(&playhead()))
}

// ── What the surface is made of ──────────────────────────────────────

/// A colour palette entry as the surface draws it: the name to send, and
/// the colour to show. A colour pool that does not show its colours is a
/// list of words, which is the whole reason to have a pool.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColorChip {
    pub name: String,
    /// The flat fill, for anywhere the swatch is a bar rather than a
    /// disc — the library's thin strip, say. A multi-colour palette
    /// flattens to a left-to-right gradient here.
    pub css: String,
    /// Every colour the palette holds, in the order it applies them.
    /// One entry is an ordinary colour; several is what makes this a
    /// palette rather than a swatch, and a disc has to *show* that —
    /// a multi-colour preset drawn as one averaged blob is a preset an
    /// operator picks by accident.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub colors: Vec<String>,
    /// The colours blend into each other across the rig rather than
    /// landing as hard bands, so the disc sweeps rather than segments.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub spread: bool,
    /// The swatch is light enough that a name written on it has to be
    /// dark. Decided where the colour is still numbers rather than a
    /// CSS string, because parsing it back to guess is how a label ends
    /// up white on Open White.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub light: bool,
}

impl ColorChip {
    /// A single colour, with no palette behind it.
    #[must_use]
    pub fn solid(name: impl Into<String>, css: impl Into<String>) -> Self {
        let css = css.into();
        Self {
            name: name.into(),
            colors: vec![css.clone()],
            css,
            spread: false,
            light: false,
        }
    }

    /// Whether a name written on this swatch should be dark.
    ///
    /// Relative luminance, the sRGB weights: green carries most of what
    /// the eye reads as brightness, blue almost none, which is why a
    /// plain average calls Deep Blue light and Straw dark — both
    /// backwards.
    #[must_use]
    pub fn is_light(red: f32, green: f32, blue: f32) -> bool {
        0.0722f32.mul_add(blue, 0.2126f32.mul_add(red, 0.7152 * green)) > 0.55
    }

    /// The fill for a **round** swatch.
    ///
    /// A disc is a wheel, so a palette's colours belong round it as
    /// wedges — the way a grandMA3 pool cell shows a preset holding
    /// several colours. A left-to-right gradient squeezed into a circle
    /// reads as one muddy colour at swatch size, which is the whole
    /// problem: two palettes that differ only in their third colour
    /// look identical.
    #[must_use]
    pub fn disc(&self) -> String {
        match self.colors.len() {
            0 => self.css.clone(),
            1 => self
                .colors
                .first()
                .map_or_else(|| self.css.clone(), Clone::clone),
            n if self.spread => {
                // A sweep has to close the loop or the last colour meets
                // the first at a seam the eye reads as a wedge that is
                // not there.
                let mut stops: Vec<String> = self
                    .colors
                    .iter()
                    .enumerate()
                    .map(|(i, c)| format!("{c} {:.2}deg", count_f32(i) * 360.0 / count_f32(n)))
                    .collect();
                let Some(first) = self.colors.first() else {
                    return self.css.clone();
                };
                stops.push(format!("{first} 360deg"));
                format!("conic-gradient({})", stops.join(", "))
            }
            n => {
                // Hard wedges: each colour owns its arc outright, which
                // is what "cycle" and "block" actually do on the rig.
                let step = 360.0 / count_f32(n);
                let wedges: Vec<String> = self
                    .colors
                    .iter()
                    .enumerate()
                    .map(|(i, c)| {
                        format!(
                            "{c} {:.2}deg {:.2}deg",
                            count_f32(i) * step,
                            count_f32(i.saturating_add(1)) * step
                        )
                    })
                    .collect();
                format!("conic-gradient({})", wedges.join(", "))
            }
        }
    }
}

/// A wedge count or index as a float, for spacing a palette's colours
/// round a disc.
///
/// A colour chip's palette is a handful of entries at most — nowhere
/// near the 2^24 where an `f32` stops counting integers exactly — so the
/// conversion is total in practice, and this is the one place it happens.
#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "a palette's colour count is small; see the doc comment"
)]
const fn count_f32(n: usize) -> f32 {
    n as f32
}

/// The named things the surface offers, resolved once from the venue by
/// the host — and, for the browser, sent over once at connect.
#[derive(Debug, Clone, Default, Props, PartialEq, Serialize, Deserialize)]
pub struct Surface {
    pub groups: Vec<String>,
    pub colors: Vec<ColorChip>,
    /// Multi-colour palette entries, drawn as a split disc — the way a
    /// grandMA3 colour preset holding several colours shows in its
    /// picker.
    pub splits: Vec<ColorChip>,
    pub focus: Vec<String>,
    pub cues: Vec<Row>,
}

/// One line of the cue list: a cue the operator can GO, or a hit the
/// song fires.
///
/// Hits are shown because an operator wants to see what is coming and
/// what just landed; they are not in the GO order — see
/// `docs/spec/triggers.md`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Row {
    Cue(Box<CueRow>),
    Hit {
        index: usize,
        name: String,
        at: ignition_core::Bars,
    },
}

/// Everything the list draws for one cue, pre-formatted.
///
/// A presentation struct, not a cue: every number is already a string in
/// the units it is shown in, so the component does no domain work and
/// the browser client needs no copy of the engine to render a sheet.
/// The studio fills it once at load; nothing here changes per frame
/// (what does — the standing cue, its fade's progress, a ringing hit —
/// rides the playhead).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CueRow {
    /// Where this cue sits in the list — what `Command::Cue` takes.
    pub index: usize,
    /// What it is *called*, which is not the index.
    // r[impl cues.number]
    pub number: String,
    pub name: String,
    // r[impl cues.note]
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
    // r[impl cues.appearance]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub appearance: Option<ignition_core::cue::Appearance>,
    /// The musical position, as the author wrote it — "CH 1 +4".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<String>,
    /// Where the clock finds it, for a click that locates there.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<ignition_core::Bars>,
    pub trig: TrigKind,
    /// The cue's own fade — "2.0s".
    pub fade: String,
    /// One cell per attribute class, condensed: fade and delay
    /// together, and only where the class differs from the cue's own.
    // r[impl studio.cuelist.condensed-timing]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub timing: Vec<ClassCell>,
    pub flags: Flags,
    /// Pre-positioning, when it is not the default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mib: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub commands: Vec<String>,
    /// What this cue resolves to on the rig it was loaded against.
    /// `None` when nothing has cooked it — an honest blank, not a
    /// guessed green.
    // r[impl studio.cuelist.status]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<CookStatus>,
    /// Its parts, one row each when the cue is expanded.
    // r[impl studio.cuelist.expand]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parts: Vec<PartRow>,
}

/// One part of a cue — a recipe, or a named timing override — as an
/// indented sub-row. A cue's recipes *are* its parts, so both draw the
/// same shape and a cue has one kind of subdivision rather than two.
// r[impl cues.parts-are-recipes]
// r[impl studio.cuelist.expand]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartRow {
    /// The part number — the recipe's place in the cue.
    pub number: usize,
    /// The part's own name, else what it is: a library effect's name, a
    /// look's, or the selection an override names.
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
    /// Who it covers, as written — "role Movers".
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub target: String,
    /// Its own arrival, when it carries one.
    // r[impl cues.recipe.timing]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub timing: Vec<ClassCell>,
    /// How many fixtures it resolved to, when something cooked it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub covers: Option<usize>,
    /// A timing override rather than a recipe: same shape, drawn
    /// quieter, because it sets no values.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub is_override: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub disabled: bool,
}

/// One class's timing, condensed to a single cell.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClassCell {
    /// "dim", "col", "pos", "beam".
    pub class: String,
    /// "2.0" — or "2.0/0.5" when the fade and delay differ, which is
    /// the whole reason this is one column instead of eight.
    pub value: String,
    /// The curve, when it is not linear.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ease: Option<String>,
}

/// How a cue is taken.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
// r[impl cues.trig]
pub enum TrigKind {
    Go,
    At,
    Follow,
    Sound,
}

impl TrigKind {
    /// The glyph the list draws. One character, because the trigger
    /// column is read at a glance in the dark and a word is not.
    #[must_use]
    pub const fn glyph(self) -> &'static str {
        match self {
            Self::Go => "▶",
            Self::At => "♪",
            Self::Follow => "↳",
            Self::Sound => "≈",
        }
    }

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Go => "taken by hand",
            Self::At => "taken by the clock",
            Self::Follow => "follows the cue before it",
            Self::Sound => "taken by a sound event",
        }
    }
}

/// The flags a cue carries, as the list shows them: a row of chips,
/// each present only when set.
///
/// Seven independent bools rather than a bitset: each one is a named
/// JSON field a show file already carries (`block`, `assert`, …), and a
/// bitset would trade that field-per-flag shape — readable in a diff,
/// stable to reorder — for a packed representation with no reader-side
/// benefit here.
#[expect(
    clippy::struct_excessive_bools,
    reason = "one bool per independent cue flag, each its own show-file field; see the doc comment"
)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Flags {
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub block: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub assert: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub cue_only: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub breaks: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub morph: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub fan: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub release: bool,
}

impl Flags {
    /// `(chip, what it means)` for every flag that is set.
    #[must_use]
    pub fn chips(self) -> Vec<(&'static str, &'static str)> {
        let mut out = Vec::new();
        if self.block {
            out.push(("B", "blocks: nothing tracks in"));
        }
        if self.assert {
            out.push(("A", "asserts: tracked values re-arrive on this cue's time"));
        }
        if self.cue_only {
            out.push(("Q", "cue only: does not track onward"));
        }
        if self.breaks {
            out.push(("K", "breaks tracking for some attributes"));
        }
        if self.morph {
            out.push(("M", "morphs from the cue before it"));
        }
        if self.fan {
            out.push(("F", "fanned: the cue wipes across its fixtures"));
        }
        if self.release {
            out.push(("R", "releases attributes to whatever is beneath"));
        }
        out
    }
}

/// What a cue resolved to on this rig — the one-glance verdict, plus
/// the counts the Program preset shows.
// r[impl cues.cooked-status]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CookStatus {
    pub state: CookState,
    /// Fixtures covered across every recipe.
    pub covers: usize,
    /// Recipes that resolved to something.
    pub recipes: usize,
    /// Direct values — the portability smell, per
    /// `r[cues.recipes-not-values]`.
    pub direct: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CookState {
    /// Every recipe resolved and nothing but recipes.
    Cooked,
    /// At least one recipe resolved to nothing.
    Failed,
    /// Recipes *and* hand-placed direct values.
    Mixed,
    /// Direct values only.
    Direct,
    /// Nothing in it at all — a dead cue.
    Empty,
}

impl CookState {
    #[must_use]
    pub const fn css(self) -> &'static str {
        match self {
            Self::Cooked => "ok",
            Self::Failed => "bad",
            Self::Mixed => "warn",
            Self::Direct => "direct",
            Self::Empty => "dead",
        }
    }

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Cooked => "every recipe resolves",
            Self::Failed => "a recipe resolves to nothing on this rig",
            Self::Mixed => "recipes and hand-placed values",
            Self::Direct => "hand-placed values only",
            Self::Empty => "dead: nothing in this cue resolves",
        }
    }
}

/// Which columns the list shows. The same rows either way — a view
/// setting, not a different panel.
// r[impl studio.cuelist.one-panel]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Preset {
    /// Running a night: what can be read at arm's length in the dark.
    // r[impl studio.cuelist.live-columns]
    #[default]
    Live,
    /// Building a show: everything a cue carries.
    // r[impl studio.cuelist.program-columns]
    Program,
}

/// Everything a Live client needs to draw before the first playhead
/// arrives.
///
/// The surface, the desk banks, whose favourites, and — for the profile
/// the library lists — the file profile the studio loaded, since the
/// browser has no disk to read it from. Also which URLs the server is
/// listening on, for the mode strip.
// r[impl studio.touch.ipad] - the browser is bootstrapped, not configured
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Bootstrap {
    pub surface: Surface,
    pub banks: Vec<desk::Bank>,
    pub operator: operators::Operator,
    pub profile: Option<ignition_core::Profile>,
    #[serde(default)]
    pub lan: Vec<String>,
}

/// What the server says to a client. One enum so a client can match on
/// it; the playhead is by far the common case.
///
/// `Playhead` is deliberately NOT boxed, which is what the size-difference
/// lint would ask for. `Hello` is sent once per connection and already
/// boxed; `Playhead` goes out on every frame the show moves, and boxing it
/// would put an allocation on that path to save width on the one message
/// that is never in a hurry. The enum is sized for the common case.
#[expect(
    clippy::large_enum_variant,
    reason = "Playhead is the hot path and stays unboxed; see the doc comment"
)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", content = "v", rename_all = "snake_case")]
pub enum ServerMessage {
    Hello(Box<Bootstrap>),
    Playhead(Playhead),
}

// ── Shared controls ──────────────────────────────────────────────────

/// The horizontal slider's track width in CSS pixels, matching
/// `live.css` / `studio.css`.
///
/// Blitz reports pointer positions relative to the element but not the
/// element's own width, so the maths needs the number from somewhere.
pub const HSLIDER_WIDTH: f32 = 90.0;

/// A small horizontal slider, for the trims. Divs, not a range input,
/// so it draws the same on Blitz and in Safari and so the grab area is
/// the whole track.
///
/// Latched on press, it follows the window's pointer
/// (`pointer::PointerRoot`) until the release, wherever that is, and
/// the value moves by how far the hand moved.
#[component]
#[expect(
    clippy::float_cmp,
    reason = "`v` is `level`'s own last value re-derived from the same \
              latch, so exact equality is the right test for \"nothing \
              moved\" — see the doc comment"
)]
pub fn HSlider(initial: f32, on_change: EventHandler<f32>) -> Element {
    let mut level = use_signal(|| initial);
    let mut latch = use_signal(|| Option::<pointer::Latch>::None);
    let feed = pointer::use_pointer_feed();
    use_effect(move || {
        // Reading the feed only while latched: an idle slider does not
        // re-render with the mouse.
        let Some(l) = latch() else {
            return;
        };
        let p = feed();
        if pointer::released(&l, &p) {
            latch.set(None);
            return;
        }
        let v = pointer::drag_right(&l, p.x, HSLIDER_WIDTH);
        if v != *level.peek() {
            level.set(v);
            on_change.call(v);
        }
    });
    rsx! {
        div {
            class: "hslider",
            onpointerdown: move |e| {
                let p = e.data.client_coordinates();
                latch.set(Some(pointer::Latch {
                    at: (pointer::coord(p.x), pointer::coord(p.y)),
                    level: level(),
                    ups: feed.peek().ups,
                }));
            },
            div { class: "hfill", style: "width: {level() * 100.0}%" }
        }
    }
}

#[cfg(test)]
mod swatch_tests {
    use super::ColorChip;

    /// One colour is a colour. Several is a palette, and the disc has
    /// to say so.
    #[test]
    fn a_palette_becomes_wedges_and_a_colour_stays_flat() {
        let solid = ColorChip::solid("Gold", "rgb(255 200 40)");
        assert_eq!(solid.disc(), "rgb(255 200 40)", "one colour needs no wheel");

        let split = ColorChip {
            name: "Trio".into(),
            css: "linear-gradient(90deg, red, green, blue)".into(),
            colors: vec!["red".into(), "green".into(), "blue".into()],
            spread: false,
            light: false,
        };
        // Three hard wedges, each owning its third of the wheel and
        // meeting the next with no blend — which is what cycle and
        // block actually do on the rig.
        assert_eq!(
            split.disc(),
            "conic-gradient(red 0.00deg 120.00deg, green 120.00deg 240.00deg, blue 240.00deg 360.00deg)"
        );
    }

    /// A spread blends across the rig, so the disc sweeps — and closes,
    /// or the last colour meets the first at a seam that reads as a
    /// wedge which is not there.
    #[test]
    fn a_spread_sweeps_and_closes_the_loop() {
        let chip = ColorChip {
            name: "Warm to cool".into(),
            css: String::new(),
            colors: vec!["red".into(), "blue".into()],
            spread: true,
            light: false,
        };
        assert_eq!(
            chip.disc(),
            "conic-gradient(red 0.00deg, blue 180.00deg, red 360deg)"
        );
    }

    /// A chip with nothing in it falls back to whatever flat fill it
    /// was given rather than drawing an empty wheel.
    #[test]
    fn an_empty_palette_falls_back_to_its_flat_fill() {
        let chip = ColorChip {
            name: "Unknown".into(),
            css: "#333".into(),
            colors: Vec::new(),
            spread: false,
            light: false,
        };
        assert_eq!(chip.disc(), "#333");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The wire is JSON of the desktop's own types; nothing is lost on
    /// the way to the iPad and back.
    // r[impl studio.touch.ipad] - the contract round-trips
    #[test]
    fn commands_round_trip_as_json() {
        let commands = vec![
            Command::Select(ignition_core::Selection::Group("Washers".into())),
            Command::Level(3, 0.5),
            Command::Key {
                index: 1,
                action: ignition_core::KeyAction::Flash,
                down: true,
            },
            Command::Page(PageMove::Set(2)),
            Command::Speed(SpeedKey::Half),
            Command::Flash(
                ignition_core::Selection::Group("All".into()),
                ignition_core::BumpKind::White,
            ),
            Command::Hold(None),
            Command::Locate(ignition_core::Bars::new(3, 1.0)),
            Command::PlaybackMaster(ignition_core::Class::Song, 0.25),
            Command::Param {
                index: 0,
                name: "depth".into(),
                value: 0.7,
            },
            Command::Look(Some("punt".into())),
            Command::StoreCue {
                index: 4,
                mode: ignition_core::cue::StoreMode::Track,
            },
            Command::StoreLook {
                name: "verse two".into(),
                kind: ignition_core::profile::LookKind::Bed,
            },
        ];
        for command in commands {
            let json = serde_json::to_string(&command).unwrap();
            let back: Command = serde_json::from_str(&json).unwrap();
            assert_eq!(format!("{back:?}"), format!("{command:?}"), "{json}");
        }
    }

    #[test]
    fn playhead_round_trips_as_json() {
        let mut playhead = Playhead {
            cue: Some(7),
            secs: 12.5,
            length: 200.0,
            playing: true,
            page: 1,
            pages: 4,
            grand: 0.8,
            held_look: Some("punt".into()),
            effects_playing: vec!["strobe".into()],
            protected: vec!["Drummer".into()],
            ..Default::default()
        };
        playhead.levels[2] = 0.4;
        playhead.latched[5] = true;
        let json = serde_json::to_string(&ServerMessage::Playhead(playhead.clone())).unwrap();
        let back: ServerMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(back, ServerMessage::Playhead(playhead));
        // An older server's playhead without a newer field still parses.
        let sparse: Playhead = serde_json::from_str(r#"{"cue":1,"secs":1.0}"#).unwrap();
        assert_eq!(sparse.cue, Some(1));
    }

    #[test]
    fn bootstrap_round_trips_as_json() {
        let boot = Bootstrap {
            surface: Surface {
                groups: vec!["All".into()],
                colors: vec![ColorChip::solid("Red", "rgb(255 0 0)")],
                cues: vec![Row::Hit {
                    index: 0,
                    name: "stab".into(),
                    at: ignition_core::Bars::new(2, 0.0),
                }],
                ..Default::default()
            },
            banks: vec![desk::Bank {
                name: "Warm".into(),
                scenes: vec![desk::Scene {
                    index: 3,
                    name: "Amber".into(),
                }],
            }],
            operator: operators::Operator::starter("cody"),
            profile: None,
            lan: vec!["http://10.0.0.2:8420".into()],
        };
        let json = serde_json::to_string(&ServerMessage::Hello(Box::new(boot.clone()))).unwrap();
        let back: ServerMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(back, ServerMessage::Hello(Box::new(boot)));

        // The profile too — compared by what the library lists rather
        // than bit for bit, because serde_json's f32 path can move a
        // focus delta by one ulp and nothing on a surface can tell.
        let profile = faders::profile().clone();
        let json = serde_json::to_string(&Some(profile.clone())).unwrap();
        let back: Option<ignition_core::Profile> = serde_json::from_str(&json).unwrap();
        let back = back.expect("a profile");
        assert_eq!(back.pages.len(), profile.pages.len());
        assert_eq!(
            back.looks.keys().collect::<Vec<_>>(),
            profile.looks.keys().collect::<Vec<_>>()
        );
        assert_eq!(
            back.macros.keys().collect::<Vec<_>>(),
            profile.macros.keys().collect::<Vec<_>>()
        );
        assert_eq!(back.roles, profile.roles);
    }
}
