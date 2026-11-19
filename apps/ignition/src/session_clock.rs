//! The show following a session's transport: the playhead its host
//! publishes (presence `lead/play`, stamped in the host's clock), as a
//! beat of the Live set.
//!
//! `--clock session:<url>#<doc>@<song>`: `url` is the session host's vox
//! endpoint (`ws://host:4142/vox`), `doc` the session's id, `song` the
//! night's song the session is (its slug, or its number from 1). The
//! session's zero is the song's locator, as a split song's is for the
//! Studio clock ([`crate::studio_clock::set_beat`]).
//!
//! The renderer is a program the app runs, not the extension: it cannot
//! call the host's `fts.session`. The pane does that for it, as
//! `ignition` (`IGNITION_SESSION_ROOM`: the session's song read through
//! `fts.session`), and hands the room this clock. The transport itself is
//! presence, never the document, so this follows it directly, with the
//! host's clock estimated as every follower of Architect's presence does
//! (`PresencePeer::clock`: the presence driver pings the host's clock).

use crate::night::Night;
use crate::studio_clock::{set_beat, slugs};
use anyhow::{Context, Result, anyhow};
use crdt::sync::{DocPresenceClient, PresencePeer};
use ignition_daw::BeatSource;
use session::Transport;
use session::doc::transport_of;
use std::sync::Arc;
use std::time::Instant;

/// What `--clock session:…` names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionArg {
    pub url: String,
    pub doc: String,
    pub song: String,
}

impl SessionArg {
    /// `<url>#<doc>@<song>` (after `session:`).
    ///
    /// # Errors
    ///
    /// A part is missing.
    pub fn parse(value: &str) -> Result<Self> {
        let (rest, song) = value
            .rsplit_once('@')
            .ok_or_else(|| anyhow!("session:{value}: no @<song>"))?;
        let (url, doc) = rest
            .rsplit_once('#')
            .ok_or_else(|| anyhow!("session:{value}: no #<doc>"))?;
        if url.is_empty() || doc.is_empty() || song.is_empty() {
            return Err(anyhow!("session:{value}: use session:<url>#<doc>@<song>"));
        }
        Ok(Self {
            url: url.to_owned(),
            doc: doc.to_owned(),
            song: song.to_owned(),
        })
    }
}

/// The night's song `song` names: its slug, or its number from 1.
#[must_use]
pub fn song_index(slugs: &[&str], song: &str) -> Option<usize> {
    if let Ok(n) = song.parse::<usize>() {
        return n.checked_sub(1).filter(|i| *i < slugs.len());
    }
    slugs.iter().position(|s| *s == song)
}

/// Where the session's playhead is, in seconds of the session, and
/// whether it plays: `None` until the host's clock is estimated and the
/// leader has published a transport.
#[must_use]
pub fn session_seconds(
    leader: Option<Transport>,
    host_now_micros: Option<f64>,
) -> Option<(f64, bool)> {
    let (leader, now) = (leader?, host_now_micros?);
    Some((leader.at(now), leader.is_playing))
}

/// The session host's clock now, in microseconds, as `peer`'s driver
/// estimates it: `None` until the first exchanges have come back.
#[must_use]
pub fn host_now_micros(peer: &PresencePeer) -> Option<f64> {
    peer.clock().now_ms().map(|ms| ms * 1000.0)
}

/// How long to wait before dialling the session's host again, after
/// `failed` dials in a row came to nothing (0: it answered, then went):
/// a quarter second, doubling, at most five seconds, so a room restored
/// before its host is up follows it within seconds of it serving.
#[must_use]
pub fn redial_after(failed: u32) -> std::time::Duration {
    const FIRST_MS: u64 = 250;
    const MOST_MS: u64 = 5_000;
    let doubled = FIRST_MS.saturating_mul(1_u64 << failed.min(16));
    std::time::Duration::from_millis(doubled.min(MOST_MS))
}

/// The session's transport as the night's beat source.
pub struct SessionClock {
    night: Arc<Night>,
    song: usize,
    peer: PresencePeer,
}

impl SessionClock {
    /// Joins the session host's presence at `arg.url` (which carries its
    /// clock), on a thread of its own, and follows its leader.
    ///
    /// # Errors
    ///
    /// The song is not the night's, or the session's id is not one.
    pub fn spawn(night: Arc<Night>, arg: &SessionArg) -> Result<Self> {
        let song = song_index(&slugs(&night), &arg.song).with_context(|| {
            format!(
                "--clock session: {} is not a song of the night ({})",
                arg.song,
                slugs(&night).join(", ")
            )
        })?;
        let doc = uuid::Uuid::parse_str(&arg.doc).context("--clock session: the session id")?;
        let (peer, mut driver) = PresencePeer::new(doc, 30_000);
        let url = arg.url.clone();
        std::thread::Builder::new()
            .name("session-clock".into())
            .spawn(move || {
                let runtime = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(rt) => rt,
                    Err(error) => {
                        tracing::error!(%error, "session clock: no runtime");
                        return;
                    }
                };
                runtime.block_on(async move {
                    let pool = architect::connect::pool();
                    let endpoint = architect::connect::Endpoint::url(&url);
                    // The host may not be up yet (a window restoring the
                    // room before the session is served), or may go and
                    // come back (the engine restarting): dial until it
                    // answers, and again whenever it goes.
                    let mut failed: u32 = 0;
                    loop {
                        let dialled: Result<DocPresenceClient, String> =
                            pool.client(&endpoint).await.map_err(|e| e.to_string());
                        match dialled {
                            Ok(presence) => {
                                if failed > 0 {
                                    tracing::info!(session.url = %url, session.dials = failed.saturating_add(1),"session clock: host answered");
                                }
                                failed = 0;
                                // The driver pings the host's clock
                                // into `peer.clock()` as it runs.
                                let _ = driver.run(&presence).await;
                                // Connected, then gone: dial again (once
                                // past the backoff, so a refusing host is
                                // not spun on).
                                pool.evict(&endpoint);
                            }
                            Err(error) => {
                                if failed == 0 {
                                    tracing::warn!(%error, session.url = %url, "session clock: no host yet; dialling until it answers");
                                }
                                failed = failed.saturating_add(1);
                            }
                        }
                        tokio::time::sleep(redial_after(failed)).await;
                    }
                });
            })
            .context("session clock: its thread")?;
        tracing::info!(
            session.url = %arg.url,
            session.song = night.songs.get(song).map_or("", |s| s.title.as_str()),
            "session clock: following"
        );
        Ok(Self { night, song, peer })
    }

    fn now(&self) -> Option<(f64, bool)> {
        let leader = self.peer.states().get("lead/play").and_then(transport_of);
        session_seconds(leader, host_now_micros(&self.peer))
    }
}

impl BeatSource for SessionClock {
    fn beats(&self, _now: Instant) -> Option<f64> {
        let (seconds, _) = self.now()?;
        set_beat(&self.night, self.song, seconds)
    }

    fn is_playing(&self) -> bool {
        self.now().is_some_and(|(_, playing)| playing)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_session_clock_names_its_host_session_and_song() {
        assert_eq!(
            SessionArg::parse(
                "ws://192.168.1.10:4142/vox#187ff9d6-3db5-40b2-9795-fd16821169ec@drive-to-la"
            )
            .unwrap(),
            SessionArg {
                url: "ws://192.168.1.10:4142/vox".into(),
                doc: "187ff9d6-3db5-40b2-9795-fd16821169ec".into(),
                song: "drive-to-la".into(),
            }
        );
        assert!(SessionArg::parse("ws://h:1/vox#doc").is_err());
        assert!(SessionArg::parse("ws://h:1/vox@song").is_err());
    }

    #[test]
    fn a_host_not_up_yet_is_dialled_again_soon_then_every_few_seconds() {
        use std::time::Duration;
        // It answered, then went: dial again at once, nearly.
        assert_eq!(redial_after(0), Duration::from_millis(250));
        assert_eq!(redial_after(1), Duration::from_millis(500));
        assert_eq!(redial_after(2), Duration::from_millis(1_000));
        assert_eq!(redial_after(4), Duration::from_millis(4_000));
        // Never longer than five seconds, however long it has been down.
        assert_eq!(redial_after(5), Duration::from_secs(5));
        assert_eq!(redial_after(u32::MAX), Duration::from_secs(5));
    }

    #[test]
    fn a_song_is_its_slug_or_its_number() {
        let slugs = ["intro", "drive-to-la", "for-you"];
        assert_eq!(song_index(&slugs, "drive-to-la"), Some(1));
        assert_eq!(song_index(&slugs, "3"), Some(2));
        assert_eq!(song_index(&slugs, "0"), None);
        assert_eq!(song_index(&slugs, "4"), None);
        assert_eq!(song_index(&slugs, "nope"), None);
    }

    #[test]
    fn the_playhead_is_the_leaders_read_on_the_hosts_clock() {
        let playing = Transport {
            host_micros: 1_000_000.0,
            playhead_seconds: 10.0,
            playrate: 1.0,
            is_playing: true,
        };
        // Two seconds later on the host's clock: 12 s into the session.
        assert_eq!(
            session_seconds(Some(playing), Some(3_000_000.0)),
            Some((12.0, true))
        );
        // Stopped: where it stopped, whenever it is read.
        let stopped = Transport {
            is_playing: false,
            ..playing
        };
        assert_eq!(
            session_seconds(Some(stopped), Some(9_000_000.0)),
            Some((10.0, false))
        );
        // No clock estimate, or no leader yet: nothing to follow.
        assert_eq!(session_seconds(Some(playing), None), None);
        assert_eq!(session_seconds(None, Some(1.0)), None);
    }
}
