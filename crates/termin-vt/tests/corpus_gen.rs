//! Generates recorded corpus files for golden and benchmark testing.

use std::fs;
use std::path::Path;

pub fn generate_all_corpora(dir: &Path) {
    fs::create_dir_all(dir).unwrap();

    // 1. ASCII corpus
    let mut ascii = Vec::new();
    for i in 0..1000 {
        ascii.extend_from_slice(
            format!(
                "Line {:04}: The quick brown fox jumps over the lazy dog. 1234567890\r\n",
                i
            )
            .as_bytes(),
        );
    }
    fs::write(dir.join("ascii.bin"), ascii).unwrap();

    // 2. SGR corpus
    let mut sgr = Vec::new();
    for i in 0..1000 {
        let fg = 31 + (i % 7);
        let bg = 41 + ((i + 1) % 7);
        sgr.extend_from_slice(
            format!(
                "\x1b[{};{}mColored line {:04}\x1b[0m with \x1b[1;4mbold underline\x1b[0m\r\n",
                fg, bg, i
            )
            .as_bytes(),
        );
    }
    fs::write(dir.join("sgr.bin"), sgr).unwrap();

    // 3. Unicode corpus (CJK + Emoji + Box drawing)
    let mut unicode = Vec::new();
    for i in 0..500 {
        unicode.extend_from_slice(
            format!(
                "Row {:03}: ┌───┐ 漢字・カタカナ・ひらがな 🦀 🚀 💻 └───┘\r\n",
                i
            )
            .as_bytes(),
        );
    }
    fs::write(dir.join("unicode.bin"), unicode).unwrap();

    // 4. Scroll corpus
    let mut scroll = Vec::new();
    for i in 0..2000 {
        scroll.extend_from_slice(
            format!("Scroll row {:05}: text text text text text\r\n", i).as_bytes(),
        );
    }
    fs::write(dir.join("scroll.bin"), scroll).unwrap();

    // 5. Claude code corpus
    let mut claude = Vec::new();
    claude.extend_from_slice(b"\x1b]7;file://host/Users/test/termin\x1b\\");
    for i in 0..50 {
        claude.extend_from_slice(b"\x1b]133;A\x1b\\\x1b[32m$\x1b[0m ");
        claude.extend_from_slice(format!("\x1b]133;B\x1b\\cargo test --run {}\r\n", i).as_bytes());
        claude.extend_from_slice(b"\x1b]133;C;cmdline_url=cargo%20test\x1b\\");
        claude.extend_from_slice(b"\x1b[?2026h");
        claude.extend_from_slice(b"   Compiling crate v0.1.0\r\n");
        claude.extend_from_slice(b"    Finished test [unoptimized] in 0.12s\r\n");
        claude.extend_from_slice(b"     Running unittests\r\n");
        claude.extend_from_slice(b"\x1b[?2026l");
        claude.extend_from_slice(b"\x1b]133;D;0\x1b\\");
    }
    fs::write(dir.join("claude_code.bin"), claude).unwrap();
}
