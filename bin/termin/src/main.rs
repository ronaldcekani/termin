//! Termin — native macOS terminal and CLI client.
//!
//! See `docs/architecture.md §1` and `docs/specs/ipc-cli.md §4`.

mod cli;

use std::env;
use std::process;
use termin_ipc::protocol::{METHOD_RUN_ACTION, RunActionParams};
use termin_ipc::{resolve_socket_path, Client};

fn is_cli_subcommand_or_flag(arg: &str) -> bool {
    matches!(
        arg,
        "ls" | "list"
            | "new"
            | "split"
            | "tab"
            | "focus"
            | "send"
            | "keys"
            | "read"
            | "notify"
            | "status"
            | "action"
            | "rpc"
            | "help"
            | "version"
            | "--help"
            | "-h"
            | "--version"
            | "-v"
            | "--json"
            | "--socket"
    ) || arg.starts_with('-')
}

fn main() {
    let args: Vec<String> = env::args().collect();

    // If argv[1] is present and starts with a subcommand or flag for CLI -> dispatch to CLI
    if args.len() > 1 && is_cli_subcommand_or_flag(&args[1]) {
        let code = cli::run(&args);
        process::exit(code);
    }

    // Single-instance check: if another instance is already running, focus it and exit
    let sock = resolve_socket_path();
    if let Ok(mut client) = Client::try_connect(&sock) {
        let params = RunActionParams {
            name: "focus_window".to_string(),
            args: None,
        };
        let _ = client.call(
            METHOD_RUN_ACTION,
            serde_json::to_value(params).unwrap_or(serde_json::Value::Null),
        );
        process::exit(0);
    }

    // Otherwise, launch GUI (or Phase 1 stub version print)
    println!("termin {}", env!("CARGO_PKG_VERSION"));
}
