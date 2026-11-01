//! Conformance tests for terminal modes, protocols, and device reports (esctest).
//!
//! Covers:
//! - DECCKM (Application Cursor Keys ?1) & keypad encoding
//! - DECKPAM (Application Keypad ESC =) & DECKPNM (Numeric Keypad ESC >)
//! - Alternate screen switching (?1049, ?1047, ?47)
//! - Bracketed paste (?2004)
//! - Focus event reporting (?1004)
//! - Synchronized output (?2026) and generation bumping
//! - Mouse tracking modes (?1000, ?1002, ?1003, ?1006 SGR) & encoding
//! - Kitty keyboard protocol flag stack (CSI > flags u, CSI < u, CSI = u, CSI ? u)
//! - Device Status Report (DSR 5 status, DSR 6 cursor position)
//! - Primary Device Attributes (DA1 CSI c) & Secondary Device Attributes (DA2 CSI > c)
//! - XTVERSION (CSI > q)
//! - DECRQM (DEC Request Mode: CSI ? Pd $ p and CSI Pa $ p)

use super::helper::TestTerm;
use termin_core::{Key, KeyEvent, Mods, MouseButton, MouseEvent, MouseKind, Point};
use termin_vt::{CellMouseEvent, CursorShape, Modes};

#[test]
fn test_decckm_cursor_keys() {
    let mut term = TestTerm::new(80, 24);

    let up_key = KeyEvent {
        key: Key::Named(termin_core::NamedKey::ArrowUp),
        physical: 0,
        mods: Mods::empty(),
        text: None,
        repeat: false,
        pressed: true,
    };

    // Normal mode: ArrowUp is ESC [ A
    let enc_normal = term.term.encode_key(&up_key);
    assert_eq!(&*enc_normal, b"\x1b[A");

    // Enable Application Cursor Keys: ?1h
    term.advance(b"\x1b[?1h");
    assert!(term.modes().contains(Modes::DECCKM));

    let enc_app = term.term.encode_key(&up_key);
    assert_eq!(&*enc_app, b"\x1bOA");

    // Disable Application Cursor Keys: ?1l
    term.advance(b"\x1b[?1l");
    assert!(!term.modes().contains(Modes::DECCKM));
    let enc_back = term.term.encode_key(&up_key);
    assert_eq!(&*enc_back, b"\x1b[A");
}

#[test]
fn test_deckpam_and_deckpnm() {
    let mut term = TestTerm::new(80, 24);

    assert!(!term.modes().contains(Modes::DECKPAM));

    // Application keypad mode: ESC =
    term.advance(b"\x1b=");
    assert!(term.modes().contains(Modes::DECKPAM));

    // Numeric keypad mode: ESC >
    term.advance(b"\x1b>");
    assert!(!term.modes().contains(Modes::DECKPAM));
}

#[test]
fn test_alt_screen_modes() {
    let mut term = TestTerm::new(40, 5);

    // Primary screen content
    term.advance(b"Primary Screen Content\x1b[2;5H");
    assert_eq!(term.row_text_trimmed(0), "Primary Screen Content");
    assert_eq!(term.cursor(), (1, 4));

    // Enter alt screen with ?1049h (saves cursor, clears alt screen)
    term.advance(b"\x1b[?1049h");
    assert!(term.is_alt_screen());
    assert!(term.modes().contains(Modes::ALT_SCREEN));
    assert_eq!(term.cursor(), (0, 0));
    assert_eq!(term.row_text_trimmed(0), "");

    // Write to alt screen
    term.advance(b"Alt Screen Content");
    assert_eq!(term.row_text_trimmed(0), "Alt Screen Content");

    // Exit alt screen with ?1049l (restores primary screen & cursor)
    term.advance(b"\x1b[?1049l");
    assert!(!term.is_alt_screen());
    assert!(!term.modes().contains(Modes::ALT_SCREEN));
    assert_eq!(term.row_text_trimmed(0), "Primary Screen Content");
    assert_eq!(term.cursor(), (1, 4));

    // Test 1047 / 47 alternate screen
    term.advance(b"\x1b[?1047h");
    assert!(term.is_alt_screen());
    term.advance(b"\x1b[?1047l");
    assert!(!term.is_alt_screen());
}

#[test]
fn test_bracketed_paste_and_focus_events() {
    let mut term = TestTerm::new(80, 24);

    term.advance(b"\x1b[?2004h");
    assert!(term.modes().contains(Modes::BRACKETED_PASTE));

    term.advance(b"\x1b[?2004l");
    assert!(!term.modes().contains(Modes::BRACKETED_PASTE));

    term.advance(b"\x1b[?1004h");
    assert!(term.modes().contains(Modes::FOCUS_EVENTS));

    term.advance(b"\x1b[?1004l");
    assert!(!term.modes().contains(Modes::FOCUS_EVENTS));
}

#[test]
fn test_synchronized_output_mode() {
    let mut term = TestTerm::new(80, 24);

    let initial_gen = term.term.generation();

    // Enable Synchronized Output: ?2026h
    term.advance(b"\x1b[?2026h");
    assert!(term.modes().contains(Modes::SYNC_OUTPUT));

    // While synchronized output is active, advance does NOT bump generation
    term.advance(b"Buffered frame 1");
    term.advance(b"\x1b[2;1HBuffered frame 2");
    assert_eq!(term.term.generation(), initial_gen);

    // Disable Synchronized Output: ?2026l -> generation bumps!
    term.advance(b"\x1b[?2026l");
    assert!(!term.modes().contains(Modes::SYNC_OUTPUT));
    assert!(term.term.generation() > initial_gen);
    assert_eq!(term.row_text_trimmed(0), "Buffered frame 1");
    assert_eq!(term.row_text_trimmed(1), "Buffered frame 2");
}

#[test]
fn test_mouse_modes_and_sgr_encoding() {
    let mut term = TestTerm::new(80, 24);

    // Normal tracking (?1000) + SGR encoding (?1006)
    term.advance(b"\x1b[?1000h\x1b[?1006h");
    assert!(term.modes().contains(Modes::MOUSE_NORMAL));
    assert!(term.modes().contains(Modes::MOUSE_SGR));

    // Mouse press left button at row 5, col 10 (1-based: 6, 11)
    let ev_press = CellMouseEvent {
        pos: termin_vt::AbsPos::new(5, 10),
        event: MouseEvent {
            kind: MouseKind::Down(MouseButton::Left),
            pos: Point { x: 80.0, y: 80.0 },
            mods: Mods::empty(),
            click_count: 1,
        },
    };
    let enc_press = term.term.encode_mouse(&ev_press).expect("mouse encoded");
    assert_eq!(&*enc_press, b"\x1b[<0;11;6M");

    // Mouse release left button at row 5, col 10
    let ev_release = CellMouseEvent {
        pos: termin_vt::AbsPos::new(5, 10),
        event: MouseEvent {
            kind: MouseKind::Up(MouseButton::Left),
            pos: Point { x: 80.0, y: 80.0 },
            mods: Mods::empty(),
            click_count: 1,
        },
    };
    let enc_release = term.term.encode_mouse(&ev_release).expect("mouse encoded");
    assert_eq!(&*enc_release, b"\x1b[<0;11;6m");

    // Disable mouse tracking
    term.advance(b"\x1b[?1000l\x1b[?1006l");
    assert_eq!(term.term.encode_mouse(&ev_press), None);
}

#[test]
fn test_kitty_keyboard_protocol_stack() {
    let mut term = TestTerm::new(80, 24);

    // Initial query: flags 0
    term.advance(b"\x1b[?u");
    assert_eq!(term.take_writes(), "\x1b[?0u");

    // Push flags 1 (disambiguate): CSI > 1 u
    term.advance(b"\x1b[>1u");
    term.advance(b"\x1b[?u");
    assert_eq!(term.take_writes(), "\x1b[?1u");

    // Push flags 3: CSI > 3 u
    term.advance(b"\x1b[>3u");
    term.advance(b"\x1b[?u");
    assert_eq!(term.take_writes(), "\x1b[?3u");

    // Pop 1 entry: CSI < 1 u
    term.advance(b"\x1b[<1u");
    term.advance(b"\x1b[?u");
    assert_eq!(term.take_writes(), "\x1b[?1u");

    // Pop final entry: CSI < u (default 1)
    term.advance(b"\x1b[<u");
    term.advance(b"\x1b[?u");
    assert_eq!(term.take_writes(), "\x1b[?0u");
}

#[test]
fn test_device_status_and_attributes_reports() {
    let mut term = TestTerm::new(80, 24);

    // DSR 5 (Operating Status): reports OK (\x1b[0n)
    term.advance(b"\x1b[5n");
    assert_eq!(term.take_writes(), "\x1b[0n");

    // DSR 6 (Cursor Position Report): at row 8, col 15 -> 1-based (8, 15)
    term.advance(b"\x1b[8;15H\x1b[6n");
    assert_eq!(term.take_writes(), "\x1b[8;15R");

    // DA1: Device Attributes CSI c
    term.advance(b"\x1b[c");
    assert_eq!(term.take_writes(), "\x1b[?62;22c");

    // DA2: Secondary Device Attributes CSI > c
    term.advance(b"\x1b[>c");
    assert_eq!(term.take_writes(), "\x1b[>1;10;0c");

    // XTVERSION: CSI > q -> DCS > | termin(0.1.0) ST
    term.advance(b"\x1b[>q");
    assert_eq!(term.take_writes(), "\x1bP>|termin(0.1.0)\x1b\\");
}

#[test]
fn test_decscusr_cursor_shapes() {
    let mut term = TestTerm::new(80, 24);

    // Shape 2: Steady Block
    term.advance(b"\x1b[2 q");
    assert_eq!(term.term.cursor().shape, CursorShape::Block);
    assert!(!term.term.cursor().blink);

    // Shape 3: Blinking Underline
    term.advance(b"\x1b[3 q");
    assert_eq!(term.term.cursor().shape, CursorShape::Underline);
    assert!(term.term.cursor().blink);

    // Shape 5: Blinking Bar
    term.advance(b"\x1b[5 q");
    assert_eq!(term.term.cursor().shape, CursorShape::Bar);
    assert!(term.term.cursor().blink);

    // Shape 6: Steady Bar
    term.advance(b"\x1b[6 q");
    assert_eq!(term.term.cursor().shape, CursorShape::Bar);
    assert!(!term.term.cursor().blink);
}

#[test]
fn test_decrqm_request_mode() {
    let mut term = TestTerm::new(80, 24);

    // Query DECTCEM (?25: cursor visible, default on -> status 1)
    term.advance(b"\x1b[?25$p");
    assert_eq!(term.take_writes(), "\x1b[?25;1$y");

    // Hide cursor (?25l) and query again -> status 2 (reset)
    term.advance(b"\x1b[?25l\x1b[?25$p");
    assert_eq!(term.take_writes(), "\x1b[?25;2$y");

    // Query DECCKM (?1: cursor keys, default off -> status 2)
    term.advance(b"\x1b[?1$p");
    assert_eq!(term.take_writes(), "\x1b[?1;2$y");

    // Enable DECCKM (?1h) and query -> status 1
    term.advance(b"\x1b[?1h\x1b[?1$p");
    assert_eq!(term.take_writes(), "\x1b[?1;1$y");

    // Query unsupported DEC private mode -> status 0 (unrecognized)
    term.advance(b"\x1b[?9999$p");
    assert_eq!(term.take_writes(), "\x1b[?9999;0$y");

    // Query ANSI Insert mode (IRM 4: default reset -> status 2)
    term.advance(b"\x1b[4$p");
    assert_eq!(term.take_writes(), "\x1b[4;2$y");

    // Set Insert mode (CSI 4 h) and query -> status 1
    term.advance(b"\x1b[4h\x1b[4$p");
    assert_eq!(term.take_writes(), "\x1b[4;1$y");
}
