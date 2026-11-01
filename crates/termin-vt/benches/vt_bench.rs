//! VT parser throughput benchmarks (`cargo bench -p termin-vt`).
//!
//! Benchmarks `Terminal::advance` across `ascii`, `sgr`, `unicode`, `scroll`, and
//! `claude-code` streams (`docs/specs/performance.md §3, §4`).

use std::hint::black_box;
use std::time::Instant;
use termin_vt::{Effect, EffectSink, Terminal, TerminalConfig};

struct NullSink;
impl EffectSink for NullSink {
    #[inline(always)]
    fn write_back(&mut self, _bytes: &[u8]) {}
    #[inline(always)]
    fn effect(&mut self, _e: Effect) {}
}

fn bench_stream(name: &str, payload: &[u8], iterations: usize) -> f64 {
    let cfg = TerminalConfig {
        cols: 120,
        rows: 40,
        scrollback_lines: 10_000,
        ..Default::default()
    };
    let mut best_mb_s = 0.0f64;
    for _ in 0..iterations {
        let mut term = Terminal::new(&cfg);
        let mut sink = NullSink;
        let start = Instant::now();
        term.advance(black_box(payload), &mut sink);
        black_box(term.generation());
        let secs = start.elapsed().as_secs_f64().max(1e-9);
        let mb_s = (payload.len() as f64 / (1024.0 * 1024.0)) / secs;
        if mb_s > best_mb_s {
            best_mb_s = mb_s;
        }
    }
    println!("bench vt/{:<16}: {:>8.1} MB/s ({} bytes)", name, best_mb_s, payload.len());
    best_mb_s
}

fn main() {
    let ascii = "Compiling termin-vt v0.1.0 (/Users/termin/crates/termin-vt) [release optimized]\r\n"
        .repeat(30_000);
    let sgr = "\x1b[1;32mCompiling\x1b[0m \x1b[38;5;208mtermin-vt\x1b[0m \x1b[38;2;80;160;255mv0.1.0\x1b[0m\r\n"
        .repeat(30_000);
    let unicode = "Row: ┌─────────┐ 【日本語・中文・한국어】 🚀 ✨ └─────────┘\r\n".repeat(20_000);
    let scroll = "scroll stream row 000123 | memory buffer test offset 0x00001000\r\n".repeat(30_000);
    let claude = "\x1b]133;A\x1b\\$ \x1b]133;B\x1b\\cargo test\r\n\x1b]133;C\x1b\\\x1b[?2026h\x1b[32mPASS\x1b[0m\r\n\x1b[?2026l\x1b]133;D;0\x1b\\\r\n"
        .repeat(20_000);

    bench_stream("ascii", ascii.as_bytes(), 5);
    bench_stream("sgr", sgr.as_bytes(), 5);
    bench_stream("unicode", unicode.as_bytes(), 5);
    bench_stream("scroll", scroll.as_bytes(), 5);
    bench_stream("claude-code", claude.as_bytes(), 5);
}
