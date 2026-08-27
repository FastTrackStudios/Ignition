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
use ignition_core::{HostRequest, MacroRunner};
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

    fn of(p: &blitz_traits::events::BlitzPointerEvent, down: bool, up: bool) -> Self {
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

    fn with_core<R>(&mut self, f: impl FnOnce(&mut VizCore) -> R) -> Option<R> {
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
        self.with_core(|core| core.can_create_surfaces(render_ctx));
    }

    fn destroy_surfaces(&mut self) {
        self.registered = None;
        self.with_core(|core| core.destroy_surfaces());
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
        self.with_core(|core| core.pointer = sample);
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
        let scene = self
            .with_core(|core| {
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
    pub fn attach() -> Self {
        Self { registered: None }
    }

    fn with_core<R>(&mut self, f: impl FnOnce(&mut VizCore) -> R) -> Option<R> {
        CORE.with(|core| core.borrow_mut().as_mut().map(f))
    }
}

impl Widget for ProgrammeWidget {
    fn connected(&mut self) {}
    fn disconnected(&mut self) {
        self.registered = None;
        self.with_core(|core| core.programme_size = None);
    }

    fn can_create_surfaces(&mut self, render_ctx: &mut dyn RenderContext) {
        self.with_core(|core| core.can_create_surfaces(render_ctx));
    }

    fn destroy_surfaces(&mut self) {
        self.registered = None;
        self.with_core(|core| core.programme_size = None);
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
        let scene = self
            .with_core(|core| core.paint_programme(render_ctx, width, height, &mut registered))
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
                match render_ctx.try_register_custom_resource(Box::new(texture)) {
                    Ok(id) => {
                        *registered = Some((id, tw, th));
                        id
                    }
                    Err(_) => {
                        *registered = None;
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
            None,
            show.map(|(path, _)| std::path::Path::new(path)),
            show.map(|(_, cue)| *cue),
            None,
            None,
            None,
            self.transport.as_ref().map(|t| t.song()),
        )
        .unwrap_or_default();
        // No per-studio GDTF setting yet: the workspace's own library
        // (`data/gdtf` + `data/gdtf/generated`), empty if absent.
        let gdtf = ignition_viz::gdtf_geometry::GdtfLibrary::load_default();
        tracing::info!(profiles = gdtf.len(), "viz.embed: GDTF library");
        let viz = EmbeddedViz::new(*config, Default::default(), playback, Some(gdtf), *gpu);
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
        let scale = self.scale as f32;
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
                match render_ctx.try_register_custom_resource(Box::new(texture)) {
                    Ok(id) => {
                        *registered = Some((id, tw, th));
                        id
                    }
                    Err(_) => {
                        *registered = None;
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
    macro_runner: &mut Option<MacroRunner>,
    show_file: Option<&str>,
) {
    use ignition_core::{Class, RecipeApply, Show};

    // A click in the viewport, as the command every other surface
    // sends — so it is logged, applied and published like one.
    // r[impl studio.program.pick-and-gizmos] - a viewport click is a Command::Select
    let picked = viz.take_selection().map(|chans| {
        if chans.is_empty() {
            Command::Deselect
        } else {
            Command::Select(ignition_core::Selection::Chans(chans))
        }
    });
    let world = viz.app_mut().world_mut();
    let Some(mut playback) = world.remove_resource::<Playback>() else {
        return;
    };
    // The Live / Program / Library commands, handled after the block
    // below — `live_commands::apply` wants the whole `Playback`, which
    // the destructuring borrows piecemeal.
    let mut deferred: Vec<Command> = Vec::new();
    {
        let Playback {
            playbacks,
            groups,
            rig,
            palettes,
            speeds,
            programmer,
            profile,
            library,
            bundles,
            ..
        } = &mut playback;
        // The looks and macros the keys name, and the roles a blackout
        // leaves alone, come from the shipped profile — the one the
        // bank is built from.
        let shipped = crate::faders::profile();
        if programmer.protected.is_empty() && !shipped.protected.is_empty() {
            // r[impl profile.protected-roles] - the programmer learns them from the profile
            programmer.protected = shipped.protected.clone();
        }
        let queued = std::iter::from_fn(|| commands.try_recv().ok());
        for command in picked.into_iter().chain(queued) {
            crate::live_commands::note(&command);
            // Rebuilt per command because `Rate` mutates `speeds`, which
            // the `Show` borrows. Cheap — it is four references.
            match command {
                Command::Take { .. }
                | Command::Untake(_)
                | Command::DeskScene(_)
                | Command::DeskRelease
                | Command::Protect { .. }
                | Command::StoreCue { .. }
                | Command::StoreLook { .. } => deferred.push(command),
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
                // r[impl viz.body-glow] - flips the viz setting in place
                Command::BodyGlow(on) => {
                    if let Some(mut settings) =
                        world.get_resource_mut::<ignition_viz::spawn::VizSettings>()
                    {
                        settings.body_glow = on;
                    }
                    tracing::info!(on, "studio: fixture body glow");
                }
                // r[impl studio.program.pick-and-gizmos] - the overlay keys flip the viz resource
                Command::Overlay { kind, on } => {
                    if let Some(mut overlays) =
                        world.get_resource_mut::<ignition_viz::gizmos::ProgramOverlays>()
                    {
                        use crate::command::OverlayKind;
                        match kind {
                            OverlayKind::Focus => overlays.focus = on,
                            OverlayKind::Beams => overlays.beams = on,
                            OverlayKind::Groups => overlays.groups = on,
                        }
                    }
                }
                Command::Labels(on) => {
                    if let Some(mut overlays) =
                        world.get_resource_mut::<ignition_viz::gizmos::ProgramOverlays>()
                    {
                        overlays.labels = on;
                    }
                }
                Command::ProgramView(on) => {
                    if let Some(mut overlays) =
                        world.get_resource_mut::<ignition_viz::gizmos::ProgramOverlays>()
                        && overlays.program != on
                    {
                        overlays.program = on;
                    }
                }
                // r[impl viz.camera-cuts] - a key or a tile cuts the programme camera
                Command::Camera { target, beats } => {
                    let (now, bpm) = camera_clock(playbacks, speeds);
                    if let Some(mut active) = world.get_resource_mut::<ignition_viz::ActiveCamera>()
                    {
                        let target = match target {
                            ignition_live_ui::command::CameraTarget::Slot(n) => {
                                ignition_viz::CameraTarget::Slot(n)
                            }
                            ignition_live_ui::command::CameraTarget::Preset(name) => {
                                ignition_viz::CameraTarget::Preset(name)
                            }
                        };
                        active.clear_queue();
                        if active.cut_to(&target, beats, now, bpm) {
                            tracing::info!(camera = ?active.preset, "studio: camera");
                        }
                    }
                }
                // r[impl studio.video.cameras-pane] - save the view the viewport is on
                Command::SaveCameraPreset { name } => {
                    let (now, _) = camera_clock(playbacks, speeds);
                    if let Some(mut active) = world.get_resource_mut::<ignition_viz::ActiveCamera>()
                    {
                        let state = active.state_at(now);
                        let preset = ignition_viz::CameraPreset {
                            ortho: state.ortho,
                            focus: state.focus,
                            ..ignition_viz::CameraPreset::new(
                                name.trim(),
                                state.eye.to_array(),
                                state.look.to_array(),
                                state.fov_deg,
                            )
                        };
                        if !preset.name.is_empty() {
                            active.cameras.store(preset);
                            active.preset = Some(name.trim().to_string());
                            save_cameras(&active.cameras);
                        }
                    }
                }
                // r[impl studio.video.cameras-pane] - set as slot N, for the operator and the venue
                Command::SetCameraSlot { slot, name } => {
                    if let Some(mut active) = world.get_resource_mut::<ignition_viz::ActiveCamera>()
                        && active.cameras.set_slot(slot, &name)
                    {
                        save_cameras(&active.cameras);
                        let operator = ignition_live_ui::operators::current_name();
                        if let Err(error) = ignition_live_ui::cameras::save_favourites(
                            &operator,
                            &active.cameras.favourites,
                        ) {
                            tracing::warn!(%error, "studio: camera favourites not saved");
                        }
                    }
                }
                // r[impl studio.video.cameras-pane] - delete
                Command::DeleteCameraPreset { name } => {
                    if let Some(mut active) = world.get_resource_mut::<ignition_viz::ActiveCamera>()
                        && active.cameras.remove(&name)
                    {
                        if active
                            .preset
                            .as_deref()
                            .is_some_and(|p| p.eq_ignore_ascii_case(&name))
                        {
                            active.preset = None;
                        }
                        save_cameras(&active.cameras);
                    }
                }
                // r[impl viz.programme-view] - the wide view's own preset
                Command::Wide { target } => {
                    if let Some(mut active) = world.get_resource_mut::<ignition_viz::ActiveCamera>()
                    {
                        let target = match target {
                            ignition_live_ui::command::CameraTarget::Slot(n) => {
                                ignition_viz::CameraTarget::Slot(n)
                            }
                            ignition_live_ui::command::CameraTarget::Preset(name) => {
                                ignition_viz::CameraTarget::Preset(name)
                            }
                        };
                        if !active.set_wide(&target) {
                            tracing::warn!(?target, "studio: wide names no preset");
                        }
                    }
                }
                // r[impl canvas.camera-source] - TO SCREENS
                Command::CanvasSource { canvas, source } => {
                    let source = if source.trim().is_empty() {
                        None
                    } else {
                        match ignition_viz::camera::CameraSource::parse(&source) {
                            Some(s) => Some(s),
                            None => {
                                tracing::warn!(source, "studio: not a camera source");
                                continue;
                            }
                        }
                    };
                    if let Some(mut switches) =
                        world.get_resource_mut::<ignition_viz::camera::CanvasSwitches>()
                    {
                        switches.set(&canvas, source);
                    }
                }
                Command::HighlightGroup(name) => {
                    if let Some(mut highlight) =
                        world.get_resource_mut::<ignition_viz::gizmos::HighlightGroup>()
                        && highlight.0 != name
                    {
                        highlight.0 = name;
                    }
                }
                // r[impl dmx.output-toggle] - flips the transmitter without touching the engine
                Command::Output(on) => {
                    if let Some(mut output) = world.get_resource_mut::<ignition_viz::DmxOutput>() {
                        output.set_enabled(on);
                    }
                    tracing::info!(on, "studio: dmx output");
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
                // r[impl playback.macro-runner] - a MACRO key starts one; the tick below runs it
                Command::Macro(name) => match MacroRunner::from_profile(shipped, &name) {
                    Some(runner) => {
                        tracing::info!(name, "studio: macro");
                        *macro_runner = Some(runner);
                    }
                    None => tracing::warn!(name, "studio: no such macro"),
                },
                // r[impl playback.look-hold] - a LOOK key latches the look on the held layer
                Command::Look(Some(name)) => {
                    let show = Show {
                        groups,
                        palettes,
                        rig,
                        speeds,
                        roles: profile,
                        library,
                        bundles,
                        looks: &shipped.looks,
                        ..Show::new(groups, rig)
                    };
                    let recipes = shipped.look_recipes(&name, &show);
                    if recipes.is_empty() {
                        tracing::warn!(name, "studio: no such look");
                    }
                    let safe = shipped
                        .looks
                        .get(&name)
                        .is_some_and(|l| l.kind == ignition_core::profile::LookKind::Safe);
                    programmer.hold_look(recipes, safe);
                }
                Command::Look(None) => programmer.release_hold(),
                // r[impl profile.effect-parameters] - the control reaches the engine's fader
                Command::Param { index, name, value } => programmer.set_param(index, &name, value),
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

        // The cues' host commands. `macro <name>` is the show starting
        // a profile macro — the drop on the last chorus's downbeat, the
        // end after the last cue — exactly as a MACRO key would, so a
        // move an operator busks and a move the show fires are one
        // thing. Anything else (`osc …`) is a host line: logged here,
        // and a transmitter's for the taking.
        // r[impl cues.command] - handed out once, when the cue goes live
        // r[impl playback.macro-runner] - a cue can start one
        // `camera …` is the show cutting the programme camera, on the
        // same clock — see `ignition_viz::camera`.
        // r[impl viz.camera-cuts] - a cue's camera command, at the cue change
        let (cam_now, cam_bpm) = camera_clock(playbacks, speeds);
        if let Some(player) = playbacks.of_class(Class::Song) {
            for command in player.drain_commands() {
                if let Some(mut active) = world.get_resource_mut::<ignition_viz::ActiveCamera>()
                    && ignition_viz::camera::apply_command_line(
                        &mut active,
                        &command,
                        cam_now,
                        cam_bpm,
                    )
                {
                    tracing::info!(command, "studio: camera (from cue)");
                    continue;
                }
                match command.strip_prefix("macro ") {
                    Some(name) => match MacroRunner::from_profile(shipped, name.trim()) {
                        Some(runner) => {
                            tracing::info!(name, "studio: macro (from cue)");
                            *macro_runner = Some(runner);
                        }
                        None => tracing::warn!(name, "studio: cue names no such macro"),
                    },
                    None => tracing::info!(command, "studio: cue command"),
                }
            }
        }

        // The running macro, stepped on the song's clock. Steps up to
        // the next wait land this frame; what the programmer cannot do
        // itself — the transmitter switch — comes back as a request.
        // r[impl playback.macro-runner]
        if let Some(runner) = macro_runner {
            let show = Show {
                groups,
                palettes,
                rig,
                speeds,
                roles: profile,
                library,
                bundles,
                looks: &shipped.looks,
                ..Show::new(groups, rig)
            };
            for request in runner.tick(programmer, playbacks, shipped, &show) {
                match request {
                    HostRequest::Output(on) => {
                        if let Some(mut output) =
                            world.get_resource_mut::<ignition_viz::DmxOutput>()
                        {
                            output.set_enabled(on);
                        }
                        tracing::info!(on, "studio: dmx output (macro)");
                    }
                }
            }
            if runner.finished() {
                *macro_runner = None;
            }
        }
    }
    if !deferred.is_empty() {
        let desk = crate::desk::path_for_venue(&crate::venue_dir());
        for command in &deferred {
            crate::live_commands::apply(
                command,
                &mut playback,
                desk.as_deref(),
                show_file.map(std::path::Path::new),
                transport.map(|t| t.song()),
            );
        }
    }
    viz.app_mut().world_mut().insert_resource(playback);
}

/// The clock a camera cut runs on: the song's, at the song's tempo.
fn camera_clock(
    playbacks: &mut ignition_core::Playbacks,
    speeds: &ignition_core::SpeedMasters,
) -> (f32, f32) {
    let now = playbacks
        .of_class(ignition_core::Class::Song)
        .map(|p| p.clock())
        .unwrap_or(0.0);
    let bpm = speeds.get("Song").copied().unwrap_or(120.0);
    (now, bpm)
}

/// The venue's `cameras.json`, written after a pane edit.
// r[impl viz.camera-presets] - saved back to the venue
fn save_cameras(cameras: &ignition_viz::Cameras) {
    let dir = std::path::PathBuf::from(crate::venue_dir());
    match cameras.save(&dir) {
        Ok(()) => {
            tracing::info!(path = %ignition_viz::Cameras::path(&dir).display(), "studio: cameras saved")
        }
        Err(error) => tracing::warn!(%error, "studio: cameras not saved"),
    }
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
    // A heartbeat in the log: what the song is doing and which cue is
    // up, once a second, so a "nothing happens" report can be read off
    // the file instead of reproduced.
    {
        use std::sync::atomic::{AtomicU64, Ordering};
        static LAST: AtomicU64 = AtomicU64::new(0);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        if now != LAST.swap(now, Ordering::Relaxed) {
            match transport {
                Some(t) => tracing::info!(
                    playing = t.is_playing(),
                    secs = t.seconds(),
                    position = ?t.position(),
                    "studio: transport"
                ),
                None => tracing::info!("studio: transport none"),
            }
        }
    }
    let playback = viz.app_mut().world().get_resource::<Playback>();
    let cue = playback
        .and_then(|p| p.song())
        .and_then(|player| player.current_index());
    let hit = playback.and_then(|p| p.triggers.last_fired_index());
    // The desk's own state, so the surface draws what the engine has
    // rather than what it last sent — a page turn from a MIDI key has
    // to move the strip on screen too.
    // What the player is *doing*, not only what the file says: how far
    // into its arrival the standing cue is, and whether the next one is
    // counting itself down. A cue list without these is a document.
    // r[impl studio.cuelist.live-state]
    let song = playback.and_then(|p| p.song());
    let mut next = Playhead {
        cue,
        hit,
        cue_fade: song.map_or(1.0, |player| player.fade_progress()),
        next_cue: song.and_then(|player| player.next_cue()),
        next_in: song.and_then(|player| player.next_in()),
        ..Default::default()
    };
    if let Some(p) = playback {
        crate::live_commands::publish(&mut next, p);
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
    let song_clock = playback
        .and_then(|p| p.song())
        .map(|p| p.clock())
        .unwrap_or(0.0);
    if let Some(t) = transport {
        next.secs = t.seconds() as f32;
        next.length = t.length() as f32;
        next.playing = t.is_playing();
    }
    if let Some(output) = viz
        .app_mut()
        .world()
        .get_resource::<ignition_viz::DmxOutput>()
    {
        next.output = output.summary();
    }
    // The programme camera, so the pane lights the right tile and can
    // save the view the viewport is on.
    // r[impl studio.video.cameras-pane] - the current view comes back on the playhead
    if let Some(active) = viz
        .app_mut()
        .world()
        .get_resource::<ignition_viz::ActiveCamera>()
    {
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
        next.camera = Some(ignition_live_ui::command::CameraState {
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
        });
    }
    // The wide preset means something only while a programme camera
    // takes the cuts; otherwise the main view is the programme.
    if let Some(camera) = next.camera.as_mut()
        && !viz
            .app_mut()
            .world()
            .get_resource::<ignition_viz::camera::ProgrammeView>()
            .is_some_and(|p| p.camera.is_some())
    {
        camera.wide = None;
    }
    // The canvases and what each shows, for TO SCREENS.
    if let Some(camera) = next.camera.as_mut() {
        let world = viz.app_mut().world_mut();
        let switched: std::collections::HashMap<String, Option<String>> = world
            .get_resource::<ignition_viz::camera::CanvasSwitches>()
            .map(|s| {
                s.current
                    .iter()
                    .map(|(k, v)| (k.clone(), v.as_ref().map(|c| c.content())))
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
        camera.canvases = names
            .into_iter()
            .map(|name| ignition_live_ui::command::CanvasRow {
                camera: switched.get(&name).cloned().flatten(),
                name,
            })
            .collect();
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
