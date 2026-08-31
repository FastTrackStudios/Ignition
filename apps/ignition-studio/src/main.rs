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
use ignition_viz::{RenderQuality, Venue, ViewPreset, VizConfig};
use std::any::Any;

mod command;
mod dock;
mod layout;
mod live_commands;
mod live_web;
mod remote;
mod sound;
mod viz_widget;
mod windows;
use command::{Command, PageMove, SpeedKey};
// The Live / Program / Library views and the surface types live in
// `ignition-live-ui`, so the same components serve an iPad from
// `live_web`. The studio re-exports the names its own panels use.
// r[impl studio.touch.ipad] - one UI crate, mounted natively here
use ignition_live_ui::{
    ColorChip, CueList, HSlider, PlayheadFeed, Row, Surface, desk, faders, library, operators,
    program, send, use_desk, use_playhead,
};

use viz_widget::VizWidget;

/// Compiled by `dx serve` from `tailwind.css` at the crate root — it
/// watches that file and writes here. Built by hand with `just tailwind`
/// when not serving, because a plain `cargo run` does not know about any
/// of this.
const TAILWIND: Asset = asset!("/assets/tailwind.css");

/// The room the studio opens, unless `IGNITION_VENUE` names another —
/// e.g. `IGNITION_VENUE=data/venues/riverside`. A venue is data, and
/// there is more than one of them now, so which room the surface is
/// driving should not be a recompile.
const DEFAULT_VENUE: &str = "data/venues/norco";

/// Where the studio's log file goes.
fn log_file_path() -> std::path::PathBuf {
    if let Ok(p) = std::env::var("IGNITION_LOG_FILE") {
        return std::path::PathBuf::from(p);
    }
    let state = std::env::var("XDG_STATE_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|_| {
            std::env::var("HOME").map(|h| std::path::PathBuf::from(h).join(".local/state"))
        })
        .unwrap_or_else(|_| std::path::PathBuf::from("."));
    state.join("ignition").join("studio.log")
}

pub fn venue_dir() -> String {
    std::env::var("IGNITION_VENUE").unwrap_or_else(|_| DEFAULT_VENUE.to_string())
}
const DEFAULT_SHOW: &str = "data/songs/bye-bye-bye.json";

/// The cue list the studio opens. `IGNITION_SHOW` overrides it.
///
/// A studio that can only ever open one hard-coded song is awkward for
/// an operator and impossible for a benchmark: the numbers that matter
/// are taken on `data/songs/benchmark.json`, which lights every mover,
/// every par and every hazer at once, and there was no way to ask for
/// it. See `docs/ops/profiling.md`.
// r[impl studio.profiling] - the show under test is a setting, not a constant
fn show_path() -> String {
    std::env::var("IGNITION_SHOW").unwrap_or_else(|_| {
        if bench_mode() {
            BENCH_SHOW.to_string()
        } else {
            DEFAULT_SHOW.to_string()
        }
    })
}

/// The cue list `r[viz.performance-budget]` is written against: one cue
/// that lights every mover, every par, every bar, the beams and the
/// hazers at once.
const BENCH_SHOW: &str = "data/songs/benchmark.json";

/// `IGNITION_BENCH=1` — open on the benchmark cue, and *take* it.
///
/// Loading the list is not enough and the difference is easy to miss: a
/// cue list that is loaded but never GOed outputs nothing, so the
/// studio comes up on a dark rig and the profiler measures an empty
/// room at a very flattering frame rate. `crates/ignition-viz/tests/
/// benchmark_cue.rs` exists because that mistake is worth a test; this
/// exists because it is worth a switch.
///
///     IGNITION_BENCH=1 IGNITION_PROFILE=1 just studio
///
/// or `just profile-bench`.
// r[impl studio.profiling] - a benchmark you can open, with the rig lit
fn bench_mode() -> bool {
    std::env::var("IGNITION_BENCH").is_ok_and(|v| v == "1")
}

/// The project the show is synced to. Optional at runtime — if it will
/// not open, the surface still busks and the cue list still steps on GO.
const PROJECT: &str = "/home/cody/Downloads/Bye Bye Bye/Bye Bye Bye.RPP";

/// The one UI-to-visualizer channel.
///
/// Globals rather than props or context, because the two ends are taken
/// at different times by parts of the tree that cannot hand anything to
/// each other: components send through `ignition_live_ui::send` (which
/// `main` points at the sender), and the Blitz widget needs the
/// receiver when it is constructed. There is exactly one channel, so
/// the alternative is ceremony for its own sake.
static RX: std::sync::Mutex<Option<command::Receiver>> = std::sync::Mutex::new(None);
/// The return path: the widget publishes the playhead, components read
/// it. Same one-window reasoning as the pair above.
static STATE_TX: std::sync::Mutex<Option<command::StateTx>> = std::sync::Mutex::new(None);
static STATE_RX: std::sync::OnceLock<command::StateRx> = std::sync::OnceLock::new();

fn main() -> anyhow::Result<()> {
    // `from_default_env()` alone defaults to ERROR when RUST_LOG is
    // unset, which silently discards every line this app logs about
    // whether the song loaded — including the warning that says it
    // didn't. The failure then looks like a dead audio device. Default
    // to info for our own crates and let RUST_LOG override; Bevy and wgpu
    // stay quiet because at info they are not.
    // r[impl studio.one-truth] - the log is the record: stderr for the
    // terminal and a file for whoever debugs later, so nothing has to be
    // copied out of a scrollback. `$XDG_STATE_HOME/ignition/studio.log`
    // (`~/.local/state/ignition/studio.log`), or `IGNITION_LOG_FILE`.
    let filter = tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        tracing_subscriber::EnvFilter::new(
            // `bevy_pbr::ssao` is silenced rather than the plugin
            // disabled. Screen-space ambient occlusion declines on this
            // device — its limits allow four storage textures per shader
            // stage where SSAO wants five — and we do not use it.
            // Calling `.disable::<ScreenSpaceAmbientOcclusionPlugin>()`
            // panics with "cannot disable a plugin that does not exist",
            // because PbrPlugin adds it rather than it being a member of
            // the group, and a crash is a poor trade for a tidy log.
            "warn,ignition_studio=info,ignition_daw=info,\
             ignition_viz=info,bevy_pbr::ssao=error",
        )
    });
    // The profiler's stage spans are `info` on one target of their own,
    // so they are off under the default filter and on with one
    // directive — no rebuild, and no turning up Blitz's own logging to
    // reach the spans that live inside it.
    // r[impl studio.profiling] - `IGNITION_PROFILE=1` and the filter follows
    let filter =
        ignition_profile::filter_directives()
            .into_iter()
            .fold(filter, |filter, directive| match directive.parse() {
                Ok(directive) => filter.add_directive(directive),
                Err(_) => filter,
            });
    let log_path = log_file_path();
    let log_file = std::fs::create_dir_all(log_path.parent().unwrap_or(std::path::Path::new(".")))
        .and_then(|_| std::fs::File::create(&log_path))
        .ok();
    {
        use tracing_subscriber::layer::SubscriberExt as _;
        use tracing_subscriber::util::SubscriberInitExt as _;
        let stderr = tracing_subscriber::fmt::layer().with_writer(std::io::stderr);
        let file = log_file.map(|f| {
            tracing_subscriber::fmt::layer()
                .with_ansi(false)
                .with_writer(std::sync::Mutex::new(f))
        });
        let _ = tracing_subscriber::registry()
            .with(filter)
            .with(stderr)
            .with(file)
            // Below the filter, so it only ever sees spans the filter
            // let through — which is what makes "off" free.
            .with(ignition_profile::from_env())
            .try_init();
    }
    tracing::info!(path = %log_path.display(), "studio: logging to a file");
    // A panic on any thread lands in the log too — a render-thread
    // panic otherwise shows only as a frozen picture.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        tracing::error!(%info, thread = ?std::thread::current().name(), "studio: panic");
        default_hook(info);
    }));

    // The DAW backend's service layer spawns tasks through architect,
    // which panics rather than erroring if no runtime is current — see
    // `SongTransport::open`. The guard has to outlive the transport, not
    // just the call that opens it, and the transport is built during the
    // first render of the viewport, so it is held here across `launch`
    // for the lifetime of the app. `playsong` does the same thing, and
    // that it plays audio while the studio did not was the difference.
    let runtime = tokio::runtime::Runtime::new()?;
    let _guard = runtime.enter();

    let (tx, rx) = command::channel();
    // Hardware and the room, on their own threads, speaking the same
    // messages the UI does. Each is optional at build time and at run
    // time: a missing port or device logs and the surface carries on.
    remote::start(tx.clone());
    sound::start(tx.clone());
    // Every component's `send`, in every window, is this sender.
    {
        let tx = tx.clone();
        ignition_live_ui::install(move |command| {
            let _ = tx.send(command);
        });
    }
    *RX.lock().expect("fresh mutex") = Some(rx);

    let (state_tx, state_rx) = command::state_channel();
    *STATE_TX.lock().expect("fresh mutex") = Some(state_tx);
    // The return path to the hardware: fader positions, key states and
    // the page, back to whatever surface asked for them.
    remote::start_feedback(state_rx.clone());
    let _ = STATE_RX.set(state_rx.clone());

    let venue = Venue::load(venue_dir())?;
    let surface = Surface {
        groups: busking_groups(&venue),
        colors: venue
            .palettes
            .colors
            .iter()
            .map(|c| {
                // The palette is linear 0–1 like the fixtures; the disc
                // only has to look like the gel, so a plain byte scale
                // is close enough and avoids a colour-management rabbit
                // hole for a swatch.
                ColorChip {
                    light: ColorChip::is_light(c.red, c.green, c.blue),
                    ..ColorChip::solid(c.name.clone(), rgb(c))
                }
            })
            .collect(),
        splits: venue
            .palettes
            .splits
            .iter()
            .filter_map(|split| {
                let (colors, distribute) = venue
                    .palettes
                    .resolve_split(&ignition_core::Ref::Named(split.name.clone()))?;
                let stops: Vec<String> = colors.iter().map(rgb).collect();
                // Spread is a gradient; cycle and block are hard bands,
                // which is also how they land on the rig.
                let css = match distribute {
                    ignition_core::Distribute::Spread => {
                        format!("linear-gradient(90deg, {})", stops.join(", "))
                    }
                    _ => {
                        let n = stops.len().max(1);
                        let bands: Vec<String> = stops
                            .iter()
                            .enumerate()
                            .map(|(i, c)| format!("{c} {}% {}%", i * 100 / n, (i + 1) * 100 / n))
                            .collect();
                        format!("linear-gradient(90deg, {})", bands.join(", "))
                    }
                };
                // A palette is light when its colours average light —
                // Ice reads as light, Hot/Deep does not, and the name
                // written across it has to follow.
                let light = colors
                    .iter()
                    .map(|c| ColorChip::is_light(c.red, c.green, c.blue) as u32)
                    .sum::<u32>() as usize
                    * 2
                    > colors.len();
                Some(ColorChip {
                    name: split.name.clone(),
                    css,
                    light,
                    // The individual colours travel too: a bar can be a
                    // gradient, but a disc has to be wedges, and only
                    // the surface knows which shape it is drawing.
                    colors: stops,
                    spread: matches!(distribute, ignition_core::Distribute::Spread),
                })
            })
            .collect(),
        focus: venue
            .palettes
            .focus
            .iter()
            .map(|f| f.name.clone())
            .collect(),
        cues: load_cue_names(&show_path()).unwrap_or_default(),
    };

    // The same surface, to an iPad — opt-in, see `live_web`. Started
    // here rather than in a component because it needs the sender and
    // the watch, not a window.
    // r[impl studio.touch.ipad] - the studio serves the Live view
    let lan = live_web::start(tx.clone(), state_rx.clone(), surface.clone());
    let _ = LAN.set(lan);

    // Blitz creates the wgpu device, so anything Bevy needs has to be
    // asked for here. Borrowing somebody else's device means inheriting
    // whatever they asked for, and a missing feature surfaces as a
    // validation error mid-frame rather than at startup. Bloom's HDR
    // buffer is `Rg11b10Ufloat`, not renderable without the first.
    #[allow(unused_mut, reason = "the solari feature adds to them")]
    let mut features = wgpu::Features::RG11B10UFLOAT_RENDERABLE
        | wgpu::Features::FLOAT32_FILTERABLE
        | wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES;
    #[allow(unused_mut, reason = "the solari feature adds to them")]
    let mut limits = wgpu::Limits::default();
    #[cfg(feature = "solari")]
    {
        // The adapter is not known until Blitz has made the device, so
        // the binding-array limit is a figure any RT-capable adapter
        // clears rather than the adapter's own.
        features |= ignition_viz::solari::required_features();
        limits.max_binding_array_elements_per_shader_stage = 100_000;
        limits.max_binding_array_sampler_elements_per_shader_stage = 1_000;
    }
    let config: Vec<Box<dyn Any>> = vec![
        Box::new(features),
        Box::new(limits),
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

    windows::with_app_id(
        WindowAttributes::default()
            .with_title("Ignition Studio")
            .with_surface_size(LogicalSize::new(1600, 950)),
    )
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

/// A palette colour as CSS. Linear 0–1 in, plain bytes out.
fn rgb(c: &ignition_core::preset::ColorPreset) -> String {
    format!(
        "rgb({} {} {})",
        (c.red * 255.0) as u8,
        (c.green * 255.0) as u8,
        (c.blue * 255.0) as u8
    )
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

/// The named things every window's panels draw from, resolved once in
/// `main` and read by whichever window asks. A global, like the
/// channels: a second window is a second `VirtualDom`, and nothing can
/// be handed across that boundary except through the process.
static SURFACE: std::sync::OnceLock<Surface> = std::sync::OnceLock::new();

fn surface() -> &'static Surface {
    SURFACE
        .get()
        .expect("surface is set before any window renders")
}

/// The URLs the Live server listens on, if it is running; shown on the
/// mode strip so they can be typed into Safari.
static LAN: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();

/// What every Live host draws from — the desk's own copy of what the
/// browser is sent at connect, so the two views are one set of data.
// r[impl studio.touch.presence] - desk and iPad start from the same bootstrap
pub fn bootstrap() -> ignition_live_ui::Bootstrap {
    ignition_live_ui::Bootstrap {
        surface: surface().clone(),
        banks: desk::load(&venue_dir()),
        operator: operators::Operator::current(),
        // The desk reads the profile file itself; only the browser
        // needs it sent.
        profile: None,
        lan: LAN.get().cloned().unwrap_or_default(),
    }
}

/// The playhead the widget publishes, fed into this window's tree as
/// the signal every `use_playhead` reads (`ignition_live_ui::PlayheadFeed`).
///
/// Polled rather than awaited on `changed()`: the UI already repaints at
/// ~60 Hz to keep the visualizer animating (see the `frame` ticker), so
/// a second timer costs nothing and avoids holding a mutable borrow of
/// the receiver across an await inside a component. Only written when
/// it actually moved: a signal write is a re-render.
pub fn provide_playhead() {
    let mut playhead = use_signal(command::Playhead::default);
    use_context_provider(|| PlayheadFeed(playhead));
    use_future(move || async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(33)).await;
            if let Some(rx) = STATE_RX.get() {
                let latest = rx.borrow().clone();
                if latest != playhead() {
                    playhead.set(latest);
                }
            }
        }
    });
}

/// The launch window's root. Installs the panel host for the whole
/// process, loads the bank, and draws window 0; the layout's other
/// windows are opened from here once this one exists — there has to be
/// a running event loop before there can be a second window.
// r[impl studio.operators.layout] - the layout is restored at launch for `IGNITION_OPERATOR`
fn app(surface: Surface) -> Element {
    let _ = SURFACE.set(surface);

    // Load every page of the bank once. They queue in the channel until
    // the visualizer exists to drain them, so this does not have to wait
    // for the widget to be built.
    // The whole resolved fader goes — filter, parameters, the further
    // recipes of a bundle or look, a role master — not just its first
    // recipe, so what the page declared is what the engine plays.
    // r[impl profile.pages] - the bank is the profile's pages, whole
    // r[impl profile.attribute-filter] - the filter reaches the engine
    // r[impl profile.effect-parameters] - and the params, at their defaults
    use_hook(|| {
        for (page, bank) in faders::bank_pages().into_iter().enumerate() {
            for (index, spec) in bank.into_iter().enumerate() {
                send(Command::FaderOnPage {
                    page,
                    index,
                    fader: Box::new(ignition_core::Fader {
                        level: 0.0,
                        ..spec.fader
                    }),
                });
            }
        }
    });

    // The layout: the selected operator's, or today's one window. The
    // host is installed before the first render of any panel, and the
    // remaining windows are asked for now — they open on the next turn
    // of the event loop, each with its own root.
    use_hook(|| {
        let layout = match layout::selected_operator() {
            Some(name) => match layout::load(&name) {
                Ok(layout) => {
                    tracing::info!(
                        operator = name,
                        windows = layout.windows.len(),
                        "studio: layout"
                    );
                    layout
                }
                Err(e) => {
                    tracing::warn!(operator = name, error = %e, "studio: no layout; one window");
                    windows::LEGACY_PLACEMENT.store(1, std::sync::atomic::Ordering::SeqCst);
                    layout::Layout::default_single_window()
                }
            },
            None => {
                windows::LEGACY_PLACEMENT.store(1, std::sync::atomic::Ordering::SeqCst);
                layout::Layout::default_single_window()
            }
        };
        let host = windows::Host::from_layout(&layout);
        let others: Vec<windows::HostId> = host.ids().into_iter().skip(1).collect();
        windows::install(host);
        for id in others {
            windows::open(id);
        }
    });

    rsx! {
        windows::WindowRoot { host: 0 }
    }
}

/// mm:ss, for the transport readout.
fn clock(secs: f32) -> String {
    let secs = secs.max(0.0) as u32;
    format!("{}:{:02}", secs / 60, secs % 60)
}

/// The transport bar across the top of the visualizer.
///
/// Above the picture rather than beside the cue list, because that is
/// where the eye already is. An operator watching the rig should not
/// have to look away to see where the song is — and a progress bar
/// tucked into a sidebar reads as a widget rather than as the timeline
/// the whole show hangs on.
///
/// The bar is the full width of the view, so position on screen maps
/// directly to position in the song: halfway across is halfway through,
/// with no scaling to think about.
#[component]
fn Transport() -> Element {
    let playhead = use_playhead();
    let desk = use_desk();
    // A viewport option, so it lives here rather than in the desk state
    // the engine owns. Starts where the viz started.
    let mut body_glow = use_signal(body_glow_default);
    let seek = move |event: Event<MouseData>| {
        let x = event.data().element_coordinates().x;
        send(Command::Scrub((x / TRACK_WIDTH) as f32));
    };
    // A transport key is a `Key` with no fader under it: the engine
    // hands the request on to the song playback.
    let key = |action: ignition_core::KeyAction, index: usize| {
        send(Command::Key {
            index,
            action,
            down: true,
        })
    };

    rsx! {
        header { class: "transport-bar",
            button {
                class: if playhead().playing { "t-btn on" } else { "t-btn" },
                onclick: move |_| send(Command::Play),
                "▶"
            }
            button {
                class: "t-btn",
                onclick: move |_| send(Command::Stop),
                "■"
            }
            span { class: "t-time", "{clock(playhead().secs)}" }
            div {
                class: "t-track",
                onclick: seek,
                div { class: "t-fill", style: "width: {playhead().fraction() * 100.0}%" }
            }
            span { class: "t-time dim", "{clock(playhead().length)}" }
            // The song *playback*'s keys, distinct from the song's
            // transport to their left: pause holds the cue list where
            // it is without stopping the music, GO− steps it back over
            // its own times, LOAD arms the next cue as the next GO.
            // r[impl playback.temp-and-pause] - pause / resume and go back, on the surface
            div { class: "t-keys",
                button {
                    class: if desk().paused { "t-key on" } else { "t-key" },
                    onclick: move |_| key(ignition_core::KeyAction::Pause, 0),
                    if desk().paused { "PAUSED" } else { "PAUSE" }
                }
                button {
                    class: "t-key",
                    onclick: move |_| key(ignition_core::KeyAction::GoBack, 0),
                    "GO−"
                }
                button {
                    class: "t-key",
                    onclick: move |_| {
                        let next = playhead().cue.map_or(0, |c| c + 1);
                        key(ignition_core::KeyAction::Load, next)
                    },
                    "LOAD ▸"
                }
            }
            // The sound-in: the fade the band levels settle over, and
            // the three levels as the recipes read them. Small, because
            // they are a meter and a trim, not a control the hand rides.
            // r[impl playback.sound-as-value] - the sound fade and the meters
            // The transmitter: lit while bytes leave the socket, red
            // when they cannot, and the per-universe detail beside it
            // so a desk that is not sending is never quiet about it.
            // r[impl dmx.output-toggle] - the OUTPUT key and its status
            div { class: "t-output",
                button {
                    class: output_class(&playhead().output),
                    title: "{playhead().output.lines.join(\"\\n\")}",
                    onclick: move |_| send(Command::Output(!playhead().output.enabled)),
                    "OUTPUT"
                }
                span { class: "t-label out-status", "{output_status(&playhead().output)}" }
            }
            // Fixture body glow: whether a lit housing shows its colour.
            // Off by default — the real fixtures are black.
            // r[impl viz.body-glow] - the GLOW key
            div { class: "t-output",
                button {
                    class: if body_glow() { "t-key on" } else { "t-key" },
                    title: "Fixture body glow: lit housings glow their own colour",
                    onclick: move |_| {
                        let on = !body_glow();
                        body_glow.set(on);
                        send(Command::BodyGlow(on));
                    },
                    "GLOW"
                }
            }
            div { class: "t-sound",
                span { class: "t-label", "SOUND FADE" }
                HSlider {
                    initial: 0.125,
                    on_change: move |v: f32| send(Command::SoundFade(v * viz_widget::SoundFade::MAX_SECS)),
                }
                div { class: "meters",
                    for (i, name) in ["LO", "MID", "HI"].into_iter().enumerate() {
                        div { class: "meter", key: "{name}",
                            div { class: "meter-track",
                                div {
                                    class: "meter-fill",
                                    style: "height: {(desk().sound[i].clamp(0.0, 1.0) * 100.0) as u32}%",
                                }
                            }
                            span { class: "meter-label", "{name}" }
                        }
                    }
                }
            }
        }
    }
}

/// The OUTPUT key's class: `on` while sending, `err` when the socket
/// or the bind failed — whatever the switch says, since a lit key over a
/// dead socket is the lie the spec forbids.
// r[impl dmx.output-toggle] - errored beats enabled on the key
fn output_class(output: &ignition_viz::OutputSummary) -> &'static str {
    if output.error.is_some() {
        "t-key output err"
    } else if output.enabled {
        "t-key output on"
    } else {
        "t-key output"
    }
}

/// The status beside the key — the overlay's own line, minus its `OUT`
/// prefix, since the key already says that.
fn output_status(output: &ignition_viz::OutputSummary) -> String {
    output.line().trim_start_matches("OUT ").to_string()
}

/// The transport track's width in CSS pixels, matching `studio.css`.
///
/// Blitz reports pointer positions relative to the element but does not
/// hand out the element's own width, so the click-to-seek maths needs
/// the number from somewhere. Kept next to the component rather than
/// only in the stylesheet, so the two are at least adjacent when one
/// changes — and the track is a fixed width rather than `flex: 1` for
/// exactly this reason: a bar that stretched would seek to the wrong
/// place on any window that was not the size this number assumes.
const TRACK_WIDTH: f64 = 980.0;

// Superseded on screen by `live::Views`, which the panel host mounts as
// the desk (`windows.rs`); kept, unmounted, until the Live surface has
// been judged against it on a real show. Delete together with `Fader`,
// `PARK_ATTRS` and `TRACK` when that has happened.
#[allow(dead_code)]
#[component]
fn Busking(surface: Surface) -> Element {
    let mut selected = use_signal(|| Option::<String>::None);
    let mut soloed = use_signal(|| false);
    let mut blind = use_signal(|| false);
    let mut highlight = use_signal(|| false);
    let mut lowlight = use_signal(|| false);
    // What the per-fader keys do. One mode for the row rather than five
    // keys under each fader: a hand learns one key per fader, and the
    // mode is a decision made before the song, not during it.
    let mut key_mode = use_signal(|| ignition_core::KeyAction::Flash);
    // Which profile look is latched on the held layer, if any — so the
    // key that took it is drawn lit, and pressing it again lets go.
    let mut look_on: Signal<Option<String>> = use_signal(|| None);
    let desk = use_desk();
    let pages = faders::bank_pages();
    let page_names = faders::page_names();

    rsx! {
        section { class: "surface",
            div { class: "col groups",
                header {
                    span { "Groups" }
                    // Solo is its own control rather than a modifier on
                    // the group buttons, because it is not a selection:
                    // selection is what the *next* palette hit lands on,
                    // and a solo has to change the output without
                    // changing what is armed. Bound to the role a group
                    // plays, so it means the same thing at any venue.
                    button {
                        class: "solo",
                        onclick: move |_| {
                            let next = if soloed() { None } else { Some("Key".to_string()) };
                            soloed.set(next.is_some());
                            send(Command::Solo(next));
                        },
                        if soloed() { "SOLO ✓" } else { "SOLO KEY" }
                    }
                }
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
                    for chip in surface.splits.iter().cloned() {
                        button {
                            key: "split-{chip.name}",
                            class: "swatch split",
                            onclick: {
                                let name = chip.name.clone();
                                move |_| send(Command::Split(name.clone()))
                            },
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
                // Park: nail the selection's pan, tilt and dimmer at
                // what the hand is holding, above every playback and
                // the hand itself. A motor screaming on one tilt is
                // fixed here in a second; nothing else on the surface
                // can hold a value against a cue.
                // r[impl playback.park] - park and unpark the selection
                div { class: "row",
                    button {
                        class: "tile park",
                        onclick: move |_| {
                            if let Some(name) = selected() {
                                send(Command::Park {
                                    selection: Selection::Group(name),
                                    attrs: PARK_ATTRS.to_vec(),
                                });
                            }
                        },
                        "PARK"
                    }
                    button {
                        class: "tile park",
                        onclick: move |_| {
                            if let Some(name) = selected() {
                                send(Command::Unpark {
                                    selection: Selection::Group(name),
                                    attrs: PARK_ATTRS.to_vec(),
                                });
                            }
                        },
                        "UNPARK"
                    }
                    if desk().parked > 0 {
                        span { class: "parked-flag", "PARKED {desk().parked}" }
                    }
                }
                // The programmer's modes. Blind is loud when on for the
                // same reason solo is: a desk left blind is a desk whose
                // every punch goes nowhere, and it has to look wrong.
                div { class: "row modes",
                    button {
                        class: if blind() { "tile mode blind on" } else { "tile mode" },
                        onclick: move |_| {
                            let next = !blind();
                            blind.set(next);
                            send(Command::Blind(next));
                        },
                        if blind() { "BLIND ●" } else { "BLIND" }
                    }
                    button {
                        class: if highlight() { "tile mode on" } else { "tile mode" },
                        onclick: move |_| {
                            let next = !highlight();
                            highlight.set(next);
                            send(Command::Highlight(next));
                        },
                        "HIGHLT"
                    }
                    button {
                        class: if lowlight() { "tile mode on" } else { "tile mode" },
                        onclick: move |_| {
                            let next = !lowlight();
                            lowlight.set(next);
                            send(Command::Lowlight(next));
                        },
                        "LOWLT"
                    }
                }
            }

            div { class: "col faders",
                header {
                    span { "Faders" }
                    // The page strip. The eight physical faders carry
                    // another eight assignments per page; a fader that
                    // is up stays live on the old page's assignment —
                    // drawn latched — until it is brought back to match.
                    div { class: "pages",
                        button { class: "page-btn", onclick: move |_| send(Command::Page(PageMove::Prev)), "◀" }
                        // The page's own name from the profile, beside
                        // its number — a hand turning to "colour" reads
                        // the word, not the digit.
                        // r[impl profile.pages] - the page is named at the desk
                        span { class: "page-num", "page {desk().page + 1} / {desk().pages} · {page_names.get(desk().page).cloned().unwrap_or_default()}" }
                        button { class: "page-btn", onclick: move |_| send(Command::Page(PageMove::Next)), "▶" }
                    }
                    // Key mode for the row.
                    div { class: "key-modes",
                        for (mode, label) in [
                            (ignition_core::KeyAction::Flash, "FLASH"),
                            (ignition_core::KeyAction::Toggle, "TOGGLE"),
                            (ignition_core::KeyAction::Swap, "SWAP"),
                            (ignition_core::KeyAction::Kill, "KILL"),
                            (ignition_core::KeyAction::Black, "BLACK"),
                            // r[impl playback.temp-and-pause] - temp as a key mode
                            (ignition_core::KeyAction::Temp, "TEMP"),
                        ] {
                            button {
                                key: "{label}",
                                class: if key_mode() == mode { "key-mode on" } else { "key-mode" },
                                onclick: move |_| key_mode.set(mode),
                                "{label}"
                            }
                        }
                    }
                    if desk().blind {
                        span { class: "blind-flag", "BLIND — viewport is a preview" }
                    }
                }
                div { class: "fader-row",
                    for i in 0..ignition_core::FADERS {
                        {
                            // Labelled from the page the desk says it is
                            // on, not the one last clicked: a MIDI page
                            // key turns the strip too.
                            let page = pages.get(desk().page).unwrap_or(&pages[0]);
                            let spec = &page[i];
                            let latched = desk().latched[i];
                            let toggled = desk().toggled[i];
                            let params = spec.params.clone();
                            rsx! {
                                div { class: "fader-slot", key: "{i}",
                                    Fader {
                                        label: spec.name.to_string(),
                                        css: spec.css.to_string(),
                                        initial: 0.0,
                                        latched,
                                        on_change: move |v: f32| send(Command::Level(i, v)),
                                    }
                                    // The effect parameters the page
                                    // declared for this fader — depth,
                                    // bars, duty — one thin slider each,
                                    // mapped over the declared range and
                                    // opening at the default.
                                    // r[impl profile.effect-parameters] - the second control beside the level
                                    for param in params.iter() {
                                        {
                                            let name = param.name.clone();
                                            let label = name.clone();
                                            let (min, max) = (param.min, param.max);
                                            let span = (max - min).max(f32::EPSILON);
                                            let initial = ((param.default - min) / span).clamp(0.0, 1.0);
                                            rsx! {
                                                div { class: "param", key: "{i}-{label}", title: "{label}",
                                                    span { class: "param-name", "{label}" }
                                                    HSlider {
                                                        initial,
                                                        on_change: move |v: f32| send(Command::Param {
                                                            index: i,
                                                            name: name.clone(),
                                                            value: min + v * span,
                                                        }),
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    // The playback key under the fader.
                                    // Held modes send the release; toggle
                                    // and kill are a press.
                                    button {
                                        class: if toggled { "pkey on" } else { "pkey" },
                                        onpointerdown: move |_| send(Command::Key { index: i, action: key_mode(), down: true }),
                                        onpointerup: move |_| send(Command::Key { index: i, action: key_mode(), down: false }),
                                        onpointerleave: move |_| send(Command::Key { index: i, action: key_mode(), down: false }),
                                        "●"
                                    }
                                }
                            }
                        }
                    }
                    // Flash keys. Momentary by nature. The fired ones
                    // are the same bump a charted snare fires — one
                    // gesture arriving from two places, not two features
                    // that resemble each other. The held ones (DROP,
                    // PUNT) are on for exactly as long as the pointer is
                    // down: the hand is the release, and `pointerleave`
                    // counts as the hand coming off so a drag out of the
                    // key cannot leave the stage in a punt.
                    div { class: "flashes",
                        for key in faders::flash_keys() {
                            match key.action {
                                faders::KeyAction::Flash(target, kind) => rsx! {
                                    button {
                                        key: "{key.label}",
                                        class: "flash",
                                        onpointerdown: move |_| {
                                            send(Command::Flash(target.clone(), kind))
                                        },
                                        "{key.label}"
                                    }
                                },
                                faders::KeyAction::Hold(recipe) => rsx! {
                                    button {
                                        key: "{key.label}",
                                        class: if key.label == "PUNT" { "flash hold punt" } else { "flash hold" },
                                        onpointerdown: move |_| {
                                            // `KeyAction::Hold` is boxed and so is
                                            // `Command::Hold` — the clone moves straight
                                            // across with no second allocation.
                                            send(Command::Hold(Some(recipe.clone())))
                                        },
                                        onpointerup: move |_| send(Command::Hold(None)),
                                        onpointerleave: move |_| send(Command::Hold(None)),
                                        "{key.label}"
                                    }
                                },
                            }
                        }
                    }
                    // Macro keys and look keys. A macro is the two-key
                    // move written down — one press runs the drop, the
                    // build, the breakdown, the end, on the song's
                    // beats. A look is a place to *be*: pressed once it
                    // latches on the held layer, pressed again it lets
                    // go, and another look replaces it.
                    // r[impl playback.macro-runner] - MACRO keys
                    // r[impl playback.look-hold] - LOOK keys
                    div { class: "flashes macros",
                        for key in faders::macro_keys() {
                            button {
                                key: "macro-{key.label}",
                                class: "flash",
                                onclick: move |_| send(Command::Macro(key.name.to_string())),
                                "{key.label}"
                            }
                        }
                        for key in faders::look_keys() {
                            button {
                                key: "look-{key.label}",
                                class: if look_on().as_deref() == Some(key.name) { "flash hold punt" } else { "flash hold" },
                                onclick: move |_| {
                                    let next = if look_on().as_deref() == Some(key.name) {
                                        None
                                    } else {
                                        Some(key.name.to_string())
                                    };
                                    look_on.set(next.clone());
                                    send(Command::Look(next));
                                },
                                "{key.label}"
                            }
                        }
                    }
                    // Role masters. Not a dimmer on the cue — the cue
                    // list keeps saying what it was saying, and this
                    // says how much of it reaches the rig. Pulling
                    // Movers down for a ballad is a decision an operator
                    // makes in the moment and should not have to edit a
                    // show to express.
                    div { class: "masters",
                        // The grand master: every intensity, last of
                        // all, after parks. The one fader that is
                        // never on a page and never hidden.
                        // r[impl playback.grand-master] - the GM fader
                        Fader {
                            label: "GM".to_string(),
                            css: "#e05050".to_string(),
                            initial: 1.0,
                            on_change: move |v: f32| send(Command::Grand(v)),
                        }
                        // Each list's own master: the song list pulled
                        // under the look list without editing either.
                        // r[impl playback.playback-master] - SONG and LOOK faders
                        Fader {
                            label: "SONG".to_string(),
                            css: "#c8a050".to_string(),
                            initial: 1.0,
                            on_change: move |v: f32| {
                                send(Command::PlaybackMaster(ignition_core::Class::Song, v))
                            },
                        }
                        Fader {
                            label: "LOOK".to_string(),
                            css: "#a0c850".to_string(),
                            initial: 1.0,
                            on_change: move |v: f32| {
                                send(Command::PlaybackMaster(ignition_core::Class::Look, v))
                            },
                        }
                        for role in ["Key", "Wash", "Movers", "Bars"] {
                            Fader {
                                key: "{role}",
                                label: role.to_string(),
                                css: "#7a6f96".to_string(),
                                initial: 1.0,
                                on_change: move |v: f32| {
                                    send(Command::Master(role.to_string(), v))
                                },
                            }
                        }
                    }
                    // The three an operator actually rides. RATE sets
                    // the tap tempo; SIZE and SPEED shape whatever is
                    // running against it.
                    div { class: "master",
                        div { class: "rate-slot",
                            Fader {
                                label: "RATE".to_string(),
                                css: "#c08a3e".to_string(),
                                initial: 0.4,
                                // 40–220 BPM over the fader's travel.
                                on_change: move |v: f32| send(Command::Rate(40.0 + v * 180.0)),
                            }
                            // The speed keys on the `Tap` master. TAP
                            // learns — averaged, so one early tap does
                            // not throw the phasers; ½ and ×2 ride a
                            // breakdown and a drop; RESET is back to
                            // the learned tempo at ×1.
                            // r[impl playback.speed-keys] - learn, half, double, reset
                            div { class: "speed-keys",
                                button {
                                    class: "speed-key tap",
                                    onpointerdown: move |_| send(Command::Speed(SpeedKey::Tap)),
                                    "TAP"
                                }
                                button {
                                    class: if desk().tap_multiplier < 0.99 { "speed-key on" } else { "speed-key" },
                                    onclick: move |_| send(Command::Speed(SpeedKey::Half)),
                                    "½"
                                }
                                button {
                                    class: if desk().tap_multiplier > 1.01 { "speed-key on" } else { "speed-key" },
                                    onclick: move |_| send(Command::Speed(SpeedKey::Double)),
                                    "×2"
                                }
                                button {
                                    class: "speed-key",
                                    onclick: move |_| send(Command::Speed(SpeedKey::Reset)),
                                    "RESET"
                                }
                                span { class: "tap-readout",
                                    if desk().tap_bpm > 0.0 { "{desk().tap_bpm as u32} bpm" } else { "— bpm" }
                                }
                            }
                        }
                        // Size, not a dimmer. At the bottom every effect
                        // is inert and the look underneath shows through
                        // unchanged — a withdrawal, not a blackout.
                        Fader {
                            label: "SIZE".to_string(),
                            css: "#6ea8c0".to_string(),
                            initial: 1.0,
                            on_change: move |v: f32| send(Command::Size(v)),
                        }
                        // Half to double, with the middle at unity so
                        // the neutral position is somewhere a hand can
                        // find without looking.
                        Fader {
                            label: "SPEED".to_string(),
                            css: "#8fb06e".to_string(),
                            initial: 0.5,
                            on_change: move |v: f32| send(Command::EffectRate(0.5 + v * 1.5)),
                        }
                        // Program time: how long every punch takes to
                        // arrive, 0–4 beats. At the bottom a palette
                        // snaps; a little way up a busk stays smooth
                        // without every gesture being pre-timed.
                        Fader {
                            label: "PROG".to_string(),
                            css: "#b06e8f".to_string(),
                            initial: 0.0,
                            on_change: move |v: f32| send(Command::ProgramTime(v * 4.0)),
                        }
                    }
                }
            }
        }
    }
}

/// What PARK and UNPARK act on: the three attributes a stuck fixture
/// is most often stuck on. A colour park would be a surprise — the
/// cue changes colour and one fixture does not — where a tilt park is
/// the whole point.
#[allow(dead_code)]
const PARK_ATTRS: [ignition_core::Attribute; 3] = [
    ignition_core::Attribute::Pan,
    ignition_core::Attribute::Tilt,
    ignition_core::Attribute::Dimmer,
];

/// How tall a fader's track is, in CSS pixels.
///
/// Known rather than measured: the value comes from the pointer's
/// position *within the element*, and a layout query in an event handler
/// is exactly the thing that deadlocks Blitz. Keep this in step with
/// `.track` in studio.css.
#[allow(dead_code)]
const TRACK: f32 = 190.0;

/// A fader built from divs rather than `<input type=range>`.
///
/// Three reasons, in order: a range input renders as a bare white bar in
/// Blitz with no way to style the fill; touch needs a hit target far
/// larger than a native thumb; and the value has to be readable as a
/// colour at a glance from two metres away, which a native control
/// cannot do.
#[allow(dead_code)]
#[component]
fn Fader(
    label: String,
    css: String,
    initial: f32,
    #[props(default)] latched: bool,
    on_change: EventHandler<f32>,
) -> Element {
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
        // Latched: the page turned under a fader that was up, and the
        // old assignment is still playing at the old level until the
        // hand brings this one back to match. Drawn hollow so the
        // operator can see which faders are not yet theirs.
        div { class: if latched { "fader latched" } else { "fader" },
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
            venue: Venue::load(venue_dir()).expect("venue already loaded once in main"),
            view: ViewPreset::House,
            width: 1280,
            height: 800,
            // 1.6, the same as `viz` — see `viz.rs`: enough particulate
            // that a par's cone reads in the house, not only its pool.
            haze: 1.6,
            // A little fill so the room reads even in a dark look — the
            // operator is looking at a panel, not sitting in the venue.
            ambient: 0.05,
            show_props: true,
            exclude: Vec::new(),
            // 12, the same as `viz` — a multiplier on real candela, see
            // `spawn::spot_lumens`. It was 2500 back when the spill
            // light was fed raw lumens.
            exposure: 0.0,
            auto_exposure: true,
            grade: ignition_viz::Grade::Neutral,
            screen_content: Some("screens/rockstars-logo.webp".to_string()),
            // The three back-wall TVs share one canvas, so this single
            // image spans all of them and each takes the slice matching
            // its real width — the whole point of canvases. The two side
            // TVs are their own canvases and play independently; they are
            // where lyrics will go, over a bed like this one.
            //
            // Stills because there is no clip in the repo to point at,
            // not because these have to be stills: a `.mov` or `.mp4`
            // here plays instead, decided by the extension alone (see
            // `ignition_viz::video`). What is still missing is the
            // clock — until the widget puts
            // `CanvasClock::at(transport.position())` into the viz world
            // beside the cue seek it already does in `follow_song`, a
            // clip free-runs rather than scrubbing with the song.
            //
            // `IGNITION_CANVAS_MAIN` / `_SIDE_LEFT` / `_SIDE_RIGHT` name a
            // clip or still to use instead — an absolute path, or one
            // under the assets dir.
            //
            // The clips live in Cody's `Lighting Resources` folder (or
            // wherever `IGNITION_CLIPS` points); the stills are the
            // fallback when a clip is not on this machine, so the
            // studio still opens with something on the wall.
            canvas_content: [
                (
                    "main",
                    "IGNITION_CANVAS_MAIN",
                    "RUN FOR YOUR LIFE - Looping Background Animation.mp4",
                    "screens/clip-particles.png",
                ),
                (
                    "side-left",
                    "IGNITION_CANVAS_SIDE_LEFT",
                    "Astronaut Blue Background Loop - Animation Videos _ No Copyright _ Visual Effects Video..mp4",
                    "screens/clip-astronaut.png",
                ),
                (
                    "side-right",
                    "IGNITION_CANVAS_SIDE_RIGHT",
                    "Astronaut Blue Background Loop - Animation Videos _ No Copyright _ Visual Effects Video..mp4",
                    "screens/clip-astronaut.png",
                ),
            ]
            .into_iter()
            .map(|(canvas, var, clip, still)| (canvas.to_string(), canvas_source(var, clip, still)))
            .collect(),
            // The back wall shows a band of its clip; sit it a little
            // below centre so the runner's face is in it.
            // `IGNITION_CANVAS_MAIN_FOCUS=0.7` slides it further.
            canvas_focus: [(
                "main".to_string(),
                std::env::var("IGNITION_CANVAS_MAIN_FOCUS")
                    .ok()
                    .and_then(|v| v.trim().parse().ok())
                    .unwrap_or(0.56),
            )]
            .into_iter()
            .collect(),
            assets_dir: concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../crates/ignition-viz/assets"
            )
            .to_string(),
            max_universe: 4,
            snapshot: None,
            settle_frames: 1,
            // Off unless `IGNITION_BODY_GLOW=1`, the same way the other
            // viewport options are carried between launches; the GLOW
            // key in the transport bar flips it live.
            // r[impl viz.body-glow] - off by default, remembered by env
            body_glow: body_glow_default(),
            labels: false,
            camera: None,
            // Loaded with the venue when the widget activates — see
            // `viz_widget::VizCore::activate`.
            cameras: Default::default(),
            camera_preset: None,
            overlay: false,
            // On, because the visualizer is the thing that has to hold
            // 120 fps and a number in the corner is how anyone notices
            // it stopped. Note what it measures here: embedded, Bevy
            // draws when Blitz asks it to, so this is the rate the
            // operator actually sees — the composite, not the renderer
            // in isolation. Standalone `viz --fps` measures the latter.
            fps: true,
            // 128 fog steps (the fewest that keeps a mover's beam from
            // flickering) and no MSAA, which does not show through bloom.
            quality: RenderQuality::live(),
            // Off until the OUTPUT key is pressed, or `IGNITION_OUTPUT=1`
            // starts the desk sending. The transmitter is bound either
            // way, so the key is instant.
            // r[impl dmx.output-toggle] - the desk starts silent unless told otherwise
            output: std::env::var("IGNITION_OUTPUT").is_ok_and(|v| v == "1"),
            // `IGNITION_LOOPBACK=1` is read by the viz itself; see
            // `ignition_viz::output::LoopbackSink` for what it is for.
            loopback: false,
        };
        // The core is built once per process; a Visualizer panel hosted
        // by a later window (or popped out and docked back) attaches to
        // the one that exists, so the receiver is taken only the first
        // time.
        CustomWidgetAttr::new(VizWidget::attach(|| {
            let rx = RX
                .lock()
                .expect("fresh mutex")
                .take()
                .expect("one visualizer core");
            let report = STATE_TX
                .lock()
                .expect("fresh mutex")
                .take()
                .expect("one visualizer core");
            viz_widget::VizCore::new(config, Some((show_path(), 0)), Some(PROJECT), rx, report)
        }))
    });

    // Blitz repaints on demand, so something has to ask for the next
    // frame. It asks *winit*, directly.
    //
    // This used to bump a signal written into a `data-frame` attribute,
    // on the reasoning that a `Widget` has no way to request a frame and
    // a dirty DOM is what makes Blitz redraw. It does — and it also
    // makes Blitz re-render a component, diff it, mutate the document,
    // restyle it and hand the whole tree to taffy, sixty times a second,
    // to change a number nothing reads. Four milliseconds a frame of
    // layout for a picture that had not changed shape since the window
    // opened.
    //
    // `use_window` hands over the `Arc<dyn Window>` this component's
    // window was built on, and `request_redraw` is exactly the ask: the
    // next `RedrawRequested` paints, the widget steps the visualizer
    // during that paint, and nothing in the document is touched at all.
    //
    // It still ticks when the widget says a frame is *done*, not on a
    // timer: a timer started after a vsync-blocking present always fired
    // a frame late, which halved the frame rate. The timeout is only for
    // before the widget is painting at all (no device yet), when nothing
    // would otherwise ask for the first frame.
    // r[impl studio.profiling] - the frame loop costs no layout
    let window = dioxus_native::use_window();
    use_future(move || {
        let window = window.clone();
        async move {
            let done = viz_widget::FRAME_DONE.clone();
            loop {
                let _ =
                    tokio::time::timeout(std::time::Duration::from_millis(100), done.notified())
                        .await;
                window.request_redraw();
            }
        }
    });

    rsx! {
        div { class: "viz",
            object { "data": widget_attr }
        }
    }
}

/// The viewport's starting position for fixture body glow: off, unless
/// `IGNITION_BODY_GLOW=1`. The real fixtures are black; the glow is an
/// affordance for reading the rig, not a picture of it.
// r[impl viz.body-glow] - off by default
fn body_glow_default() -> bool {
    std::env::var("IGNITION_BODY_GLOW").is_ok_and(|v| v == "1")
}

/// Where the clips are. `IGNITION_CLIPS` overrides the folder.
const CLIPS: &str = "/home/cody/Downloads/Lighting Resources";

/// A canvas's source: the env override if set, else the named clip in
/// the clips folder if it exists, else the still that ships in assets.
fn canvas_source(var: &str, clip: &str, still: &str) -> String {
    if let Ok(path) = std::env::var(var) {
        return path;
    }
    let dir = std::env::var("IGNITION_CLIPS").unwrap_or_else(|_| CLIPS.to_string());
    let path = std::path::Path::new(&dir).join(clip);
    if path.is_file() {
        return path.to_string_lossy().into_owned();
    }
    tracing::warn!(clip, dir, "clip not found; the canvas shows a still");
    still.to_string()
}

/// The rows the cue list draws. The player inside the visualizer owns
/// the real cues; this is only what to draw.
///
/// Nothing here cooks the list. Cooked status needs the assembled
/// `Show` — groups, palettes, library, looks — and that lives in the
/// visualizer, which is the one place it is built; a second assembly
/// here would be a second truth about what resolves. So the status
/// column reads "not cooked" rather than a guessed green until the
/// visualizer's own cook report is plumbed back.
// r[impl studio.one-truth] - the visualizer owns what resolves
fn load_cue_names(path: &str) -> anyhow::Result<Vec<Row>> {
    let raw = std::fs::read_to_string(path)?;
    let list: ignition_core::CueList = serde_json::from_str(&raw)?;
    Ok(ignition_live_ui::cuelist::rows(&list, None))
}
