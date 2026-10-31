#![allow(dead_code)]

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

#[path = "src/spec.rs"]
mod spec;
#[path = "src/spec_compile.rs"]
mod spec_compile;

use spec::CommandSpec;

fn main() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let specs_dir = Path::new(&manifest_dir).join("../../resources/specs");

    println!("cargo:rerun-if-changed={}", specs_dir.display());

    let mut specs = Vec::new();

    if let Ok(entries) = fs::read_dir(&specs_dir) {
        let mut paths: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|ext| ext == "toml"))
            .collect();

        // Deterministic ordering
        paths.sort();

        for path in paths {
            println!("cargo:rerun-if-changed={}", path.display());
            let content = match fs::read_to_string(&path) {
                Ok(c) => c,
                Err(e) => {
                    panic!("Failed to read spec file {}: {}", path.display(), e);
                }
            };

            let spec: CommandSpec = match spec_compile::parse_spec_toml(&content) {
                Ok(s) => s,
                Err(e) => {
                    panic!("Failed to parse spec TOML {}: {}", path.display(), e);
                }
            };

            specs.push(spec);
        }
    }

    let code = spec_compile::compile_specs_to_rust(&specs);

    let out_dir = env::var("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("compiled_specs.rs");
    fs::write(&dest_path, code).unwrap();
}
