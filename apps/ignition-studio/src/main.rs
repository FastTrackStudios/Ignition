//! Ignition Studio — the operator application.
//!
//! Dioxus owns the window and everything in it; the Bevy visualizer is
//! one element in the layout, not a separate window. Cue list on the
//! left, venue on the right.

use dioxus::prelude::*;
use dioxus_native_dom::CustomWidgetAttr;
use ignition_viz::spawn::BeamStyle;
use ignition_viz::{Venue, ViewPreset, VizConfig};
use std::any::Any;

mod viz_widget;
use viz_widget::VizWidget;

const VENUE: &str = "data/venues/norco";
const SHOW: &str = "data/shows/effects-demo.json";

fn main() -> anyhow::Result<()> {
    // Structured only, behind RUST_LOG — the visualizer's own tracing
    // subscriber is not installed in this process.
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .try_init();

    // Load once here so a bad path fails at startup with a real error
    // rather than inside a render.
    let _ = Venue::load(VENUE)?;
    let cues = load_cue_names(SHOW)?;

    // Blitz creates the wgpu device, so Bevy's requirements have to be
    // stated *here* — the device is already made by the time the widget
    // sees it.
    let config: Vec<Box<dyn Any>> = vec![
        // Blitz creates the device, so anything Bevy needs has to be
        // asked for *here*. Bevy normally enables these itself when the
        // adapter supports them; borrowing somebody else's device means
        // inheriting whatever they asked for, and a missing feature
        // surfaces as a wgpu validation error mid-frame rather than at
        // startup. Bloom's HDR buffer is `Rg11b10Ufloat`, which is not
        // renderable without the first of these.
        Box::new(
            wgpu::Features::RG11B10UFLOAT_RENDERABLE
                | wgpu::Features::FLOAT32_FILTERABLE
                | wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES,
        ),
        Box::new(wgpu::Limits::default()),
        Box::new(
            dioxus_native::winit::window::WindowAttributes::default().with_title("Ignition Studio"),
        ),
    ];

    dioxus_native::launch_cfg_with_props(app, StudioProps { cues }, Vec::new(), config);
    Ok(())
}

/// `Venue` is deliberately not in here: Dioxus props must be `PartialEq`
/// so the runtime can skip unchanged re-renders, and comparing a whole
/// venue every frame to decide whether a viewport needs redrawing would
/// be absurd. The viewport loads it itself, once.
#[derive(Clone, Props, PartialEq)]
struct StudioProps {
    cues: Vec<String>,
}

fn app(props: StudioProps) -> Element {
    let mut selected = use_signal(|| 0usize);

    rsx! {
        style { {include_str!("studio.css")} }
        div { class: "studio",
            aside { class: "cues",
                header { "Cue List" }
                ol {
                    for (i, name) in props.cues.iter().enumerate() {
                        li {
                            key: "{i}",
                            class: if selected() == i { "cue selected" } else { "cue" },
                            onclick: move |_| selected.set(i),
                            span { class: "num", "{i}" }
                            span { class: "name", "{name}" }
                        }
                    }
                }
            }
            main { class: "viewport", Viewport {} }
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
            // A little fill so the room reads even in a dark cue — the
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
        CustomWidgetAttr::new(VizWidget::new(config, Some((SHOW.to_string(), 5))))
    });

    // Blitz repaints on demand, and a `Widget` has no way to ask for a
    // frame. A signal ticking at ~60 Hz and bound to an attribute is
    // what keeps the DOM dirty, and therefore keeps the visualizer
    // animating. Coarse — it re-diffs one element per frame — but the
    // alternative is patching Blitz.
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

/// Cue names, straight out of the show file — enough for the list until
/// the real player is wired through.
fn load_cue_names(path: &str) -> anyhow::Result<Vec<String>> {
    let raw = std::fs::read_to_string(path)?;
    let list: ignition_core::CueList = serde_json::from_str(&raw)?;
    Ok(list.cues.into_iter().map(|c| c.name).collect())
}
