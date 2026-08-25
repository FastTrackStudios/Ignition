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
mod viz_widget;
use command::{Command, Sender};

use viz_widget::VizWidget;

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
            .map(|c| c.name.clone())
            .collect(),
        focus: venue
            .palettes
            .focus
            .iter()
            .map(|f| f.name.clone())
            .collect(),
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
        Box::new(
            dioxus_native::winit::window::WindowAttributes::default()
                .with_title("Ignition Studio")
                .with_surface_size(dioxus_native::winit::dpi::LogicalSize::new(1600, 950)),
        ),
    ];

    dioxus_native::launch_cfg_with_props(app, surface, Vec::new(), config);
    Ok(())
}

/// The named things the surface offers, resolved once from the venue.
#[derive(Clone, Props, PartialEq)]
struct Surface {
    groups: Vec<String>,
    colors: Vec<String>,
    focus: Vec<String>,
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
    rsx! {
        style { {include_str!("studio.css")} }
        div { class: "studio",
            Busking { surface: surface.clone() }
            main { class: "viewport", Viewport {} }
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
    let mut rate = use_signal(|| 120.0f32);
    let mut levels = use_signal(|| [0.0f32; ignition_core::FADERS]);

    rsx! {
        aside { class: "surface",
            section {
                header { "Groups" }
                div { class: "chips",
                    for name in surface.groups.iter().cloned() {
                        button {
                            key: "{name}",
                            class: if selected() == Some(name.clone()) { "chip on" } else { "chip" },
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

            section {
                header { "Colour" }
                div { class: "chips",
                    for name in surface.colors.iter().cloned() {
                        button {
                            key: "{name}",
                            class: "chip",
                            onclick: {
                                let name = name.clone();
                                move |_| send(Command::Color(name.clone()))
                            },
                            "{name}"
                        }
                    }
                }
            }

            section {
                header { "Focus" }
                div { class: "chips",
                    for name in surface.focus.iter().cloned() {
                        button {
                            key: "{name}",
                            class: "chip",
                            onclick: {
                                let name = name.clone();
                                move |_| send(Command::Focus(name.clone()))
                            },
                            "{name}"
                        }
                    }
                }
            }

            section {
                header { "Intensity" }
                div { class: "chips",
                    for pct in [0u32, 25, 50, 75, 100] {
                        button {
                            key: "{pct}",
                            class: "chip",
                            onclick: move |_| send(Command::Dimmer(pct as f32 / 100.0)),
                            "{pct}%"
                        }
                    }
                    button { class: "chip warn", onclick: move |_| send(Command::Release), "Release" }
                    button { class: "chip warn", onclick: move |_| send(Command::ClearValues), "Clear" }
                }
            }

            section { class: "faders",
                header { "Faders" }
                div { class: "fader-row",
                    for i in 0..ignition_core::FADERS {
                        div { key: "{i}", class: "fader",
                            input {
                                r#type: "range",
                                min: "0", max: "100", step: "1",
                                value: "{(levels()[i] * 100.0) as u32}",
                                oninput: move |e| {
                                    let v = e.value().parse::<f32>().unwrap_or(0.0) / 100.0;
                                    levels.write()[i] = v;
                                    send(Command::Level(i, v));
                                },
                            }
                            span { class: "fader-label", "{i + 1}" }
                        }
                    }
                    div { class: "fader master",
                        input {
                            r#type: "range",
                            min: "40", max: "220", step: "1",
                            value: "{rate() as u32}",
                            oninput: move |e| {
                                let v = e.value().parse::<f32>().unwrap_or(120.0);
                                rate.set(v);
                                send(Command::Rate(v));
                            },
                        }
                        span { class: "fader-label", "{rate() as u32}" }
                    }
                }
            }
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
