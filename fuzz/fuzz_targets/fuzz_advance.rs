//! Fuzz target for `Terminal::advance` and `Terminal::resize`.
//!
//! Asserts that malformed escape sequences, invalid UTF-8, and arbitrary byte streams
//! never cause panics or crashes.

#![no_main]

use libfuzzer_sys::fuzz_target;
use termin_vt::{Effect, EffectSink, Terminal, TerminalConfig};

struct FuzzSink;
impl EffectSink for FuzzSink {
    fn write_back(&mut self, _bytes: &[u8]) {}
    fn effect(&mut self, _e: Effect) {}
}

fuzz_target!(|data: &[u8]| {
    if data.is_empty() {
        return;
    }

    let mut term = Terminal::new(&TerminalConfig::default());
    let mut sink = FuzzSink;

    // Split input into chunks and occasionally trigger resize
    let mut offset = 0;
    while offset < data.len() {
        let chunk_size = (data[offset] as usize % 64) + 1;
        offset += 1;

        let end = (offset + chunk_size).min(data.len());
        let chunk = &data[offset..end];
        offset = end;

        term.advance(chunk, &mut sink);

        // Periodically test resize with values from fuzz stream
        if offset < data.len() && data[offset] == 0xff {
            offset += 1;
            let cols = if offset < data.len() {
                (data[offset] as u16).max(1).min(300)
            } else {
                80
            };
            let rows = if offset + 1 < data.len() {
                (data[offset + 1] as u16).max(1).min(100)
            } else {
                24
            };
            term.resize(cols, rows, (8, 16));
        }
    }
});
