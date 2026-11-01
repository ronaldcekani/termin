# Termin v1.0 — Terminal Compatibility Matrix (Track 4D)

This document records the VT conformance and application compatibility verification for **Termin v1.0** (`docs/implementation-phases.md §Phase 4 — 4D`, `docs/specs/terminal-core.md`).

All 18 target terminal applications and workflows have been verified against `termin-vt` via the `vttest`/`esctest`-derived conformance test suite (`crates/termin-vt/tests/conformance_tests.rs`) and corpus replay fixtures (`crates/termin-vt/tests/corpus_replay_tests.rs`).

---

## 1. Conformance Suite Summary (`vttest` / `esctest`)

| Suite Area | Test Module | Coverage | Status |
|---|---|---|---|
| **vttest Menu 1: Cursor Movements** | `tests/conformance/cursor.rs` | `CUP`, `HVP`, `CUU`, `CUD`, `CUF`, `CUB`, `CNL`, `CPL`, `CHA`, `VPA`, `HPA`, `VPR`, `HPR`, `DECOM` (Origin Mode), `DECAWM` (Autowrap + pending wrap), `IND`, `RI`, `NEL`, `DECSC`/`DECRC` | **PASS** |
| **vttest Menu 2: Screen Features** | `tests/conformance/screen.rs` | `ED` (0/1/2/3), `EL` (0/1/2), `IL`, `DL`, `ICH`, `DCH`, `ECH`, `SU`, `SD`, `DECSTBM` scroll regions, `BCE` (Background Color Erase), `HT`, `HTS`, `TBC` (0/3), `CHT`, `CBT` | **PASS** |
| **vttest Menu 3: Character Sets** | `tests/conformance/charsets.rs` | `G0`/`G1` designation (`ESC ( 0`, `ESC ( B`, `ESC ) 0`, `ESC ) B`, `ESC ( A`), `SO` (`0x0E`) / `SI` (`0x0F`), full DEC Special Graphics box-drawing table | **PASS** |
| **esctest: Modes & Device Reports** | `tests/conformance/modes_and_reports.rs` | `DECCKM` (`?1`), `DECKPAM`/`DECKPNM`, Alt Screen (`?1049`, `?1047`, `?47`), Bracketed Paste (`?2004`), Focus Events (`?1004`), Synchronized Output (`?2026`), Mouse (`?1000`, `?1002`, `?1003`, `?1006` SGR), Kitty Keyboard Protocol (`CSI > flags u`, `CSI < u`, `CSI ? u`), `DECSCUSR`, `DSR` (`5n`/`6n`), `DA1`, `DA2`, `XTVERSION`, `DECRQM` | **PASS** |
| **OSC & Shell Integration** | `tests/conformance/osc_and_integration.rs` | `OSC 0`/`2` (Title), `OSC 7` (CWD), `OSC 8` (Hyperlinks), `OSC 9`/`99`/`777` (Notifications), `OSC 4`/`10`/`11` (Color Queries), `OSC 52` (Clipboard), `OSC 133` (`A`/`B`/`C`/`D` FinalTerm/Warp blocks), 64 KiB payload cap | **PASS** |
| **TUI Application Replays** | `tests/conformance/tui_replay.rs` | Full-screen and inline TUI session replays across all 18 target applications | **PASS** |

---

## 2. Target Application Compatibility Matrix

| # | Application | Primary Escape Sequences & Features Exercised | Conformance Test Coverage | Status | Accepted v1 Limitations / Notes |
|---|---|---|---|---|---|
| 1 | **`vim`** | `?1049h`/`?1049l` (alt screen), `DECSTBM` (statusline preserve), `DECSCUSR` (`2 q` block / `6 q` bar cursor), `BCE`, `?1002h` + `?1006h` mouse, 256-color & 24-bit SGR | `test_tui_replay_vim_nvim_session`, `test_alt_screen_modes`, `test_decscusr_cursor_shapes` | **PASS** | Automatically switches tab to Classic mode while `?1049h` is active and returns to Block mode on `?1049l` (`D15`). |
| 2 | **`nvim`** | `?1049h`, `CSI > 1 u` (Kitty keyboard protocol), `?2026h`/`?2026l` (synchronized updates), `DECSCUSR`, `OSC 52` yank clipboard, truecolor SGR (`38;2;r;g;b`), undercurl (`4:3m`) | `test_tui_replay_vim_nvim_session`, `test_kitty_keyboard_protocol_stack`, `test_osc_52_clipboard` | **PASS** | Full truecolor, undercurl, and Kitty keyboard disambiguation supported. |
| 3 | **`emacs -nw`** | `?1049h`, `DECSTBM` split windows, `IL`/`DL`/`ICH`/`DCH`, `DECCKM`, `modifyOtherKeys` / Kitty keys, `ALT` (`Option` as Meta/Alt) | `test_il_and_dl_respect_scroll_margins`, `test_ich_dch_ech_character_operations` | **PASS** | `option_as_alt = "both"` (or `"left"`) in `config.toml` sends `ESC`-prefixed Meta chords cleanly. |
| 4 | **`tmux`** | `DECSTBM`, `OSC 0`/`2` pane title propagation, `ESC ( 0` box-drawing borders, `?1000h`/`?1002h`/`?1006h` mouse passthrough, `OSC 52` clipboard, `DA1`/`DA2`/`XTVERSION` | `test_tui_replay_tmux_screen_session`, `test_g0_g1_designation_and_so_si` | **PASS** | Standard terminal multiplexer mode works seamlessly. `tmux -CC` control mode is out of scope for v1 (`docs/README.md`). |
| 5 | **`screen`** | `?47h`/`?1047h` alt screen, `DECSTBM`, `DECSC`/`DECRC`, `RI` (reverse index at top margin), `ESC ( 0` line graphics, `BCE` | `test_tui_replay_tmux_screen_session`, `test_alt_screen_modes`, `test_ind_ri_nel` | **PASS** | Legacy `?47h` and `?1047h` alternate screen modes fully supported. |
| 6 | **`htop`** | `?1049h`, `DECTCEM` (`?25l` hidden cursor), `BCE` meter bars, `CHA`/`CUP`/`EL`, `?1000h` mouse clicks on column headers | `test_tui_replay_htop_btop_session`, `test_bce_background_color_erase` | **PASS** | Procedural box-drawing and block glyphs (`U+2580`–`U+259F`) render crisp meter bars at any DPI. |
| 7 | **`btop`** | `?1049h`, `?2026h` sync output, 24-bit RGB gradients (`38;2;...`), Unicode Braille patterns (`U+2800`–`U+28FF`), box drawing (`U+2500`–`U+257F`), `?1003h` any-event mouse | `test_tui_replay_htop_btop_session`, `test_tui_box_drawing_layout` | **PASS** | Procedural braille + box-drawing glyphs in `termin-text` ensure pixel-aligned graphs without font gaps. |
| 8 | **`less`** | `?1049h`, `SGR 1`/`4`/`7` (bold/underline/inverse search highlights), `RI` scroll back one line, `ED`/`EL`, backspace overstrike (`c\x08c` bold, `_\x08c` underline) | `test_tui_replay_less_man_git_pager`, `test_ind_ri_nel` | **PASS** | Mouse wheel automatically translates to arrow keys on alt screen when mouse mode is off (`scroll.alt_screen_wheel_lines`). |
| 9 | **`man`** | `less` pager pipeline, backspace overstrike sequences, SGR bold/italic/underline formatting, wide Unicode tables | `test_tui_replay_less_man_git_pager` | **PASS** | Clean return to Block mode after exiting `man`. |
| 10 | **`git` (pager + `add -p`)** | Inline colored diffs (`SGR 31`/`32`/`36`/`1`), `OSC 8` hyperlinks (when `delta` pager enabled), single-key raw prompts in `git add -p` | `test_tui_replay_less_man_git_pager`, `test_osc_8_hyperlinks` | **PASS** | In `git add -p`, interactive single-char prompt works in Classic mode or transitions to raw input after 2 s fallback in Block mode. |
| 11 | **`fzf`** | Inline partial-screen rendering (`DSR 6n` cursor position query, `CUU`/`CUD`/`EL`, `DECSTBM`), or `--height 100%` (`?1049h`), `?2026h` sync redraw, 24-bit SGR | `test_tui_replay_fzf_lazygit_session`, `test_device_status_and_attributes_reports` | **PASS** | `CSI 6 n` (`CPR`) immediately writes back `\x1b[{row};{col}R` via `EffectSink::write_back`, preventing startup delay in inline `fzf`. |
| 12 | **`lazygit`** | `?1049h`, Unicode rounded box corners (`╭╮╰╯`), `?1002h`+`?1006h` mouse pane clicking/scrolling, 256-color/truecolor SGR | `test_tui_replay_fzf_lazygit_session`, `test_tui_box_drawing_layout` | **PASS** | Rounded corners (`U+256D`–`U+2570`) rendered procedurally. |
| 13 | **`ssh`** | Remote PTY pass-through, `TERM=xterm-256color` fallback or `termin`, `OSC 0`/`2` remote title updates, `DECCKM`, bracketed paste `?2004` | `test_bracketed_paste_and_focus_events`, `test_osc_window_title` | **PASS** | Per `D15`, remote `ssh` sessions without Termin shell integration run in Classic mode automatically; no remote bootstrap injection in v1. |
| 14 | **`mosh`** | Predictive local echo (`SGR 4` underline predictions), `CUP`, `ECH`, `ICH`/`DCH`, UTF-8 stream sync, `?1049h` | `test_ich_dch_ech_character_operations`, `test_cup_and_hvp_positioning` | **PASS** | Zero panics on rapid cursor jumps and `ECH` prediction overwrites. |
| 15 | **`Claude Code`** | `?2026h`/`?2026l` synchronized output frames, `CSI > 1 u` Kitty keyboard, `OSC 9` / `BEL` ("waiting for input" attention), inline diff rendering (`\r\x1b[2K` spinner updates), `OSC 8` file links | `test_tui_replay_claude_code_codex_cli_session`, `test_synchronized_output_mode`, `test_osc_notifications` | **PASS** | Attention controller + sidebar card surfaces "Claude is waiting for your input" indicator and amber attention ring (`D5`). |
| 16 | **`Codex CLI`** | `?2026h`/`?2026l` synchronized output, `OSC 9`/`777` notifications, truecolor SGR diffs, bracketed paste `?2004h` multi-line prompts | `test_tui_replay_claude_code_codex_cli_session`, `test_synchronized_output_mode` | **PASS** | Multi-line paste framed via `\x1b[200~...\x1b[201~` with embedded `\x1b[201~` sanitized. |
| 17 | **`Python REPL`** | Readline / `pyrepl` bracketed paste (`?2004h`), `CHA`/`CUB`/`EL` inline editing & syntax coloring (Python 3.13+), multi-line continuation (`... `) | `test_decawm_autowrap_and_pending_wrap`, `test_el_erase_line` | **PASS** | Interactive REPL runs inside command block (or raw fallback on continuation prompt). |
| 18 | **`node REPL`** | `CHA` (`\x1b[1G`), `EL` (`\x1b[0J`), SGR gray ghost completion preview (`\x1b[90m`), `CSI 6 n` cursor query | `test_device_status_and_attributes_reports`, `test_horizontal_and_vertical_absolute_and_relative` | **PASS** | Instant `DSR 6n` response and `CHA`/`ED` redraw keep Node's inline autocomplete responsive. |

---

## 3. Documented v1 Scope Limitations (`docs/README.md`)

1. **Inline Image Protocols (`sixel`, Kitty graphics `APC G`, iTerm2 `OSC 1337 File=`)**: Ignored gracefully as no-op sequences by the parser (out of scope for v1 per `docs/README.md`).
2. **`tmux -CC` Control Mode**: Standard `tmux` is fully supported; iTerm2 `-CC` control protocol is out of scope for v1.
3. **Automatic Remote `ssh` Shell Integration Bootstrap**: Remote shells default to Classic mode unless the user installs the Termin shell integration snippet on the remote host (`D15`).

---

## 4. Fuzzing Verification (`fuzz/`)

| Target | Crate Exercised | Invariant Verified | Status |
|---|---|---|---|
| `fuzz_advance` | `termin-vt` | Arbitrary byte streams, malformed CSI/OSC/DCS/APC, invalid UTF-8, and interleaved `resize()` calls never panic or violate grid bounds. | **PASS** |
| `fuzz_tokenize` | `termin-editor` | Arbitrary UTF-8 strings, unclosed quotes, nested `$()` substitutions, and highlight span extraction never panic. | **PASS** |
| `fuzz_config_parse` | `termin-config` | Arbitrary TOML config, theme strings, color specs, and key chord strings never panic and always return structured `Diagnostic` values. | **PASS** |
