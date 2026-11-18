//! The studio's end of the wire.
//!
//! `Command` and `Playhead` themselves live in `ignition-live-ui`, so
//! the same types reach the browser; what is here is the desktop's
//! transport for them — an mpsc into the Blitz widget and a tokio watch
//! out of it. Everything in this crate keeps saying `crate::command::`.

pub use ignition_live_ui::command::{Command, OverlayKind, PageMove, Playhead, SpeedKey};

pub type Sender = std::sync::mpsc::Sender<Command>;
pub type Receiver = std::sync::mpsc::Receiver<Command>;

pub fn channel() -> (Sender, Receiver) {
    std::sync::mpsc::channel()
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
