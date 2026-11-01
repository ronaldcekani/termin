//! Conformance tests for TUI application session replay.
//!
//! Simulates realistic escape sequence traces and idioms used by:
//! - vim / nvim (alternate screen, scroll region status line, DECSCUSR cursor shapes, RGB colors)
//! - tmux / screen (DECSTBM partial scroll regions, pane splitting, reverse video)
//! - htop / btop (braille meters, box-drawing characters, RGB usage bars, rapid in-place updates)
//! - less / man / git (overstrike text, alternate screen pager switching, status bar redraw)
//! - fzf / lazygit (interactive filtering, cursor save/restore CSI s/u, mouse selection)
//! - Claude Code / Codex CLI (synchronized output ?2026h/?2026l, kitty keyboard flags, OSC 9 alerts, OSC 133)

use super::helper::TestTerm;
use termin_vt::{Attrs, Color, CursorShape, Effect};

#[test]
fn test_tui_replay_vim_nvim_session() {
    let mut term = TestTerm::new(80, 24);

    // 1. Vim startup: Switch to alternate screen, steady block cursor
    term.advance(b"\x1b[?1049h\x1b[2 q");
    assert!(term.is_alt_screen());
    assert_eq!(term.term.cursor().shape, CursorShape::Block);

    // 2. Set scroll region for text editing (lines 1..23), leaving line 24 for statusline
    term.advance(b"\x1b[1;23r");

    // 3. Draw statusline on row 24 with inverse video and bold
    term.advance(b"\x1b[24;1H\x1b[7;1m[No Name] - NORMAL\x1b[0m");
    assert_eq!(&term.row_text_trimmed(23)[0..18], "[No Name] - NORMAL");

    // 4. Enter INSERT mode: change cursor to blinking bar (DECSCUSR 5)
    term.advance(b"\x1b[5 q");
    assert_eq!(term.term.cursor().shape, CursorShape::Bar);

    // 5. Type code with 24-bit TrueColor syntax highlighting
    term.advance(b"\x1b[1;1H\x1b[38;2;255;100;50mfn \x1b[38;2;100;200;255mmain\x1b[0m() {\r\n");
    term.advance(b"    \x1b[38;2;120;220;100mprintln!\x1b[0m(\"Hello Termin\");\r\n");
    term.advance(b"}\r\n");

    assert_eq!(term.row_text_trimmed(0), "fn main() {");
    assert_eq!(term.row_text_trimmed(1), "    println!(\"Hello Termin\");");
    assert_eq!(term.row_text_trimmed(2), "}");

    // Verify RGB color on 'fn' keyword
    let fn_style = term.style_at(0, 0).expect("style");
    assert_eq!(fn_style.fg, Color::Rgb(255, 100, 50));

    // 6. Return to NORMAL mode and exit vim: cursor block, exit alt screen
    term.advance(b"\x1b[2 q\x1b[?1049l");
    assert!(!term.is_alt_screen());
}

#[test]
fn test_tui_replay_tmux_screen_session() {
    let mut term = TestTerm::new(80, 24);

    // 1. Tmux initializes: set title, switch to alt screen, draw bottom status bar
    term.advance(b"\x1b]2;tmux - 0:zsh*\x1b\\\x1b[?1049h");
    assert!(term.is_alt_screen());
    assert!(term.sink.effects.iter().any(|e| match e {
        Effect::Title(t) => t.as_ref() == "tmux - 0:zsh*",
        _ => false,
    }));

    // Status bar at line 24 with green background
    term.advance(b"\x1b[24;1H\x1b[42;30m[0] 0:zsh* 1:bash-                             \"termin\" 12:00\x1b[0m");
    assert!(term.row_text_trimmed(23).contains("[0] 0:zsh*"));

    // 2. Set scroll region for top pane (rows 1..=23)
    term.advance(b"\x1b[1;23r");

    // 3. Fill lines and scroll inside the top pane
    for i in 1..=25 {
        term.advance(format!("Pane line {i}\r\n").as_bytes());
    }

    // Status line on row 24 is preserved!
    assert!(term.row_text_trimmed(23).contains("[0] 0:zsh*"));
    assert_eq!(term.style_at(23, 0).unwrap().bg, Color::Indexed(2));
}

#[test]
fn test_tui_replay_htop_btop_session() {
    let mut term = TestTerm::new(80, 24);

    // 1. Enter alt screen, hide cursor
    term.advance(b"\x1b[?1049h\x1b[?25l");
    assert!(!term.term.cursor().visible);

    // 2. Draw CPU and RAM bar gauges using braille and block characters
    // '⠋', '⠙', '█', '▌'
    term.advance("\x1b[1;1H\x1b[1;32m1  [████████████████████                    50.0%] Tasks: 124, 521 thr\x1b[0m\r\n".as_bytes());
    term.advance("\x1b[1;36m2  [██████████                              25.0%] Load average: 1.25\x1b[0m\r\n".as_bytes());
    term.advance("\x1b[1;33mMem[████████████████████████████████       12.4G/32.0G]\x1b[0m\r\n".as_bytes());

    assert!(term.row_text_trimmed(0).contains("50.0%"));
    assert!(term.row_text_trimmed(1).contains("25.0%"));
    assert!(term.row_text_trimmed(2).contains("12.4G/32.0G"));

    // 3. Draw process table headers with reverse video
    term.advance(b"\x1b[5;1H\x1b[7m  PID USER      PRI  NI  VIRT   RES   SHR S CPU% MEM%   TIME+  Command     \x1b[0m\r\n");
    term.advance(b" 1234 developer  20   0 12.5G  1.2G  200M S 15.2  3.8  2:45.12 termin      \r\n");

    let header_style = term.style_at(4, 0).expect("style");
    assert!(header_style.attrs.contains(Attrs::INVERSE));
    assert!(term.row_text_trimmed(5).contains("termin"));

    // 4. Update in place without screen flicker: position at (1, 1) and rewrite row 1
    term.advance("\x1b[1;1H\x1b[1;32m1  [████████████████████████████            70.0%] Tasks: 125, 523 thr\x1b[0m".as_bytes());
    assert!(term.row_text_trimmed(0).contains("70.0%"));
}

#[test]
fn test_tui_replay_less_man_git_pager() {
    let mut term = TestTerm::new(80, 24);

    // 1. Less starts: alt screen (?1049h)
    term.advance(b"\x1b[?1049h");

    // 2. Render manpage / git log with overstrike formatting and bold/underline
    term.advance(b"\x1b[1mNAME\x1b[0m\r\n");
    term.advance(b"       termin - terminal emulator\r\n\r\n");
    term.advance(b"\x1b[1mDESCRIPTION\x1b[0m\r\n");
    term.advance(b"       \x1b[4mtermin\x1b[0m is a GPU-accelerated terminal for macOS.\r\n");

    assert_eq!(term.row_text_trimmed(0), "NAME");
    assert_eq!(term.row_text_trimmed(1), "       termin - terminal emulator");
    assert_eq!(term.row_text_trimmed(3), "DESCRIPTION");

    let name_style = term.style_at(0, 0).expect("style");
    assert!(name_style.attrs.contains(Attrs::BOLD));

    let desc_style = term.style_at(4, 7).expect("style");
    assert!(desc_style.attrs.contains(Attrs::UNDERLINE));

    // 3. Status prompt at row 24 with inverse video
    term.advance(b"\x1b[24;1H\x1b[7m: \x1b[0m");
    assert_eq!(term.row_text_trimmed(23), ":");

    // 4. Exit pager
    term.advance(b"\x1b[?1049l");
    assert!(!term.is_alt_screen());
}

#[test]
fn test_tui_replay_fzf_lazygit_session() {
    let mut term = TestTerm::new(80, 10);

    // Fzf quick interface: save cursor with CSI s, draw search prompt
    term.advance(b"\x1b[s");
    term.advance(b"\x1b[1;1H\x1b[2K> my_search_query\r\n");
    term.advance(b"  100/100\r\n");
    term.advance(b"\x1b[7m> src/main.rs\x1b[0m\r\n");
    term.advance(b"  src/lib.rs\r\n");
    term.advance(b"  src/parser.rs\r\n");

    assert_eq!(term.row_text_trimmed(0), "> my_search_query");
    assert_eq!(term.row_text_trimmed(2), "> src/main.rs");
    assert!(term.style_at(2, 0).unwrap().attrs.contains(Attrs::INVERSE));

    // Restore cursor with CSI u
    term.advance(b"\x1b[u");
}

#[test]
fn test_tui_replay_claude_code_codex_cli_session() {
    let mut term = TestTerm::new(80, 24);

    // 1. Shell integration starts prompt
    term.advance(b"\x1b]133;A\x1b\\");

    // 2. Claude Code pushes Kitty keyboard flags to disambiguate escape keys
    term.advance(b"\x1b[>1u");

    // 3. User runs claude: prompt end, command output start
    term.advance(b"\x1b]133;B\x1b\\claude\r\n\x1b]133;C;cmdline_url=claude\x1b\\");

    // 4. Claude Code begins synchronized frame output: ?2026h
    term.advance(b"\x1b[?2026h");
    term.advance(b"\x1b[1;1H\x1b[1;35mClaude Code\x1b[0m v0.2.29\r\n");
    term.advance(b"Thinking...\r\n");
    // End synchronized frame output: ?2026l
    term.advance(b"\x1b[?2026l");

    assert_eq!(term.row_text_trimmed(0), "Claude Code v0.2.29");
    assert_eq!(term.row_text_trimmed(1), "Thinking...");

    // 5. Claude requests user input, emitting desktop notification via OSC 9
    term.advance(b"\x1b]9;Claude is waiting for user confirmation\x1b\\");
    assert!(term.sink.effects.iter().any(|e| match e {
        Effect::Notify { body, .. } => body.as_ref() == "Claude is waiting for user confirmation",
        _ => false,
    }));

    // 6. Claude completes: pop kitty keyboard flags, finish command block
    term.advance(b"\x1b[<u\x1b]133;D;0\x1b\\");
}
