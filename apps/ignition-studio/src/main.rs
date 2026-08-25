//! Ignition Studio — the operator application.
//!
//! Busking is the priority: the surface is a selection, palettes, and
//! eight assignable faders with one master rate. Cue playback sits
//! underneath and fills in whatever the operator is not holding, which
//! is the order the layer cascade was built for — see
//! `ignition_core::programmer`.
//!
//! Dioxus owns the window and everything in it; the Bevy visualizer is
//! one element in the layout, not a separate window.

use dioxus::prelude::*;
use dioxus_native_dom::CustomWidgetAttr;
use ignition_core::Selection;
use ignition_viz::spawn::BeamStyle;
use ignition_viz::{Venue, ViewPreset, VizConfig};
use std::any::Any;

mod command;
mod faders;
mod viz_widget;
use command::{Command, Sender};

use viz_widget::VizWidget;

/// Compiled by `dx serve` from `tailwind.css` at the crate root — it
/// watches that file and writes here. Built by hand with `just tailwind`
/// when not serving, because a plain `cargo run` does not know about any
/// of this.
const TAILWIND: Asset = asset!("/assets/tailwind.css");

const VENUE: &str = "data/venues/norco";
const SHOW: &str = "data/shows/effects-demo.json";

/// The one UI-to-visualizer channel.
///
/// A pair of globals rather than props or context, because the two ends
/// are taken at different times by parts of the tree that cannot hand
/// anything to each other: components need the sender during render, and
/// the Blitz widget needs the receiver when it is constructed. There is
/// exactly one window and exactly one channel, so the alternative is
/// ceremony for its own sake.
static TX: std::sync::OnceLock<Sender> = std::sync::OnceLock::new();
static RX: std::sync::Mutex<Option<command::Receiver>> = std::sync::Mutex::new(None);

fn main() -> anyhow::Result<()> {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .try_init();

    let (tx, rx) = command::channel();
    let _ = TX.set(tx);
    *RX.lock().expect("fresh mutex") = Some(rx);

    let venue = Venue::load(VENUE)?;
    let surface = Surface {
        groups: busking_groups(&venue),
        colors: venue
            .palettes
            .colors
            .iter()
            .map(|c| ColorChip {
                name: c.name.clone(),
                // The palette is linear 0–1 like the fixtures; the disc
                // only has to look like the gel, so a plain byte scale
                // is close enough and avoids a colour-management rabbit
                // hole for a swatch.
                css: format!(
                    "rgb({} {} {})",
                    (c.red * 255.0) as u8,
                    (c.green * 255.0) as u8,
                    (c.blue * 255.0) as u8
                ),
            })
            .collect(),
        focus: venue
            .palettes
            .focus
            .iter()
            .map(|f| f.name.clone())
            .collect(),
        cues: load_cue_names(SHOW).unwrap_or_default(),
    };

    // Blitz creates the wgpu device, so anything Bevy needs has to be
    // asked for here. Borrowing somebody else's device means inheriting
    // whatever they asked for, and a missing feature surfaces as a
    // validation error mid-frame rather than at startup. Bloom's HDR
    // buffer is `Rg11b10Ufloat`, not renderable without the first.
    let config: Vec<Box<dyn Any>> = vec![
        Box::new(
            wgpu::Features::RG11B10UFLOAT_RENDERABLE
                | wgpu::Features::FLOAT32_FILTERABLE
                | wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES,
        ),
        Box::new(wgpu::Limits::default()),
        Box::new(window_attributes()),
    ];

    dioxus_native::launch_cfg_with_props(app, surface, Vec::new(), config);
    Ok(())
}

/// The window's initial size. Placement happens later — see
/// [`place_window`].
fn window_attributes() -> dioxus_native::winit::window::WindowAttributes {
    use dioxus_native::winit::dpi::LogicalSize;
    use dioxus_native::winit::window::WindowAttributes;

    WindowAttributes::default()
        .with_title("Ignition Studio")
        .with_surface_size(LogicalSize::new(1600, 950))
}

/// Puts the window borderless-fullscreen on a chosen monitor.
///
/// Done *after* startup rather than in `WindowAttributes`, and that is
/// forced rather than stylistic. Under Wayland a client cannot place
/// itself — `with_position` is silently ignored and the compositor puts
/// the window wherever it likes, which is how the first attempt ended up
/// fullscreen on the middle monitor. The only reliable route is to wait
/// until there *is* a window, ask it what monitors exist, and pick one.
/// That also works on X11, so there is one code path rather than two.
///
/// (There is no `dioxus.toml` option for any of this. That file
/// configures the build and the bundle; nothing in it reaches winit.)
///
/// `IGNITION_MONITOR` accepts, in order of preference:
///
/// ```text
/// DP-3        an output name, as `xrandr --listmonitors` prints it
/// 6560,0      a monitor's top-left corner
/// right|left  the outermost monitor by position — machine-independent
/// primary     whatever the compositor calls primary
/// ```
///
/// `IGNITION_FULLSCREEN=0` opts out entirely.
fn place_window() {
    use dioxus_native::winit::monitor::{Fullscreen, MonitorHandle};

    let window = dioxus_native::use_window();
    use_effect(move || {
        if std::env::var("IGNITION_FULLSCREEN").is_ok_and(|v| v == "0") {
            return;
        }
        let want = std::env::var("IGNITION_MONITOR").unwrap_or_default();
        let monitors: Vec<MonitorHandle> = window.available_monitors().collect();
        let chosen = pick_monitor(&monitors, &want).or_else(|| window.primary_monitor());
        tracing::info!(
            monitor = ?chosen.as_ref().and_then(|m| m.name().map(|n| n.to_string())),
            of = monitors.len(),
            "studio: going fullscreen"
        );
        // `Borderless(None)` would mean "wherever this window already
        // is", which on Wayland is the compositor's guess.
        window.set_fullscreen(Some(Fullscreen::Borderless(chosen)));
    });
}

fn pick_monitor(
    monitors: &[dioxus_native::winit::monitor::MonitorHandle],
    want: &str,
) -> Option<dioxus_native::winit::monitor::MonitorHandle> {
    use dioxus_native::winit::monitor::MonitorHandle;

    if want.is_empty() || monitors.is_empty() {
        return None;
    }
    let by_name = monitors
        .iter()
        .find(|m| m.name().is_some_and(|n| n.eq_ignore_ascii_case(want)));
    if by_name.is_some() {
        return by_name.cloned();
    }
    if let Some((x, y)) = want
        .split_once(',')
        .and_then(|(x, y)| Some((x.trim().parse::<i32>().ok()?, y.trim().parse::<i32>().ok()?)))
    {
        let by_corner = monitors
            .iter()
            .find(|m| m.position().is_some_and(|p| p.x == x && p.y == y));
        if by_corner.is_some() {
            return by_corner.cloned();
        }
    }
    // A monitor with no reported position sorts as if it were at the
    // far left, so "right" never accidentally picks an unknown one.
    let x_of = |m: &MonitorHandle| m.position().map(|p| p.x).unwrap_or(i32::MIN);
    match want {
        "right" => monitors.iter().max_by_key(|m| x_of(m)).cloned(),
        "left" => monitors.iter().min_by_key(|m| x_of(m)).cloned(),
        _ => None,
    }
}

/// A colour palette entry as the surface draws it: the name to send, and
/// the colour to show. A colour pool that does not show its colours is a
/// list of words, which is the whole reason to have a pool.
#[derive(Clone, PartialEq)]
struct ColorChip {
    name: String,
    css: String,
}

/// The named things the surface offers, resolved once from the venue.
#[derive(Clone, Props, PartialEq)]
struct Surface {
    groups: Vec<String>,
    colors: Vec<ColorChip>,
    focus: Vec<String>,
    cues: Vec<String>,
}

/// The groups worth a button. The venue carries 127, most of them
/// numeric slices nobody busks with; these are the role groups the rig
/// was actually laid out into.
fn busking_groups(venue: &Venue) -> Vec<String> {
    const WANTED: [&str; 12] = [
        "All",
        "Washers",
        "Back Wall Pars",
        "Center Washers",
        "Downstage L Washers",
        "Downstage R Washers",
        "Upstage L Washers",
        "Upstage R Washers",
        "Drummer Washers",
        "OH Movers",
        "Floor Movers",
        "Strips All",
    ];
    let have: Vec<String> = venue.groups().into_iter().map(|g| g.name).collect();
    WANTED
        .iter()
        .filter(|w| have.iter().any(|h| h == *w))
        .map(|w| w.to_string())
        .collect()
}

fn app(surface: Surface) -> Element {
    place_window();

    // Load the eight faders once. They queue in the channel until the
    // visualizer exists to drain them, so this does not have to wait for
    // the widget to be built.
    use_hook(|| {
        for (i, spec) in faders::defaults().into_iter().enumerate() {
            send(Command::Fader(
                i,
                Box::new(ignition_core::Fader {
                    name: spec.name.to_string(),
                    recipe: Some(spec.recipe),
                    level: 0.0,
                }),
            ));
        }
    });

    rsx! {
        // Both, on purpose, and only for as long as the migration takes.
        // `studio.css` is the working stylesheet; Tailwind is proved out
        // one column at a time, Groups first. Blitz's style engine is
        // stylo — Firefox's — so `@layer`, `@property` and `color-mix()`
        // in Tailwind v4's output should all resolve, but nobody in this
        // tree has run them through it yet and a wholesale conversion
        // that turned out not to render would take the entire surface
        // with it. If the Groups pool comes up right, the rest follows
        // and `studio.css` goes.
        style { {include_str!("studio.css")} }
        document::Stylesheet { href: TAILWIND }
        div { class: "studio",
            CueList { cues: surface.cues.clone() }
            main { class: "stage",
                div { class: "viewport", Viewport {} }
                Busking { surface: surface.clone() }
            }
        }
    }
}

/// The cue stack. Underneath the busking layer, not beside it: a cue
/// fills in whatever the operator is not currently holding.
#[component]
fn CueList(cues: Vec<String>) -> Element {
    let mut current = use_signal(|| Option::<usize>::None);

    rsx! {
        aside { class: "cues",
            header {
                span { "Cue List" }
                button {
                    class: "go",
                    onclick: move |_| {
                        current.set(Some(current().map_or(0, |i| i + 1)));
                        send(Command::Go);
                    },
                    "GO"
                }
            }
            ol {
                for (i, name) in cues.iter().enumerate() {
                    li {
                        key: "{i}",
                        class: if current() == Some(i) { "cue on" } else { "cue" },
                        onclick: move |_| {
                            current.set(Some(i));
                            send(Command::Cue(i));
                        },
                        span { class: "num", "{i}" }
                        span { class: "name", "{name}" }
                    }
                }
            }
        }
    }
}

/// A free function rather than a captured closure: every control needs
/// to send, closures in `rsx!` are `FnMut`, and a captured `Sender`
/// cannot be moved out of one more than once.
fn send(command: Command) {
    if let Some(tx) = TX.get() {
        let _ = tx.send(command);
    }
}

#[component]
fn Busking(surface: Surface) -> Element {
    let mut selected = use_signal(|| Option::<String>::None);

    rsx! {
        section { class: "surface",
            div { class: "col groups",
                header { "Groups" }
                // Same uniform pool grid as Focus. Fixed-size cells
                // rather than pills that size to their label, so the
                // pool stays a predictable grid an operator can learn
                // the shape of — position is how you find a group on a
                // console, not reading.
                div { class: "flex flex-wrap gap-2",
                    for name in surface.groups.iter().cloned() {
                        button {
                            key: "{name}",
                            // Tailwind, as the migration probe. Same
                            // shape as `.pad` in studio.css so the two
                            // pools can be compared side by side.
                            class: if selected() == Some(name.clone()) {
                                "w-21 h-16 p-1 text-[11px] rounded-md cursor-pointer \
                                 bg-sel border border-sel-line text-white"
                            } else {
                                "w-21 h-16 p-1 text-[11px] rounded-md cursor-pointer \
                                 bg-pad border border-pad-line text-ink \
                                 hover:bg-pad-hover hover:border-[#3d3d4a]"
                            },
                            onclick: {
                                let name = name.clone();
                                move |_| {
                                    selected.set(Some(name.clone()));
                                    send(Command::Select(Selection::Group(name.clone())));
                                }
                            },
                            "{name}"
                        }
                    }
                }
            }

            div { class: "col colours",
                header { "Colour" }
                div { class: "swatches",
                    for chip in surface.colors.iter().cloned() {
                        button {
                            key: "{chip.name}",
                            class: "swatch",
                            onclick: {
                                let name = chip.name.clone();
                                move |_| send(Command::Color(name.clone()))
                            },
                            // The disc carries the colour and the whole
                            // control is the hit target, so the label can
                            // stay small without making it hard to press.
                            span { class: "disc", style: "background: {chip.css}" }
                            span { class: "swatch-label", "{chip.name}" }
                        }
                    }
                }
            }

            div { class: "col focus",
                header { "Focus" }
                div { class: "pool",
                    for name in surface.focus.iter().cloned() {
                        button {
                            key: "{name}",
                            class: "pad",
                            onclick: {
                                let name = name.clone();
                                move |_| send(Command::Focus(name.clone()))
                            },
                            "{name}"
                        }
                    }
                }
            }

            div { class: "col intensity",
                header { "Intensity" }
                div { class: "intensity-row",
                    // Presets to the left of the fader, the way a
                    // console puts its dimmer pool beside the wheel.
                    div { class: "presets",
                        for pct in [100u32, 75, 50, 25, 0] {
                            button {
                                key: "{pct}",
                                class: "preset",
                                onclick: move |_| send(Command::Dimmer(pct as f32 / 100.0)),
                                "{pct}"
                            }
                        }
                    }
                    Fader {
                        label: "INT".to_string(),
                        css: "#e8e8e8".to_string(),
                        initial: 0.0,
                        on_change: move |v: f32| send(Command::Dimmer(v)),
                    }
                }
                div { class: "row",
                    button { class: "tile warn", onclick: move |_| send(Command::Release), "Release" }
                    button { class: "tile warn", onclick: move |_| send(Command::ClearValues), "Clear" }
                }
            }

            div { class: "col faders",
                header { "Faders" }
                div { class: "fader-row",
                    for (i, spec) in faders::defaults().into_iter().enumerate() {
                        Fader {
                            key: "{i}",
                            label: spec.name.to_string(),
                            css: spec.css.to_string(),
                            initial: 0.0,
                            on_change: move |v: f32| send(Command::Level(i, v)),
                        }
                    }
                    div { class: "master",
                        Fader {
                            label: "RATE".to_string(),
                            css: "#c08a3e".to_string(),
                            initial: 0.4,
                            // 40–220 BPM over the fader's travel.
                            on_change: move |v: f32| send(Command::Rate(40.0 + v * 180.0)),
                        }
                    }
                }
            }
        }
    }
}

/// How tall a fader's track is, in CSS pixels.
///
/// Known rather than measured: the value comes from the pointer's
/// position *within the element*, and a layout query in an event handler
/// is exactly the thing that deadlocks Blitz. Keep this in step with
/// `.track` in studio.css.
const TRACK: f32 = 190.0;

/// A fader built from divs rather than `<input type=range>`.
///
/// Three reasons, in order: a range input renders as a bare white bar in
/// Blitz with no way to style the fill; touch needs a hit target far
/// larger than a native thumb; and the value has to be readable as a
/// colour at a glance from two metres away, which a native control
/// cannot do.
#[component]
fn Fader(label: String, css: String, initial: f32, on_change: EventHandler<f32>) -> Element {
    let mut level = use_signal(|| initial);
    let mut held = use_signal(|| false);

    // `element_coordinates` is relative to the element the handler is
    // on, which is why the track owns the events rather than the whole
    // fader — no measuring, no layout query.
    let mut set_from = move |y: f64| {
        let v = (1.0 - (y as f32 / TRACK)).clamp(0.0, 1.0);
        level.set(v);
        on_change.call(v);
    };

    rsx! {
        div { class: "fader",
            div {
                class: "track",
                onpointerdown: move |e| {
                    held.set(true);
                    set_from(e.data.element_coordinates().y);
                },
                onpointermove: move |e| {
                    if held() {
                        set_from(e.data.element_coordinates().y);
                    }
                },
                onpointerup: move |_| held.set(false),
                onpointerleave: move |_| held.set(false),
                div {
                    class: "fill",
                    style: "height: {level() * 100.0}%; background: {css}",
                }
                div {
                    class: "handle",
                    style: "bottom: {level() * 100.0}%; border-color: {css}",
                }
            }
            span { class: "fader-label", "{label}" }
            span { class: "fader-value", "{(level() * 100.0) as u32}" }
        }
    }
}

#[component]
fn Viewport() -> Element {
    let widget_attr = use_hook(|| {
        let config = VizConfig {
            venue: Venue::load(VENUE).expect("venue already loaded once in main"),
            view: ViewPreset::House,
            width: 1280,
            height: 800,
            haze: 1.0,
            // A little fill so the room reads even in a dark look — the
            // operator is looking at a panel, not sitting in the venue.
            ambient: 0.05,
            show_props: true,
            exclude: Vec::new(),
            exposure: 2500.0,
            screen_content: Some("screens/rockstars-logo.webp".to_string()),
            assets_dir: concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../crates/ignition-viz/assets"
            )
            .to_string(),
            beam_style: BeamStyle::Volumetric,
            max_universe: 4,
            snapshot: None,
            settle_frames: 1,
            camera: None,
            overlay: false,
        };
        let rx = RX
            .lock()
            .expect("fresh mutex")
            .take()
            .expect("one viewport");
        CustomWidgetAttr::new(VizWidget::new(config, Some((SHOW.to_string(), 0)), rx))
    });

    // Blitz repaints on demand and a `Widget` has no way to ask for a
    // frame, so a signal ticking at ~60 Hz keeps the DOM dirty and the
    // visualizer animating. Coarse, and the honest alternative is
    // patching Blitz.
    let mut frame = use_signal(|| 0u64);
    use_future(move || async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(16)).await;
            frame += 1;
        }
    });

    rsx! {
        div { class: "viz", "data-frame": "{frame}",
            object { "data": widget_attr }
        }
    }
}

/// Cue names for the list. The player inside the visualizer owns the
/// real cues; this is only what to draw.
fn load_cue_names(path: &str) -> anyhow::Result<Vec<String>> {
    let raw = std::fs::read_to_string(path)?;
    let list: ignition_core::CueList = serde_json::from_str(&raw)?;
    Ok(list.cues.into_iter().map(|c| c.name).collect())
}
