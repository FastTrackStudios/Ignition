//! Where the frame went.
//!
//! The studio draws with two renderers in one process. Blitz lays the
//! document out and paints it with Vello; inside one element of that
//! paint, Bevy steps the whole visualizer and hands back a texture. A
//! frame time tells you the two of them together took 44 ms and nothing
//! about which one spent it — which is exactly the question, and the
//! reason the frame-rate readout has never been enough to act on.
//!
//! This is a [`tracing_subscriber::Layer`], and that is the whole
//! design. Every crate in the process already emits `tracing` spans, or
//! can be made to with one line: ours do, Bevy does with its `trace`
//! feature (`ignition-viz/trace`), the vendored Blitz crates do because
//! they are ours to patch. None of them has to depend on this crate, or
//! know it exists. Spans are the wire protocol; the profiler is a
//! reader.
//!
//! Two outputs, from the same measurements:
//!
//! * a table in the log every couple of seconds, sorted by **self**
//!   time — the time a stage spent that was not inside a nested stage.
//!   That column is the one to act on: `blitz.render` is always 100% of
//!   the frame and always tells you nothing.
//! * a Chrome trace-event file (`IGNITION_PROFILE_TRACE=<path>`), which
//!   opens in <https://ui.perfetto.dev> and is plain JSON, so a script
//!   can read it too.
//!
//! Deliberately *not* Tracy. Bevy 0.19 pins `tracing-tracy` 0.11, whose
//! protocol wants a Tracy 0.11 server; nixpkgs ships 0.13. Matching
//! those up is a build rabbit hole for a picture, and the number this
//! answers to — which renderer is over budget — does not need one.
//!
//! # Running it
//!
//! ```text
//! IGNITION_PROFILE=1 just studio            # the stage table, every 2 s
//! IGNITION_PROFILE=all just studio          # plus every other span in the process
//! IGNITION_PROFILE_TRACE=/tmp/ig.json …     # and a Perfetto trace
//! ```

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tracing::Subscriber;
use tracing::span::{Attributes, Id};
use tracing_subscriber::layer::{Context, Layer};
use tracing_subscriber::registry::LookupSpan;

mod report;
mod trace;

pub use report::{Line, Report};

/// The stages the default table shows, outermost first.
///
/// Order is the nesting order, not an alphabetical one, because the
/// table is read as a breakdown: each row's self time is time the row
/// below it did not account for. A name here that nothing emits simply
/// does not appear — which is itself the signal that a stage was
/// compiled out (Bevy's spans without `ignition-viz/trace`) or never
/// ran (`viz.programme` with no Programme pane up).
pub const STAGES: &[&str] = &[
    // The winit loop itself. Measured because the render and document
    // stages together accounted for barely half of a studio frame, and
    // the missing half had to be somewhere — it is not a detail, it was
    // most of the answer.
    "loop.window_event",
    "loop.wake",
    "loop.wait",
    "loop.new_events",
    // Blitz: one per window, per frame.
    "blitz.render",
    "blitz.scene",
    "blitz.submit",
    "blitz.resolve",
    "blitz.style",
    "blitz.layout",
    // The visualizer, inside the scene of whichever window paints it.
    "viz.paint",
    "viz.commands",
    "viz.step",
    "viz.render",
    "viz.programme",
    // Bevy, with `ignition-viz/trace` on.
    "schedule",
    "system",
    "system_commands",
    "extract",
    "render",
];

/// The target every Ignition stage span carries.
///
/// One target for all of them, and not the emitting crate's module
/// path, so a single filter directive turns the whole profiler on
/// wherever the spans live — including inside the vendored Blitz crates,
/// whose own logging nobody wants turned up to reach them.
pub const TARGET: &str = "ignition::profile";

/// What to append to the log filter so the spans this reads are
/// actually created.
///
/// A span below the filter's level is never constructed, so a profiler
/// with nothing to say looks exactly like a fast frame. This is the
/// difference, and it belongs next to the layer rather than in whoever
/// installs it.
pub fn filter_directives() -> Vec<&'static str> {
    match std::env::var("IGNITION_PROFILE").as_deref() {
        Ok("all") => vec![
            "ignition::profile=info",
            // Bevy's own spans — one per system, per schedule, per
            // render pass — which exist only in a `ignition-viz/trace`
            // build and are the reason `all` is a separate mode.
            "bevy_ecs=info",
            "bevy_app=info",
            "bevy_render=info",
        ],
        Ok("1") | Ok("stages") | Ok("true") | Ok("on") => vec!["ignition::profile=info"],
        _ => Vec::new(),
    }
}

/// Which spans are measured.
enum Focus {
    /// [`STAGES`] and nothing else — cheap enough to leave on.
    Stages(HashSet<&'static str>),
    /// Every span in the process. Useful with `ignition-viz/trace`,
    /// which turns every Bevy system into one; expect the table to be
    /// long and the profiler itself to cost a millisecond or two.
    All,
}

impl Focus {
    fn wants(&self, name: &str) -> bool {
        match self {
            Focus::Stages(set) => set.contains(name),
            Focus::All => true,
        }
    }
}

/// Per-span state, parked in the registry's extensions for the span's
/// life. A span with no `Timing` is one the focus does not want, and
/// every hook below leaves it alone — that is the filter.
struct Timing {
    /// When the current enter began; `None` while not entered. A span
    /// can be entered and exited more than once, so this accumulates.
    entered: Option<Instant>,
    /// Time between enter and exit, summed.
    busy: Duration,
    /// How much of `busy` a nested measured span accounted for.
    child: Duration,
    /// First enter, for the trace file's timeline.
    began: Option<Instant>,
}

/// One span name's share of the window.
#[derive(Default, Clone)]
struct Row {
    calls: u64,
    busy: Duration,
    own: Duration,
    /// Per-call busy time in milliseconds, for the tail. Capped: a
    /// profiler that grows without bound is a leak with a nice table.
    samples: Vec<f32>,
}

const MAX_SAMPLES: usize = 8192;

/// What has accumulated since the last table.
#[derive(Default)]
struct Window {
    started: Option<Instant>,
    frames: u64,
    rows: HashMap<&'static str, Row>,
}

struct Shared {
    focus: Focus,
    /// The span whose close ends a frame — `blitz.render` by default,
    /// which is one window's whole paint.
    frame: String,
    interval: Duration,
    window: Mutex<Window>,
    trace: Option<trace::TraceWriter>,
    /// Wall clock for the trace file, so every event is relative to one
    /// origin across threads.
    origin: Instant,
}

/// The layer. Add it to the studio's registry with `.with(layer)`.
pub struct ProfileLayer {
    shared: Arc<Shared>,
}

/// Builds the layer from the environment, or `None` when
/// `IGNITION_PROFILE` is unset — which is the normal case, and costs
/// the process nothing.
///
/// * `IGNITION_PROFILE` — `1`/`stages` for [`STAGES`], `all` for every
///   span, anything else off.
/// * `IGNITION_PROFILE_INTERVAL` — seconds between tables, default 2.
/// * `IGNITION_PROFILE_FRAME` — the span that counts as a frame,
///   default `blitz.render`.
/// * `IGNITION_PROFILE_TRACE` — a path to write a Chrome trace-event
///   file to. Absent, none is written.
pub fn from_env() -> Option<ProfileLayer> {
    let mode = std::env::var("IGNITION_PROFILE").ok()?;
    let focus = match mode.as_str() {
        "all" => Focus::All,
        "1" | "stages" | "true" | "on" => Focus::Stages(STAGES.iter().copied().collect()),
        _ => return None,
    };
    let interval = std::env::var("IGNITION_PROFILE_INTERVAL")
        .ok()
        .and_then(|v| v.parse::<f64>().ok())
        .filter(|v| *v > 0.0)
        .unwrap_or(2.0);
    let frame =
        std::env::var("IGNITION_PROFILE_FRAME").unwrap_or_else(|_| "blitz.render".to_string());
    let trace = std::env::var("IGNITION_PROFILE_TRACE")
        .ok()
        .and_then(|path| match trace::TraceWriter::create(&path) {
            Ok(w) => {
                eprintln!("profile: writing a Perfetto trace to {path}");
                Some(w)
            }
            Err(e) => {
                eprintln!("profile: no trace file at {path}: {e}");
                None
            }
        });
    // `eprintln!`, not `tracing`: this runs while the subscriber is
    // being built, so an event here has nowhere to go yet — and a
    // profiler that silently did not start is the one failure mode
    // worth spending a line of stderr on.
    eprintln!(
        "profile: {mode} — a table every {interval:.1} s, frames counted by `{frame}`, \
         sorted by self time"
    );
    let shared = Arc::new(Shared {
        focus,
        frame,
        interval: Duration::from_secs_f64(interval),
        window: Mutex::new(Window::default()),
        trace,
        origin: Instant::now(),
    });
    spawn_reporter(Arc::clone(&shared));
    Some(ProfileLayer { shared })
}

impl Shared {
    /// Folds a closed span into the window.
    fn record(&self, name: &'static str, busy: Duration, own: Duration) {
        let mut window = self.window.lock().expect("profile window");
        window.started.get_or_insert_with(Instant::now);
        let row = window.rows.entry(name).or_default();
        row.calls += 1;
        row.busy += busy;
        row.own += own;
        if row.samples.len() < MAX_SAMPLES {
            row.samples.push(busy.as_secs_f32() * 1e3);
        }
        if name == self.frame {
            window.frames += 1;
        }
    }

    /// Takes everything since the last call and turns it into a table.
    /// `None` when no frame has closed since — a window with no frames
    /// in it is a paused studio, not a slow one, and printing zeroes at
    /// it would bury the last real table.
    fn drain(&self) -> Option<Report> {
        let mut window = self.window.lock().expect("profile window");
        if window.frames == 0 {
            return None;
        }
        let elapsed = window.started.map(|t| t.elapsed()).unwrap_or_default();
        let taken = std::mem::take(&mut *window);
        drop(window);
        Some(report::build(taken, elapsed))
    }
}

/// The reporter: one thread, asleep for the interval, printing what the
/// layer accumulated.
///
/// Printing from `on_close` instead would mean emitting a `tracing`
/// event from inside the subscriber's own span-close path, with that
/// span's registry entry half torn down. A thread costs nothing here
/// and the question does not arise.
fn spawn_reporter(shared: Arc<Shared>) {
    std::thread::Builder::new()
        .name("ignition-profile".into())
        .spawn(move || {
            loop {
                std::thread::sleep(shared.interval);
                if let Some(trace) = shared.trace.as_ref() {
                    trace.flush();
                }
                if let Some(report) = shared.drain() {
                    // The table goes out on the same target as the
                    // spans, so the one directive that turns the
                    // profiler on is also what lets its output through.
                    tracing::info!(target: TARGET, "{}", report.render());
                }
            }
        })
        .expect("profile reporter thread");
}

impl<S> Layer<S> for ProfileLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(&self, _attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(id) else { return };
        if !self.shared.focus.wants(span.name()) {
            return;
        }
        span.extensions_mut().insert(Timing {
            entered: None,
            busy: Duration::ZERO,
            child: Duration::ZERO,
            began: None,
        });
    }

    fn on_enter(&self, id: &Id, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(id) else { return };
        let mut ext = span.extensions_mut();
        let Some(timing) = ext.get_mut::<Timing>() else {
            return;
        };
        let now = Instant::now();
        timing.began.get_or_insert(now);
        timing.entered = Some(now);
    }

    fn on_exit(&self, id: &Id, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(id) else { return };
        let mut ext = span.extensions_mut();
        let Some(timing) = ext.get_mut::<Timing>() else {
            return;
        };
        if let Some(entered) = timing.entered.take() {
            timing.busy += entered.elapsed();
        }
    }

    fn on_close(&self, id: Id, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(&id) else { return };
        let (busy, child, began) = {
            let mut ext = span.extensions_mut();
            let Some(timing) = ext.get_mut::<Timing>() else {
                return;
            };
            // A span closed while still entered — an early return past
            // a guard, or a `.entered()` dropped with the span — still
            // counts what it did.
            if let Some(entered) = timing.entered.take() {
                timing.busy += entered.elapsed();
            }
            (timing.busy, timing.child, timing.began)
        };
        let own = busy.saturating_sub(child);
        let name = span.name();

        // Charge this span to the nearest *measured* ancestor, which is
        // not always the immediate parent: with `IGNITION_PROFILE=1` the
        // Bevy spans between `viz.step` and its systems are unmeasured,
        // and skipping them is what keeps `viz.step`'s self time honest.
        let mut parent = span.parent();
        while let Some(ancestor) = parent {
            let mut ext = ancestor.extensions_mut();
            if let Some(timing) = ext.get_mut::<Timing>() {
                timing.child += busy;
                break;
            }
            drop(ext);
            parent = ancestor.parent();
        }

        if let (Some(trace), Some(began)) = (self.shared.trace.as_ref(), began) {
            trace.push(
                name,
                began.saturating_duration_since(self.shared.origin),
                busy,
            );
        }

        self.shared.record(name, busy, own);
    }
}
