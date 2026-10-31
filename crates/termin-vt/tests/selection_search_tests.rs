//! Tests for text selection and incremental search.
//!
//! See `docs/specs/terminal-core.md §8` and `§9`.

use termin_vt::{
    AbsLine, AbsPos, Effect, EffectSink, LineRange, SearchProgress, SearchQuery, Terminal,
    TerminalConfig,
};

struct DummySink;
impl EffectSink for DummySink {
    fn write_back(&mut self, _bytes: &[u8]) {}
    fn effect(&mut self, _e: Effect) {}
}

#[test]
fn test_selection_soft_wrap_joining() {
    let cfg = TerminalConfig {
        cols: 10,
        rows: 5,
        ..Default::default()
    };
    let mut term = Terminal::new(&cfg);
    let mut sink = DummySink;

    // Line 1: soft-wraps across 2 rows (20 chars)
    term.advance(b"1234567890abcdefghij\r\n", &mut sink);
    // Line 2: normal row
    term.advance(b"hello     \r\n", &mut sink);

    let text = term.text_range(LineRange {
        start: AbsLine(0),
        end: AbsLine(3),
    });

    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 2);
    // Soft wrapped rows joined without newline
    assert_eq!(lines[0], "1234567890abcdefghij");
    // Trailing spaces trimmed
    assert_eq!(lines[1], "hello");
}

#[test]
fn test_block_command_and_output_text() {
    let cfg = TerminalConfig::default();
    let mut term = Terminal::new(&cfg);
    let mut sink = DummySink;

    // Prompt A
    term.advance(b"\x1b]133;A\x1b\\$ ", &mut sink);
    // Input B
    term.advance(b"\x1b]133;B\x1bgit status\r\n", &mut sink);
    // Output C
    term.advance(b"\x1b]133;C;cmdline_url=git%20status\x1b\\", &mut sink);
    term.advance(b"On branch main\r\nnothing to commit\r\n", &mut sink);
    // Finished D
    term.advance(b"\x1b]133;D;0\x1b\\", &mut sink);

    let block_id = term.blocks().iter().next().unwrap().id;

    let cmd = term.block_command_text(block_id).unwrap();
    assert_eq!(cmd, "git status");

    let output = term.block_output_text(block_id).unwrap();
    assert!(output.contains("On branch main"));
    assert!(output.contains("nothing to commit"));
}

#[test]
fn test_search_hot_and_cold_scrollback() {
    let cfg = TerminalConfig {
        cols: 80,
        rows: 5,
        scrollback_lines: 1000,
        ..Default::default()
    };
    let mut term = Terminal::new(&cfg);
    let mut sink = DummySink;

    // Fill screen and scroll lines into scrollback
    term.advance(b"First line containing needle_alpha\r\n", &mut sink);
    for i in 0..20 {
        term.advance(format!("Filler line {}\r\n", i).as_bytes(), &mut sink);
    }
    term.advance(b"Latest line containing needle_beta\r\n", &mut sink);

    // Search for needle_beta (visible screen)
    let q1 = SearchQuery {
        text: "needle_beta".into(),
        case_sensitive: true,
        whole_word: false,
    };
    let m1 = term.search(&q1, AbsPos { line: AbsLine(term.total_lines()), col: 80 }).unwrap();
    assert!(m1.start.col > 0);

    // Search for needle_alpha (scrolled back in hot scrollback ring)
    let q2 = SearchQuery {
        text: "needle_alpha".into(),
        case_sensitive: true,
        whole_word: false,
    };
    let m2 = term.search(&q2, AbsPos { line: AbsLine(term.total_lines()), col: 80 }).unwrap();
    assert_eq!(m2.start.line, AbsLine(0));

    // Incremental search stepping
    let step_res = term.search_step(&q1, None, 5);
    match step_res {
        SearchProgress::Found(m) => {
            assert_eq!(m.start.line, m1.start.line);
        }
        _ => panic!("expected match in first 5 lines"),
    }
}
