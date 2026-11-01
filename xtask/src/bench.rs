//! Benchmark runner (`cargo xtask bench`).
//!
//! Supports `cargo xtask bench` and `cargo xtask bench --quick` (`docs/specs/performance.md §4, §5`).

use std::process::Command;

use crate::{project_root, throughput};

pub fn run(args: &[String]) {
    let quick = args.iter().any(|a| a == "--quick");
    let root = project_root();

    if quick {
        throughput::run(&["--quick".to_string()]);
        return;
    }

    throughput::run(args);

    for pkg in ["termin-vt", "termin-render", "termin-editor"] {
        println!("\nRunning `cargo bench -p {pkg}`...");
        let status = Command::new("cargo")
            .current_dir(&root)
            .args(["bench", "-p", pkg])
            .status()
            .expect("failed to execute cargo bench");
        if !status.success() {
            std::process::exit(1);
        }
    }
}
