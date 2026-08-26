//! Hardware and the network driving the surface: MIDI and OSC.
//!
//! Both arrive as the same `Command`s the UI sends, through the same
//! channel, so a fader moved on a nanoKONTROL and a fader dragged on
//! screen are one gesture arriving from two places. The mapping is a
//! document — `data/profiles/remote.json` by default,
//! `IGNITION_REMOTE` to point elsewhere — not code, per
//! `r[playback.remote-inputs]`: which CC is which fader is a fact
//! about the controller on the desk tonight, and recompiling to change
//! it would be the wrong tool.
//!
//! The parser and the translation from a control to a command are
//! pure and tested without hardware; the two listener threads are
//! behind the `midi` and `osc` features so a default build needs
//! neither library. A port or socket that is not there logs and the
//! surface carries on.
//!
//! Mapping format:
//!
//! ```json
//! {
//!   "midi": [
//!     { "port": "nanoKONTROL",
//!       "cc":    { "0": {"fader": 0}, "16": {"master": "Key"}, "23": "rate" },
//!       "notes": { "32": {"key": {"index": 0, "action": "Flash"}}, "41": "play" } }
//!   ],
//!   "osc": {
//!     "port": 9000,
//!     "addresses": { "/fader/1": {"fader": 0}, "/go": "go" }
//!   }
//! }
//! ```
//!
//! `midi` is one device or a list of them; every device whose port is
//! present connects. A binding is either a bare verb (`"go"`,
//! `"play"`, `"blind"`, `"page_next"`) or an object naming a target —
//! see [`Binding`].

// The parser and translator are pure and tested without hardware; in a
// build with neither listener they are only reached by the tests.
#![cfg_attr(not(any(feature = "midi", feature = "osc")), allow(dead_code))]

use crate::command::{Command, PageMove, Sender};
use ignition_core::KeyAction;
use serde::Deserialize;
use std::collections::BTreeMap;

/// Where the mapping lives unless `IGNITION_REMOTE` says otherwise.
pub const DEFAULT_MAPPING: &str = "data/profiles/remote.json";

/// The whole mapping document.
// r[impl playback.remote-inputs] - MIDI CC/notes and OSC, mapped by data/profiles/remote.json rather than code
#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RemoteConfig {
    /// Room for a note at the top of the file. JSON has no comments.
    #[serde(default, rename = "_comment")]
    pub comment: Option<String>,
    /// One controller or several. Each connects if its port is found.
    #[serde(default, deserialize_with = "one_or_many")]
    pub midi: Vec<MidiConfig>,
    #[serde(default)]
    pub osc: Option<OscConfig>,
}

/// One MIDI controller.
#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MidiConfig {
    /// A substring of the port's name as the OS reports it —
    /// `"nanoKONTROL"` matches `nanoKONTROL2 nanoKONTROL2 _ CTRL`.
    pub port: String,
    /// Control change number → binding. Keys are strings because that
    /// is what JSON object keys are; they parse as `u8`.
    #[serde(default)]
    pub cc: BTreeMap<String, Binding>,
    /// Note number → binding. Note-on is the hand going down, note-off
    /// (or a note-on at velocity zero) is it coming up.
    #[serde(default)]
    pub notes: BTreeMap<String, Binding>,
    /// Only listen on this channel (1–16). `None` is any channel.
    #[serde(default)]
    pub channel: Option<u8>,
}

/// The OSC listener.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct OscConfig {
    pub port: u16,
    /// Address → binding. The first numeric argument is the value; a
    /// message with no arguments is a press.
    #[serde(default)]
    pub addresses: BTreeMap<String, Binding>,
}

/// What a control does.
///
/// Externally tagged, so a bare string is a verb with no target and
/// an object names one. The value the control carries — a CC's 0–127,
/// a note's on/off, an OSC float — is normalised to 0..=1 and a
/// boolean before it reaches `translate`.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Binding {
    /// A bank fader's level, by slot 0–7.
    Fader(usize),
    /// A playback key on a slot. Down on press, up on release.
    Key {
        index: usize,
        action: KeyAction,
    },
    /// A role master's level.
    Master(String),
    /// The surface's `Rate` master, 40–220 BPM over the control's
    /// travel — the same range the on-screen fader has.
    Rate,
    /// Effect size, 0..=1.
    Size,
    /// Effect speed multiplier, half to double.
    Speed,
    /// Program time, 0–4 beats.
    ProgramTime,
    /// Intensity for the current selection.
    Dimmer,
    /// Toggles: a press flips the state, and the value is ignored.
    Blind,
    Highlight,
    Lowlight,
    /// A press.
    Go,
    LookGo,
    Play,
    Stop,
    PageNext,
    PagePrev,
    Page(usize),
    /// Solo a role while held; release clears.
    Solo(String),
    /// Clear the hand.
    Release,
}

/// A control's value, normalised: a level 0..=1 and whether the hand
/// is down. A CC at 0 is `on: false`; a note-on is `on: true`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Input {
    pub value: f32,
    pub on: bool,
}

impl Input {
    pub fn cc(value: u8) -> Self {
        let value = f32::from(value.min(127)) / 127.0;
        Self {
            value,
            on: value > 0.5,
        }
    }

    pub fn note(on: bool) -> Self {
        Self {
            value: if on { 1.0 } else { 0.0 },
            on,
        }
    }
}

/// The toggles keep their state here rather than asking the engine,
/// because a MIDI button is momentary and the toggle is the surface's
/// idea, not the controller's.
#[derive(Debug, Default, Clone)]
pub struct Toggles {
    blind: bool,
    highlight: bool,
    lowlight: bool,
}

/// What a binding sends for one input. Empty for a release of a
/// control that has no release.
// r[impl playback.remote-inputs] - one input against its binding becomes fader, key and master commands
pub fn translate(binding: &Binding, input: Input, toggles: &mut Toggles) -> Vec<Command> {
    let press = input.on;
    match binding {
        Binding::Fader(index) => vec![Command::Level(*index, input.value)],
        Binding::Key { index, action } => vec![Command::Key {
            index: *index,
            action: *action,
            down: press,
        }],
        Binding::Master(role) => vec![Command::Master(role.clone(), input.value)],
        Binding::Rate => vec![Command::Rate(40.0 + input.value * 180.0)],
        Binding::Size => vec![Command::Size(input.value)],
        Binding::Speed => vec![Command::EffectRate(0.5 + input.value * 1.5)],
        Binding::ProgramTime => vec![Command::ProgramTime(input.value * 4.0)],
        Binding::Dimmer => vec![Command::Dimmer(input.value)],
        Binding::Blind => {
            if !press {
                return Vec::new();
            }
            toggles.blind = !toggles.blind;
            vec![Command::Blind(toggles.blind)]
        }
        Binding::Highlight => {
            if !press {
                return Vec::new();
            }
            toggles.highlight = !toggles.highlight;
            vec![Command::Highlight(toggles.highlight)]
        }
        Binding::Lowlight => {
            if !press {
                return Vec::new();
            }
            toggles.lowlight = !toggles.lowlight;
            vec![Command::Lowlight(toggles.lowlight)]
        }
        Binding::Go => press.then_some(Command::Go).into_iter().collect(),
        Binding::LookGo => press.then_some(Command::LookGo).into_iter().collect(),
        Binding::Play => press.then_some(Command::Play).into_iter().collect(),
        Binding::Stop => press.then_some(Command::Stop).into_iter().collect(),
        Binding::PageNext => press
            .then_some(Command::Page(PageMove::Next))
            .into_iter()
            .collect(),
        Binding::PagePrev => press
            .then_some(Command::Page(PageMove::Prev))
            .into_iter()
            .collect(),
        Binding::Page(n) => press
            .then_some(Command::Page(PageMove::Set(*n)))
            .into_iter()
            .collect(),
        Binding::Solo(role) => vec![Command::Solo(press.then(|| role.clone()))],
        Binding::Release => press.then_some(Command::Release).into_iter().collect(),
    }
}

/// A MIDI message this cares about, decoded from the wire bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MidiMsg {
    /// Channel 1–16.
    Cc {
        channel: u8,
        cc: u8,
        value: u8,
    },
    NoteOn {
        channel: u8,
        note: u8,
        velocity: u8,
    },
    NoteOff {
        channel: u8,
        note: u8,
    },
}

/// Decodes a channel-voice message. Anything else — sysex, clock,
/// aftertouch — is `None`. A note-on at velocity zero is a note-off,
/// which is how most controllers spell release.
pub fn decode_midi(bytes: &[u8]) -> Option<MidiMsg> {
    let (&status, data) = bytes.split_first()?;
    let channel = (status & 0x0f) + 1;
    match status & 0xf0 {
        0xb0 => Some(MidiMsg::Cc {
            channel,
            cc: *data.first()?,
            value: *data.get(1)?,
        }),
        0x90 => {
            let note = *data.first()?;
            let velocity = *data.get(1)?;
            Some(if velocity == 0 {
                MidiMsg::NoteOff { channel, note }
            } else {
                MidiMsg::NoteOn {
                    channel,
                    note,
                    velocity,
                }
            })
        }
        0x80 => Some(MidiMsg::NoteOff {
            channel,
            note: *data.first()?,
        }),
        _ => None,
    }
}

impl MidiConfig {
    /// The commands one message sends under this mapping.
    pub fn commands_for(&self, msg: MidiMsg, toggles: &mut Toggles) -> Vec<Command> {
        let (channel, binding, input) = match msg {
            MidiMsg::Cc { channel, cc, value } => {
                (channel, self.cc.get(&cc.to_string()), Input::cc(value))
            }
            MidiMsg::NoteOn { channel, note, .. } => (
                channel,
                self.notes.get(&note.to_string()),
                Input::note(true),
            ),
            MidiMsg::NoteOff { channel, note } => (
                channel,
                self.notes.get(&note.to_string()),
                Input::note(false),
            ),
        };
        if self.channel.is_some_and(|want| want != channel) {
            return Vec::new();
        }
        match binding {
            Some(binding) => translate(binding, input, toggles),
            None => Vec::new(),
        }
    }
}

impl OscConfig {
    /// The commands one OSC message sends. `value` is the first
    /// numeric argument, if any; a message with no arguments is a
    /// press, and a numeric one is on when above half — or, for a
    /// bare integer 0/1 as most OSC button apps send, when non-zero.
    pub fn commands_for(
        &self,
        address: &str,
        value: Option<f32>,
        toggles: &mut Toggles,
    ) -> Vec<Command> {
        let Some(binding) = self.addresses.get(address) else {
            return Vec::new();
        };
        let input = match value {
            Some(v) => Input {
                value: v.clamp(0.0, 1.0),
                on: v > 0.5,
            },
            None => Input::note(true),
        };
        translate(binding, input, toggles)
    }
}

/// Every key of a `cc` / `notes` map has to be a byte, or the mapping
/// is wrong in a way that would otherwise show up as a dead fader.
pub fn validate(config: &RemoteConfig) -> Result<(), String> {
    for device in &config.midi {
        for key in device.cc.keys().chain(device.notes.keys()) {
            key.parse::<u8>()
                .map_err(|_| format!("{}: {key:?} is not a MIDI number", device.port))?;
        }
        for binding in device.cc.values().chain(device.notes.values()) {
            check_slot(binding)?;
        }
    }
    if let Some(osc) = &config.osc {
        for (address, binding) in &osc.addresses {
            if !address.starts_with('/') {
                return Err(format!("OSC address {address:?} must start with '/'"));
            }
            check_slot(binding)?;
        }
    }
    Ok(())
}

fn check_slot(binding: &Binding) -> Result<(), String> {
    let slot = match binding {
        Binding::Fader(i) | Binding::Key { index: i, .. } => Some(*i),
        _ => None,
    };
    match slot {
        Some(i) if i >= ignition_core::FADERS => Err(format!(
            "fader slot {i} is past the last of {}",
            ignition_core::FADERS
        )),
        _ => Ok(()),
    }
}

pub fn parse(text: &str) -> Result<RemoteConfig, String> {
    let config: RemoteConfig = serde_json::from_str(text).map_err(|e| e.to_string())?;
    validate(&config)?;
    Ok(config)
}

/// `"midi": {...}` or `"midi": [{...}, {...}]` — one controller or a
/// desk full of them.
fn one_or_many<'de, D>(deserializer: D) -> Result<Vec<MidiConfig>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum OneOrMany {
        One(MidiConfig),
        Many(Vec<MidiConfig>),
    }
    Ok(match OneOrMany::deserialize(deserializer)? {
        OneOrMany::One(one) => vec![one],
        OneOrMany::Many(many) => many,
    })
}

/// Loads the mapping and starts whichever listeners are compiled in.
///
/// A mapping that is missing is not an error: the surface works
/// without a controller. One that is present but wrong is logged in
/// full, since a silently ignored line is a fader that "does nothing".
pub fn start(tx: Sender) {
    let path = std::env::var("IGNITION_REMOTE").unwrap_or_else(|_| DEFAULT_MAPPING.to_string());
    let config = match std::fs::read_to_string(&path) {
        Ok(text) => match parse(&text) {
            Ok(config) => config,
            Err(e) => {
                tracing::warn!(path, error = %e, "remote: mapping rejected");
                return;
            }
        },
        Err(e) => {
            tracing::info!(path, error = %e, "remote: no mapping; no MIDI or OSC");
            return;
        }
    };
    tracing::info!(
        path,
        midi = config.midi.len(),
        osc = config.osc.as_ref().map(|o| o.port),
        "remote: mapping loaded"
    );
    for device in config.midi {
        midi::start(device, tx.clone());
    }
    if let Some(osc) = config.osc {
        osc_listener::start(osc, tx);
    }
}

#[cfg(feature = "midi")]
mod midi {
    use super::{MidiConfig, Toggles, decode_midi};
    use crate::command::Sender;
    use midir::{Ignore, MidiInput};

    /// Opens the first port whose name contains `device.port` and
    /// forwards its messages for as long as the app runs. The
    /// connection has to be kept alive, so the thread parks on it.
    pub fn start(device: MidiConfig, tx: Sender) {
        std::thread::Builder::new()
            .name(format!("midi {}", device.port))
            .spawn(move || {
                let mut input = match MidiInput::new("ignition-studio") {
                    Ok(input) => input,
                    Err(e) => {
                        tracing::warn!(error = %e, "midi: no MIDI system");
                        return;
                    }
                };
                input.ignore(Ignore::All);
                let ports = input.ports();
                let found = ports.iter().find(|p| {
                    input
                        .port_name(p)
                        .is_ok_and(|name| name.contains(&device.port))
                });
                let Some(port) = found else {
                    let names: Vec<String> = ports
                        .iter()
                        .filter_map(|p| input.port_name(p).ok())
                        .collect();
                    tracing::warn!(
                        want = device.port,
                        have = ?names,
                        "midi: port not found; that controller is off tonight"
                    );
                    return;
                };
                let name = input.port_name(port).unwrap_or_default();
                let mut toggles = Toggles::default();
                let connection = input.connect(
                    port,
                    "ignition-studio",
                    move |_stamp, bytes, _| {
                        if let Some(msg) = decode_midi(bytes) {
                            for command in device.commands_for(msg, &mut toggles) {
                                let _ = tx.send(command);
                            }
                        }
                    },
                    (),
                );
                match connection {
                    Ok(_connection) => {
                        tracing::info!(port = name, "midi: connected");
                        loop {
                            std::thread::park();
                        }
                    }
                    Err(e) => tracing::warn!(port = name, error = %e, "midi: connect failed"),
                }
            })
            .ok();
    }
}

#[cfg(not(feature = "midi"))]
mod midi {
    use super::MidiConfig;
    use crate::command::Sender;

    pub fn start(device: MidiConfig, _tx: Sender) {
        tracing::info!(
            port = device.port,
            "midi: mapped but this build has no `midi` feature"
        );
    }
}

#[cfg(feature = "osc")]
mod osc_listener {
    use super::{OscConfig, Toggles};
    use crate::command::Sender;
    use rosc::{OscPacket, OscType};

    /// The first numeric argument as a float; a bool as 0/1.
    fn value_of(args: &[OscType]) -> Option<f32> {
        args.iter().find_map(|arg| match arg {
            OscType::Float(v) => Some(*v),
            OscType::Double(v) => Some(*v as f32),
            OscType::Int(v) => Some(*v as f32),
            OscType::Long(v) => Some(*v as f32),
            OscType::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
            _ => None,
        })
    }

    fn handle(packet: OscPacket, config: &OscConfig, toggles: &mut Toggles, tx: &Sender) {
        match packet {
            OscPacket::Message(msg) => {
                let value = value_of(&msg.args);
                for command in config.commands_for(&msg.addr, value, toggles) {
                    let _ = tx.send(command);
                }
            }
            OscPacket::Bundle(bundle) => {
                for inner in bundle.content {
                    handle(inner, config, toggles, tx);
                }
            }
        }
    }

    pub fn start(config: OscConfig, tx: Sender) {
        std::thread::Builder::new()
            .name("osc".into())
            .spawn(move || {
                let socket = match std::net::UdpSocket::bind(("0.0.0.0", config.port)) {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::warn!(port = config.port, error = %e, "osc: cannot bind");
                        return;
                    }
                };
                tracing::info!(port = config.port, "osc: listening");
                let mut toggles = Toggles::default();
                let mut buf = [0u8; 4096];
                loop {
                    let Ok((n, _from)) = socket.recv_from(&mut buf) else {
                        continue;
                    };
                    match rosc::decoder::decode_udp(&buf[..n]) {
                        Ok((_, packet)) => handle(packet, &config, &mut toggles, &tx),
                        Err(e) => tracing::debug!(error = %e, "osc: bad packet"),
                    }
                }
            })
            .ok();
    }
}

#[cfg(not(feature = "osc"))]
mod osc_listener {
    use super::OscConfig;
    use crate::command::Sender;

    pub fn start(config: OscConfig, _tx: Sender) {
        tracing::info!(
            port = config.port,
            "osc: mapped but this build has no `osc` feature"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAPPING: &str = r#"{
        "midi": { "port": "nanoKONTROL",
                  "cc": { "0": {"fader": 0}, "7": {"fader": 7}, "16": {"master": "Key"}, "23": "rate" },
                  "notes": { "32": {"key": {"index": 0, "action": "Flash"}},
                             "48": {"key": {"index": 0, "action": "Toggle"}},
                             "41": "play", "46": "blind", "59": "page_next" } },
        "osc": { "port": 9000,
                 "addresses": { "/fader/1": {"fader": 0}, "/go": "go", "/blind": "blind" } }
    }"#;

    fn assert_level(commands: &[Command], index: usize, level: f32) {
        match commands {
            [Command::Level(i, v)] => {
                assert_eq!(*i, index);
                assert!((v - level).abs() < 1e-3, "{v} != {level}");
            }
            other => panic!("expected one Level, got {other:?}"),
        }
    }

    /// r[verify playback.remote-inputs]
    #[test]
    fn the_mapping_parses_one_device_or_many() {
        let config = parse(MAPPING).expect("parses");
        assert_eq!(config.midi.len(), 1);
        assert_eq!(config.midi[0].port, "nanoKONTROL");
        assert_eq!(config.midi[0].cc["0"], Binding::Fader(0));
        assert_eq!(config.midi[0].cc["23"], Binding::Rate);
        assert_eq!(
            config.midi[0].notes["32"],
            Binding::Key {
                index: 0,
                action: KeyAction::Flash
            }
        );
        assert_eq!(config.osc.as_ref().map(|o| o.port), Some(9000));

        let many = parse(r#"{"midi": [{"port": "a"}, {"port": "b"}]}"#).expect("parses");
        assert_eq!(many.midi.len(), 2);
    }

    #[test]
    fn a_bad_slot_or_a_bad_number_is_rejected_with_a_reason() {
        let e = parse(r#"{"midi": {"port": "x", "cc": {"0": {"fader": 9}}}}"#).unwrap_err();
        assert!(e.contains("slot 9"), "{e}");
        let e = parse(r#"{"midi": {"port": "x", "cc": {"zero": {"fader": 0}}}}"#).unwrap_err();
        assert!(e.contains("not a MIDI number"), "{e}");
        let e = parse(r#"{"osc": {"port": 1, "addresses": {"go": "go"}}}"#).unwrap_err();
        assert!(e.contains("start with '/'"), "{e}");
    }

    #[test]
    fn midi_bytes_decode_to_channel_voice_messages() {
        assert_eq!(
            decode_midi(&[0xb0, 7, 127]),
            Some(MidiMsg::Cc {
                channel: 1,
                cc: 7,
                value: 127
            })
        );
        assert_eq!(
            decode_midi(&[0x91, 32, 100]),
            Some(MidiMsg::NoteOn {
                channel: 2,
                note: 32,
                velocity: 100
            })
        );
        // Velocity zero is a release, the way most controllers send it.
        assert_eq!(
            decode_midi(&[0x90, 32, 0]),
            Some(MidiMsg::NoteOff {
                channel: 1,
                note: 32
            })
        );
        assert_eq!(decode_midi(&[0xf8]), None, "clock is ignored");
        assert_eq!(decode_midi(&[0xb0]), None, "a truncated message is ignored");
    }

    /// r[verify playback.remote-inputs]
    #[test]
    fn a_cc_moves_the_fader_it_is_mapped_to() {
        let config = parse(MAPPING).unwrap();
        let mut toggles = Toggles::default();
        let device = &config.midi[0];
        let out = device.commands_for(
            MidiMsg::Cc {
                channel: 1,
                cc: 0,
                value: 127,
            },
            &mut toggles,
        );
        assert_level(&out, 0, 1.0);
        let out = device.commands_for(
            MidiMsg::Cc {
                channel: 1,
                cc: 7,
                value: 0,
            },
            &mut toggles,
        );
        assert_level(&out, 7, 0.0);
        // A CC nobody mapped does nothing.
        let out = device.commands_for(
            MidiMsg::Cc {
                channel: 1,
                cc: 99,
                value: 64,
            },
            &mut toggles,
        );
        assert!(out.is_empty());
    }

    #[test]
    fn a_note_is_a_key_going_down_and_coming_up() {
        let config = parse(MAPPING).unwrap();
        let mut toggles = Toggles::default();
        let device = &config.midi[0];
        let down = device.commands_for(
            MidiMsg::NoteOn {
                channel: 1,
                note: 32,
                velocity: 90,
            },
            &mut toggles,
        );
        assert!(matches!(
            down[..],
            [Command::Key {
                index: 0,
                action: KeyAction::Flash,
                down: true
            }]
        ));
        let up = device.commands_for(
            MidiMsg::NoteOff {
                channel: 1,
                note: 32,
            },
            &mut toggles,
        );
        assert!(matches!(
            up[..],
            [Command::Key {
                index: 0,
                action: KeyAction::Flash,
                down: false
            }]
        ));
    }

    #[test]
    fn verbs_fire_on_press_only_and_toggles_flip() {
        let config = parse(MAPPING).unwrap();
        let mut toggles = Toggles::default();
        let device = &config.midi[0];
        let on = |note| MidiMsg::NoteOn {
            channel: 1,
            note,
            velocity: 1,
        };
        let off = |note| MidiMsg::NoteOff { channel: 1, note };
        assert!(matches!(
            device.commands_for(on(41), &mut toggles)[..],
            [Command::Play]
        ));
        assert!(device.commands_for(off(41), &mut toggles).is_empty());
        assert!(matches!(
            device.commands_for(on(59), &mut toggles)[..],
            [Command::Page(PageMove::Next)]
        ));
        assert!(matches!(
            device.commands_for(on(46), &mut toggles)[..],
            [Command::Blind(true)]
        ));
        assert!(device.commands_for(off(46), &mut toggles).is_empty());
        assert!(matches!(
            device.commands_for(on(46), &mut toggles)[..],
            [Command::Blind(false)]
        ));
    }

    #[test]
    fn the_rate_binding_spans_the_same_range_as_the_on_screen_fader() {
        let mut toggles = Toggles::default();
        match translate(&Binding::Rate, Input::cc(0), &mut toggles)[..] {
            [Command::Rate(bpm)] => assert!((bpm - 40.0).abs() < 1e-3),
            _ => panic!(),
        }
        match translate(&Binding::Rate, Input::cc(127), &mut toggles)[..] {
            [Command::Rate(bpm)] => assert!((bpm - 220.0).abs() < 1e-3),
            _ => panic!(),
        }
    }

    #[test]
    fn a_channel_filter_drops_other_channels() {
        let mut device = parse(MAPPING).unwrap().midi.remove(0);
        device.channel = Some(3);
        let mut toggles = Toggles::default();
        let msg = MidiMsg::Cc {
            channel: 1,
            cc: 0,
            value: 10,
        };
        assert!(device.commands_for(msg, &mut toggles).is_empty());
        let msg = MidiMsg::Cc {
            channel: 3,
            cc: 0,
            value: 10,
        };
        assert_eq!(device.commands_for(msg, &mut toggles).len(), 1);
    }

    /// r[verify playback.remote-inputs]
    #[test]
    fn osc_addresses_translate_the_same_way() {
        let config = parse(MAPPING).unwrap();
        let osc = config.osc.unwrap();
        let mut toggles = Toggles::default();
        assert_level(
            &osc.commands_for("/fader/1", Some(0.25), &mut toggles),
            0,
            0.25,
        );
        assert!(matches!(
            osc.commands_for("/go", None, &mut toggles)[..],
            [Command::Go]
        ));
        // A button app sending 1 then 0: the press fires, the release
        // does not.
        assert!(matches!(
            osc.commands_for("/go", Some(1.0), &mut toggles)[..],
            [Command::Go]
        ));
        assert!(osc.commands_for("/go", Some(0.0), &mut toggles).is_empty());
        assert!(osc.commands_for("/nothing", None, &mut toggles).is_empty());
    }

    /// The file that ships has to parse, or the first night with a
    /// controller is the night this is found out.
    #[test]
    fn the_shipped_mapping_parses() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../",
            "data/profiles/remote.json"
        );
        let text = std::fs::read_to_string(path).expect("remote.json ships");
        let config = parse(&text).expect("remote.json parses");
        let ports: Vec<&str> = config.midi.iter().map(|d| d.port.as_str()).collect();
        assert!(ports.iter().any(|p| p.contains("nanoKONTROL")), "{ports:?}");
        assert!(ports.iter().any(|p| p.contains("X-TOUCH")), "{ports:?}");
        assert!(config.osc.is_some());
    }
}
