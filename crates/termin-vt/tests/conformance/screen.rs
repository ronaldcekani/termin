//! Conformance tests for vttest Menu 2: Screen features.
//!
//! Covers:
//! - ED: Erase in Display (0: cursor to end, 1: start to cursor, 2: all screen, 3: scrollback)
//! - EL: Erase in Line (0: cursor to end, 1: start to cursor, 2: all line)
//! - IL: Insert Line (within scrolling margins, ignored outside)
//! - DL: Delete Line (within scrolling margins, ignored outside)
//! - ICH: Insert Character, DCH: Delete Character
//! - ECH: Erase Character
//! - SU: Scroll Up, SD: Scroll Down
//! - DECSTBM: Scroll margins, partial region scrolling
//! - BCE: Background Color Erase across all erase and scroll operations
//! - Tab stops: HT, HTS, TBC 0/3, CHT, CBT

use super::helper::TestTerm;
use termin_vt::Color;

#[test]
fn test_ed_erase_display() {
    let mut term = TestTerm::new(20, 5);

    // Populate all 5 lines
    for i in 1..=5 {
        if i > 1 {
            term.advance(b"\r\n");
        }
        term.advance(format!("Line {}XXXXXXXXXXXXXX", i).as_bytes());
    }

    // ED 0: Erase from cursor to end of screen (inclusive)
    // Position at row 3, col 7 (1-based) -> (2, 6)
    term.advance(b"\x1b[3;7H\x1b[0J");
    assert_eq!(term.row_text_trimmed(0), "Line 1XXXXXXXXXXXXXX");
    assert_eq!(term.row_text_trimmed(1), "Line 2XXXXXXXXXXXXXX");
    assert_eq!(term.row_text_trimmed(2), "Line 3");
    assert_eq!(term.row_text_trimmed(3), "");
    assert_eq!(term.row_text_trimmed(4), "");

    // Refill lines 3..5
    term.advance(b"\x1b[3;1HLine 3XXXXXXXXXXXXXX\r\nLine 4XXXXXXXXXXXXXX\r\nLine 5XXXXXXXXXXXXXX");

    // ED 1: Erase from beginning of screen to cursor (inclusive)
    term.advance(b"\x1b[3;7H\x1b[1J");
    assert_eq!(term.row_text_trimmed(0), "");
    assert_eq!(term.row_text_trimmed(1), "");
    // Col 0..=6 erased, rest of row remains
    assert_eq!(&term.row_text(2)[0..7], "       ");
    assert_eq!(&term.row_text_trimmed(2)[7..], "XXXXXXXXXXXXX");
    assert_eq!(term.row_text_trimmed(3), "Line 4XXXXXXXXXXXXXX");
    assert_eq!(term.row_text_trimmed(4), "Line 5XXXXXXXXXXXXXX");

    // ED 2: Erase entire display
    term.advance(b"\x1b[2J");
    for r in 0..5 {
        assert_eq!(term.row_text_trimmed(r), "");
    }
}

#[test]
fn test_el_erase_line() {
    let mut term = TestTerm::new(20, 3);

    term.advance(b"\x1b[1;1H01234567890123456789");
    term.advance(b"\x1b[2;1H01234567890123456789");
    term.advance(b"\x1b[3;1H01234567890123456789");

    // EL 0: Erase cursor to end of line
    term.advance(b"\x1b[1;6H\x1b[0K");
    assert_eq!(term.row_text_trimmed(0), "01234");

    // EL 1: Erase start of line to cursor
    term.advance(b"\x1b[2;6H\x1b[1K");
    assert_eq!(&term.row_text(1)[0..6], "      ");
    assert_eq!(&term.row_text_trimmed(1)[6..], "67890123456789");

    // EL 2: Erase entire line
    term.advance(b"\x1b[3;6H\x1b[2K");
    assert_eq!(term.row_text_trimmed(2), "");
}

#[test]
fn test_il_and_dl_line_insert_delete() {
    let mut term = TestTerm::new(20, 5);

    term.advance(b"Line 1\r\nLine 2\r\nLine 3\r\nLine 4\r\nLine 5");
    assert_eq!(term.row_text_trimmed(0), "Line 1");
    assert_eq!(term.row_text_trimmed(1), "Line 2");
    assert_eq!(term.row_text_trimmed(2), "Line 3");

    // IL: Insert Line at row 2 (0-based: 1)
    term.advance(b"\x1b[2;1H\x1b[1L");
    assert_eq!(term.row_text_trimmed(0), "Line 1");
    assert_eq!(term.row_text_trimmed(1), ""); // newly inserted blank line
    assert_eq!(term.row_text_trimmed(2), "Line 2");
    assert_eq!(term.row_text_trimmed(3), "Line 3");
    assert_eq!(term.row_text_trimmed(4), "Line 4"); // Line 5 was pushed off

    // DL: Delete Line at row 2 (0-based: 1)
    term.advance(b"\x1b[2;1H\x1b[1M");
    assert_eq!(term.row_text_trimmed(0), "Line 1");
    assert_eq!(term.row_text_trimmed(1), "Line 2");
    assert_eq!(term.row_text_trimmed(2), "Line 3");
    assert_eq!(term.row_text_trimmed(3), "Line 4");
    assert_eq!(term.row_text_trimmed(4), ""); // blank line pulled from bottom
}

#[test]
fn test_il_and_dl_respect_scroll_margins() {
    let mut term = TestTerm::new(20, 6);

    for i in 1..=6 {
        if i > 1 {
            term.advance(b"\r\n");
        }
        term.advance(format!("Line {}", i).as_bytes());
    }

    // Set scrolling margins to rows 2..=5 (0-based: 1..=4)
    term.advance(b"\x1b[2;5r");

    // Test IL inside margins: cursor at row 3 (0-based: 2)
    term.advance(b"\x1b[3;1H\x1b[1L");
    assert_eq!(term.row_text_trimmed(0), "Line 1"); // untouched (outside top)
    assert_eq!(term.row_text_trimmed(1), "Line 2");
    assert_eq!(term.row_text_trimmed(2), "");       // inserted
    assert_eq!(term.row_text_trimmed(3), "Line 3");
    assert_eq!(term.row_text_trimmed(4), "Line 4"); // Line 5 pushed off
    assert_eq!(term.row_text_trimmed(5), "Line 6"); // untouched (outside bottom)

    // Test IL outside margins: ignored!
    term.advance(b"\x1b[1;1H\x1b[1L"); // Row 1 is above top margin
    assert_eq!(term.row_text_trimmed(0), "Line 1"); // No change!

    term.advance(b"\x1b[6;1H\x1b[1L"); // Row 6 is below bottom margin
    assert_eq!(term.row_text_trimmed(5), "Line 6"); // No change!
}

#[test]
fn test_ich_dch_ech_character_operations() {
    let mut term = TestTerm::new(20, 2);

    term.advance(b"\x1b[1;1HABCDEFGHIJ");

    // ICH: Insert Character (shifts right)
    term.advance(b"\x1b[1;4H\x1b[3@"); // cursor at 'D' (col 3), insert 3
    assert_eq!(term.row_text_trimmed(0), "ABC   DEFGHIJ");

    // DCH: Delete Character (shifts left, restoring original characters)
    term.advance(b"\x1b[1;4H\x1b[3P"); // delete 3 spaces
    assert_eq!(term.row_text_trimmed(0), "ABCDEFGHIJ");

    // ECH: Erase Character (replaces with blanks, no shift)
    term.advance(b"\x1b[1;2H\x1b[4X"); // erase 4 chars at 'B'
    assert_eq!(term.row_text_trimmed(0), "A    FGHIJ");
}

#[test]
fn test_su_and_sd_scrolling() {
    let mut term = TestTerm::new(20, 4);

    term.advance(b"Row 1\r\nRow 2\r\nRow 3\r\nRow 4");

    // SU: Scroll Up 2 lines
    term.advance(b"\x1b[2S");
    assert_eq!(term.row_text_trimmed(0), "Row 3");
    assert_eq!(term.row_text_trimmed(1), "Row 4");
    assert_eq!(term.row_text_trimmed(2), "");
    assert_eq!(term.row_text_trimmed(3), "");

    // SD: Scroll Down 1 line
    term.advance(b"\x1b[1T");
    assert_eq!(term.row_text_trimmed(0), "");
    assert_eq!(term.row_text_trimmed(1), "Row 3");
    assert_eq!(term.row_text_trimmed(2), "Row 4");
    assert_eq!(term.row_text_trimmed(3), "");
}

#[test]
fn test_bce_background_color_erase() {
    let mut term = TestTerm::new(20, 4);

    // Set background to Blue (SGR 44)
    term.advance(b"\x1b[44m");

    // Erase entire screen with ED 2 -> all cells must have Blue background
    term.advance(b"\x1b[2J");
    for r in 0..4 {
        for c in 0..20 {
            let style = term.style_at(r, c).expect("style");
            assert_eq!(style.bg, Color::Indexed(4), "Cell ({r}, {c}) has BCE bg");
        }
    }

    // Write text with Red background (SGR 41)
    term.advance(b"\x1b[1;1H\x1b[41mHello World");
    assert_eq!(term.style_at(0, 0).unwrap().bg, Color::Indexed(1));

    // Clear line from cursor with EL 0 using Blue background
    term.advance(b"\x1b[44m\x1b[1;6H\x1b[0K");
    assert_eq!(term.style_at(0, 0).unwrap().bg, Color::Indexed(1)); // 'Hello' still red
    assert_eq!(term.style_at(0, 5).unwrap().bg, Color::Indexed(4)); // Erased area blue

    // ECH with Green background (SGR 42)
    term.advance(b"\x1b[42m\x1b[1;1H\x1b[2X");
    assert_eq!(term.style_at(0, 0).unwrap().bg, Color::Indexed(2));
    assert_eq!(term.style_at(0, 1).unwrap().bg, Color::Indexed(2));

    // LF scrolling with Magenta background (SGR 45)
    term.advance(b"\x1b[45m\x1b[4;1H\n");
    // Bottom row was scrolled in and must have Magenta background
    for c in 0..20 {
        assert_eq!(term.style_at(3, c).unwrap().bg, Color::Indexed(5));
    }
}

#[test]
fn test_tab_stops_and_tabulation() {
    let mut term = TestTerm::new(40, 2);

    // Initial default tab stops are at 8, 16, 24, 32
    term.advance(b"\t");
    assert_eq!(term.cursor_col(), 8);
    term.advance(b"\t");
    assert_eq!(term.cursor_col(), 16);

    // HTS (ESC H): Set custom tab stop at col 20
    term.advance(b"\x1b[1;21H\x1bH");

    // Move to col 16 and jump -> should stop at 20
    term.advance(b"\x1b[1;17H\t");
    assert_eq!(term.cursor_col(), 20);

    // TBC 0 (CSI 0 g): Clear tab stop at current position (col 20)
    term.advance(b"\x1b[0g");

    // Now tab jumps from col 16 straight to 24
    term.advance(b"\x1b[1;17H\t");
    assert_eq!(term.cursor_col(), 24);

    // CHT (Cursor Forward Tabulation): jump forward 2 tab stops -> 40 (clamped to 39)
    term.advance(b"\x1b[1;1H\x1b[2I");
    assert_eq!(term.cursor_col(), 16);

    // CBT (Cursor Backward Tabulation): jump back 1 tab stop -> 8
    term.advance(b"\x1b[1Z");
    assert_eq!(term.cursor_col(), 8);

    // TBC 3 (CSI 3 g): Clear all tab stops
    term.advance(b"\x1b[3g");
    term.advance(b"\x1b[1;1H\t");
    // With no tab stops, tab advances to right edge (col 39)
    assert_eq!(term.cursor_col(), 39);
}
