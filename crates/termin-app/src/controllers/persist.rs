//! App-model persistence orchestration helpers (Phase 3E).
//!
//! The app lifecycle must call these helpers on structural changes and termination.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use termin_persist::{self as persist, PersistError, Snapshot};

use crate::model::AppModel;

/// Directory owned by Termin for session and state data.
pub fn state_dir() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        std::env::var_os("HOME")
            .map(|home| PathBuf::from(home).join("Library/Application Support/Termin"))
    }
    #[cfg(not(target_os = "macos"))]
    {
        std::env::var_os("XDG_STATE_HOME")
            .or_else(|| {
                std::env::var_os("HOME")
                    .map(|home| PathBuf::from(home).join(".local/state").into_os_string())
            })
            .map(|root| PathBuf::from(root).join("Termin"))
    }
}

/// Load the saved snapshot if present. The persistence crate quarantines malformed files.
pub fn load(dir: &Path) -> Result<Option<Snapshot>, PersistError> {
    persist::load(dir)
}

/// Save current structural state immediately (used for termination and debounced writes).
pub fn save_model(dir: &Path, model: &AppModel) -> Result<(), PersistError> {
    persist::save(dir, &model.to_snapshot())
}

/// A tiny debounce tracker; call `mark_dirty` on layout mutations and save when `due` is true.
#[derive(Clone, Debug, Default)]
pub struct SaveDebounce {
    deadline: Option<Instant>,
}

impl SaveDebounce {
    pub fn mark_dirty(&mut self) {
        self.deadline = Some(Instant::now() + Duration::from_secs(1));
    }

    pub fn due(&self, now: Instant) -> bool {
        self.deadline.is_some_and(|deadline| now >= deadline)
    }

    pub fn clear(&mut self) {
        self.deadline = None;
    }
}

/// Resolve a saved cwd to itself, its nearest existing ancestor, or the user's home directory.
pub fn restore_cwd(saved: &str) -> PathBuf {
    let expanded = if saved == "~" {
        std::env::var_os("HOME").map(PathBuf::from)
    } else if let Some(rest) = saved.strip_prefix("~/") {
        std::env::var_os("HOME").map(|home| PathBuf::from(home).join(rest))
    } else {
        Some(PathBuf::from(saved))
    };
    let mut path = expanded.unwrap_or_default();
    while !path.as_os_str().is_empty() && !path.is_dir() {
        if !path.pop() {
            break;
        }
    }
    if path.is_dir() {
        path
    } else {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debounce_waits_one_second() {
        let mut debounce = SaveDebounce::default();
        debounce.mark_dirty();
        let marked = Instant::now();
        assert!(!debounce.due(marked));
        assert!(debounce.due(marked + Duration::from_secs(1)));
    }

    #[test]
    fn restore_cwd_skips_files() {
        let dir = std::env::temp_dir().join(format!("termin-cwd-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("file");
        std::fs::write(&file, b"test").unwrap();
        assert_eq!(restore_cwd(file.to_str().unwrap()), dir);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
