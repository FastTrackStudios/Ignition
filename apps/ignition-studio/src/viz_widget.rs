//! The visualizer, as a Blitz widget.
//!
//! Blitz creates the wgpu device for its own Vello renderer; this hands
//! that same device to Bevy (`EmbeddedViz`), renders the venue into a
//! texture on it, and registers that texture as an anyrender resource
//! so Blitz can paint it into a DOM element. One device, one texture, no
//! copy — the 3D view is a `<div>` as far as the rest of the UI is
//! concerned, and HTML can sit above or below it.

use crate::command::{Command, Receiver};
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

        let playback = match &self.show {
            Some((path, cue)) => Playback::load(
                &config.venue,
                None,
                Some(std::path::Path::new(path)),
                Some(*cue),
                None,
                None,
                None,
            )
            .unwrap_or_default(),
            None => Playback::default(),
        };
        let viz = EmbeddedViz::new(*config, Default::default(), playback, None, *gpu);
        tracing::info!(width, height, "viz.embed: built on the host device");
        self.state = State::Active(Box::new(viz));
    }

    pub fn new(
        config: VizConfig,
        show: Option<(String, usize)>,
        project: Option<&str>,
        commands: Receiver,
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
        drain(&self.commands, viz, self.transport.as_ref());
        follow_song(self.transport.as_ref(), viz);
        let Some(texture) = viz.render(width, height) else {
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
fn drain(commands: &Receiver, viz: &mut EmbeddedViz, transport: Option<&SongTransport>) {
    use ignition_core::{RecipeApply, Show};

    let world = viz.app_mut().world_mut();
    let Some(mut playback) = world.remove_resource::<Playback>() else {
        return;
    };
    {
        let Playback {
            cues,
            groups,
            rig,
            palettes,
            speeds,
            programmer,
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
                Command::Rate(bpm) => {
                    speeds.insert("Rate".to_string(), bpm);
                }
                Command::Color(name) => {
                    let show = Show {
                        groups,
                        palettes,
                        rig,
                        speeds,
                    };
                    programmer.apply(RecipeApply::Color(Ref::Named(name)), &show);
                }
                Command::Focus(name) => {
                    let show = Show {
                        groups,
                        palettes,
                        rig,
                        speeds,
                    };
                    programmer.apply(RecipeApply::FocusPoint(Ref::Named(name)), &show);
                }
                Command::Dimmer(level) => {
                    let show = Show {
                        groups,
                        palettes,
                        rig,
                        speeds,
                    };
                    programmer.apply(RecipeApply::Dimmer(level), &show);
                }
                Command::Release => {
                    let show = Show {
                        groups,
                        palettes,
                        rig,
                        speeds,
                    };
                    programmer.release(&show);
                }
                Command::Go => {
                    if let Some(player) = cues.as_mut() {
                        let show = Show {
                            groups,
                            palettes,
                            rig,
                            speeds,
                        };
                        player.go(&show);
                    }
                }
                Command::Cue(index) => {
                    if let Some(player) = cues.as_mut() {
                        let show = Show {
                            groups,
                            palettes,
                            rig,
                            speeds,
                        };
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
            }
        }
    }
    viz.app_mut().world_mut().insert_resource(playback);
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

    let world = viz.app_mut().world_mut();
    let Some(mut playback) = world.remove_resource::<Playback>() else {
        return;
    };
    {
        let Playback {
            cues,
            groups,
            rig,
            palettes,
            speeds,
            ..
        } = &mut playback;
        speeds.insert("Song".to_string(), bpm);
        if let Some(player) = cues.as_mut() {
            let show = Show {
                groups,
                palettes,
                rig,
                speeds,
            };
            player.seek(position, &show);
        }
    }
    viz.app_mut().world_mut().insert_resource(playback);
}
