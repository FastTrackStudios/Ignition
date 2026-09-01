//! The component that draws a tree, and the machinery on top of it.
//!
//! Blitz (Dioxus Native) has no pointer capture, so a drag is a
//! `mousedown` on the tab and `mousemove` / `mouseup` on the dock root,
//! which everything under the pointer bubbles to. Every rectangle this
//! reasons about comes from [`super::geometry`] rather than from the
//! document.

use super::*;
use crate::windows::{self, HostId};
use dioxus::prelude::*;

/// The height of the mode strip above the dock, in CSS pixels —
/// what turns a window-relative pointer into a dock-relative one.
/// `.mode-strip` in `windows.rs` fixes the same number.
pub const STRIP: f32 = 28.0;

/// A tab being dragged.
#[derive(Debug, Clone, PartialEq)]
struct Drag {
    pane: PaneKind,
    start: (f32, f32),
    at: (f32, f32),
    /// Past the dead zone: the ghost shows and a drop counts.
    active: bool,
    /// The tab bar the pointer is over, and the index there.
    hover_tab: Option<(Path, usize)>,
    target: Option<(Path, DropZone)>,
}

#[derive(Debug, Clone, PartialEq)]
struct SplitDrag {
    path: Path,
    index: usize,
}

#[derive(Debug, Clone, PartialEq)]
enum MenuKind {
    Tab { pane: PaneKind, path: Path },
    Bar { path: Path },
}

#[derive(Debug, Clone, PartialEq)]
struct Menu {
    at: (f32, f32),
    kind: MenuKind,
}

/// The window's whole dock: the tree from the host, plus the drag,
/// splitter and menu state that lives only in this window.
#[component]
pub fn Dock(host: HostId, tree: DockNode, solo: bool) -> Element {
    let window = dioxus_native::use_window();
    let mut drag = use_signal(|| Option::<Drag>::None);
    let mut splitting = use_signal(|| Option::<SplitDrag>::None);
    let mut menu = use_signal(|| Option::<Menu>::None);

    let size = window.surface_size();
    let scale = window.scale_factor() as f32;
    let dock_rect = Rect::new(
        0.0,
        0.0,
        size.width as f32 / scale,
        (size.height as f32 / scale - STRIP).max(0.0),
    );
    let placed = layout(&tree, dock_rect);
    let placed_for_move = placed.clone();
    let placed_for_down = placed.clone();

    // Every pointer move, from a tab, a bar or the root: the ghost
    // follows, the zone updates, a splitter drags.
    let mut on_move = move |x: f32, y: f32, hover: Option<(Path, usize)>| {
        let (x, y) = (x, y - STRIP);
        if let Some(s) = splitting() {
            if let Some(sp) = placed_for_move.splitter(&s.path, s.index) {
                let fraction = fraction_for(sp, x, y);
                windows::edit_tree(host, |t| {
                    t.set_split(&s.path, s.index, fraction);
                });
            }
            return;
        }
        let Some(mut d) = drag() else {
            return;
        };
        d.at = (x, y);
        if !d.active {
            let dx = x - d.start.0;
            let dy = y - d.start.1;
            if dx * dx + dy * dy < 16.0 {
                drag.set(Some(d));
                return;
            }
            d.active = true;
        }
        if let Some(h) = hover {
            d.hover_tab = Some(h);
        }
        d.target = placed_for_move.leaf_at(x, y).and_then(|leaf| {
            let zone = if y < leaf.rect.y + TAB_BAR {
                let index = match &d.hover_tab {
                    Some((p, i)) if *p == leaf.path => *i,
                    _ => leaf.panes.len(),
                };
                DropZone::TabBar(index)
            } else {
                hit_test(leaf.rect, &[], x, y)?
            };
            Some((leaf.path.clone(), zone))
        });
        drag.set(Some(d));
    };
    let mut on_move_from_child = on_move.clone();

    let mut on_up = move || {
        splitting.set(None);
        let Some(d) = drag.take() else {
            return;
        };
        if !d.active {
            return;
        }
        match d.target {
            Some((path, zone)) => {
                windows::edit_tree(host, |t| {
                    t.drop_pane(d.pane, &path, zone);
                });
            }
            None => {
                // Off every pane: the tab becomes a window.
                if let Some(Some(new)) = windows::with_host(|h| h.pop_out(host, d.pane)) {
                    windows::open(new);
                }
            }
        }
    };

    let ghost = drag().filter(|d| d.active);
    let preview = ghost.as_ref().and_then(|d| {
        let (path, zone) = d.target.as_ref()?;
        let leaf = placed.leaf(path)?;
        Some((zone_rect(leaf.rect, *zone), zone.label()))
    });
    let root_class = if ghost.is_some() {
        "dock dragging"
    } else if splitting().is_some() {
        "dock splitting"
    } else {
        "dock"
    };

    rsx! {
        div {
            class: "{root_class}",
            onmousemove: move |e| {
                let p = e.data.client_coordinates();
                on_move(p.x as f32, p.y as f32, None);
            },
            onmouseup: move |_| on_up(),
            onmouseleave: move |_| {
                splitting.set(None);
                drag.set(None);
            },
            onmousedown: move |e| {
                menu.set(None);
                // A press within the grab band of a splitter drags
                // it, whatever element is under the pointer — the
                // bar is two pixels and nobody can hit that.
                if e.data.trigger_button() != Some(dioxus::html::input_data::MouseButton::Primary) {
                    return;
                }
                let p = e.data.client_coordinates();
                if let Some(sp) = placed_for_down.splitter_at(p.x as f32, p.y as f32 - STRIP) {
                    splitting.set(Some(SplitDrag { path: sp.path.clone(), index: sp.index }));
                }
            },
            Node {
                host,
                node: tree.clone(),
                path: Vec::new(),
                solo,
                drag,
                splitting,
                menu,
                on_move: EventHandler::new(move |(x, y, hover): MoveTo| on_move_from_child(x, y, hover)),
            }
            if let Some((rect, label)) = preview {
                div {
                    class: "drop-preview",
                    style: "left: {rect.x}px; top: {rect.y}px; width: {rect.w}px; height: {rect.h}px;",
                    span { "{label}" }
                }
            }
            if let Some(d) = ghost {
                if d.target.is_none() {
                    div { class: "drop-detach", "release to open a new window" }
                }
                div {
                    class: "tab-ghost",
                    style: "left: {d.at.0 + 12.0}px; top: {d.at.1 + 8.0}px;",
                    "{d.pane.label()}"
                }
            }
            if let Some(m) = menu() {
                ContextMenu { host, menu_at: m.at, kind: m.kind, tree: tree.clone(), solo, close: move |_| menu.set(None) }
            }
        }
    }
}

/// Where the pointer is, and which tab strip it is over.
///
/// `(x, y)` in the host window's coordinates, plus the tab strip the
/// pointer is inside and the insertion index within it — `None` when
/// the move came from the root rather than from a strip. Named
/// because it is threaded through every `Node` in the tree and a
/// bare triple says nothing at the receiving end.
type MoveTo = (f32, f32, Option<(Path, usize)>);

#[component]
fn Node(
    host: HostId,
    node: DockNode,
    path: Path,
    solo: bool,
    drag: Signal<Option<Drag>>,
    splitting: Signal<Option<SplitDrag>>,
    menu: Signal<Option<Menu>>,
    on_move: EventHandler<MoveTo>,
) -> Element {
    match node {
        DockNode::Split {
            axis,
            ratios,
            children,
        } => {
            let sum: f32 = ratios.iter().sum::<f32>().max(f32::EPSILON);
            let n = children.len();
            rsx! {
                div { class: if axis == Axis::Row { "dock-split row" } else { "dock-split col" },
                    for (i, child) in children.into_iter().enumerate() {
                        {
                            let grow = ratios.get(i).copied().unwrap_or(1.0 / n as f32) / sum;
                            let mut child_path = path.clone();
                            child_path.push(i);
                            let reset_path = path.clone();
                            rsx! {
                                div { key: "c{i}", class: "dock-child", style: "flex-grow: {grow};",
                                    Node { host, node: child, path: child_path, solo, drag, splitting, menu, on_move }
                                }
                                if i + 1 < n {
                                    div {
                                        key: "s{i}",
                                        class: "dock-splitter",
                                        ondoubleclick: move |_| {
                                            windows::edit_tree(host, |t| {
                                                t.reset_split(&reset_path, i);
                                            });
                                        },
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        DockNode::Tabs { panes, active } => {
            let bar_path = path.clone();
            let bar_menu_path = path.clone();
            let shown = panes.get(active).copied();
            let bar_hover_len = panes.len();
            rsx! {
                div { class: "dock-leaf",
                    div {
                        class: "tab-bar",
                        onmousemove: move |e| {
                            if drag().is_some() {
                                e.stop_propagation();
                                let p = e.data.client_coordinates();
                                on_move.call((p.x as f32, p.y as f32, Some((bar_path.clone(), bar_hover_len))));
                            }
                        },
                        oncontextmenu: move |e| {
                            e.stop_propagation();
                            let p = e.data.client_coordinates();
                            menu.set(Some(Menu {
                                at: (p.x as f32, p.y as f32 - STRIP),
                                kind: MenuKind::Bar { path: bar_menu_path.clone() },
                            }));
                        },
                        for (i, pane) in panes.iter().copied().enumerate() {
                            {
                                let tab_path = path.clone();
                                let move_path = path.clone();
                                let menu_path = path.clone();
                                let is_target = drag().and_then(|d| d.target).is_some_and(|(p, z)| p == path && z == DropZone::TabBar(i));
                                let class = match (i == active, is_target) {
                                    (true, true) => "tab on insert-before",
                                    (true, false) => "tab on",
                                    (false, true) => "tab insert-before",
                                    (false, false) => "tab",
                                };
                                rsx! {
                                    div {
                                        key: "{pane.key()}",
                                        class: "{class}",
                                        onmousedown: move |e| {
                                            e.stop_propagation();
                                            menu.set(None);
                                            if e.data.trigger_button() != Some(dioxus::html::input_data::MouseButton::Primary) {
                                                return;
                                            }
                                            let p = e.data.client_coordinates();
                                            windows::edit_tree(host, |t| {
                                                if let Some(DockNode::Tabs { active, .. }) = t.at_mut(&tab_path) {
                                                    *active = i;
                                                }
                                            });
                                            drag.set(Some(Drag {
                                                pane,
                                                start: (p.x as f32, p.y as f32 - STRIP),
                                                at: (p.x as f32, p.y as f32 - STRIP),
                                                active: false,
                                                hover_tab: None,
                                                target: None,
                                            }));
                                        },
                                        onmousemove: move |e| {
                                            if drag().is_some() {
                                                e.stop_propagation();
                                                let p = e.data.client_coordinates();
                                                on_move.call((p.x as f32, p.y as f32, Some((move_path.clone(), i))));
                                            }
                                        },
                                        oncontextmenu: move |e| {
                                            e.stop_propagation();
                                            let p = e.data.client_coordinates();
                                            menu.set(Some(Menu {
                                                at: (p.x as f32, p.y as f32 - STRIP),
                                                kind: MenuKind::Tab { pane, path: menu_path.clone() },
                                            }));
                                        },
                                        "{pane.label()}"
                                    }
                                }
                            }
                        }
                        if solo {
                            span { class: "tab-note", "SOLO" }
                        }
                    }
                    div { class: "tab-body",
                        if let Some(pane) = shown {
                            windows::PaneBody { pane }
                        }
                    }
                }
            }
        }
    }
}

/// The right-click menu: on a tab, the pane's moves; on the bar's
/// empty space, the presets and the panes that can be added.
#[component]
fn ContextMenu(
    host: HostId,
    menu_at: (f32, f32),
    kind: MenuKind,
    tree: DockNode,
    solo: bool,
    close: EventHandler<()>,
) -> Element {
    let window = dioxus_native::use_window();
    let others: Vec<(HostId, String)> = windows::read_host(|h| {
        h.windows
            .iter()
            .filter(|w| w.id != host)
            .map(|w| (w.id, w.spec.title()))
            .collect()
    })
    .unwrap_or_default();
    let here = tree.panes();
    let is_launch = host == 0;
    rsx! {
        div {
            class: "dock-menu",
            style: "left: {menu_at.0}px; top: {menu_at.1}px;",
            onmousedown: move |e| e.stop_propagation(),
            oncontextmenu: move |e| e.stop_propagation(),
            match kind {
                MenuKind::Tab { pane, path } => {
                    let p1 = path.clone();
                    let p2 = path.clone();
                    rsx! {
                        div { class: "menu-title", "{pane.label()}" }
                        button { class: "menu-item",
                            onclick: move |_| {
                                close.call(());
                                if let Some(Some(new)) = windows::with_host(|h| h.pop_out(host, pane)) {
                                    windows::open(new);
                                }
                            },
                            "Detach to new window"
                        }
                        if !others.is_empty() {
                            div { class: "menu-sub", "Move to" }
                            for (id, title) in others.iter().cloned() {
                                button { key: "w{id}", class: "menu-item indent",
                                    onclick: move |_| {
                                        close.call(());
                                        windows::with_host(|h| h.move_pane(host, pane, id));
                                    },
                                    "{title}"
                                }
                            }
                        }
                        button { class: "menu-item",
                            onclick: move |_| {
                                close.call(());
                                windows::edit_tree(host, |t| { t.drop_pane(pane, &p1, DropZone::Right); });
                            },
                            "Split right with this pane"
                        }
                        button { class: "menu-item",
                            onclick: move |_| {
                                close.call(());
                                windows::edit_tree(host, |t| { t.drop_pane(pane, &p2, DropZone::Bottom); });
                            },
                            "Split down with this pane"
                        }
                        button { class: "menu-item",
                            onclick: move |_| {
                                close.call(());
                                if solo {
                                    windows::with_host(|h| h.restore(host));
                                } else {
                                    windows::with_host(|h| h.solo(host, pane));
                                }
                            },
                            if solo { "Restore layout" } else { "Solo this pane" }
                        }
                        button { class: "menu-item",
                            disabled: is_launch && here.len() == 1,
                            onclick: {
                                let window = window.clone();
                                move |_| {
                                    close.call(());
                                    let empty = windows::with_host(|h| h.close_pane(host, pane)).unwrap_or(false);
                                    if empty && !is_launch {
                                        dioxus_native::close_window(window.id());
                                    }
                                }
                            },
                            "Close"
                        }
                    }
                }
                MenuKind::Bar { path } => {
                    let addable: Vec<PaneKind> = PaneKind::ALL.iter().copied().filter(|p| !here.contains(p)).collect();
                    rsx! {
                        div { class: "menu-title", "Layout" }
                        for preset in Preset::ALL {
                            button { key: "{preset.label()}", class: "menu-item",
                                onclick: move |_| {
                                    close.call(());
                                    windows::with_host(|h| h.apply_preset(host, preset));
                                },
                                "{preset.label()}"
                            }
                        }
                        if solo {
                            button { class: "menu-item",
                                onclick: move |_| {
                                    close.call(());
                                    windows::with_host(|h| h.restore(host));
                                },
                                "Restore layout"
                            }
                        }
                        if !addable.is_empty() {
                            div { class: "menu-sub", "Add pane" }
                            div { class: "menu-grid",
                                for pane in addable {
                                    {
                                        let path = path.clone();
                                        rsx! {
                                            button { key: "{pane.key()}", class: "menu-item indent",
                                                onclick: move |_| {
                                                    close.call(());
                                                    windows::edit_tree(host, |t| {
                                                        let len = match t.at(&path) {
                                                            Some(DockNode::Tabs { panes, .. }) => panes.len(),
                                                            _ => 0,
                                                        };
                                                        if !t.insert_tab(&path, len, pane) {
                                                            t.adopt(pane);
                                                        }
                                                    });
                                                },
                                                "{pane.label()}"
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
