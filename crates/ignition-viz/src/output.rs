//! DMX out of the visualizer's own universes.
//!
//! The encoder (`show.rs::apply_output`) writes every frame's bytes into
//! `DmxUniverses`; the visualizer decodes them (`spawn.rs`) and the
//! transmitter here sends them. One buffer, read twice, so what is on
//! screen and what leaves the socket can only ever be the same bytes.
//!
//! The transmit crate (`ignition_io`) owns the sockets and the rate; this
//! module owns the seam: when to snapshot, how to say what is happening,
//! and the switch.

use crate::dmx::DmxUniverses;
use bevy::prelude::*;
use ignition_io::{OutputConfig, Protocol, Sender, Sink, Status};
use std::time::Instant;

/// The transmitter, as a resource. `sender` is `None` for a viz with no
/// transmitter at all (a snapshot, an export). A socket that would not
/// bind is not a failure here: the sender reports it in its status and
/// the overlay shows it, because a desk whose sockets did not open is
/// still a desk with a visualizer.
#[derive(Resource, Default)]
pub struct DmxOutput {
    sender: Option<Sender>,
    /// What the operator asked for. Kept apart from the sender so the
    /// status line can say "off" and "errored" as different things.
    enabled: bool,
}

impl DmxOutput {
    /// Binds `config` under `source_name`, enabled or not. Never fails:
    /// a bind error is a status, not a crash.
    // r[impl dmx.output-toggle] - bind errors are visible state, not a panic
    pub fn bind(config: &OutputConfig, source_name: &str, enabled: bool) -> Self {
        let sender = Sender::bind(config, source_name);
        sender.set_enabled(enabled);
        for error in &sender.status().errors {
            tracing::warn!(error = %error, "dmx output: bind");
        }
        Self {
            sender: Some(sender),
            enabled,
        }
    }

    /// A resource that sends nothing and says so — for a viz with no
    /// transmitter at all (an export, a snapshot).
    pub fn disabled() -> Self {
        Self::default()
    }

    /// Adds a sink that sees every frame the sender sends — see
    /// `LoopbackSink`.
    pub fn with_sink(mut self, sink: Box<dyn Sink>) -> Self {
        if let Some(sender) = self.sender.take() {
            self.sender = Some(sender.with_sink(sink));
        }
        self
    }

    // r[impl dmx.output-toggle] - switchable without stopping the engine
    pub fn set_enabled(&mut self, on: bool) {
        self.enabled = on;
        if let Some(sender) = &self.sender {
            sender.set_enabled(on);
        }
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// Sends `frame` now. A missing sender is a no-op: the error is
    /// already on the status.
    pub fn send(&self, frame: &std::collections::HashMap<u16, [u8; 512]>) {
        if let Some(sender) = &self.sender {
            sender.send(frame, Instant::now());
        }
    }

    /// What is happening, for the overlay and the surface.
    // r[impl dmx.output-toggle] - sending, which universes, at what rate, and any error
    pub fn summary(&self) -> OutputSummary {
        match &self.sender {
            None => OutputSummary {
                enabled: self.enabled,
                ..Default::default()
            },
            Some(sender) => OutputSummary::of(&sender.status(), self.enabled),
        }
    }

    pub fn stop(&mut self) {
        if let Some(sender) = self.sender.take() {
            sender.stop();
        }
    }
}

/// The transmitter's state, flattened for display. Its own type rather
/// than `ignition_io::Status` so the overlay and the studio's button can
/// be tested without a socket.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct OutputSummary {
    pub enabled: bool,
    /// Protocol names in use — `sACN`, `Art-Net` — in that order.
    pub protocols: Vec<String>,
    /// How many universes are configured.
    pub universes: usize,
    /// Frames per second per universe, as the sender measures it.
    pub hz: f32,
    /// The first error the sender or the bind reported.
    pub error: Option<String>,
    /// One line per universe, for the surface's detail.
    pub lines: Vec<String>,
}

impl OutputSummary {
    fn of(status: &Status, enabled: bool) -> Self {
        let name = |p: &Protocol| match p {
            Protocol::Sacn => "sACN",
            Protocol::Artnet => "Art-Net",
        };
        let mut protocols: Vec<String> = Vec::new();
        let mut lines = Vec::new();
        let mut hz = 0.0f32;
        for u in &status.per_universe {
            for p in &u.protocols {
                let n = name(p).to_string();
                if !protocols.contains(&n) {
                    protocols.push(n);
                }
            }
            hz = hz.max(u.hz);
            let names: Vec<&str> = u.protocols.iter().map(name).collect();
            let via = if names.is_empty() {
                "no socket".to_string()
            } else {
                names.join(" + ")
            };
            lines.push(format!("U{} {via} {:.0}Hz", u.universe, u.hz));
        }
        lines.extend(status.errors.iter().map(|e| format!("error: {e}")));
        Self {
            enabled,
            protocols,
            universes: status.per_universe.len(),
            hz,
            error: status.errors.first().cloned(),
            lines,
        }
    }

    /// The overlay's line: `OUT sACN ×4 44Hz`, `OUT off`, or the error.
    // r[impl dmx.output-toggle] - the state, on the picture
    pub fn line(&self) -> String {
        if let Some(e) = &self.error {
            return format!("OUT ERROR {e}");
        }
        if !self.enabled {
            return "OUT off".to_string();
        }
        if self.universes == 0 {
            return "OUT on (no universes)".to_string();
        }
        let protocols = if self.protocols.is_empty() {
            "none".to_string()
        } else {
            self.protocols.join("+")
        };
        format!("OUT {protocols} ×{} {:.0}Hz", self.universes, self.hz)
    }
}

/// Snapshots the universes and hands them to the sender. Runs after the
/// encoder has written the frame and before anything else could touch
/// it — the bytes the sender gets are the bytes `resolve_live_dmx`
/// decodes this frame.
// r[impl dmx.one-frame] - one snapshot per frame feeds every protocol
pub fn send_output(dmx: Option<Res<crate::spawn::DmxRes>>, output: Option<Res<DmxOutput>>) {
    let (Some(dmx), Some(output)) = (dmx, output) else {
        return;
    };
    if !output.enabled() {
        return;
    }
    output.send(&dmx.0.snapshot());
}

/// A sink that writes the transmitted frame back into `DmxUniverses`.
///
/// Not the normal path, and deliberately so. The visualizer already
/// reads the encoder's own universes — the same buffer the sender
/// snapshots — so there is nothing to loop back in day-to-day use;
/// a loopback in the loop would only write the bytes over themselves
/// a frame late. What this is for is verification: with it attached
/// (`--loopback`, or `IGNITION_LOOPBACK=1`), what the screen shows
/// has passed through the transmitter's frame path, so a bug that
/// reordered or dropped a universe on the way out would show on the
/// picture rather than only on a rig.
// r[impl dmx.loopback] - the sent frame, byte for byte, back into the receive path
pub struct LoopbackSink(pub DmxUniverses);

impl Sink for LoopbackSink {
    fn frame(&mut self, universe: u16, data: &[u8; 512]) {
        self.0.write_universe(universe, data);
    }
}

/// Whether the loopback sink is wanted: the flag, or the environment.
pub fn loopback_requested(flag: bool) -> bool {
    flag || std::env::var("IGNITION_LOOPBACK").is_ok_and(|v| v == "1")
}

/// Applies one of the transmit flags to `enabled`; `false` if `arg` is
/// not one of them. Kept as a function so the parse is testable away
/// from the binary's argument loop.
pub fn parse_output_flag(arg: &str, enabled: &mut bool) -> bool {
    match arg {
        "--output" => {
            *enabled = true;
            true
        }
        "--no-output" => {
            *enabled = false;
            true
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary(enabled: bool, protocols: &[&str], universes: usize, hz: f32) -> OutputSummary {
        OutputSummary {
            enabled,
            protocols: protocols.iter().map(|s| s.to_string()).collect(),
            universes,
            hz,
            error: None,
            lines: Vec::new(),
        }
    }

    /// r[verify dmx.output-toggle] - the line says sending, universes, rate, or the error
    #[test]
    fn the_overlay_line_says_what_is_happening() {
        assert_eq!(summary(true, &["sACN"], 4, 44.0).line(), "OUT sACN ×4 44Hz");
        assert_eq!(
            summary(true, &["sACN", "Art-Net"], 1, 43.6).line(),
            "OUT sACN+Art-Net ×1 44Hz"
        );
        assert_eq!(summary(false, &["sACN"], 4, 44.0).line(), "OUT off");
        let mut errored = summary(true, &["sACN"], 4, 0.0);
        errored.error = Some("bind 0.0.0.0:6454: address in use".into());
        assert_eq!(
            errored.line(),
            "OUT ERROR bind 0.0.0.0:6454: address in use"
        );
    }

    /// r[verify dmx.output-toggle] - `--output` turns sending on, `--no-output` off, default off
    #[test]
    fn output_flags_parse() {
        let mut on = false;
        assert!(parse_output_flag("--output", &mut on));
        assert!(on);
        assert!(parse_output_flag("--no-output", &mut on));
        assert!(!on);
        assert!(!parse_output_flag("--venue", &mut on));
        assert!(!on, "an unrelated flag leaves the default alone");
    }

    /// r[verify dmx.loopback] - the sink writes the sent frame into the universes
    #[test]
    fn the_loopback_sink_writes_the_frame_it_is_handed() {
        let dmx = DmxUniverses::new();
        let mut sink = LoopbackSink(dmx.clone());
        let mut frame = [0u8; 512];
        frame[9] = 200;
        sink.frame(3, &frame);
        assert_eq!(dmx.snapshot().get(&3).map(|u| u[9]), Some(200));
    }
}
