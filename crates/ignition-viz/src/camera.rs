//! Cameras: named presets, the ten favourite slots, setups, and cuts.
//!
//! A *preset* is a named eye/look-at/field-of-view for one venue, kept
//! in `data/venues/<venue>/cameras.json` beside the room it frames —
//! "Drum cam" is a place in *this* room, so it lives with the room and
//! not with a song. A venue with no file gets the three auto-framed
//! `ViewPreset`s as presets, so nothing here is required.
//!
//! Ten of them sit on the number keys, `1`–`9` then `0` for the tenth
//! (`r[viz.camera-favourites]`); a *setup* names N of them as the
//! cameras a cut list addresses (`r[viz.camera-setups]`); and a cue's
//! `commands` carry `camera <slot|preset> [in <beats>] [after <beats>]
//! [for <beats>]`, so the transport-synced cut is part of the song file
//! and there is no second timeline to keep in step
//! (`r[viz.camera-cuts]`).
//!
//! The Bevy half is one resource, [`ActiveCamera`], and one system that
//! writes the main camera's `Transform` and `Projection` from it every
//! frame. A dissolve is a linear tween on the song clock: it lands on
//! the beat the cue said, whatever the frame rate, and an export
//! (`r[viz.export]`) renders exactly the cut the studio showed.

use crate::playback::Playback;
use crate::view::ViewPreset;
use bevy::camera::{OrthographicProjection, RenderTarget, ScalingMode};
use bevy::post_process::dof::DepthOfField;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The file a venue keeps its cameras in.
pub const FILE: &str = "cameras.json";

/// The names the shipped show vocabulary cuts between. A song cuts to
/// `Drums`, and a venue's file says where the drums are — the same
/// split the profile makes for roles (`r[song.no-room]`). A venue may
/// name as many others as it likes.
pub const STANDARD: [&str; 10] = [
    "Wide",
    "Singer",
    "Drums",
    "Guitar",
    "Bass",
    "Keys",
    "Side stage",
    "Super wide",
    "Flat front",
    "Bird's eye",
];

/// One named camera position in a venue.
// r[impl viz.camera-presets] - eye, look-at, fov, optional focus, per venue
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CameraPreset {
    pub name: String,
    /// Where the camera is, in venue metres (Z up).
    pub eye: [f32; 3],
    /// What it looks at.
    pub look: [f32; 3],
    /// Vertical field of view in degrees. For an orthographic preset
    /// this is the *equivalent* view: the vertical extent is what a
    /// perspective camera of this angle would see at the look-at
    /// distance, which keeps one number meaning one framing.
    #[serde(default = "default_fov")]
    pub fov_deg: f32,
    /// Where depth of field focuses, in metres from the eye. Absent
    /// means on the look-at point.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focus: Option<f32>,
    /// A true plan: orthographic, and the ceiling hidden while it is up
    /// so the XY plot of everything reads (`r[viz.camera-birdseye]`).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub ortho: bool,
    /// What this camera is for, for the pane.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub about: String,
}

fn default_fov() -> f32 {
    50.0
}

impl CameraPreset {
    pub fn new(name: &str, eye: [f32; 3], look: [f32; 3], fov_deg: f32) -> Self {
        Self {
            name: name.to_string(),
            eye,
            look,
            fov_deg,
            focus: None,
            ortho: false,
            about: String::new(),
        }
    }

    /// The state this preset puts the camera in.
    pub fn state(&self) -> CameraState {
        CameraState {
            eye: Vec3::from(self.eye),
            look: Vec3::from(self.look),
            fov_deg: self.fov_deg,
            ortho: self.ortho,
            focus: self.focus,
        }
    }

    /// A preset from an auto-framed `ViewPreset` on a venue's bounds.
    pub fn from_view(view: ViewPreset, min: Vec3, max: Vec3) -> Self {
        let (eye, look) = view.eye_target(min, max);
        let name = match view {
            ViewPreset::House => "House",
            ViewPreset::Stage => "Stage",
            ViewPreset::Top => "Top",
        };
        Self {
            ortho: matches!(view, ViewPreset::Top),
            ..Self::new(name, eye.to_array(), look.to_array(), view.fov_y_deg())
        }
    }
}

/// N presets, in slot order, that a cut list addresses by number.
// r[impl viz.camera-setups] - a named list of presets, one per slot
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CameraSetup {
    pub name: String,
    pub slots: Vec<String>,
}

/// A venue's cameras: every preset, the venue's default favourites and
/// its setups.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Cameras {
    #[serde(default)]
    pub presets: Vec<CameraPreset>,
    /// The ten on the keys, in key order (`1`..`9`, `0`). The venue's
    /// defaults; an operator file's `cameras.favourites` replaces them.
    // r[impl viz.camera-favourites] - the venue's default ten
    #[serde(default)]
    pub favourites: Vec<String>,
    #[serde(default)]
    pub setups: Vec<CameraSetup>,
}

/// How many keys there are.
pub const SLOTS: usize = 10;

impl Cameras {
    pub fn path(dir: &Path) -> PathBuf {
        dir.join(FILE)
    }

    /// The venue's file, if it has one. A file that will not parse is an
    /// error; a missing file is `None`.
    // r[impl viz.camera-presets] - stored per venue, unlimited
    pub fn load(dir: &Path) -> anyhow::Result<Option<Self>> {
        let path = Self::path(dir);
        match std::fs::read_to_string(&path) {
            Ok(raw) => {
                let cameras: Self = serde_json::from_str(&raw)
                    .map_err(|e| anyhow::anyhow!("parsing {}: {e}", path.display()))?;
                Ok(Some(cameras))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(anyhow::anyhow!("reading {}: {e}", path.display())),
        }
    }

    /// The file, or the three auto-framed views when there is none.
    pub fn load_or_builtin(dir: &Path, min: Vec3, max: Vec3) -> Self {
        match Self::load(dir) {
            Ok(Some(cameras)) => cameras,
            Ok(None) => Self::builtin(min, max),
            Err(error) => {
                tracing::warn!(%error, "viz: cameras.json ignored");
                Self::builtin(min, max)
            }
        }
    }

    pub fn save(&self, dir: &Path) -> anyhow::Result<()> {
        let path = Self::path(dir);
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, json + "\n")
            .map_err(|e| anyhow::anyhow!("writing {}: {e}", path.display()))
    }

    /// House, Stage and Top from the venue's bounds — what a venue with
    /// no file gets, and the same three `--view` always offered.
    pub fn builtin(min: Vec3, max: Vec3) -> Self {
        let presets: Vec<CameraPreset> = [ViewPreset::House, ViewPreset::Stage, ViewPreset::Top]
            .into_iter()
            .map(|v| CameraPreset::from_view(v, min, max))
            .collect();
        let favourites = presets.iter().map(|p| p.name.clone()).collect();
        Self {
            presets,
            favourites,
            setups: vec![CameraSetup {
                name: "two".into(),
                slots: vec!["House".into(), "Stage".into()],
            }],
        }
    }

    /// A preset by name, case-insensitively.
    pub fn preset(&self, name: &str) -> Option<&CameraPreset> {
        let name = name.trim();
        self.presets
            .iter()
            .find(|p| p.name.eq_ignore_ascii_case(name))
    }

    pub fn setup(&self, name: &str) -> Option<&CameraSetup> {
        self.setups
            .iter()
            .find(|s| s.name.eq_ignore_ascii_case(name))
    }

    /// The preset on key `slot` (`1`..`9`, `0` is the tenth) — from the
    /// setup when one is named, else from the favourites.
    // r[impl viz.camera-favourites] - ten slots, 1..9 then 0
    pub fn slot(&self, slot: u8, setup: Option<&str>) -> Option<&str> {
        let index = slot_index(slot)?;
        let list = match setup.and_then(|s| self.setup(s)) {
            Some(setup) => &setup.slots,
            None => &self.favourites,
        };
        list.get(index).map(String::as_str)
    }

    /// Which key a preset is on, if any: `1`..`9`, `0` for the tenth.
    pub fn slot_of(&self, name: &str) -> Option<u8> {
        self.favourites
            .iter()
            .position(|n| n.eq_ignore_ascii_case(name))
            .and_then(slot_key)
    }

    /// Put `name` on key `slot`, growing the list as needed.
    pub fn set_slot(&mut self, slot: u8, name: &str) -> bool {
        let Some(index) = slot_index(slot) else {
            return false;
        };
        // One preset per key: a preset moved to a new key leaves its old one.
        for entry in self.favourites.iter_mut() {
            if entry.eq_ignore_ascii_case(name) {
                entry.clear();
            }
        }
        while self.favourites.len() <= index {
            self.favourites.push(String::new());
        }
        self.favourites[index] = name.to_string();
        while self.favourites.last().is_some_and(String::is_empty) {
            self.favourites.pop();
        }
        true
    }

    /// Add or replace a preset by name.
    pub fn store(&mut self, preset: CameraPreset) {
        match self
            .presets
            .iter_mut()
            .find(|p| p.name.eq_ignore_ascii_case(&preset.name))
        {
            Some(existing) => *existing = preset,
            None => self.presets.push(preset),
        }
    }

    /// Remove a preset, and every slot and setup entry naming it.
    pub fn remove(&mut self, name: &str) -> bool {
        let before = self.presets.len();
        self.presets.retain(|p| !p.name.eq_ignore_ascii_case(name));
        for entry in self.favourites.iter_mut() {
            if entry.eq_ignore_ascii_case(name) {
                entry.clear();
            }
        }
        while self.favourites.last().is_some_and(String::is_empty) {
            self.favourites.pop();
        }
        for setup in &mut self.setups {
            for entry in setup.slots.iter_mut() {
                if entry.eq_ignore_ascii_case(name) {
                    entry.clear();
                }
            }
        }
        self.presets.len() != before
    }

    /// The preset a target names.
    pub fn resolve(&self, target: &CameraTarget, setup: Option<&str>) -> Option<&CameraPreset> {
        match target {
            CameraTarget::Preset(name) => self.preset(name),
            CameraTarget::Slot(slot) => self.slot(*slot, setup).and_then(|n| self.preset(n)),
        }
    }

    /// Every favourite that names a preset the file does not have.
    pub fn dangling(&self) -> Vec<String> {
        let mut out: Vec<String> = self
            .favourites
            .iter()
            .filter(|n| !n.is_empty() && self.preset(n).is_none())
            .cloned()
            .collect();
        for setup in &self.setups {
            for n in &setup.slots {
                if !n.is_empty() && self.preset(n).is_none() && !out.contains(n) {
                    out.push(n.clone());
                }
            }
        }
        out
    }
}

/// `1`..`9` -> 0..8, `0` -> 9.
pub fn slot_index(slot: u8) -> Option<usize> {
    match slot {
        1..=9 => Some(slot as usize - 1),
        0 => Some(9),
        _ => None,
    }
}

/// 0..8 -> `1`..`9`, 9 -> `0`.
pub fn slot_key(index: usize) -> Option<u8> {
    match index {
        0..=8 => Some(index as u8 + 1),
        9 => Some(0),
        _ => None,
    }
}

/// What a cut names: a key, or a preset by name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CameraTarget {
    Slot(u8),
    Preset(String),
}

impl CameraTarget {
    /// A single digit is a key; anything else is a name.
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        if text.is_empty() {
            return None;
        }
        match text.parse::<u8>() {
            Ok(n) if n <= 9 => Some(Self::Slot(n)),
            _ => Some(Self::Preset(text.to_string())),
        }
    }
}

impl std::fmt::Display for CameraTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Slot(n) => write!(f, "{n}"),
            Self::Preset(name) => f.write_str(name),
        }
    }
}

/// A cue's `camera …` command, parsed.
///
/// `camera <slot|preset> [in <beats>] [after <beats>] [for <beats>]`:
/// `in` dissolves over that many beats (a cut when absent), `after`
/// delays the cut from the cue's own moment, and `for` is a punch-in
/// that returns to the camera it left after that many beats — a drum
/// fill seen and then the wide shot back.
// r[impl viz.camera-cuts] - the command grammar
#[derive(Debug, Clone, PartialEq)]
pub struct CameraCommand {
    pub target: CameraTarget,
    pub dissolve_beats: f32,
    pub after_beats: f32,
    pub hold_beats: Option<f32>,
}

impl CameraCommand {
    /// `None` for a line that is not a camera command.
    pub fn parse(line: &str) -> Option<Self> {
        let rest = line.trim().strip_prefix("camera")?;
        if !rest.is_empty() && !rest.starts_with(char::is_whitespace) {
            return None;
        }
        let words: Vec<&str> = rest.split_whitespace().collect();
        // The target runs up to the first keyword that is followed by a
        // number, so a preset called "Side stage" survives.
        let is_kw = |i: usize| matches!(words[i], "in" | "after" | "for");
        let end = (0..words.len()).find(|&i| is_kw(i)).unwrap_or(words.len());
        let target = CameraTarget::parse(&words[..end].join(" "))?;
        let mut out = Self {
            target,
            dissolve_beats: 0.0,
            after_beats: 0.0,
            hold_beats: None,
        };
        let mut i = end;
        while i + 1 < words.len() {
            let value: f32 = words[i + 1].parse().ok()?;
            match words[i] {
                "in" => out.dissolve_beats = value.max(0.0),
                "after" => out.after_beats = value.max(0.0),
                "for" => out.hold_beats = Some(value.max(0.0)),
                _ => return None,
            }
            i += 2;
        }
        if i != words.len() {
            return None;
        }
        Some(out)
    }
}

/// Where the camera is at one moment. What a preset resolves to, what
/// a dissolve interpolates, and what the studio reports back so the
/// pane can save the view an operator has dragged to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraState {
    pub eye: Vec3,
    pub look: Vec3,
    pub fov_deg: f32,
    pub ortho: bool,
    pub focus: Option<f32>,
}

impl CameraState {
    pub fn new(eye: Vec3, look: Vec3, fov_deg: f32) -> Self {
        Self {
            eye,
            look,
            fov_deg,
            ortho: false,
            focus: None,
        }
    }

    /// Linear between two states. The projection kind cannot blend, so
    /// it switches at the midpoint.
    // r[impl viz.camera-cuts] - a dissolve is linear on the song clock
    pub fn lerp(a: &Self, b: &Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        Self {
            eye: a.eye.lerp(b.eye, t),
            look: a.look.lerp(b.look, t),
            fov_deg: a.fov_deg + (b.fov_deg - a.fov_deg) * t,
            ortho: if t < 0.5 { a.ortho } else { b.ortho },
            focus: match (a.focus, b.focus) {
                (Some(x), Some(y)) => Some(x + (y - x) * t),
                _ => {
                    if t < 0.5 {
                        a.focus
                    } else {
                        b.focus
                    }
                }
            },
        }
    }

    /// The transform: Z up, unless the camera looks straight along Z,
    /// where Y is up so the look-at is not degenerate (a plan reads
    /// with the stage at the top).
    pub fn transform(&self) -> Transform {
        let dir = (self.look - self.eye).normalize_or_zero();
        let up = if dir.z.abs() > 0.995 {
            Vec3::Y
        } else {
            Vec3::Z
        };
        Transform::from_translation(self.eye).looking_at(self.look, up)
    }

    /// The projection: perspective at the field of view, or an
    /// orthographic one whose vertical extent matches that field of
    /// view at the look-at distance.
    // r[impl viz.camera-birdseye] - a true orthographic plan
    pub fn projection(&self, far: f32) -> Projection {
        if self.ortho {
            let distance = self.eye.distance(self.look).max(0.5);
            let height = 2.0 * distance * (self.fov_deg.to_radians() * 0.5).tan();
            Projection::Orthographic(OrthographicProjection {
                near: 0.05,
                far,
                scaling_mode: ScalingMode::FixedVertical {
                    viewport_height: height.max(0.5),
                },
                ..OrthographicProjection::default_3d()
            })
        } else {
            Projection::Perspective(PerspectiveProjection {
                fov: self.fov_deg.clamp(1.0, 170.0).to_radians(),
                near: 0.05,
                far,
                ..default()
            })
        }
    }

    /// Where depth of field focuses.
    pub fn focus_distance(&self) -> f32 {
        self.focus
            .unwrap_or_else(|| self.eye.distance(self.look))
            .max(0.5)
    }
}

/// The main camera — the one presets move. The haze camera and the
/// overlay camera are not it.
#[derive(Component)]
pub struct MainCamera;

/// A cut waiting for its moment.
#[derive(Debug, Clone, PartialEq)]
struct Pending {
    at: f32,
    target: CameraTarget,
    dissolve_beats: f32,
    hold_beats: Option<f32>,
}

/// The programme camera: which preset it is on, where it is between
/// two, and what is queued.
// r[impl viz.camera-cuts] - one programme camera, cut by cue or key
#[derive(Resource, Debug, Clone)]
pub struct ActiveCamera {
    pub cameras: Cameras,
    /// The setup slots resolve through, if one is chosen; else the
    /// favourites.
    pub setup: Option<String>,
    /// The preset the camera is on or dissolving to.
    pub preset: Option<String>,
    from: CameraState,
    to: CameraState,
    /// Clock seconds the current dissolve started, and how long it is.
    start: f32,
    duration: f32,
    pending: Vec<Pending>,
    /// Where a punch-in goes back to, and when.
    punch_return: Option<(CameraTarget, f32)>,
    /// Whether the number keys cut — on in a window, off when a host
    /// owns the keyboard.
    pub keys: bool,
    /// Whether the host drains the cue commands itself (the studio
    /// does, beside its macro handling); when it does, the engine's own
    /// drain stays out of the way so nothing is taken twice.
    pub host_drains_cues: bool,
    far: f32,
    /// The camera needs writing this frame.
    dirty: bool,
    ceiling_hidden: bool,
    /// The preset the main camera holds while a separate programme
    /// camera takes the cuts — the wide view. `None` means the first
    /// favourite.
    pub wide: Option<String>,
}

impl ActiveCamera {
    /// Starting on `initial` (a preset name, else the given state).
    pub fn new(cameras: Cameras, initial: Option<&str>, fallback: CameraState, far: f32) -> Self {
        let (preset, state) = match initial.and_then(|n| cameras.preset(n)) {
            Some(p) => (Some(p.name.clone()), p.state()),
            None => {
                if let Some(name) = initial {
                    tracing::warn!(name, "viz: no such camera preset; using the view");
                }
                (None, fallback)
            }
        };
        Self {
            cameras,
            setup: None,
            preset,
            from: state,
            to: state,
            start: 0.0,
            duration: 0.0,
            pending: Vec::new(),
            punch_return: None,
            keys: true,
            host_drains_cues: false,
            far,
            dirty: true,
            ceiling_hidden: false,
            wide: None,
        }
    }

    /// Put the main camera on `target` while a programme camera takes
    /// the cuts. `false` when the target names nothing.
    // r[impl viz.programme-view] - the wide view is its own, selectable preset
    pub fn set_wide(&mut self, target: &CameraTarget) -> bool {
        let Some(preset) = self.cameras.resolve(target, self.setup.as_deref()) else {
            return false;
        };
        self.wide = Some(preset.name.clone());
        self.dirty = true;
        true
    }

    /// The wide preset's name: what was chosen, else the first favourite,
    /// else the first preset.
    pub fn wide_name(&self) -> Option<String> {
        self.wide
            .clone()
            .or_else(|| {
                self.cameras
                    .favourites
                    .iter()
                    .find(|n| !n.is_empty())
                    .cloned()
            })
            .or_else(|| self.cameras.presets.first().map(|p| p.name.clone()))
    }

    /// Where the camera is at `now`.
    pub fn state_at(&self, now: f32) -> CameraState {
        if self.duration <= 0.0 {
            return self.to;
        }
        let t = (now - self.start) / self.duration;
        if t >= 1.0 {
            self.to
        } else {
            CameraState::lerp(&self.from, &self.to, t.max(0.0))
        }
    }

    pub fn is_dissolving(&self, now: f32) -> bool {
        self.duration > 0.0 && now < self.start + self.duration
    }

    /// Cut (or dissolve over `beats`) to `target` now. `false` when the
    /// target names nothing.
    // r[impl viz.camera-cuts] - instant at zero beats
    pub fn cut_to(&mut self, target: &CameraTarget, beats: f32, now: f32, bpm: f32) -> bool {
        let Some(preset) = self.cameras.resolve(target, self.setup.as_deref()) else {
            tracing::warn!(%target, "viz: camera cut names no preset");
            return false;
        };
        let name = preset.name.clone();
        let state = preset.state();
        self.from = self.state_at(now);
        self.to = state;
        self.preset = Some(name);
        self.start = now;
        self.duration = if beats > 0.0 {
            beats * 60.0 / bpm.max(1.0)
        } else {
            0.0
        };
        self.dirty = true;
        true
    }

    /// Put the camera somewhere by hand — a drag in a window, a host
    /// nudge. Drops the preset: it is no longer on one.
    pub fn set_free(&mut self, state: CameraState) {
        self.from = state;
        self.to = state;
        self.duration = 0.0;
        self.preset = None;
        self.dirty = true;
    }

    /// A cue's command, from its own moment `now`: queued for `after`,
    /// and the return queued for `for`.
    pub fn schedule(&mut self, command: CameraCommand, now: f32, bpm: f32) {
        let secs = |beats: f32| beats * 60.0 / bpm.max(1.0);
        let pending = Pending {
            at: now + secs(command.after_beats),
            target: command.target,
            dissolve_beats: command.dissolve_beats,
            hold_beats: command.hold_beats,
        };
        if pending.at <= now {
            self.fire(pending, now, bpm);
        } else {
            self.pending.push(pending);
        }
    }

    fn fire(&mut self, pending: Pending, now: f32, bpm: f32) {
        // A punch-in remembers where it left from — the preset, so the
        // return lands on it even if the camera was mid-dissolve.
        let leaving = self.preset.clone().map(CameraTarget::Preset);
        if self.cut_to(&pending.target, pending.dissolve_beats, now, bpm) {
            match (pending.hold_beats, leaving) {
                (Some(beats), Some(back)) => {
                    self.punch_return = Some((back, now + beats * 60.0 / bpm.max(1.0)));
                }
                // A cut that is not a punch-in cancels any return still
                // pending: the show moved on.
                _ => self.punch_return = None,
            }
        }
    }

    /// Fire everything whose moment has come. Called once a frame.
    pub fn advance(&mut self, now: f32, bpm: f32) {
        // A clock that jumped backwards (a locate) drops what was queued
        // for a future that is no longer coming.
        if self.pending.iter().any(|p| p.at > now + 600.0) {
            self.pending.clear();
        }
        let mut due: Vec<Pending> = Vec::new();
        self.pending.retain(|p| {
            if p.at <= now {
                due.push(p.clone());
                false
            } else {
                true
            }
        });
        due.sort_by(|a, b| a.at.total_cmp(&b.at));
        for p in due {
            self.fire(p, now, bpm);
        }
        if let Some((back, at)) = self.punch_return.clone()
            && now >= at
        {
            self.punch_return = None;
            self.cut_to(&back, 0.0, now, bpm);
        }
    }

    /// Everything queued, dropped — a locate backwards.
    pub fn clear_queue(&mut self) {
        self.pending.clear();
        self.punch_return = None;
    }

    /// The presets on the keys, for a surface: ten entries, `None`
    /// where a key is empty.
    pub fn slots(&self) -> Vec<Option<String>> {
        (0..SLOTS)
            .map(|i| {
                slot_key(i)
                    .and_then(|k| self.cameras.slot(k, self.setup.as_deref()))
                    .filter(|n| !n.is_empty())
                    .map(str::to_string)
            })
            .collect()
    }
}

/// The starting camera from the CLI's config: the named preset, else
/// `--eye/--look`, else the view's auto-framing.
pub(crate) fn active_from_config(config: &crate::app::VizConfig) -> ActiveCamera {
    let (min, max) = config.venue.bounds();
    let far = config.view.far(min, max);
    let fallback = match config.camera {
        Some((eye, look)) => CameraState::new(eye, look, config.view.fov_y_deg()),
        None => {
            let (eye, look) = config.view.eye_target(min, max);
            CameraState {
                ortho: matches!(config.view, ViewPreset::Top),
                ..CameraState::new(eye, look, config.view.fov_y_deg())
            }
        }
    };
    let mut active = ActiveCamera::new(
        config.cameras.clone(),
        config.camera_preset.as_deref(),
        fallback,
        far,
    );
    // Nothing named and nothing free: the camera is not on a preset,
    // and the spawn already put it where the view says. Only a preset
    // start needs the first frame to move it.
    if config.camera_preset.is_none() {
        active.dirty = false;
    }
    active.wide = active.wide_name();
    active
}

/// The song's clock and tempo, or the app's when there is no song.
fn clock(time: &Time, playback: Option<&Playback>) -> (f32, f32) {
    match playback {
        Some(p) => {
            let bpm = p.speeds.get("Song").copied().unwrap_or(120.0);
            match p.song() {
                Some(player) => (player.clock(), bpm),
                None => (time.elapsed_secs(), bpm),
            }
        }
        None => (time.elapsed_secs(), 120.0),
    }
}

/// The number keys, in a window: `1`..`9` and `0` cut to the slots.
// r[impl viz.camera-favourites] - 1..0 on the keyboard
pub fn camera_keys(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    playback: Option<Res<Playback>>,
    mut active: ResMut<ActiveCamera>,
) {
    if !active.keys {
        return;
    }
    const DIGITS: [(KeyCode, u8); 10] = [
        (KeyCode::Digit1, 1),
        (KeyCode::Digit2, 2),
        (KeyCode::Digit3, 3),
        (KeyCode::Digit4, 4),
        (KeyCode::Digit5, 5),
        (KeyCode::Digit6, 6),
        (KeyCode::Digit7, 7),
        (KeyCode::Digit8, 8),
        (KeyCode::Digit9, 9),
        (KeyCode::Digit0, 0),
    ];
    let Some((_, slot)) = DIGITS.iter().find(|(k, _)| keys.just_pressed(*k)) else {
        return;
    };
    let (now, bpm) = clock(&time, playback.as_deref());
    // Shift dissolves over a beat; plain is a cut.
    let beats = if keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight) {
        1.0
    } else {
        0.0
    };
    active.clear_queue();
    if active.cut_to(&CameraTarget::Slot(*slot), beats, now, bpm) {
        info!("camera -> {}", active.preset.as_deref().unwrap_or("?"));
    }
}

/// A cue's `camera …` commands, taken as the cue goes live. Only when
/// no host is draining the commands itself.
// r[impl viz.camera-cuts] - the cue command cuts the programme camera
pub fn cue_camera_commands(
    time: Res<Time>,
    mut playback: Option<ResMut<Playback>>,
    mut active: ResMut<ActiveCamera>,
) {
    if active.host_drains_cues {
        return;
    }
    let Some(playback) = playback.as_deref_mut() else {
        return;
    };
    let (now, bpm) = clock(&time, Some(playback));
    let Some(player) = playback.song_mut() else {
        return;
    };
    for line in player.drain_commands() {
        match CameraCommand::parse(&line) {
            Some(command) => active.schedule(command, now, bpm),
            None => info!("cue command: {line}"),
        }
    }
}

/// Apply one camera command line from a host, at the host's clock.
/// Returns whether it was a camera command.
pub fn apply_command_line(active: &mut ActiveCamera, line: &str, now: f32, bpm: f32) -> bool {
    match CameraCommand::parse(line) {
        Some(command) => {
            active.schedule(command, now, bpm);
            true
        }
        None => false,
    }
}

/// What a camera is written from: its pose, projection and focus.
type CameraParts = (
    &'static mut Transform,
    &'static mut Projection,
    Option<&'static mut DepthOfField>,
);
/// Room entities: neither the main nor the programme camera.
type NotACamera = (Without<MainCamera>, Without<ProgrammeCamera>);

/// Writes the programme camera into the main camera every frame it
/// moves, and hides the ceiling while a plan is up.
// r[impl viz.camera-birdseye] - ceiling entities hidden while the plan is active
pub fn drive_camera(
    time: Res<Time>,
    playback: Option<Res<Playback>>,
    programme: Option<Res<ProgrammeView>>,
    mut active: ResMut<ActiveCamera>,
    mut mains: Query<CameraParts, (With<MainCamera>, Without<ProgrammeCamera>)>,
    mut programmes: Query<CameraParts, (With<ProgrammeCamera>, Without<MainCamera>)>,
    mut room: Query<(&Name, &mut Visibility), NotACamera>,
) {
    let (now, bpm) = clock(&time, playback.as_deref());
    active.advance(now, bpm);
    let moving = active.is_dissolving(now);
    if !(active.dirty || moving) {
        return;
    }
    let state = active.state_at(now);
    let far = active.far;
    let separate = programme_is_separate(programme.as_deref());
    // The cut lands on the programme camera when there is one, else on
    // the main; the main then sits on the wide preset.
    // r[impl viz.programme-view] - the keys and cues move the programme camera; the wide view stays put
    let wide = separate
        .then(|| {
            active
                .wide
                .as_deref()
                .and_then(|n| active.cameras.preset(n))
                .map(|p| p.state())
        })
        .flatten();
    let write = |state: &CameraState,
                 transform: &mut Transform,
                 projection: &mut Projection,
                 dof: Option<Mut<DepthOfField>>,
                 snap: bool| {
        *transform = state.transform();
        let is_ortho = matches!(*projection, Projection::Orthographic(_));
        if is_ortho != state.ortho || snap {
            *projection = state.projection(far);
        } else if let Projection::Perspective(p) = &mut *projection {
            p.fov = state.fov_deg.clamp(1.0, 170.0).to_radians();
        }
        if let Some(mut dof) = dof {
            dof.focal_distance = state.focus_distance();
        }
    };
    let snap = !moving || active.dirty;
    for (mut transform, mut projection, dof) in &mut programmes {
        write(&state, &mut transform, &mut projection, dof, snap);
    }
    for (mut transform, mut projection, dof) in &mut mains {
        match &wide {
            Some(wide) => write(wide, &mut transform, &mut projection, dof, true),
            None => write(&state, &mut transform, &mut projection, dof, snap),
        }
    }
    // The plan hides the ceiling: for the whole world, since the plan is
    // the one reading it — the wide view loses its roof for the moment
    // the programme is on the bird's eye, which is the cheaper of the
    // two honest answers.
    if state.ortho != active.ceiling_hidden {
        for (name, mut visibility) in &mut room {
            if name.as_str().contains("Ceiling") {
                *visibility = if state.ortho {
                    Visibility::Hidden
                } else {
                    Visibility::Inherited
                };
            }
        }
        active.ceiling_hidden = state.ortho;
    }
    active.dirty = moving;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cameras() -> Cameras {
        Cameras {
            presets: vec![
                CameraPreset::new("Wide", [0.0, -10.0, 2.0], [0.0, 0.0, 1.0], 60.0),
                CameraPreset::new("Singer", [0.0, -6.0, 1.6], [0.0, -3.0, 1.4], 30.0),
                CameraPreset {
                    ortho: true,
                    ..CameraPreset::new("Bird's eye", [0.0, 0.0, 12.0], [0.0, 0.0, 0.0], 45.0)
                },
                CameraPreset::new("Drums", [0.0, -4.0, 1.5], [0.0, 0.5, 1.0], 35.0),
            ],
            favourites: vec!["Wide".into(), "Singer".into(), "Drums".into()],
            setups: vec![CameraSetup {
                name: "two".into(),
                slots: vec!["Wide".into(), "Singer".into()],
            }],
        }
    }

    /// r[verify viz.camera-presets] - the file round-trips, and a missing one is the built-in three
    #[test]
    fn presets_round_trip_through_the_file_and_a_missing_file_is_builtin() {
        let dir = std::env::temp_dir().join(format!("ig-cameras-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(Cameras::load(&dir).unwrap().is_none());
        let builtin =
            Cameras::load_or_builtin(&dir, Vec3::new(-5.0, -10.0, 0.0), Vec3::new(5.0, 5.0, 4.0));
        assert_eq!(builtin.presets.len(), 3);
        assert!(builtin.preset("top").unwrap().ortho);

        let mine = cameras();
        mine.save(&dir).unwrap();
        let back = Cameras::load(&dir).unwrap().unwrap();
        assert_eq!(back, mine);
        assert!(back.dangling().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A cut list addresses a setup's slots, and the shipped venue
    /// ships the three the spec names.
    ///
    /// A show authored for eight cameras cuts to slot 8. Whether that
    /// means anything is the venue's answer, not the show's, and it is
    /// the file that has to be right: a setup with a hole in it, or one
    /// naming a preset that was renamed, resolves to nothing and the
    /// cut silently does not happen.
    ///
    /// r[verify viz.camera-setups]
    #[test]
    fn a_cut_list_addresses_a_setups_slots_and_the_shipped_setups_are_whole() {
        let mut c = cameras();
        c.setups.push(CameraSetup {
            name: "four".into(),
            slots: vec![
                "Wide".into(),
                "Singer".into(),
                "Drums".into(),
                "Bird's eye".into(),
            ],
        });

        // The same slot number is a different camera under each setup,
        // and different again with no setup at all.
        assert_eq!(c.slot(3, Some("four")), Some("Drums"));
        assert_eq!(c.slot(3, Some("two")), None, "a two-camera setup has no slot 3");
        assert_eq!(c.slot(3, None), Some("Drums"), "no setup: the favourites");
        assert_eq!(c.slot(2, Some("two")), Some("Singer"));

        // And a cut resolves through the setup that is active.
        let target = CameraTarget::Slot(2);
        assert_eq!(
            c.resolve(&target, Some("four")).map(|p| p.name.as_str()),
            Some("Singer")
        );

        // The shipped room ships `two`, `four` and `eight`, each whole.
        let dir = std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../data/venues/norco"
        ));
        let Ok(Some(shipped)) = Cameras::load(dir) else {
            return; // repo data absent — a runner outside the checkout
        };
        for (name, count) in [("two", 2), ("four", 4), ("eight", 8)] {
            let setup = shipped
                .setups
                .iter()
                .find(|s| s.name == name)
                .unwrap_or_else(|| panic!("norco ships no {name:?} setup"));
            assert_eq!(setup.slots.len(), count, "{name} is not {count} cameras");
            for (i, slot) in setup.slots.iter().enumerate() {
                assert!(
                    shipped.preset(slot).is_some(),
                    "{name} slot {} names {slot:?}, which this room has no preset for",
                    i + 1
                );
            }
        }
        assert!(
            shipped.dangling().is_empty(),
            "norco's cameras name presets it does not have: {:?}",
            shipped.dangling()
        );
    }

    /// The main view keeps its own wide preset while the cuts go
    /// somewhere else, and nothing pays for a programme camera nobody
    /// is looking at.
    ///
    /// The pair is the rule: an operator docks the whole rig beside the
    /// cut, so the cut must not drag the wide view along with it — and
    /// a lone viewport, which is most of the time, must not be
    /// rendering a second camera into a texture no one reads.
    ///
    /// r[verify viz.programme-view]
    #[test]
    fn the_wide_view_holds_while_the_cuts_move_and_an_unwatched_programme_costs_nothing() {
        let mut active = ActiveCamera::new(
            cameras(),
            Some("Wide"),
            CameraState::new(Vec3::ZERO, Vec3::Y, 60.0),
            100.0,
        );

        // The wide view is selectable and its own choice.
        assert_eq!(active.wide_name().as_deref(), Some("Wide"), "the first favourite");
        assert!(active.set_wide(&CameraTarget::Preset("Bird's eye".into())));
        assert_eq!(active.wide_name().as_deref(), Some("Bird's eye"));

        // A cut moves what the programme camera shows...
        assert!(active.cut_to(&CameraTarget::Slot(2), 0.0, 0.0, 120.0));
        assert_eq!(active.preset.as_deref(), Some("Singer"));
        let singer = active.cameras.preset("Singer").expect("Singer").state();
        assert_eq!(active.state_at(0.0).eye, singer.eye);

        // ...and leaves the wide view where the operator put it, which
        // is what makes the two panes worth having.
        assert_eq!(active.wide_name().as_deref(), Some("Bird's eye"));

        // The second camera exists only while something shows it.
        let mut view = ProgrammeView {
            target: Handle::default(),
            size: SOURCE_SIZE,
            host_wants: false,
            canvas_wants: false,
            camera: None,
        };
        assert!(!view.wanted(), "a lone viewport pays for no second camera");
        view.host_wants = true;
        assert!(view.wanted());
        view.host_wants = false;
        view.canvas_wants = true;
        assert!(view.wanted(), "a canvas sampling it is reason enough");
        view.canvas_wants = false;
        assert!(!view.wanted());
    }

    /// r[verify viz.camera-favourites] - keys 1..9 then 0, and a setup overrides the favourites
    #[test]
    fn slots_map_the_keys_and_setups_override_favourites() {
        let mut c = cameras();
        assert_eq!(c.slot(1, None), Some("Wide"));
        assert_eq!(c.slot(3, None), Some("Drums"));
        assert_eq!(c.slot(0, None), None);
        assert_eq!(c.slot(3, Some("two")), None);
        assert_eq!(c.slot_of("drums"), Some(3));
        assert!(c.set_slot(0, "Bird's eye"));
        assert_eq!(c.slot(0, None), Some("Bird's eye"));
        assert_eq!(c.favourites.len(), 10);
        // Moving a preset to another key vacates its old one.
        c.set_slot(2, "Drums");
        assert_eq!(c.slot(2, None), Some("Drums"));
        assert_eq!(c.slot(3, None), Some(""));
        assert!(!c.set_slot(11, "Wide"));
        assert!(c.remove("Drums"));
        assert!(c.preset("Drums").is_none());
        assert_eq!(c.slot(2, None), Some(""));
    }

    /// r[verify viz.camera-cuts] - `camera 3`, `camera Drums in 2`, and the rest of the grammar
    #[test]
    fn cue_commands_parse() {
        let c = CameraCommand::parse("camera 3").unwrap();
        assert_eq!(c.target, CameraTarget::Slot(3));
        assert_eq!(c.dissolve_beats, 0.0);
        let c = CameraCommand::parse("camera Drums in 2").unwrap();
        assert_eq!(c.target, CameraTarget::Preset("Drums".into()));
        assert_eq!(c.dissolve_beats, 2.0);
        let c = CameraCommand::parse("camera Side stage after 4 for 1.5").unwrap();
        assert_eq!(c.target, CameraTarget::Preset("Side stage".into()));
        assert_eq!(c.after_beats, 4.0);
        assert_eq!(c.hold_beats, Some(1.5));
        assert_eq!(
            CameraCommand::parse("camera 0").unwrap().target,
            CameraTarget::Slot(0)
        );
        assert!(CameraCommand::parse("macro drop").is_none());
        assert!(CameraCommand::parse("cameras 1").is_none());
        assert!(CameraCommand::parse("camera").is_none());
        assert!(CameraCommand::parse("camera Wide in x").is_none());
    }

    /// r[verify viz.camera-cuts] - a dissolve is linear on the clock and instant at zero
    #[test]
    fn a_dissolve_tweens_linearly_on_the_clock_and_a_cut_is_instant() {
        let wide = cameras().preset("Wide").unwrap().state();
        let mut active = ActiveCamera::new(cameras(), Some("Wide"), wide, 100.0);
        // Two beats at 120 BPM is one second.
        assert!(active.cut_to(&CameraTarget::Preset("Singer".into()), 2.0, 10.0, 120.0));
        assert_eq!(active.preset.as_deref(), Some("Singer"));
        let half = active.state_at(10.5);
        assert!((half.eye.y - (-8.0)).abs() < 1e-4, "{half:?}");
        assert!((half.fov_deg - 45.0).abs() < 1e-4);
        assert!(active.is_dissolving(10.9));
        assert!(!active.is_dissolving(11.0));
        assert_eq!(active.state_at(12.0).eye.y, -6.0);
        // A cut lands at once.
        assert!(active.cut_to(&CameraTarget::Slot(1), 0.0, 12.0, 120.0));
        assert_eq!(active.state_at(12.0).eye.y, -10.0);
        assert!(!active.is_dissolving(12.0));
        // An unknown target leaves everything alone.
        assert!(!active.cut_to(&CameraTarget::Preset("Nope".into()), 0.0, 12.0, 120.0));
        assert_eq!(active.preset.as_deref(), Some("Wide"));
    }

    /// r[verify viz.camera-cuts] - `after` delays, `for` punches in and returns
    #[test]
    fn scheduled_cuts_fire_on_time_and_punch_ins_return() {
        let wide = cameras().preset("Wide").unwrap().state();
        let mut active = ActiveCamera::new(cameras(), Some("Wide"), wide, 100.0);
        active.schedule(
            CameraCommand::parse("camera Drums after 2 for 2").unwrap(),
            0.0,
            120.0,
        );
        active.advance(0.5, 120.0);
        assert_eq!(active.preset.as_deref(), Some("Wide"));
        active.advance(1.0, 120.0);
        assert_eq!(active.preset.as_deref(), Some("Drums"));
        active.advance(1.5, 120.0);
        assert_eq!(active.preset.as_deref(), Some("Drums"));
        active.advance(2.0, 120.0);
        assert_eq!(active.preset.as_deref(), Some("Wide"));
    }

    /// r[verify viz.camera-birdseye] - the plan is orthographic and looks straight down
    #[test]
    fn the_birds_eye_is_an_orthographic_plan() {
        let state = cameras().preset("Bird's eye").unwrap().state();
        assert!(matches!(
            state.projection(50.0),
            Projection::Orthographic(_)
        ));
        let t = state.transform();
        assert!(t.forward().z < -0.99, "{:?}", t.forward());
        let wide = cameras().preset("Wide").unwrap().state();
        assert!(matches!(wide.projection(50.0), Projection::Perspective(_)));
    }

    /// r[verify viz.camera-birdseye] - ceiling entities are hidden while the plan is up, and back after
    #[test]
    fn the_plan_hides_the_ceiling_and_brings_it_back() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        let wide = cameras().preset("Wide").unwrap().state();
        app.insert_resource(ActiveCamera::new(cameras(), Some("Wide"), wide, 100.0));
        app.add_systems(Update, drive_camera);
        let ceiling = app
            .world_mut()
            .spawn((Name::new("Ceiling"), Visibility::default()))
            .id();
        let wall = app
            .world_mut()
            .spawn((Name::new("Wall - Upstage"), Visibility::default()))
            .id();
        let camera = app
            .world_mut()
            .spawn((
                MainCamera,
                Transform::default(),
                Projection::Perspective(PerspectiveProjection::default()),
            ))
            .id();
        app.update();
        let now = app.world().resource::<Time>().elapsed_secs();
        app.world_mut().resource_mut::<ActiveCamera>().cut_to(
            &CameraTarget::Preset("Bird's eye".into()),
            0.0,
            now,
            120.0,
        );
        app.update();
        assert_eq!(
            *app.world().get::<Visibility>(ceiling).unwrap(),
            Visibility::Hidden
        );
        assert_eq!(
            *app.world().get::<Visibility>(wall).unwrap(),
            Visibility::Inherited
        );
        assert!(matches!(
            app.world().get::<Projection>(camera).unwrap(),
            Projection::Orthographic(_)
        ));
        let now = app.world().resource::<Time>().elapsed_secs();
        app.world_mut().resource_mut::<ActiveCamera>().cut_to(
            &CameraTarget::Slot(1),
            0.0,
            now,
            120.0,
        );
        app.update();
        assert_eq!(
            *app.world().get::<Visibility>(ceiling).unwrap(),
            Visibility::Inherited
        );
        assert!(matches!(
            app.world().get::<Projection>(camera).unwrap(),
            Projection::Perspective(_)
        ));
        assert_eq!(
            app.world().get::<Transform>(camera).unwrap().translation,
            Vec3::new(0.0, -10.0, 2.0)
        );
    }

    /// r[verify viz.camera-presets] - every shipped venue file resolves: favourites and setups name real presets
    #[test]
    fn the_shipped_venue_files_resolve() {
        for venue in ["norco", "riverside"] {
            let dir =
                Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/venues")).join(venue);
            let Some(cameras) = Cameras::load(&dir).unwrap() else {
                continue;
            };
            assert_eq!(cameras.dangling(), Vec::<String>::new(), "{venue}");
            for setup in &cameras.setups {
                assert!(!setup.slots.is_empty(), "{venue}: {}", setup.name);
            }
            if venue == "norco" {
                for name in STANDARD {
                    assert!(cameras.preset(name).is_some(), "norco lacks {name}");
                }
                assert_eq!(cameras.favourites.len(), SLOTS);
                assert!(cameras.preset("Bird's eye").unwrap().ortho);
                for s in ["two", "four", "eight"] {
                    assert!(cameras.setup(s).is_some(), "norco lacks setup {s}");
                }
                assert_eq!(cameras.setup("eight").unwrap().slots.len(), 8);
            }
        }
    }
}

// ── The programme view, and cameras as canvas sources ─────────────────
//
// With one camera in the world it *is* the programme: the keys and the
// cues move it. The studio can also ask for a second, a *programme*
// camera rendering to its own texture (`r[viz.programme-view]`), so the
// operator docks the wide view and the cut side by side — and a canvas
// can take a camera as its source (`r[canvas.camera-source]`), which
// puts the cut onto the side screens the way a real show's IMAG does.
//
// When a programme camera exists the main camera stays on its *wide*
// preset and the cuts move the programme camera; when none exists the
// main camera takes the cuts as before. The programme camera is spawned
// only while something wants it — a Programme pane, or a canvas whose
// source is `camera:programme` — so a lone viewport pays nothing.

/// Content-string prefix for a camera as a canvas source:
/// `camera:programme`, or `camera:<preset>`.
pub const CAMERA_PREFIX: &str = "camera:";

/// What a `camera:` canvas source names.
// r[impl canvas.camera-source] - the programme, or a named preset's camera
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CameraSource {
    Programme,
    Preset(String),
}

impl CameraSource {
    /// `None` for content that is not a camera.
    pub fn parse(content: &str) -> Option<Self> {
        let rest = content.trim().strip_prefix(CAMERA_PREFIX)?.trim();
        if rest.is_empty() {
            return None;
        }
        if rest.eq_ignore_ascii_case("programme") || rest.eq_ignore_ascii_case("program") {
            Some(Self::Programme)
        } else {
            Some(Self::Preset(rest.to_string()))
        }
    }

    pub fn content(&self) -> String {
        match self {
            Self::Programme => format!("{CAMERA_PREFIX}programme"),
            Self::Preset(name) => format!("{CAMERA_PREFIX}{name}"),
        }
    }
}

/// The second camera: the cut, rendered to its own texture.
#[derive(Component)]
pub struct ProgrammeCamera;

/// A fixed camera on one preset, rendering to a texture a canvas
/// samples — `camera:<preset>`.
#[derive(Component)]
pub struct SourceCamera(pub String);

/// The size a camera texture is rendered at unless a host says otherwise.
pub const SOURCE_SIZE: (u32, u32) = (1280, 720);

/// The programme camera's target and who wants it.
// r[impl viz.programme-view] - one programme target, spawned on demand
#[derive(Resource, Debug, Clone)]
pub struct ProgrammeView {
    /// The texture the programme camera draws into. Allocated up front
    /// so a canvas can bind it before the camera exists; the camera
    /// only draws into it while something wants the view.
    pub target: Handle<Image>,
    pub size: (u32, u32),
    /// A host pane is showing it.
    pub host_wants: bool,
    /// A canvas is sampling it.
    pub canvas_wants: bool,
    /// The camera entity, while spawned.
    pub camera: Option<Entity>,
}

impl ProgrammeView {
    pub fn wanted(&self) -> bool {
        self.host_wants || self.canvas_wants
    }
}

/// The preset cameras canvases asked for, by preset name.
#[derive(Resource, Debug, Clone, Default)]
pub struct CameraSources {
    pub targets: std::collections::HashMap<String, Handle<Image>>,
    spawned: std::collections::HashSet<String>,
}

impl CameraSources {
    /// The texture for `source`, allocating one the first time. The
    /// programme's comes from `ProgrammeView`.
    pub fn target_for(
        &mut self,
        source: &CameraSource,
        programme: &mut ProgrammeView,
        images: &mut Assets<Image>,
    ) -> Handle<Image> {
        match source {
            CameraSource::Programme => {
                programme.canvas_wants = true;
                programme.target.clone()
            }
            CameraSource::Preset(name) => self
                .targets
                .entry(name.clone())
                .or_insert_with(|| images.add(camera_target(SOURCE_SIZE)))
                .clone(),
        }
    }
}

/// An offscreen colour target a camera renders into and a material
/// samples.
pub fn camera_target((width, height): (u32, u32)) -> Image {
    let mut target = Image::new_target_texture(
        width.max(1),
        height.max(1),
        bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
        None,
    );
    target.texture_descriptor.usage |= bevy::render::render_resource::TextureUsages::COPY_SRC;
    target
}

/// A panel's home content, so a canvas can be switched to a camera and
/// back at runtime — the studio's TO SCREENS key.
// r[impl canvas.camera-source] - switchable live
#[derive(Component, Debug, Clone)]
pub struct CanvasPanel {
    pub canvas: String,
    /// The panel's own slice of its canvas, the panel's size and its
    /// bezel depth: what a camera quad in its place is built from.
    pub slice: crate::canvas::Slice,
    pub size: Vec3,
    pub depth: f32,
    /// The parent body the quad hangs under.
    pub body: Entity,
}

/// A quad showing a camera over a panel's home content.
#[derive(Component, Debug, Clone)]
pub struct CameraQuad {
    pub canvas: String,
    pub source: CameraSource,
}

/// Canvas sources the host has switched since spawn, by canvas name:
/// `Some(source)` for a camera, `None` for the panel's own content.
#[derive(Resource, Debug, Clone, Default)]
pub struct CanvasSwitches {
    pub current: std::collections::HashMap<String, Option<CameraSource>>,
    pending: Vec<(String, Option<CameraSource>)>,
}

impl CanvasSwitches {
    /// Ask for `canvas` to show `source` (or its own content).
    pub fn set(&mut self, canvas: &str, source: Option<CameraSource>) {
        self.current.insert(canvas.to_string(), source.clone());
        self.pending.push((canvas.to_string(), source));
    }

    /// What the host asked since the last frame.
    fn take(&mut self) -> Vec<(String, Option<CameraSource>)> {
        std::mem::take(&mut self.pending)
    }
}

/// Spawns the programme camera while something wants it, and despawns
/// it when nothing does; spawns a fixed camera for every preset a
/// canvas samples.
// r[impl viz.programme-view] - rendered only while a pane or a canvas shows it
pub fn manage_camera_views(
    mut commands: Commands,
    spec: Option<Res<crate::app::MainCameraSpec>>,
    mut curves: ResMut<Assets<bevy::post_process::auto_exposure::AutoExposureCompensationCurve>>,
    mut images: ResMut<Assets<Image>>,
    mut programme: ResMut<ProgrammeView>,
    mut sources: ResMut<CameraSources>,
    mut active: ResMut<ActiveCamera>,
) {
    let Some(spec) = spec else { return };
    // The programme camera.
    match (programme.wanted(), programme.camera) {
        (true, None) => {
            // Sized to what the host asked; a canvas-only programme is
            // the source size.
            if let Some(image) = images.get(&programme.target)
                && (image.width(), image.height()) != programme.size
            {
                let target = images.add(camera_target(programme.size));
                programme.target = target;
            }
            // A preview of the room the viewport is already showing —
            // see `RenderQuality::preview`.
            // r[impl viz.performance-budget] - a preview is not the viewport
            let mut spec = spec.0;
            spec.quality = spec.quality.preview();
            let camera = crate::app::spawn_camera(&mut commands, &mut curves, spec);
            commands.entity(camera).remove::<MainCamera>().insert((
                ProgrammeCamera,
                Name::new("Programme camera"),
                RenderTarget::Image(programme.target.clone().into()),
                // Before the main camera, so a canvas sampling the
                // programme shows this frame's cut, not last frame's.
                Camera {
                    order: -2,
                    ..default()
                },
            ));
            programme.camera = Some(camera);
            active.dirty = true;
            tracing::info!("viz: programme camera up");
        }
        (false, Some(camera)) => {
            commands.entity(camera).despawn();
            programme.camera = None;
            active.dirty = true;
            tracing::info!("viz: programme camera down");
        }
        _ => {}
    }
    // Preset cameras, once each.
    let wanted: Vec<(String, Handle<Image>)> = sources
        .targets
        .iter()
        .filter(|(name, _)| !sources.spawned.contains(*name))
        .map(|(n, h)| (n.clone(), h.clone()))
        .collect();
    for (name, target) in wanted {
        let Some(preset) = active.cameras.preset(&name) else {
            tracing::warn!(name, "viz: canvas names no such camera preset");
            sources.spawned.insert(name);
            continue;
        };
        let state = preset.state();
        // Likewise a canvas source: a screen in the room, not the
        // operator's view of it.
        // r[impl viz.performance-budget] - a preview is not the viewport
        let mut spec = spec.0;
        spec.quality = spec.quality.preview();
        let camera = crate::app::spawn_camera(&mut commands, &mut curves, spec);
        commands.entity(camera).remove::<MainCamera>().insert((
            SourceCamera(name.clone()),
            Name::new(format!("Camera: {name}")),
            RenderTarget::Image(target.into()),
            Camera {
                order: -3,
                ..default()
            },
            state.transform(),
            state.projection(active.far),
        ));
        sources.spawned.insert(name);
    }
}

/// Which camera the cuts move: the programme camera when there is one,
/// else the main.
pub fn programme_is_separate(programme: Option<&ProgrammeView>) -> bool {
    programme.is_some_and(|p| p.camera.is_some())
}

/// Applies the host's canvas switches: a camera quad over the panel's
/// own content, or the quad removed and the content shown again.
// r[impl canvas.camera-source] - the canvas quad samples the camera's target
#[allow(clippy::too_many_arguments)]
pub fn apply_canvas_switches(
    mut commands: Commands,
    mut switches: ResMut<CanvasSwitches>,
    mut programme: ResMut<ProgrammeView>,
    mut sources: ResMut<CameraSources>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut panels: Query<(Entity, &CanvasPanel, &mut Visibility)>,
    quads: Query<(Entity, &CameraQuad)>,
) {
    for (canvas, source) in switches.take() {
        for (entity, quad) in &quads {
            if quad.canvas == canvas {
                commands.entity(entity).despawn();
            }
        }
        let matching = panels
            .iter_mut()
            .filter(|(_, p, _)| p.canvas == canvas)
            .collect::<Vec<_>>();
        if matching.is_empty() {
            tracing::warn!(canvas, "viz: no such canvas to switch");
            continue;
        }
        for (_, panel, mut visibility) in matching {
            match &source {
                Some(source) => {
                    let target = sources.target_for(source, &mut programme, &mut images);
                    let aspect = SOURCE_SIZE.0 as f32 / SOURCE_SIZE.1 as f32;
                    let slice = panel
                        .slice
                        .cover(panel.size.x / panel.size.y.max(0.01), aspect);
                    let quad = camera_quad(
                        &mut commands,
                        &mut materials,
                        &mut meshes,
                        panel,
                        slice,
                        target,
                    );
                    commands.entity(quad).insert(CameraQuad {
                        canvas: canvas.clone(),
                        source: source.clone(),
                    });
                    *visibility = Visibility::Hidden;
                }
                None => {
                    *visibility = Visibility::Inherited;
                }
            }
        }
        if source.is_none()
            && !switches
                .current
                .values()
                .any(|s| matches!(s, Some(CameraSource::Programme)))
        {
            programme.canvas_wants = false;
        }
    }
}

/// A display quad in a panel's place, lit by `target`.
fn camera_quad(
    commands: &mut Commands,
    materials: &mut Assets<StandardMaterial>,
    meshes: &mut Assets<Mesh>,
    panel: &CanvasPanel,
    slice: crate::canvas::Slice,
    target: Handle<Image>,
) -> Entity {
    commands
        .spawn((
            crate::spawn::ScreenSurface,
            Mesh3d(meshes.add(crate::canvas::sliced_quad(slice))),
            MeshMaterial3d(crate::spawn::display_material(materials, target)),
            Transform {
                translation: Vec3::Z * (panel.depth * 0.5 + 0.006),
                scale: Vec3::new(panel.size.x * 0.94, panel.size.y * 0.94, 1.0),
                ..default()
            },
            ChildOf(panel.body),
        ))
        .id()
}

#[cfg(test)]
mod source_tests {
    use super::*;

    /// r[verify canvas.camera-source] - `camera:programme` and `camera:<preset>` parse; the rest is not a camera
    #[test]
    fn camera_sources_parse() {
        assert_eq!(
            CameraSource::parse("camera:programme"),
            Some(CameraSource::Programme)
        );
        assert_eq!(
            CameraSource::parse("camera:program"),
            Some(CameraSource::Programme)
        );
        assert_eq!(
            CameraSource::parse("camera:Drums"),
            Some(CameraSource::Preset("Drums".into()))
        );
        assert_eq!(
            CameraSource::parse("camera: Side stage "),
            Some(CameraSource::Preset("Side stage".into()))
        );
        assert_eq!(CameraSource::parse("camera:"), None);
        assert_eq!(CameraSource::parse("proc:rainbow"), None);
        assert_eq!(CameraSource::parse("clips/city.mp4"), None);
        assert_eq!(CameraSource::Programme.content(), "camera:programme");
    }

    /// r[verify canvas.camera-source] - a switched canvas gets a quad whose material samples the programme target
    #[test]
    fn a_switched_canvas_samples_the_programme_target() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::asset::AssetPlugin::default()));
        app.init_asset::<Image>();
        app.init_asset::<Mesh>();
        app.init_asset::<StandardMaterial>();
        let target = app
            .world_mut()
            .resource_mut::<Assets<Image>>()
            .add(camera_target((64, 36)));
        app.insert_resource(ProgrammeView {
            target: target.clone(),
            size: (64, 36),
            host_wants: false,
            canvas_wants: false,
            camera: None,
        });
        app.init_resource::<CameraSources>();
        app.init_resource::<CanvasSwitches>();
        app.add_systems(Update, apply_canvas_switches);
        let body = app.world_mut().spawn(Transform::default()).id();
        let panel = app
            .world_mut()
            .spawn((
                CanvasPanel {
                    canvas: "side-left".into(),
                    slice: crate::canvas::Slice::FULL,
                    size: Vec3::new(1.6, 0.9, 0.05),
                    depth: 0.05,
                    body,
                },
                Visibility::default(),
            ))
            .id();
        app.world_mut()
            .resource_mut::<CanvasSwitches>()
            .set("side-left", Some(CameraSource::Programme));
        app.update();
        assert!(app.world().resource::<ProgrammeView>().canvas_wants);
        assert_eq!(
            *app.world().get::<Visibility>(panel).unwrap(),
            Visibility::Hidden
        );
        let mut quads = app
            .world_mut()
            .query::<(&CameraQuad, &MeshMaterial3d<StandardMaterial>)>();
        let (quad, material) = quads.single(app.world()).expect("one camera quad");
        assert_eq!(quad.source, CameraSource::Programme);
        let materials = app.world().resource::<Assets<StandardMaterial>>();
        let material = materials.get(&material.0).unwrap();
        assert_eq!(material.emissive_texture.as_ref(), Some(&target));
        assert_eq!(material.base_color_texture.as_ref(), Some(&target));
        // Switched back: the quad goes, the panel shows, nothing wants the programme.
        app.world_mut()
            .resource_mut::<CanvasSwitches>()
            .set("side-left", None);
        app.update();
        assert_eq!(quads.iter(app.world()).count(), 0);
        assert_eq!(
            *app.world().get::<Visibility>(panel).unwrap(),
            Visibility::Inherited
        );
        assert!(!app.world().resource::<ProgrammeView>().canvas_wants);
    }
}
