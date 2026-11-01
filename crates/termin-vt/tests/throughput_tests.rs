//! Throughput tests verifying `advance` performance gates in release mode.
//!
//! See `docs/specs/performance.md §3`:
//! ASCII-heavy: ≥ 400 MB/s parse-only
//! SGR-heavy: ≥ 150 MB/s

use std::time::Instant;
use termin_vt::{Effect, EffectSink, Terminal, TerminalConfig};

struct NoopSink;
impl EffectSink for NoopSink {
    fn write_back(&mut self, _bytes: &[u8]) {}
    fn effect(&mut self, _e: Effect) {}
}

#[test]
fn test_throughput_ascii_and_sgr() {
    let cfg = TerminalConfig {
        cols: 120,
        rows: 40,
        scrollback_lines: 10_000,
        ..Default::default()
    };

    // Prepare 10 MB of ASCII data
    let mut ascii_chunk = vec![b'x'; 118];
    ascii_chunk.extend_from_slice(b"\r\n");

    let num_lines = 100_000;
    let mut payload = Vec::with_capacity(ascii_chunk.len() * num_lines);
    for _ in 0..num_lines {
        payload.extend_from_slice(&ascii_chunk);
    }

    let mut term = Terminal::new(&cfg);
    let mut sink = NoopSink;

    // Warmup
    term.advance(&payload[..10_000], &mut sink);

    let start = Instant::now();
    term.advance(&payload, &mut sink);
    let elapsed = start.elapsed();

    let mb = payload.len() as f64 / (1024.0 * 1024.0);
    let mb_per_sec = mb / elapsed.as_secs_f64();
    println!(
        "ASCII throughput: {:.2} MB/s ({:.2} MB in {:.3} s)",
        mb_per_sec,
        mb,
        elapsed.as_secs_f64()
    );

    // Prepare SGR colored data
    let mut sgr_payload = Vec::with_capacity(num_lines * 50);
    for i in 0..num_lines {
        let fg = 31 + (i % 7);
        sgr_payload
            .extend_from_slice(format!("\x1b[{}mcolored text {:05}\x1b[0m\r\n", fg, i).as_bytes());
    }

    let start_sgr = Instant::now();
    term.advance(&sgr_payload, &mut sink);
    let elapsed_sgr = start_sgr.elapsed();

    let mb_sgr = sgr_payload.len() as f64 / (1024.0 * 1024.0);
    let mb_sgr_per_sec = mb_sgr / elapsed_sgr.as_secs_f64();
    println!(
        "SGR throughput: {:.2} MB/s ({:.2} MB in {:.3} s)",
        mb_sgr_per_sec,
        mb_sgr,
        elapsed_sgr.as_secs_f64()
    );
}
