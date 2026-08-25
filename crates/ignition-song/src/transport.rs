//! Playing the project, and telling the lights where it is.
//!
//! The DAW backend is embedded rather than talked to over a wire: the
//! project *is* the show, and a console that has to ask another process
//! where the music got to has already added a hop that can drift. This
//! owns a `Standalone`, its cpal output stream, and the tempo map that
//! turns its playhead into bars.
//!
//! What it deliberately does **not** own is the cue player. This
//! reports a position; whether anything follows it is the caller's
//! business, which is what keeps "manually runnable" true — unplug the
//! transport and the same cue list still steps on GO.

use crate::SongMap;
use anyhow::{Context, Result};
use daw::standalone::media_bay::ProjectRelativeResolver;
use daw::standalone::project_loader::load_rpp_via_bay;
use daw::standalone::sync::Standalone;
use daw_proto::ProjectContext;
use daw_proto::transport::service::Transport as TransportService;
use ignition_core::Bars;
use std::path::{Path, PathBuf};

/// A loaded project, playing or ready to.
pub struct SongTransport {
    daw: Standalone,
    ctx: ProjectContext,
    /// The cpal output stream. Dropping it stops the audio, so it is
    /// held even though nothing calls it — hence the name rather than an
    /// `_engine` that reads like an oversight.
    _output: daw::standalone::audio_engine::AudioEngine,
    song: SongMap,
}

impl SongTransport {
    /// Loads a project, decodes its audio and opens an output stream.
    ///
    /// The song map comes from the same file in the same pass, so the
    /// tempo the lights convert with is by construction the tempo the
    /// audio is playing at.
    ///
    /// **Must be called with a Tokio runtime current.** The backend's
    /// service layer spawns tasks, and without one this panics inside
    /// architect rather than returning an error — so a caller that is
    /// not already async needs `rt.enter()` held for the lifetime of
    /// the transport, not just this call.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "project".to_string());
        let dir = path
            .parent()
            .map(PathBuf::from)
            .unwrap_or_else(|| ".".into());

        let daw = Standalone::new();
        // Source paths in a project are relative to it. Without this the
        // project loads, reports zero decoded sources, and plays
        // silence — which looks like an audio-device problem and is not.
        daw.media_bay()
            .set_file_resolver(Box::new(ProjectRelativeResolver::new(dir)));

        let (project, audio) =
            load_rpp_via_bay(&daw, &name, path.to_string_lossy().as_ref(), &text)
                .map_err(|e| anyhow::anyhow!("loading {}: {e}", path.display()))?;
        for (take, err) in &audio.failed {
            tracing::warn!(take, error = %err, "song: a source failed to decode");
        }
        tracing::info!(
            name = %name,
            tracks = project.track_count,
            items = project.item_count,
            decoded = audio.loaded,
            failed = audio.failed.len(),
            "song: project loaded"
        );

        let ctx = ProjectContext::Project(project.project_guid.clone());
        let output = daw
            .attach_audio_engine(&project.project_guid)
            .map_err(|e| anyhow::anyhow!("opening the audio output: {e}"))?;
        let song = crate::from_rpp(&text, &name)?;

        Ok(Self {
            daw,
            ctx,
            _output: output,
            song,
        })
    }

    pub fn song(&self) -> &SongMap {
        &self.song
    }

    pub fn play(&self) {
        if let Err(e) = TransportService::play(&self.daw, self.ctx.clone()) {
            tracing::warn!(error = ?e, "song: play failed");
        }
    }

    pub fn stop(&self) {
        if let Err(e) = TransportService::stop(&self.daw, self.ctx.clone()) {
            tracing::warn!(error = ?e, "song: stop failed");
        }
    }

    pub fn is_playing(&self) -> bool {
        use daw_proto::transport::PlayState;
        matches!(
            TransportService::get_play_state(&self.daw, self.ctx.clone()),
            PlayState::Playing | PlayState::Recording
        )
    }

    /// The playhead, in seconds.
    pub fn seconds(&self) -> f64 {
        TransportService::get_position(&self.daw, self.ctx.clone())
    }

    /// The playhead, in bars and beats.
    ///
    /// This is the whole point of the module: the lights are told
    /// *where in the song* the music is, not how many seconds have
    /// passed, so a tempo change or a section edit moves them with it.
    pub fn position(&self) -> Bars {
        self.song.tempo.position_at(self.seconds())
    }

    /// Moves the playhead to a musical position — how a section is
    /// looped, and how "start from the last chorus" works.
    pub fn locate(&self, position: Bars) {
        let seconds = self.song.tempo.seconds_at(position);
        if let Err(e) = TransportService::set_position(&self.daw, self.ctx.clone(), seconds) {
            tracing::warn!(error = ?e, "song: locate failed");
        }
    }

    /// Moves the playhead to the start of a named section.
    pub fn locate_section(&self, name: &str) -> bool {
        match self.song.section(name).map(|s| s.start) {
            Some(start) => {
                self.locate(start);
                true
            }
            None => false,
        }
    }
}
