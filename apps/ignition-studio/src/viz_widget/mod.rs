//! The visualizer, as a Blitz widget.
//!
//! Blitz creates the wgpu device for its own Vello renderer; this hands
//! that same device to Bevy (`EmbeddedViz`), renders the venue into a
//! texture on it, and registers that texture as an anyrender resource
//! so Blitz can paint it into a DOM element. One device, one texture, no
//! copy — the 3D view is a `<div>` as far as the rest of the UI is
//! concerned, and HTML can sit above or below it.

use crate::command::{Receiver, StateTx};
use anyrender::{PaintRef, PaintScene, RenderContext, ResourceId, Scene};
use blitz_dom::Widget;
use blitz_dom::node::ComputedStyles;
use dioxus_native::DeviceHandle;
use ignition_core::MacroRunner;
use ignition_daw::SongTransport;
use ignition_viz::VizConfig;
use ignition_viz::embedded::{EmbeddedViz, HostGpu};
use ignition_viz::playback::Playback;
use peniko::kurbo::{Affine, Rect};
use peniko::{Fill, ImageBrush, ImageSampler};
use std::sync::{Arc, LazyLock};
use tokio::sync::Notify;

mod commands;
mod publish;
pub mod sound;

use commands::drain;
use publish::{follow_song, publish};
pub use sound::SoundFade;
use sound::smooth_sound;

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

/// The one visualizer in the process, and everything needed to build it
/// once a device shows up.
///
/// The widget is constructed by Dioxus during render, but Blitz does not
/// hand out a device until `can_create_surfaces` — so the config waits
/// here and the Bevy app is built at that moment, not before.
///
/// One per process, not per widget: the Visualizer *panel* can be
/// hosted by any window (`r[studio.windows.visualizer-anywhere]`), and
/// moving it means the old window's widget is dropped and a new one
/// built in the new window. The Bevy app, the transport and the command
/// receiver survive that move here, in a thread-local the widgets
/// borrow — thread-local because every Blitz window paints on the one
/// event-loop thread, and a Bevy `App` is not `Send`.
pub struct VizCore {
    commands: Receiver,
    pending: Option<Box<VizConfig>>,
    /// The cue to sit on once the visualizer exists. Loading the show
    /// needs the venue, which lives inside `pending` until then.
    show: Option<(String, usize)>,
    state: State,
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
    /// The profile macro running, if one is. One at a time: a MACRO key
    /// replaces whatever was running, and the new macro's own release
    /// lets go of what the old one took.
    // r[impl playback.macro-runner] - the widget ticks it every frame
    macro_runner: Option<MacroRunner>,
    /// The latest pointer over the viewport, and the DPI scale paint
    /// last saw, which turns its CSS pixels into texture pixels.
    pointer: PointerSample,
    scale: f64,
    /// When the app last stepped, so a Programme pane hosted without a
    /// Visualizer pane can step it itself without doubling the frame
    /// rate when both are up — see `ProgrammeWidget`.
    last_step: Option<std::time::Instant>,
    /// When this core was built, for the free-running canvas clock that
    /// benchmark mode uses in place of a transport.
    started: std::time::Instant,
    /// The size the Programme pane last painted at; `None` when no
    /// pane shows it, which takes the programme camera down.
    programme_size: Option<(u32, u32)>,
}

/// The pointer as Blitz last reported it over the viewport, in the
/// element's own CSS pixels. Scaled to texture pixels on the way in.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct PointerSample {
    position: Option<(f32, f32)>,
    primary: bool,
    shift: bool,
    ctrl: bool,
}

impl PointerSample {
    /// `keyboard_types::Modifiers` bits — the crate is not a direct
    /// dependency here, and the two flags wanted are stable.
    const SHIFT: u32 = 0x200;
    const CONTROL: u32 = 0x8;

    const fn of(p: &blitz_traits::events::BlitzPointerEvent, down: bool, up: bool) -> Self {
        let primary_held = p
            .buttons
            .contains(blitz_traits::events::MouseEventButtons::Primary);
        let primary = if down {
            true
        } else if up {
            false
        } else {
            primary_held
        };
        let mods = p.mods.bits();
        Self {
            position: Some((p.element.x, p.element.y)),
            primary,
            shift: mods & Self::SHIFT != 0,
            ctrl: mods & Self::CONTROL != 0,
        }
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

thread_local! {
    static CORE: std::cell::RefCell<Option<VizCore>> = const { std::cell::RefCell::new(None) };
}

/// The visualizer, as a Blitz widget: a view onto [`VizCore`] from one
/// window. Holds only what is per window — the texture registration
/// with *that* window's renderer.
// r[impl studio.windows.visualizer-anywhere] - per-window registration of one shared texture
pub struct VizWidget {
    /// The texture handed to this window's renderer last frame, and its
    /// size. Re-used across frames: registering a new resource every
    /// frame would leak one per frame. Keyed by the widget, and so by
    /// the window: a second window's renderer needs its own
    /// registration of the same texture, on the same device.
    registered: Option<(ResourceId, u32, u32)>,
}

impl VizWidget {
    /// A widget over the process's visualizer, building it with `build`
    /// if this is the first window to host the panel.
    pub fn attach(build: impl FnOnce() -> VizCore) -> Self {
        CORE.with(|core| {
            let mut core = core.borrow_mut();
            if core.is_none() {
                *core = Some(build());
            } else {
                tracing::info!("viz.embed: panel re-hosted; the visualizer carries over");
            }
        });
        Self { registered: None }
    }

    fn with_core<R>(f: impl FnOnce(&mut VizCore) -> R) -> Option<R> {
        CORE.with(|core| core.borrow_mut().as_mut().map(f))
    }
}

impl Widget for VizWidget {
    fn connected(&mut self) {}
    fn disconnected(&mut self) {
        // The renderer that held this registration unregisters it
        // itself when the widget node drops; nothing to do but forget.
        self.registered = None;
    }

    fn can_create_surfaces(&mut self, render_ctx: &mut dyn RenderContext) {
        Self::with_core(|core| core.can_create_surfaces(render_ctx));
    }

    fn destroy_surfaces(&mut self) {
        self.registered = None;
        Self::with_core(VizCore::destroy_surfaces);
    }

    /// Blitz forwards the pointer events that land on the widget's node.
    /// They are kept as the latest sample and handed to the visualizer
    /// on the next paint, which is when it steps — so a click and the
    /// frame it lands on are the same frame.
    // r[impl studio.program.pick-and-gizmos] - Blitz pointer events reach the viewport
    fn handle_event(&mut self, event: &blitz_traits::events::UiEvent) {
        use blitz_traits::events::UiEvent;
        let sample = match event {
            UiEvent::PointerMove(p) | UiEvent::PointerDown(p) | UiEvent::PointerUp(p) => {
                PointerSample::of(
                    p,
                    matches!(event, UiEvent::PointerDown(_)),
                    matches!(event, UiEvent::PointerUp(_)),
                )
            }
            UiEvent::PointerCancel(_) => PointerSample::default(),
            _ => return,
        };
        Self::with_core(|core| core.pointer = sample);
    }

    fn paint(
        &mut self,
        render_ctx: &mut dyn RenderContext,
        _styles: &ComputedStyles,
        width: u32,
        height: u32,
        scale: f64,
    ) -> Scene {
        let mut registered = self.registered.take();
        let scene = Self::with_core(|core| {
            // The pointer arrives in CSS pixels; the texture is
            // `scale` times that. Kept here so the sample is scaled
            // by the value this paint actually used.
            core.scale = scale;
            core.paint(render_ctx, width, height, &mut registered)
        })
        .unwrap_or_default();
        self.registered = registered;
        scene
    }
}

/// The programme camera's texture, as a Blitz widget: the cut, beside
/// or away from the wide Visualizer pane. A view onto the same
/// [`VizCore`]; it never builds one. When no Visualizer pane is painting
/// it steps the app itself, so a Programme pane alone still animates.
// r[impl viz.programme-view] - a second widget over the same core
pub struct ProgrammeWidget {
    registered: Option<(ResourceId, u32, u32)>,
}

impl ProgrammeWidget {
    pub const fn attach() -> Self {
        Self { registered: None }
    }

    fn with_core<R>(f: impl FnOnce(&mut VizCore) -> R) -> Option<R> {
        CORE.with(|core| core.borrow_mut().as_mut().map(f))
    }
}

impl Widget for ProgrammeWidget {
    fn connected(&mut self) {}
    fn disconnected(&mut self) {
        self.registered = None;
        Self::with_core(|core| core.programme_size = None);
    }

    fn can_create_surfaces(&mut self, render_ctx: &mut dyn RenderContext) {
        Self::with_core(|core| core.can_create_surfaces(render_ctx));
    }

    fn destroy_surfaces(&mut self) {
        self.registered = None;
        Self::with_core(|core| core.programme_size = None);
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
        let mut registered = self.registered.take();
        let scene = Self::with_core(|core| {
            core.paint_programme(render_ctx, width, height, &mut registered)
        })
        .unwrap_or_default();
        self.registered = registered;
        scene
    }
}

impl VizCore {
    /// The Programme pane's frame: asks for the programme camera at
    /// this size, steps the app if nothing else is, and hands the
    /// programme texture to the window's renderer.
    fn paint_programme(
        &mut self,
        render_ctx: &mut dyn RenderContext,
        width: u32,
        height: u32,
        registered: &mut Option<(ResourceId, u32, u32)>,
    ) -> Scene {
        // r[impl studio.profiling] - the second pane's share
        let _span = tracing::info_span!(target: "ignition::profile", "viz.programme").entered();
        let mut scene = Scene::new();
        if width == 0 || height == 0 {
            return scene;
        }
        self.programme_size = Some((width, height));
        if matches!(self.state, State::Ready(_)) {
            // No Visualizer pane has built the core yet: build it at
            // this size, which is as good a first size as any.
            self.activate(width, height);
        }
        // A Visualizer pane that painted in the last few milliseconds
        // has already stepped this frame; otherwise this pane drives.
        let stale = self
            .last_step
            .is_none_or(|t| t.elapsed() > std::time::Duration::from_millis(6));
        if stale {
            let State::Active(viz) = &mut self.state else {
                return scene;
            };
            drain(
                &self.commands,
                viz,
                self.transport.as_ref(),
                &mut self.sound,
                &mut self.macro_runner,
                self.show.as_ref().map(|(path, _)| path.as_str()),
            );
            smooth_sound(&mut self.sound, viz);
            follow_song(self.transport.as_ref(), viz);
            // With no transport there is nothing to scrub the screens
            // against, and a benchmark with frozen canvases is not the
            // thing being measured — the clips are part of the load.
            // Wall time, since in bench mode there is no song to be
            // wrong about.
            // r[impl studio.profiling] - a benchmark you can open, with the rig lit
            if self.transport.is_none() && crate::bench_mode() {
                let secs = self.started.elapsed().as_secs_f64();
                viz.app_mut()
                    .world_mut()
                    .insert_resource(ignition_viz::CanvasClock::at(secs));
            }
            publish(&self.report, self.transport.as_ref(), viz);
            viz.set_programme(self.programme_size);
            self.last_step = Some(std::time::Instant::now());
            // Zero keeps the main target at whatever size it last was.
            let _ = viz.render(0, 0);
            FRAME_DONE.notify_one();
        }
        let State::Active(viz) = &mut self.state else {
            return scene;
        };
        let Some(texture) = viz.programme_texture() else {
            return scene;
        };
        let (tw, th) = (texture.width(), texture.height());
        let resource = match *registered {
            Some((id, w, h)) if (w, h) == (tw, th) => id,
            other => {
                if let Some((old, _, _)) = other {
                    render_ctx.unregister_resource(old);
                }
                if let Ok(id) = render_ctx.try_register_custom_resource(Box::new(texture)) {
                    *registered = Some((id, tw, th));
                    id
                } else {
                    *registered = None;
                    return scene;
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
            &Rect::from_origin_size((0.0, 0.0), (f64::from(width), f64::from(height))),
        );
        scene
    }
}

/// The Programme pane's element: the programme widget over the one
/// core, kept repainting by the same frame signal the Visualizer uses.
// r[impl viz.programme-view] - the pane
#[dioxus::prelude::component]
pub fn Programme() -> dioxus::prelude::Element {
    use dioxus::prelude::*;
    let widget_attr =
        use_hook(|| dioxus_native_dom::CustomWidgetAttr::new(ProgrammeWidget::attach()));
    // Winit, not the DOM — see the Viewport's own loop in `main.rs` for
    // why a signal here cost four milliseconds a frame of layout.
    // r[impl studio.profiling] - the frame loop costs no layout
    let window = dioxus_native::use_window();
    use_future(move || {
        let window = window.clone();
        async move {
            let done = FRAME_DONE.clone();
            loop {
                let _ =
                    tokio::time::timeout(std::time::Duration::from_millis(100), done.notified())
                        .await;
                window.request_redraw();
            }
        }
    });
    rsx! {
        div { class: "viz programme",
            object { "data": widget_attr }
        }
    }
}

impl VizCore {
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
        // The venue's cameras, with this operator's ten on the keys.
        // r[impl viz.camera-presets] - loaded with the venue
        // r[impl viz.camera-favourites] - the operator file's list replaces the venue's
        {
            let venue_dir = std::path::PathBuf::from(crate::venue_dir());
            let (min, max) = config.venue.bounds();
            config.cameras = ignition_viz::Cameras::load_or_builtin(&venue_dir, min, max);
            let operator = ignition_live_ui::operators::current_name();
            if let Some(favourites) = ignition_live_ui::cameras::favourites(&operator) {
                config.cameras.favourites = favourites;
            }
            if config.camera_preset.is_none() {
                config.camera_preset = config.cameras.favourites.first().cloned();
            }
        }

        // Loaded through `load` even with no show, so the look list the
        // operator GOes through exists either way.
        let show = self.show.as_ref();
        // The transport's song map resolves the show's relative
        // positions on load, so the cues land on *this* arrangement.
        let playback = Playback::load(
            &config.venue,
            ignition_viz::playback::LoadOptions {
                recipes: show.map(|(path, _)| std::path::Path::new(path)),
                jump_to_cue: show.map(|(_, cue)| *cue),
                song: self
                    .transport
                    .as_ref()
                    .map(ignition_daw::SongTransport::song),
                ..Default::default()
            },
        )
        .unwrap_or_default();
        // No per-studio GDTF setting yet: the workspace's own library
        // (`data/gdtf` + `data/gdtf/generated`), empty if absent.
        let gdtf = ignition_viz::gdtf_geometry::GdtfLibrary::load_default();
        tracing::info!(profiles = gdtf.len(), "viz.embed: GDTF library");
        let viz = EmbeddedViz::new(
            *config,
            ignition_viz::DmxUniverses::default(),
            playback,
            Some(gdtf),
            *gpu,
        );
        tracing::info!(width, height, "viz.embed: built on the host device");
        // Benchmark mode takes its cue, rather than merely standing on
        // it. Down the same channel the GO button uses, so what is
        // measured is a cue fired the way an operator fires one — and
        // it lands on the drain a few lines below, in this same paint.
        // r[impl studio.profiling] - a benchmark you can open, with the rig lit
        if crate::bench_mode() {
            tracing::info!("studio: benchmark mode — taking the cue");
            ignition_live_ui::send(ignition_live_ui::command::Command::Go);
        }
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
        // Benchmark mode opens no project, and that is the whole of why
        // it works. `follow_song` seeks the cue player to the
        // transport's position *every frame*; with a project loaded and
        // its transport parked at bar one, it drags the player off the
        // benchmark cue as fast as GO puts it there, and the studio
        // comes up on a dark rig having done everything right. The cue
        // was firing; something else was overwriting it.
        // r[impl studio.profiling] - a benchmark you can open, with the rig lit
        let project = if crate::bench_mode() { None } else { project };
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
            transport,
            report,
            sound: SoundFade::default(),
            macro_runner: None,
            pointer: PointerSample::default(),
            started: std::time::Instant::now(),
            scale: 1.0,
            last_step: None,
            programme_size: None,
        }
    }
}

impl VizCore {
    fn can_create_surfaces(&mut self, render_ctx: &dyn RenderContext) {
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
    }

    /// One frame: drain the UI's commands, step the show, render, and
    /// hand the texture to `render_ctx` — the renderer of whichever
    /// window is painting, registered once per window and re-used.
    fn paint(
        &mut self,
        render_ctx: &mut dyn RenderContext,
        width: u32,
        height: u32,
        registered: &mut Option<(ResourceId, u32, u32)>,
    ) -> Scene {
        // r[impl studio.profiling] - the visualizer's share of a Blitz scene
        let _span = tracing::info_span!(target: "ignition::profile", "viz.paint").entered();
        let mut scene = Scene::new();
        if matches!(self.state, State::Ready(_)) && width > 0 && height > 0 {
            self.activate(width, height);
        }
        let State::Active(viz) = &mut self.state else {
            tracing::debug!(width, height, "viz.embed: paint with no active viz");
            return scene;
        };
        let scale = crate::num::f32_of_f64(self.scale);
        viz.pointer(
            self.pointer.position.map(|(x, y)| (x * scale, y * scale)),
            self.pointer.primary,
            self.pointer.shift,
            self.pointer.ctrl,
        );
        // Everything between the operator and the show: the UI's
        // commands, the sound fade, the transport, the state published
        // back. Cheap by construction — if it ever is not, the table
        // says so rather than the frame just being slower.
        // r[impl studio.profiling] - the show side of a frame
        {
            let _span = tracing::info_span!(target: "ignition::profile", "viz.commands").entered();
            drain(
                &self.commands,
                viz,
                self.transport.as_ref(),
                &mut self.sound,
                &mut self.macro_runner,
                self.show.as_ref().map(|(path, _)| path.as_str()),
            );
            smooth_sound(&mut self.sound, viz);
            follow_song(self.transport.as_ref(), viz);
            publish(&self.report, self.transport.as_ref(), viz);
        }
        // The programme camera renders only while a Programme pane is
        // up, at that pane's size.
        // r[impl viz.programme-view] - on while a pane shows it
        viz.set_programme(self.programme_size);
        self.last_step = Some(std::time::Instant::now());
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
        // Keyed on the texture's own size, not the requested one: after
        // a resize the renderer hands back the previous target for a
        // frame or two while the new one warms up, and keying on the
        // request registered that stale texture under the new size —
        // after which nothing ever re-registered, and the picture stayed
        // frozen at the moment of the resize.
        // r[impl studio.windows.visualizer-anywhere] - the texture is registered by what it is
        let (tw, th) = (texture.width(), texture.height());
        let resource = match *registered {
            Some((id, w, h)) if (w, h) == (tw, th) => id,
            other => {
                if let Some((old, _, _)) = other {
                    render_ctx.unregister_resource(old);
                }
                if let Ok(id) = render_ctx.try_register_custom_resource(Box::new(texture)) {
                    *registered = Some((id, tw, th));
                    id
                } else {
                    *registered = None;
                    return scene;
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
            &Rect::from_origin_size((0.0, 0.0), (f64::from(width), f64::from(height))),
        );
        scene
    }
}
