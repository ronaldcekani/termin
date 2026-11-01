//! Configuration reload helpers (Phase 3E).
//!
//! Runtime application of the returned diff belongs to the app wiring layer.

use std::path::{Path, PathBuf};
use termin_config::{self as config, Config, ConfigDiff, Diagnostic};

use crate::{action::Action, model::AppModel};

/// User-owned config location (`$XDG_CONFIG_HOME/termin/config.toml`, else `~/.config/...`).
pub fn config_path() -> Option<PathBuf> {
    if let Some(root) = std::env::var_os("XDG_CONFIG_HOME") {
        return Some(PathBuf::from(root).join("termin/config.toml"));
    }
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config/termin/config.toml"))
}

/// Load a candidate config without mutating caller state. Parse diagnostics are retained so
/// the app can display warnings while preserving the previous config on fatal parse errors.
pub fn load_candidate(path: &Path, previous: &Config) -> (Config, ConfigDiff, Vec<Diagnostic>) {
    let (candidate, diagnostics) = config::load(path);
    let has_error = diagnostics
        .iter()
        .any(|diagnostic| matches!(diagnostic.level, termin_config::DiagnosticLevel::Error));
    if has_error {
        (previous.clone(), ConfigDiff::empty(), diagnostics)
    } else {
        let changes = config::diff(previous, &candidate);
        (candidate, changes, diagnostics)
    }
}

/// Keep the frozen action dispatcher aware that these two actions are handled here.
pub fn handle(_model: &mut AppModel, action: &Action) -> bool {
    matches!(action, Action::ReloadConfig | Action::OpenConfig)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_reload_keeps_previous_config() {
        let dir =
            std::env::temp_dir().join(format!("termin-config-controller-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        std::fs::write(&path, "[font]\nsize = \"wrong\"\n").unwrap();
        let previous = Config::default();
        let (loaded, diff, diagnostics) = load_candidate(&path, &previous);
        assert_eq!(loaded, previous);
        assert!(diff.is_empty());
        assert!(!diagnostics.is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }
}
