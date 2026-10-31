use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let shader_source = Path::new("../../resources/shaders/termin.metal");
    println!("cargo:rerun-if-changed={}", shader_source.display());

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let metallib_dest = out_dir.join("termin.metallib");

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os == "macos" && shader_source.exists() {
        let air_dest = out_dir.join("termin.air");

        // Try compiling .metal -> .air via xcrun -sdk macosx metal
        let metal_compile = Command::new("xcrun")
            .args([
                "-sdk",
                "macosx",
                "metal",
                "-c",
                shader_source.to_str().unwrap(),
                "-o",
                air_dest.to_str().unwrap(),
            ])
            .status();

        if let Ok(status) = metal_compile {
            if status.success() {
                // Try linking .air -> .metallib via xcrun -sdk macosx metallib
                let metallib_link = Command::new("xcrun")
                    .args([
                        "-sdk",
                        "macosx",
                        "metallib",
                        air_dest.to_str().unwrap(),
                        "-o",
                        metallib_dest.to_str().unwrap(),
                    ])
                    .status();

                if let Ok(status) = metallib_link {
                    if status.success() {
                        println!("cargo:rustc-cfg=has_metallib");
                        return;
                    }
                }
            }
        }
    }

    // Fallback: create empty placeholder so include_bytes compiles
    if !metallib_dest.exists() {
        let _ = std::fs::write(&metallib_dest, []);
    }
}
