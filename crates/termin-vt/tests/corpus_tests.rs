//! Golden corpus tests for terminal output.
//!
//! Tests realistic terminal streams: vim session, htop, ls --color,
//! progress bars, CJK / emoji, and Claude Code sessions.

use termin_vt::{CellFlags, Color, Effect, EffectSink, ScrollPos, Terminal, TerminalConfig};

struct TestSink {
    pub writes: Vec<u8>,
    pub effects: Vec<Effect>,
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
fn test_corpus_vim_session() {
    let cfg = TerminalConfig {
        cols: 80,
        rows: 24,
        ..Default::default()
    };
    let mut term = Terminal::new(&cfg);
    let mut sink = TestSink {
        writes: Vec::new(),
        effects: Vec::new(),
    };

    // Simulate vim startup:
    // 1. Enter alternate screen with cursor save
    // 2. Set scrolling region
    // 3. Clear screen
    // 4. Draw buffer content (~ for empty lines)
    // 5. Draw status line with inverted style
    // 6. Draw command line
    let mut stream = Vec::new();
    stream.extend_from_slice(b"\x1b[?1049h"); // alt screen
    stream.extend_from_slice(b"\x1b[?1h\x1b="); // DECCKM + keypad
    stream.extend_from_slice(b"\x1b[1;24r"); // DECSTBM
    stream.extend_from_slice(b"\x1b[2J"); // clear screen
    stream.extend_from_slice(b"\x1b[1;1H"); // cursor to 1,1
    stream.extend_from_slice(b"fn main() {\r\n    println!(\"Hello, world!\");\r\n}\r\n");

    // Empty lines with ~
    for row in 4..=22 {
        stream.extend_from_slice(format!("\x1b[{};1H\x1b[34m~\x1b[0m", row).as_bytes());
    }

    // Statusline on row 23 (inverted style)
    stream.extend_from_slice(
        b"\x1b[23;1H\x1b[7m main.rs [+]              utf-8 [unix]   3,2          All \x1b[0m",
    );

    // Commandline on row 24
    stream.extend_from_slice(b"\x1b[24;1H:w");

    term.advance(&stream, &mut sink);

    assert!(term.is_alt_screen());
    let view: Vec<_> = term.view(ScrollPos::Bottom).collect();
    assert_eq!(view.len(), 24);

    // Row 0 has "fn main() {"
    let row0_text: String = view[0]
        .cells
        .iter()
        .take(11)
        .filter_map(|c| char::from_u32(c.ch))
        .collect();
    assert_eq!(row0_text, "fn main() {");

    // Row 22 (0-indexed, 23rd line) has inverted statusline
    let status_cell = &view[22].cells[1];
    let style = term.styles().get(status_cell.style).unwrap();
    assert!(style.attrs.contains(termin_vt::Attrs::INVERSE));

    // Exit vim
    term.advance(b"\x1b[?1049l\x1b[?1l\x1b>", &mut sink);
    assert!(!term.is_alt_screen());
}

#[test]
fn test_corpus_htop_session() {
    let cfg = TerminalConfig {
        cols: 80,
        rows: 24,
        ..Default::default()
    };
    let mut term = Terminal::new(&cfg);
    let mut sink = TestSink {
        writes: Vec::new(),
        effects: Vec::new(),
    };

    let mut stream = Vec::new();
    stream.extend_from_slice(b"\x1b[?1049h\x1b[?25l"); // alt screen + hide cursor
    stream.extend_from_slice(b"\x1b[1;1H");

    // Draw CPU bar: [|||||||||||||||||||         52.3%]
    stream.extend_from_slice(
        b"1  [\x1b[32m|||||||||||\x1b[33m|||||\x1b[31m|||\x1b[0m          52.3%] Tasks: 124\r\n",
    );
    stream.extend_from_slice(
        b"2  [\x1b[32m|||||||\x1b[0m                     24.1%] Load average: 1.85 2.10\r\n",
    );
    stream.extend_from_slice(
        b"Mem[\x1b[32m|||||||||||||||\x1b[0m       4.12G/16.0G] Uptime: 5 days, 03:12:44\r\n",
    );

    // Process table header
    stream.extend_from_slice(b"\x1b[5;1H\x1b[7m  PID USER      PRI  NI  VIRT   RES   SHR S CPU% MEM%   TIME+  Command     \x1b[0m\r\n");
    stream.extend_from_slice(
        b" 1420 user       20   0 14.2G  1.2G  120M S 45.2  7.5  12:34.56 termin      \r\n",
    );
    stream.extend_from_slice(
        b"  892 root       20   0  2.1G  240M   45M S  8.1  1.5   3:12.10 WindowServer\r\n",
    );

    term.advance(&stream, &mut sink);

    let view: Vec<_> = term.view(ScrollPos::Bottom).collect();
    let cpu1_line: String = view[0]
        .cells
        .iter()
        .take(15)
        .filter_map(|c| char::from_u32(c.ch))
        .collect();
    assert!(cpu1_line.starts_with("1  [|||||||||||"));

    // Check header inversion
    let header_cell = &view[4].cells[3];
    let style = term.styles().get(header_cell.style).unwrap();
    assert!(style.attrs.contains(termin_vt::Attrs::INVERSE));
}

#[test]
fn test_corpus_ls_color() {
    let cfg = TerminalConfig {
        cols: 80,
        rows: 24,
        ..Default::default()
    };
    let mut term = Terminal::new(&cfg);
    let mut sink = TestSink {
        writes: Vec::new(),
        effects: Vec::new(),
    };

    // ls -la output with directories in blue, executables in green, symlinks in cyan
    let mut stream = Vec::new();
    stream
        .extend_from_slice(b"drwxr-xr-x   5 user staff   160 Oct 31 12:00 \x1b[1;34m.\x1b[0m\r\n");
    stream
        .extend_from_slice(b"drwxr-xr-x  10 user staff   320 Oct 31 11:00 \x1b[1;34m..\x1b[0m\r\n");
    stream.extend_from_slice(
        b"-rwxr-xr-x   1 user staff 12048 Oct 31 12:01 \x1b[1;32mtermin\x1b[0m\r\n",
    );
    stream.extend_from_slice(b"lrwxr-xr-x   1 user staff     6 Oct 31 12:02 \x1b[1;36mbin\x1b[0m -> \x1b[1;34m.\x1b[0m\r\n");
    stream.extend_from_slice(b"-rw-r--r--   1 user staff  2048 Oct 31 12:03 Cargo.toml\r\n");

    term.advance(&stream, &mut sink);

    let view: Vec<_> = term.view(ScrollPos::Bottom).collect();
    // Verify row 2 has green executable name "termin"
    let termin_row = &view[2];
    // Find the cell where "termin" starts
    let mut green_found = false;
    for cell in termin_row.cells.iter() {
        if cell.ch == 't' as u32 {
            let style = term.styles().get(cell.style).unwrap();
            if style.fg == Color::Indexed(2) && style.attrs.contains(termin_vt::Attrs::BOLD) {
                green_found = true;
                break;
            }
        }
    }
    assert!(
        green_found,
        "expected bold green executable text for termin"
    );
}

#[test]
fn test_corpus_progress_bar_carriage_return() {
    let cfg = TerminalConfig {
        cols: 80,
        rows: 24,
        ..Default::default()
    };
    let mut term = Terminal::new(&cfg);
    let mut sink = TestSink {
        writes: Vec::new(),
        effects: Vec::new(),
    };

    // Progress updates with '\r' (overwriting in place without scrolling)
    term.advance(b"Downloading: [          ] 0%\r", &mut sink);
    term.advance(b"Downloading: [====      ] 40%\r", &mut sink);
    term.advance(b"Downloading: [==========] 100%\r\nDone!\r\n", &mut sink);

    let view: Vec<_> = term.view(ScrollPos::Bottom).collect();
    let row0: String = view[0]
        .cells
        .iter()
        .take(30)
        .filter_map(|c| char::from_u32(c.ch))
        .collect();
    assert_eq!(row0, "Downloading: [==========] 100%");

    let row1: String = view[1]
        .cells
        .iter()
        .take(5)
        .filter_map(|c| char::from_u32(c.ch))
        .collect();
    assert_eq!(row1, "Done!");
}

#[test]
fn test_corpus_cjk_and_emoji() {
    let cfg = TerminalConfig {
        cols: 80,
        rows: 24,
        ..Default::default()
    };
    let mut term = Terminal::new(&cfg);
    let mut sink = TestSink {
        writes: Vec::new(),
        effects: Vec::new(),
    };

    // "Hello 世界 🦀\r\n"
    term.advance("Hello 世界 🦀\r\n".as_bytes(), &mut sink);

    let view: Vec<_> = term.view(ScrollPos::Bottom).collect();
    let row = &view[0];

    // 'H', 'e', 'l', 'l', 'o', ' ' -> cols 0..5
    assert_eq!(row.cells[0].ch, 'H' as u32);
    assert_eq!(row.cells[5].ch, ' ' as u32);

    // '世' (CJK wide) at col 6, spacer at col 7
    assert_eq!(row.cells[6].ch, '世' as u32);
    assert!(row.cells[6].flags.contains(CellFlags::WIDE));
    assert!(row.cells[7].flags.contains(CellFlags::WIDE_SPACER));

    // '界' (CJK wide) at col 8, spacer at col 9
    assert_eq!(row.cells[8].ch, '界' as u32);
    assert!(row.cells[8].flags.contains(CellFlags::WIDE));
    assert!(row.cells[9].flags.contains(CellFlags::WIDE_SPACER));

    // ' ' at col 10
    assert_eq!(row.cells[10].ch, ' ' as u32);

    // '🦀' (emoji wide) at col 11, spacer at col 12
    assert_eq!(row.cells[11].ch, '🦀' as u32);
    assert!(row.cells[11].flags.contains(CellFlags::WIDE));
    assert!(row.cells[12].flags.contains(CellFlags::WIDE_SPACER));
}

#[test]
fn test_corpus_claude_code_session() {
    let cfg = TerminalConfig {
        cols: 80,
        rows: 24,
        ..Default::default()
    };
    let mut term = Terminal::new(&cfg);
    let mut sink = TestSink {
        writes: Vec::new(),
        effects: Vec::new(),
    };

    let mut stream = Vec::new();

    // 1. CWD update via OSC 7
    stream.extend_from_slice(b"\x1b]7;file://macbook/Users/ronaldcekani/termin\x1b\\");

    // 2. OSC 133;A (Prompt start)
    stream.extend_from_slice(b"\x1b]133;A\x1b\\");
    stream.extend_from_slice(b"\x1b[36mtermin\x1b[0m $ ");

    // 3. OSC 133;B (Input start)
    stream.extend_from_slice(b"\x1b]133;B\x1b\\");
    stream.extend_from_slice(b"cargo build\r\n");

    // 4. OSC 133;C (Command execution start)
    stream.extend_from_slice(b"\x1b]133;C;cmdline_url=cargo%20build\x1b\\");

    // 5. Synchronized output start (?2026h)
    stream.extend_from_slice(b"\x1b[?2026h");
    stream.extend_from_slice(b"   Compiling termin-vt v0.1.0\r\n");
    stream.extend_from_slice(b"    Finished dev [unoptimized + debuginfo] in 0.52s\r\n");
    // Synchronized output end (?2026l)
    stream.extend_from_slice(b"\x1b[?2026l");

    // 6. OSC 133;D (Command finish with exit code 0)
    stream.extend_from_slice(b"\x1b]133;D;0\x1b\\");

    term.advance(&stream, &mut sink);

    assert_eq!(term.blocks().len(), 1);
    let block = term.blocks().iter().next().unwrap();
    assert_eq!(block.exit, Some(0));
    assert_eq!(block.command.as_deref(), Some("cargo build"));
    assert_eq!(block.cwd.as_deref(), Some("/Users/ronaldcekani/termin"));
}
