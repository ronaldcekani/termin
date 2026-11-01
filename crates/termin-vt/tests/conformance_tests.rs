//! Master integration test runner for Termin VT Conformance & Compatibility suite.
//!
//! Submodules:
//! - `cursor`: vttest Menu 1 (CUP, HVP, CUU, CUD, CUF, CUB, CNL, CPL, CHA, VPA, DECOM, DECAWM, IND, RI, NEL, DECSC)
//! - `screen`: vttest Menu 2 (ED, EL, IL, DL, ICH, DCH, ECH, SU, SD, DECSTBM, BCE, tab stops)
//! - `charsets`: vttest Menu 3 (G0/G1 designation, SO/SI, DEC Special Graphics, UK charset)
//! - `modes_and_reports`: esctest (DECCKM, DECKPAM, alt screen, bracketed paste, focus, sync output, mouse, kitty, DSR, DA1, DA2, XTVERSION, DECRQM)
//! - `osc_and_integration`: OSC 0/2, 7, 8, 9/99/777, 10/11/12/4, 52, 133
//! - `tui_replay`: Realistic TUI application traces (vim, tmux, htop, less, fzf, Claude Code)

mod conformance;
