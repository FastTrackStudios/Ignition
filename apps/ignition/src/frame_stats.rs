//! Where a pane's frame went, phase by phase, and the summary of a few
//! hundred of them.
//!
//! A frame's phases are fields of its span (`pane.frame`, target
//! `ignition::profile`, as the visualizer's own stages are); this keeps
//! the same numbers for a window of frames and says their p50 and p99
//! in one event (`ignition::frames`), with the machine's load average
//! beside them: a frame time read without it is not a measurement on a
//! machine that builds as well.
//! `docs/lighting/research/frame-breakdown.md` is what was read from it.

use std::time::Instant;

/// How many frames a summary covers unless `IGNITION_FRAME_STATS` says
/// (0 for none).
const WINDOW: usize = 300;

/// The microseconds since `since`, saturating.
#[must_use]
pub fn micros_since(since: Instant) -> u32 {
    u32::try_from(since.elapsed().as_micros()).unwrap_or(u32::MAX)
}

/// One frame's phases, in microseconds, in the order they happen.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FrameTimes {
    /// The show followed and the visualizer stepped: the main world's
    /// update and the extraction, which waits for the render thread to
    /// have finished encoding the frame before.
    pub update: u32,
    /// `IGNITION_FRAME_DRAIN=1` only: the wait for the GPU to finish
    /// what was already submitted, before the frame is copied out. It
    /// takes the rendering out of `wait`, leaving the copy there.
    pub drain: u32,
    /// The copy out of the target encoded and submitted.
    pub submit: u32,
    /// The wait for the GPU: the rendering still running, then the copy
    /// and (shared memory) the readback's mapping.
    pub wait: u32,
    /// Shared memory: the readback copied into the slot.
    pub memcpy: u32,
    /// The frame's message sent to the app.
    pub send: u32,
    /// The whole frame, as the wire's `render_us`.
    pub total: u32,
    /// From the frame before's start to this one's: `total` plus the
    /// wait for the app to take the frame and release a slot.
    pub interval: u32,
}

/// A phase's name and its reading of a frame, in the order summarised.
type Phase = (&'static str, fn(&FrameTimes) -> u32);

const PHASES: [Phase; 8] = [
    ("update", |t| t.update),
    ("drain", |t| t.drain),
    ("submit", |t| t.submit),
    ("wait", |t| t.wait),
    ("memcpy", |t| t.memcpy),
    ("send", |t| t.send),
    ("total", |t| t.total),
    ("interval", |t| t.interval),
];

/// A phase over a window of frames.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Spread {
    pub p50: u32,
    pub p99: u32,
    pub max: u32,
}

/// The p50, p99 and largest of `samples` (nearest rank; zeros for none).
#[must_use]
pub fn spread(samples: &mut [u32]) -> Spread {
    samples.sort_unstable();
    let last = samples.len().saturating_sub(1);
    // Nearest rank over the sorted samples, in integers.
    let at = |percent: usize| {
        let index = last
            .saturating_mul(percent)
            .saturating_add(50)
            .checked_div(100)
            .unwrap_or(0);
        samples.get(index).copied().unwrap_or(0)
    };
    Spread {
        p50: at(50),
        p99: at(99),
        max: samples.last().copied().unwrap_or(0),
    }
}

/// The frames of a window, and when the one before began.
#[derive(Debug)]
pub struct FrameStats {
    window: usize,
    frames: Vec<FrameTimes>,
    began: Option<Instant>,
}

impl FrameStats {
    /// A window of `IGNITION_FRAME_STATS` frames (300 unless it says; 0
    /// summarises nothing).
    #[must_use]
    pub fn from_env() -> Self {
        let window = std::env::var("IGNITION_FRAME_STATS")
            .ok()
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(WINDOW);
        Self::new(window)
    }

    #[must_use]
    pub fn new(window: usize) -> Self {
        Self {
            window,
            frames: Vec::with_capacity(window),
            began: None,
        }
    }

    /// A frame begins at `now`: the microseconds since the one before
    /// began (0 for the first).
    pub fn begin(&mut self, now: Instant) -> u32 {
        let interval = self.began.map_or(0, |before| {
            u32::try_from(now.saturating_duration_since(before).as_micros()).unwrap_or(u32::MAX)
        });
        self.began = Some(now);
        interval
    }

    /// Keeps a frame; the window's summary once it is full.
    pub fn push(&mut self, times: FrameTimes) -> Option<Summary> {
        if self.window == 0 {
            return None;
        }
        self.frames.push(times);
        (self.frames.len() >= self.window).then(|| self.take())
    }

    /// The summary of the frames kept so far, which starts a new window.
    pub fn take(&mut self) -> Summary {
        let frames = std::mem::take(&mut self.frames);
        let phases = PHASES.map(|(name, read)| {
            let mut samples: Vec<u32> = frames.iter().map(read).collect();
            (name, spread(&mut samples))
        });
        Summary {
            frames: frames.len(),
            phases,
        }
    }
}

/// A window of frames, each phase's spread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    pub frames: usize,
    pub phases: [(&'static str, Spread); 8],
}

impl Summary {
    fn of(&self, phase: &str) -> Spread {
        self.phases
            .iter()
            .find(|(name, _)| *name == phase)
            .map(|(_, spread)| *spread)
            .unwrap_or_default()
    }

    /// The window as one event: every phase's p50 and p99 in
    /// microseconds, what the frames were (`transport`, size) and the
    /// load average they were drawn under.
    pub fn emit(&self, transport: &str, (width, height): (u32, u32)) {
        let (update, drain, submit, wait) = (
            self.of("update"),
            self.of("drain"),
            self.of("submit"),
            self.of("wait"),
        );
        let (memcpy, send, total, interval) = (
            self.of("memcpy"),
            self.of("send"),
            self.of("total"),
            self.of("interval"),
        );
        tracing::info!(
            target: "ignition::frames",
            frames = self.frames,
            transport,
            width,
            height,
            load1 = load_average().as_deref().unwrap_or("?"),
            update_p50_us = update.p50,
            update_p99_us = update.p99,
            drain_p50_us = drain.p50,
            drain_p99_us = drain.p99,
            submit_p50_us = submit.p50,
            submit_p99_us = submit.p99,
            wait_p50_us = wait.p50,
            wait_p99_us = wait.p99,
            memcpy_p50_us = memcpy.p50,
            memcpy_p99_us = memcpy.p99,
            send_p50_us = send.p50,
            send_p99_us = send.p99,
            total_p50_us = total.p50,
            total_p99_us = total.p99,
            total_max_us = total.max,
            interval_p50_us = interval.p50,
            interval_p99_us = interval.p99,
            "frames"
        );
    }
}

/// The one-minute load average (Linux's `/proc/loadavg`), as it is
/// written there.
fn load_average() -> Option<String> {
    let text = std::fs::read_to_string("/proc/loadavg").ok()?;
    text.split_whitespace().next().map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_spread_of_one_to_a_hundred() {
        let mut samples: Vec<u32> = (1..=100).rev().collect();
        let spread = spread(&mut samples);
        assert_eq!(
            spread,
            Spread {
                p50: 51,
                p99: 99,
                max: 100
            }
        );
    }

    #[test]
    fn no_samples_spread_to_zero() {
        assert_eq!(spread(&mut []), Spread::default());
    }

    #[test]
    fn a_full_window_is_summarised_and_starts_again() {
        let mut stats = FrameStats::new(3);
        let frame = |total| FrameTimes {
            total,
            wait: total / 2,
            ..FrameTimes::default()
        };
        assert_eq!(stats.push(frame(10)), None);
        assert_eq!(stats.push(frame(30)), None);
        let summary = stats.push(frame(20)).expect("the window filled");
        assert_eq!(summary.frames, 3);
        assert_eq!(summary.of("total").p50, 20);
        assert_eq!(summary.of("total").max, 30);
        assert_eq!(summary.of("wait").p50, 10);
        assert_eq!(stats.push(frame(1)), None, "a new window");
    }

    #[test]
    fn a_window_of_none_summarises_nothing() {
        let mut stats = FrameStats::new(0);
        assert_eq!(stats.push(FrameTimes::default()), None);
    }

    #[test]
    fn the_interval_is_between_two_frames_starts() {
        let mut stats = FrameStats::new(3);
        let first = Instant::now();
        assert_eq!(stats.begin(first), 0);
        let second = first + std::time::Duration::from_micros(1500);
        assert_eq!(stats.begin(second), 1500);
    }
}
