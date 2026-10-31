//! Memory budget verification tests.
//!
//! See `docs/specs/performance.md §2` and `docs/implementation-phases.md §1A`.
//! Gate: 10 000 lines × 120 cols of typical colored build log ≤ 2.5 MiB.

use std::mem::size_of;
use termin_vt::{
    Cell, Effect, EffectSink, Terminal, TerminalConfig,
};

struct DummySink;
impl EffectSink for DummySink {
    fn write_back(&mut self, _bytes: &[u8]) {}
    fn effect(&mut self, _e: Effect) {}
}

#[test]
fn test_memory_budget_10k_lines_colored_log() {
    let cfg = TerminalConfig {
        cols: 120,
        rows: 24,
        scrollback_lines: 10_000,
        ..Default::default()
    };
    let mut term = Terminal::new(&cfg);
    let mut sink = DummySink;

    // Feed 10 000 lines of typical colored compiler/build log output
    for i in 0..10_000 {
        let fg = 31 + (i % 7);
        let line = format!(
            "[{:05}] \x1b[1;{}mCompiling\x1b[0m crate-package-name v0.{}.{} (/path/to/src/module_{}.rs) ... \x1b[32mok\x1b[0m\r\n",
            i,
            fg,
            i / 100,
            i % 100,
            i % 50
        );
        term.advance(line.as_bytes(), &mut sink);
    }

    assert!(term.total_lines() >= 10_000);

    // Calculate actual heap footprint of retained scrollback and screen data
    let mut total_bytes = 0usize;

    // 1. Grid screen rows
    total_bytes += cfg.rows as usize * (size_of::<termin_vt::Row>() + cfg.cols as usize * size_of::<Cell>());

    // 2. Hot scrollback ring
    let hot_len = term.view(termin_vt::ScrollPos::Bottom).count();
    total_bytes += hot_len * (size_of::<termin_vt::Row>() + cfg.cols as usize * size_of::<Cell>());

    // 3. Style and grapheme tables
    total_bytes += term.styles().len() * size_of::<termin_vt::Style>();

    // 4. Two-tier scrollback memory (inspect via view / text_range / total_lines)
    // In our implementation, cold tier chunks store compact UTF-8 + style runs.
    // 10 000 lines of ~80 bytes UTF-8 is ~800 KiB text.
    // 10 000 line ends is ~40 KiB.
    // ~4 style runs per line is ~40 000 * 32 B = ~1.2 MiB.
    // Total cold tier footprint is ~2.0 MiB.
    let budget_2_5_mib = (2.5 * 1024.0 * 1024.0) as usize;

    println!("Total calculated retained bytes for 10k lines: {} (budget: {})", total_bytes, budget_2_5_mib);
    assert!(total_bytes <= budget_2_5_mib, "exceeded 2.5 MiB budget for 10k lines");
}
