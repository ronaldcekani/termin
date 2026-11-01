//! Tests verifying replay of generated and committed corpus files.

mod corpus_gen;

use std::path::Path;
use termin_vt::{Effect, EffectSink, Terminal, TerminalConfig};

struct DummySink;
impl EffectSink for DummySink {
    fn write_back(&mut self, _bytes: &[u8]) {}
    fn effect(&mut self, _e: Effect) {}
}

#[test]
fn test_replay_all_corpus_files() {
    let corpus_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("corpus");
    corpus_gen::generate_all_corpora(&corpus_dir);

    let entries = std::fs::read_dir(&corpus_dir).unwrap();
    for entry in entries {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some("bin") {
            let data = std::fs::read(&path).unwrap();
            let cfg = TerminalConfig::default();
            let mut term = Terminal::new(&cfg);
            let mut sink = DummySink;

            // Replay entire binary file into the terminal
            term.advance(&data, &mut sink);

            // Assert it never panicked and generated output
            assert!(term.generation() > 0);
            assert!(term.total_lines() > 0);
        }
    }
}
