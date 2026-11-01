//! Comprehensive unit tests for CSI, ESC, and OSC escape sequences.
//!
//! See `docs/specs/terminal-core.md §4`, `§5`, and `§12`.

use termin_vt::{Attrs, Color, CursorShape, Effect, EffectSink, Modes, Terminal, TerminalConfig};

#[derive(Default)]
struct TestSink {
    writes: Vec<u8>,
    effects: Vec<Effect>,
}

impl EffectSink for TestSink {
    fn write_back(&mut self, bytes: &[u8]) {
        self.writes.extend_from_slice(bytes);
    }
    fn effect(&mut self, e: Effect) {
        self.effects.push(e);
    }
}

#[test]
fn test_csi_cursor_movements() {
    let cfg = TerminalConfig {
        cols: 80,
        rows: 24,
        ..Default::default()
    };
    let mut term = Terminal::new(&cfg);
    let mut sink = TestSink::default();

    // Position at 10, 15 (1-based -> 9, 14 0-based)
    term.advance(b"\x1b[10;15H", &mut sink);
    assert_eq!(term.cursor().pos.col, 14);

    // Up 3
    term.advance(b"\x1b[3A", &mut sink);
    // Down 2
    term.advance(b"\x1b[2B", &mut sink);
    // Forward 5
    term.advance(b"\x1b[5C", &mut sink);
    assert_eq!(term.cursor().pos.col, 19);

    // Backward 4
    term.advance(b"\x1b[4D", &mut sink);
    assert_eq!(term.cursor().pos.col, 15);

    // Horizontal absolute: col 30 (1-based -> 29)
    term.advance(b"\x1b[30G", &mut sink);
    assert_eq!(term.cursor().pos.col, 29);

    // Vertical absolute: row 5 (1-based)
    term.advance(b"\x1b[5d", &mut sink);
}

#[test]
fn test_csi_erase_operations() {
    let cfg = TerminalConfig {
        cols: 20,
        rows: 5,
        ..Default::default()
    };
    let mut term = Terminal::new(&cfg);
    let mut sink = TestSink::default();

    // Fill 5 rows without scrolling off the top
    for i in 0..5 {
        if i > 0 {
            term.advance(b"\r\n", &mut sink);
        }
        term.advance(b"XXXXXXXXXXXXXXXXXXXX", &mut sink);
    }

    // Move to 3, 5 and erase to end of line: EL 0
    term.advance(b"\x1b[3;5H\x1b[0K", &mut sink);
    let text = term.text_range(termin_vt::LineRange {
        start: termin_vt::AbsLine(2),
        end: termin_vt::AbsLine(3),
    });
    assert_eq!(text.trim_end(), "XXXX");

    // Erase entire line: EL 2
    term.advance(b"\x1b[2;1H\x1b[2K", &mut sink);
    let text_row2 = term.text_range(termin_vt::LineRange {
        start: termin_vt::AbsLine(1),
        end: termin_vt::AbsLine(2),
    });
    assert_eq!(text_row2.trim_end(), "");

    // Erase display: ED 2
    term.advance(b"\x1b[2J", &mut sink);
    let full = term.text_range(termin_vt::LineRange {
        start: termin_vt::AbsLine(0),
        end: termin_vt::AbsLine(5),
    });
    assert_eq!(full.trim(), "");
}

#[test]
fn test_sgr_extended_colors_and_styles() {
    let cfg = TerminalConfig::default();
    let mut term = Terminal::new(&cfg);
    let mut sink = TestSink::default();

    // SGR 256 color: 38;5;123
    term.advance(b"\x1b[38;5;123mColor\x1b[0m", &mut sink);
    let cell = &term
        .view(termin_vt::ScrollPos::Bottom)
        .next()
        .unwrap()
        .cells[0];
    let style = term.styles().get(cell.style).unwrap();
    assert_eq!(style.fg, Color::Indexed(123));

    // SGR TrueColor RGB: 38;2;10;20;30 and colon style 48:2::40:50:60
    term.advance(b"\x1b[38;2;10;20;30;48:2::40:50:60mRGB\x1b[0m", &mut sink);
    let cell_rgb = &term
        .view(termin_vt::ScrollPos::Bottom)
        .next()
        .unwrap()
        .cells[5];
    let style_rgb = term.styles().get(cell_rgb.style).unwrap();
    assert_eq!(style_rgb.fg, Color::Rgb(10, 20, 30));
    assert_eq!(style_rgb.bg, Color::Rgb(40, 50, 60));

    // Underline styles: curly underline 4:3 and underline color 58;2;255;0;0
    term.advance(b"\x1b[4:3;58;2;255;0;0mCurly\x1b[0m", &mut sink);
    let cell_ul = &term
        .view(termin_vt::ScrollPos::Bottom)
        .next()
        .unwrap()
        .cells[8];
    let style_ul = term.styles().get(cell_ul.style).unwrap();
    assert!(style_ul.attrs.contains(Attrs::CURLY_UNDERLINE));
    assert_eq!(style_ul.ul_color, Color::Rgb(255, 0, 0));
}

#[test]
fn test_scroll_regions_and_scrolling() {
    let cfg = TerminalConfig {
        cols: 20,
        rows: 5,
        ..Default::default()
    };
    let mut term = Terminal::new(&cfg);
    let mut sink = TestSink::default();

    // Set scroll region from line 2 to line 4 (1-based -> 1..3 0-based)
    term.advance(b"\x1b[2;4r", &mut sink);

    // Print text on line 1 (outside scroll region)
    term.advance(b"\x1b[1;1HHeader Line", &mut sink);

    // Print text in scroll region and scroll up
    term.advance(b"\x1b[2;1HRow A\r\nRow B\r\nRow C", &mut sink);

    // Scroll up 1 line in region: CSI 1 S
    term.advance(b"\x1b[1S", &mut sink);

    let view: Vec<_> = term.view(termin_vt::ScrollPos::Bottom).collect();
    let row1: String = view[0]
        .cells
        .iter()
        .take(11)
        .filter_map(|c| char::from_u32(c.ch))
        .collect();
    // Header Line must remain intact
    assert_eq!(row1, "Header Line");
}

#[test]
fn test_device_reports() {
    let cfg = TerminalConfig {
        cols: 80,
        rows: 24,
        cell_px: (10, 20),
        ..Default::default()
    };
    let mut term = Terminal::new(&cfg);
    let mut sink = TestSink::default();

    // DA1: CSI c
    term.advance(b"\x1b[c", &mut sink);
    assert_eq!(sink.writes, b"\x1b[?62;22c");
    sink.writes.clear();

    // DA2: CSI > c
    term.advance(b"\x1b[>c", &mut sink);
    assert_eq!(sink.writes, b"\x1b[>1;10;0c");
    sink.writes.clear();

    // DSR status: CSI 5 n
    term.advance(b"\x1b[5n", &mut sink);
    assert_eq!(sink.writes, b"\x1b[0n");
    sink.writes.clear();

    // CPR: CSI 6 n at row 7, col 12 (1-based)
    term.advance(b"\x1b[7;12H\x1b[6n", &mut sink);
    assert_eq!(sink.writes, b"\x1b[7;12R");
    sink.writes.clear();

    // XTWINOPS 14 (window pixels), 16 (cell pixels), 18 (chars)
    term.advance(b"\x1b[14t", &mut sink);
    assert_eq!(
        sink.writes,
        format!("\x1b[4;{};{}t", 24 * 20, 80 * 10).as_bytes()
    );
    sink.writes.clear();

    term.advance(b"\x1b[16t", &mut sink);
    assert_eq!(sink.writes, b"\x1b[6;20;10t");
    sink.writes.clear();

    term.advance(b"\x1b[18t", &mut sink);
    assert_eq!(sink.writes, b"\x1b[8;24;80t");
    sink.writes.clear();

    // XTVERSION: CSI > q
    term.advance(b"\x1b[>q", &mut sink);
    assert_eq!(sink.writes, b"\x1bP>|termin(0.1.0)\x1b\\");
}

#[test]
fn test_osc_handling() {
    let cfg = TerminalConfig::default();
    let mut term = Terminal::new(&cfg);
    let mut sink = TestSink::default();

    // OSC 0/1/2 Title
    term.advance(b"\x1b]0;Termin Window\x1b\\", &mut sink);
    assert!(sink
        .effects
        .iter()
        .any(|e| matches!(e, Effect::Title(t) if &**t == "Termin Window")));

    // OSC 7 CWD
    term.advance(
        b"\x1b]7;file://localhost/Users/alice/projects%20dir\x1b\\",
        &mut sink,
    );
    assert!(sink
        .effects
        .iter()
        .any(|e| matches!(e, Effect::Cwd(p) if &**p == "/Users/alice/projects dir")));

    // OSC 8 Hyperlinks
    term.advance(
        b"\x1b]8;id=link1;https://example.com\x1b\\Link Text\x1b]8;;\x1b\\",
        &mut sink,
    );
    let cell_link = &term
        .view(termin_vt::ScrollPos::Bottom)
        .next()
        .unwrap()
        .cells[0];
    assert!(cell_link.style > 0);
    let style = term.styles().get(cell_link.style).unwrap();
    assert!(style.link > 0);

    // OSC 9 & 777 Notifications
    term.advance(b"\x1b]9;Alert message\x1b\\", &mut sink);
    assert!(sink
        .effects
        .iter()
        .any(|e| matches!(e, Effect::Notify { body, .. } if &**body == "Alert message")));

    term.advance(b"\x1b]777;notify;Build;Done\x1b\\", &mut sink);
    assert!(sink.effects.iter().any(
        |e| matches!(e, Effect::Notify { title, body } if &**title == "Build" && &**body == "Done")
    ));

    // OSC 52 Clipboard write
    term.advance(b"\x1b]52;c;SGVsbG8=\x1b\\", &mut sink);
    assert!(sink
        .effects
        .iter()
        .any(|e| matches!(e, Effect::ClipboardSet(b) if &**b == b"Hello")));
}

#[test]
fn test_cursor_shapes_and_modes() {
    let cfg = TerminalConfig::default();
    let mut term = Terminal::new(&cfg);
    let mut sink = TestSink::default();

    // DECSCUSR shapes
    term.advance(b"\x1b[2 q", &mut sink); // Steady block
    assert_eq!(term.cursor().shape, CursorShape::Block);
    assert!(!term.cursor().blink);

    term.advance(b"\x1b[4 q", &mut sink); // Steady underline
    assert_eq!(term.cursor().shape, CursorShape::Underline);
    assert!(!term.cursor().blink);

    term.advance(b"\x1b[5 q", &mut sink); // Blinking bar
    assert_eq!(term.cursor().shape, CursorShape::Bar);
    assert!(term.cursor().blink);

    // Modes
    term.advance(b"\x1b[?1000h\x1b[?1006h", &mut sink);
    assert!(term.modes().contains(Modes::MOUSE_NORMAL));
    assert!(term.modes().contains(Modes::MOUSE_SGR));

    term.advance(b"\x1b[?1000l", &mut sink);
    assert!(!term.modes().contains(Modes::MOUSE_NORMAL));
}
