//! CLI symlink installation controller (`Install CLI in PATH` palette action).
//!
//! Owned by Phase 4B (`docs/implementation-phases.md §Phase 4 — 4B`).
//!
//! Installs a symlink to the `termin` executable in `/usr/local/bin/termin` if writable,
//! or falls back to `~/.local/bin/termin` (creating `~/.local/bin` if needed).
//! Also provides administrative elevation via `osascript` when authorized by the user.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::action::Action;
use crate::model::AppModel;

/// System-wide CLI symlink target.
pub const SYSTEM_CLI_PATH: &str = "/usr/local/bin/termin";

/// Relative user-local CLI symlink target within home directory.
pub const USER_CLI_SUBPATH: &str = ".local/bin/termin";

/// Installed location of the CLI symlink.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InstallLocation {
    /// Installed in `/usr/local/bin/termin` (or specified system path).
    System(PathBuf),
    /// Installed in `~/.local/bin/termin` (or specified user path).
    User(PathBuf),
}

impl fmt::Display for InstallLocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::System(p) => write!(f, "system location: {}", p.display()),
            Self::User(p) => write!(f, "user location: {}", p.display()),
        }
    }
}

/// Errors during CLI symlink installation.
#[derive(Debug)]
pub enum CliInstallError {
    /// Source executable could not be resolved or does not exist.
    SourceNotFound(PathBuf),
    /// User home directory could not be determined for fallback.
    NoUserHome,
    /// Both system and user installation attempts failed.
    AllLocationsFailed {
        system_path: PathBuf,
        system_err: std::io::Error,
        user_path: PathBuf,
        user_err: std::io::Error,
    },
    /// An I/O error occurred during installation.
    Io(std::io::Error),
    /// Elevated installation via AppleScript failed.
    ElevationFailed(String),
}

impl fmt::Display for CliInstallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SourceNotFound(path) => {
                write!(f, "source executable not found at {}", path.display())
            }
            Self::NoUserHome => write!(f, "could not determine user home directory"),
            Self::AllLocationsFailed {
                system_path,
                system_err,
                user_path,
                user_err,
            } => write!(
                f,
                "failed to install CLI at {} ({}) and fallback at {} ({})",
                system_path.display(),
                system_err,
                user_path.display(),
                user_err
            ),
            Self::Io(err) => write!(f, "I/O error during installation: {}", err),
            Self::ElevationFailed(msg) => {
                write!(f, "elevated installation with administrator privileges failed: {}", msg)
            }
        }
    }
}

impl std::error::Error for CliInstallError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(err) => Some(err),
            _ => None,
        }
    }
}

impl From<std::io::Error> for CliInstallError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}

/// Resolves the source path of the `termin` executable.
///
/// Checks `std::env::current_exe()`. If valid and existing, returns it.
/// Otherwise falls back to standard bundle path `/Applications/Termin.app/Contents/MacOS/termin`.
pub fn resolve_cli_source() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if exe.exists() {
            return exe;
        }
    }
    PathBuf::from("/Applications/Termin.app/Contents/MacOS/termin")
}

/// Returns the default system target path (`/usr/local/bin/termin`).
pub fn default_system_path() -> PathBuf {
    PathBuf::from(SYSTEM_CLI_PATH)
}

/// Returns the default user target path (`~/.local/bin/termin`).
pub fn default_user_path() -> Option<PathBuf> {
    if let Some(xdg_bin) = std::env::var_os("XDG_BIN_HOME") {
        return Some(PathBuf::from(xdg_bin).join("termin"));
    }
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(USER_CLI_SUBPATH))
}

/// Atomically creates or replaces a symlink at `target` pointing to `source`.
///
/// Ensures the parent directory exists. Replaces any existing file or symlink atomically.
pub fn create_or_replace_symlink(source: &Path, target: &Path) -> std::io::Result<()> {
    if let Some(parent) = target.parent() {
        if !parent.exists() {
            fs::create_dir_all(parent)?;
        }
    }

    // Use a unique temporary symlink in the target directory for atomic replacement
    let file_name = target
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("termin");
    let tmp_name = format!(".{}.tmp.{}", file_name, std::process::id());
    let tmp_path = target
        .parent()
        .map(|p| p.join(&tmp_name))
        .unwrap_or_else(|| PathBuf::from(tmp_name));

    // Clean up any stale temp file first
    let _ = fs::remove_file(&tmp_path);

    #[cfg(unix)]
    std::os::unix::fs::symlink(source, &tmp_path)?;

    // Atomic rename over target
    if let Err(_rename_err) = fs::rename(&tmp_path, target) {
        // If atomic rename fails (e.g. cross-filesystem or older OS quirk), clean up and try remove + symlink
        let _ = fs::remove_file(&tmp_path);
        if target.symlink_metadata().is_ok() {
            let _ = fs::remove_file(target);
        }
        #[cfg(unix)]
        std::os::unix::fs::symlink(source, target)?;
        #[cfg(not(unix))]
        return Err(rename_err);
    }

    Ok(())
}

/// Attempts installation at `system_path`, falling back to `user_path` if system is not writable.
pub fn install_to_locations(
    source: &Path,
    system_path: &Path,
    user_path: Option<&Path>,
) -> Result<InstallLocation, CliInstallError> {
    if !source.exists() {
        return Err(CliInstallError::SourceNotFound(source.to_path_buf()));
    }

    // Try installing to system location first
    match create_or_replace_symlink(source, system_path) {
        Ok(()) => Ok(InstallLocation::System(system_path.to_path_buf())),
        Err(system_err) => {
            log::debug!(
                "Failed to install CLI at system path {} ({}): attempting user fallback",
                system_path.display(),
                system_err
            );
            if let Some(user_target) = user_path {
                match create_or_replace_symlink(source, user_target) {
                    Ok(()) => Ok(InstallLocation::User(user_target.to_path_buf())),
                    Err(user_err) => Err(CliInstallError::AllLocationsFailed {
                        system_path: system_path.to_path_buf(),
                        system_err,
                        user_path: user_target.to_path_buf(),
                        user_err,
                    }),
                }
            } else {
                Err(CliInstallError::NoUserHome)
            }
        }
    }
}

/// Constructs the AppleScript command used for elevated installation with administrator privileges.
pub fn format_auth_script(source: &Path, target: &Path) -> String {
    let source_str = source.to_string_lossy().replace('\'', "'\\''");
    let target_str = target.to_string_lossy().replace('\'', "'\\''");
    format!(
        "do shell script \"mkdir -p '{}' && ln -sf '{}' '{}'\" with administrator privileges",
        target.parent().unwrap_or(Path::new("/usr/local/bin")).to_string_lossy().replace('\'', "'\\''"),
        source_str,
        target_str
    )
}

/// Performs elevated CLI installation using macOS `osascript` prompting the user for admin credentials.
pub fn install_cli_with_auth_prompt(
    source: &Path,
    target: &Path,
) -> Result<(), CliInstallError> {
    if !source.exists() {
        return Err(CliInstallError::SourceNotFound(source.to_path_buf()));
    }

    let script = format_auth_script(source, target);
    let output = Command::new("osascript")
        .arg("-e")
        .arg(&script)
        .output()
        .map_err(CliInstallError::Io)?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(CliInstallError::ElevationFailed(stderr))
    }
}

/// Primary CLI installation entry point.
///
/// Resolves the current binary, attempts `/usr/local/bin/termin`, falls back to `~/.local/bin/termin`.
pub fn install_cli() -> Result<InstallLocation, CliInstallError> {
    let source = resolve_cli_source();
    let sys_path = default_system_path();
    let usr_path = default_user_path();
    install_to_locations(&source, &sys_path, usr_path.as_deref())
}

/// Handle `Action::InstallCli`.
pub fn handle(_model: &mut AppModel, action: &Action) -> bool {
    if !matches!(action, Action::InstallCli) {
        return false;
    }

    match install_cli() {
        Ok(location) => {
            log::info!("Termin CLI successfully installed to {}", location);
            true
        }
        Err(err) => {
            log::warn!("Could not install Termin CLI automatically: {}", err);
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;

    struct TempDirGuard {
        path: PathBuf,
    }

    impl TempDirGuard {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!("termin-cli-test-{}-{}", name, std::process::id()));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).unwrap();
            Self { path }
        }
    }

    impl Drop for TempDirGuard {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn create_dummy_file(path: &Path) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        let mut file = File::create(path).unwrap();
        writeln!(file, "#!/bin/sh\necho termin").unwrap();
    }

    #[test]
    fn test_create_and_replace_symlink() {
        let temp = TempDirGuard::new("symlink-basic");
        let bin_source = temp.path.join("bin/termin");
        let link_target = temp.path.join("out/termin");

        create_dummy_file(&bin_source);

        // 1. Initial symlink creation
        create_or_replace_symlink(&bin_source, &link_target).unwrap();
        assert!(link_target.exists());
        assert_eq!(fs::read_link(&link_target).unwrap(), bin_source);

        // 2. Replacement with a new source target
        let bin_source_v2 = temp.path.join("bin/termin-v2");
        create_dummy_file(&bin_source_v2);

        create_or_replace_symlink(&bin_source_v2, &link_target).unwrap();
        assert_eq!(fs::read_link(&link_target).unwrap(), bin_source_v2);
    }

    #[test]
    fn test_fallback_when_system_unwritable() {
        let temp = TempDirGuard::new("fallback");
        let bin_source = temp.path.join("bin/termin");
        create_dummy_file(&bin_source);

        // Simulated system directory: make it non-writable
        let read_only_dir = temp.path.join("sys_bin");
        fs::create_dir_all(&read_only_dir).unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&read_only_dir, fs::Permissions::from_mode(0o555)).unwrap();
        }

        let sys_target = read_only_dir.join("termin");
        let user_target = temp.path.join("user_bin/termin");

        let res = install_to_locations(&bin_source, &sys_target, Some(&user_target));

        // Restore permissions for cleanup
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&read_only_dir, fs::Permissions::from_mode(0o755));
        }

        match res {
            Ok(InstallLocation::User(p)) => {
                assert_eq!(p, user_target);
                assert!(user_target.exists());
                assert_eq!(fs::read_link(&user_target).unwrap(), bin_source);
            }
            other => panic!("expected fallback to User location, got: {:?}", other),
        }
    }

    #[test]
    fn test_missing_source_fails() {
        let temp = TempDirGuard::new("missing-src");
        let non_existent_source = temp.path.join("ghost");
        let target = temp.path.join("termin");

        let res = install_to_locations(&non_existent_source, &target, None);
        assert!(matches!(res, Err(CliInstallError::SourceNotFound(_))));
    }

    #[test]
    fn test_format_auth_script() {
        let src = Path::new("/Applications/Termin.app/Contents/MacOS/termin");
        let dst = Path::new("/usr/local/bin/termin");
        let script = format_auth_script(src, dst);

        assert!(script.starts_with("do shell script \""));
        assert!(script.ends_with("\" with administrator privileges"));
        assert!(script.contains("ln -sf '/Applications/Termin.app/Contents/MacOS/termin' '/usr/local/bin/termin'"));
    }

    #[test]
    fn test_handle_actions() {
        let mut model = AppModel::new();
        // Action::InstallCli is handled
        let _ = handle(&mut model, &Action::InstallCli);

        // Other actions return false
        assert!(!handle(&mut model, &Action::NewTab));
    }
}
