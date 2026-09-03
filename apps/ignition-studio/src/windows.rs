//! The panel host: every OS window of the studio, what it holds, and the
//! root component that draws it.
//!
//! One process, one engine, any number of windows. The engine state is
//! the process-wide channels in `main.rs` (`TX`, `STATE_RX`), so a
//! window needs nothing handed to it beyond *which panels to draw* —
//! and that lives here, in [`HOST`], keyed by a [`HostId`] the window's
//! root component is given as its one prop. Each window is its own
//! `VirtualDom` (Dioxus signals do not cross that boundary), so windows
//! learn of each other's changes by polling a version counter, which at
//! a few times a second costs nothing and needs no cross-runtime plumbing.
//!
//! The pop-out / dock-back state machine is [`Host`], plain data with no
//! window in sight, so it is tested without a display.

// r[impl studio.windows.multiple] - any number of windows, each hosting any panes, one engine
// r[impl studio.panels] - the host: a pane is drawn by whichever window's tree holds it
// r[impl studio.dock] - each OS window renders one dock tree

use crate::dock::{DockNode, DockState, PaneKind, Preset};
use crate::layout::{
    self, Layout, MonitorInfo, Placement, Region, View, WindowSpec, docked_rect, pick_monitor_index,
};
use dioxus::prelude::*;
use dioxus_native::winit::window::WindowId;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

/// A window as the host knows it. Not a `WindowId`: the host has to
/// name a window before winit has made it.
pub type HostId = u64;

/// One window: its spec, where it came from if it was popped out, and
/// the OS window once it exists.
#[derive(Debug, Clone, PartialEq)]
pub struct Hosted {
    pub id: HostId,
    pub spec: WindowSpec,
    /// While one pane is soloed, the whole tree it goes back to.
    pub solo: Option<DockNode>,
    /// The window a pop-out came from; its panes go back there when
    /// this one closes.
    pub popped_from: Option<HostId>,
    pub window: Option<WindowId>,
}

impl Hosted {
    /// The window's dock as a value: the drawn tree and the remembered one.
    fn dock(&self) -> DockState {
        DockState {
            tree: self.spec.tree.clone(),
            solo: self.solo.clone(),
        }
    }

    fn set_dock(&mut self, dock: DockState) {
        self.spec.tree = dock.tree;
        self.solo = dock.solo;
    }

    /// Edit the real tree (leaving solo first).
    fn edit(&mut self, f: impl FnOnce(&mut DockNode)) {
        let mut dock = self.dock();
        dock.edit(f);
        self.set_dock(dock);
    }

    /// The spec a layout file gets: the whole tree, soloed or not.
    fn persisted(&self) -> WindowSpec {
        let mut spec = self.spec.clone();
        spec.tree = self.dock().persisted().clone();
        spec
    }

    fn panes(&self) -> Vec<PaneKind> {
        self.dock().persisted().panes()
    }
}

/// Every window in the process.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Host {
    next: HostId,
    pub windows: Vec<Hosted>,
}

impl Host {
    pub fn from_layout(layout: &Layout) -> Self {
        let mut host = Self::default();
        for spec in &layout.windows {
            host.push(spec.clone(), None);
        }
        host
    }

    fn push(&mut self, spec: WindowSpec, popped_from: Option<HostId>) -> HostId {
        let id = self.next;
        self.next = self.next.saturating_add(1);
        self.windows.push(Hosted {
            id,
            spec,
            solo: None,
            popped_from,
            window: None,
        });
        id
    }

    pub fn get(&self, id: HostId) -> Option<&Hosted> {
        self.windows.iter().find(|w| w.id == id)
    }

    pub fn get_mut(&mut self, id: HostId) -> Option<&mut Hosted> {
        self.windows.iter_mut().find(|w| w.id == id)
    }

    /// The ids of every window, in opening order.
    pub fn ids(&self) -> Vec<HostId> {
        self.windows.iter().map(|w| w.id).collect()
    }

    /// Whether `pane` may leave `from`: it is there, and it is not the
    /// window's last pane — an empty window is a window nobody can
    /// dock anything back into.
    pub fn can_pop_out(&self, from: HostId, pane: PaneKind) -> bool {
        self.get(from).is_some_and(|w| {
            let panes = w.panes();
            panes.contains(&pane) && panes.len() > 1
        })
    }

    /// Moves `pane` out of `from` into a new window of its own, on the
    /// same monitor and view, docked to the centre half. Returns the new
    /// window's host id, or `None` when the pane cannot leave.
    // r[impl studio.windows.multiple] - pop out: the pane leaves one window for a new one
    // r[impl studio.dock.tabs-are-handles] - "Detach to new window", and a drop off every pane
    pub fn pop_out(&mut self, from: HostId, pane: PaneKind) -> Option<HostId> {
        if !self.can_pop_out(from, pane) {
            return None;
        }
        let origin = self.get_mut(from)?;
        origin.edit(|t| {
            t.remove(pane);
        });
        let spec = WindowSpec {
            monitor: origin.spec.monitor.clone(),
            placement: Placement::Docked {
                region: Region::Centre,
                fraction: 0.5,
            },
            tree: DockNode::tab(pane),
            view: origin.spec.view,
            parked: None,
        };
        Some(self.push(spec, Some(from)))
    }

    /// `pane` leaves `from` for `to`, as a tab in its first leaf.
    pub fn move_pane(&mut self, from: HostId, pane: PaneKind, to: HostId) -> bool {
        if from == to || !self.can_pop_out(from, pane) || self.get(to).is_none() {
            return false;
        }
        // Both lookups are already guaranteed by the checks above
        // (`can_pop_out` proved `from`, `self.get(to)` proved `to`); the
        // `else` arms are unreachable in practice, but `get over []`
        // applies here too — this is state, not a fixture, but the same
        // "never panic on a lookup" idiom holds.
        let Some(origin) = self.get_mut(from) else {
            return false;
        };
        origin.edit(|t| {
            t.remove(pane);
        });
        let Some(dest) = self.get_mut(to) else {
            return false;
        };
        dest.edit(|t| t.adopt(pane));
        true
    }

    /// Edit a window's tree in place.
    pub fn edit(&mut self, id: HostId, f: impl FnOnce(&mut DockNode)) -> bool {
        self.get_mut(id).is_some_and(|w| {
            w.edit(f);
            true
        })
    }

    pub fn solo(&mut self, id: HostId, pane: PaneKind) -> bool {
        let Some(w) = self.get_mut(id) else {
            return false;
        };
        let mut dock = w.dock();
        let ok = dock.solo(pane);
        w.set_dock(dock);
        ok
    }

    pub fn restore(&mut self, id: HostId) {
        if let Some(w) = self.get_mut(id) {
            let mut dock = w.dock();
            dock.restore();
            w.set_dock(dock);
        }
    }

    /// Take `pane` out of the window for good. True when the window is
    /// now empty and should close (the launch window keeps its last
    /// pane instead).
    pub fn close_pane(&mut self, id: HostId, pane: PaneKind) -> bool {
        let is_launch = self.windows.first().is_some_and(|w| w.id == id);
        let Some(w) = self.get_mut(id) else {
            return false;
        };
        if is_launch && w.panes().len() <= 1 {
            return false;
        }
        w.edit(|t| {
            t.remove(pane);
        });
        w.panes().is_empty()
    }

    /// Re-lay a window's panes on a preset.
    // r[impl studio.dock.presets]
    pub fn apply_preset(&mut self, id: HostId, preset: Preset) -> bool {
        let Some(w) = self.get_mut(id) else {
            return false;
        };
        let panes = w.panes();
        w.set_dock(DockState::new(preset.build(&panes)));
        true
    }

    /// A window is gone. A popped-out window's panes return to where
    /// they came from (or to the first remaining window, if the origin
    /// has closed since); windows popped out of this one become
    /// free-standing. Returns the panes that were re-homed.
    // r[impl studio.windows.multiple] - dock back: closing a popped window returns its panes
    pub fn window_closed(&mut self, id: HostId) -> Vec<PaneKind> {
        let Some(index) = self.windows.iter().position(|w| w.id == id) else {
            return Vec::new();
        };
        let closed = self.windows.remove(index);
        for w in &mut self.windows {
            if w.popped_from == Some(id) {
                w.popped_from = None;
            }
        }
        let Some(origin) = closed.popped_from else {
            return Vec::new();
        };
        let target = match self.get_mut(origin) {
            Some(w) => Some(w),
            None => self.windows.first_mut(),
        };
        let Some(target) = target else {
            return Vec::new();
        };
        let mut returned = Vec::new();
        for pane in closed.panes() {
            if !target.panes().contains(&pane) {
                target.edit(|t| t.adopt(pane));
                returned.push(pane);
            }
        }
        returned
    }

    /// The layout as it stands, for "save layout".
    pub fn layout(&self) -> Layout {
        Layout {
            windows: self.windows.iter().map(Hosted::persisted).collect(),
        }
    }
}

/// The process's host, and a counter every change bumps so each
/// window's root can notice from its own runtime.
pub static HOST: Mutex<Option<Host>> = Mutex::new(None);
static VERSION: AtomicU64 = AtomicU64::new(0);

/// Recovers a poisoned host mutex rather than propagating the panic
/// that poisoned it. The lock only guards in-memory window layout, not
/// the show itself; a stale or half-updated layout after some other
/// panic is a UI glitch, and taking every window down with it — the
/// crash that follows from `.expect()`ing this lock — is a strictly
/// worse outcome for an operator mid-show.
fn lock_host() -> std::sync::MutexGuard<'static, Option<Host>> {
    HOST.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub fn install(host: Host) {
    *lock_host() = Some(host);
    bump();
}

fn bump() {
    VERSION.fetch_add(1, Ordering::SeqCst);
}

/// Runs `f` on the host, if one is installed, and marks a change.
pub fn with_host<R>(f: impl FnOnce(&mut Host) -> R) -> Option<R> {
    let mut guard = lock_host();
    let out = guard.as_mut().map(f);
    drop(guard);
    bump();
    out
}

pub fn read_host<R>(f: impl FnOnce(&Host) -> R) -> Option<R> {
    lock_host().as_ref().map(f)
}

/// Edit one window's tree and show the result in this window at once,
/// not on the next poll — a splitter follows the hand.
pub fn edit_tree(host: HostId, f: impl FnOnce(&mut DockNode)) {
    with_host(|h| h.edit(host, f));
    refresh(host);
}

/// The snapshot signal of the window whose component is running, so an
/// edit made from a handler redraws before the poll comes round.
#[derive(Clone, Copy)]
struct Refresh(Signal<Option<Snapshot>>);

pub fn refresh(host: HostId) {
    if let Some(Refresh(mut snap)) = try_consume_context::<Refresh>() {
        let next = snapshot(host);
        if next != snap() {
            snap.set(next);
        }
    }
}

/// What a window's root draws from: its spec and whether it is a
/// pop-out. `None` once the window has been closed out of the host.
#[derive(Debug, Clone, PartialEq)]
struct Snapshot {
    spec: WindowSpec,
    solo: bool,
    popped: bool,
}

fn snapshot(host: HostId) -> Option<Snapshot> {
    read_host(|h| {
        h.get(host).map(|w| Snapshot {
            spec: w.spec.clone(),
            solo: w.solo.is_some(),
            popped: w.popped_from.is_some(),
        })
    })
    .flatten()
}

/// Whether the launch window should keep the legacy `IGNITION_MONITOR`
/// placement — true when no operator layout is in force.
pub static LEGACY_PLACEMENT: AtomicU64 = AtomicU64::new(0);

/// Opens the OS window for a hosted window that does not have one yet.
/// The launch window is made by `launch_cfg_with_props`; every other
/// one goes through here, from inside a component (the proxy is a root
/// context of every window).
// r[impl studio.windows.implementation] - `open_window` from the vendored dioxus-native
pub fn open(host: HostId) {
    let Some(spec) = snapshot(host).map(|s| s.spec) else {
        return;
    };
    let attributes = attributes_for(&spec);
    let on_closed: Box<dyn FnOnce() + Send + Sync> = Box::new(move || {
        let returned = with_host(|h| h.window_closed(host)).unwrap_or_default();
        tracing::info!(host, ?returned, "studio: window closed");
    });
    let opened = dioxus_native::open_window_with_props(
        attributes,
        WindowRoot,
        WindowRootProps { host },
        Some(on_closed),
    );
    // The id arrives once the event loop has made the window; it is
    // recorded from inside that window's root (`use_window().id()`),
    // so nothing has to wait here.
    drop(opened);
}

/// The attributes a new window is created with: its title (the panels
/// it hosts), the app id a compositor rule keys on, and a starting size
/// that is the docked region when the monitor is known.
// r[impl studio.windows.wayland] - app id + title are what a KWin rule matches
fn attributes_for(spec: &WindowSpec) -> dioxus_native::WindowAttributes {
    use dioxus_native::winit::dpi::LogicalSize;
    with_app_id(
        dioxus_native::WindowAttributes::default()
            .with_title(spec.title())
            .with_surface_size(LogicalSize::new(1280, 800)),
    )
}

/// Stamps the studio's app id on a window's attributes — the Wayland
/// `app_id` (`KWin`'s "window class"), which is what a window rule keys
/// on. Every studio window carries it, the launch window included.
pub fn with_app_id(attrs: dioxus_native::WindowAttributes) -> dioxus_native::WindowAttributes {
    #[cfg(target_os = "linux")]
    {
        use dioxus_native::winit::platform::wayland::WindowAttributesWayland;
        attrs.with_platform_attributes(Box::new(
            WindowAttributesWayland::default().with_name(APP_ID, APP_ID),
        ))
    }
    #[cfg(not(target_os = "linux"))]
    {
        attrs
    }
}

/// The Wayland app id (and X11 class) of every studio window.
pub const APP_ID: &str = "ignition-studio";

fn monitors_of(window: &dyn dioxus_native::winit::window::Window) -> Vec<MonitorInfo> {
    window
        .available_monitors()
        .map(|m| {
            let pos = m.position().unwrap_or_default();
            // A monitor's size is its current mode's; one with no mode
            // reported has no usable size, and a zero-width monitor
            // never matches a docked region.
            let size = m
                .current_video_mode()
                .map(|mode| mode.size())
                .unwrap_or_default();
            MonitorInfo {
                name: m.name().map(|n| n.to_string()),
                x: pos.x,
                y: pos.y,
                width: size.width,
                height: size.height,
            }
        })
        .collect()
}

/// Puts a window where its spec says. Fullscreen is honoured directly;
/// a docked window gets the region's size and its title, and the
/// compositor (a `KWin` rule on app id + title — see
/// `docs/ops/kwin-window-rules.md`) does the placing. The position is
/// still asked for, which X11 honours and Wayland ignores.
// r[impl studio.windows.wayland] - fullscreen-on-monitor directly; docked = size + title, compositor places
fn apply_placement(window: &dyn dioxus_native::winit::window::Window, spec: &WindowSpec) {
    use dioxus_native::winit::dpi::{PhysicalPosition, PhysicalSize};
    use dioxus_native::winit::monitor::Fullscreen;

    window.set_title(&spec.title());
    let handles: Vec<_> = window.available_monitors().collect();
    let infos = monitors_of(window);
    let primary = window.primary_monitor().and_then(|p| {
        handles
            .iter()
            .position(|m| m.name() == p.name() && m.position() == p.position())
    });
    let index = pick_monitor_index(&infos, &spec.monitor, primary).or(primary);
    match spec.placement {
        Placement::Fullscreen => {
            let chosen = index.and_then(|i| handles.get(i).cloned());
            tracing::info!(
                monitor = ?chosen.as_ref().and_then(|m| m.name().map(|n| n.to_string())),
                title = spec.title(),
                "studio: window fullscreen"
            );
            window.set_fullscreen(Some(Fullscreen::Borderless(chosen)));
        }
        Placement::Docked { region, fraction } => {
            let Some(info) = index.and_then(|i| infos.get(i)) else {
                tracing::warn!(
                    want = spec.monitor,
                    "studio: no monitor for a docked window"
                );
                return;
            };
            let rect = docked_rect(info, region, fraction);
            tracing::info!(
                monitor = ?info.name,
                x = rect.x,
                width = rect.width,
                height = rect.height,
                title = spec.title(),
                "studio: window docked (size set; position is the compositor's)"
            );
            window.set_fullscreen(None);
            let _ = window.request_surface_size(PhysicalSize::new(rect.width, rect.height).into());
            window.set_outer_position(PhysicalPosition::new(rect.x, rect.y).into());
        }
    }
}

/// A window's whole content. One of these per OS window; `host` says
/// which tree.
#[component]
pub fn WindowRoot(host: HostId) -> Element {
    let snap = use_signal(|| snapshot(host));
    use_context_provider(|| Refresh(snap));
    // One operator for every pane in the window — favourites starred
    // in one pane show starred in the next.
    use_context_provider(|| Signal::new(crate::operators::Operator::current()));
    let window = dioxus_native::use_window();
    crate::provide_playhead();

    // Record the OS window, and place it. Once, when the window exists.
    {
        let window = window.clone();
        use_hook(move || {
            with_host(|h| {
                if let Some(w) = h.get_mut(host) {
                    w.window = Some(window.id());
                }
            });
        });
    }
    {
        let window = window.clone();
        let legacy = LEGACY_PLACEMENT.load(Ordering::SeqCst) == 1 && host == 0;
        use_effect(move || {
            if legacy {
                // No operator layout: the launch window keeps the
                // `IGNITION_MONITOR` / `IGNITION_FULLSCREEN` behaviour.
                return;
            }
            if let Some(s) = snapshot(host) {
                apply_placement(&*window, &s.spec);
            }
        });
    }
    if LEGACY_PLACEMENT.load(Ordering::SeqCst) == 1 && host == 0 {
        crate::place_window();
    }

    // Other windows change this one's tree (a pop-out closing returns
    // its pane here; "Move to" lands one). Poll the version; a few Hz
    // is plenty for something a hand did in another window.
    use_future(move || async move {
        let mut snap = snap;
        let mut seen = VERSION.load(Ordering::SeqCst);
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(150)).await;
            let now = VERSION.load(Ordering::SeqCst);
            if now != seen {
                seen = now;
                let next = snapshot(host);
                if next != snap() {
                    snap.set(next);
                }
            }
        }
    });

    let Some(current) = snap() else {
        return rsx! { div { class: "studio", "closed" } };
    };
    let popped = current.popped;

    rsx! {
        style { {ignition_live_ui::live::TOKENS_CSS} }
        style { {include_str!("studio.css")} }
        style { {ignition_live_ui::live::LIVE_CSS} }
        style { {include_str!("panel.css")} }
        style { {include_str!("dock.css")} }
        document::Stylesheet { href: crate::TAILWIND }
        ignition_live_ui::pointer::PointerRoot {
            div { class: "window",
                ModeStrip { host, view: current.spec.view, title: current.spec.title(), popped }
                crate::dock::Dock { host, tree: current.spec.tree.clone(), solo: current.solo }
            }
        }
    }
}

/// A pane's component, by kind. The ones that exist draw themselves;
/// the rest say what they will be.
// r[impl studio.panels] - one implementation per pane, whichever leaf hosts it
#[component]
pub fn PaneBody(pane: PaneKind) -> Element {
    use ignition_live_ui::library::Tab;
    use ignition_live_ui::operators::Kind;
    use ignition_live_ui::panes;
    let surface = crate::surface();
    let kind_pane = |tab: Tab| rsx! { panes::KindPane { tab, surface: surface.clone() } };
    match pane {
        PaneKind::CueList => rsx! { crate::CueList { cues: surface.cues.clone() } },
        PaneKind::Visualizer => rsx! { div { class: "viewport", crate::Viewport {} } },
        PaneKind::Transport => rsx! { crate::Transport {} },
        PaneKind::Faders => rsx! { panes::FadersPane {} },
        PaneKind::Looks => kind_pane(Tab::Kind(Kind::Look)),
        PaneKind::Macros => rsx! {
            panes::EffectsPane { surface: surface.clone(), only: panes::EffectKinds::Macros }
        },
        PaneKind::Groups => kind_pane(Tab::Kind(Kind::Group)),
        PaneKind::Colours => kind_pane(Tab::Kind(Kind::Colour)),
        PaneKind::Splits => kind_pane(Tab::Splits),
        PaneKind::Focus => kind_pane(Tab::Kind(Kind::Focus)),
        PaneKind::Effects => rsx! {
            panes::EffectsPane { surface: surface.clone(), only: panes::EffectKinds::Rig }
        },
        PaneKind::Movers => rsx! {
            panes::EffectsPane { surface: surface.clone(), only: panes::EffectKinds::Movement }
        },
        PaneKind::Tricks => kind_pane(Tab::Kind(Kind::Trick)),
        PaneKind::Bundles => kind_pane(Tab::Kind(Kind::Bundle)),
        PaneKind::Programmer => rsx! { crate::program::Programmer { surface: surface.clone() } },
        PaneKind::Library => rsx! { crate::library::Library { surface: surface.clone() } },
        PaneKind::Desk => {
            rsx! { panes::DeskPane { banks: crate::desk::load(&crate::venue_dir()) } }
        }
        // r[impl studio.video.cameras-pane] - mounted like any other pane
        PaneKind::Cameras => rsx! { ignition_live_ui::cameras::CamerasPane {} },
        // r[impl viz.programme-view] - the cut, dockable anywhere
        PaneKind::Programme => rsx! { div { class: "viewport", crate::viz_widget::Programme {} } },
        // The Setup view — `docs/spec/patch.md`.
        PaneKind::Patch => rsx! { ignition_live_ui::patch::PatchPane {} },
        PaneKind::Universes => rsx! { ignition_live_ui::patch::UniversesPane {} },
        PaneKind::FixtureTypes => rsx! { ignition_live_ui::fixtures::FixtureTypesPane {} },
        PaneKind::FixtureEditor => rsx! { ignition_live_ui::fixtures::FixtureEditorPane {} },
        other => rsx! {
            div { class: "placeholder",
                span { class: "placeholder-name", "{other.label()}" }
                span { class: "placeholder-note", "not built yet — this window will host it" }
            }
        },
    }
}

/// The strip along the top of every window: the mode (Lights is the
/// only one that exists; Graphics and Video are where the rest of the
/// spec goes), the view, and "save layout".
// r[impl studio.operators.layout] - "save layout" lives on the mode strip
#[component]
fn ModeStrip(host: HostId, view: View, title: String, popped: bool) -> Element {
    let mut saved = use_signal(String::new);
    rsx! {
        header { class: "mode-strip",
            div { class: "modes",
                span { class: "mode on", "LIGHTS" }
                span { class: "mode", title: "Graphics mode — not built yet", "GRAPHICS" }
                span { class: "mode", title: "Video mode — not built yet", "VIDEO" }
            }
            span { class: "window-title", "{title}" }
            div { class: "strip-right",
                button {
                    class: "panel-key view",
                    title: "Cycle this window through Program, Live and Setup",
                    onclick: move |_| {
                        // Program and Live share a desk; Setup has its
                        // own, so this may swap the whole dock tree —
                        // see `WindowSpec::show`.
                        let next = view.next();
                        with_host(|h| {
                            if let Some(w) = h.get_mut(host) {
                                w.spec.show(next);
                            }
                        });
                        // The viewport draws the programmer's overlays
                        // only in Program.
                        // r[impl studio.program.pick-and-gizmos] - Live has the overlays off
                        ignition_live_ui::send(ignition_live_ui::Command::ProgramView(
                            next == View::Program,
                        ));
                        refresh(host);
                    },
                    "{view.label().to_uppercase()}"
                }
                if !popped {
                    button {
                        class: "panel-key",
                        title: "Write every window's monitor, placement and panels to the operator file",
                        onclick: move |_| {
                            let name = layout::selected_operator()
                                .unwrap_or_else(crate::operators::current_name);
                            let layout = read_host(Host::layout).flatten_layout();
                            match layout::save(&name, &layout) {
                                Ok(path) => {
                                    tracing::info!(path = %path.display(), "studio: layout saved");
                                    saved.set(format!("saved {}", path.display()));
                                }
                                Err(e) => {
                                    tracing::warn!(error = %e, "studio: layout not saved");
                                    saved.set(format!("not saved: {e}"));
                                }
                            }
                        },
                        "SAVE LAYOUT"
                    }
                }
                span { class: "saved", "{saved}" }
            }
        }
    }
}

trait FlattenLayout {
    fn flatten_layout(self) -> Layout;
}
impl FlattenLayout for Option<Layout> {
    fn flatten_layout(self) -> Layout {
        self.unwrap_or_else(Layout::default_single_window)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dock::Axis;

    fn three_windows() -> Host {
        Host::from_layout(&layout::parse(
            r#"{"windows":[
              {"monitor":"DP-5","placement":"fullscreen","tree":{"tabs":{"panes":["cue_list"]}},"view":"live"},
              {"monitor":"DP-4","placement":{"docked":{"region":"right","fraction":0.5}},
               "tree":{"split":{"axis":"col","ratios":[0.8,0.2],"children":[
                 {"tabs":{"panes":["visualizer"]}},{"tabs":{"panes":["transport"]}}]}}},
              {"monitor":"DP-3","placement":"fullscreen","panels":["busking","palettes","library"],"view":"live"}
            ]}"#,
        )
        .unwrap())
    }

    /// Multi-window is the patched `dioxus-native`, not a fork of Blitz
    /// and not the one-window-per-monitor fallback.
    ///
    /// The rule names a specific route because the alternatives are
    /// expensive in different ways: forking Blitz means carrying a
    /// renderer, and the fallback means a process that cannot open a
    /// window an operator asks for after launch. What makes the chosen
    /// route work is a runtime embedder event reaching the shell's
    /// window map, and a `VirtualDom` per window — so those are what is
    /// checked, in the vendored crate the workspace actually builds
    /// against.
    ///
    /// r[verify studio.windows.implementation]
    #[test]
    fn windows_come_from_the_patched_dioxus_native_at_runtime() {
        let manifest = include_str!("../../../Cargo.toml");
        assert!(
            manifest.contains("dioxus-native = { path = \"crates/dioxus-native-vendored\" }"),
            "the workspace no longer builds against the vendored dioxus-native, so this \
             studio's windows come from somewhere else"
        );

        let vendored =
            include_str!("../../../crates/dioxus-native-vendored/src/dioxus_application.rs");
        assert!(
            vendored.contains("NewWindow {"),
            "the runtime embedder event the patch adds is gone"
        );
        assert!(
            vendored.contains("IGNITION PATCH"),
            "the vendored crate carries no marked patch, so it is either upstream or a fork"
        );

        // And the studio opens its windows through it, rather than
        // making them all at launch.
        let source = include_str!("windows.rs");
        assert!(
            source.contains("dioxus_native::open_window_with_props("),
            "windows are not opened through the patched hook"
        );
    }

    /// Every stylesheet reaches the document as a `<style>` block.
    ///
    /// `document::Stylesheet` is inert under Blitz (Dioxus Native): a
    /// sheet delivered only through that link never arrives. Nothing
    /// catches it — the app compiles, the suite passes, and the desk
    /// comes up with no layout at all, the `.viz` element collapsed to
    /// zero size and the visualizer silently never initialising. That
    /// is exactly what happened when these five were briefly replaced
    /// by an `@import` into `tailwind.css` and one link.
    ///
    /// The Tailwind link stays, for the utility classes; it is simply
    /// not allowed to be the only thing delivering a sheet.
    #[test]
    fn every_stylesheet_is_injected_as_a_style_block() {
        let source = include_str!("windows.rs");
        let injected: Vec<&str> = source
            .lines()
            .map(str::trim)
            .filter(|l| l.starts_with("style {"))
            .collect();
        for sheet in [
            "TOKENS_CSS",
            "studio.css",
            "LIVE_CSS",
            "panel.css",
            "dock.css",
        ] {
            assert!(
                injected.iter().any(|l| l.contains(sheet)),
                "{sheet} is not injected as a <style> block; under Blitz that \
                 means it does not arrive at all. Injected: {injected:?}"
            );
        }
    }

    /// The chrome is not selectable, and that is a crash fix rather than
    /// a matter of taste: a text selection whose anchor is removed by a
    /// re-render makes blitz compare two nodes with no common root, and
    /// `compare_document_order` underflows into a `usize::MAX` index.
    /// Inputs keep their selection, or the search fields stop working.
    #[test]
    fn the_chrome_cannot_be_drag_selected_but_inputs_can() {
        let css = include_str!("studio.css");
        assert!(
            css.contains(".window, .window * { -webkit-user-select: none; user-select: none; }"),
            "the window is drag-selectable; a stale selection anchor panics blitz"
        );
        let inputs = css
            .split(".window input, .window textarea")
            .nth(1)
            .expect("inputs are exempted");
        assert!(
            inputs.starts_with(" { -webkit-user-select: text; user-select: text; }"),
            "a search field you cannot select in is a search field you cannot edit"
        );
    }

    /// r[verify studio.windows.multiple]
    #[test]
    fn a_pane_pops_out_into_its_own_window_and_comes_back_on_close() {
        let mut host = three_windows();
        let ids = host.ids();
        assert_eq!(ids, vec![0, 1, 2]);

        let new = host
            .pop_out(1, PaneKind::Transport)
            .expect("transport can leave");
        // The split collapsed around the one pane left.
        assert_eq!(
            host.get(1).unwrap().spec.tree,
            DockNode::tab(PaneKind::Visualizer)
        );
        let popped = host.get(new).unwrap();
        assert_eq!(popped.spec.tree, DockNode::tab(PaneKind::Transport));
        assert_eq!(popped.popped_from, Some(1));
        // Same monitor and view as where it came from; docked, not
        // fullscreen, so it does not cover its origin.
        assert_eq!(popped.spec.monitor, "DP-4");
        assert_eq!(popped.spec.view, View::Program);
        assert!(matches!(popped.spec.placement, Placement::Docked { .. }));

        let returned = host.window_closed(new);
        assert_eq!(returned, vec![PaneKind::Transport]);
        // Home again, as a tab beside the visualizer.
        assert_eq!(
            host.get(1).unwrap().spec.tree,
            DockNode::Tabs {
                panes: vec![PaneKind::Visualizer, PaneKind::Transport],
                active: 1
            }
        );
        assert!(host.get(new).is_none());
        assert_eq!(host.windows.len(), 3);
    }

    /// r[verify studio.windows.multiple]
    #[test]
    fn the_last_pane_cannot_leave_and_a_missing_one_cannot_either() {
        let mut host = three_windows();
        assert!(!host.can_pop_out(0, PaneKind::CueList));
        assert_eq!(host.pop_out(0, PaneKind::CueList), None);
        assert_eq!(host.pop_out(0, PaneKind::Lyrics), None);
        assert_eq!(host.pop_out(99, PaneKind::CueList), None);
        assert_eq!(host.windows.len(), 3);
    }

    /// r[verify studio.windows.multiple]
    #[test]
    fn a_pop_out_whose_origin_has_closed_returns_to_the_first_window() {
        let mut host = three_windows();
        let new = host.pop_out(2, PaneKind::Library).unwrap();
        // The origin goes away first.
        assert!(host.window_closed(2).is_empty());
        // The pop-out is now free-standing …
        assert_eq!(host.get(new).unwrap().popped_from, None);
        // … so closing it re-homes nothing; its pane simply closes.
        assert!(host.window_closed(new).is_empty());
        assert_eq!(host.ids(), vec![0, 1]);

        // Whereas a pop-out that still remembers its origin, when that
        // origin is gone but another window remains, lands there.
        let mut host = three_windows();
        let new = host.pop_out(2, PaneKind::Splits).unwrap();
        host.get_mut(new).unwrap().popped_from = Some(2);
        host.windows.retain(|w| w.id != 2);
        assert_eq!(host.window_closed(new), vec![PaneKind::Splits]);
        assert!(host.get(0).unwrap().spec.tree.contains(PaneKind::Splits));
    }

    /// r[verify studio.windows.multiple]
    #[test]
    fn closing_a_window_does_not_duplicate_a_pane_already_home() {
        let mut host = three_windows();
        let new = host.pop_out(2, PaneKind::Splits).unwrap();
        // Something put Splits back by hand in the meantime.
        host.get_mut(2).unwrap().edit(|t| t.adopt(PaneKind::Splits));
        assert!(host.window_closed(new).is_empty());
        let panes = host.get(2).unwrap().panes();
        assert_eq!(panes.iter().filter(|p| **p == PaneKind::Splits).count(), 1);
    }

    /// r[verify studio.dock.tabs-are-handles]
    #[test]
    fn move_to_another_window_tabs_the_pane_into_it() {
        let mut host = three_windows();
        assert!(host.move_pane(1, PaneKind::Transport, 0));
        assert_eq!(
            host.get(1).unwrap().spec.tree,
            DockNode::tab(PaneKind::Visualizer)
        );
        assert_eq!(
            host.get(0).unwrap().panes(),
            vec![PaneKind::CueList, PaneKind::Transport]
        );
        // The last pane stays; a window cannot be emptied by a move.
        assert!(!host.move_pane(1, PaneKind::Visualizer, 0));
        assert!(!host.move_pane(0, PaneKind::CueList, 0));
        assert!(!host.move_pane(0, PaneKind::CueList, 77));
    }

    /// r[verify studio.dock]
    #[test]
    fn solo_is_drawn_but_the_whole_tree_is_saved_and_edits_leave_solo() {
        let mut host = three_windows();
        assert!(host.solo(1, PaneKind::Transport));
        assert_eq!(
            host.get(1).unwrap().spec.tree,
            DockNode::tab(PaneKind::Transport)
        );
        assert!(matches!(
            host.layout().windows[1].tree,
            DockNode::Split {
                axis: Axis::Col,
                ..
            }
        ));
        // An edit lands on the real tree and ends the solo.
        assert!(host.edit(1, |t| {
            t.adopt(PaneKind::Output);
        }));
        let w = host.get(1).unwrap();
        assert!(w.solo.is_none());
        assert_eq!(
            w.panes(),
            vec![PaneKind::Visualizer, PaneKind::Output, PaneKind::Transport]
        );
        host.solo(1, PaneKind::Visualizer);
        host.restore(1);
        assert_eq!(host.get(1).unwrap().panes().len(), 3);
    }

    /// r[verify studio.dock.presets]
    #[test]
    fn a_preset_relays_the_panes_the_window_has() {
        let mut host = three_windows();
        assert!(host.apply_preset(1, Preset::TwoColumns));
        let tree = &host.get(1).unwrap().spec.tree;
        assert!(matches!(
            tree,
            DockNode::Split {
                axis: Axis::Row,
                ..
            }
        ));
        assert_eq!(
            tree.panes(),
            vec![PaneKind::Visualizer, PaneKind::Transport]
        );
        assert!(host.apply_preset(0, Preset::Console));
        assert!(host.get(0).unwrap().spec.tree.contains(PaneKind::Faders));
        assert!(host.get(0).unwrap().spec.tree.contains(PaneKind::CueList));
    }

    /// r[verify studio.dock]
    #[test]
    fn closing_panes_empties_a_pop_out_but_never_the_launch_window() {
        let mut host = three_windows();
        assert!(!host.close_pane(0, PaneKind::CueList));
        assert!(host.get(0).unwrap().spec.tree.contains(PaneKind::CueList));
        assert!(
            !host.close_pane(1, PaneKind::Transport),
            "one pane still there"
        );
        assert!(
            host.close_pane(1, PaneKind::Visualizer),
            "now empty: close the window"
        );
    }

    /// r[verify studio.operators.layout]
    #[test]
    fn the_host_writes_back_the_layout_it_was_given() {
        let host = three_windows();
        let layout = host.layout();
        assert_eq!(layout.windows.len(), 3);
        assert_eq!(
            layout.windows[1].panes(),
            vec![PaneKind::Visualizer, PaneKind::Transport]
        );
        // And a change shows in the save.
        let mut host = host;
        host.pop_out(2, PaneKind::Library).unwrap();
        assert_eq!(host.layout().windows.len(), 4);
        assert_eq!(
            host.layout().windows[3].tree,
            DockNode::tab(PaneKind::Library)
        );
    }
}
