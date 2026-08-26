//! The visualizer, as a Blitz widget.
//!
//! Blitz creates the wgpu device for its own Vello renderer; this hands
//! that same device to Bevy (`EmbeddedViz`), renders the venue into a
//! texture on it, and registers that texture as an anyrender resource
//! so Blitz can paint it into a DOM element. One device, one texture, no
//! copy — the 3D view is a `<div>` as far as the rest of the UI is
//! concerned, and HTML can sit above or below it.

use crate::command::{Command, PageMove, Playhead, Receiver, StateTx};
use anyrender::{PaintRef, PaintScene, RenderContext, ResourceId, Scene};
use blitz_dom::Widget;
use blitz_dom::node::ComputedStyles;
use dioxus_native::DeviceHandle;
use ignition_core::preset::Ref;
use ignition_song::SongTransport;
use ignition_viz::VizConfig;
use ignition_viz::embedded::{EmbeddedViz, HostGpu};
use ignition_viz::playback::Playback;
use peniko::kurbo::{Affine, Rect};
use peniko::{Fill, ImageBrush, ImageSampler};
use std::sync::{Arc, LazyLock};
use tokio::sync::Notify;

/// Fires once per painted frame, the moment the visualizer has stepped.
///
/// Blitz redraws only when the DOM changes, and the presentation is
/// vsync-blocking FIFO, so a redraw that is asked for on a *timer* is
/// asked for late: the timer starts after present returns, and by the
/// time it fires the next vblank has gone. Every frame missed its slot
/// and the studio ran at exactly half the refresh rate. Waking the
/// ticker from here instead means the next DOM write — and the redraw
/// request it carries — is already queued while this frame presents.
pub static FRAME_DONE: LazyLock<Arc<Notify>> = LazyLock::new(|| Arc::new(Notify::new()));

/// Everything needed to build the visualizer once a device shows up.
///
/// The widget is constructed by Dioxus during render, but Blitz does not
/// hand out a device until `can_create_surfaces` — so the config waits
/// here and the Bevy app is built at that moment, not before.
pub struct VizWidget {
    commands: Receiver,
    pending: Option<Box<VizConfig>>,
    /// The cue to sit on once the visualizer exists. Loading the show
    /// needs the venue, which lives inside `pending` until then.
    show: Option<(String, usize)>,
    state: State,
    /// The texture handed to Blitz last frame, and its size. Re-used
    /// across frames: registering a new resource every frame would leak
    /// one per frame.
    registered: Option<(ResourceId, u32, u32)>,
    /// The song, if one was opened. Lives here rather than in a
    /// component because the position has to be read on the same frame
    /// the visualizer renders — a transport polled from the UI would be
    /// a frame behind, and a frame late on a downbeat is visible.
    transport: Option<SongTransport>,
    /// Where the show is, published back to the UI every frame.
    report: StateTx,
    /// The sound-in's levels, raw as they arrived and smoothed by the
    /// sound fade — see [`SoundFade`].
    sound: SoundFade,
}

/// The sound fade: a one-pole smoother over the band levels, with the
/// time constant the operator sets.
///
/// Here rather than in the engine because `Show::sound` is defined as
/// *already smoothed* — a recipe stays a pure function of what it is
/// handed, and the same recipe on the same levels is the same value
/// everywhere. The host owns the time.
// r[impl playback.sound-as-value] - the sound fade, smoothing the levels the recipes read
#[derive(Debug, Clone, PartialEq)]
pub struct SoundFade {
    /// Seconds to settle, 0–2. Zero passes the raw meter through.
    pub secs: f32,
    /// What the input last reported.
    pub raw: [f32; 3],
    /// What the recipes read.
    pub smoothed: [f32; 3],
    last_step: Option<std::time::Instant>,
}

impl Default for SoundFade {
    fn default() -> Self {
        Self {
            secs: 0.25,
            raw: [0.0; 3],
            smoothed: [0.0; 3],
            last_step: None,
        }
    }
}

impl SoundFade {
    /// The longest fade the slider offers.
    pub const MAX_SECS: f32 = 2.0;

    /// Advances the smoothing by `dt` seconds. A fade of zero snaps.
    /// Exponential rather than linear so a kick reads as a lift with a
    /// tail, which is what "fade" means at a desk.
    pub fn step(&mut self, dt: f32) -> [f32; 3] {
        let k = if self.secs <= 0.0 || dt <= 0.0 {
            1.0
        } else {
            (1.0 - (-dt / self.secs).exp()).clamp(0.0, 1.0)
        };
        for (s, r) in self.smoothed.iter_mut().zip(self.raw) {
            *s += k * (r - *s);
        }
        self.smoothed
    }

    /// Steps by the wall clock since the last call.
    fn tick(&mut self) -> [f32; 3] {
        let now = std::time::Instant::now();
        let dt = self
            .last_step
            .map(|t| now.duration_since(t).as_secs_f32())
            .unwrap_or(0.0);
        self.last_step = Some(now);
        self.step(dt)
    }
}

enum State {
    Waiting,
    /// Device in hand, waiting for `paint` to say how big the viewport
    /// actually is.
    ///
    /// Deferring construction to the first paint is not fussiness: Bevy
    /// allocates its render target when the app is built, and swapping
    /// that target afterwards leaves the new one un-extracted — the
    /// viewport stays black with nothing logged anywhere. Building once,
    /// at the size the layout actually produced, avoids the resize
    /// entirely.
    Ready(Box<HostGpu>),
    Active(Box<EmbeddedViz>),
    /// A non-wgpu Blitz backend, or a device we could not build on. The
    /// UI still works; the viewport is empty.
    Unavailable,
}

impl VizWidget {
    /// Builds the visualizer at the viewport's real size.
    fn activate(&mut self, width: u32, height: u32) {
        let State::Ready(gpu) = std::mem::replace(&mut self.state, State::Waiting) else {
            return;
        };
        let Some(mut config) = self.pending.take() else {
            self.state = State::Unavailable;
            return;
        };
        config.width = width;
        config.height = height;

        // Loaded through `load` even with no show, so the look list the
        // operator GOes through exists either way.
        let show = self.show.as_ref();
        // The transport's song map resolves the show's relative
        // positions on load, so the cues land on *this* arrangement.
        let playback = Playback::load(
            &config.venue,
            None,
            show.map(|(path, _)| std::path::Path::new(path)),
            show.map(|(_, cue)| *cue),
            None,
            None,
            None,
            self.transport.as_ref().map(|t| t.song()),
        )
        .unwrap_or_default();
        let viz = EmbeddedViz::new(*config, Default::default(), playback, None, *gpu);
        tracing::info!(width, height, "viz.embed: built on the host device");
        self.state = State::Active(Box::new(viz));
    }

    pub fn new(
        config: VizConfig,
        show: Option<(String, usize)>,
        project: Option<&str>,
        commands: Receiver,
        report: StateTx,
    ) -> Self {
        // A project that will not open is not fatal — the surface still
        // busks and the cue list still steps on GO, which is the point
        // of keeping the transport optional.
        let transport = project.and_then(|path| match SongTransport::open(path) {
            Ok(t) => Some(t),
            Err(e) => {
                tracing::warn!(path, error = %e, "studio: no song loaded");
                None
            }
        });
        Self {
            commands,
            pending: Some(Box::new(config)),
            show,
            state: State::Waiting,
            registered: None,
            transport,
            report,
            sound: SoundFade::default(),
        }
    }
}

impl Widget for VizWidget {
    fn connected(&mut self) {}
    fn disconnected(&mut self) {}

    fn can_create_surfaces(&mut self, render_ctx: &mut dyn RenderContext) {
        if matches!(self.state, State::Active(_) | State::Ready(_)) {
            return;
        }
        let Some(context) = render_ctx.renderer_specific_context() else {
            tracing::warn!("viz.embed: rendering backend returned no context");
            self.state = State::Unavailable;
            return;
        };
        let Ok(handle) = context.downcast::<DeviceHandle>() else {
            // Blitz is running on a CPU backend; there is no device to
            // share and nothing to embed into.
            tracing::warn!("viz.embed: backend is not wgpu; no device to share");
            self.state = State::Unavailable;
            return;
        };
        let Some(config) = self.pending.take() else {
            self.state = State::Unavailable;
            return;
        };

        self.pending = Some(config);
        self.state = State::Ready(Box::new(HostGpu {
            instance: handle.instance.clone(),
            adapter: handle.adapter.clone(),
            device: handle.device.clone(),
            queue: handle.queue.clone(),
        }));
        tracing::info!("viz.embed: host device acquired");
    }

    fn destroy_surfaces(&mut self) {
        self.state = State::Waiting;
        self.registered = None;
    }

    fn handle_event(&mut self, _event: &blitz_traits::events::UiEvent) {}

    fn paint(
        &mut self,
        render_ctx: &mut dyn RenderContext,
        _styles: &ComputedStyles,
        width: u32,
        height: u32,
        _scale: f64,
    ) -> Scene {
        let mut scene = Scene::new();
        if matches!(self.state, State::Ready(_)) && width > 0 && height > 0 {
            self.activate(width, height);
        }
        let State::Active(viz) = &mut self.state else {
            tracing::debug!(width, height, "viz.embed: paint with no active viz");
            return scene;
        };
        drain(
            &self.commands,
            viz,
            self.transport.as_ref(),
            &mut self.sound,
        );
        smooth_sound(&mut self.sound, viz);
        follow_song(self.transport.as_ref(), viz);
        publish(&self.report, self.transport.as_ref(), viz);
        let rendered = viz.render(width, height);
        FRAME_DONE.notify_one();
        let Some(texture) = rendered else {
            tracing::debug!(width, height, "viz.embed: no target texture yet");
            return scene;
        };
        tracing::debug!(width, height, "viz.embed: painted");

        // Re-register only when the texture changes identity, which at
        // a steady size it does not. Registering every frame would leak
        // one resource per frame.
        let resource = match self.registered {
            Some((id, w, h)) if (w, h) == (width, height) => id,
            other => {
                if let Some((old, _, _)) = other {
                    render_ctx.unregister_resource(old);
                }
                match render_ctx.try_register_custom_resource(Box::new(texture)) {
                    Ok(id) => {
                        self.registered = Some((id, width, height));
                        id
                    }
                    Err(_) => {
                        self.registered = None;
                        return scene;
                    }
                }
            }
        };

        scene.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            PaintRef::Resource(ImageBrush {
                image: resource,
                sampler: ImageSampler::default(),
            }),
            None,
            &Rect::from_origin_size((0.0, 0.0), (width as f64, height as f64)),
        );
        scene
    }
}

/// Applies everything the UI has said since the last frame.
///
/// Drained rather than blocked on: a dropped frame's worth of messages
/// is better than a stalled frame, and the sender will send again.
fn drain(
    commands: &Receiver,
    viz: &mut EmbeddedViz,
    transport: Option<&SongTransport>,
    sound_fade: &mut SoundFade,
) {
    use ignition_core::{Class, RecipeApply, Show};

    let world = viz.app_mut().world_mut();
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
            programmer,
            profile,
            ..
        } = &mut playback;
        while let Ok(command) = commands.try_recv() {
            // Rebuilt per command because `Rate` mutates `speeds`, which
            // the `Show` borrows. Cheap — it is four references.
            match command {
                Command::Select(selection) => programmer.select(selection),
                Command::Deselect => programmer.deselect(),
                Command::ClearValues => programmer.clear_values(),
                Command::Level(index, level) => programmer.set_level(index, level),
                Command::Fader(index, fader) => programmer.set_fader(index, *fader),
                Command::FaderOnPage { page, index, fader } => {
                    while programmer.pages.len() <= page {
                        programmer.add_page();
                    }
                    if page == programmer.page {
                        programmer.set_fader(index, *fader);
                    } else if let Some(slot) = programmer.pages[page].get_mut(index) {
                        *slot = *fader;
                    }
                }
                Command::Key {
                    index,
                    action,
                    down,
                } => {
                    if down {
                        // A transport key lands on a playback, which the
                        // programmer cannot reach; it hands the request back.
                        // r[impl playback.temp-and-pause] - pause, go back, load
                        if let Some(request) = programmer.key_down(index, action) {
                            let show = Show {
                                groups,
                                palettes,
                                rig,
                                speeds,
                                roles: profile,
                                ..Show::new(groups, rig)
                            };
                            playbacks.transport(request, &show);
                        }
                    } else {
                        programmer.key_up(index);
                    }
                }
                Command::Page(PageMove::Next) => programmer.next_page(),
                Command::Page(PageMove::Prev) => programmer.prev_page(),
                Command::Page(PageMove::Set(page)) => programmer.set_page(page),
                Command::ProgramTime(beats) => programmer.program_time_beats = beats.max(0.0),
                Command::Blind(on) => programmer.blind = on,
                Command::Highlight(on) => programmer.highlight = on,
                Command::Lowlight(on) => programmer.lowlight = on,
                Command::Tap(bpm) => {
                    if bpm.is_finite() && bpm > 0.0 {
                        speeds.insert("Tap".to_string(), bpm);
                    }
                }
                // Raw levels land on the fade, not the engine: what the
                // engine reads is written by `smooth_sound` every frame.
                Command::SoundLevels { low, mid, high } => {
                    sound_fade.raw = [low, mid, high];
                }
                Command::SoundFade(secs) => {
                    sound_fade.secs = secs.clamp(0.0, SoundFade::MAX_SECS);
                }
                // r[impl playback.grand-master]
                Command::Grand(level) => programmer.set_grand(level),
                // r[impl playback.playback-master]
                Command::PlaybackMaster(class, level) => {
                    for entry in playbacks.entries.iter_mut().filter(|e| e.class == class) {
                        entry.master = level.clamp(0.0, 1.0);
                    }
                }
                // r[impl playback.park] - at the programmer's held value per fixture
                Command::Park { selection, attrs } => {
                    let held: std::collections::HashMap<_, _> = programmer
                        .captured()
                        .into_iter()
                        .map(|v| ((v.chan, v.attr), v.value))
                        .collect();
                    let chans = ignition_core::selection::resolve(&selection, groups, rig);
                    let mut parked = 0usize;
                    for chan in chans {
                        for attr in &attrs {
                            if let Some(value) = held.get(&(chan, attr.clone())) {
                                programmer.park_chan(chan, attr.clone(), *value);
                                parked += 1;
                            }
                        }
                    }
                    tracing::info!(parked, "studio: parked");
                }
                Command::Unpark { selection, attrs } => {
                    for chan in ignition_core::selection::resolve(&selection, groups, rig) {
                        for attr in &attrs {
                            programmer.unpark_chan(chan, attr);
                        }
                    }
                }
                // r[impl playback.speed-keys]
                Command::Speed(key) => {
                    programmer.key_down(0, key.action());
                }
                Command::Rate(bpm) => {
                    speeds.insert("Rate".to_string(), bpm);
                }
                Command::Size(v) => programmer.size = v.clamp(0.0, 1.0),
                Command::EffectRate(v) => programmer.rate = v.max(0.0),
                Command::Master(role, level) => programmer.set_master(&role, level),
                Command::Solo(role) => match role {
                    Some(role) => programmer.solo(&role),
                    None => programmer.clear_solo(),
                },
                Command::Hold(Some(recipe)) => programmer.hold(*recipe),
                Command::Hold(None) => programmer.release_hold(),
                Command::Flash(target, kind) => {
                    // Fired against the player's clock, which is the song
                    // while a transport is loaded — so a hand-played
                    // flash and a charted one are timed by the same
                    // thing.
                    let now = playbacks
                        .of_class(Class::Song)
                        .map(|c| c.clock())
                        .unwrap_or_default();
                    programmer.flash(target, kind, now);
                }
                Command::Color(name) => {
                    let show = Show {
                        groups,
                        palettes,
                        rig,
                        speeds,
                        roles: profile,
                        ..Show::new(groups, rig)
                    };
                    programmer.apply(RecipeApply::Color(Ref::Named(name)), &show);
                }
                Command::Split(name) => {
                    let show = Show {
                        groups,
                        palettes,
                        rig,
                        speeds,
                        roles: profile,
                        ..Show::new(groups, rig)
                    };
                    programmer.apply(RecipeApply::Split(Ref::Named(name)), &show);
                }
                Command::Focus(name) => {
                    let show = Show {
                        groups,
                        palettes,
                        rig,
                        speeds,
                        roles: profile,
                        ..Show::new(groups, rig)
                    };
                    programmer.apply(RecipeApply::FocusPoint(Ref::Named(name)), &show);
                }
                Command::Dimmer(level) => {
                    let show = Show {
                        groups,
                        palettes,
                        rig,
                        speeds,
                        roles: profile,
                        ..Show::new(groups, rig)
                    };
                    programmer.apply(RecipeApply::Dimmer(level), &show);
                }
                Command::Release => {
                    let show = Show {
                        groups,
                        palettes,
                        rig,
                        speeds,
                        roles: profile,
                        ..Show::new(groups, rig)
                    };
                    programmer.release(&show);
                }
                Command::Go => {
                    if let Some(player) = playbacks.of_class(Class::Song) {
                        let show = Show {
                            groups,
                            palettes,
                            rig,
                            speeds,
                            roles: profile,
                            ..Show::new(groups, rig)
                        };
                        player.go(&show);
                    }
                }
                Command::LookGo => {
                    if let Some(player) = playbacks.of_class(Class::Look) {
                        let show = Show {
                            groups,
                            palettes,
                            rig,
                            speeds,
                            roles: profile,
                            ..Show::new(groups, rig)
                        };
                        player.go(&show);
                    }
                }
                Command::Cue(index) => {
                    if let Some(player) = playbacks.of_class(Class::Song) {
                        let show = Show {
                            groups,
                            palettes,
                            rig,
                            speeds,
                            roles: profile,
                            ..Show::new(groups, rig)
                        };
                        // Take the song to the cue's own position, not
                        // to a section with the cue's name. Every cue is
                        // written at a musical position, so every cue is
                        // seekable — which section names only managed
                        // for the nineteen cues that were sections, and
                        // the accents, being called things like
                        // "· fig 0 · 1/3", simply failed.
                        if let Some(at) = player.cues().get(index).and_then(|c| c.position())
                            && let Some(transport) = transport
                        {
                            transport.locate(at);
                        }
                        player.jump_to_end_of(index, &show);
                    }
                }
                // Transport. Nothing here touches the cue player: the
                // song moves, and `follow_song` notices on the next
                // frame. Keeping the two apart is what lets the same
                // list run with no transport at all.
                Command::Play => {
                    if let Some(transport) = transport {
                        transport.play();
                    }
                }
                Command::Stop => {
                    if let Some(transport) = transport {
                        transport.stop();
                    }
                }
                Command::Section(name) => {
                    if let Some(transport) = transport
                        && !transport.locate_section(&name)
                    {
                        tracing::warn!(name, "studio: no such section");
                    }
                }
                Command::Scrub(fraction) => {
                    if let Some(transport) = transport {
                        transport.scrub(fraction);
                    }
                }
                Command::Locate(position) => {
                    if let Some(transport) = transport {
                        transport.locate(position);
                    }
                }
            }
        }
    }
    viz.app_mut().world_mut().insert_resource(playback);
}

/// Writes the smoothed band levels into the engine for this frame.
///
/// Every frame, even when nothing arrived: the fade is what makes a
/// kick decay rather than vanish, and the decay happens between inputs.
// r[impl playback.sound-as-value] - `Show.sound` is written every frame
fn smooth_sound(fade: &mut SoundFade, viz: &mut EmbeddedViz) {
    let [low, mid, high] = fade.tick();
    let world = viz.app_mut().world_mut();
    if let Some(mut playback) = world.get_resource_mut::<Playback>() {
        playback.sound = ignition_viz::playback::SoundLevels { low, mid, high };
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
fn publish(state: &StateTx, transport: Option<&SongTransport>, viz: &mut EmbeddedViz) {
    let playback = viz.app_mut().world().get_resource::<Playback>();
    let cue = playback
        .and_then(|p| p.song())
        .and_then(|player| player.current_index());
    let hit = playback.and_then(|p| p.triggers.last_fired_index());
    // The desk's own state, so the surface draws what the engine has
    // rather than what it last sent — a page turn from a MIDI key has
    // to move the strip on screen too.
    let mut next = Playhead {
        cue,
        hit,
        ..Default::default()
    };
    if let Some(p) = playback {
        let prog = &p.programmer;
        next.page = prog.page;
        next.pages = prog.pages.len().max(1);
        for i in 0..ignition_core::FADERS {
            next.latched[i] = prog.is_latched(i);
            next.toggled[i] = prog.is_toggled(i);
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
            next.levels[i] = prog.faders[i].level;
        }
        for entry in &p.playbacks.entries {
            let slot = match entry.class {
                ignition_core::Class::Song => 0,
                ignition_core::Class::Look => 1,
                _ => continue,
            };
            next.playback_masters[slot] = entry.master;
            if entry.class == ignition_core::Class::Song {
                next.paused = entry.player.is_paused();
            }
        }
    }
    if let Some(t) = transport {
        next.secs = t.seconds() as f32;
        next.length = t.length() as f32;
        next.playing = t.is_playing();
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
fn follow_song(transport: Option<&SongTransport>, viz: &mut EmbeddedViz) {
    use ignition_core::Show;
    let Some(transport) = transport else { return };
    let position = transport.position();
    let bpm = transport.song().tempo.at(position).bpm as f32;
    let secs = transport.seconds() as f32;

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

#[cfg(test)]
mod sound_fade_tests {
    use super::SoundFade;

    /// r[verify playback.sound-as-value]
    #[test]
    fn a_zero_fade_snaps_and_a_long_one_lags() {
        let mut snap = SoundFade {
            secs: 0.0,
            raw: [1.0, 0.5, 0.0],
            ..Default::default()
        };
        assert_eq!(snap.step(0.016), [1.0, 0.5, 0.0]);
        let mut slow = SoundFade {
            secs: 1.0,
            raw: [1.0, 0.0, 0.0],
            ..Default::default()
        };
        let first = slow.step(0.1)[0];
        assert!(first > 0.0 && first < 0.2, "{first}");
        // Approaches the raw level and never overshoots.
        let mut last = first;
        for _ in 0..100 {
            let next = slow.step(0.1)[0];
            assert!(next >= last && next <= 1.0);
            last = next;
        }
        assert!(last > 0.99, "{last}");
        // And decays when the input stops.
        slow.raw = [0.0; 3];
        assert!(slow.step(0.5)[0] < last);
    }
}
