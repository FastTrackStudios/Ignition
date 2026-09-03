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

// A crate-wide suppression, which `docs/ops/clippy.md` calls a last
// resort. The argument for it here:
//
// This crate is *instrumentation*. Its entire job is to turn durations
// into numbers a person reads — `u64` nanoseconds into `f64`
// milliseconds, `u64` sample counts into `f32` — and to accumulate
// counters while doing it. Every one of those conversions is lossy in
// the way `as_conversions` means and none of them is lossy in a way
// that matters: a stage that took 3.7 ms is reported as 3.7 ms whether
// the arithmetic saturates or wraps, and a call counter that overflowed
// `u64` describes a studio that has been running for longer than the
// heat death of anything. Wrapping the tree's audited-helper idiom
// around a hundred sites here would add a layer of indirection to code
// whose only purpose is to be read while something else is being
// debugged.
//
// It is also off by default: nothing in this crate runs unless
// `IGNITION_PROFILE` is set, so none of it is in the path of a show.
#![expect(
    clippy::arithmetic_side_effects,
    reason = "see the paragraph above: this crate accumulates statistics, and it is off in a show build"
)]

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
    "loop.poll",
    "loop.redraw",
    "loop.resume",
    "loop.shell_other",
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
#[must_use]
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
        Ok("1" | "stages" | "true" | "on") => vec!["ignition::profile=info"],
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
            Self::Stages(set) => set.contains(name),
            Self::All => true,
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
    /// What to call this span in the table.
    label: Label,
}

/// A row's key.
///
/// Usually the span's name. Bevy is the exception that makes this worth
/// having: every one of its systems opens a span *called* `system` and
/// carries which system it is in a `name` field, so aggregating by name
/// alone produces one row saying "the systems took 19 ms" — which is
/// the question, not the answer.
#[derive(Clone)]
enum Label {
    Name(&'static str),
    Field(Box<str>),
}

impl Label {
    /// The span's `name` field if it has one, else its own name — read
    /// once, when the span opens, because a field is only visitable
    /// from the `Attributes` and the table needs it at close.
    fn of(attrs: &Attributes<'_>) -> Self {
        struct Take(Option<String>);
        impl tracing::field::Visit for Take {
            fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
                if field.name() == "name" {
                    self.0 = Some(value.to_string());
                }
            }
            fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
                if field.name() == "name" && self.0.is_none() {
                    self.0 = Some(format!("{value:?}").trim_matches('"').to_string());
                }
            }
        }
        let name = attrs.metadata().name();
        if attrs.metadata().fields().field("name").is_none() {
            return Self::Name(name);
        }
        let mut take = Take(None);
        attrs.record(&mut take);
        take.0.map_or(Self::Name(name), |value| {
            Self::Field(format!("{name} {value}").into_boxed_str())
        })
    }

    fn as_str(&self) -> &str {
        match self {
            Self::Name(name) => name,
            Self::Field(text) => text,
        }
    }
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
    rows: HashMap<Box<str>, Row>,
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
#[must_use]
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
    fn record(&self, name: &str, busy: Duration, own: Duration) {
        // See `trace.rs`: a poisoned window is a panic elsewhere, and
        // the profiler is not the place to turn one failure into two.
        let mut window = self
            .window
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        window.started.get_or_insert_with(Instant::now);
        // Looked up before it is inserted, so the steady state — every
        // row already present — allocates nothing. In `all` mode this
        // runs tens of thousands of times a frame.
        // `map_or_else`, which is what clippy asks for here, borrows
        // `window.rows` mutably in both arms at once and does not
        // compile. The match does, and the lookup-before-insert is the
        // point: in `all` mode this runs tens of thousands of times a
        // frame and must not allocate once every row exists.
        #[expect(
            clippy::option_if_let_else,
            reason = "the suggested map_or_else double-borrows `window.rows`"
        )]
        let row = match window.rows.get_mut(name) {
            Some(row) => row,
            None => window.rows.entry(name.into()).or_default(),
        };
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
        let mut window = self
            .window
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
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
    let spawned = std::thread::Builder::new()
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
        });
    if let Err(error) = spawned {
        // The OS refused a thread. The profiler then measures nothing
        // and says so once; taking the studio down over a *debugging*
        // feature failing to start would be the wrong trade in exactly
        // the situation where someone is already debugging something.
        tracing::warn!(
            target: TARGET,
            "the profiler could not start its reporter thread ({error}); no tables will be printed"
        );
    }
}

impl<S> Layer<S> for ProfileLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(id) else { return };
        if !self.shared.focus.wants(span.name()) {
            return;
        }
        span.extensions_mut().insert(Timing {
            entered: None,
            busy: Duration::ZERO,
            child: Duration::ZERO,
            began: None,
            label: Label::of(attrs),
        });
    }

    fn on_enter(&self, id: &Id, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(id) else { return };
        // `Instant::now()` before the lock, and the lock dropped as soon
        // as the write is done: `extensions_mut` is contended by every
        // thread closing a span, and this runs tens of thousands of
        // times a frame.
        let now = Instant::now();
        let mut ext = span.extensions_mut();
        let Some(timing) = ext.get_mut::<Timing>() else {
            return;
        };
        timing.began.get_or_insert(now);
        timing.entered = Some(now);
        drop(ext);
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
        drop(ext);
    }

    fn on_close(&self, id: Id, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(&id) else { return };
        let (busy, child, began, label) = {
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
            let taken = (
                timing.busy,
                timing.child,
                timing.began,
                timing.label.clone(),
            );
            // `extensions_mut` is contended by every thread closing a
            // span; the rest of this function does not need it.
            drop(ext);
            taken
        };
        let own = busy.saturating_sub(child);
        let name = label.as_str();

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

#[cfg(test)]
mod spec_tests {
    use super::*;

    /// The studio can say where a frame went, and the stages it names
    /// cover the whole of one.
    ///
    /// The reason the loop stages are in here at all is that the render
    /// and document stages together accounted for barely half a studio
    /// frame, and the missing half turned out to *be* the answer. A
    /// profiler that measures the parts somebody already suspected is
    /// the one that finds nothing.
    ///
    /// r[verify studio.profiling]
    #[test]
    fn every_stage_of_a_frame_is_named_and_carries_one_target() {
        for stage in [
            // Blitz's style, layout, scene build and submit.
            "blitz.style",
            "blitz.layout",
            "blitz.scene",
            "blitz.submit",
            // The visualizer's command drain, main-world step, render.
            "viz.commands",
            "viz.step",
            "viz.render",
            // And the loop around them, which is where the missing half
            // of the frame was.
            "loop.window_event",
            "loop.wait",
        ] {
            assert!(
                STAGES.contains(&stage),
                "`{stage}` is not a measured stage, so a frame spent there is invisible"
            );
        }

        // One target for all of them, so a single directive turns the
        // profiler on wherever the spans live — including inside the
        // vendored Blitz crates, whose own logging nobody wants turned
        // up to reach them.
        assert_eq!(TARGET, "ignition::profile");
    }

    /// `IGNITION_PROFILE` is what installs the spans, and without it
    /// nothing is constructed at all.
    ///
    /// A span below the filter's level is never built, so a profiler
    /// with nothing to say looks exactly like a fast frame. That is the
    /// failure this dial has to avoid on both sides: off means no cost,
    /// on means the spans actually exist.
    ///
    /// r[verify studio.profiling]
    #[test]
    fn the_profile_dial_turns_the_spans_on_and_off() {
        // Serialised against the other env-reading test in this module
        // by being the only one that touches it.
        let restore = std::env::var("IGNITION_PROFILE").ok();

        unsafe { std::env::remove_var("IGNITION_PROFILE") };
        assert!(
            filter_directives().is_empty(),
            "the profiler costs something when nobody asked for it"
        );

        for on in ["1", "stages", "true", "on"] {
            unsafe { std::env::set_var("IGNITION_PROFILE", on) };
            assert!(
                filter_directives().contains(&"ignition::profile=info"),
                "`IGNITION_PROFILE={on}` did not raise the stage spans into existence"
            );
        }

        unsafe { std::env::set_var("IGNITION_PROFILE", "all") };
        let all = filter_directives();
        assert!(all.contains(&"ignition::profile=info"));
        assert!(
            all.iter().any(|d| d.starts_with("bevy_")),
            "`all` does not reach Bevy's own spans, which is what it is for"
        );

        match restore {
            Some(value) => unsafe { std::env::set_var("IGNITION_PROFILE", value) },
            None => unsafe { std::env::remove_var("IGNITION_PROFILE") },
        }
    }
}
