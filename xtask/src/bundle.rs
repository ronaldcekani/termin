//! macOS `.app` bundle, codesign, notarization, and archive assembler (`cargo xtask bundle`).
//!
//! Owned by Phase 4B (`docs/implementation-phases.md §Phase 4 — 4B`).
//!
//! Assembles `Termin.app` bundle:
//! - `Contents/Info.plist` (version-stamped from root `Cargo.toml`)
//! - `Contents/PkgInfo` (`APPL????`)
//! - `Contents/MacOS/termin` (arm64, x86_64, or lipo universal binary)
//! - `Contents/Resources/Termin.icns`
//! - `Contents/Resources/shell/` + `themes/`
//! - Ad-hoc or Developer ID codesigning with hardened runtime
//! - Distribution archives (`Termin.app.zip` via `ditto` and `.dmg` via `hdiutil`)
//! - Verification against `budgets.toml` (`app_bundle_zipped_arm64_mib = 4.0`).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone)]
pub struct BundleOptions {
    /// Target triple (e.g. `aarch64-apple-darwin`, `x86_64-apple-darwin`).
    pub target: Option<String>,
    /// Build universal binary combining arm64 and x86_64.
    pub universal: bool,
    /// Signing identity (e.g. `"-"` for ad-hoc, or `"Developer ID Application: ..."`).
    pub sign_identity: Option<String>,
    /// Whether to generate a `.zip` archive via `ditto`.
    pub create_zip: bool,
    /// Whether to generate a `.dmg` disk image via `hdiutil`.
    pub create_dmg: bool,
    /// Custom output directory for the bundle and archives.
    pub output_dir: Option<PathBuf>,
    /// Skip compiling the binaries (assume they are already present in target/).
    pub skip_build: bool,
}

impl Default for BundleOptions {
    fn default() -> Self {
        Self {
            target: None,
            universal: false,
            sign_identity: std::env::var("APPLE_SIGNING_IDENTITY").ok(),
            create_zip: true,
            create_dmg: false,
            output_dir: None,
            skip_build: false,
        }
    }
}

#[derive(serde::Deserialize, Default)]
struct Budgets {
    size: SizeBudgets,
}

#[derive(serde::Deserialize, Default)]
struct SizeBudgets {
    app_bundle_zipped_arm64_mib: Option<f64>,
    #[serde(alias = "app_zipped_arm64_mib")]
    app_zipped_arm64_mib: Option<f64>,
}

/// Entry point called by `xtask/src/main.rs`.
pub fn run(args: &[String]) {
    let mut opts = BundleOptions::default();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--target" => {
                i += 1;
                if i < args.len() {
                    opts.target = Some(args[i].clone());
                }
            }
            "--arch" => {
                i += 1;
                if i < args.len() {
                    match args[i].as_str() {
                        "arm64" | "aarch64" | "aarch64-apple-darwin" => {
                            opts.target = Some("aarch64-apple-darwin".to_string());
                        }
                        "x86_64" | "x64" | "x86_64-apple-darwin" => {
                            opts.target = Some("x86_64-apple-darwin".to_string());
                        }
                        "universal" => {
                            opts.universal = true;
                        }
                        other => {
                            opts.target = Some(other.to_string());
                        }
                    }
                }
            }
            "--universal" => {
                opts.universal = true;
            }
            "--sign" => {
                if i + 1 < args.len() && !args[i + 1].starts_with("--") {
                    i += 1;
                    opts.sign_identity = Some(args[i].clone());
                } else {
                    opts.sign_identity = Some("-".to_string());
                }
            }
            s if s.starts_with("--sign=") => {
                let id = &s["--sign=".len()..];
                opts.sign_identity = Some(if id.is_empty() { "-".to_string() } else { id.to_string() });
            }
            "--archive" | "--zip" => {
                opts.create_zip = true;
            }
            "--dmg" => {
                opts.create_dmg = true;
            }
            "--skip-build" => {
                opts.skip_build = true;
            }
            "--out-dir" => {
                i += 1;
                if i < args.len() {
                    opts.output_dir = Some(PathBuf::from(&args[i]));
                }
            }
            "--help" | "-h" => {
                print_help();
                return;
            }
            other => {
                eprintln!("Unknown argument: {}", other);
                print_help();
                std::process::exit(1);
            }
        }
        i += 1;
    }

    if let Err(err) = execute_bundle(&opts) {
        eprintln!("Bundle error: {}", err);
        std::process::exit(1);
    }
}

fn print_help() {
    println!("Usage: cargo xtask bundle [OPTIONS]");
    println!();
    println!("Options:");
    println!("  --arch <ARCH>         Architecture: arm64, x86_64, or universal");
    println!("  --target <TRIPLE>     Target architecture (e.g. aarch64-apple-darwin, x86_64-apple-darwin)");
    println!("  --universal           Build universal binary for macOS (arm64 + x86_64)");
    println!("  --sign [IDENTITY]     Codesign bundle (default: '-' ad-hoc, or $APPLE_SIGNING_IDENTITY)");
    println!("  --zip, --archive      Create Termin.app.zip archive via ditto (default: true)");
    println!("  --dmg                 Create Termin.dmg disk image via hdiutil");
    println!("  --out-dir <PATH>      Bundle destination directory (default: target/release/bundle)");
    println!("  --skip-build          Skip cargo build step (use existing binary in target/)");
    println!("  --help, -h            Show this help message");
}

/// Execute the bundle assembly and packaging pipeline.
pub fn execute_bundle(opts: &BundleOptions) -> Result<PathBuf, String> {
    let root = crate::project_root();
    println!("================================================================================");
    println!("                     Termin macOS Application Bundler                           ");
    println!("================================================================================");

    // 1. Resolve output directory
    let bundle_dir = opts
        .output_dir
        .clone()
        .unwrap_or_else(|| root.join("target/release/bundle"));
    let app_path = bundle_dir.join("Termin.app");

    if app_path.exists() {
        println!("Cleaning existing bundle at {}...", app_path.display());
        let _ = fs::remove_dir_all(&app_path);
    }
    fs::create_dir_all(&bundle_dir)
        .map_err(|e| format!("Failed to create bundle dir {}: {}", bundle_dir.display(), e))?;

    // 2. Extract version from root Cargo.toml
    let cargo_toml_path = root.join("Cargo.toml");
    let cargo_content = fs::read_to_string(&cargo_toml_path)
        .map_err(|e| format!("Failed to read Cargo.toml: {}", e))?;
    let version = extract_workspace_version(&cargo_content)
        .unwrap_or_else(|| "0.1.0".to_string());
    println!("Target version: {}", version);

    // 3. Resolve or build executable
    let executable_path = resolve_or_build_binary(&root, opts)?;
    println!("Executable ready: {}", executable_path.display());

    // 4. Assemble bundle structure
    println!("Assembling Termin.app at {}...", app_path.display());
    assemble_bundle_structure(
        &root,
        &app_path,
        &executable_path,
        &version,
    )?;

    // 5. Codesign if requested
    if let Some(ref identity) = opts.sign_identity {
        println!("Codesigning bundle with identity '{}'...", identity);
        sign_bundle(&app_path, identity)?;
    } else {
        println!("Skipping codesign (no --sign identity provided).");
    }

    // 6. Create archives
    if opts.create_zip {
        let zip_name = if opts.universal {
            "Termin-macos-universal.zip".to_string()
        } else if let Some(ref target) = opts.target {
            let arch = if target.contains("aarch64") { "arm64" } else { "x86_64" };
            format!("Termin-macos-{}.zip", arch)
        } else {
            "Termin.app.zip".to_string()
        };
        let zip_path = bundle_dir.join(&zip_name);
        println!("Creating zip archive at {}...", zip_path.display());
        create_zip_archive(&app_path, &zip_path)?;

        // Also create standard Termin.app.zip symlink / copy for consistency
        let standard_zip = bundle_dir.join("Termin.app.zip");
        if zip_path != standard_zip {
            let _ = fs::copy(&zip_path, &standard_zip);
        }

        // Verify size budget
        verify_zip_size(&root, &standard_zip)?;
    }

    if opts.create_dmg {
        let dmg_path = bundle_dir.join("Termin.dmg");
        println!("Creating DMG disk image at {}...", dmg_path.display());
        create_dmg_image(&app_path, &dmg_path)?;
    }

    println!("Bundle assembly complete: {}", app_path.display());
    Ok(app_path)
}

/// Extract `workspace.package.version` from Cargo.toml.
pub fn extract_workspace_version(cargo_toml_content: &str) -> Option<String> {
    if let Ok(value) = cargo_toml_content.parse::<toml::Value>() {
        if let Some(v) = value
            .get("workspace")
            .and_then(|w| w.get("package"))
            .and_then(|p| p.get("version"))
            .and_then(|v| v.as_str())
        {
            return Some(v.to_string());
        }
    }

    for line in cargo_toml_content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("version = \"") && trimmed.ends_with('"') {
            let start = "version = \"".len();
            return Some(trimmed[start..trimmed.len() - 1].to_string());
        }
    }
    None
}

/// Replace `{{VERSION}}` placeholder in Info.plist with the target version string.
pub fn stamp_info_plist(template: &str, version: &str) -> String {
    template.replace("{{VERSION}}", version)
}

/// Resolves existing binaries or compiles them via cargo.
fn resolve_or_build_binary(root: &Path, opts: &BundleOptions) -> Result<PathBuf, String> {
    if opts.universal {
        let arm64_target = "aarch64-apple-darwin";
        let x86_target = "x86_64-apple-darwin";
        let arm64_bin = root.join("target").join(arm64_target).join("release/termin");
        let x86_bin = root.join("target").join(x86_target).join("release/termin");

        if !opts.skip_build {
            println!("Compiling arm64 release binary (cargo build --release --target {})...", arm64_target);
            run_cargo_build(root, Some(arm64_target))?;
            println!("Compiling x86_64 release binary (cargo build --release --target {})...", x86_target);
            run_cargo_build(root, Some(x86_target))?;
        }

        if !arm64_bin.exists() {
            return Err(format!("Missing arm64 binary at {}", arm64_bin.display()));
        }
        if !x86_bin.exists() {
            return Err(format!("Missing x86_64 binary at {}", x86_bin.display()));
        }

        let universal_out = root.join("target/release/termin-universal");
        println!("Creating universal Mach-O binary via lipo...");
        let lipo_status = Command::new("lipo")
            .args([
                "-create",
                "-output",
                universal_out.to_str().unwrap(),
                arm64_bin.to_str().unwrap(),
                x86_bin.to_str().unwrap(),
            ])
            .status()
            .map_err(|e| format!("Failed to invoke lipo: {}", e))?;

        if !lipo_status.success() {
            return Err("lipo command failed to create universal binary".to_string());
        }
        Ok(universal_out)
    } else if let Some(ref target) = opts.target {
        let bin_path = root.join("target").join(target).join("release/termin");
        if !opts.skip_build {
            println!("Compiling release binary for target {}...", target);
            run_cargo_build(root, Some(target))?;
        }
        if !bin_path.exists() {
            return Err(format!("Binary not found at {}", bin_path.display()));
        }
        Ok(bin_path)
    } else {
        let bin_path = root.join("target/release/termin");
        if !opts.skip_build {
            println!("Compiling release binary (host)...");
            run_cargo_build(root, None)?;
        }
        if !bin_path.exists() {
            return Err(format!("Binary not found at {}", bin_path.display()));
        }
        Ok(bin_path)
    }
}

fn run_cargo_build(root: &Path, target: Option<&str>) -> Result<(), String> {
    let mut cmd = Command::new("cargo");
    cmd.current_dir(root).args(["build", "--release", "-p", "termin"]);
    if let Some(t) = target {
        cmd.args(["--target", t]);
    }
    let status = cmd.status().map_err(|e| format!("Failed to spawn cargo: {}", e))?;
    if !status.success() {
        return Err(format!("Cargo build failed with exit status {:?}", status));
    }
    Ok(())
}

/// Assembles the complete directory layout of the macOS `.app` bundle.
pub fn assemble_bundle_structure(
    root: &Path,
    app_path: &Path,
    executable_src: &Path,
    version: &str,
) -> Result<(), String> {
    let contents = app_path.join("Contents");
    let macos_dir = contents.join("MacOS");
    let resources_dir = contents.join("Resources");

    fs::create_dir_all(&macos_dir)
        .map_err(|e| format!("Failed to create {}: {}", macos_dir.display(), e))?;
    fs::create_dir_all(&resources_dir)
        .map_err(|e| format!("Failed to create {}: {}", resources_dir.display(), e))?;

    // 1. Contents/PkgInfo
    fs::write(contents.join("PkgInfo"), b"APPL????")
        .map_err(|e| format!("Failed to write PkgInfo: {}", e))?;

    // 2. Contents/Info.plist
    let plist_src = root.join("resources/Info.plist");
    let plist_template = fs::read_to_string(&plist_src)
        .map_err(|e| format!("Failed to read {}: {}", plist_src.display(), e))?;
    let plist_stamped = stamp_info_plist(&plist_template, version);
    fs::write(contents.join("Info.plist"), plist_stamped)
        .map_err(|e| format!("Failed to write Info.plist: {}", e))?;

    // 3. Contents/MacOS/termin
    let dest_bin = macos_dir.join("termin");
    fs::copy(executable_src, &dest_bin)
        .map_err(|e| format!("Failed to copy executable: {}", e))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&dest_bin, fs::Permissions::from_mode(0o755));
    }

    // 4. Contents/Resources/Termin.icns
    let icns_src = root.join("resources/Termin.icns");
    if icns_src.exists() {
        fs::copy(&icns_src, resources_dir.join("Termin.icns"))
            .map_err(|e| format!("Failed to copy icon {}: {}", icns_src.display(), e))?;
    } else {
        println!("Warning: resources/Termin.icns not found!");
    }

    // 5. Shell integration scripts
    let shell_src = root.join("resources/shell");
    if shell_src.exists() {
        let shell_dst = resources_dir.join("shell");
        copy_dir_recursive(&shell_src, &shell_dst)?;
    }

    // 6. Themes
    let themes_src = root.join("resources/themes");
    if themes_src.exists() {
        let themes_dst = resources_dir.join("themes");
        copy_dir_recursive(&themes_src, &themes_dst)?;
    }

    Ok(())
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), String> {
    if !dst.exists() {
        fs::create_dir_all(dst)
            .map_err(|e| format!("Failed to create dir {}: {}", dst.display(), e))?;
    }
    for entry in fs::read_dir(src).map_err(|e| format!("Failed to read {}: {}", src.display(), e))? {
        let entry = entry.map_err(|e| format!("Dir entry error: {}", e))?;
        let file_type = entry.file_type().map_err(|e| format!("File type error: {}", e))?;
        let dest_path = dst.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir_recursive(&entry.path(), &dest_path)?;
        } else {
            fs::copy(entry.path(), &dest_path)
                .map_err(|e| format!("Failed to copy {} to {}: {}", entry.path().display(), dest_path.display(), e))?;
        }
    }
    Ok(())
}

/// Sign the bundle with Apple codesign tool.
pub fn sign_bundle(app_path: &Path, identity: &str) -> Result<(), String> {
    let status = Command::new("codesign")
        .args([
            "--force",
            "--options",
            "runtime",
            "--sign",
            identity,
            app_path.to_str().unwrap(),
        ])
        .status()
        .map_err(|e| format!("Failed to run codesign: {}", e))?;

    if !status.success() {
        return Err(format!("codesign failed with status {:?}", status));
    }

    // Verify signature
    let verify_status = Command::new("codesign")
        .args(["--verify", "--verbose=2", app_path.to_str().unwrap()])
        .status()
        .map_err(|e| format!("Failed to verify signature: {}", e))?;

    if !verify_status.success() {
        return Err("codesign signature verification failed".to_string());
    }

    println!("Signature verified for {}", app_path.display());
    Ok(())
}

/// Create a compressed ZIP archive of the bundle preserving macOS metadata via `ditto`.
pub fn create_zip_archive(app_path: &Path, zip_path: &Path) -> Result<(), String> {
    if zip_path.exists() {
        let _ = fs::remove_file(zip_path);
    }
    let status = Command::new("ditto")
        .env("COPYFILE_DISABLE", "1")
        .args([
            "-c",
            "-k",
            "--keepParent",
            app_path.to_str().unwrap(),
            zip_path.to_str().unwrap(),
        ])
        .status()
        .map_err(|e| format!("Failed to invoke ditto: {}", e))?;

    if !status.success() {
        return Err(format!("ditto failed to create zip archive: {:?}", status));
    }
    Ok(())
}

/// Create a DMG disk image via `hdiutil`.
pub fn create_dmg_image(app_path: &Path, dmg_path: &Path) -> Result<(), String> {
    if dmg_path.exists() {
        let _ = fs::remove_file(dmg_path);
    }
    let bundle_dir = dmg_path.parent().unwrap_or(app_path.parent().unwrap());
    let staging_dir = bundle_dir.join("dmg_staging");
    if staging_dir.exists() {
        let _ = fs::remove_dir_all(&staging_dir);
    }
    fs::create_dir_all(&staging_dir)
        .map_err(|e| format!("Failed to create dmg staging dir: {}", e))?;

    // Copy Termin.app into staging
    let staged_app = staging_dir.join("Termin.app");
    copy_dir_recursive(app_path, &staged_app)?;

    // Create /Applications symlink for drag-and-drop install convenience
    #[cfg(unix)]
    let _ = std::os::unix::fs::symlink("/Applications", staging_dir.join("Applications"));

    let status = Command::new("hdiutil")
        .args([
            "create",
            "-volname",
            "Termin",
            "-srcfolder",
            staging_dir.to_str().unwrap(),
            "-ov",
            "-format",
            "UDZO",
            dmg_path.to_str().unwrap(),
        ])
        .status()
        .map_err(|e| format!("Failed to invoke hdiutil: {}", e))?;

    // Clean up staging dir
    let _ = fs::remove_dir_all(&staging_dir);

    if !status.success() {
        return Err(format!(
            "hdiutil failed to create dmg (status: {:?}). Note: DMG creation requires disk device access (/dev/disk) which may be restricted in sandboxed environments.",
            status
        ));
    }
    Ok(())
}

/// Verify zip archive size against `budgets.toml`.
pub fn verify_zip_size(root: &Path, zip_path: &Path) -> Result<(), String> {
    let budgets_path = root.join("budgets.toml");
    let budget_mib = if budgets_path.exists() {
        let content = fs::read_to_string(&budgets_path).unwrap_or_default();
        let budgets: Budgets = toml::from_str(&content).unwrap_or_default();
        budgets
            .size
            .app_bundle_zipped_arm64_mib
            .or(budgets.size.app_zipped_arm64_mib)
            .unwrap_or(4.0)
    } else {
        4.0
    };

    let metadata = fs::metadata(zip_path)
        .map_err(|e| format!("Failed to read metadata of {}: {}", zip_path.display(), e))?;
    let size_bytes = metadata.len();
    let size_mib = size_bytes as f64 / (1024.0 * 1024.0);

    println!("--------------------------------------------------------------------------------");
    println!("Zip Archive:   {}", zip_path.display());
    println!("Actual Size:   {:.2} MiB ({} bytes)", size_mib, size_bytes);
    println!("Budget Limit:  {:.2} MiB", budget_mib);

    if size_mib <= budget_mib {
        println!("Status:        PASS (within budget)");
        println!("--------------------------------------------------------------------------------");
        Ok(())
    } else {
        println!("Status:        FAIL (exceeds budget by {:.2} MiB)", size_mib - budget_mib);
        println!("--------------------------------------------------------------------------------");
        Err(format!(
            "Zipped bundle size {:.2} MiB exceeds budget limit {:.2} MiB",
            size_mib, budget_mib
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_workspace_version() {
        let sample_toml = r#"
[workspace]
members = ["bin/termin"]

[workspace.package]
version = "0.2.1"
edition = "2021"
"#;
        let v = extract_workspace_version(sample_toml);
        assert_eq!(v.as_deref(), Some("0.2.1"));
    }

    #[test]
    fn test_stamp_info_plist() {
        let template = r#"
<dict>
    <key>CFBundleShortVersionString</key>
    <string>{{VERSION}}</string>
    <key>CFBundleVersion</key>
    <string>{{VERSION}}</string>
</dict>
"#;
        let stamped = stamp_info_plist(template, "1.4.0");
        assert!(stamped.contains("<string>1.4.0</string>"));
        assert!(!stamped.contains("{{VERSION}}"));
    }

    #[test]
    fn test_icns_header_validity() {
        let root = crate::project_root();
        let icns_path = root.join("resources/Termin.icns");
        assert!(icns_path.exists(), "Termin.icns must exist in resources");

        let data = fs::read(&icns_path).expect("Failed to read Termin.icns");
        assert!(data.len() >= 8, "ICNS file must have at least 8 bytes");
        // Magic 'icns'
        assert_eq!(&data[0..4], b"icns", "Magic header must be 'icns'");

        let length = u32::from_be_bytes([data[4], data[5], data[6], data[7]]) as usize;
        assert_eq!(length, data.len(), "Header length must match file size");

        // First chunk tag
        let first_tag = &data[8..12];
        assert!(
            first_tag == b"ic07" || first_tag == b"ic08" || first_tag == b"ic09" || first_tag == b"ic10",
            "First chunk must be a recognized icon element tag"
        );
    }

    #[test]
    fn test_bundle_structure_assembly() {
        let temp_dir = std::env::temp_dir().join(format!("termin-bundle-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).unwrap();

        let fake_root = temp_dir.join("repo");
        let fake_res = fake_root.join("resources");
        fs::create_dir_all(&fake_res).unwrap();
        fs::write(fake_res.join("Info.plist"), "<dict><key>CFBundleVersion</key><string>{{VERSION}}</string></dict>").unwrap();
        fs::write(fake_res.join("Termin.icns"), b"icns\x00\x00\x00\x08").unwrap();

        let fake_bin = temp_dir.join("termin_bin");
        fs::write(&fake_bin, b"#!/bin/sh\nexit 0\n").unwrap();

        let target_app = temp_dir.join("Termin.app");
        let res = assemble_bundle_structure(&fake_root, &target_app, &fake_bin, "0.9.9");
        assert!(res.is_ok());

        assert!(target_app.join("Contents/MacOS/termin").exists());
        assert!(target_app.join("Contents/Info.plist").exists());
        assert!(target_app.join("Contents/PkgInfo").exists());
        assert!(target_app.join("Contents/Resources/Termin.icns").exists());

        let plist_content = fs::read_to_string(target_app.join("Contents/Info.plist")).unwrap();
        assert!(plist_content.contains("0.9.9"));

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_verify_zip_size() {
        let temp_dir = std::env::temp_dir().join(format!("termin-size-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).unwrap();

        // 1. Small zip file within budget
        let small_zip = temp_dir.join("small.zip");
        fs::write(&small_zip, vec![0u8; 1024]).unwrap();

        let fake_budgets = temp_dir.join("budgets.toml");
        fs::write(&fake_budgets, "[size]\napp_bundle_zipped_arm64_mib = 1.0\n").unwrap();

        assert!(verify_zip_size(&temp_dir, &small_zip).is_ok());

        // 2. Huge zip exceeding budget
        fs::write(&fake_budgets, "[size]\napp_bundle_zipped_arm64_mib = 0.0001\n").unwrap();
        assert!(verify_zip_size(&temp_dir, &small_zip).is_err());

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
