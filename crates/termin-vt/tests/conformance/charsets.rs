//! Conformance tests for vttest Menu 3: Character sets.
//!
//! Covers:
//! - G0/G1 designation: ESC ( 0, ESC ( B, ESC ) 0, ESC ) B, ESC ( A
//! - Shift In (SI 0x0F) / Shift Out (SO 0x0E) charset switching
//! - Complete DEC Special Graphics translation table (box-drawing characters, scanlines, symbols)
//! - UK charset national replacement characters (# -> £)
//! - Drawing full TUI window borders using DEC Special Graphics

use super::helper::TestTerm;

#[test]
fn test_g0_g1_designation_and_so_si() {
    let mut term = TestTerm::new(40, 5);

    // Designate G1 as DEC Special Graphics: ESC ) 0
    term.advance(b"\x1b)0");

    // Default GL is G0 (ASCII)
    term.advance(b"Normal Text ");

    // Shift Out (SO 0x0E): switch GL to G1 (DEC Special Graphics)
    term.advance(b"\x0eqqq\x0f"); // 'q' -> '─'

    // Shift In (SI 0x0F): switch GL back to G0 (ASCII)
    term.advance(b" Back to ASCII");

    assert_eq!(term.row_text_trimmed(0), "Normal Text ─── Back to ASCII");
}

#[test]
fn test_g0_designation_special_graphics() {
    let mut term = TestTerm::new(40, 2);

    // Designate G0 directly as DEC Special Graphics: ESC ( 0
    term.advance(b"\x1b(0");
    term.advance(b"lqqqk"); // ┌───┐
    assert_eq!(term.row_text_trimmed(0), "┌───┐");

    // Restore G0 to ASCII: ESC ( B
    term.advance(b"\x1b(B");
    term.advance(b"lqqqk");
    assert_eq!(term.row_text_trimmed(0), "┌───┐lqqqk");
}

#[test]
fn test_dec_special_graphics_full_table() {
    let mut term = TestTerm::new(40, 4);

    // Set G0 to DEC Special Graphics
    term.advance(b"\x1b(0");

    // Line drawing corners and tees
    term.advance(b"jklmn");
    // j -> ┘, k -> ┐, l -> ┌, m -> └, n -> ┼
    assert_eq!(term.row_text_trimmed(0), "┘┐┌└┼");

    // Horizontal & vertical lines and tees
    term.advance(b"\r\n");
    term.advance(b"qtuvwx");
    // q -> ─, t -> ├, u -> ┤, v -> ┴, w -> ┬, x -> │
    assert_eq!(term.row_text_trimmed(1), "─├┤┴┬│");

    // Math symbols and indicators
    term.advance(b"\r\n");
    term.advance(b"afg~yz{|}");
    // a -> ▒, f -> °, g -> ±, ~ -> ·, y -> ≤, z -> ≥, { -> π, | -> ≠, } -> £
    assert_eq!(term.row_text_trimmed(2), "▒°±·≤≥π≠£");

    // Scan lines
    term.advance(b"\r\n");
    term.advance(b"oprs`");
    // o -> ⎺, p -> ⎻, r -> ⎼, s -> ⎽, ` -> ◆
    assert_eq!(term.row_text_trimmed(3), "⎺⎻⎼⎽◆");
}

#[test]
fn test_uk_charset() {
    let mut term = TestTerm::new(30, 2);

    // Designate G0 as UK: ESC ( A
    term.advance(b"\x1b(A");
    term.advance(b"Price: #50");
    // In UK charset, '#' maps to '£'
    assert_eq!(term.row_text_trimmed(0), "Price: £50");

    // Restore to US ASCII: ESC ( B
    term.advance(b"\x1b(B");
    term.advance(b" and #100");
    assert_eq!(term.row_text_trimmed(0), "Price: £50 and #100");
}

#[test]
fn test_tui_box_drawing_layout() {
    let mut term = TestTerm::new(20, 5);

    // Draw a bordered modal dialog box using DEC Special Graphics
    term.advance(b"\x1b(0");
    term.advance(b"lqqqqqqqqqqqqqqqqqqk\r\n");
    term.advance(b"x");
    term.advance(b"\x1b(B"); // Switch to ASCII for label
    term.advance(b"   DIALOG TITLE   ");
    term.advance(b"\x1b(0");
    term.advance(b"x\r\n");
    term.advance(b"tqqqqqqqqqqqqqqqqqqu\r\n");
    term.advance(b"x");
    term.advance(b"\x1b(B");
    term.advance(b" Confirm [Y/n]?   ");
    term.advance(b"\x1b(0");
    term.advance(b"x\r\n");
    term.advance(b"mqqqqqqqqqqqqqqqqqqj");

    assert_eq!(term.row_text_trimmed(0), "┌──────────────────┐");
    assert_eq!(term.row_text_trimmed(1), "│   DIALOG TITLE   │");
    assert_eq!(term.row_text_trimmed(2), "├──────────────────┤");
    assert_eq!(term.row_text_trimmed(3), "│ Confirm [Y/n]?   │");
    assert_eq!(term.row_text_trimmed(4), "└──────────────────┘");
}
