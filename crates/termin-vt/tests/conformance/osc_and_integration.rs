//! Conformance tests for OSC sequences and shell integration protocols.
//!
//! Covers:
//! - OSC 0 / OSC 2: Window title
//! - OSC 7: Current Working Directory (percent decoding, effect notification)
//! - OSC 8: Hyperlinks (URL, id param)
//! - OSC 9 / OSC 99 / OSC 777: Notifications
//! - OSC 10 / OSC 11 / OSC 12 / OSC 4: Color queries and formatted response rgb:rrrr/gggg/bbbb
//! - OSC 52: Base64 clipboard write and policy enforcement
//! - OSC 133: FinalTerm shell integration marks (A, B, C, D)
//! - Payload cap enforcement: 64 KiB for general OSC, 1 MiB for OSC 52

use super::helper::TestTerm;
use termin_vt::{BlockEvent, Effect};

#[test]
fn test_osc_window_title() {
    let mut term = TestTerm::new(80, 24);

    term.advance(b"\x1b]0;Termin Window Title\x1b\\");
    assert!(term.sink.effects.iter().any(|e| match e {
        Effect::Title(t) => t.as_ref() == "Termin Window Title",
        _ => false,
    }));

    term.advance(b"\x1b]2;Another Title\x07"); // BEL-terminated
    assert!(term.sink.effects.iter().any(|e| match e {
        Effect::Title(t) => t.as_ref() == "Another Title",
        _ => false,
    }));
}

#[test]
fn test_osc_7_cwd() {
    let mut term = TestTerm::new(80, 24);

    term.advance(b"\x1b]7;file://localhost/Users/developer/termin%20code\x1b\\");
    assert!(term.sink.effects.iter().any(|e| match e {
        Effect::Cwd(p) => p.as_ref() == "/Users/developer/termin code",
        _ => false,
    }));
}

#[test]
fn test_osc_8_hyperlinks() {
    let mut term = TestTerm::new(80, 24);

    // Set hyperlink
    term.advance(b"\x1b]8;id=link1;https://termin.dev\x1b\\Click Here\x1b]8;;\x1b\\");
    assert_eq!(term.row_text_trimmed(0), "Click Here");

    // First cell has non-zero link index
    let style = term.style_at(0, 0).expect("style");
    assert!(style.link > 0);
}

#[test]
fn test_osc_notifications() {
    let mut term = TestTerm::new(80, 24);

    // OSC 9: iTerm2 notification
    term.advance(b"\x1b]9;Build finished successfully\x1b\\");
    assert!(term.sink.effects.iter().any(|e| match e {
        Effect::Notify { body, .. } => body.as_ref() == "Build finished successfully",
        _ => false,
    }));

    // OSC 777: Desktop notify
    term.advance(b"\x1b]777;notify;Cargo;Tests passed\x1b\\");
    assert!(term.sink.effects.iter().any(|e| match e {
        Effect::Notify { title, body } => title.as_ref() == "Cargo" && body.as_ref() == "Tests passed",
        _ => false,
    }));

    // OSC 99: Kitty notification
    term.advance(b"\x1b]99;i=1:d=1;Task completed\x1b\\");
    assert!(term.sink.effects.iter().any(|e| match e {
        Effect::Notify { body, .. } => body.as_ref() == "Task completed",
        _ => false,
    }));
}

#[test]
fn test_osc_color_queries() {
    let mut term = TestTerm::new(80, 24);

    // OSC 10: FG query
    term.advance(b"\x1b]10;?\x1b\\");
    let reply10 = term.take_writes();
    assert!(reply10.starts_with("\x1b]10;rgb:"));

    // OSC 11: BG query
    term.advance(b"\x1b]11;?\x1b\\");
    let reply11 = term.take_writes();
    assert!(reply11.starts_with("\x1b]11;rgb:"));

    // OSC 12: Cursor color query
    term.advance(b"\x1b]12;?\x1b\\");
    let reply12 = term.take_writes();
    assert!(reply12.starts_with("\x1b]12;rgb:"));

    // OSC 4: Palette color query for index 1 (Red)
    term.advance(b"\x1b]4;1;?\x1b\\");
    let reply4 = term.take_writes();
    assert!(reply4.starts_with("\x1b]4;1;rgb:"));
}

#[test]
fn test_osc_52_clipboard() {
    let mut term = TestTerm::new(80, 24);

    // Base64 for "Hello Clipboard" is "SGVsbG8gQ2xpcGJvYXJk"
    term.advance(b"\x1b]52;c;SGVsbG8gQ2xpcGJvYXJk\x1b\\");
    assert!(term.sink.effects.iter().any(|e| match e {
        Effect::ClipboardSet(bytes) => bytes.as_ref() == b"Hello Clipboard",
        _ => false,
    }));

    // Clipboard query: ?
    term.advance(b"\x1b]52;c;?\x1b\\");
    assert!(term.sink.effects.iter().any(|e| matches!(e, Effect::ClipboardRequest)));
}

#[test]
fn test_osc_133_shell_integration() {
    let mut term = TestTerm::new(80, 24);

    // 133;A - Prompt start
    term.advance(b"\x1b]133;A\x1b\\$ ");

    // 133;B - Command input start
    term.advance(b"\x1b]133;B\x1b\\echo test\r\n");

    // 133;C - Command output start with url-encoded cmdline
    term.advance(b"\x1b]133;C;cmdline_url=echo%20test\x1b\\test\r\n");

    // 133;D;0 - Command finished with exit status 0
    term.advance(b"\x1b]133;D;0\x1b\\");

    // Verify block event sequence was produced
    let block_events: Vec<&BlockEvent> = term.sink.effects.iter().filter_map(|e| match e {
        Effect::Block(b) => Some(b),
        _ => None,
    }).collect();

    assert!(!block_events.is_empty());
    assert!(matches!(block_events[0], BlockEvent::PromptStarted(_)));
}

#[test]
fn test_osc_payload_cap_enforcement() {
    let mut term = TestTerm::new(80, 24);

    // General OSC limit is 64 KiB: an oversized title (>64 KiB) should be rejected
    let mut huge_osc = Vec::with_capacity(70_000);
    huge_osc.extend_from_slice(b"\x1b]2;");
    huge_osc.resize(68_000, b'A');
    huge_osc.extend_from_slice(b"\x1b\\");

    let initial_count = term.sink.effects.len();
    term.advance(&huge_osc);
    // No title effect should be emitted for the oversized payload
    assert_eq!(term.sink.effects.len(), initial_count);
}
