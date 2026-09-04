//! The page's end of the desk.
//!
//! `ignition-live-ui` splits the VIEW from its transport — a component
//! never owns a channel, it calls `send`, and the host decides what that
//! is wired to. `ignition_viz::WebDesk` splits the ENGINE from the same
//! transport. This is the piece in the middle, and it is the reason the
//! demo can use the desk's own components rather than lookalikes.
//!
//! On a desk the two ends are a process apart (an mpsc into the Blitz
//! widget, a watch back out) and on an iPad they are a WebSocket apart.
//! Here they are neither: the engine and the surface are the same wasm
//! module, so the transport is two mutexes and the "network" is a
//! function call. Everything above and below is unchanged, which is the
//! whole point of putting the seam there.
//!
//! What is wired so far is the cue list — GO, and clicking a cue — plus
//! the transport, and the playhead that makes the list an instrument
//! rather than a document. The rest of `Command` is the studio's
//! `viz_widget::commands::drain`, 735 lines that reach for a
//! `SongTransport` and a `MacroRunner` this page does not have; moving
//! that into a place both hosts share is the next piece of work, not a
//! copy to make here.

use std::sync::{Arc, Mutex, PoisonError};

use ignition_live_ui::command::{Command, Playhead};

/// The queue in and the slot out.
///
/// Both halves are only ever touched from the browser: the SSG
/// pre-render builds the page's shell and never launches an engine.
///
/// A `Mutex` rather than a `RefCell` because Bevy wants its resources
/// `Send + Sync`; a browser is one thread, so it is never contended and
/// a poisoned lock could only mean a prior panic on this same mutex.
#[derive(Default)]
pub struct Desk {
    /// What the surface has asked for since the last frame.
    pending: Mutex<Vec<Command>>,
    // `playhead` below is written only by the engine, which exists only
    // in the browser — so the SSG pre-render sees a field nothing reads.
    /// What the show is doing, as of the last frame. Written by the
    /// engine, which exists only in the browser.
    playhead: Mutex<Playhead>,
}

#[cfg_attr(
    not(target_arch = "wasm32"),
    expect(dead_code, reason = "only the browser runs an engine to talk to")
)]
impl Desk {
    /// Queue a command from the surface. Called by `live-ui`'s `send`.
    pub fn push(&self, command: Command) {
        self.pending
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(command);
    }

    /// The latest playhead, for the surface to draw.
    #[must_use]
    pub fn playhead(&self) -> Playhead {
        self.playhead
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

#[cfg(target_arch = "wasm32")]
impl ignition_viz::WebDesk for Desk {
    fn drain(&self, playback: &mut ignition_viz::playback::Playback) {
        use ignition_core::{Class, Show};
        use ignition_viz::playback::Playback;

        let queued: Vec<Command> =
            std::mem::take(&mut *self.pending.lock().unwrap_or_else(PoisonError::into_inner));
        if queued.is_empty() {
            return;
        }
        // Destructured once, exactly as the studio's own drain is and
        // for the same reason: `Show` borrows the caches immutably while
        // every arm wants `playbacks` mutably, and the borrow checker
        // will only allow that if it can see they are different fields.
        let Playback {
            playbacks,
            groups,
            rig,
            palettes,
            profile,
            speeds,
            ..
        } = playback;
        let show = Show {
            groups,
            palettes,
            rig,
            speeds,
            roles: profile,
            ..Show::new(groups, rig)
        };
        for command in queued {
            match command {
                Command::Go => {
                    if let Some(player) = playbacks.of_class(Class::Song) {
                        player.go(&show);
                    }
                }
                Command::Cue(index) => {
                    if let Some(player) = playbacks.of_class(Class::Song) {
                        // To the END of the cue's fade, not the start of
                        // it: with no transport to locate, this is a
                        // desk stepping to a cue rather than a song
                        // arriving at one.
                        player.jump_to_end_of(index, &show);
                    }
                }
                // Everything else needs the drain that is still in the
                // studio — see the module doc comment. Dropped rather
                // than half-applied: a command that does a third of what
                // it says is worse than one that visibly does nothing.
                //
                // `Play`/`Stop` are in that set for a different reason:
                // they move a `SongTransport`, and a page has no DAW to
                // move. The show clock here is the frame clock.
                _ => {}
            }
        }
    }

    fn publish(&self, playback: &ignition_viz::playback::Playback) {
        let song = playback.song();
        let next = Playhead {
            cue: song.and_then(ignition_core::CuePlayer::current_index),
            // What the player is DOING, not only what the file says: how
            // far into its arrival the standing cue is, and whether the
            // next one is counting itself down. A cue list without these
            // is a document.
            // r[impl studio.cuelist.live-state]
            cue_fade: song.map_or(1.0, ignition_core::CuePlayer::fade_progress),
            next_cue: song.and_then(ignition_core::CuePlayer::next_cue),
            next_in: song.and_then(ignition_core::CuePlayer::next_in),
            hit: playback.triggers.last_fired_index(),
            ..Default::default()
        };
        *self.playhead.lock().unwrap_or_else(PoisonError::into_inner) = next;
    }
}

/// The one desk this page has.
///
/// A static because `live-ui`'s `install` takes a `'static` bridge and
/// the Bevy app takes an `Arc` of the same object — they must be the
/// same desk, or the surface would be talking to something the engine
/// never reads.
pub fn desk() -> Arc<Desk> {
    static DESK: std::sync::OnceLock<Arc<Desk>> = std::sync::OnceLock::new();
    Arc::clone(DESK.get_or_init(|| Arc::new(Desk::default())))
}
