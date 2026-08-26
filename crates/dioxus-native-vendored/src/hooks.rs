use crate::event_handlers::{WindowEventHandlers, WinitEventHandlerId};

use dioxus_core::{Runtime, consume_context, current_scope_id, use_hook_with_cleanup};
use std::rc::Rc;
use winit::{
    event::{ElementState, WindowEvent},
    event_loop::ActiveEventLoop,
    keyboard::{Key, NamedKey},
};

/// Register an event handler that runs when a winit event is processed.
pub fn use_window_event(
    mut handler: impl FnMut(&WindowEvent, &dyn ActiveEventLoop) + 'static,
) -> WinitEventHandlerId {
    let runtime = Runtime::current();
    let scope_id = current_scope_id();
    let window_id = crate::use_window().id();

    use_hook_with_cleanup(
        move || {
            let handlers: Rc<WindowEventHandlers> = consume_context();
            handlers.add(window_id, move |event, target| {
                runtime.in_scope(scope_id, || handler(event, target))
            })
        },
        move |handler| handler.remove(),
    )
}

/// Register a handler that runs when the back button is pressed.
///
/// This builds on top of [`use_window_event`]: the back button is delivered by `winit` as a
/// [`WindowEvent::KeyboardInput`] whose logical key is [`NamedKey::BrowserBack`]. This most
/// commonly comes from the Android hardware/system back button, but may also be produced by a
/// keyboard or mouse back key on other platforms. The provided `handler` is called once each
/// time the button is pressed (key repeats are ignored).
///
/// Returns a [`WinitEventHandlerId`] which can be used to remove the handler.
pub fn use_back_button(mut handler: impl FnMut() + 'static) -> WinitEventHandlerId {
    use_window_event(move |event, _target| {
        if let WindowEvent::KeyboardInput { event, .. } = event
            && event.state == ElementState::Pressed
            && !event.repeat
            && event.logical_key == Key::Named(NamedKey::BrowserBack)
        {
            handler();
        }
    })
}

// ---------------------------------------------------------------------
// IGNITION PATCH (multi-window)
// ---------------------------------------------------------------------

use crate::DioxusNativeEvent;
use blitz_shell::{BlitzShellEvent, BlitzShellProxy};
use dioxus_core::{ComponentFunction, Element, VirtualDom, use_hook};
use winit::window::{WindowAttributes, WindowId};

/// The id of a window opened with [`open_window`], once the event loop
/// has made it. Creation happens on the event-loop thread after the
/// current render returns, so the id is never available synchronously:
/// poll [`WindowOpen::try_id`] from an effect or a future.
pub struct WindowOpen {
    rx: std::sync::mpsc::Receiver<WindowId>,
    id: std::cell::Cell<Option<WindowId>>,
}

impl WindowOpen {
    /// The window's id, if the event loop has created it yet.
    pub fn try_id(&self) -> Option<WindowId> {
        if self.id.get().is_none()
            && let Ok(id) = self.rx.try_recv()
        {
            self.id.set(Some(id));
        }
        self.id.get()
    }

    /// Blocks until the window exists. Never call this on the event-loop
    /// thread — that is the thread that has to create it.
    pub fn wait(&self) -> Option<WindowId> {
        if let Some(id) = self.try_id() {
            return Some(id);
        }
        let id = self.rx.recv().ok()?;
        self.id.set(Some(id));
        Some(id)
    }
}

/// The event-loop proxy of the window whose component is rendering.
/// Provided as a root context on every window (see `init_window`).
pub fn use_shell_proxy() -> BlitzShellProxy {
    use_hook(consume_context::<BlitzShellProxy>)
}

/// Opens another OS window, from a component, with its own `VirtualDom`
/// rooted at `app`. `on_closed` runs on the event-loop thread when that
/// window closes, by whichever route.
pub fn open_window(
    attributes: WindowAttributes,
    app: fn() -> Element,
    on_closed: Option<Box<dyn FnOnce() + Send + Sync>>,
) -> WindowOpen {
    open_window_with_props(attributes, app, (), on_closed)
}

/// [`open_window`] with root props.
pub fn open_window_with_props<P, M>(
    attributes: WindowAttributes,
    app: impl ComponentFunction<P, M> + Send + Sync + 'static,
    props: P,
    on_closed: Option<Box<dyn FnOnce() + Send + Sync>>,
) -> WindowOpen
where
    P: Clone + Send + Sync + 'static,
    M: 'static,
{
    let proxy = consume_context::<BlitzShellProxy>();
    open_window_via(&proxy, attributes, app, props, on_closed)
}

/// [`open_window_with_props`] for callers that hold a proxy already —
/// code outside any component, such as a startup layout.
pub fn open_window_via<P, M>(
    proxy: &BlitzShellProxy,
    attributes: WindowAttributes,
    app: impl ComponentFunction<P, M> + Send + Sync + 'static,
    props: P,
    on_closed: Option<Box<dyn FnOnce() + Send + Sync>>,
) -> WindowOpen
where
    P: Clone + Send + Sync + 'static,
    M: 'static,
{
    let (tx, rx) = std::sync::mpsc::channel();
    proxy.send_event(BlitzShellEvent::embedder_event(
        DioxusNativeEvent::NewWindow {
            attributes,
            root: Box::new(move || VirtualDom::new_with_props(app, props)),
            on_created: Some(tx),
            on_closed,
        },
    ));
    WindowOpen {
        rx,
        id: std::cell::Cell::new(None),
    }
}

/// Closes a window by id, from a component. Closing the last window
/// ends the application.
pub fn close_window(id: WindowId) {
    consume_context::<BlitzShellProxy>().send_event(BlitzShellEvent::embedder_event(
        DioxusNativeEvent::CloseWindow(id),
    ));
}

/// [`close_window`] through a held proxy.
pub fn close_window_via(proxy: &BlitzShellProxy, id: WindowId) {
    proxy.send_event(BlitzShellEvent::embedder_event(
        DioxusNativeEvent::CloseWindow(id),
    ));
}
