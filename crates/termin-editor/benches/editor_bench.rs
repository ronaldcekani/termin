//! Input editor autosuggest and synchronous completion benchmarks (`cargo bench -p termin-editor`).
//!
//! Verifies `docs/specs/performance.md` & `docs/specs/input-editor.md`:
//! - Autosuggest over 50k history entries < 0.5 ms
//! - Synchronous completion < 2.0 ms

use std::hint::black_box;
use std::time::Instant;
use termin_editor::{complete, CompletionContext, EditorState, HistoryIndex};

fn main() {
    let mut store = HistoryIndex::new();
    for i in 0..50_000 {
        store.push_ref(
            1_700_000_000 + i as u64,
            Some(0),
            Some(12),
            "/Users/termin/workspace",
            "zsh",
            &format!("git commit -m \"feat(module-{}): implement fast path\"", i),
        );
    }

    let mut best_suggest_ms = f64::MAX;
    for _ in 0..25 {
        let start = Instant::now();
        let res = store.suggest("git commit -m \"feat(module-4999", "/Users/termin/workspace");
        black_box(res);
        let ms = start.elapsed().as_secs_f64() * 1000.0;
        if ms < best_suggest_ms {
            best_suggest_ms = ms;
        }
    }
    println!(
        "bench editor/autosuggest_50k:       {:>8.3} ms (budget <= 0.5 ms)",
        best_suggest_ms
    );

    let mut editor = EditorState::new();
    editor.insert("git checkout --");
    let ctx = CompletionContext::default();
    let mut best_complete_ms = f64::MAX;
    for _ in 0..25 {
        let start = Instant::now();
        let res = complete(&editor.text, editor.cursor, &ctx);
        black_box(res);
        let ms = start.elapsed().as_secs_f64() * 1000.0;
        if ms < best_complete_ms {
            best_complete_ms = ms;
        }
    }
    println!(
        "bench editor/complete_sync:         {:>8.3} ms (budget <= 2.0 ms)",
        best_complete_ms
    );
}
