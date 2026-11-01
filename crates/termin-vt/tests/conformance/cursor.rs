//! Conformance tests for vttest Menu 1: Cursor movements.
//!
//! Covers:
//! - CUP (Cursor Position) / HVP (Horizontal and Vertical Position)
//! - CUU (Cursor Up), CUD (Cursor Down), CUF (Cursor Forward), CUB (Cursor Backward)
//! - CNL (Cursor Next Line), CPL (Cursor Previous Line)
//! - CHA / HPA (Cursor Horizontal Absolute)
//! - VPA (Vertical Position Absolute), VPR (Vertical Position Relative), HPR (Horizontal Position Relative)
//! - DECOM (Origin Mode) clamping, home on enable/disable, scroll region boundaries
//! - DECAWM (Autowrap) & pending wrap state behavior with BS, CR, CUF, and disabled wrap
//! - IND (Index), RI (Reverse Index), NEL (Next Line)
//! - DECSC (Save Cursor) / DECRC (Restore Cursor) & CSI s / CSI u

use super::helper::TestTerm;
use termin_vt::{Attrs, Color, Modes};

#[test]
fn test_cup_and_hvp_positioning() {
    let mut term = TestTerm::new(80, 24);

    // Initial position is (0, 0)
    assert_eq!(term.cursor(), (0, 0));

    // CUP explicit 1-based: row 10, col 20 -> 0-based (9, 19)
    term.advance(b"\x1b[10;20H");
    assert_eq!(term.cursor(), (9, 19));

    // HVP is identical to CUP
    term.advance(b"\x1b[5;15f");
    assert_eq!(term.cursor(), (4, 14));

    // Default parameters should home to (0, 0)
    term.advance(b"\x1b[H");
    assert_eq!(term.cursor(), (0, 0));

    // Single parameter: row only, col defaults to 1
    term.advance(b"\x1b[7H");
    assert_eq!(term.cursor(), (6, 0));

    // Semicolon with omitted first param: row defaults to 1, col 30
    term.advance(b"\x1b[;30H");
    assert_eq!(term.cursor(), (0, 29));

    // Out of bounds values clamp to margins / screen size
    term.advance(b"\x1b[999;999H");
    assert_eq!(term.cursor(), (23, 79));

    // Zero parameters are treated as 1 (1-based)
    term.advance(b"\x1b[0;0H");
    assert_eq!(term.cursor(), (0, 0));
}

#[test]
fn test_relative_cursor_movements() {
    let mut term = TestTerm::new(80, 24);

    term.advance(b"\x1b[10;10H");
    assert_eq!(term.cursor(), (9, 9));

    // CUU: Cursor Up 3 -> row 6 (0-based)
    term.advance(b"\x1b[3A");
    assert_eq!(term.cursor(), (6, 9));

    // CUD: Cursor Down 4 -> row 10
    term.advance(b"\x1b[4B");
    assert_eq!(term.cursor(), (10, 9));

    // CUF: Cursor Forward 5 -> col 14
    term.advance(b"\x1b[5C");
    assert_eq!(term.cursor(), (10, 14));

    // CUB: Cursor Backward 2 -> col 12
    term.advance(b"\x1b[2D");
    assert_eq!(term.cursor(), (10, 12));

    // Default count is 1
    term.advance(b"\x1b[A");
    assert_eq!(term.cursor(), (9, 12));
    term.advance(b"\x1b[B");
    assert_eq!(term.cursor(), (10, 12));
    term.advance(b"\x1b[C");
    assert_eq!(term.cursor(), (10, 13));
    term.advance(b"\x1b[D");
    assert_eq!(term.cursor(), (10, 12));

    // Clamping at boundaries: moving up 100 clamps at top (row 0), does not scroll
    term.advance(b"\x1b[100A");
    assert_eq!(term.cursor(), (0, 12));

    // Moving left 100 clamps at col 0
    term.advance(b"\x1b[100D");
    assert_eq!(term.cursor(), (0, 0));

    // Moving down 100 clamps at row 23
    term.advance(b"\x1b[100B");
    assert_eq!(term.cursor(), (23, 0));

    // Moving right 100 clamps at col 79
    term.advance(b"\x1b[100C");
    assert_eq!(term.cursor(), (23, 79));
}

#[test]
fn test_cnl_and_cpl() {
    let mut term = TestTerm::new(80, 24);

    term.advance(b"\x1b[5;25H");
    assert_eq!(term.cursor(), (4, 24));

    // CNL: Cursor Next Line down 2 -> row 6, col 0
    term.advance(b"\x1b[2E");
    assert_eq!(term.cursor(), (6, 0));

    // CPL: Cursor Previous Line up 3 -> row 3, col 0
    term.advance(b"\x1b[3F");
    assert_eq!(term.cursor(), (3, 0));

    // Default parameter is 1
    term.advance(b"\x1b[E");
    assert_eq!(term.cursor(), (4, 0));
    term.advance(b"\x1b[F");
    assert_eq!(term.cursor(), (3, 0));
}

#[test]
fn test_horizontal_and_vertical_absolute_and_relative() {
    let mut term = TestTerm::new(80, 24);

    // CHA / HPA: Horizontal Position Absolute
    term.advance(b"\x1b[42G");
    assert_eq!(term.cursor_col(), 41);

    term.advance(b"\x1b[17`");
    assert_eq!(term.cursor_col(), 16);

    // VPA: Vertical Position Absolute
    term.advance(b"\x1b[12d");
    assert_eq!(term.cursor_row(), 11);

    // VPR: Vertical Position Relative down 4 -> row 15
    term.advance(b"\x1b[4e");
    assert_eq!(term.cursor_row(), 15);

    // HPR: Horizontal Position Relative right 10 -> col 26
    term.advance(b"\x1b[10a");
    assert_eq!(term.cursor_col(), 26);
}

#[test]
fn test_decom_origin_mode() {
    let mut term = TestTerm::new(80, 24);

    // Set scroll margins to rows 5..=15 (0-based: 4..=14)
    term.advance(b"\x1b[5;15r");

    // Setting DECSTBM moves cursor to (0, 0)
    assert_eq!(term.cursor(), (0, 0));

    // Enable Origin Mode: ?6h
    term.advance(b"\x1b[?6h");
    assert!(term.modes().contains(Modes::ORIGIN));

    // Enabling DECOM moves cursor to top-left of scroll region: row 4, col 0
    assert_eq!(term.cursor(), (4, 0));

    // In DECOM, CUP (1, 1) refers to top-left of the scroll region
    term.advance(b"\x1b[1;1H");
    assert_eq!(term.cursor(), (4, 0));

    // CUP row 3 -> row 4 + 2 = 6
    term.advance(b"\x1b[3;10H");
    assert_eq!(term.cursor(), (6, 9));

    // Cursor cannot be moved above top margin with CUU in DECOM
    term.advance(b"\x1b[100A");
    assert_eq!(term.cursor(), (4, 9));

    // Cursor cannot be moved below bottom margin with CUD in DECOM
    term.advance(b"\x1b[100B");
    assert_eq!(term.cursor(), (14, 9));

    // VPA in DECOM is relative to top margin: row 1 -> row 4
    term.advance(b"\x1b[1d");
    assert_eq!(term.cursor_row(), 4);

    // Out of bounds CUP clamps to bottom margin in DECOM
    term.advance(b"\x1b[99;1H");
    assert_eq!(term.cursor(), (14, 0));

    // Device Status Report (DSR 6 CPR) in DECOM is relative to top margin
    term.clear_writes();
    term.advance(b"\x1b[6n");
    // Row 14 is the 11th row of the region (1-based: 11)
    assert_eq!(term.last_write(), "\x1b[11;1R");

    // Disable Origin Mode: ?6l
    term.advance(b"\x1b[?6l");
    assert!(!term.modes().contains(Modes::ORIGIN));

    // Disabling DECOM moves cursor to (0, 0) of the whole screen
    assert_eq!(term.cursor(), (0, 0));

    // Full screen cursor addressing works again
    term.advance(b"\x1b[2;2H");
    assert_eq!(term.cursor(), (1, 1));
}

#[test]
fn test_decawm_autowrap_and_pending_wrap() {
    let mut term = TestTerm::new(10, 4);

    // Write 10 chars to fill line 0
    term.advance(b"0123456789");

    // Cursor remains on last column (col 9) with pending wrap
    assert_eq!(term.cursor(), (0, 9));

    // Next printable character wraps to line 1 and writes at col 1
    term.advance(b"A");
    assert_eq!(term.cursor(), (1, 1));
    assert_eq!(term.char_at(0, 9), Some('9'));
    assert_eq!(term.char_at(1, 0), Some('A'));

    // Test pending wrap cancellation via Backspace (BS 0x08)
    term.advance(b"\x1b[1;1H");
    term.advance(b"0123456789");
    assert_eq!(term.cursor(), (0, 9));

    // Backspace cancels pending wrap and moves to col 8
    term.advance(b"\x08X");
    assert_eq!(term.cursor(), (0, 9));
    assert_eq!(term.char_at(0, 8), Some('X'));

    // Test pending wrap cancellation via Carriage Return (CR 0x0d)
    term.advance(b"\x1b[2;1H");
    term.advance(b"0123456789");
    assert_eq!(term.cursor(), (1, 9));
    term.advance(b"\rY");
    assert_eq!(term.cursor(), (1, 1));
    assert_eq!(term.char_at(1, 0), Some('Y'));

    // Test Autowrap disabled: ?7l
    term.advance(b"\x1b[?7l");
    assert!(!term.modes().contains(Modes::AUTOWRAP));

    term.advance(b"\x1b[3;1H");
    term.advance(b"0123456789ZZZ");
    // With autowrap off, characters at the right margin overwrite col 9 in place
    assert_eq!(term.cursor(), (2, 9));
    assert_eq!(term.char_at(2, 9), Some('Z'));
    // Line 4 remains empty
    assert_eq!(term.row_text_trimmed(3), "");
}

#[test]
fn test_ind_ri_nel() {
    let mut term = TestTerm::new(20, 5);

    // IND: Index (ESC D) - down 1 line without column reset
    term.advance(b"\x1b[2;5H");
    term.advance(b"\x1bD");
    assert_eq!(term.cursor(), (2, 4));

    // At bottom of screen, IND scrolls up
    term.advance(b"\x1b[5;1HLine 5");
    term.advance(b"\x1bD");
    assert_eq!(term.cursor_row(), 4);
    assert_eq!(term.row_text_trimmed(3), "Line 5");

    // RI: Reverse Index (ESC M) - up 1 line without column reset
    term.advance(b"\x1b[3;10H");
    term.advance(b"\x1bM");
    assert_eq!(term.cursor(), (1, 9));

    // At top of screen, RI scrolls down
    term.advance(b"\x1b[1;1HTop Line");
    term.advance(b"\x1bM");
    assert_eq!(term.cursor_row(), 0);
    assert_eq!(term.row_text_trimmed(1), "Top Line");

    // NEL: Next Line (ESC E) - CR + LF
    term.advance(b"\x1b[2;15H");
    term.advance(b"\x1bE");
    assert_eq!(term.cursor(), (2, 0));

    // At bottom of screen, NEL scrolls up and homes col to 0
    term.advance(b"\x1b[5;10H");
    term.advance(b"\x1bE");
    assert_eq!(term.cursor(), (4, 0));
}

#[test]
fn test_decsc_decrc_and_ansi_save_restore() {
    let mut term = TestTerm::new(40, 10);

    // Enable origin mode, position cursor, set bold + red style
    term.advance(b"\x1b[?6h\x1b[3;10H\x1b[1;31m");
    assert_eq!(term.cursor(), (2, 9));
    assert!(term.modes().contains(Modes::ORIGIN));

    // Save with DECSC (ESC 7)
    term.advance(b"\x1b7");

    // Mutate state: move to (1, 1), clear style, disable origin mode
    term.advance(b"\x1b[1;1H\x1b[0m\x1b[?6l");
    assert_eq!(term.cursor(), (0, 0));
    assert!(!term.modes().contains(Modes::ORIGIN));

    // Restore with DECRC (ESC 8)
    term.advance(b"\x1b8");
    assert_eq!(term.cursor(), (2, 9));
    assert!(term.modes().contains(Modes::ORIGIN));

    // Write a char and verify restored style had bold + red
    term.advance(b"X");
    let style = term.style_at(2, 9).expect("style present");
    assert!(style.attrs.contains(Attrs::BOLD));
    assert_eq!(style.fg, Color::Indexed(1));

    // Test ANSI.SYS save and restore (CSI s and CSI u)
    term.advance(b"\x1b[5;20H");
    term.advance(b"\x1b[s");
    term.advance(b"\x1b[1;1H");
    assert_eq!(term.cursor(), (0, 0));
    term.advance(b"\x1b[u");
    assert_eq!(term.cursor(), (4, 19));
}
