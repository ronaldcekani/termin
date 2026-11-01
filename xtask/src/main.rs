//! Internal build and maintenance automation for Termin (`xtask`).
//!
//! See `docs/architecture.md §2` and `docs/specs/performance.md §1`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub mod bench;
pub mod bundle;
pub mod corpus;
pub mod mem;
pub mod throughput;

#[derive(serde::Deserialize)]
struct Budgets {
    size: SizeBudgets,
}

#[derive(serde::Deserialize)]
struct SizeBudgets {
    binary_arm64_stripped_mib: f64,
    binary_arm64_stripped_stretch_mib: f64,
    embedded_resources_kib: f64,
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let subcommand = args.get(1).map(|s| s.as_str()).unwrap_or("help");
    let sub_args = if args.len() > 2 { &args[2..] } else { &[] };

    match subcommand {
        "size" => run_size(),
        "mem" => mem::run(sub_args),
        "bench" => bench::run(sub_args),
        "throughput" => throughput::run(sub_args),
        "corpus" => corpus::run(sub_args),
        "bundle" => bundle::run(sub_args),
        _ => {
            println!("Termin xtask runner");
            println!("Usage: cargo xtask <size|mem|bench|throughput|corpus|bundle>");
        }
    }
}

pub fn project_root() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir.parent().unwrap().to_path_buf()
}

fn run_size() {
    let root = project_root();
    let budgets_path = root.join("budgets.toml");
    let budgets: Budgets = if budgets_path.exists() {
        let content = fs::read_to_string(&budgets_path).expect("Failed to read budgets.toml");
        toml::from_str(&content).expect("Failed to parse budgets.toml")
    } else {
        Budgets {
            size: SizeBudgets {
                binary_arm64_stripped_mib: 8.0,
                binary_arm64_stripped_stretch_mib: 5.0,
                embedded_resources_kib: 600.0,
            },
        }
    };

    println!("================================================================================");
    println!("                           Termin Binary Size Report                            ");
    println!("================================================================================");

    // 1. Check/build release binary
    let release_bin = root.join("target/release/termin");
    if !release_bin.exists() {
        println!("Building release binary (cargo build --release -p termin)...");
        let status = Command::new("cargo")
            .current_dir(&root)
            .args(["build", "--release", "-p", "termin"])
            .status()
            .expect("Failed to build release binary");
        if !status.success() {
            eprintln!("Failed to compile termin binary for size check.");
            std::process::exit(1);
        }
    }

    let bin_size_bytes = fs::metadata(&release_bin).map(|m| m.len()).unwrap_or(0);
    let bin_size_mib = bin_size_bytes as f64 / (1024.0 * 1024.0);

    // 2. Compute total embedded resources in resources/
    let resources_dir = root.join("resources");
    let res_size_bytes = dir_size(&resources_dir);
    let res_size_kib = res_size_bytes as f64 / 1024.0;

    println!(
        "{:<32} {:>12} {:>12} {:>12} {:>8}",
        "Artifact", "Actual", "Budget", "Stretch", "Status"
    );
    println!("--------------------------------------------------------------------------------");

    let bin_status = if bin_size_mib <= budgets.size.binary_arm64_stripped_stretch_mib {
        "EXCELLENT"
    } else if bin_size_mib <= budgets.size.binary_arm64_stripped_mib {
        "PASS"
    } else {
        "FAIL"
    };

    println!(
        "{:<32} {:>10.2} MiB {:>10.2} MiB {:>10.2} MiB {:>8}",
        "termin binary",
        bin_size_mib,
        budgets.size.binary_arm64_stripped_mib,
        budgets.size.binary_arm64_stripped_stretch_mib,
        bin_status
    );

    let res_status = if res_size_kib <= budgets.size.embedded_resources_kib {
        "PASS"
    } else {
        "FAIL"
    };

    println!(
        "{:<32} {:>10.2} KiB {:>10.2} KiB {:>10}     {:>8}",
        "resources/ total", res_size_kib, budgets.size.embedded_resources_kib, "-", res_status
    );

    println!("--------------------------------------------------------------------------------");
    if bin_status == "FAIL" || res_status == "FAIL" {
        eprintln!("Size budget exceeded!");
        std::process::exit(1);
    }
}

fn dir_size(path: &Path) -> u64 {
    let mut total = 0;
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                total += dir_size(&p);
            } else if p.is_file() {
                total += entry.metadata().map(|m| m.len()).unwrap_or(0);
            }
        }
    }
    total
}
