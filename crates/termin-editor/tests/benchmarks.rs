use std::time::Instant;
use termin_editor::{
    complete, CompletionContext, HistoryIndex, StaticCommandSpec, StaticOptionSpec,
    StaticSubcommandSpec, COMPILED_SPECS,
};

#[test]
fn bench_autosuggest_50k_items() {
    let mut history = HistoryIndex::new();
    for i in 0..50_000 {
        history.push_ref(
            1730000000 + i,
            Some(0),
            Some(10),
            "/workspace/project",
            "zsh",
            &format!("git commit -m 'commit number {}'", i),
        );
    }

    // Warm-up
    let _ = history.suggest("git commit -m 'commit number 499", "/workspace/project");

    let mut samples = Vec::with_capacity(20);
    for _ in 0..20 {
        let start = Instant::now();
        let sug = history.suggest("git commit -m 'commit number 4999", "/workspace/project");
        let elapsed = start.elapsed();
        assert!(sug.is_some());
        samples.push(elapsed);
    }

    let min_sample = samples.iter().min().unwrap();
    let avg_sample: std::time::Duration =
        samples.iter().sum::<std::time::Duration>() / (samples.len() as u32);

    println!(
        "[BENCH] autosuggest over 50k entries: min={:?}, avg={:?}",
        min_sample, avg_sample
    );

    // Budget: < 0.5 ms (500 microseconds)
    assert!(
        min_sample.as_micros() < 500,
        "Autosuggest exceeded budget (min {:?} >= 500 µs)",
        min_sample
    );
}

#[test]
fn bench_sync_completion_large_dir() {
    let temp_dir = std::env::temp_dir().join("termin_bench_5k_files");
    let _ = std::fs::create_dir_all(&temp_dir);

    for i in 0..5_000 {
        let _ = std::fs::write(temp_dir.join(format!("file_{:04}.txt", i)), b"");
    }

    let ctx = CompletionContext::new(temp_dir.to_str().unwrap(), "/usr/bin", "zsh");

    // Warm up
    let _ = complete("cat file_49", 11, &ctx);

    let mut samples = Vec::with_capacity(10);
    for _ in 0..10 {
        let start = Instant::now();
        let res = complete("cat file_499", 12, &ctx);
        let elapsed = start.elapsed();
        assert!(!res.candidates.is_empty());
        samples.push(elapsed);
    }

    let _ = std::fs::remove_dir_all(&temp_dir);

    let min_sample = samples.iter().min().unwrap();
    let avg_sample: std::time::Duration =
        samples.iter().sum::<std::time::Duration>() / (samples.len() as u32);

    println!(
        "[BENCH] sync completion over 5k files: min={:?}, avg={:?}",
        min_sample, avg_sample
    );

    // Budget: < 2 ms (2000 microseconds) in release mode; < 10 ms in unoptimized debug
    let budget_us = if cfg!(debug_assertions) {
        10_000
    } else {
        2_000
    };
    assert!(
        min_sample.as_micros() < budget_us,
        "Sync completion exceeded budget (min {:?} >= {} µs)",
        min_sample,
        budget_us
    );
}

#[test]
fn bench_compiled_spec_size_budget() {
    let mut total_bytes = std::mem::size_of_val(COMPILED_SPECS);
    for cmd in COMPILED_SPECS {
        total_bytes += std::mem::size_of::<StaticCommandSpec>();
        total_bytes += cmd.name.len();
        total_bytes += cmd.description.len();
        for sub in cmd.subcommands {
            total_bytes += std::mem::size_of::<StaticSubcommandSpec>();
            total_bytes += sub.name.len();
            total_bytes += sub.description.len();
        }
        for opt in cmd.options {
            total_bytes += std::mem::size_of::<StaticOptionSpec>();
            total_bytes += opt.description.len();
        }
    }

    println!(
        "[BENCH] compiled specs table memory: {} bytes ({:.2} KiB)",
        total_bytes,
        total_bytes as f64 / 1024.0
    );

    // Budget: ≤ 150 KiB (153600 bytes)
    assert!(
        total_bytes <= 150 * 1024,
        "Compiled specs exceeded budget: {} bytes > 150 KiB",
        total_bytes
    );
}
