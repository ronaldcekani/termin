//! Deterministic corpus generator (`cargo xtask corpus`).
//!
//! Generates benchmark and test corpora:
//! - `ascii.bin`: dense ASCII text (parser throughput target >= 400 MB/s)
//! - `sgr.bin`: ANSI color sequences, 256-color, truecolor (throughput target >= 150 MB/s)
//! - `unicode.bin`: CJK wide chars, emojis, box drawing, combining chars
//! - `scroll.bin`: high-speed scrolling eviction into scrollback
//! - `claude-code.bin`: realistic Claude Code session with OSC 133, OSC 7, diffs
//!
//! See `docs/specs/performance.md §4`.

use std::fs;
use std::io;
use std::path::Path;

use crate::project_root;

pub fn run(args: &[String]) {
    let root = project_root();
    let target_dir = root.join("target").join("corpus");
    let test_dir = root
        .join("crates")
        .join("termin-vt")
        .join("tests")
        .join("corpus");

    println!("================================================================================");
    println!("                         Termin Corpus Generator                                ");
    println!("================================================================================");

    let quiet = args.iter().any(|a| a == "--quiet" || a == "-q");

    if let Err(e) = generate_all_corpora(&target_dir, &test_dir, quiet) {
        eprintln!("Error generating corpus: {}", e);
        std::process::exit(1);
    }
}

pub fn generate_all_corpora(
    target_dir: &Path,
    test_dir: &Path,
    quiet: bool,
) -> io::Result<CorpusStats> {
    fs::create_dir_all(target_dir)?;
    let _ = fs::create_dir_all(test_dir);

    let mut stats = CorpusStats::default();

    // 1. ASCII corpus (~8.5 MB, 100,000 lines)
    let (ascii_bytes, ascii_lines) = generate_ascii(100_000);
    write_corpus(target_dir, test_dir, "ascii.bin", &ascii_bytes)?;
    stats.ascii_bytes = ascii_bytes.len();
    stats.ascii_lines = ascii_lines;

    // 2. SGR corpus (~5.2 MB, 50,000 lines)
    let (sgr_bytes, sgr_lines) = generate_sgr(50_000);
    write_corpus(target_dir, test_dir, "sgr.bin", &sgr_bytes)?;
    stats.sgr_bytes = sgr_bytes.len();
    stats.sgr_lines = sgr_lines;

    // 3. Unicode corpus (~3.5 MB, 30,000 lines)
    let (unicode_bytes, unicode_lines) = generate_unicode(30_000);
    write_corpus(target_dir, test_dir, "unicode.bin", &unicode_bytes)?;
    stats.unicode_bytes = unicode_bytes.len();
    stats.unicode_lines = unicode_lines;

    // 4. Scroll corpus (~5.5 MB, 80,000 lines)
    let (scroll_bytes, scroll_lines) = generate_scroll(80_000);
    write_corpus(target_dir, test_dir, "scroll.bin", &scroll_bytes)?;
    stats.scroll_bytes = scroll_bytes.len();
    stats.scroll_lines = scroll_lines;

    // 5. Claude code corpus (~2.4 MB, 20,000 lines)
    let (claude_bytes, claude_lines) = generate_claude_code(20_000);
    // Write both claude-code.bin and claude_code.bin for compatibility
    write_corpus(target_dir, test_dir, "claude-code.bin", &claude_bytes)?;
    write_corpus(target_dir, test_dir, "claude_code.bin", &claude_bytes)?;
    stats.claude_bytes = claude_bytes.len();
    stats.claude_lines = claude_lines;

    if !quiet {
        print_summary(&stats, target_dir);
    }

    Ok(stats)
}

#[derive(Default, Debug, Clone)]
pub struct CorpusStats {
    pub ascii_bytes: usize,
    pub ascii_lines: usize,
    pub sgr_bytes: usize,
    pub sgr_lines: usize,
    pub unicode_bytes: usize,
    pub unicode_lines: usize,
    pub scroll_bytes: usize,
    pub scroll_lines: usize,
    pub claude_bytes: usize,
    pub claude_lines: usize,
}

impl CorpusStats {
    pub fn total_bytes(&self) -> usize {
        self.ascii_bytes
            + self.sgr_bytes
            + self.unicode_bytes
            + self.scroll_bytes
            + self.claude_bytes
    }
}

fn write_corpus(
    target_dir: &Path,
    test_dir: &Path,
    filename: &str,
    data: &[u8],
) -> io::Result<()> {
    let p1 = target_dir.join(filename);
    fs::write(&p1, data)?;
    if test_dir.exists() {
        let p2 = test_dir.join(filename);
        let _ = fs::write(p2, data);
    }
    Ok(())
}

fn print_summary(stats: &CorpusStats, dir: &Path) {
    println!(
        "{:<20} {:>12} {:>14} {:>10}",
        "Corpus File", "Lines", "Size (bytes)", "Size (MiB)"
    );
    println!("--------------------------------------------------------------------------------");

    let print_row = |name: &str, lines: usize, bytes: usize| {
        let mib = bytes as f64 / (1024.0 * 1024.0);
        println!("{:<20} {:>12} {:>14} {:>9.2} MiB", name, lines, bytes, mib);
    };

    print_row("ascii.bin", stats.ascii_lines, stats.ascii_bytes);
    print_row("sgr.bin", stats.sgr_lines, stats.sgr_bytes);
    print_row("unicode.bin", stats.unicode_lines, stats.unicode_bytes);
    print_row("scroll.bin", stats.scroll_lines, stats.scroll_bytes);
    print_row("claude-code.bin", stats.claude_lines, stats.claude_bytes);

    println!("--------------------------------------------------------------------------------");
    let total_mib = stats.total_bytes() as f64 / (1024.0 * 1024.0);
    println!(
        "{:<20} {:>12} {:>14} {:>9.2} MiB",
        "Total",
        stats.ascii_lines
            + stats.sgr_lines
            + stats.unicode_lines
            + stats.scroll_lines
            + stats.claude_lines,
        stats.total_bytes(),
        total_mib
    );
    println!("Target directory: {}", dir.display());
    println!("================================================================================");
}

/// Generates pure ASCII log lines: typical build or server logs.
fn generate_ascii(lines: usize) -> (Vec<u8>, usize) {
    let mut out = Vec::with_capacity(lines * 85);
    for i in 0..lines {
        use std::fmt::Write;
        let mut buf = String::with_capacity(100);
        let sample = match i % 5 {
            0 => "Compiling termin-core v0.1.0 (/Users/termin/crates/termin-core)",
            1 => "Running unittests src/lib.rs (target/release/deps/termin_core-hash)",
            2 => "Checking dependencies and linting rules across workspace targets",
            3 => "Optimizing LTO codegen units and performing dead code elimination",
            _ => "Finished release [optimized] target(s) in 0.42s; test execution passed",
        };
        let _ = write!(buf, "[{:06}] {}: line payload checksum {:08x}\r\n", i, sample, (i * 2654435761) & 0xFFFFFFFF);
        out.extend_from_slice(buf.as_bytes());
    }
    (out, lines)
}

/// Generates ANSI SGR colored text (standard 16, 256 colors, truecolor, bold, underline, italic).
fn generate_sgr(lines: usize) -> (Vec<u8>, usize) {
    let mut out = Vec::with_capacity(lines * 105);
    for i in 0..lines {
        use std::fmt::Write;
        let mut buf = String::with_capacity(128);
        match i % 4 {
            0 => {
                // Standard 16 colors + bold
                let fg = 31 + (i % 7);
                let _ = write!(buf, "\x1b[1;{}m[WARN]\x1b[0m module_{:04} failed check: \x1b[4;33mretry count={}\x1b[0m\r\n", fg, i % 100, i % 5);
            }
            1 => {
                // 256 colors
                let c1 = (i * 7) % 256;
                let c2 = (i * 13) % 256;
                let _ = write!(buf, "\x1b[38;5;{}m256-color {:03}\x1b[0m \x1b[48;5;{}m bg-sample \x1b[0m line {:06}\r\n", c1, c1, c2, i);
            }
            2 => {
                // 24-bit Truecolor RGB
                let r = ((i * 11) % 256) as u8;
                let g = ((i * 23) % 256) as u8;
                let b = ((i * 37) % 256) as u8;
                let _ = write!(buf, "\x1b[38;2;{};{};{}mRGB({:03},{:03},{:03})\x1b[0m gradient step {:06} \x1b[3mitalic note\x1b[0m\r\n", r, g, b, r, g, b, i);
            }
            _ => {
                // Mixed attributes
                let _ = write!(buf, "\x1b[1;32mSUCCESS\x1b[0m {:06}: \x1b[7m INVERSE \x1b[27m \x1b[9mstrikethrough\x1b[29m \x1b[36mpath/to/file.rs\x1b[0m\r\n", i);
            }
        }
        out.extend_from_slice(buf.as_bytes());
    }
    (out, lines)
}

/// Generates Unicode text: CJK wide characters, emoji, box drawing, combining accents.
fn generate_unicode(lines: usize) -> (Vec<u8>, usize) {
    let mut out = Vec::with_capacity(lines * 115);
    for i in 0..lines {
        use std::fmt::Write;
        let mut buf = String::with_capacity(140);
        match i % 4 {
            0 => {
                // CJK full-width
                let _ = write!(buf, "Row {:05}: ┌─────────┐ 【日本語・中文・한국어】 格子端末 └─────────┘\r\n", i);
            }
            1 => {
                // Box drawing + symbols
                let _ = write!(buf, "Tree {:05}: ├── 📦 cargo-tree ├── 🦀 rustc v1.75 └── ⚡ lightning fast\r\n", i);
            }
            2 => {
                // Emoji and ZWJ / modifiers
                let _ = write!(buf, "Status {:05}: 🚀 Deploying 🧑‍💻 engineer 💡 spark ✨ magic 🎯 target\r\n", i);
            }
            _ => {
                // Combining characters & accents
                let _ = write!(buf, "Math {:05}: e^{{iπ}} + 1 = 0 · ∀x∈ℝ · ça va bien · na\u{0308}ve test\r\n", i);
            }
        }
        out.extend_from_slice(buf.as_bytes());
    }
    (out, lines)
}

/// Generates rapid scrolling text to exercise scrollback ring buffer and cold tier eviction.
fn generate_scroll(lines: usize) -> (Vec<u8>, usize) {
    let mut out = Vec::with_capacity(lines * 70);
    for i in 0..lines {
        use std::fmt::Write;
        let mut buf = String::with_capacity(80);
        let _ = write!(buf, "scroll stream row {:06} | memory buffer test offset 0x{:08X}\r\n", i, i * 64);
        out.extend_from_slice(buf.as_bytes());
    }
    (out, lines)
}

/// Generates realistic Claude Code session: OSC 7 cwd, OSC 133 semantic markers, diffs, progress.
fn generate_claude_code(lines: usize) -> (Vec<u8>, usize) {
    let mut out = Vec::with_capacity(lines * 120);

    // Initial OSC 7 cwd report
    out.extend_from_slice(b"\x1b]7;file://localhost/Users/termin/workspace\x1b\\");

    let num_sessions = (lines / 40).max(1);
    let mut current_line = 0;

    for s in 0..num_sessions {
        use std::fmt::Write;
        let mut buf = String::with_capacity(256);

        // Prompt marker (OSC 133;A)
        let _ = write!(buf, "\x1b]133;A\x1b\\\x1b[32mtermin\x1b[0m:\x1b[34m~/workspace\x1b[0m$ ");
        // Command line marker (OSC 133;B)
        let _ = write!(buf, "\x1b]133;B\x1b\\cargo build --release -p termin-vt #{}\r\n", s);
        // Command executed marker (OSC 133;C)
        let _ = write!(buf, "\x1b]133;C;cmdline_url=cargo%20build\x1b\\");
        // Synchronized output start
        let _ = write!(buf, "\x1b[?2026h");
        out.extend_from_slice(buf.as_bytes());
        current_line += 2;

        // Simulated compilation output and diff
        for step_idx in 0..10 {
            let mut step_buf = String::with_capacity(128);
            let _ = write!(step_buf, "   \x1b[1;32mCompiling\x1b[0m subcrate_{} v0.1.0 (/path/to/crates/sub_{})\r\n", step_idx, step_idx);
            out.extend_from_slice(step_buf.as_bytes());
            current_line += 1;
        }

        // Git diff chunk in Claude Code card
        let diff = "\
\x1b[1;37mdiff --git a/src/lib.rs b/src/lib.rs\x1b[0m\r\n\
\x1b[36m@@ -42,6 +42,7 @@\x1b[0m\r\n\
\x1b[31m-    let old_value = calculate_slow();\x1b[0m\r\n\
\x1b[32m+    let new_value = calculate_zero_alloc();\x1b[0m\r\n\
\x1b[32m+    renderer.mark_dirty();\x1b[0m\r\n";
        out.extend_from_slice(diff.as_bytes());
        current_line += 5;

        // Progress spinner and Claude Code card status
        let status = "\
\x1b[1;35m● Claude Code\x1b[0m: \x1b[33mSynthesizing solution across 12 files...\x1b[0m\r\n\
\x1b[2K\r\x1b[1;35m● Claude Code\x1b[0m: \x1b[32mAll tests and benchmarks passed (400 MB/s)\x1b[0m\r\n";
        out.extend_from_slice(status.as_bytes());
        current_line += 2;

        // End synchronized output & Command finished marker (OSC 133;D;0)
        let end = "\x1b[?2026l\x1b]133;D;0\x1b\\\r\n";
        out.extend_from_slice(end.as_bytes());
        current_line += 1;

        if current_line >= lines {
            break;
        }
    }

    (out, current_line)
}
