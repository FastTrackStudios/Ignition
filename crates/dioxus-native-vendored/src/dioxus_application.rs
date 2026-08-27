use blitz_dom::HtmlParserProvider;
use blitz_shell::{BlitzApplication, BlitzShellProxy, View};
use blitz_traits::net::NetProvider;
use dioxus_core::{ScopeId, VirtualDom, provide_context};
use dioxus_history::{History, MemoryHistory};
use rustc_hash::FxHashMap;
use std::rc::Rc;
use std::sync::Arc;
use winit::application::ApplicationHandler;
use winit::event::{StartCause, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::window::WindowAttributes;
use winit::window::WindowId;

#[cfg(target_os = "macos")]
use winit::platform::macos::ApplicationHandlerExtMacOS;

use crate::DioxusNativeWindowRenderer;
use crate::event_handlers::WindowEventHandlers;
use crate::{BlitzShellEvent, DioxusDocument, WindowConfig, contexts::DioxusNativeDocument};

/// Dioxus-native specific event type
pub enum DioxusNativeEvent {
    /// A hotreload event, basically telling us to update our templates.
    #[cfg(all(feature = "hot-reload", debug_assertions))]
    DevserverEvent(dioxus_devtools::DevserverMsg),

    /// Create a new head element from the Link and Title elements
    ///
    /// todo(jon): these should probabkly be synchronous somehow
    CreateHeadElement {
        window: WindowId,
        name: String,
        attributes: Vec<(String, String)>,
        contents: Option<String>,
    },

    /// IGNITION PATCH (multi-window): open another OS window at runtime,
    /// with its own `VirtualDom`. Handled on the event-loop thread, where
    /// the `ActiveEventLoop` needed to create a window exists. `root`
    /// builds the vdom there (a `VirtualDom` is not `Send`, a closure
    /// that makes one can be). `on_created` receives the new window's id;
    /// `on_closed` runs when that window goes away, however it went.
    NewWindow {
        attributes: WindowAttributes,
        root: Box<dyn FnOnce() -> VirtualDom + Send + Sync>,
        on_created: Option<std::sync::mpsc::Sender<WindowId>>,
        on_closed: Option<Box<dyn FnOnce() + Send + Sync>>,
    },

    /// IGNITION PATCH: close one window. `BlitzShellEvent::CloseWindow`
    /// does the same for the shell; this one also runs the window's
    /// `on_closed` hook, so the two are kept together here.
    CloseWindow(WindowId),
}

/// IGNITION PATCH: everything a second window needs that the first one
/// got from `launch_cfg_with_props`. Held by the application so a
/// runtime `NewWindow` builds an identical document and renderer —
/// same net provider, same HTML parser, same wgpu features and limits
/// (which, with the shared device pool in `anyrender_vello`, is what
/// makes a later window land on the first window's device).
pub struct WindowFactory {
    pub net_provider: Arc<dyn NetProvider>,
    pub html_parser_provider: Option<Arc<dyn HtmlParserProvider>>,
    #[cfg(any(feature = "vello", feature = "vello-hybrid"))]
    pub features: Option<crate::Features>,
    #[cfg(any(feature = "vello", feature = "vello-hybrid"))]
    pub limits: Option<crate::Limits>,
}

impl WindowFactory {
    /// A document and renderer for `vdom`, configured like the launch
    /// window's.
    pub fn window_config(
        &self,
        vdom: VirtualDom,
        attributes: WindowAttributes,
    ) -> WindowConfig<DioxusNativeWindowRenderer> {
        vdom.provide_root_context(Arc::clone(&self.net_provider));
        if let Some(html) = &self.html_parser_provider {
            vdom.provide_root_context(Arc::clone(html));
        }
        let navigation_provider =
            Some(Arc::new(crate::link_handler::DioxusNativeNavigationProvider) as _);
        let doc = DioxusDocument::new(
            vdom,
            crate::DocumentConfig {
                net_provider: Some(Arc::clone(&self.net_provider)),
                html_parser_provider: self.html_parser_provider.clone(),
                navigation_provider,
                ..Default::default()
            },
        );
        #[cfg(any(feature = "vello", feature = "vello-hybrid"))]
        let renderer = DioxusNativeWindowRenderer::with_features_and_limits(
            self.features,
            self.limits.clone(),
        );
        #[cfg(not(any(feature = "vello", feature = "vello-hybrid")))]
        let renderer = DioxusNativeWindowRenderer::new();
        WindowConfig::with_attributes(Box::new(doc) as _, renderer, attributes)
    }
}

pub struct DioxusNativeApplication {
    pending_window: Option<WindowConfig<DioxusNativeWindowRenderer>>,
    inner: BlitzApplication<DioxusNativeWindowRenderer>,
    event_handlers: Rc<WindowEventHandlers>,
    /// IGNITION PATCH: see [`WindowFactory`]. `None` when the application
    /// was built without one, in which case `NewWindow` is ignored.
    factory: Option<WindowFactory>,
    /// IGNITION PATCH: per-window close hooks from `NewWindow`.
    on_closed: FxHashMap<WindowId, Box<dyn FnOnce() + Send + Sync>>,
}

impl DioxusNativeApplication {
    pub fn new(
        proxy: BlitzShellProxy,
        event_queue: std::sync::mpsc::Receiver<BlitzShellEvent>,
        config: WindowConfig<DioxusNativeWindowRenderer>,
    ) -> Self {
        Self {
            pending_window: Some(config),
            inner: BlitzApplication::new(proxy, event_queue),
            event_handlers: Rc::new(WindowEventHandlers::default()),
            factory: None,
            on_closed: FxHashMap::default(),
        }
    }

    /// IGNITION PATCH: enable runtime `NewWindow` events.
    pub fn with_window_factory(mut self, factory: WindowFactory) -> Self {
        self.factory = Some(factory);
        self
    }

    pub fn add_window(&mut self, window_config: WindowConfig<DioxusNativeWindowRenderer>) {
        self.inner.add_window(window_config);
    }

    /// IGNITION PATCH: the shared part of bringing a Dioxus window up —
    /// what `can_create_surfaces` did inline for the launch window, made
    /// reusable for windows opened later. Provides every root context a
    /// component in that window may ask for, runs the initial build and
    /// inserts the view.
    fn init_window(
        &mut self,
        config: WindowConfig<DioxusNativeWindowRenderer>,
        event_loop: &dyn ActiveEventLoop,
    ) -> WindowId {
        let mut window = View::init(config, event_loop, &self.inner.proxy);
        let winit_window = Arc::clone(&window.window);
        let renderer = window.renderer.clone();
        let window_id = window.window_id();
        let doc = window.downcast_doc_mut::<DioxusDocument>();

        let proxy = self.inner.proxy.clone();
        let event_handlers = self.event_handlers.clone();
        doc.vdom.in_scope(ScopeId::ROOT, || {
            let shared: Rc<dyn dioxus_document::Document> =
                Rc::new(DioxusNativeDocument::new(proxy.clone(), window_id));
            provide_context(shared);
            provide_context(event_handlers);
            // The proxy itself, so `open_window` / `close_window` can
            // reach the event loop from any component.
            provide_context(proxy);
        });

        // Add shell provider
        let shell_provider = doc.inner.borrow().shell_provider.clone();
        doc.vdom
            .in_scope(ScopeId::ROOT, move || provide_context(shell_provider));

        // Add history
        let history_provider: Rc<dyn History> = Rc::new(MemoryHistory::default());
        doc.vdom
            .in_scope(ScopeId::ROOT, move || provide_context(history_provider));

        // Add renderer
        doc.vdom
            .in_scope(ScopeId::ROOT, move || provide_context(renderer));

        // Add winit window
        doc.vdom
            .in_scope(ScopeId::ROOT, move || provide_context(winit_window));

        // Queue rebuild
        doc.initial_build();

        // And then request redraw
        window.request_redraw();

        self.inner.windows.insert(window_id, window);
        window_id
    }

    /// IGNITION PATCH: drop a window and run its close hook. Exits the
    /// event loop when it was the last one, as the shell does.
    fn close_window(&mut self, window_id: WindowId, event_loop: &dyn ActiveEventLoop) {
        // Drop window before exiting event loop
        // See https://github.com/rust-windowing/winit/issues/4135
        let window = self.inner.windows.remove(&window_id);
        drop(window);
        if let Some(hook) = self.on_closed.remove(&window_id) {
            hook();
        }
        if self.inner.windows.is_empty() {
            event_loop.exit();
        }
    }

    fn handle_dioxus_native_event(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        event: DioxusNativeEvent,
    ) {
        match event {
            #[cfg(all(feature = "hot-reload", debug_assertions))]
            DioxusNativeEvent::DevserverEvent(event) => match event {
                dioxus_devtools::DevserverMsg::HotReload(hotreload_message) => {
                    for window in self.inner.windows.values_mut() {
                        let doc = window.downcast_doc_mut::<DioxusDocument>();

                        // Apply changes to vdom
                        dioxus_devtools::apply_changes(&doc.vdom, &hotreload_message);

                        // Reload changed assets
                        for asset_path in &hotreload_message.assets {
                            if let Some(url) = asset_path.to_str() {
                                doc.inner.borrow_mut().reload_resource_by_href(url);
                            }
                        }

                        window.poll();
                    }
                }
                dioxus_devtools::DevserverMsg::Shutdown => event_loop.exit(),
                dioxus_devtools::DevserverMsg::FullReloadStart => {}
                dioxus_devtools::DevserverMsg::FullReloadFailed => {}
                dioxus_devtools::DevserverMsg::FullReloadCommand => {}
                _ => {}
            },

            DioxusNativeEvent::CreateHeadElement {
                name,
                attributes,
                contents,
                window,
            } => {
                if let Some(window) = self.inner.windows.get_mut(&window) {
                    let doc = window.downcast_doc_mut::<DioxusDocument>();
                    doc.create_head_element(&name, &attributes, &contents);
                    window.poll();
                }
            }

            DioxusNativeEvent::NewWindow {
                attributes,
                root,
                on_created,
                on_closed,
            } => {
                let Some(factory) = &self.factory else {
                    #[cfg(feature = "tracing")]
                    tracing::warn!("NewWindow ignored: application has no WindowFactory");
                    return;
                };
                let config = factory.window_config(root(), attributes);
                let window_id = self.init_window(config, event_loop);
                // The launch window is resumed by the shell's
                // `can_create_surfaces`; a window made after that has to
                // start its renderer itself.
                if let Some(view) = self.inner.windows.get_mut(&window_id) {
                    view.resume();
                }
                if let Some(hook) = on_closed {
                    self.on_closed.insert(window_id, hook);
                }
                if let Some(tx) = on_created {
                    let _ = tx.send(window_id);
                }
            }

            DioxusNativeEvent::CloseWindow(window_id) => {
                self.close_window(window_id, event_loop);
            }
        }
    }
}

impl ApplicationHandler for DioxusNativeApplication {
    #[cfg(target_os = "macos")]
    fn macos_handler(&mut self) -> Option<&mut dyn ApplicationHandlerExtMacOS> {
        self.inner.macos_handler()
    }

    fn resumed(&mut self, event_loop: &dyn ActiveEventLoop) {
        self.inner.resumed(event_loop);
    }

    fn suspended(&mut self, event_loop: &dyn ActiveEventLoop) {
        self.inner.suspended(event_loop);
    }

    fn destroy_surfaces(&mut self, event_loop: &dyn ActiveEventLoop) {
        self.inner.destroy_surfaces(event_loop);
    }

    fn about_to_wait(&mut self, event_loop: &dyn ActiveEventLoop) {
        // Where the shell decides whether to ask for another frame, and
        // where the document is re-resolved. Measured because the frame
        // stages alone accounted for barely half of a studio frame, and
        // the missing half had to be somewhere in the event loop.
        // IGNITION PATCH (profiling): r[impl studio.profiling] - the event loop's own share
        #[cfg(feature = "tracing")]
        let _span = tracing::info_span!(target: "ignition::profile", "loop.wait").entered();
        self.inner.about_to_wait(event_loop);
    }

    fn can_create_surfaces(&mut self, event_loop: &dyn ActiveEventLoop) {
        #[cfg(feature = "tracing")]
        tracing::debug!("Injecting document provider into all windows");

        if let Some(config) = self.pending_window.take() {
            self.init_window(config, event_loop);
        }

        self.inner.can_create_surfaces(event_loop);
    }

    fn new_events(&mut self, event_loop: &dyn ActiveEventLoop, cause: StartCause) {
        // IGNITION PATCH (profiling): r[impl studio.profiling] - the event loop's own share
        #[cfg(feature = "tracing")]
        let _span = tracing::info_span!(target: "ignition::profile", "loop.new_events").entered();
        self.inner.new_events(event_loop, cause);
    }

    fn window_event(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        // The event's own name as a field: a frame lost to a flood of
        // pointer moves and one lost to a slow redraw look identical in
        // a total, and are not the same problem.
        // IGNITION PATCH (profiling): r[impl studio.profiling] - the event loop's own share
        #[cfg(feature = "tracing")]
        let _span = tracing::info_span!(
            target: "ignition::profile",
            "loop.window_event",
            event = window_event_name(&event)
        )
        .entered();
        self.event_handlers
            .apply_event(window_id, &event, event_loop);
        // IGNITION PATCH: route the close through our own path so the
        // window's close hook runs. The shell would otherwise drop the
        // view silently.
        if matches!(event, WindowEvent::CloseRequested) {
            self.close_window(window_id, event_loop);
            return;
        }
        self.inner.window_event(event_loop, window_id, event);
    }

    fn proxy_wake_up(&mut self, event_loop: &dyn ActiveEventLoop) {
        // IGNITION PATCH (profiling): r[impl studio.profiling] - the event loop's own share
        #[cfg(feature = "tracing")]
        let _span = tracing::info_span!(target: "ignition::profile", "loop.wake").entered();
        while let Ok(event) = self.inner.event_queue.try_recv() {
            match event {
                BlitzShellEvent::Embedder(event) => {
                    // IGNITION PATCH: taken by value — `NewWindow` carries
                    // a `FnOnce`. `Arc::try_unwrap` succeeds because the
                    // queue held the only reference; a shared one falls
                    // back to the by-reference events, which are `Clone`.
                    if let Ok(event) = event.downcast::<DioxusNativeEvent>() {
                        match Arc::try_unwrap(event) {
                            Ok(event) => self.handle_dioxus_native_event(event_loop, event),
                            Err(shared) => {
                                if let Some(event) = shared.clone_by_ref() {
                                    self.handle_dioxus_native_event(event_loop, event);
                                }
                            }
                        }
                    }
                }
                BlitzShellEvent::CloseWindow { window_id } => {
                    self.close_window(window_id, event_loop);
                }
                event => {
                    // A span *name* per variant, not one span with a
                    // field: `loop.wake` on its own was sixteen
                    // milliseconds a frame with nothing inside it —
                    // half the studio's frame — and "the event queue is
                    // slow" is not a finding anyone can act on. The
                    // profiler's table is keyed by name, so the name is
                    // where the distinction has to live.
                    // IGNITION PATCH (profiling): r[impl studio.profiling] - which shell event
                    #[cfg(feature = "tracing")]
                    let _span = shell_event_span(&event).entered();
                    self.inner.handle_blitz_shell_event(event_loop, event)
                }
            }
        }
    }
}

/// One span per `BlitzShellEvent` variant, named for it.
///
/// `Poll` is the one that matters: it is the Dioxus virtual DOM coming
/// up for air — rendering every dirty component and applying the
/// mutations to the document — and in the studio it is the single most
/// expensive thing in a frame, several times the visualizer.
// IGNITION PATCH (profiling): r[impl studio.profiling] - which shell event
#[cfg(feature = "tracing")]
fn shell_event_span(event: &BlitzShellEvent) -> tracing::Span {
    const TARGET: &str = "ignition::profile";
    match event {
        BlitzShellEvent::Poll { .. } => tracing::info_span!(target: TARGET, "loop.poll"),
        BlitzShellEvent::RequestRedraw { .. } => {
            tracing::info_span!(target: TARGET, "loop.redraw")
        }
        BlitzShellEvent::ResumeReady { .. } => {
            tracing::info_span!(target: TARGET, "loop.resume")
        }
        _ => tracing::info_span!(target: TARGET, "loop.shell_other"),
    }
}

/// A `WindowEvent`'s variant, as a `&'static str` for a span field.
///
/// `Debug` would do it, but formats the payload too — a pointer move a
/// frame is a string allocation a frame, in the profiler, about the
/// profiler.
// IGNITION PATCH (profiling): r[impl studio.profiling] - which event, without allocating to say so
#[cfg(feature = "tracing")]
fn window_event_name(event: &WindowEvent) -> &'static str {
    match event {
        WindowEvent::RedrawRequested => "redraw",
        WindowEvent::PointerMoved { .. } => "pointer-moved",
        WindowEvent::PointerButton { .. } => "pointer-button",
        WindowEvent::MouseWheel { .. } => "wheel",
        WindowEvent::KeyboardInput { .. } => "key",
        WindowEvent::SurfaceResized { .. } => "resize",
        WindowEvent::ScaleFactorChanged { .. } => "scale",
        WindowEvent::CloseRequested => "close",
        _ => "other",
    }
}

impl DioxusNativeEvent {
    /// IGNITION PATCH: the events that can be handled from a shared
    /// reference — everything but `NewWindow`, whose closure is single
    /// use.
    fn clone_by_ref(&self) -> Option<Self> {
        match self {
            #[cfg(all(feature = "hot-reload", debug_assertions))]
            Self::DevserverEvent(e) => Some(Self::DevserverEvent(e.clone())),
            Self::CreateHeadElement {
                window,
                name,
                attributes,
                contents,
            } => Some(Self::CreateHeadElement {
                window: *window,
                name: name.clone(),
                attributes: attributes.clone(),
                contents: contents.clone(),
            }),
            Self::CloseWindow(id) => Some(Self::CloseWindow(*id)),
            Self::NewWindow { .. } => None,
        }
    }
}
