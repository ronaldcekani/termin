//! Opt-in phase timing for diagnosing live-resize latency.
//!
//! Enabled by `TERMIN_RESIZE_TRACE=1`. When disabled every call is a single
//! cached bool check. Timings accumulate per thread (all phases run on the main
//! thread); the platform layer prints one line per resize tick and a summary
//! when live resize ends, to stderr.

use std::cell::RefCell;
use std::io::Write;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

/// A timed phase of a resize tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Whole `setFrameSize:` handler.
    Tick,
    /// `CAMetalLayer.setDrawableSize`.
    DrawableResize,
    /// Dispatching `PlatformEvent::Resized` to the app.
    ResizeEvent,
    /// Whole app frame (CPU build + render submission).
    Frame,
    /// PTY winsize + terminal reflow.
    TermResize,
    /// Blocking in `CAMetalLayer.nextDrawable`.
    NextDrawable,
    /// Waiting for a free in-flight frame slot (GPU still busy).
    SlotWait,
    /// Uploads + command encoding.
    Encode,
    /// `commit` (+ `waitUntilScheduled` when presenting with transaction).
    Submit,
    /// `present` / `presentDrawable`.
    Present,
}

const N: usize = 10;

const PHASES: [Phase; N] = [
    Phase::Tick,
    Phase::DrawableResize,
    Phase::ResizeEvent,
    Phase::Frame,
    Phase::TermResize,
    Phase::NextDrawable,
    Phase::SlotWait,
    Phase::Encode,
    Phase::Submit,
    Phase::Present,
];

impl Phase {
    fn name(self) -> &'static str {
        match self {
            Phase::Tick => "tick",
            Phase::DrawableResize => "drawable",
            Phase::ResizeEvent => "event",
            Phase::Frame => "frame",
            Phase::TermResize => "term",
            Phase::NextDrawable => "next_drawable",
            Phase::SlotWait => "slot_wait",
            Phase::Encode => "encode",
            Phase::Submit => "submit",
            Phase::Present => "present",
        }
    }
}

#[derive(Default)]
struct Acc {
    tick: [Duration; N],
    total: [Duration; N],
    max: [Duration; N],
    ticks: u32,
}

thread_local! {
    static ACC: RefCell<Acc> = RefCell::new(Acc::default());
}

/// Whether tracing is enabled (`TERMIN_RESIZE_TRACE` set and not `0`).
#[inline]
pub fn enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| {
        std::env::var_os("TERMIN_RESIZE_TRACE").is_some_and(|v| !v.is_empty() && v != "0")
    })
}

/// Start timing `phase`; the elapsed time is recorded when the guard drops.
#[inline]
pub fn span(phase: Phase) -> Span {
    Span {
        phase,
        start: enabled().then(Instant::now),
    }
}

/// Guard returned by [`span`].
pub struct Span {
    phase: Phase,
    start: Option<Instant>,
}

impl Drop for Span {
    fn drop(&mut self) {
        if let Some(start) = self.start {
            let d = start.elapsed();
            let i = self.phase as usize;
            ACC.with(|a| a.borrow_mut().tick[i] += d);
        }
    }
}

/// Finish a resize tick: print its phases and fold them into the summary.
pub fn end_tick(width: f32, height: f32) {
    if !enabled() {
        return;
    }
    ACC.with(|a| {
        let mut a = a.borrow_mut();
        a.ticks += 1;
        let mut line = format!("[resize] {width:.0}x{height:.0}");
        for p in PHASES {
            let i = p as usize;
            let d = a.tick[i];
            a.total[i] += d;
            a.max[i] = a.max[i].max(d);
            line.push_str(&format!(" {}={:.2}ms", p.name(), ms(d)));
        }
        a.tick = [Duration::ZERO; N];
        let _ = writeln!(std::io::stderr(), "{line}");
    });
}

/// Print and reset the summary (call when live resize ends).
pub fn end_session() {
    if !enabled() {
        return;
    }
    ACC.with(|a| {
        let mut a = a.borrow_mut();
        if a.ticks == 0 {
            return;
        }
        let n = a.ticks as f64;
        let mut out = format!("[resize] summary over {} ticks (avg / max):\n", a.ticks);
        for p in PHASES {
            let i = p as usize;
            out.push_str(&format!(
                "  {:<14} {:>7.2} / {:>7.2} ms\n",
                p.name(),
                ms(a.total[i]) / n,
                ms(a.max[i])
            ));
        }
        *a = Acc::default();
        let _ = std::io::stderr().write_all(out.as_bytes());
    });
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_span_records_nothing() {
        // The env var is not set in tests.
        if enabled() {
            return;
        }
        {
            let _s = span(Phase::Frame);
        }
        ACC.with(|a| assert_eq!(a.borrow().tick[Phase::Frame as usize], Duration::ZERO));
        end_tick(1.0, 1.0);
        end_session();
    }
}
