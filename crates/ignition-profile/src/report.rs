//! The table.
//!
//! One row per span name, sorted by **self** time per frame. That sort
//! is the whole point of the thing: `blitz.render` is by construction
//! 100% of the frame every time and says nothing, while the row that
//! floats to the top of a self-time sort is, without further argument,
//! the code to go and look at.

use std::time::Duration;

use crate::{Row, STAGES, Window};

/// A window's worth of measurement, ready to print.
pub struct Report {
    /// Wall time the window covered.
    pub elapsed: Duration,
    /// Frames — closes of the frame span — in it.
    pub frames: u64,
    pub rows: Vec<Line>,
}

/// One span name's line.
pub struct Line {
    pub name: Box<str>,
    pub calls: u64,
    /// Total time inside the span, per frame.
    pub busy_per_frame_ms: f64,
    /// Of that, time not inside a nested measured span.
    pub self_per_frame_ms: f64,
    /// Per call, not per frame — a stage that runs twice a frame (two
    /// windows painting) has a per-call time half its per-frame one,
    /// and mixing them up is how a cheap stage looks expensive.
    pub avg_ms: f64,
    pub p99_ms: f64,
    pub max_ms: f64,
}

pub(crate) fn build(window: Window, elapsed: Duration) -> Report {
    let frames = window.frames.max(1);
    let mut rows: Vec<Line> = window
        .rows
        .into_iter()
        .map(|(name, row)| line(name, row, frames))
        .collect();
    // Self time descending; ties by the stage order, so a table of
    // near-zero rows still reads outermost-first rather than shuffling
    // between prints.
    rows.sort_by(|a, b| {
        b.self_per_frame_ms
            .total_cmp(&a.self_per_frame_ms)
            .then_with(|| rank(&a.name).cmp(&rank(&b.name)))
    });
    Report {
        elapsed,
        frames: window.frames,
        rows,
    }
}

fn rank(name: &str) -> usize {
    STAGES.iter().position(|s| *s == name).unwrap_or(usize::MAX)
}

fn line(name: Box<str>, mut row: Row, frames: u64) -> Line {
    row.samples.sort_by(f32::total_cmp);
    let pct = |p: f64| -> f64 {
        if row.samples.is_empty() {
            return 0.0;
        }
        let i = ((row.samples.len() - 1) as f64 * p).round() as usize;
        row.samples[i] as f64
    };
    let per_frame = |d: Duration| d.as_secs_f64() * 1e3 / frames as f64;
    Line {
        name,
        calls: row.calls,
        busy_per_frame_ms: per_frame(row.busy),
        self_per_frame_ms: per_frame(row.own),
        avg_ms: if row.calls == 0 {
            0.0
        } else {
            row.busy.as_secs_f64() * 1e3 / row.calls as f64
        },
        p99_ms: pct(0.99),
        max_ms: row.samples.last().copied().unwrap_or(0.0) as f64,
    }
}

/// The refresh the studio is aiming at. Everything in the table is read
/// against this: a stage over the whole budget on its own is the answer,
/// and there is rarely more than one.
const BUDGET_HZ: f64 = 120.0;

impl Report {
    /// The table, as one multi-line string for a single log line —
    /// deliberately one event rather than one per row, so a table never
    /// arrives interleaved with the studio's own logging.
    pub fn render(&self) -> String {
        let secs = self.elapsed.as_secs_f64().max(f64::EPSILON);
        let frame_ms = secs * 1e3 / self.frames.max(1) as f64;
        let budget_ms = 1e3 / BUDGET_HZ;
        let mut out = String::new();
        out.push_str(&format!(
            "\nprofile: {:.1} s · {} frames · {:.2} ms/frame · {:.1} fps (budget {:.2} ms at {:.0} Hz)\n",
            secs,
            self.frames,
            frame_ms,
            self.frames as f64 / secs,
            budget_ms,
            BUDGET_HZ,
        ));
        out.push_str(&format!(
            "  {:<28} {:>7} {:>10} {:>10} {:>8} {:>8} {:>8}\n",
            "stage", "calls", "self/frame", "all/frame", "avg", "p99", "max"
        ));
        for line in &self.rows {
            // A row that cannot round to a hundredth of a millisecond a
            // frame is noise; in `all` mode there are hundreds of them.
            if line.self_per_frame_ms < 0.005 && line.busy_per_frame_ms < 0.005 {
                continue;
            }
            out.push_str(&format!(
                "  {:<28} {:>7} {:>9.2}{} {:>9.2}  {:>7.2} {:>7.2} {:>7.2}\n",
                line.name,
                line.calls,
                line.self_per_frame_ms,
                // The one stage over budget on its own, flagged, so the
                // table answers the question without being read closely.
                if line.self_per_frame_ms > budget_ms {
                    "!"
                } else {
                    " "
                },
                line.busy_per_frame_ms,
                line.avg_ms,
                line.p99_ms,
                line.max_ms,
            ));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn row(calls: u64, busy_ms: f64, own_ms: f64) -> Row {
        Row {
            calls,
            busy: Duration::from_secs_f64(busy_ms / 1e3),
            own: Duration::from_secs_f64(own_ms / 1e3),
            samples: vec![(busy_ms / calls as f64) as f32; calls as usize],
        }
    }

    /// The sort is the feature: the outermost span always has the most
    /// total time and must never be what the table points at.
    #[test]
    fn self_time_sorts_above_total_time() {
        let mut rows = HashMap::new();
        rows.insert("blitz.render".into(), row(10, 200.0, 20.0));
        rows.insert("viz.render".into(), row(10, 180.0, 180.0));
        let report = build(
            Window {
                started: None,
                frames: 10,
                rows,
            },
            Duration::from_millis(200),
        );
        assert_eq!(&*report.rows[0].name, "viz.render");
        assert_eq!(&*report.rows[1].name, "blitz.render");
        // Ten frames, 180 ms of self time: 18 ms a frame.
        assert!((report.rows[0].self_per_frame_ms - 18.0).abs() < 1e-6);
    }

    /// Two windows painting means two calls a frame, and a per-call
    /// average that is *not* the per-frame cost.
    #[test]
    fn per_frame_and_per_call_differ_when_a_stage_runs_twice() {
        let mut rows = HashMap::new();
        rows.insert("blitz.scene".into(), row(20, 100.0, 100.0));
        let report = build(
            Window {
                started: None,
                frames: 10,
                rows,
            },
            Duration::from_millis(100),
        );
        assert!((report.rows[0].self_per_frame_ms - 10.0).abs() < 1e-6);
        assert!((report.rows[0].avg_ms - 5.0).abs() < 1e-6);
    }

    /// A stage over the whole 120 Hz budget on its own gets the marker.
    #[test]
    fn over_budget_is_flagged() {
        let mut rows = HashMap::new();
        rows.insert("viz.render".into(), row(1, 30.0, 30.0));
        let report = build(
            Window {
                started: None,
                frames: 1,
                rows,
            },
            Duration::from_millis(30),
        );
        assert!(report.render().contains("30.00!"));
    }
}
