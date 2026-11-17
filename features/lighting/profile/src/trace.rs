//! A Chrome trace-event file, for when the table is not enough.
//!
//! The table says a stage costs 12 ms a frame. It cannot say that the
//! cost is one frame in ten costing 120 ms, or that two windows are
//! painting in lockstep and serialising, or where in the frame the
//! stall sits. A timeline can, so this writes one.
//!
//! The format is Chrome's "JSON Array Format", which
//! <https://ui.perfetto.dev> reads directly — and which is explicitly
//! allowed to be truncated: an unterminated array is a valid trace. So
//! the file is opened with a `[`, events are appended, and there is no
//! close to get right. Kill the studio however you like and the trace
//! still opens.
//!
//! Written by hand rather than with `tracing-chrome` because it is
//! thirty lines and one fewer dependency in a tree that already pins
//! wgpu across two renderers.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::sync::Mutex;
use std::time::Duration;

pub struct TraceWriter {
    file: Mutex<BufWriter<File>>,
}

impl TraceWriter {
    pub fn create(path: &str) -> std::io::Result<Self> {
        let mut file = BufWriter::new(File::create(path)?);
        file.write_all(b"[\n")?;
        Ok(Self {
            file: Mutex::new(file),
        })
    }

    /// One complete event: a span that ran for `busy`, starting `at`
    /// after the profiler's origin.
    ///
    /// The thread id is the OS thread's, so Bevy's render thread and
    /// the winit thread land in their own tracks — which is how you see
    /// that they are not overlapping.
    pub fn push(&self, name: &str, at: Duration, busy: Duration) {
        let tid = thread_id();
        // A poisoned lock is a thread that panicked while writing a
        // trace line. The trace is a debugging aid; refusing to write
        // any more of it would turn a panic somewhere else into a
        // second failure here.
        let mut file = self
            .file
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = writeln!(
            file,
            r#"{{"name":"{}","cat":"ignition","ph":"X","ts":{:.3},"dur":{:.3},"pid":1,"tid":{}}},"#,
            name,
            at.as_secs_f64() * 1e6,
            busy.as_secs_f64() * 1e6,
            tid,
        );
        drop(file);
    }

    pub fn flush(&self) {
        let mut file = self
            .file
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = file.flush();
    }
}

/// A small stable number per thread. `ThreadId::as_u64` is still
/// unstable, and Perfetto only wants the tracks to be distinct.
fn thread_id() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    thread_local! {
        static ID: u64 = NEXT.fetch_add(1, Ordering::Relaxed);
    }
    ID.with(|id| *id)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A truncated file is a valid trace — the array is never closed,
    /// so what matters is that every line before the cut parses.
    #[test]
    fn writes_parseable_events() {
        let dir = std::env::temp_dir().join(format!("ig-trace-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("t.json");
        let writer = TraceWriter::create(path.to_str().expect("utf-8 path")).expect("create");
        writer.push(
            "viz.render",
            Duration::from_millis(1),
            Duration::from_micros(8500),
        );
        writer.flush();
        let text = std::fs::read_to_string(&path).expect("read back");
        assert!(text.starts_with("[\n"));
        assert!(text.contains(r#""name":"viz.render""#));
        assert!(text.contains(r#""ph":"X""#));
        assert!(text.contains(r#""ts":1000.000"#));
        assert!(text.contains(r#""dur":8500.000"#));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
