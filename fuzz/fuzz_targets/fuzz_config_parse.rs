//! Fuzz target for `termin_config` TOML configuration, theme, and keybinding chord parsers.
//!
//! Asserts that malformed TOML files, invalid color strings, and arbitrary key chords
//! never panic and instead return structured diagnostics.

#![no_main]

use libfuzzer_sys::fuzz_target;
use termin_config::{parse_chord, parse_color, parse_config_str, parse_theme};

fuzz_target!(|data: &[u8]| {
    if let Ok(input) = std::str::from_utf8(data) {
        let (_cfg, _diags) = parse_config_str(input, |action| {
            matches!(action, "new_tab" | "close_tab" | "split_right" | "quit")
        });
        let _ = parse_theme(input);
        for line in input.lines().take(16) {
            let _ = parse_color(line);
            let _ = parse_chord(line);
        }
    }
});
