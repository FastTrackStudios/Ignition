//! The return path: what the visualizer tells the surface.
//!
//! [`publish`] reports where the show actually is — read off the
//! **player**, not off what the sidebar last sent, because those agree
//! only until the song starts driving the cues. [`follow_song`] is the
//! other direction of the same relationship: the transport's position
//! moving the player. Camera persistence rides along here because it is
//! the same once-a-frame housekeeping.

use crate::command::{Playhead, StateTx};
use ignition_daw::SongTransport;
use ignition_viz::embedded::EmbeddedViz;
use ignition_viz::playback::Playback;

/// The clock a camera cut runs on: the song's, at the song's tempo.
pub(super) fn camera_clock(
    playbacks: &mut ignition_core::Playbacks,
    speeds: &ignition_core::SpeedMasters,
) -> (f32, f32) {
    let now = playbacks
        .of_class(ignition_core::Class::Song)
        .map_or(0.0, |p| p.clock());
    let bpm = speeds.get("Song").copied().unwrap_or(120.0);
    (now, bpm)
}

/// The venue's `cameras.json`, written after a pane edit.
// r[impl viz.camera-presets] - saved back to the venue
pub(super) fn save_cameras(cameras: &ignition_viz::Cameras) {
    let dir = std::path::PathBuf::from(crate::venue_dir());
    match cameras.save(&dir) {
        Ok(()) => {
            tracing::info!(path = %ignition_viz::Cameras::path(&dir).display(), "studio: cameras saved");
        }
        Err(error) => tracing::warn!(%error, "studio: cameras not saved"),
    }
}

/// Reports where the show actually is, for the UI to render.
///
/// The cue index comes from the **player**, not from what the sidebar
/// last sent. Those two agree only for as long as every change comes
/// from a click; once the song is driving the cues the sidebar's own
/// memory is stale, and it went on highlighting a cue the transport had
/// left several sections ago.
///
/// `send_if_modified` so an idle frame does not wake the UI. `Playhead`
/// is `PartialEq` for exactly this: seconds change constantly while
/// playing, so the UI does re-render then, but a stopped show settles.
/// A heartbeat in the log: what the song is doing and which cue is up,
/// once a second, so a "nothing happens" report can be read off the
/// file instead of reproduced.
fn log_heartbeat(transport: Option<&SongTransport>) {
    use std::sync::atomic::{AtomicU64, Ordering};
    static LAST: AtomicU64 = AtomicU64::new(0);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    if now != LAST.swap(now, Ordering::Relaxed) {
        if let Some(t) = transport {
            tracing::info!(
                playing = t.is_playing(),
                secs = t.seconds(),
                position = ?t.position(),
                "studio: transport"
            );
        } else {
            tracing::info!("studio: transport none");
        }
    }
}

/// The player and programmer's own state, read fresh every frame — see
/// the module doc comment on why the cue index comes from here rather
/// than from what the sidebar last sent.
fn playhead_from_playback(playback: Option<&Playback>) -> Playhead {
    let cue = playback
        .and_then(|p| p.song())
        .and_then(ignition_core::CuePlayer::current_index);
    let hit = playback.and_then(|p| p.triggers.last_fired_index());
    // What the player is *doing*, not only what the file says: how far
    // into its arrival the standing cue is, and whether the next one is
    // counting itself down. A cue list without these is a document.
    // r[impl studio.cuelist.live-state]
    let song = playback.and_then(|p| p.song());
    let mut next = Playhead {
        cue,
        hit,
        cue_fade: song.map_or(1.0, ignition_core::CuePlayer::fade_progress),
        next_cue: song.and_then(ignition_core::CuePlayer::next_cue),
        next_in: song.and_then(ignition_core::CuePlayer::next_in),
        ..Default::default()
    };
    let Some(p) = playback else {
        return next;
    };
    // The desk's own state, so the surface draws what the engine has
    // rather than what it last sent — a page turn from a MIDI key has
    // to move the strip on screen too.
    crate::live_commands::publish(&mut next, p);
    let prog = &p.programmer;
    next.page = prog.page;
    next.pages = prog.pages.len().max(1);
    for i in 0..ignition_core::FADERS {
        if let Some(slot) = next.latched.get_mut(i) {
            *slot = prog.is_latched(i);
        }
        if let Some(slot) = next.toggled.get_mut(i) {
            *slot = prog.is_toggled(i);
        }
    }
    next.blind = prog.blind;
    next.tap_bpm = prog
        .tap
        .bpm()
        .or_else(|| p.speeds.get("Tap").copied())
        .unwrap_or(0.0);
    next.tap_multiplier = prog.tap.multiplier;
    next.sound = [p.sound.low, p.sound.mid, p.sound.high];
    next.grand = prog.grand;
    next.parked = prog.parked.len();
    for i in 0..ignition_core::FADERS {
        if let (Some(slot), Some(fader)) = (next.levels.get_mut(i), prog.faders.get(i)) {
            *slot = fader.level;
        }
    }
    for entry in &p.playbacks.entries {
        let slot = match entry.class {
            ignition_core::Class::Song => 0,
            ignition_core::Class::Look => 1,
            _ => continue,
        };
        if let Some(m) = next.playback_masters.get_mut(slot) {
            *m = entry.master;
        }
        if entry.class == ignition_core::Class::Song {
            next.paused = entry.player.is_paused();
        }
    }
    next
}

/// The programme camera, so the pane lights the right tile and can save
/// the view the viewport is on.
// r[impl studio.video.cameras-pane] - the current view comes back on the playhead
fn camera_state(
    viz: &mut EmbeddedViz,
    song_clock: f32,
) -> Option<ignition_live_ui::command::CameraState> {
    let active = viz
        .app_mut()
        .world()
        .get_resource::<ignition_viz::ActiveCamera>()?;
    let state = active.state_at(song_clock);
    // Tenths: the pane does not need to re-render on a sub-millimetre
    // change mid-dissolve, and the playhead is compared for equality.
    let round = |v: bevy::math::Vec3| {
        [
            (v.x * 100.0).round() / 100.0,
            (v.y * 100.0).round() / 100.0,
            (v.z * 100.0).round() / 100.0,
        ]
    };
    Some(ignition_live_ui::command::CameraState {
        preset: active.preset.clone(),
        eye: round(state.eye),
        look: round(state.look),
        fov_deg: (state.fov_deg * 10.0).round() / 10.0,
        presets: active
            .cameras
            .presets
            .iter()
            .map(|p| p.name.clone())
            .collect(),
        slots: active.slots(),
        wide: active.wide_name(),
        canvases: Vec::new(),
    })
}

/// The canvases and what each shows, for TO SCREENS.
fn canvas_rows(viz: &mut EmbeddedViz) -> Vec<ignition_live_ui::command::CanvasRow> {
    let world = viz.app_mut().world_mut();
    let switched: std::collections::HashMap<String, Option<String>> = world
        .get_resource::<ignition_viz::camera::CanvasSwitches>()
        .map(|s| {
            s.current
                .iter()
                .map(|(k, v)| {
                    (
                        k.clone(),
                        v.as_ref().map(ignition_viz::camera::CameraSource::content),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    let mut names: Vec<String> = world
        .query::<&ignition_viz::camera::CanvasPanel>()
        .iter(world)
        .map(|p| p.canvas.clone())
        .collect();
    names.sort();
    names.dedup();
    names
        .into_iter()
        .map(|name| ignition_live_ui::command::CanvasRow {
            camera: switched.get(&name).cloned().flatten(),
            name,
        })
        .collect()
}

pub(super) fn publish(state: &StateTx, transport: Option<&SongTransport>, viz: &mut EmbeddedViz) {
    log_heartbeat(transport);
    let playback = viz.app_mut().world().get_resource::<Playback>();
    let mut next = playhead_from_playback(playback);
    let song_clock = playback
        .and_then(|p| p.song())
        .map_or(0.0, ignition_core::CuePlayer::clock);
    if let Some(t) = transport {
        next.secs = crate::num::f32_of_f64(t.seconds());
        next.length = crate::num::f32_of_f64(t.length());
        next.playing = t.is_playing();
    }
    if let Some(output) = viz
        .app_mut()
        .world()
        .get_resource::<ignition_viz::DmxOutput>()
    {
        next.output = output.summary();
    }
    next.camera = camera_state(viz, song_clock);
    if let Some(camera) = next.camera.as_mut() {
        camera.canvases = canvas_rows(viz);
    }
    state.send_if_modified(|current| {
        if *current == next {
            false
        } else {
            *current = next;
            true
        }
    });
}

/// Points the cue player at wherever the song is.
///
/// Every frame, and cheap when nothing moved — `seek` returns
/// immediately unless the position implies a different cue. The song's
/// tempo also drives the `Song` speed master, so a chase written as
/// "one cycle per bar" is one cycle per bar of *this* song rather than
/// a rate somebody dialled in.
pub(super) fn follow_song(transport: Option<&SongTransport>, viz: &mut EmbeddedViz) {
    use ignition_core::Show;
    let Some(transport) = transport else { return };
    let position = transport.position();
    let bpm = crate::num::f32_of_f64(transport.song().tempo.at(position).bpm);
    let secs = crate::num::f32_of_f64(transport.seconds());

    let world = viz.app_mut().world_mut();
    // The screens scrub with the song too: a clip's frame is a function
    // of the transport, never of its own wall clock.
    world.insert_resource(ignition_viz::CanvasClock::at(f64::from(secs)));
    let Some(mut playback) = world.remove_resource::<Playback>() else {
        return;
    };
    {
        let Playback {
            playbacks,
            groups,
            rig,
            palettes,
            speeds,
            profile,
            triggers,
            ..
        } = &mut playback;
        speeds.insert("Song".to_string(), bpm);
        // The same transport, the same frame, so a section cue and the
        // hit on its downbeat land together. A backwards move is a
        // locate inside `advance`; a stopped playhead fires nothing.
        // r[impl triggers.wired]
        // r[impl song.transport.position-per-frame]
        triggers.advance(position, secs);
        if let Some(player) = playbacks.of_class(ignition_core::Class::Song) {
            let show = Show {
                groups,
                palettes,
                rig,
                speeds,
                roles: profile,
                ..Show::new(groups, rig)
            };
            // The song *is* the clock while a transport is loaded. Left
            // free-running, effects keep their rate but lose their
            // phase — a pulse written on two and four lands wherever the
            // app happened to start — and they go on running after the
            // song stops, which is not what "synced to the music" means.
            player.set_clock(secs);
            player.seek(position, &show);
        }
    }
    viz.app_mut().world_mut().insert_resource(playback);
}
