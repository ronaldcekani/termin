//! Fuzz target for `termin_editor` shell tokenizer and syntax highlighter.
//!
//! Asserts that arbitrary input strings (unclosed quotes, nested substitutions,
//! malformed escape sequences, arbitrary UTF-8) never panic during tokenization
//! or editor buffer manipulation.

#![no_main]

use libfuzzer_sys::fuzz_target;
use termin_editor::{tokenize, EditorState};

fuzz_target!(|data: &[u8]| {
    if let Ok(input) = std::str::from_utf8(data) {
        let tokens = tokenize(input);
        let _ = tokens.len();

        let mut editor = EditorState::new();
        let _ = editor.try_insert(input);
        editor.move_home();
        editor.move_word_right();
        editor.move_end();
        let _ = editor.highlight_spans(|cmd| cmd == "git" || cmd == "cargo");
    }
});
