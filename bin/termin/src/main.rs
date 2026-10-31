//! Termin — native macOS terminal and CLI client.
//!
//! See `docs/architecture.md §1` and `docs/specs/ipc-cli.md §4`.

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 && (args[1] == "--version" || args[1] == "-v") {
        println!("termin {}", env!("CARGO_PKG_VERSION"));
        return;
    }

    // Default Phase 0 execution: print version
    println!("termin {}", env!("CARGO_PKG_VERSION"));
}
