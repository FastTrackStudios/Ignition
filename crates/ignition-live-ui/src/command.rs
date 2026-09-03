//! What the UI says to the visualizer, and what comes back.
//!
//! Dioxus components and the Bevy app live on the same thread but in
//! different ownership worlds — the app is inside a Blitz widget the
//! components cannot reach. A channel is the seam: components send, the
//! widget drains before it steps. That also means every control is a
//! *message*, which is what a hardware surface or a remote sends
//! without the UI having to change shape — and, now, what an iPad's
//! browser sends over a WebSocket. Both types are serde so the wire
//! carries exactly the desktop's contract (`r[studio.touch.ipad]`).

use ignition_core::{Attribute, Class, FADERS, Fader, KeyAction, Recipe, Selection};

/// `Deselect` and `Section` have no control on the surface yet.
///
/// Kept rather than deleted because this enum is a *wire contract*: the
/// widget handles both, and the surface that will send them — a clear
/// key, a rehearsal jump — is UI work rather than a change here.
/// Deleting them would mean re-deriving the handler when the control
/// arrives, which is how a protocol loses the pieces nobody happened to
/// need on the day.
// r[impl studio.touch.ipad] - the one Command type, on the desktop and on the wire
#[allow(dead_code)]
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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
    // ── Additive, for the Live / Program / Library panels (`live.rs`
    // and friends). Handled in `live_commands::apply`, which the widget's
    // drain calls for anything it does not match itself.
    /// Take a library effect or bundle by name on the macro layer at
    /// `level` — what tapping an effect tile in the library does.
    // r[impl studio.views.whole-profile] - every effect is reachable by name
    Take {
        name: String,
        level: f32,
    },
    /// Let go of an effect or bundle taken by `Take`.
    Untake(String),
    /// Fire a desk scene — a cue of the venue's console show — on the
    /// Show-class playback, under the looks and the song.
    // r[impl studio.live.desk-scenes] - a desk scene is a cue in a playback
    DeskScene(usize),
    /// Fold the desk playback out of the stack without forgetting it.
    DeskRelease,
    /// Protect, or stop protecting, a role — an operator decision made
    /// on the Live surface.
    // r[impl studio.views.seven-busking-features] - protection is toggled live
    Protect {
        role: String,
        on: bool,
    },
    /// Store the programmer's captured values into cue `index` of the
    /// show file on disk, in a store mode, and into the running player
    /// at the same time — see `live_commands`. Past the end appends.
    // r[impl studio.program.cue-editing] - store to a cue
    StoreCue {
        index: usize,
        mode: ignition_core::cue::StoreMode,
    },
    /// Store what the hand holds — a latched look plus every apply
    /// since CLEAR — as a look of this name and kind, in the profile's
    /// authored overlay beside the baked file. The looks bank shows it
    /// at once.
    // r[impl studio.program.cue-editing] - store to a look
    // r[impl profile.looks.authored]
    StoreLook {
        name: String,
        kind: ignition_core::profile::LookKind,
    },
    // ── Additive, for the 3D-aware programmer (`r[studio.program.pick-and-gizmos]`).
    /// One of the Program view's overlays on the visualizer, on or off.
    // r[impl studio.program.pick-and-gizmos] - FOCUS / BEAMS / GROUPS keys
    Overlay {
        kind: OverlayKind,
        on: bool,
    },
    /// The DMX address over every fixture in the visualizer.
    // r[impl studio.program.pick-and-gizmos] - the LABELS key
    Labels(bool),
    /// Outline this group's fixtures in the visualizer — what hovering
    /// a group tile in the Library sends; `None` when the pointer leaves.
    // r[impl studio.program.pick-and-gizmos] - a hovered group is outlined
    HighlightGroup(Option<String>),
    /// Whether the Program view is showing. The overlays draw only
    /// while it is; Live is the stage, not the rig.
    // r[impl studio.program.pick-and-gizmos] - Live has the overlays off
    ProgramView(bool),
    // ── Additive, for the Cameras pane (`cameras.rs`). Handled in the
    // widget beside the cue's own `camera …` commands.
    /// Cut the programme camera to a slot or a preset, dissolving over
    /// `beats` (zero is a cut) — what a number key and a pane tile send.
    // r[impl viz.camera-favourites] - the studio's number keys
    // r[impl viz.camera-cuts] - the same cut a cue would make
    Camera {
        target: CameraTarget,
        beats: f32,
    },
    /// Save the camera the viewport is on right now as a preset of this
    /// name in the venue's `cameras.json` — new, or replacing one.
    // r[impl studio.video.cameras-pane] - save current view as preset
    SaveCameraPreset {
        name: String,
    },
    /// Put a preset on key `slot` (`1`..`9`, `0`), for this operator and
    /// as the venue's default.
    // r[impl studio.video.cameras-pane] - set as slot N
    SetCameraSlot {
        slot: u8,
        name: String,
    },
    /// Remove a preset from the venue file, and from every slot.
    // r[impl studio.video.cameras-pane] - delete
    DeleteCameraPreset {
        name: String,
    },
    /// The preset the main (wide) view holds while a Programme pane
    /// takes the cuts.
    // r[impl viz.programme-view] - the wide view is selectable
    Wide {
        target: CameraTarget,
    },
    /// What a canvas shows: `camera:programme`, `camera:<preset>`, or
    /// an empty string for the canvas's own content — the TO SCREENS
    /// key on the Cameras pane.
    // r[impl canvas.camera-source] - switched live from the surface
    CanvasSource {
        canvas: String,
        source: String,
    },
    /// An edit to the room's patch, from the Setup view.
    ///
    /// One variant rather than eight, because they all take the same
    /// path — mutate the venue, re-resolve the patch, republish the
    /// sheet — and a wire contract with eight near-identical variants is
    /// eight things to keep in step.
    // r[impl patch.writes-the-venue] - the edit, on its way to the file
    Patch(PatchEdit),
}

/// What the Setup view can change about a room.
///
/// Every variant names a fixture by its **channel**, not by its index:
/// an index is a position in a file that inserting a fixture changes,
/// and a command that arrives one frame late would then edit the wrong
/// light.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PatchEdit {
    /// Move a fixture on the wire.
    Address {
        chan: u32,
        universe: u16,
        /// 1-based within the universe.
        address: u16,
    },
    /// Take a fixture off the wire, leaving it in the room
    /// (`r[patch.unpatched]`).
    Unpatch { chan: u32 },
    /// The operator's own word for a fixture.
    Label { chan: u32, label: String },
    /// The gel in front of it.
    Gel { chan: u32, gel: String },
    /// Add fixtures: `count` of one type, from `chan`, addressed from
    /// `universe`.`address`, each `offset` channels after the last
    /// (`r[patch.insert]`). An `offset` of zero means "the type's own
    /// footprint", which is what packing a bar wants.
    Insert {
        fixture_type: String,
        count: u16,
        chan: u32,
        universe: u16,
        address: u16,
        offset: u16,
    },
    /// Remove a fixture from the room entirely.
    Remove { chan: u32 },
    /// Write the venue back to disk (`r[patch.explicit-save]`).
    Save,
}

/// What a camera cut names: a number key, or a preset by name.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CameraTarget {
    Slot(u8),
    Preset(String),
}

/// Where the programme camera is, as the engine reports it: the preset
/// it is on (`None` mid-dissolve or after a free move), its pose, and
/// the presets and slots the pane lists.
///
/// The pose is what *save current view as preset* writes.
// r[impl studio.video.cameras-pane] - the current view comes back on the playhead
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct CameraState {
    pub preset: Option<String>,
    pub eye: [f32; 3],
    pub look: [f32; 3],
    pub fov_deg: f32,
    /// Every preset the venue has, in file order.
    pub presets: Vec<String>,
    /// The ten keys, `1`..`9` then `0`; `None` where a key is empty.
    pub slots: Vec<Option<String>>,
    /// The preset the wide (main) view is on while a programme camera
    /// takes the cuts; `None` when the main view *is* the programme.
    pub wide: Option<String>,
    /// Every canvas of the venue, and the camera it is switched to
    /// (`None` for its own content).
    pub canvases: Vec<CanvasRow>,
}

/// One canvas on the Cameras pane's TO SCREENS row.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CanvasRow {
    pub name: String,
    /// `camera:programme` / `camera:<preset>` while switched; `None`
    /// for the canvas's own content.
    pub camera: Option<String>,
}

/// The switchable overlays of the Program view's visualizer.
// r[impl studio.program.pick-and-gizmos]
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OverlayKind {
    /// The venue's focus points.
    Focus,
    /// The selected fixtures' beam axes.
    Beams,
    /// The outline of the group under the pointer.
    Groups,
}

impl OverlayKind {
    pub const ALL: [Self; 3] = [Self::Focus, Self::Beams, Self::Groups];

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Focus => "FOCUS",
            Self::Beams => "BEAMS",
            Self::Groups => "GROUPS",
        }
    }
}

/// The keys beside RATE. `Tap` is a learn tap — averaged, so a hand a
/// little early on one beat does not throw every phaser slaved to it.
// r[impl playback.speed-keys]
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpeedKey {
    Tap,
    Half,
    Double,
    Reset,
}

impl SpeedKey {
    /// The engine's key for this one.
    #[must_use]
    pub const fn action(self) -> KeyAction {
        match self {
            Self::Tap => KeyAction::Learn,
            Self::Half => KeyAction::HalfSpeed,
            Self::Double => KeyAction::DoubleSpeed,
            Self::Reset => KeyAction::ResetSpeed,
        }
    }
}

/// Where a page turn goes. `Set` has no on-screen control — a remote's
/// `{"page": n}` binding sends it — and is kept for the same wire-
/// contract reason `Deselect` is.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PageMove {
    Next,
    Prev,
    Set(usize),
}

/// A playhead from a client that predates fade progress reports its cue
/// as arrived rather than as never having landed.
const fn one_f32() -> f32 {
    1.0
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
// r[impl studio.touch.ipad] - and the one Playhead, back to every surface
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Playhead {
    /// Bumped whenever the patch changes.
    ///
    /// The sheet itself is far too big to put on a per-frame message —
    /// seventy rows sixty times a second to say nothing — so what
    /// travels is a counter, and the host re-reads the sheet when it
    /// moves. Zero means nothing has edited the patch this run.
    // r[impl patch.writes-the-venue] - the surface hears about an edit
    #[serde(default)]
    pub patch_revision: u64,
    /// Index of the cue the player is actually standing on.
    pub cue: Option<usize>,
    /// How far into its arrival that cue is, 0 to 1. The list draws it
    /// as a bar across the standing row: a cue list that only shows
    /// stored data is a document, and an operator needs an instrument.
    // r[impl studio.cuelist.live-state]
    #[serde(default = "one_f32")]
    pub cue_fade: f32,
    /// Which cue a GO would take next, and — when it takes itself —
    /// how many seconds until it does.
    // r[impl studio.cuelist.live-state]
    #[serde(default)]
    pub next_cue: Option<usize>,
    #[serde(default)]
    pub next_in: Option<f32>,
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
    pub output: ignition_proto::OutputSummary,
    // ── Additive, for the Live / Program panels. Filled by
    // `live_commands::publish`.
    /// The profile look latched on the held layer, by name.
    // r[impl studio.one-truth] - the held look comes back from the engine side
    pub held_look: Option<String>,
    /// Effects and bundles taken on the macro layer, in firing order.
    pub effects_playing: Vec<String>,
    /// The desk scene the Show-class playback is standing on, if that
    /// playback is enabled.
    pub desk_scene: Option<usize>,
    /// The roles the programmer protects right now.
    pub protected: Vec<String>,
    /// The programmer's selection, described for the surface.
    pub selection: Option<String>,
    /// The selection's channels, when it names them directly.
    ///
    /// The description above is for reading; this is for pointing. The
    /// patch sheet lights the row of whatever was clicked in the
    /// viewport, and matching on prose would be matching on a sentence
    /// somebody may reword (`r[patch.pick]`). Only populated for a
    /// selection that is literally a channel list — which is exactly
    /// what a viewport pick produces — because resolving a role against
    /// the rig every frame to light a row is not worth it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub selected_chans: Vec<u32>,
    /// How many direct values the programmer holds — what a store
    /// would write.
    pub captured: usize,
    // ── Additive, for the Cameras pane.
    /// The programme camera as the engine has it — see `CameraState`.
    /// `None` until a visualizer exists.
    // r[impl studio.video.cameras-pane] - the active camera comes from the engine
    pub camera: Option<CameraState>,
}

impl Playhead {
    /// The song playback's master, for the surface.
    #[must_use]
    pub const fn song_master(&self) -> f32 {
        self.playback_masters[0]
    }

    /// The look playback's master.
    #[must_use]
    pub const fn look_master(&self) -> f32 {
        self.playback_masters[1]
    }
}

impl Playhead {
    /// How far through, 0..=1. A zero-length song reports 0 rather than
    /// NaN, which would otherwise reach a CSS percentage and lay the bar
    /// out at a width no one can explain.
    #[must_use]
    pub fn fraction(&self) -> f32 {
        if self.length > 0.0 {
            (self.secs / self.length).clamp(0.0, 1.0)
        } else {
            0.0
        }
    }
}
