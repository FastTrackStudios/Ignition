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

// r[impl studio.windows.multiple] - any number of windows, each hosting any panels, one engine
// r[impl studio.panels] - the host: a panel is drawn by whichever window lists it

use crate::layout::{
    self, Layout, MonitorInfo, Panel, Placement, Region, View, WindowSpec, docked_rect,
    pick_monitor_index,
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
    /// The window a pop-out came from; its panel goes back there when
    /// this one closes.
    pub popped_from: Option<HostId>,
    pub window: Option<WindowId>,
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
        self.next += 1;
        self.windows.push(Hosted {
            id,
            spec,
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

    /// Whether `panel` may leave `from`: it is there, and it is not the
    /// window's last panel — an empty window is a window nobody can
    /// dock anything back into.
    pub fn can_pop_out(&self, from: HostId, panel: Panel) -> bool {
        self.get(from)
            .is_some_and(|w| w.spec.panels.contains(&panel) && w.spec.panels.len() > 1)
    }

    /// Moves `panel` out of `from` into a new window of its own, on the
    /// same monitor and view, docked to the centre half. Returns the new
    /// window's host id, or `None` when the panel cannot leave.
    // r[impl studio.windows.multiple] - pop out: the panel leaves one window for a new one
    pub fn pop_out(&mut self, from: HostId, panel: Panel) -> Option<HostId> {
        if !self.can_pop_out(from, panel) {
            return None;
        }
        let origin = self.get_mut(from)?;
        origin.spec.panels.retain(|p| *p != panel);
        let spec = WindowSpec {
            monitor: origin.spec.monitor.clone(),
            placement: Placement::Docked {
                region: Region::Centre,
                fraction: 0.5,
            },
            panels: vec![panel],
            view: origin.spec.view,
        };
        Some(self.push(spec, Some(from)))
    }

    /// A window is gone. A popped-out window's panels return to where
    /// they came from (or to the first remaining window, if the origin
    /// has closed since); windows popped out of this one become
    /// free-standing. Returns the panels that were re-homed.
    // r[impl studio.windows.multiple] - dock back: closing a popped window returns its panel
    pub fn window_closed(&mut self, id: HostId) -> Vec<Panel> {
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
        for panel in closed.spec.panels {
            if !target.spec.panels.contains(&panel) {
                target.spec.panels.push(panel);
                returned.push(panel);
            }
        }
        returned
    }

    /// The layout as it stands, for "save layout".
    pub fn layout(&self) -> Layout {
        Layout {
            windows: self.windows.iter().map(|w| w.spec.clone()).collect(),
        }
    }
}

/// The process's host, and a counter every change bumps so each
/// window's root can notice from its own runtime.
pub static HOST: Mutex<Option<Host>> = Mutex::new(None);
static VERSION: AtomicU64 = AtomicU64::new(0);

pub fn install(host: Host) {
    *HOST.lock().expect("host mutex") = Some(host);
    bump();
}

fn bump() {
    VERSION.fetch_add(1, Ordering::SeqCst);
}

/// Runs `f` on the host, if one is installed, and marks a change.
pub fn with_host<R>(f: impl FnOnce(&mut Host) -> R) -> Option<R> {
    let mut guard = HOST.lock().expect("host mutex");
    let out = guard.as_mut().map(f);
    drop(guard);
    bump();
    out
}

fn read_host<R>(f: impl FnOnce(&Host) -> R) -> Option<R> {
    HOST.lock().expect("host mutex").as_ref().map(f)
}

/// What a window's root draws from: its spec and whether it is a
/// pop-out. `None` once the window has been closed out of the host.
#[derive(Debug, Clone, PartialEq)]
struct Snapshot {
    spec: WindowSpec,
    popped: bool,
}

fn snapshot(host: HostId) -> Option<Snapshot> {
    read_host(|h| {
        h.get(host).map(|w| Snapshot {
            spec: w.spec.clone(),
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
/// `app_id` (KWin's "window class"), which is what a window rule keys
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
/// compositor (a KWin rule on app id + title — see
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
/// which panels.
#[component]
pub fn WindowRoot(host: HostId) -> Element {
    let mut snap = use_signal(|| snapshot(host));
    let window = dioxus_native::use_window();

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

    // Other windows change this one's panel list (a pop-out closing
    // returns its panel here). Poll the version; a few Hz is plenty for
    // something a hand did.
    use_future(move || async move {
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
    let surface = crate::surface();
    let popped = current.popped;
    let can_pop = current.spec.panels.len() > 1 && !popped;

    rsx! {
        style { {include_str!("studio.css")} }
        style { {crate::live::LIVE_CSS} }
        style { {PANEL_CSS} }
        document::Stylesheet { href: crate::TAILWIND }
        div { class: "window",
            ModeStrip { host, view: current.spec.view, title: current.spec.title(), popped }
            if is_classic(&current.spec.panels) {
                // Today's arrangement, kept exactly: cue list down the
                // left, transport over the visualizer over the desk.
                div { class: "studio",
                    PanelFrame { host, panel: Panel::CueList, can_pop, popped,
                        crate::CueList { cues: surface.cues.clone() }
                    }
                    main { class: "stage",
                        PanelFrame { host, panel: Panel::Transport, can_pop, popped, crate::Transport {} }
                        PanelFrame { host, panel: Panel::Visualizer, can_pop, popped,
                            div { class: "viewport", crate::Viewport {} }
                        }
                        PanelFrame { host, panel: Panel::Busking, can_pop, popped,
                            crate::live::Views { surface: surface.clone() }
                        }
                    }
                }
            } else {
                div { class: if current.spec.panels.contains(&Panel::Visualizer) { "panels column" } else { "panels row" },
                    for panel in current.spec.panels.iter().copied() {
                        PanelFrame { key: "{panel.key()}", host, panel, can_pop, popped,
                            PanelBody { panel }
                        }
                    }
                }
            }
        }
    }
}

/// Whether a panel set is the original single-window studio.
fn is_classic(panels: &[Panel]) -> bool {
    panels.len() == 4
        && [
            Panel::CueList,
            Panel::Transport,
            Panel::Visualizer,
            Panel::Busking,
        ]
        .iter()
        .all(|p| panels.contains(p))
}

/// A panel's component, by name. The ones that exist draw themselves;
/// the rest say what they will be.
#[component]
fn PanelBody(panel: Panel) -> Element {
    let surface = crate::surface();
    match panel {
        Panel::CueList => rsx! { crate::CueList { cues: surface.cues.clone() } },
        Panel::Visualizer => rsx! { div { class: "viewport", crate::Viewport {} } },
        Panel::Transport => rsx! { crate::Transport {} },
        // The desk: the Live / Program views over the busking surface.
        Panel::Busking => rsx! { crate::live::Views { surface: surface.clone() } },
        Panel::Library => rsx! { crate::library::Library { surface: surface.clone() } },
        Panel::Palettes => rsx! { crate::live::Palettes { surface: surface.clone() } },
        Panel::Programmer => rsx! { crate::program::Programmer { surface: surface.clone() } },
        other => rsx! {
            div { class: "placeholder",
                span { class: "placeholder-name", "{other.label()}" }
                span { class: "placeholder-note", "not built yet — this window will host it" }
            }
        },
    }
}

/// The frame around a panel: its name and the pop-out (or dock-back)
/// key, then the panel itself.
#[component]
fn PanelFrame(
    host: HostId,
    panel: Panel,
    can_pop: bool,
    popped: bool,
    children: Element,
) -> Element {
    let window = dioxus_native::use_window();
    rsx! {
        div { class: "panel {panel.key()}",
            div { class: "panel-bar",
                span { class: "panel-name", "{panel.label()}" }
                if popped {
                    button {
                        class: "panel-key",
                        title: "Dock this panel back where it came from",
                        onclick: move |_| {
                            // Closing is the return path: the window's
                            // close hook puts the panel back.
                            dioxus_native::close_window(window.id());
                        },
                        "DOCK BACK"
                    }
                } else if can_pop {
                    button {
                        class: "panel-key",
                        title: "Open this panel in its own window",
                        onclick: move |_| {
                            if let Some(Some(new)) = with_host(|h| h.pop_out(host, panel)) {
                                open(new);
                            }
                        },
                        "POP OUT"
                    }
                }
            }
            div { class: "panel-body", {children} }
        }
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
                    title: "Switch this window between the Program and Live views",
                    onclick: move |_| {
                        with_host(|h| {
                            if let Some(w) = h.get_mut(host) {
                                w.spec.view = w.spec.view.other();
                            }
                        });
                    },
                    "{view.label().to_uppercase()}"
                }
                if !popped {
                    button {
                        class: "panel-key",
                        title: "Write every window's monitor, placement and panels to the operator file",
                        onclick: move |_| {
                            let name = layout::selected_operator()
                                .unwrap_or_else(|| crate::operators::current_name());
                            let layout = read_host(|h| h.layout()).flatten_layout();
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

/// Styling for the frame this module adds around the existing panels.
/// Inline, not in `studio.css`: that file is the panels' own, and the
/// host should be removable without touching it.
const PANEL_CSS: &str = r#"
.window { display: flex; flex-direction: column; width: 100%; height: 100vh; }
.window .studio { flex: 1; min-height: 0; height: auto; }
.mode-strip { display: flex; align-items: center; gap: 14px; padding: 4px 10px;
              background: #101014; border-bottom: 1px solid #26262c; flex: 0 0 auto; }
.mode-strip .modes { display: flex; gap: 8px; }
.mode-strip .mode { font-size: 10px; letter-spacing: 0.1em; color: #55555f; padding: 3px 6px; }
.mode-strip .mode.on { color: #e8a040; border-bottom: 2px solid #e8a040; }
.mode-strip .window-title { font-size: 10px; color: #8d8d99; letter-spacing: 0.06em; }
.mode-strip .strip-right { margin-left: auto; display: flex; align-items: center; gap: 8px; }
.mode-strip .saved { font-size: 9px; color: #6a6a78; }
.panel { display: flex; flex-direction: column; min-width: 0; min-height: 0; position: relative; }
.panel-bar { display: flex; align-items: center; gap: 8px; padding: 2px 8px; height: 18px;
             background: #0e0e12; border-bottom: 1px solid #1f1f26; flex: 0 0 auto; }
.panel-name { font-size: 9px; letter-spacing: 0.1em; text-transform: uppercase; color: #6a6a78; }
.panel-key { margin-left: auto; height: 14px; padding: 0 6px; font-size: 8px; letter-spacing: 0.08em;
             border-radius: 3px; cursor: pointer; color: rgba(255,255,255,0.7);
             background: #23232e; border: 1px solid #33333f; }
.panel-key:hover { background: #2c2c3a; }
.panel-key.view { margin-left: 0; height: 18px; font-size: 9px; color: #cfe0f0; border-color: #3d5a80; background: #2c3f5a; }
.panel-body { flex: 1; min-height: 0; min-width: 0; display: flex; flex-direction: column; }
.panel-body > * { flex: 1; min-height: 0; }
.studio > .panel.cue_list { flex: 0 0 auto; }
.stage > .panel.transport, .stage > .panel.busking { flex: 0 0 auto; }
.stage > .panel.visualizer { flex: 1; min-height: 0; }
.panels { flex: 1; min-height: 0; display: flex; }
.panels.row { flex-direction: row; }
.panels.row > .panel { flex: 1; }
.panels.column { flex-direction: column; }
.panels.column > .panel { flex: 0 0 auto; }
.panels.column > .panel.visualizer { flex: 1; }
.placeholder { flex: 1; display: flex; flex-direction: column; align-items: center; justify-content: center;
               gap: 6px; background: #121216; color: #55555f; }
.placeholder-name { font-size: 14px; letter-spacing: 0.12em; text-transform: uppercase; color: #8d8d99; }
.placeholder-note { font-size: 10px; }
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn three_windows() -> Host {
        Host::from_layout(&layout::parse(
            r#"{"windows":[
              {"monitor":"DP-5","placement":"fullscreen","panels":["cue_list"],"view":"live"},
              {"monitor":"DP-4","placement":{"docked":{"region":"right","fraction":0.5}},
               "panels":["visualizer","transport"]},
              {"monitor":"DP-3","placement":"fullscreen","panels":["busking","palettes","library"],"view":"live"}
            ]}"#,
        )
        .unwrap())
    }

    /// r[verify studio.windows.multiple]
    #[test]
    fn a_panel_pops_out_into_its_own_window_and_comes_back_on_close() {
        let mut host = three_windows();
        let ids = host.ids();
        assert_eq!(ids, vec![0, 1, 2]);

        let new = host
            .pop_out(1, Panel::Transport)
            .expect("transport can leave");
        assert_eq!(host.get(1).unwrap().spec.panels, vec![Panel::Visualizer]);
        let popped = host.get(new).unwrap();
        assert_eq!(popped.spec.panels, vec![Panel::Transport]);
        assert_eq!(popped.popped_from, Some(1));
        // Same monitor and view as where it came from; docked, not
        // fullscreen, so it does not cover its origin.
        assert_eq!(popped.spec.monitor, "DP-4");
        assert_eq!(popped.spec.view, View::Program);
        assert!(matches!(popped.spec.placement, Placement::Docked { .. }));

        let returned = host.window_closed(new);
        assert_eq!(returned, vec![Panel::Transport]);
        assert_eq!(
            host.get(1).unwrap().spec.panels,
            vec![Panel::Visualizer, Panel::Transport]
        );
        assert!(host.get(new).is_none());
        assert_eq!(host.windows.len(), 3);
    }

    /// r[verify studio.windows.multiple]
    #[test]
    fn the_last_panel_cannot_leave_and_a_missing_one_cannot_either() {
        let mut host = three_windows();
        assert!(!host.can_pop_out(0, Panel::CueList));
        assert_eq!(host.pop_out(0, Panel::CueList), None);
        assert_eq!(host.pop_out(0, Panel::Lyrics), None);
        assert_eq!(host.pop_out(99, Panel::CueList), None);
        assert_eq!(host.windows.len(), 3);
    }

    /// r[verify studio.windows.multiple]
    #[test]
    fn a_pop_out_whose_origin_has_closed_returns_to_the_first_window() {
        let mut host = three_windows();
        let new = host.pop_out(2, Panel::Library).unwrap();
        // The origin goes away first.
        assert!(host.window_closed(2).is_empty());
        // The pop-out is now free-standing …
        assert_eq!(host.get(new).unwrap().popped_from, None);
        // … so closing it re-homes nothing; its panel simply closes.
        assert!(host.window_closed(new).is_empty());
        assert_eq!(host.ids(), vec![0, 1]);

        // Whereas a pop-out that still remembers its origin, when that
        // origin is gone but another window remains, lands there.
        let mut host = three_windows();
        let new = host.pop_out(2, Panel::Palettes).unwrap();
        host.get_mut(new).unwrap().popped_from = Some(2);
        host.windows.retain(|w| w.id != 2);
        assert_eq!(host.window_closed(new), vec![Panel::Palettes]);
        assert!(host.get(0).unwrap().spec.panels.contains(&Panel::Palettes));
    }

    /// r[verify studio.windows.multiple]
    #[test]
    fn closing_a_window_does_not_duplicate_a_panel_already_home() {
        let mut host = three_windows();
        let new = host.pop_out(2, Panel::Palettes).unwrap();
        // Something put Palettes back by hand in the meantime.
        host.get_mut(2).unwrap().spec.panels.push(Panel::Palettes);
        assert!(host.window_closed(new).is_empty());
        let panels = &host.get(2).unwrap().spec.panels;
        assert_eq!(panels.iter().filter(|p| **p == Panel::Palettes).count(), 1);
    }

    /// r[verify studio.operators.layout]
    #[test]
    fn the_host_writes_back_the_layout_it_was_given() {
        let host = three_windows();
        let layout = host.layout();
        assert_eq!(layout.windows.len(), 3);
        assert_eq!(
            layout.windows[1].panels,
            vec![Panel::Visualizer, Panel::Transport]
        );
        // And a change shows in the save.
        let mut host = host;
        host.pop_out(2, Panel::Library).unwrap();
        assert_eq!(host.layout().windows.len(), 4);
        assert_eq!(host.layout().windows[3].panels, vec![Panel::Library]);
    }

    #[test]
    fn classic_is_the_original_four_in_any_order() {
        assert!(is_classic(&[
            Panel::Busking,
            Panel::CueList,
            Panel::Visualizer,
            Panel::Transport
        ]));
        assert!(!is_classic(&[
            Panel::CueList,
            Panel::Visualizer,
            Panel::Transport
        ]));
        assert!(!is_classic(&[
            Panel::CueList,
            Panel::Visualizer,
            Panel::Transport,
            Panel::Library
        ]));
    }
}
