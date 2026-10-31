//! Session snapshot persistence and state restoration (`termin-persist`).
//!
//! See `docs/specs/persistence.md`.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::fs;
use std::io;
use std::path::Path;

/// Tab state snapshot.
///
/// See `docs/specs/persistence.md §3`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TabSnapshot {
    pub cwd: String,
    pub title: Option<String>,
    pub mode: String,
}

/// Pane state snapshot.
///
/// See `docs/specs/persistence.md §3`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PaneSnapshot {
    pub tabs: Vec<TabSnapshot>,
    pub active: usize,
}

/// Split tree snapshot node.
///
/// See `docs/specs/persistence.md §3`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum TreeSnapshot {
    Pane {
        pane: PaneSnapshot,
    },
    Split {
        split: String, // "right" | "down"
        ratio: f32,
        a: Box<TreeSnapshot>,
        b: Box<TreeSnapshot>,
    },
}

/// Workspace state snapshot.
///
/// See `docs/specs/persistence.md §3`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceSnapshot {
    pub name: String,
    pub focused_pane: Vec<usize>,
    pub zoomed: Option<Vec<usize>>,
    pub tree: TreeSnapshot,
}

/// Sidebar layout snapshot.
///
/// See `docs/specs/persistence.md §3`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SidebarSnapshot {
    pub visible: bool,
    pub width: f32,
}

/// Window state snapshot.
///
/// See `docs/specs/persistence.md §3`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WindowSnapshot {
    pub frame: [f32; 4],
    pub screen: String,
    pub fullscreen: bool,
    pub sidebar: SidebarSnapshot,
    pub active_workspace: usize,
    pub workspaces: Vec<WorkspaceSnapshot>,
}

/// Root snapshot of all windows, workspaces, splits, and tabs.
///
/// See `docs/specs/persistence.md §3`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub version: u32,
    pub saved_at: u64,
    pub focused_window: usize,
    pub windows: Vec<WindowSnapshot>,
}

/// Global application auxiliary state file (`state.json`).
///
/// See `docs/specs/persistence.md §2`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AppStateFile {
    pub confirm_close_ignore: Vec<String>,
    pub last_seen_version: String,
}

/// Persistence error type.
#[derive(Debug)]
pub enum PersistError {
    Io(io::Error),
    Json(serde_json::Error),
}

impl fmt::Display for PersistError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "persistence IO error: {}", e),
            Self::Json(e) => write!(f, "persistence JSON error: {}", e),
        }
    }
}

impl std::error::Error for PersistError {}

impl From<io::Error> for PersistError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<serde_json::Error> for PersistError {
    fn from(e: serde_json::Error) -> Self {
        Self::Json(e)
    }
}

/// Load session snapshot from Application Support directory.
///
/// See `docs/specs/persistence.md §4`.
pub fn load(dir: &Path) -> Result<Option<Snapshot>, PersistError> {
    let path = dir.join("session.json");
    if !path.exists() {
        return Ok(None);
    }
    let data = fs::read(&path)?;
    let snapshot: Snapshot = serde_json::from_slice(&data)?;
    Ok(Some(snapshot))
}

/// Atomically save session snapshot to Application Support directory.
///
/// See `docs/specs/persistence.md §2, §4`.
pub fn save(dir: &Path, s: &Snapshot) -> Result<(), PersistError> {
    fs::create_dir_all(dir)?;
    let tmp = dir.join("session.json.tmp");
    let dest = dir.join("session.json");
    let data = serde_json::to_vec_pretty(s)?;
    fs::write(&tmp, data)?;
    fs::rename(tmp, dest)?;
    Ok(())
}

/// Load app auxiliary state from Application Support directory.
///
/// See `docs/specs/persistence.md §2`.
pub fn load_state(dir: &Path) -> Result<Option<AppStateFile>, PersistError> {
    let path = dir.join("state.json");
    if !path.exists() {
        return Ok(None);
    }
    let data = fs::read(&path)?;
    let state: AppStateFile = serde_json::from_slice(&data)?;
    Ok(Some(state))
}

/// Atomically save app auxiliary state.
///
/// See `docs/specs/persistence.md §2`.
pub fn save_state(dir: &Path, state: &AppStateFile) -> Result<(), PersistError> {
    fs::create_dir_all(dir)?;
    let tmp = dir.join("state.json.tmp");
    let dest = dir.join("state.json");
    let data = serde_json::to_vec_pretty(state)?;
    fs::write(&tmp, data)?;
    fs::rename(tmp, dest)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_snapshot_round_trip() {
        let snapshot = Snapshot {
            version: 1,
            saved_at: 123456789,
            focused_window: 0,
            windows: Vec::new(),
        };
        let encoded = serde_json::to_string(&snapshot).unwrap();
        let decoded: Snapshot = serde_json::from_str(&encoded).unwrap();
        assert_eq!(snapshot, decoded);
    }
}
