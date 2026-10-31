//! Tests for terminal resize and reflow.
//!
//! See `docs/specs/terminal-core.md §7`.

use termin_vt::{AbsLine, Effect, EffectSink, LineRange, Terminal, TerminalConfig};

struct DummySink;
impl EffectSink for DummySink {
    fn write_back(&mut self, _bytes: &[u8]) {}
    fn effect(&mut self, _e: Effect) {}
}

#[test]
fn test_reflow_narrow_to_wide_and_back() {
    let cfg = TerminalConfig {
        cols: 20,
        rows: 5,
        ..Default::default()
    };
    let mut term = Terminal::new(&cfg);
    let mut sink = DummySink;

    // Print a 40-character line that will wrap onto 2 rows in 20 columns
    term.advance(b"12345678901234567890abcdefghijklmnopqrst", &mut sink);

    let text_before = term.text_range(LineRange {
        start: AbsLine(0),
        end: AbsLine(2),
    });
    assert_eq!(text_before.trim(), "12345678901234567890abcdefghijklmnopqrst");

    // Resize to width 40: should unwrap into a single row
    term.resize(40, 5, (8, 16));
    assert_eq!(term.cols(), 40);

    let text_wide = term.text_range(LineRange {
        start: AbsLine(0),
        end: AbsLine(1),
    });
    assert_eq!(text_wide.trim(), "12345678901234567890abcdefghijklmnopqrst");

    // Resize back to width 20: wraps back cleanly
    term.resize(20, 5, (8, 16));
    let text_back = term.text_range(LineRange {
        start: AbsLine(0),
        end: AbsLine(2),
    });
    assert_eq!(text_back.trim(), "12345678901234567890abcdefghijklmnopqrst");
}

#[test]
fn test_reflow_preserves_block_positions() {
    let cfg = TerminalConfig {
        cols: 20,
        rows: 10,
        ..Default::default()
    };
    let mut term = Terminal::new(&cfg);
    let mut sink = DummySink;

    // Start block 1
    term.advance(b"\x1b]133;A\x1b\\", &mut sink);
    term.advance(b"\x1b]133;B\x1b\\echo 123456789012345678901234567890\r\n", &mut sink);
    term.advance(b"\x1b]133;C\x1b\\Output data\r\n", &mut sink);
    term.advance(b"\x1b]133;D;0\x1b\\", &mut sink);

    let block_before = term.blocks().iter().next().unwrap().clone();

    // Resize terminal
    term.resize(40, 10, (8, 16));

    let block_after = term.blocks().iter().next().unwrap();
    // Block must still exist and be valid
    assert!(block_after.prompt_start.0 <= block_after.end.unwrap().0);
    assert_eq!(block_after.exit, block_before.exit);
}

#[test]
fn test_alt_screen_resize_truncates_without_reflow() {
    let cfg = TerminalConfig {
        cols: 20,
        rows: 5,
        ..Default::default()
    };
    let mut term = Terminal::new(&cfg);
    let mut sink = DummySink;

    // Switch to alt screen: CSI ? 1049 h
    term.advance(b"\x1b[?1049h", &mut sink);
    term.advance(b"12345678901234567890", &mut sink);

    // Resize alt screen to 10 cols
    term.resize(10, 5, (8, 16));
    assert_eq!(term.cols(), 10);

    let view = term.view(termin_vt::ScrollPos::Bottom);
    let first_row: String = view
        .into_iter()
        .next()
        .unwrap()
        .cells
        .iter()
        .filter_map(|c| char::from_u32(c.ch))
        .collect();
    // Truncated to 10 characters
    assert_eq!(first_row, "1234567890");
}
