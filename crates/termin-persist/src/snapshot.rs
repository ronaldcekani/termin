//! Session snapshot models and schema version 1.
//!
//! See `docs/specs/persistence.md §3`.

use std::time::{SystemTime, UNIX_EPOCH};
use serde::{Deserialize, Serialize};

/// Current session snapshot format version.
pub const SNAPSHOT_VERSION: u32 = 1;

/// Tab state snapshot.
///
/// See `docs/specs/persistence.md §3`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TabSnapshot {
    pub cwd: String,
    pub title: Option<String>,
    pub mode: String,
}

impl TabSnapshot {
    pub fn new(cwd: impl Into<String>, title: Option<String>, mode: impl Into<String>) -> Self {
        Self {
            cwd: cwd.into(),
            title,
            mode: mode.into(),
        }
    }
}

/// Pane state snapshot.
///
/// See `docs/specs/persistence.md §3`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PaneSnapshot {
    pub tabs: Vec<TabSnapshot>,
    pub active: usize,
}

impl PaneSnapshot {
    pub fn new(tabs: Vec<TabSnapshot>, active: usize) -> Self {
        Self { tabs, active }
    }
}

/// Split tree snapshot node.
///
/// Either a leaf pane or an internal split node with ratio and child subtrees.
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

impl TreeSnapshot {
    pub fn pane(pane: PaneSnapshot) -> Self {
        Self::Pane { pane }
    }

    pub fn split(
        split: impl Into<String>,
        ratio: f32,
        a: TreeSnapshot,
        b: TreeSnapshot,
    ) -> Self {
        Self::Split {
            split: split.into(),
            ratio,
            a: Box::new(a),
            b: Box::new(b),
        }
    }
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

impl WorkspaceSnapshot {
    pub fn default_workspace() -> Self {
        Self {
            name: "Default".to_string(),
            focused_pane: vec![0],
            zoomed: None,
            tree: TreeSnapshot::pane(PaneSnapshot::new(
                vec![TabSnapshot::new("~", None, "auto")],
                0,
            )),
        }
    }
}

/// Sidebar layout snapshot.
///
/// See `docs/specs/persistence.md §3`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SidebarSnapshot {
    pub visible: bool,
    pub width: f32,
}

impl Default for SidebarSnapshot {
    fn default() -> Self {
        Self {
            visible: true,
            width: 240.0,
        }
    }
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

impl WindowSnapshot {
    pub fn default_window() -> Self {
        Self {
            frame: [100.0, 100.0, 960.0, 640.0],
            screen: "main".to_string(),
            fullscreen: false,
            sidebar: SidebarSnapshot::default(),
            active_workspace: 0,
            workspaces: vec![WorkspaceSnapshot::default_workspace()],
        }
    }
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

impl Snapshot {
    /// Create a fresh default snapshot with a single window and workspace.
    pub fn fresh() -> Self {
        let saved_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        Self {
            version: SNAPSHOT_VERSION,
            saved_at,
            focused_window: 0,
            windows: vec![WindowSnapshot::default_window()],
        }
    }
}

impl Default for Snapshot {
    fn default() -> Self {
        Self::fresh()
    }
}
